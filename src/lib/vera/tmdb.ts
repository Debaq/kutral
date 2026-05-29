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

// Cache de géneros por tmdb_id en localStorage.
// Sin TTL — los géneros de una peli no cambian. La pobla mapear() cada vez
// que enriquecemos una peli, y la consume historial.getPerfilHistorico().
// Permite tener perfil histórico real sin pegarle de nuevo a TMDb.
const CACHE_GENEROS = "vera_generos_cache";

function cacheGenerosDe(tmdbId: number, generos: string[]): void {
  if (generos.length === 0) return;
  try {
    const raw = localStorage.getItem(CACHE_GENEROS);
    const cache: Record<string, string[]> = raw ? JSON.parse(raw) : {};
    cache[String(tmdbId)] = generos;
    localStorage.setItem(CACHE_GENEROS, JSON.stringify(cache));
  } catch {
    // localStorage lleno / privado — ignorar, no es fatal.
  }
}

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

// Caches viejas del pool. Se purgan al iniciar Vera. NO cachear pools
// nuevos: B6 prioriza variedad ("cada entrada a Vera = pelis nuevas").
const CACHE_VIEJA_V2 = "vera_catalogo_v2";
const CACHE_VIEJA_PREFIX = "vera_pool_";

// Keyword TMDb sombrilla para representación LGBT+.
// Garantiza presencia mínima en cada pool, sin importar el intent.
// Sin marcar, sin etiquetar, sin sección aparte — solo presencia mezclada.
const KEYWORD_LGBT = "158718";
const PRESENCIA_GARANTIZADA_LGBT = 1;

// Mínimo de pelis para el pool. Por debajo, activamos SEMILLA_FALLBACK.
// 8 = cantidad de cartas que el usuario va a calificar.
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
  images: string[];
}

// Filtros del cliente para `tmdb_discover`. Camel case en el invoke (Tauri convención).
export interface FiltrosDiscover {
  with_genres?: string;              // "28,12" estilo TMDb (coma=AND, pipe=OR)
  with_keywords?: string;            // ej. "158718" para LGBT+ (mismas reglas)
  primary_release_date_gte?: string; // "2020-01-01"
  primary_release_date_lte?: string;
  vote_average_gte?: number;
  vote_count_gte?: number;           // default 100
  with_original_language?: string;   // "es", "en"
  sort_by?: string;                  // default "popularity.desc"
  page?: number;                     // default 1 (la frescura la da el TTL)
}

// Tabla de pesos por género para determinar el tono.
// Reemplaza los sets binarios anteriores (LIVIANOS/DENSOS) para capturar
// gravedad: una peli "Comedia + Crimen + Acción" no debería contar como
// liviana porque tenga Comedia entre sus etiquetas.
//   +N → tira a liviano
//   -N → tira a denso
//    0 → neutro (no aporta)
const PESOS_TONO: Record<string, number> = {
  Comedia: 2,
  Animación: 2,
  Familia: 2,
  Romance: 1,
  Aventura: 1,
  Música: 1,
  Fantasía: 1,
  Drama: -1,
  Documental: -1,
  Historia: -1,
  Misterio: -1,
  "Ciencia ficción": -1,
  Western: -1,
  Acción: -2,
  Crimen: -2,
  Suspense: -2,
  Terror: -3,
  Bélica: -3,
  "Película de TV": 0,
};

