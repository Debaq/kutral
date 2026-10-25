// Lo que se eligió en Vera (Descubrir), para preguntar después "¿qué tal?".
//
// Vera no se mete en el reproductor: la próxima vez que se abre, si alguna
// elegida ya se vio (terminada o más de la mitad, según watch_history del
// perfil) y no tiene opinión, ofrece un 👍 / 👎. Responder es opcional y lo
// que se responde entra al perfil como cualquier reacción.

import { getDb } from "./db";
import { getReaccion, setReaccion } from "./ratings";
import { pesoReaccionPersistida } from "./historial";
import { registrarCambio } from "./aprendizaje";
import { perfilActual } from "$lib/perfilActivo";
import type { Pelicula } from "./tipos";

const K = "vera_elegidas";
const MAX = 20;
// Más vieja que esto ya no se pregunta: nadie se acuerda.
const VENCE_MS = 30 * 86_400_000;
// Cuánto vale un 👍 / 👎 en la escala de juicio (-5..+5).
export const JUICIO_GUSTO = 4;

type Elegida = { pelicula: Pelicula; perfil: string; t: number };

function leer(): Elegida[] {
  try {
    const v = JSON.parse(localStorage.getItem(K) || "[]") as Elegida[];
    return Array.isArray(v) ? v.filter((e) => Date.now() - e.t < VENCE_MS) : [];
  } catch {
    return [];
  }
}

function escribir(lista: Elegida[]) {
  try {
    localStorage.setItem(K, JSON.stringify(lista.slice(-MAX)));
  } catch {
    /* sin storage: no se pregunta después, nada grave */
  }
}

export function anotarElegida(p: Pelicula): void {
  const lista = leer().filter((e) => e.pelicula.id !== p.id);
  lista.push({ pelicula: p, perfil: perfilActual(), t: Date.now() });
  escribir(lista);
}

function olvidar(id: string) {
  escribir(leer().filter((e) => e.pelicula.id !== id));
}

/** La primera elegida de este perfil que ya se vio y espera opinión. */
export async function porOpinar(): Promise<Pelicula | null> {
  const perfil = perfilActual();
  const mias = leer().filter((e) => e.perfil === perfil && getReaccion(e.pelicula.id)?.juicio == null);
  if (mias.length === 0) return null;
  const db = await getDb();
  if (!db) return null;
  for (const e of mias.reverse()) {
    try {
      const filas = await db.select<{ completed: number; watched_seconds: number; runtime_seconds: number | null }[]>(
        `SELECT completed, watched_seconds, runtime_seconds FROM watch_history
          WHERE perfil = $1 AND tmdb_id = $2 AND media_type = 'movie' AND season = -1`,
        [perfil, Number(e.pelicula.id)],
      );
      const f = filas[0];
      if (!f) continue;
      const vista =
        f.completed === 1 || (!!f.runtime_seconds && f.watched_seconds / f.runtime_seconds >= 0.5);
      if (vista) return e.pelicula;
    } catch (err) {
      console.warn("[vera/elegidas] no se pudo leer el historial:", err);
      return null;
    }
  }
  return null;
}

/**
 * Lo que opinó de una peli (al terminar, o con "Ya la vi" en una carta).
 * `null` = ni fu ni fa: cuenta como vista, sin empujar el perfil.
 */
export async function opinar(p: Pelicula, gusto: boolean | null): Promise<void> {
  const previa = getReaccion(p.id);
  const pesoAnterior = previa ? pesoReaccionPersistida(previa) : 0;
  const juicio = gusto === null ? 0 : gusto ? JUICIO_GUSTO : -JUICIO_GUSTO;
  setReaccion(p.id, { tmdb: p.rating, juicio });
  olvidar(p.id);
  await registrarCambio(p, pesoAnterior, juicio);
}

/** "Ahora no": no se vuelve a preguntar por esta. */
export function noPreguntar(p: Pelicula): void {
  olvidar(p.id);
}
