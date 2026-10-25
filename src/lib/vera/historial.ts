// Señales del usuario para alimentar el motor de recomendación.
// Fusiona cuatro fuentes:
//   1. localStorage `vera_reacciones_v2` (👍/👎 de Vera y calificaciones viejas)
//   2. SQLite `watch_history`            (lo que la home escribe al reproducir)
//   3. SQLite `favorites`                (lo que marcaste como favorito)
//   4. SQLite `vera_weights`             (memoria larga, ver aprendizaje.ts)
//
// Lo de la base es POR PERFIL (el que está viendo): cada persona de la casa
// tiene su Vera. Las reacciones y la memoria larga son del equipo.
//
// Solo lectura. Solo media_type='movie' (las series quedan fuera de Vera).
// Si la DB no se puede abrir (no-Tauri, permisos), degrada a solo reacciones.

import { getDb } from "./db";
import { getReaccionesMap, type ReaccionGuardada } from "./ratings";
import { getPesosAprendidos, familiaDeTag, pesoDeFamilia } from "./aprendizaje";
import { generoPorSlug } from "./generos";
import type { PerfilHistorico } from "./tipos";
import { perfilActual } from "$lib/perfilActivo";

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
export function pesoReaccionPersistida(r: ReaccionGuardada): number {
  if (r.juicio !== null) {
    return r.juicio; // -5..+5 directo (juicio manda)
  }
  return r.interes * 0.6; // intuición previa, peso reducido
}
//
// =============================================================================

interface WatchRow {
  tmdb_id: number;
}

interface FilaVista {
  tmdb_id: number;
  completed: number;
  watched_seconds: number;
  runtime_seconds: number | null;
  last_watched: number;
}

// Pesos de lo que se hace fuera de Vera. "Terminó la peli" no es "le gustó"
// (se termina por inercia), así que pesa menos que un 👍 (+4). Un favorito sí
// es una declaración. Dejarla a medias es una señal débil en contra.
const PESO_COMPLETA = 1;
const PESO_FAVORITA = 2;
const PESO_ABANDONADA = -0.75;
// A medias = entre el 5 % y el 40 %, y sin volver a ella en 2 días.
const ABANDONO_MIN = 0.05;
const ABANDONO_MAX = 0.4;
const ABANDONO_DIAS_MS = 2 * 86_400_000;

function abandonada(f: FilaVista, ahora: number): boolean {
  if (f.completed === 1 || !f.runtime_seconds || f.runtime_seconds <= 0) return false;
  const r = f.watched_seconds / f.runtime_seconds;
  return r >= ABANDONO_MIN && r <= ABANDONO_MAX && ahora - f.last_watched > ABANDONO_DIAS_MS;
}

// Películas vistas (todas, también las abandonadas) del perfil activo.
async function filasVistas(): Promise<FilaVista[]> {
  const db = await getDb();
  if (!db) return [];
  try {
    return await db.select<FilaVista[]>(
      `SELECT tmdb_id, completed, watched_seconds, runtime_seconds, last_watched
         FROM watch_history
        WHERE media_type = 'movie' AND perfil = $1 AND season = -1`,
      [perfilActual()],
    );
  } catch (e) {
    console.warn("[vera/historial] error leyendo watch_history:", e);
    return [];
  }
}

async function favoritas(): Promise<string[]> {
  const db = await getDb();
  if (!db) return [];
  try {
    const rows = await db.select<WatchRow[]>(
      `SELECT tmdb_id FROM favorites
        WHERE media_type = 'movie' AND perfil = $1
        ORDER BY added_at DESC`,
      [perfilActual()],
    );
    return rows.map((r) => String(r.tmdb_id));
  } catch (e) {
    console.warn("[vera/historial] error leyendo favoritos:", e);
    return [];
  }
}

/**
 * Las pelis de este perfil que le enseñan algo a Vera (vistas, a medias y
 * favoritas). Para pedir sus géneros antes de armar el perfil (ver
 * asegurarGeneros en tmdb.ts): lo visto desde la home no pasó por Vera.
 */
