// Cliente TMDb para el catálogo de Vera.
// Lee la API key de localStorage (la setea el usuario en /vera/catalog o en home).
// Llama a comandos Tauri en src-tauri/src/lib.rs:
//   - tmdb_discover     (extendido con filtros opcionales)
//   - tmdb_recommendations
//   - tmdb_detail       (enriquecimiento)
// Cachea el pool con TTL 24h, clave por hash(filtros + semillas).

import { invoke } from "@tauri-apps/api/core";
import type { Pelicula, Tono, Intencion } from "./tipos";
import { SEMILLA_FALLBACK } from "./probes";
import { getVistas, getSemillas } from "./historial";

// Géneros TMDb (numéricos) por intención. Reservado para B3.
// Cada intent abarca varios géneros para que discover devuelva variedad.
const GENEROS_POR_INTENCION: Record<Intencion, string> = {
  // Comedia, Animación, Familia, Aventura, Romance
  liviano: "35,16,10751,12,10749",
  // Drama, Documental, Historia, Misterio
  denso: "18,99,36,9648",
  // Acción, Suspense, Terror, Ciencia ficción, Bélica
  adrenalina: "28,53,27,878,10752",
  // Sin filtro de género — solo populares.
  sorpresa: "",
};
// Referencia explícita para evitar warning de unused — se usará en B3.
void GENEROS_POR_INTENCION;

interface TmdbItemMini {
  id: number;
  title?: string;
  name?: string;
  poster_path?: string | null;
  overview?: string;
  vote_average?: number;
  release_date?: string;
}
interface TmdbListResp {
  page: number;
  total_pages: number;
  results: TmdbItemMini[];
}

// Cache vieja del sistema previo (pool fijo). Se borra una sola vez
// en migrarCacheVieja(). No reutilizar este nombre.
const CACHE_VIEJA = "vera_catalogo_v2";

// Cache nueva: clave = vera_pool_<hash> donde el hash combina filtros + semillas.
// TTL 24h porque /discover varía poco intra-día pero queremos frescura semanal.
const CACHE_PREFIX = "vera_pool_";
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

// Mínimo de pelis para que el mazo (8) funcione bien.
// Por debajo, activamos el fallback de SEMILLA_FALLBACK.
const MIN_POOL = 8;

// Lo que devuelve `tmdb_detail` desde Rust. Solo los campos que usamos.
interface TmdbDetail {
  id: number;
  media_type: string;
  title: string;
  overview: string;
  poster_path: string | null;
  backdrop_path: string | null;
  vote_average: number;
  year: string;
  imdb_id: string | null;
  runtime: number | null;
  genres: string[];
  directors: { id: number; name: string }[];
  cast: { id: number; name: string; character: string | null }[];
}

// Filtros del cliente para `tmdb_discover`. Camel case en el invoke (Tauri convención).
export interface FiltrosDiscover {
  with_genres?: string;              // "28,12" estilo TMDb
  primary_release_date_gte?: string; // "2020-01-01"
  primary_release_date_lte?: string;
  vote_average_gte?: number;
  vote_count_gte?: number;           // default 100
  with_original_language?: string;   // "es", "en"
  sort_by?: string;                  // default "popularity.desc"
  page?: number;                     // default 1 (la frescura la da el TTL)
}

// Géneros TMDb en español (LANG=es-ES en Rust).
// Mapeo a tono. Si una peli tiene varios géneros, gana el bloque que más aporta.
const GENEROS_LIVIANOS = new Set([
  "Comedia",
  "Animación",
  "Familia",
  "Música",
  "Romance",
  "Aventura",
]);
const GENEROS_DENSOS = new Set([
  "Drama",
  "Terror",
  "Suspense",
  "Misterio",
  "Crimen",
  "Bélica",
  "Documental",
  "Historia",
]);

function determinarTono(generos: string[]): Tono {
  let l = 0;
  let d = 0;
  for (const g of generos) {
    if (GENEROS_LIVIANOS.has(g)) l++;
    if (GENEROS_DENSOS.has(g)) d++;
  }
  return l >= d ? "liviano" : "denso";
}

// familyFriendly conservador: solo lo marcamos true si hay género family/animation.
// Cualquier género adulto-ish lo descarta. Si no hay señal, false.
const GENEROS_FAMILY_OK = new Set(["Familia", "Animación"]);
const GENEROS_NO_FAMILY = new Set([
  "Terror",
  "Suspense",
  "Crimen",
  "Bélica",
  "Misterio",
]);