function determinarTono(generos: string[]): Tono {
  let s = 0;
  for (const g of generos) s += PESOS_TONO[g] ?? 0;
  return s >= 0 ? "liviano" : "denso";
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

// Tabla canónica de géneros TMDb (movies, LANG=es-ES). Una sola fuente de verdad.
// Si TMDb agrega o renombra un género, actualizar acá y la inversa se reajusta.
const GENEROS_TMDB: Record<string, string> = {
  "28":    "Acción",
  "12":    "Aventura",
  "16":    "Animación",
  "35":    "Comedia",
  "80":    "Crimen",
  "99":    "Documental",
  "18":    "Drama",
  "10751": "Familia",
  "14":    "Fantasía",
  "36":    "Historia",
  "27":    "Terror",
  "10402": "Música",
  "9648":  "Misterio",
  "10749": "Romance",
  "878":   "Ciencia ficción",
  "10770": "Película de TV",
  "53":    "Suspense",
  "10752": "Bélica",
  "37":    "Western",
};

// Inversa derivada — única fuente: GENEROS_TMDB.
const NOMBRE_A_ID_TMDB: Record<string, string> = Object.fromEntries(
  Object.entries(GENEROS_TMDB).map(([id, n]) => [n, id]),
);

// Verificación de boot: caza typos en las tablas locales contra la canónica.
// Si algún nombre usado en PESOS_TONO o en los sets de familia no existe
// en GENEROS_TMDB → warning. Costo: 0, corre solo al cargar el módulo.
{
  const canonicos = new Set(Object.values(GENEROS_TMDB));
  for (const s of [
    ...Object.keys(PESOS_TONO),
    ...GENEROS_FAMILY_OK,
    ...GENEROS_NO_FAMILY,
  ]) {
    if (!canonicos.has(s)) {
      console.warn(
        "[vera/tmdb] género en tabla local no existe en GENEROS_TMDB:",
        s,
      );
    }
  }
}

// Tono esperado para un intent dado, derivado del signo agregado de los
// pesos de sus géneros. Si el intent está vacío (sorpresa) → null = sin
// restricción de tono.
function tonoEsperadoDeIntent(idsIntencion: Set<string>): Tono | null {
  if (idsIntencion.size === 0) return null;
  let s = 0;
  for (const id of idsIntencion) {
    const nombre = GENEROS_TMDB[id];
    if (!nombre) continue;
    s += PESOS_TONO[nombre] ?? 0;
  }
  if (s === 0) return null;
  return s > 0 ? "liviano" : "denso";
}

// Decide si una peli es compatible con el intent.
// Dos condiciones (ambas deben cumplirse):
//   (a) al menos un género en común con el intent.
//   (b) el tono de la peli coincide con el tono esperado del intent.
// Sin intent (sorpresa, idsIntencion vacío) acepta todo.
// Si encuentra un género que TMDb devolvió pero NO está en GENEROS_TMDB,
// warnea — caza desincronización silenciosa entre TMDb y nuestra tabla.
function compatibleConIntencion(
  p: Pelicula,
  idsIntencion: Set<string>,
): boolean {
  if (idsIntencion.size === 0) return true;

  // (a) Matcheo de género.
  let matchGenero = false;
  for (const g of p.generos) {
    const id = NOMBRE_A_ID_TMDB[g];
    if (id === undefined) {
      console.warn(
        "[vera/tmdb] género TMDb sin id canónico (revisar GENEROS_TMDB):",
        g,
        "en",
        p.titulo,
      );
      continue;
    }
    if (idsIntencion.has(id)) {
      matchGenero = true;
      break;
    }
  }
  if (!matchGenero) return false;

  // (b) Matcheo de tono. Una peli con "Comedia + Crimen + Acción" matcheaba
  // por Comedia pero su tono ponderado es denso → no la queremos en liviano.
  const tonoReq = tonoEsperadoDeIntent(idsIntencion);
  if (tonoReq !== null && p.tono !== tonoReq) return false;

  return true;
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
  // Cacheo géneros para que getPerfilHistorico no tenga que pegarle a TMDb.
  cacheGenerosDe(d.id, generos);
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
    imagenes: d.images ?? [],
    trivia: generarTrivia(d),
    // Default: discover (la mayoría viene de ahí). construirPool sobreescribe
    // a "reco" o "fallback" cuando corresponde.
    procedencia: "discover",
  };
}

// --- Purga de cache vieja (B6: ya no cacheamos pools) ---

