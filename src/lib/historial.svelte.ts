// Historial de reproducción y favoritos.
//
// Fuente de verdad: SQLite (`watch_history`, `favorites`), migración v7. Este
// módulo mantiene además una copia en memoria reactiva para que el grid pueda
// pintar el tick verde / la barra de progreso sin una query por tarjeta.
//
// Quién escribe:
//   - HistorialTracker.svelte  → progreso real de mpv (la vía normal hoy).
//   - routes/+page.svelte      → el iframe del player web (apagado, ver
//                                features.ts) y el marcado manual.
//
// Clave de un título (`clave`): el imdb_id cuando existe. El anime SIEMPRE usa
// la clave sintética `anilist:<id>`, incluso cuando ani.zip le encontró imdb:
// la tarjeta del catálogo de anime solo conoce el id de AniList, y si el
// historial se escribiera con imdb esa tarjeta nunca encontraría su propia
// fila. Es TEXT en la DB, no hay validación de tt*.

import { getDb } from "$lib/vera/db";

export type FilaHistorial = {
  imdb_id: string;
  /** -1 en películas. */
  season: number;
  /** -1 en películas. */
  episode: number;
  tmdb_id: number;
  media_type: string;
  title: string;
  poster_path: string | null;
  still_path: string | null;
  episode_title: string | null;
  watched_seconds: number;
  runtime_seconds: number | null;
  progress_real: number | null;
  completed: number;
  last_watched: number;
};

export type FilaFavorito = {
  imdb_id: string;
  tmdb_id: number;
  media_type: string;
  title: string;
  poster_path: string | null;
  added_at: number;
};

/** Lo que hace falta para escribir una fila. `season`/`episode` = -1 en películas. */
export type MediaMeta = {
  clave: string;
  tmdbId: number;
  mediaType: string;
  title: string;
  posterPath?: string | null;
  season?: number | null;
  episode?: number | null;
  stillPath?: string | null;
  episodeTitle?: string | null;
  runtimeSeconds?: number | null;
};

/** Resumen por título, para la tarjeta del catálogo. */
export type EstadoMedio = {
  /** Película terminada, o serie con algún capítulo terminado y ninguno a medias. */
  visto: boolean;
  /** Hay algo empezado sin terminar. */
  parcial: boolean;
  /** 0..100 del ítem a medias (0 si no hay ninguno). */
  pct: number;
  /** Último capítulo tocado (series). null en películas. */
  ultimoEp: { season: number; episode: number } | null;
  /** Capítulos terminados (series). */
  vistos: number;
  last_watched: number;
};

// Umbral de "terminado". Igual al que usaba el player web: los últimos minutos
// son créditos y nadie los ve enteros.
const COMPLETO = 0.9;
// Debajo de esto no es "empezado", es un click curioso. Evita llenar el
// historial de entradas de 3 segundos.
const MINIMO_SEGUNDOS = 30;

export const historial = $state({
  cargado: false,
  /** Todas las filas, orden last_watched DESC. */
  filas: [] as FilaHistorial[],
  /** clave → resumen. Lo que consulta el grid. */
  porClave: {} as Record<string, EstadoMedio>,
  /** `clave|season|episode` → fila. Lo que consulta la lista de capítulos. */
  porEpisodio: {} as Record<string, FilaHistorial>,
  favoritos: {} as Record<string, FilaFavorito>,
});

/** Clave del título: imdb si hay, si no la sintética de AniList. */
export function claveMedio(imdbId?: string | null, anilistId?: number | null): string {
  if (imdbId) return imdbId;
  if (anilistId) return `anilist:${anilistId}`;
  return "";
}

function claveEp(clave: string, season: number, episode: number): string {
  return `${clave}|${season}|${episode}`;
}

function pctDe(f: FilaHistorial): number {
  const real =
    f.progress_real ??
    (f.runtime_seconds && f.runtime_seconds > 0 ? f.watched_seconds / f.runtime_seconds : null);
  if (real == null) return 0;
  return Math.max(0, Math.min(100, Math.round(real * 100)));
}

