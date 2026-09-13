// Puente entre un título del catálogo y el archivo que quedó en disco.
//
// La cola de torrents (torrents.svelte.ts) solo conoce nombres de release y
// ids de sesión: con eso, al volver a la ficha de una película que ya bajaste,
// nadie sabía que el archivo estaba ahí y Descubrir salía a la red de nuevo.
// Acá se guarda la otra mitad: qué título es cada descarga.
//
// Fuente de verdad: SQLite (tabla `descargas`, migración v8). Misma clave que
// el historial — imdb, o `anilist:<id>` en anime — y el mismo -1/-1 para
// películas, así una fila se busca con lo que la ficha ya tiene a mano.
//
// Una fila solo vale si el archivo SIGUE existiendo: el usuario puede borrarlo
// a mano, cambiar la carpeta de descargas o desmontar el disco. Por eso todo
// lo que se ofrece pasa antes por `local_file_size`; si el archivo no está, la
// fila se borra sola y el flujo normal (buscar fuentes) sigue como siempre.

import { invoke } from "@tauri-apps/api/core";
import { getDb } from "$lib/vera/db";

export type FilaDescarga = {
  clave: string;
  /** -1 en películas. */
  season: number;
  /** -1 en películas. */
  episode: number;
  info_hash: string;
  /** Ruta absoluta del archivo de video. */
  ruta: string;
  /** Nombre del release (el del torrent, no el de la ficha). */
  release_title: string;
  quality: string;
  size_bytes: number;
  /** 0 mientras baja. Un archivo a medias no se ofrece como copia local. */
  completa: number;
  added_at: number;
};

export const descargas = $state({
  filas: [] as FilaDescarga[],
  cargado: false,
});

function claveEp(clave: string, season: number, episode: number): string {
  return `${clave}|${season}|${episode}`;
}


/** Carga la tabla a memoria. Idempotente: repetirla refresca. */
export async function cargarDescargas(): Promise<void> {
  const db = await getDb();
  if (!db) {
    descargas.cargado = true;
    return;
  }
  try {
    descargas.filas = await db.select<FilaDescarga[]>(
      "SELECT * FROM descargas ORDER BY added_at DESC",
    );
  } catch (e) {
    console.warn("[descargas] no se pudo cargar:", e);
  }
  descargas.cargado = true;
}

/**
 * Fila completa de este título/capítulo, o null. NO verifica el disco: eso
 * cuesta una llamada al backend y acá se pregunta desde el render del grid.
 * Antes de reproducir hay que pasar por `descargaUsable`.
 */
export function descargaDe(
  clave: string,
  season = -1,
  episode = -1,
): FilaDescarga | null {
  if (!clave) return null;
  const k = claveEp(clave, season, episode);
  return (
    descargas.filas.find(
      (f) => f.completa === 1 && claveEp(f.clave, f.season, f.episode) === k,
    ) ?? null
  );
}

/**
 * Estado de la descarga de un título/capítulo, sin tocar el disco:
 * "completa" (se puede ver sin red), "bajando" (encolada o a medias) o null.
 * La ficha lo usa para no ofrecer dos veces la misma bajada.
 */
export function estadoDescarga(
  clave: string,
  season = -1,
  episode = -1,
): "completa" | "bajando" | null {
  if (!clave) return null;
  const f = descargas.filas.find(
    (x) => x.clave === clave && x.season === season && x.episode === episode,
  );
  if (!f) return null;
  return f.completa === 1 ? "completa" : "bajando";
}

/**
 * ¿Este título tiene algo bajado? (chip de la ficha, sin tocar el disco). Se
 * recorre la lista en vez de mantener un índice: son unas pocas decenas de
 * filas y así el $state de arriba es la única estructura que hay que cuidar.
 */
export function tieneDescarga(clave: string): boolean {
  if (!clave) return false;
  return descargas.filas.some((f) => f.clave === clave && f.completa === 1);
}

/**
 * Como `descargaDe`, pero confirmando que el archivo sigue en disco. Si ya no
 * está, la fila se olvida (y devuelve null): ofrecer una copia fantasma sería
 * peor que no ofrecer nada.
 */
export async function descargaUsable(
  clave: string,
  season = -1,
  episode = -1,
): Promise<FilaDescarga | null> {
  const fila = descargaDe(clave, season, episode);
  if (!fila) return null;
  try {
    const size = await invoke<number | null>("local_file_size", { path: fila.ruta });
    if (size === null) {
      await olvidarDescarga(fila.clave, fila.season, fila.episode);
      return null;
    }
  } catch (e) {
    // Sin backend (dev en navegador) no podemos confirmar: mejor no ofrecerla.
    console.warn("[descargas] no se pudo verificar el archivo:", e);
    return null;
  }
  return fila;
}