// One-shot al inicio de cada construirPool: barre todas las claves de cache
// previas (B5 y antes) que se acumulan en localStorage. Idempotente.
// B6 NO escribe cache nueva: cada entrada a Vera trae pool fresco.
function purgarCachesViejas(): void {
  try {
    if (localStorage.getItem(CACHE_VIEJA_V2) !== null) {
      localStorage.removeItem(CACHE_VIEJA_V2);
    }
    const aBorrar: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && k.startsWith(CACHE_VIEJA_PREFIX)) aBorrar.push(k);
    }
    for (const k of aBorrar) localStorage.removeItem(k);
  } catch {
    // Sin localStorage — nada que purgar.
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
    withKeywords: filtros.with_keywords,
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
// Construye el pool del mazo. B6: SIN cache. Cada llamada = pool fresco
// desde TMDb. La variedad entre sesiones se garantiza con page+sort_by
// aleatorios en intenciones.ts.
//
// Sí mantiene la firma con yaVistas por si en el futuro se quiere usar.
// Por ahora yaVistas se ignora — las vistas aparecen en el ranking con badge.
export async function construirPool(
  filtros: FiltrosDiscover,
  semillas: string[],
  yaVistas: string[],
): Promise<Pelicula[]> {
  // Limpieza one-shot de caches viejos (B5 y anteriores) acumulados.
  purgarCachesViejas();

  const apiKey = localStorage.getItem("tmdb_key") ?? "";
  if (!apiKey) throw new TmdbNoKeyError();

  const [disc, reco] = await Promise.all([
    fetchDiscover(filtros).catch(() => [] as Pelicula[]),
    semillas.length > 0
      ? fetchRecommendations(semillas).catch(() => [] as Pelicula[])
      : Promise.resolve([] as Pelicula[]),
  ]);

  // Filtrar recommendations a las compatibles con el intent.
  // Decisión de producto (B3 fix): el intent específico manda sobre el
  // historial. Sin esto, recommendations del historial pisa al intent
  // ("pedí comedia, me trajo crimen"). Para sorpresa (idsIntencion vacío)
  // acepta todo. Split defensivo coma O pipe (intenciones.ts usa pipe).
  const idsIntencion = new Set(
    (filtros.with_genres ?? "").split(/[,|]/).filter(Boolean),
  );
  const recoCompatible = reco.filter((p) =>
    compatibleConIntencion(p, idsIntencion),
  );

  // Merge: discover primero (representa el intent puro), reco rellena.
  // Procedencia asignada al insertar para que el motor pueda pesar "reco".
  const pool = new Map<string, Pelicula>();
  for (const p of disc) pool.set(p.id, { ...p, procedencia: "discover" });
  for (const p of recoCompatible) {
    if (!pool.has(p.id)) pool.set(p.id, { ...p, procedencia: "reco" });
  }

  // 4ta fuente: presencia LGBT+ garantizada.
  // /discover?with_keywords=158718 SIN cruzar con género del intent (la
  // intersección sería casi vacía). NO se filtra con compatibleConIntencion
  // por el mismo motivo. Se inyectan hasta PRESENCIA_GARANTIZADA_LGBT.
  try {
    const lgbt = await fetchDiscover({
      with_keywords: KEYWORD_LGBT,
      sort_by: "popularity.desc",
      vote_count_gte: 50,
      page: 1,
    });
    let inyectadas = 0;
    for (const p of lgbt) {
      if (inyectadas >= PRESENCIA_GARANTIZADA_LGBT) break;
      if (pool.has(p.id)) continue;
      pool.set(p.id, { ...p, procedencia: "diversidad" });
      inyectadas++;
    }
  } catch (e) {
    console.warn("[vera/tmdb] fuente LGBT+ falló (best-effort):", e);
  }

  let completo = [...pool.values()];

  // Fallback: si el pool no llega al mínimo, traer SEMILLA_FALLBACK
  // (también filtrada por intent — mejor 5 livianos genuinos que 8 mezclados).
  if (completo.length < MIN_POOL) {
    const yaEnPool = new Set(completo.map((p) => p.id));
    const candidatos = SEMILLA_FALLBACK.filter(
      (id) => !yaEnPool.has(String(id)),
    );
    const fallback = await enriquecerIds(candidatos, apiKey);
    const fallbackCompatible = fallback
      .filter((p) => compatibleConIntencion(p, idsIntencion))
      .map((p) => ({ ...p, procedencia: "fallback" as const }));
    completo = [...completo, ...fallbackCompatible];
  }

  // yaVistas: B6 lo ignora deliberadamente (las vistas aparecen en ranking
  // con badge). Lo dejamos como param sin uso para no romper la firma.
  void yaVistas;

  return completo;
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

// URL de un fotograma extra (path crudo TMDb). Para las miniaturas de la ficha.
export function imageUrl(
  path: string,
  size: "w300" | "w500" | "w780" = "w300",
): string {
  if (!path) return "";
  return `${IMG_BASE}/${size}${path}`;
}