function determinarFamily(generos: string[]): boolean {
  if (generos.some((g) => GENEROS_NO_FAMILY.has(g))) return false;
  if (generos.some((g) => GENEROS_FAMILY_OK.has(g))) return true;
  return false;
}

// Una trivia chica, generada por reglas. No es "trivia" real (eso necesita IA
// o DB específica como IMDb). Es un resumen factual con sabor a guiño.
function generarTrivia(d: TmdbDetail): string {
  const opts: string[] = [];
  if (d.directors[0]) opts.push(`Dirigida por ${d.directors[0].name}`);
  if (d.runtime) opts.push(`${d.runtime} minutos exactos`);
  if (d.vote_average >= 8) {
    opts.push(`TMDb le pone ${d.vote_average.toFixed(1)}/10`);
  }
  if (d.cast.length >= 2) {
    opts.push(`Con ${d.cast[0].name} y ${d.cast[1].name}`);
  }
  if (d.year && parseInt(d.year, 10) < 2000) {
    opts.push(`Clásico del ${d.year}, sigue de pie`);
  }
  if (opts.length === 0) return "";
  return opts[Math.floor(Math.random() * opts.length)];
}

// Primer oración del overview, como gancho corto.
function ganchoDe(overview: string): string {
  if (!overview) return "";
  const punto = overview.indexOf(". ");
  if (punto > 30 && punto < 140) return overview.slice(0, punto + 1);
  if (overview.length <= 140) return overview;
  return overview.slice(0, 137) + "…";
}

// idx/total como proxy de popularidad. Lo deja B4 para mejorar
// con popularity / vote_count reales.
function mapear(d: TmdbDetail, idx: number, total: number): Pelicula {
  const generos = d.genres ?? [];
  const tono = determinarTono(generos);
  const familyFriendly = determinarFamily(generos);
  const director = d.directors[0]?.name ?? "Sin director";
  const actores = d.cast.slice(0, 4).map((c) => c.name);
  const popularidad = 1 - idx / Math.max(1, total - 1);
  return {
    id: String(d.id),
    titulo: d.title,
    generos,
    tono,
    familyFriendly,
    rating: d.vote_average,
    popularidad,
    pais: "",
    poster: "#1f3a4d",
    gancho: ganchoDe(d.overview),
    descripcion: d.overview ?? "",
    director,
    actores,
    anio: d.year ?? "",
    runtime: d.runtime,
    posterPath: d.poster_path,
    backdropPath: d.backdrop_path,
    trivia: generarTrivia(d),
  };
}

// --- Cache: hash, lectura, escritura, purga, migración ---

function hashFiltros(filtros: FiltrosDiscover, semillas: string[]): string {
  const payload = {
    ...filtros,
    _seeds: [...semillas].sort().join(","),
  };
  const stable = JSON.stringify(
    payload,
    Object.keys(payload).sort(),
  );
  // djb2 simple, suficiente para una clave de localStorage.
  let h = 5381;
  for (let i = 0; i < stable.length; i++) {
    h = ((h * 33) ^ stable.charCodeAt(i)) >>> 0;
  }
  return h.toString(36);
}

interface PoolCacheado {
  ts: number;
  pelis: Pelicula[];
}

function leerPool(hash: string): Pelicula[] | null {
  try {
    const raw = localStorage.getItem(CACHE_PREFIX + hash);
    if (!raw) return null;
    const data = JSON.parse(raw) as PoolCacheado;
    if (Date.now() - data.ts > CACHE_TTL_MS) {
      // Expirada: borrar y reportar miss.
      localStorage.removeItem(CACHE_PREFIX + hash);
      return null;
    }
    return data.pelis;
  } catch {
    return null;
  }
}

function guardarPool(hash: string, pelis: Pelicula[]): void {
  try {
    const body: PoolCacheado = { ts: Date.now(), pelis };
    localStorage.setItem(CACHE_PREFIX + hash, JSON.stringify(body));
  } catch {
    // localStorage lleno / privado — ignorar.
  }
}

// Barre todas las claves CACHE_PREFIX y borra las vencidas.
// Importante porque los hashes cambian con filtros/semillas y se acumulan.
// Idempotente, segura de llamar muchas veces.
function purgarPoolsVencidos(): void {
  try {
    const aBorrar: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k || !k.startsWith(CACHE_PREFIX)) continue;
      try {
        const raw = localStorage.getItem(k);
        if (!raw) continue;
        const data = JSON.parse(raw) as PoolCacheado;
        if (Date.now() - data.ts > CACHE_TTL_MS) aBorrar.push(k);
      } catch {
        // Entrada corrupta: borrar también.
        aBorrar.push(k);
      }
    }
    for (const k of aBorrar) localStorage.removeItem(k);
  } catch {
    // Sin localStorage — nada que purgar.
  }
}

