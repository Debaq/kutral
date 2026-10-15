// Pack de temporada que está sonando. Con él, el siguiente capítulo (o el que
// se elija en el menú "Capítulos" del reproductor) sale del mismo torrent, sin
// volver a buscar fuentes.
//
// Lo anota SourcePicker al lanzar una fuente: si el backend dice que el
// torrent trae varios capítulos (pack.rs), queda acá; si no, se borra. Así
// siempre describe lo que está en pantalla.

import type { Src } from "$lib/fuentes";

/** Lo que el backend usa para elegir el archivo dentro del torrent. */
export type Pista = {
  file_idx: number | null;
  filename: string | null;
  season: number | null;
  episode: number | null;
};

/** Un capítulo dentro del pack (pack::ArchivoPack). */
export type ArchivoPack = {
  idx: number;
  nombre: string;
  bytes: number;
  season: number | null;
  episode: number;
};

type Pack = {
  /** Título del historial (imdb o `anilist:<id>`). */
  clave: string;
  /** Temporada que se pidió al abrirlo: la de los archivos que no la dicen. */
  season: number | null;
  src: Src;
  /** Por dónde se resolvió: el siguiente va por el mismo camino. */
  via: "rd" | "local";
  archivos: ArchivoPack[];
};

let actual: Pack | null = null;

export function pistaDe(s: Src, season: number | null, episode: number | null): Pista {
  return {
    file_idx: s.file_idx ?? null,
    filename: s.filename ?? null,
    season,
    episode,
  };
}

/** Anota lo que se acaba de lanzar. Sin capítulos (no es pack) lo olvida. */
export function recordarPack(
  clave: string,
  season: number | null,
  src: Src,
  via: Pack["via"],
  archivos: ArchivoPack[],
): void {
  if (!clave || archivos.length < 2) {
    actual = null;
    return;
  }
  // Ya se estaba viendo este mismo pack: la temporada de referencia es la
  // del primero que se abrió, no la del capítulo de ahora.
  const mismo = actual && actual.clave === clave && actual.src.info_hash === src.info_hash;
  actual = { clave, season: mismo ? actual!.season : season, src, via, archivos };
}

export function olvidarPack(): void {
  actual = null;
}

function temporada(a: ArchivoPack, p: Pack): number {
  return a.season ?? p.season ?? 1;
}

/** ¿El pack que suena es de este título? */
export function hayPack(clave: string): boolean {
  return !!actual && actual.clave === clave;
}

/**
 * El capítulo pedido, si está en el pack que suena: la fuente (con el archivo
 * ya apuntado) y por dónde resolverla.
 */
export function capituloDelPack(
  clave: string,
  season: number | null,
  episode: number | null,
): { src: Src; via: Pack["via"] } | null {
  const p = actual;
  if (!p || p.clave !== clave || episode == null) return null;
  const a = p.archivos.find((x) => temporada(x, p) === (season ?? 1) && x.episode === episode);
  if (!a) return null;
  return {
    // El título del release pasa a ser el del archivo: es lo que miran los
    // buscadores de subtítulos para emparejar la versión.
    src: { ...p.src, file_idx: a.idx, filename: a.nombre, title: a.nombre, _count: undefined },
    via: p.via,
  };
}

/** Los capítulos del pack que suena, en orden. */
export function capitulosDelPack(clave: string): { season: number; episode: number; nombre: string }[] {
  const p = actual;
  if (!p || p.clave !== clave) return [];
  return p.archivos
    .map((a) => ({ season: temporada(a, p), episode: a.episode, nombre: a.nombre }))
    .sort((x, y) => x.season - y.season || x.episode - y.episode);
}

/** Respuesta de `rd_resolver`. */
export type Resuelto = { url: string; pack: ArchivoPack[] };