// Reconstruye los índices desde `filas`. Barato (el historial de una casa son
// cientos de filas, no millones) y evita mantener tres estructuras a mano.
function reindexar(): void {
  const porClave: Record<string, EstadoMedio> = {};
  const porEpisodio: Record<string, FilaHistorial> = {};
  for (const f of historial.filas) {
    porEpisodio[claveEp(f.imdb_id, f.season, f.episode)] = f;
    const prev = porClave[f.imdb_id];
    const terminado = f.completed === 1;
    const pct = pctDe(f);
    const enCurso = !terminado && f.watched_seconds >= MINIMO_SEGUNDOS;
    const est: EstadoMedio = prev ?? {
      visto: false,
      parcial: false,
      pct: 0,
      ultimoEp: null,
      vistos: 0,
      last_watched: 0,
    };
    if (terminado) est.vistos++;
    if (enCurso) {
      est.parcial = true;
      // El % que muestra la tarjeta es el del ítem más reciente a medias.
      if (f.last_watched >= est.last_watched) est.pct = pct;
    }
    if (f.last_watched >= est.last_watched) {
      est.last_watched = f.last_watched;
      if (f.season >= 0) est.ultimoEp = { season: f.season, episode: f.episode };
    }
    // Película: el tick es directo. Serie: tick solo si terminó algo y no
    // quedó nada a medias (no sabemos cuántos capítulos tiene en total, así
    // que "al día" es lo más honesto que podemos afirmar).
    est.visto = est.vistos > 0 && !est.parcial;
    porClave[f.imdb_id] = est;
  }
  historial.porClave = porClave;
  historial.porEpisodio = porEpisodio;
}

/** Carga historial + favoritos. Idempotente: repetirla refresca. */
export async function cargarHistorial(): Promise<void> {
  const db = await getDb();
  if (!db) {
    historial.cargado = true;
    return;
  }
  try {
    historial.filas = await db.select<FilaHistorial[]>(
      "SELECT * FROM watch_history ORDER BY last_watched DESC",
    );
    const favs = await db.select<FilaFavorito[]>(
      "SELECT * FROM favorites ORDER BY added_at DESC",
    );
    const mapa: Record<string, FilaFavorito> = {};
    for (const f of favs) mapa[f.imdb_id] = f;
    historial.favoritos = mapa;
    reindexar();
  } catch (e) {
    console.warn("[historial] no se pudo cargar:", e);
  }
  historial.cargado = true;
}

export function estadoDe(clave: string): EstadoMedio | null {
  return clave ? (historial.porClave[clave] ?? null) : null;
}

export function estadoEpisodio(
  clave: string,
  season: number,
  episode: number,
): FilaHistorial | null {
  return clave ? (historial.porEpisodio[claveEp(clave, season, episode)] ?? null) : null;
}

/** Fila de una película (o del título entero si no es serie). */
export function filaPelicula(clave: string): FilaHistorial | null {
  return estadoEpisodio(clave, -1, -1);
}

/** Segundos desde donde retomar, o 0 si no corresponde (sin progreso o ya visto). */
export function segundosParaRetomar(clave: string, season = -1, episode = -1): number {
  const f = estadoEpisodio(clave, season, episode);
  if (!f || f.completed === 1) return 0;
  if (f.watched_seconds < MINIMO_SEGUNDOS) return 0;
  return f.watched_seconds;
}

/**
 * Escribe (o actualiza) el progreso de un título/capítulo.
 * `real` es la fracción 0..1 cuando el player la sabe; si no, se deduce del
 * runtime conocido. Sin ninguno de los dos la fila queda sin % (la UI muestra
 * minutos en vez de porcentaje).
 */
