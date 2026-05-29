// Señales del usuario para alimentar el motor de recomendación.
// Fusiona dos fuentes:
//   1. localStorage `vera_reacciones_v2` (lo que Vera registra en flujo B5)
//   2. SQLite `watch_history`           (lo que la home escribe al reproducir)
//
// Solo lectura. Solo media_type='movie' (las series quedan fuera de Vera).
// Handle SQLite propio cacheado — no compartido con la home, para no acoplar.
// Si la DB no se puede abrir (no-Tauri, permisos), degrada a solo reacciones.

import Database from "@tauri-apps/plugin-sql";
import { getReaccionesMap, type ReaccionGuardada } from "./ratings";
import type { PerfilHistorico } from "./tipos";

// =============================================================================
// LÓGICA DE PESO — REVISAR A OJO, EL TYPE-CHECK NO LA VALIDA.
// =============================================================================
//
// Cada reacción persistida tiene { interes ∈ [-5..+5], juicio ∈ [-5..+5] | null }.
// Cuando hay juicio (la peli fue vista y calificada post-vista), MANDA juicio.
// Si solo hay interes (intuición previa), pesa menos porque es predicción no
// validada.
//
// Coeficiente para interes-sin-juicio: 0.6. Convierte rango -5..+5 a -3..+3
// efectivo. Es deliberadamente menor que el rango completo del juicio.
//
// Si juicio === 0 (vista pero "ni fu ni fa"), el peso es 0 — no aporta señal
// pero tampoco se cae al interes previo. La decisión de marcar visto borra
// la intuición.
function pesoReaccionPersistida(r: ReaccionGuardada): number {
  if (r.juicio !== null) {
    return r.juicio; // -5..+5 directo (juicio manda)
  }
  return r.interes * 0.6; // intuición previa, peso reducido
}
//
// =============================================================================

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

