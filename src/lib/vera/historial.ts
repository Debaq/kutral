// Señales del usuario para alimentar el motor de recomendación.
// Fusiona dos fuentes:
//   1. localStorage `vera_ratings_v1`   (lo que Vera ya recoge en su flujo)
//   2. SQLite `watch_history`          (lo que la home escribe al reproducir)
//
// Solo lectura. Solo media_type='movie' (las series quedan fuera de Vera).
// Handle SQLite propio cacheado — no compartido con la home, para no acoplar.
// Si la DB no se puede abrir (no-Tauri, permisos), degrada a solo ratings.

import Database from "@tauri-apps/plugin-sql";
import { getRatingsMap } from "./ratings";

// Singleton del handle de SQLite. Si el primer intento falla, no reintenta:
// la mayoría de las fallas (no-Tauri, permisos) son permanentes en la sesión.
let dbCache: Database | null = null;
let dbAttempted = false;

async function getDb(): Promise<Database | null> {
  if (dbCache) return dbCache;
  if (dbAttempted) return null;
  dbAttempted = true;
  try {
    dbCache = await Database.load("sqlite:kutral.db");
    return dbCache;
  } catch (e) {
    console.warn("[vera/historial] DB no disponible:", e);
    return null;
  }
}

interface WatchRow {
  tmdb_id: number;
}

// Todas las pelis que el usuario ya vio. Se excluyen del pool en construirPool.
// "Visto" = vera_ratings_v1[id].visto === true  OR  watch_history.completed = 1
export async function getVistas(): Promise<string[]> {
  const vistas = new Set<string>();

  // Fuente 1: ratings locales de Vera.
  const ratings = getRatingsMap();
  for (const [id, rec] of Object.entries(ratings)) {
    if (rec.visto) vistas.add(id);
  }

  // Fuente 2: watch_history. Solo películas completadas (≥90%).
  const db = await getDb();
  if (db) {
    try {
      const rows = await db.select<WatchRow[]>(
        `SELECT tmdb_id
           FROM watch_history
          WHERE completed = 1
            AND media_type = 'movie'`,
      );
      for (const r of rows) vistas.add(String(r.tmdb_id));
    } catch (e) {
      console.warn("[vera/historial] error leyendo vistas:", e);
    }
  }

  return [...vistas];
}

// Top n pelis "que le gustaron" para usar como semilla de /recommendations.
// Prioridad:
//   1. ratings explícitos user >= 4 (orden por rating DESC, tie-break por id ASC para determinismo)
//   2. Si no llenan n, proxy débil: completed=1 por last_watched DESC.
// Devuelve [] si no hay historial — es válido, no es error.
export async function getSemillas(n = 3): Promise<string[]> {
  const semillas: string[] = [];

  // 1. Ratings explícitos altos.
  const ratings = Object.entries(getRatingsMap())
    .filter(([, r]) => r.user != null && r.user >= 4)
    .sort(([idA, a], [idB, b]) => {
      const diff = (b.user ?? 0) - (a.user ?? 0);
      if (diff !== 0) return diff;
      return idA.localeCompare(idB);
    });
  for (const [id] of ratings) {
    if (!semillas.includes(id)) semillas.push(id);
    if (semillas.length >= n) return semillas;
  }

  // 2. Proxy débil: completed=1 más recientes.
  const db = await getDb();
  if (db) {
    try {
      // Pedimos n+5 para tener margen ante duplicados con ratings ya añadidos.
      const rows = await db.select<WatchRow[]>(
        `SELECT tmdb_id
           FROM watch_history
          WHERE completed = 1
            AND media_type = 'movie'
          ORDER BY last_watched DESC
          LIMIT $1`,
        [n + 5],
      );
      for (const r of rows) {
        const id = String(r.tmdb_id);
        if (!semillas.includes(id)) semillas.push(id);
        if (semillas.length >= n) break;
      }
    } catch (e) {
      console.warn("[vera/historial] error leyendo semillas:", e);
    }
  }

  // TODO REMOVE [B3-test] — verificar pruebas B3.
  const ratingsAltos = Object.entries(getRatingsMap()).filter(
    ([, r]) => r.user != null && r.user >= 4,
  ).length;
  console.log("[B3-test] semillas", {
    semillas,
    ratingsAltos,
  });

  return semillas;
}