export async function guardarProgreso(
  meta: MediaMeta,
  p: { watched: number; runtime?: number | null; real?: number | null },
): Promise<void> {
  if (!meta.clave || p.watched < MINIMO_SEGUNDOS) return;
  const db = await getDb();
  if (!db) return;
  const season = meta.season ?? -1;
  const episode = meta.episode ?? -1;
  const runtime = p.runtime ?? meta.runtimeSeconds ?? null;
  const real = p.real ?? (runtime && runtime > 0 ? p.watched / runtime : null);
  const completed = real != null && real >= COMPLETO ? 1 : 0;
  const ahora = Date.now();
  // MAX(completed) en el UPSERT: una vez terminado no vuelve a 0 porque el
  // user rebobinó al final para ver los créditos.

  try {
    await db.execute(
      `INSERT INTO watch_history
         (imdb_id, season, episode, tmdb_id, media_type, title, poster_path,
          still_path, episode_title, watched_seconds, runtime_seconds,
          progress_real, completed, last_watched)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
       ON CONFLICT(imdb_id, season, episode) DO UPDATE SET
         watched_seconds = excluded.watched_seconds,
         runtime_seconds = COALESCE(excluded.runtime_seconds, watch_history.runtime_seconds),
         progress_real   = COALESCE(excluded.progress_real,   watch_history.progress_real),
         still_path      = COALESCE(excluded.still_path,      watch_history.still_path),
         episode_title   = COALESCE(excluded.episode_title,   watch_history.episode_title),
         poster_path     = COALESCE(excluded.poster_path,     watch_history.poster_path),
         completed       = MAX(excluded.completed, watch_history.completed),
         last_watched    = excluded.last_watched`,
      [
        meta.clave,
        season,
        episode,
        meta.tmdbId,
        meta.mediaType,
        meta.title,
        meta.posterPath ?? null,
        meta.stillPath ?? null,
        meta.episodeTitle ?? null,
        Math.floor(p.watched),
        runtime,
        real,
        completed,
        ahora,
      ],
    );
  } catch (e) {
    console.warn("[historial] error guardando:", e);
    return;
  }
  aplicarLocal({
    imdb_id: meta.clave,
    season,
    episode,
    tmdb_id: meta.tmdbId,
    media_type: meta.mediaType,
    title: meta.title,
    poster_path: meta.posterPath ?? null,
    still_path: meta.stillPath ?? null,
    episode_title: meta.episodeTitle ?? null,
    watched_seconds: Math.floor(p.watched),
    runtime_seconds: runtime,
    progress_real: real,
    completed,
    last_watched: ahora,
  });
}

/** Marca visto/no visto a mano (tick del panel de info y de la lista). */
export async function marcarVisto(meta: MediaMeta, visto: boolean): Promise<void> {
  if (!meta.clave) return;
  const season = meta.season ?? -1;
  const episode = meta.episode ?? -1;
  if (!visto) {
    await borrarProgreso(meta.clave, season, episode);
    return;
  }
  const runtime = meta.runtimeSeconds ?? null;
  const db = await getDb();
  if (!db) return;
  const ahora = Date.now();
  const watched = runtime && runtime > 0 ? runtime : 0;
  try {
    await db.execute(
      `INSERT INTO watch_history
         (imdb_id, season, episode, tmdb_id, media_type, title, poster_path,
          still_path, episode_title, watched_seconds, runtime_seconds,
          progress_real, completed, last_watched)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1.0,1,$12)
       ON CONFLICT(imdb_id, season, episode) DO UPDATE SET
         progress_real = 1.0,
         completed     = 1,
         last_watched  = excluded.last_watched`,
      [
        meta.clave,
        season,
        episode,
        meta.tmdbId,
        meta.mediaType,
        meta.title,
        meta.posterPath ?? null,
        meta.stillPath ?? null,
        meta.episodeTitle ?? null,
        watched,
        runtime,
        ahora,
      ],
    );
  } catch (e) {
    console.warn("[historial] error marcando visto:", e);
    return;
  }
  aplicarLocal({
    imdb_id: meta.clave,
    season,
    episode,
    tmdb_id: meta.tmdbId,
    media_type: meta.mediaType,
    title: meta.title,
    poster_path: meta.posterPath ?? null,
    still_path: meta.stillPath ?? null,
    episode_title: meta.episodeTitle ?? null,
    watched_seconds: watched,
    runtime_seconds: runtime,
    progress_real: 1,
    completed: 1,
    last_watched: ahora,
  });
}

/** Borra una fila (película, capítulo) o el título entero si no se pasa season. */
export async function borrarProgreso(
  clave: string,
  season?: number,
  episode?: number,
): Promise<void> {
  if (!clave) return;
  const db = await getDb();
  if (!db) return;
  try {
    if (season == null || episode == null) {
      await db.execute("DELETE FROM watch_history WHERE imdb_id = $1", [clave]);
      historial.filas = historial.filas.filter((f) => f.imdb_id !== clave);
    } else {
      await db.execute(
        "DELETE FROM watch_history WHERE imdb_id = $1 AND season = $2 AND episode = $3",
        [clave, season, episode],
      );
      historial.filas = historial.filas.filter(
        (f) => !(f.imdb_id === clave && f.season === season && f.episode === episode),
      );
    }
    reindexar();
  } catch (e) {
    console.warn("[historial] error borrando:", e);
  }
}