export async function idsConSenal(): Promise<string[]> {
  const ahora = Date.now();
  const vistas = (await filasVistas())
    .filter((f) => f.completed === 1 || abandonada(f, ahora))
    .sort((a, b) => b.last_watched - a.last_watched)
    .map((f) => String(f.tmdb_id));
  return [...new Set([...(await favoritas()), ...vistas])];
}

// Todas las pelis que el usuario ya vio.
// "Visto" =  vera_reacciones_v2[id].juicio !== null  OR  watch_history.completed = 1
// NO se usa para excluir del pool — las vistas aparecen con badge y penalizadas
// en el score (ver motor.ts). Sí se usa para ese cálculo y para las semillas.
export async function getVistas(): Promise<string[]> {
  const vistas = new Set<string>();

  // Fuente 1: reacciones locales con juicio (= visto y calificado).
  const reacciones = getReaccionesMap();
  for (const [id, r] of Object.entries(reacciones)) {
    if (r.juicio !== null) vistas.add(id);
  }

  // Fuente 2: watch_history del perfil, solo películas completadas (≥90%).
  for (const f of await filasVistas()) {
    if (f.completed === 1) vistas.add(String(f.tmdb_id));
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

  // 2) Favoritas del perfil: las marcó a propósito.
  for (const id of await favoritas()) {
    if (!semillas.includes(id)) semillas.push(id);
    if (semillas.length >= n) return semillas;
  }

  // 3) Proxy débil: lo último que terminó de ver este perfil.
  const completas = (await filasVistas())
    .filter((f) => f.completed === 1)
    .sort((a, b) => b.last_watched - a.last_watched);
  for (const f of completas) {
    const id = String(f.tmdb_id);
    if (!semillas.includes(id)) semillas.push(id);
    if (semillas.length >= n) break;
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
// Fuentes y pesos:
//   reacciones con juicio     → pesoReaccionPersistida(r) = juicio     ∈ [-5..+5]
//   reacciones solo interes   → pesoReaccionPersistida(r) = interes*0.6 ∈ [-3..+3]
//   watch_history completed=1 sin reacción → +1    (ver PESO_COMPLETA)
//   favorita sin reacción                  → +2
//   abandonada a medias                    → -0.75
//   vera_weights              → memoria larga, ya acumulada (aprendizaje.ts)
//
// Por qué +1 y no más: "terminó la peli" no es lo mismo que "le gustó" (se
// termina por inercia, costumbre o curiosidad). Sin el cuestionario viejo es
// la señal más abundante, así que pesa algo, pero muy por debajo de un 👍 (+4).
//
// IMPORTANTE: la decisión de "juicio manda sobre interes" vive en
// pesoReaccionPersistida() arriba. Una peli con juicio=-4 e interes=+3
// suma -4, NO -4+1.8. Si intuiste mal y la viste, gana la realidad.
//
// Los géneros salen de dos lados y NO se duplican: vera_generos_cache cubre
// lo que Vera mostró alguna vez, vera_weights cubre todo lo demás (y sobrevive
// a un borrado de localStorage). Se suman porque miden lo mismo en escalas
// compatibles: ambas son acumulados de pesoReaccionPersistida.
export async function getPerfilHistorico(): Promise<PerfilHistorico> {
  const generosPesos = new Map<string, number>();
  const decadasPesos = new Map<string, number>();
  const directoresPesos = new Map<string, number>();
  const actoresPesos = new Map<string, number>();
  const cacheGen = leerGenerosCache();
  let muestras = 0;

  function sumar(m: Map<string, number>, clave: string, peso: number): void {
    m.set(clave, (m.get(clave) ?? 0) + peso);
  }

  // 1) Reacciones persistidas (géneros vía cache local).
  const reacciones = getReaccionesMap();
  for (const [id, r] of Object.entries(reacciones)) {
    const peso = pesoReaccionPersistida(r);
    // Una reacción cuenta como muestra aunque su peso sea 0: el usuario opinó,
    // y eso es lo que mide `muestras` (cuánto te conozco, no cuánto te gusta).
    muestras++;
    if (peso === 0) continue;
    const generos = cacheGen[id];
    if (!generos) continue; // sin géneros cacheados, no atribuible.
    for (const g of generos) sumar(generosPesos, g, peso);
  }

  // 2) Lo que el perfil hizo fuera de Vera: vistas completas, a medias y
  // favoritas. Lo que ya tiene reacción explícita no se cuenta dos veces.
  const ahora = Date.now();
  const favs = new Set(await favoritas());
  const sinReaccion = (id: string) => !reacciones[id];
  const aportar = (id: string, peso: number) => {
    muestras++;
    const generos = cacheGen[id];
    if (!generos) return;
    for (const g of generos) sumar(generosPesos, g, peso);
  };
  for (const f of await filasVistas()) {
    const id = String(f.tmdb_id);
    if (!sinReaccion(id) || favs.has(id)) continue;
    if (f.completed === 1) aportar(id, PESO_COMPLETA);
    else if (abandonada(f, ahora)) aportar(id, PESO_ABANDONADA);
  }
  for (const id of favs) if (sinReaccion(id)) aportar(id, PESO_FAVORITA);

  // 3) Memoria larga: vera_weights. Las etiquetas vienen con prefijo de
  // familia y cada familia va a su propio mapa, ya escalada por su confianza
  // (un director acierta más que un actor de reparto; ver PESO_FAMILIA).
  for (const [tag, peso] of await getPesosAprendidos()) {
    const escala = pesoDeFamilia(tag);
    if (escala === 0) continue;
    const valor = peso * escala;
    const nombre = tag.slice(tag.indexOf(":") + 1);
    switch (familiaDeTag(tag)) {
      case "g": {
        // vera_weights guarda slugs v3; el resto del perfil habla en nombres
        // es-ES de TMDb. Sin esta traducción los dos mapas no se cruzarían.
        const g = generoPorSlug(nombre);
        if (g) sumar(generosPesos, g.nombre, valor);
        break;
      }
      case "dir":
        sumar(directoresPesos, nombre, valor);
        break;
      case "act":
        sumar(actoresPesos, nombre, valor);
        break;
      case "dec":
        sumar(decadasPesos, nombre, valor);
        break;
      // "tono" se ignora acá: el motor ya lo calcula por sesión y sumarlo
      // otra vez desde la memoria larga lo contaría doble.
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

  return {
    generosPesos,
    decadasPesos,
    directoresPesos,
    actoresPesos,
    top3,
    muestras,
  };
}
//
// =============================================================================

/**
 * Títulos de pelis de este perfil (historial y favoritos), por id TMDb. Para
 * explicar una recomendación con "porque viste X".
 */
export async function titulosDe(ids: string[]): Promise<Map<string, string>> {
  const out = new Map<string, string>();
  const nums = [...new Set(ids)].map(Number).filter(Number.isFinite);
  if (nums.length === 0) return out;
  const db = await getDb();
  if (!db) return out;
  const marcas = nums.map((_, i) => `$${i + 2}`).join(", ");
  try {
    for (const tabla of ["watch_history", "favorites"]) {
      const filas = await db.select<{ tmdb_id: number; title: string }[]>(
        `SELECT tmdb_id, title FROM ${tabla}
          WHERE perfil = $1 AND media_type = 'movie' AND tmdb_id IN (${marcas})`,
        [perfilActual(), ...nums],
      );
      for (const f of filas) if (f.title && !out.has(String(f.tmdb_id))) out.set(String(f.tmdb_id), f.title);
    }
  } catch (e) {
    console.warn("[vera/historial] no se pudieron leer los títulos:", e);
  }
  return out;
}
