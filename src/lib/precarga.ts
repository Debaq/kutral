// Fuentes buscadas de antemano. Lo que más tarda de Descubrir es preguntarle a
// los scrapers (kodios_search, varios segundos): la ficha lo pide apenas se
// abre, mientras el usuario la mira, y el selector de fuentes lo toma listo.
//
// Se usa UNA vez: lo que el selector toma se saca de acá. Así "Rebuscar
// fuentes" o volver a Descubrir después de un fallo buscan de nuevo, como
// siempre.
import { invoke } from "@tauri-apps/api/core";
import type { Kind, Src } from "$lib/fuentes";

export type PedidoFuentes = {
  imdbId: string;
  kind: Kind;
  title: string;
  originalTitle: string | null;
  season: number | null;
  episode: number | null;
  kitsuId: number | null;
};

// Más viejo que esto ya no se usa: los seeds y lo que hay cambian.
const VIGENCIA_MS = 10 * 60_000;
const MAX = 6;

const precargadas = new Map<string, { t: number; p: Promise<Src[]> }>();

function clave(p: PedidoFuentes): string {
  return JSON.stringify([p.imdbId, p.kind, p.season, p.episode, p.kitsuId]);
}

function pedir(p: PedidoFuentes): Promise<Src[]> {
  return invoke<Src[]>("kodios_search", {
    imdbId: p.imdbId,
    kind: p.kind,
    title: p.title,
    originalTitle: p.originalTitle ?? undefined,
    season: p.season ?? undefined,
    episode: p.episode ?? undefined,
    kitsuId: p.kitsuId ?? undefined,
  });
}

/** Empieza a buscar en segundo plano (si no hay ya una búsqueda vigente). */
export function precargarFuentes(p: PedidoFuentes): void {
  const k = clave(p);
  const e = precargadas.get(k);
  if (e && Date.now() - e.t < VIGENCIA_MS) return;
  const pr = pedir(p);
  // Si falla, que el selector busque de cero: puede ser un hipo de red.
  pr.catch(() => precargadas.delete(k));
  precargadas.set(k, { t: Date.now(), p: pr });
  // Mapa en orden de inserción: se va lo más viejo.
  while (precargadas.size > MAX) precargadas.delete(precargadas.keys().next().value!);
}

/**
 * Las fuentes para el selector: las precargadas si las hay (y se sacan), o
 * una búsqueda nueva. Copias: el selector las marca y las ordena.
 */
export async function buscarFuentes(p: PedidoFuentes): Promise<Src[]> {
  const k = clave(p);
  const e = precargadas.get(k);
  precargadas.delete(k);
  if (e && Date.now() - e.t < VIGENCIA_MS) {
    try {
      return (await e.p).map((s) => ({ ...s }));
    } catch {
      /* la precarga falló: se busca de nuevo */
    }
  }
  return pedir(p);
}