export function esFavorito(clave: string): boolean {
  return !!clave && clave in historial.favoritos;
}

/** Alterna el favorito. Devuelve el estado resultante. */
export async function alternarFavorito(meta: MediaMeta): Promise<boolean> {
  if (!meta.clave) return false;
  const db = await getDb();
  if (!db) return esFavorito(meta.clave);
  const activo = esFavorito(meta.clave);
  try {
    if (activo) {
      await db.execute("DELETE FROM favorites WHERE imdb_id = $1", [meta.clave]);
      const copia = { ...historial.favoritos };
      delete copia[meta.clave];
      historial.favoritos = copia;
      return false;
    }
    const fila: FilaFavorito = {
      imdb_id: meta.clave,
      tmdb_id: meta.tmdbId,
      media_type: meta.mediaType,
      title: meta.title,
      poster_path: meta.posterPath ?? null,
      added_at: Date.now(),
    };
    await db.execute(
      `INSERT INTO favorites (imdb_id, tmdb_id, media_type, title, poster_path, added_at)
       VALUES ($1,$2,$3,$4,$5,$6)
       ON CONFLICT(imdb_id) DO UPDATE SET added_at = excluded.added_at`,
      [fila.imdb_id, fila.tmdb_id, fila.media_type, fila.title, fila.poster_path, fila.added_at],
    );
    historial.favoritos = { ...historial.favoritos, [fila.imdb_id]: fila };
    return true;
  } catch (e) {
    console.warn("[historial] error en favorito:", e);
    return activo;
  }
}

export function listaFavoritos(): FilaFavorito[] {
  return Object.values(historial.favoritos).sort((a, b) => b.added_at - a.added_at);
}

/**
 * La fila a medias más reciente de un título: lo que lo pone en "seguir
 * viendo". En una serie es el capítulo, no la serie entera — es lo que hay que
 * borrar para sacarla de la lista.
 */
export function filaEnCurso(clave: string): FilaHistorial | null {
  if (!clave) return null;
  return (
    historial.filas.find(
      (f) => f.imdb_id === clave && f.completed === 0 && f.watched_seconds >= MINIMO_SEGUNDOS,
    ) ?? null
  );
}

/** Lo empezado y sin terminar, más reciente primero. */
export function seguirViendo(limite = 20): FilaHistorial[] {
  return historial.filas
    .filter((f) => f.completed === 0 && f.watched_seconds >= MINIMO_SEGUNDOS)
    .slice(0, limite);
}

// Inserta/actualiza en memoria sin releer la DB entera.
function aplicarLocal(fila: FilaHistorial): void {
  const i = historial.filas.findIndex(
    (f) => f.imdb_id === fila.imdb_id && f.season === fila.season && f.episode === fila.episode,
  );
  if (i >= 0) {
    // `completed` no retrocede (misma regla que el MAX() del UPSERT).
    const prev = historial.filas[i];
    historial.filas[i] = { ...fila, completed: Math.max(fila.completed, prev.completed) };
    // Reordenar: la fila tocada pasa al frente.
    const [tocada] = historial.filas.splice(i, 1);
    historial.filas.unshift(tocada);
  } else {
    historial.filas.unshift(fila);
  }
  reindexar();
}

// ─────────────────────────────────────────────────────────────────────────────
// Contexto de reproducción
// ─────────────────────────────────────────────────────────────────────────────
//
// mpv no sabe qué está reproduciendo en términos de catálogo: recibe una URL.
// La home deja acá los metadatos ANTES de lanzar la reproducción, y
// HistorialTracker los usa para escribir las filas mientras corre.
//
// Se limpia al reproducir algo que no es catálogo (trailer, IPTV): sin esto un
// trailer de 2 minutos marcaría la película como vista.

let contexto: MediaMeta | null = null;

export function setContexto(meta: MediaMeta | null): void {
  contexto = meta && meta.clave ? meta : null;
}

export function contextoActual(): MediaMeta | null {
  return contexto;
}

export function limpiarContexto(): void {
  contexto = null;
}