// Migración one-shot: borra la cache fija del sistema previo.
// Idempotente, segura de llamar muchas veces.
function migrarCacheVieja(): void {
  try {
    if (localStorage.getItem(CACHE_VIEJA) !== null) {
      localStorage.removeItem(CACHE_VIEJA);
    }
  } catch {
    // Sin localStorage — nada que migrar.
  }
}

// --- Capa baja: invoca comandos Rust ---

async function discoverItems(
  filtros: FiltrosDiscover,
  apiKey: string,
): Promise<TmdbItemMini[]> {
  const resp = await invoke<TmdbListResp>("tmdb_discover", {
    mediaType: "movie",
    page: filtros.page ?? 1,
    apiKey,
    sortBy: filtros.sort_by ?? "popularity.desc",
    withGenres: filtros.with_genres,
    voteAverageGte: filtros.vote_average_gte,
    voteCountGte: filtros.vote_count_gte ?? 100,
    primaryReleaseDateGte: filtros.primary_release_date_gte,
    primaryReleaseDateLte: filtros.primary_release_date_lte,
    withOriginalLanguage: filtros.with_original_language,
  });
  return resp.results ?? [];
}

async function recommendationsItems(
  seedId: string,
  apiKey: string,
  kind: "recommendations" | "similar" = "recommendations",
  page = 1,
): Promise<TmdbItemMini[]> {
  const resp = await invoke<TmdbListResp>("tmdb_recommendations", {
    mediaType: "movie",
    id: Number(seedId),
    page,
    apiKey,
    kind,
  });
  return resp.results ?? [];
}

// Enriquece N items de listado con tmdb_detail (cast, director, runtime, etc.).
// Paraleliza. Los que fallan se descartan.
async function enriquecer(
  items: TmdbItemMini[],
  apiKey: string,
): Promise<Pelicula[]> {
  const total = items.length;
  const promesas = items.map(async (it, idx) => {
    try {
      const d = await invoke<TmdbDetail>("tmdb_detail", {
        mediaType: "movie",
        id: it.id,
        apiKey,
      });
      return mapear(d, idx, total);
    } catch (e) {
      console.warn("[vera/tmdb] no se pudo enriquecer", it.id, e);
      return null;
    }
  });
  const rs = await Promise.all(promesas);
  return rs.filter((p): p is Pelicula => p !== null);
}

// Igual que enriquecer pero a partir de IDs sueltas (para SEMILLA_FALLBACK).
async function enriquecerIds(
  ids: number[],
  apiKey: string,
): Promise<Pelicula[]> {
  const total = ids.length;
  const promesas = ids.map(async (id, idx) => {
    try {
      const d = await invoke<TmdbDetail>("tmdb_detail", {
        mediaType: "movie",
        id,
        apiKey,
      });
      return mapear(d, idx, total);
    } catch (e) {
      console.warn("[vera/tmdb] fallback falló", id, e);
      return null;
    }
  });
  return (await Promise.all(promesas)).filter(
    (p): p is Pelicula => p !== null,
  );
}

// --- API pública ---

export class TmdbNoKeyError extends Error {
  constructor() {
    super(
      "Falta API key TMDb. Configúrala desde la pantalla principal o desde /vera/catalog.",
    );
  }
}

export async function fetchDiscover(
  filtros: FiltrosDiscover,
): Promise<Pelicula[]> {
  const apiKey = localStorage.getItem("tmdb_key") ?? "";
  if (!apiKey) throw new TmdbNoKeyError();
  const items = await discoverItems(filtros, apiKey);
  return enriquecer(items.slice(0, 20), apiKey);
}

export async function fetchRecommendations(
  seedIds: string[],
  kind: "recommendations" | "similar" = "recommendations",
): Promise<Pelicula[]> {
  const apiKey = localStorage.getItem("tmdb_key") ?? "";
  if (!apiKey) throw new TmdbNoKeyError();
  const seeds = seedIds.slice(0, 3);
  const items: TmdbItemMini[] = [];
  for (const seed of seeds) {
    try {
      const rs = await recommendationsItems(seed, apiKey, kind, 1);
      items.push(...rs);
    } catch (e) {
      console.warn("[vera/tmdb] reco falló para", seed, e);
    }
  }
  // Dedupe antes de enriquecer (más barato).
  const dedup = new Map<number, TmdbItemMini>();
  for (const it of items) if (!dedup.has(it.id)) dedup.set(it.id, it);
  return enriquecer([...dedup.values()].slice(0, 20), apiKey);
}