/**
 * Registra (o pisa) la descarga de un título. Se llama al ARRANCAR la bajada,
 * con `completa` en 0: así, si la app se cierra a mitad, la fila ya existe y
 * el poll de la cola la marca completa cuando termine.
 */
export async function registrarDescarga(f: {
  clave: string;
  season?: number | null;
  episode?: number | null;
  infoHash: string;
  ruta: string;
  releaseTitle: string;
  quality?: string;
  sizeBytes?: number;
  completa?: boolean;
}): Promise<void> {
  if (!f.clave || !f.ruta) return;
  const fila: FilaDescarga = {
    clave: f.clave,
    season: f.season ?? -1,
    episode: f.episode ?? -1,
    info_hash: f.infoHash,
    ruta: f.ruta,
    release_title: f.releaseTitle,
    quality: f.quality || "unknown",
    size_bytes: f.sizeBytes || 0,
    completa: f.completa ? 1 : 0,
    added_at: Math.floor(Date.now() / 1000),
  };
  const db = await getDb();
  if (db) {
    try {
      await db.execute(
        `INSERT INTO descargas
           (clave, season, episode, info_hash, ruta, release_title, quality,
            size_bytes, completa, added_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT(clave, season, episode) DO UPDATE SET
           info_hash = excluded.info_hash,
           ruta = excluded.ruta,
           release_title = excluded.release_title,
           quality = excluded.quality,
           size_bytes = excluded.size_bytes,
           completa = excluded.completa,
           added_at = excluded.added_at`,
        [
          fila.clave, fila.season, fila.episode, fila.info_hash, fila.ruta,
          fila.release_title, fila.quality, fila.size_bytes, fila.completa,
          fila.added_at,
        ],
      );
    } catch (e) {
      console.warn("[descargas] no se pudo registrar:", e);
      return;
    }
  }
  const k = claveEp(fila.clave, fila.season, fila.episode);
  descargas.filas = [
    fila,
    ...descargas.filas.filter((x) => claveEp(x.clave, x.season, x.episode) !== k),
  ];
}

/**
 * La descarga terminó: recién ahora el archivo sirve para verlo sin red. Se
 * llama desde el poll de la cola, que es quien ve la transición.
 */
export async function marcarCompleta(infoHash: string): Promise<void> {
  if (!infoHash) return;
  const hash = infoHash.toLowerCase();
  const pendiente = descargas.filas.some(
    (f) => f.info_hash.toLowerCase() === hash && f.completa === 0,
  );
  if (!pendiente) return;
  const db = await getDb();
  if (db) {
    try {
      await db.execute(
        "UPDATE descargas SET completa = 1 WHERE lower(info_hash) = $1",
        [hash],
      );
    } catch (e) {
      console.warn("[descargas] no se pudo marcar completa:", e);
      return;
    }
  }
  descargas.filas = descargas.filas.map((f) =>
    f.info_hash.toLowerCase() === hash ? { ...f, completa: 1 } : f,
  );
}

/** Olvida la fila (archivo borrado, o el usuario quitó la descarga). */
export async function olvidarDescarga(
  clave: string,
  season = -1,
  episode = -1,
): Promise<void> {
  const db = await getDb();
  if (db) {
    try {
      await db.execute(
        "DELETE FROM descargas WHERE clave = $1 AND season = $2 AND episode = $3",
        [clave, season, episode],
      );
    } catch (e) {
      console.warn("[descargas] no se pudo olvidar:", e);
      return;
    }
  }
  const k = claveEp(clave, season, episode);
  descargas.filas = descargas.filas.filter(
    (f) => claveEp(f.clave, f.season, f.episode) !== k,
  );
}

/** Olvida por infohash: lo que sabe la cola cuando se borra una descarga. */
export async function olvidarPorHash(infoHash: string): Promise<void> {
  if (!infoHash) return;
  const hash = infoHash.toLowerCase();
  const db = await getDb();
  if (db) {
    try {
      await db.execute("DELETE FROM descargas WHERE lower(info_hash) = $1", [hash]);
    } catch (e) {
      console.warn("[descargas] no se pudo olvidar:", e);
      return;
    }
  }
  descargas.filas = descargas.filas.filter(
    (f) => f.info_hash.toLowerCase() !== hash,
  );
}