// Todas las pelis que el usuario ya vio.
// "Visto" =  vera_reacciones_v2[id].juicio !== null  OR  watch_history.completed = 1
// En B5 NO se usa para excluir del pool — las vistas aparecen con badge.
// Se mantiene por si futuros consumidores quieren la lista.
export async function getVistas(): Promise<string[]> {
  const vistas = new Set<string>();

  // Fuente 1: reacciones locales con juicio (= visto y calificado).
  const reacciones = getReaccionesMap();
  for (const [id, r] of Object.entries(reacciones)) {
    if (r.juicio !== null) vistas.add(id);
  }

  // Fuente 2: watch_history, solo películas completadas (≥90%).
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

// =============================================================================
// SEMILLAS — REVISAR LA REGLA DE PRIORIDAD A OJO.
// =============================================================================
//
// Top n pelis "que le gustaron" para usar como semilla de /recommendations.
//
// Thresholds EXPLÍCITOS por tipo de señal (no usar el *0.6 para decidir el
// corte de forma indirecta — el número que se lee es el número que aplica):
//   juicio  >= 2  (la calificó bien tras verla)
//   interes >= 3  (intuición fuerte sin haberla visto, "me llama bastante")
//
// Tie-break para ordenar candidatos: por peso descendente (pesoReaccionPersistida,
// que aplica el *0.6 solo dentro del ordenamiento), después por id ASC para
// determinismo. El *0.6 sirve para que un juicio=4 ordene antes que un
// interes=5 (interes*0.6 = 3 < 4). Pero el threshold de entrada al pool de
// candidatos NO depende del *0.6.
//
// Si no llenan n: proxy débil (watch_history completed=1 más reciente).
// Devuelve [] si no hay historial — válido, no es error.
export async function getSemillas(n = 3): Promise<string[]> {
  const semillas: string[] = [];

  // Candidatos con peso (para ordenarlos).
  const candidatos: { id: string; peso: number }[] = [];

  // Thresholds explícitos por tipo de señal.
  for (const [id, r] of Object.entries(getReaccionesMap())) {
    const calificaJuicio = r.juicio !== null && r.juicio >= 2;
    const calificaInteres = r.juicio === null && r.interes >= 3;
    if (calificaJuicio || calificaInteres) {
      candidatos.push({ id, peso: pesoReaccionPersistida(r) });
    }
  }

  // Ordenar por peso DESC (el *0.6 dentro de pesoReaccionPersistida hace
  // que juicio ordene antes que interes con el mismo número), tie-break id ASC.
  candidatos.sort((a, b) => {
    const d = b.peso - a.peso;
    return d !== 0 ? d : a.id.localeCompare(b.id);
  });

  for (const c of candidatos) {
    if (!semillas.includes(c.id)) semillas.push(c.id);
    if (semillas.length >= n) return semillas;
  }

  // 3) Proxy débil: completed=1 más recientes (no hay reacción explícita).
  const db = await getDb();
  if (db) {
    try {
      const rows = await db.select<WatchRow[]>(
        `SELECT tmdb_id
           FROM watch_history
          WHERE completed = 1
            AND media_type = 'movie'
          ORDER BY last_watched DESC
          LIMIT $1`,
        [n + 5], // margen ante dups con reacciones ya añadidas
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

  return semillas;
}
//
// =============================================================================

// Cache de géneros poblada por tmdb.ts.mapear(). Lectura solo, no escribe.
const CACHE_GENEROS = "vera_generos_cache";

function leerGenerosCache(): Record<string, string[]> {
  try {
    const raw = localStorage.getItem(CACHE_GENEROS);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

// =============================================================================
// PERFIL HISTÓRICO — REVISAR PESOS A OJO. EL TYPE-CHECK NO LOS VALIDA.
// =============================================================================
//
// Perfil histórico del usuario para el motor.
// Fuentes:
//   1. vera_reacciones_v2 (reacciones explícitas de Vera).
//   2. watch_history (visto en la home sin reacción en Vera).
// Cruza con vera_generos_cache para obtener géneros sin pegarle a TMDb.
//
// Pelis no cacheadas (vistas solo en la home, nunca por Vera) se saltean.
// Entran al perfil naturalmente cuando aparezcan en un pool y pasen por
// mapear(). NO llamamos tmdb_detail explícito acá.
//
// Pesos por fuente:
//   reacciones con juicio → pesoReaccionPersistida(r) = juicio   ∈ [-5..+5]
//   reacciones solo interes → pesoReaccionPersistida(r) = interes*0.6 ∈ [-3..+3]
//   watch_history completed=1 SIN reacción → +0.5  (proxy MUY débil)
//
// Por qué +0.5 (y no +1): "terminó la peli" no es lo mismo que "le gustó".
// Una peli se termina por inercia, costumbre, o simple curiosidad. Equiparar
// este comportamiento inferido con un juicio explícito de +1 sobrevalora la
// señal pasiva. Lo dejamos abajo del juicio chico más bajo.
//
// IMPORTANTE: la decisión de "juicio manda sobre interes" vive en
// pesoReaccionPersistida() arriba. Una peli con juicio=-4 e interes=+3
// suma -4, NO -4+1.8. Si intuiste mal y la viste, gana la realidad.
export async function getPerfilHistorico(): Promise<PerfilHistorico> {
  const generosPesos = new Map<string, number>();
  const cacheGen = leerGenerosCache();

  function sumar(generos: string[], peso: number): void {
    for (const g of generos) {
      generosPesos.set(g, (generosPesos.get(g) ?? 0) + peso);
    }
  }

  // 1) Reacciones persistidas.
  const reacciones = getReaccionesMap();
  for (const [id, r] of Object.entries(reacciones)) {
    const generos = cacheGen[id];
    if (!generos) continue; // sin géneros cacheados, no atribuible.
    const peso = pesoReaccionPersistida(r);
    if (peso === 0) continue; // optimización: no sumar ceros.
    sumar(generos, peso);
  }

  // 2) watch_history sin reacción local (peso fijo bajo).
  const db = await getDb();
  if (db) {
    try {
      const rows = await db.select<WatchRow[]>(
        `SELECT tmdb_id
           FROM watch_history
          WHERE completed = 1
            AND media_type = 'movie'`,
      );
      for (const r of rows) {
        const id = String(r.tmdb_id);
        if (reacciones[id]) continue; // ya entró por reacciones, no duplicar.
        const generos = cacheGen[id];
        if (!generos) continue;
        sumar(generos, 0.5); // proxy MUY débil — comportamiento inferido
      }
    } catch (e) {
      console.warn("[vera/historial] error perfil watch_history:", e);
    }
  }

  // Top 3 géneros por peso descendente, tie-break por nombre ASC.
  const top3 = [...generosPesos.entries()]
    .sort(([na, a], [nb, b]) => {
      const d = b - a;
      return d !== 0 ? d : na.localeCompare(nb);
    })
    .slice(0, 3)
    .map(([g]) => g);

  return { generosPesos, top3 };
}
//
// =============================================================================