// Pool del mazo. Mezcla discover + recommendations, dedupe por id,
// garantiza mínimo con SEMILLA_FALLBACK.
// El filtro de yaVistas se aplica SIEMPRE a la salida — la cache guarda
// el pool completo, no filtrado, para que el historial pueda crecer sin
// congelar el snapshot.
// La firma incluye yaVistas desde B1 (B1 lo deja vacío; B2 lo conectará).
export async function construirPool(
  filtros: FiltrosDiscover,
  semillas: string[],
  yaVistas: string[],
): Promise<Pelicula[]> {
  migrarCacheVieja();
  purgarPoolsVencidos();

  const hash = hashFiltros(filtros, semillas);
  const yaSet = new Set(yaVistas);

  // TODO REMOVE [B3-test] — instrumentación temporal para verificar pruebas B3.
  console.log("[B3-test] construirPool", {
    hash,
    filtros,
    semillas,
    yaVistas_count: yaVistas.length,
  });

  const cached = leerPool(hash);
  if (cached) {
    // TODO REMOVE [B3-test]
    console.log("[B3-test] cache HIT", {
      hash,
      cached_count: cached.length,
    });
    // Cache hit: pool completo sin filtrar. Filtrar SIEMPRE a la salida.
    return cached.filter((p) => !yaSet.has(p.id));
  }

  const apiKey = localStorage.getItem("tmdb_key") ?? "";
  if (!apiKey) throw new TmdbNoKeyError();

  const [disc, reco] = await Promise.all([
    fetchDiscover(filtros).catch(() => [] as Pelicula[]),
    semillas.length > 0
      ? fetchRecommendations(semillas).catch(() => [] as Pelicula[])
      : Promise.resolve([] as Pelicula[]),
  ]);

  // TODO REMOVE [B3-test]
  console.log("[B3-test] traído", {
    discover: disc.length,
    reco: reco.length,
  });

  // Merge: prioriza recommendations (señal del usuario), discover rellena.
  const pool = new Map<string, Pelicula>();
  for (const p of reco) pool.set(p.id, p);
  for (const p of disc) if (!pool.has(p.id)) pool.set(p.id, p);

  let completo = [...pool.values()];

  // Fallback: si el pool COMPLETO no llega al mínimo, traer SEMILLA_FALLBACK.
  if (completo.length < MIN_POOL) {
    const yaEnPool = new Set(completo.map((p) => p.id));
    const candidatos = SEMILLA_FALLBACK.filter(
      (id) => !yaEnPool.has(String(id)),
    );
    const fallback = await enriquecerIds(candidatos, apiKey);
    completo = [...completo, ...fallback];
  }

  // TODO REMOVE [B3-test]
  console.log("[B3-test] pool final", {
    total: completo.length,
    titulos: completo.slice(0, 8).map((p) => p.titulo),
  });

  // Cache: pool completo, sin filtrar por vistas.
  guardarPool(hash, completo);

  // Filtro vistas a la salida.
  return completo.filter((p) => !yaSet.has(p.id));
}

// Carga el pool para un intent ya resuelto a filtros.
// El caller (B3 = +page.svelte) computa los filtros UNA vez con
// filtrosParaIntent(intent) y los pasa acá. No recalcular acá adentro
// porque el sort_by de "sorpresa" es aleatorio y debe quedar fijo
// durante la sesión.
//
// Internamente compone las señales del historial (B2):
//   - semillas: top pelis que el usuario calificó alto (o completó reciente)
//   - yaVistas: para excluir lo que ya consumió
// y delega a construirPool, que maneja merge, dedupe, cache y fallback.
export async function cargarCatalogoPorIntent(
  filtros: FiltrosDiscover,
): Promise<Pelicula[]> {
  const [semillas, yaVistas] = await Promise.all([
    getSemillas(3),
    getVistas(),
  ]);
  return construirPool(filtros, semillas, yaVistas);
}

// URLs públicas del CDN TMDb. No requieren API key.
const IMG_BASE = "https://image.tmdb.org/t/p";

export function posterUrl(
  p: Pelicula,
  size: "w342" | "w500" | "w780" = "w500",
): string {
  if (!p.posterPath) return "";
  return `${IMG_BASE}/${size}${p.posterPath}`;
}

export function backdropUrl(
  p: Pelicula,
  size: "w780" | "w1280" = "w1280",
): string {
  if (!p.backdropPath) return "";
  return `${IMG_BASE}/${size}${p.backdropPath}`;
}
