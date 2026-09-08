// Cliente TMDb para el catálogo de Vera.
// Lee la API key de localStorage (la setea el usuario en /vera/catalog o en home).
// Llama a comandos Tauri en src-tauri/src/lib.rs:
//   - tmdb_discover        (extendido con filtros opcionales)
//   - tmdb_recommendations
//   - tmdb_detail          (enriquecimiento perezoso)
//
// CÓMO SE ARMA EL POOL (y por qué cambió)
//
// Antes: cada fuente traía 20 títulos y a cada uno se le pedía /detail para
// poder mapearlo. Tres fuentes = ~60 requests serializados por ronda, todos
// antes de pintar la primera carta. Ese era el "Buscando lo que pediste…" largo.
//
// Ahora: /discover y /recommendations ya devuelven título, sinopsis, póster,
// vote_average, vote_count, popularity y genre_ids — todo lo que el motor
// necesita para rankear. El pool se arma con eso, en 3 requests paralelos, y
// /detail queda para después: solo las pelis que el usuario va a MIRAR (las
// cartas de calibración y la ficha activa del ranking) se enriquecen, en lotes
// chicos y en segundo plano. `Pelicula.enriquecida` dice en qué estado está.

import { invoke } from "@tauri-apps/api/core";
import type { Pelicula, Tono, PerfilSetup } from "./tipos";
import { SEMILLA_FALLBACK } from "./probes";
import { getSemillas } from "./historial";
import {
  GENEROS,
  nombresDesdeIds,
  slugsDesdeNombres,
  tonoDeGeneros,
  tonoDeIds,
  esFamilyFriendly,
  generoPorNombre,
} from "./generos";
import {
  anotarConCatalogo,
  candidatosLocales,
  contarPeliculasLocales,
} from "./catalogoLocal";
import { LOTE_ENRIQUECIMIENTO } from "./config";

// Cache de géneros por tmdb_id en localStorage.
// Sin TTL — los géneros de una peli no cambian. La pobla el mapeo cada vez que
// una peli entra al pool, y la consume historial.getPerfilHistorico().
// Permite tener perfil histórico real sin pegarle de nuevo a TMDb.
const CACHE_GENEROS = "vera_generos_cache";

function cacheGenerosDe(tmdbId: number | string, generos: string[]): void {
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

// Lo que devuelven tmdb_discover / tmdb_recommendations por título.
// genre_ids, vote_count y popularity se agregaron a TmdbItem en Rust para
// poder armar el pool sin un /detail por peli.
interface TmdbItemMini {
  id: number;
  title?: string;
  name?: string;
  poster_path?: string | null;
  overview?: string;
  vote_average?: number;
  release_date?: string;
  genre_ids?: number[];
  vote_count?: number | null;
  popularity?: number | null;
  original_language?: string | null;
}
interface TmdbListResp {
  page: number;
  total_pages: number;
  results: TmdbItemMini[];
}

// Caches viejas del pool. Se purgan al iniciar Vera. NO cachear pools nuevos:
// se prioriza variedad ("cada entrada a Vera = pelis nuevas").
const CACHE_VIEJA_V2 = "vera_catalogo_v2";
const CACHE_VIEJA_PREFIX = "vera_pool_";

// Keyword TMDb sombrilla para representación LGBT+.
// Garantiza presencia mínima en cada pool, sin importar el intent.
// Sin marcar, sin etiquetar, sin sección aparte — solo presencia mezclada.
const KEYWORD_LGBT = "158718";
const PRESENCIA_GARANTIZADA_LGBT = 1;

// Mínimo de pelis para que el pool sea usable. Por debajo se activan los
// caminos de respaldo (catálogo local y después SEMILLA_FALLBACK).
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
  vote_count?: number | null;
  popularity?: number | null;
  production_countries?: string[];
  original_language?: string | null;
  sensitive_themes?: string[];
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
  page?: number;                     // default 1
}

// Verificación de boot: caza desincronización entre la tabla canónica de
// géneros y lo que TMDb devuelve. Costo 0, corre solo al cargar el módulo.
{
  const vistos = new Set<string>();
  for (const g of GENEROS) {
    if (vistos.has(g.id)) {
      console.warn("[vera/tmdb] id de género duplicado en GENEROS:", g.id);
    }
    vistos.add(g.id);
  }
}

// Decide si una peli es compatible con el intent.
// Dos condiciones (ambas deben cumplirse):
//   (a) al menos un género en común con el intent.
//   (b) el tono de la peli coincide con el tono esperado del intent.
// Sin intent (sorpresa, idsIntencion vacío) acepta todo.
function compatibleConIntencion(
  p: Pelicula,
  idsIntencion: Set<string>,
): boolean {
  if (idsIntencion.size === 0) return true;

  // (a) Matcheo de género.
  let matchGenero = false;
  for (const g of p.generos) {
    const id = generoPorNombre(g)?.id;
    if (id === undefined) {
      console.warn(
        "[vera/tmdb] género TMDb sin id canónico (revisar generos.ts):",
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

  // (b) Matcheo de tono. Una peli "Comedia + Crimen + Acción" matchea por
  // Comedia pero su tono ponderado es denso → no la queremos en liviano.
  const tonoReq = tonoDeIds(idsIntencion);
  if (tonoReq !== null && p.tono !== tonoReq) return false;

  return true;
}

// Una trivia chica, generada por reglas. No es "trivia" real (eso necesita IA
// o DB específica como IMDb). Es un resumen factual con sabor a guiño.
// Determinista por id: la misma peli muestra siempre la misma frase, así no
// cambia sola entre renders.
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
  return opts[d.id % opts.length];
}

// Primera oración del overview, como gancho corto.
function ganchoDe(overview: string): string {
  if (!overview) return "";
  const punto = overview.indexOf(". ");
  if (punto > 30 && punto < 140) return overview.slice(0, punto + 1);
  if (overview.length <= 140) return overview;
  return overview.slice(0, 137) + "…";
}

// --- Mapeo ---

// Peli a partir de un item de listado (discover / recommendations).
// SIN request extra. Queda `enriquecida: false`: falta director, reparto,
// duración, imdb_id, backdrops y temas sensibles.
function mapearItem(it: TmdbItemMini): Pelicula {
  const generos = nombresDesdeIds(it.genre_ids ?? []);
  cacheGenerosDe(it.id, generos);
  const overview = it.overview ?? "";
  const anio = (it.release_date ?? "").split("-")[0] ?? "";
  return {
    id: String(it.id),
    titulo: it.title ?? it.name ?? "",
    generos,
    tono: tonoDeGeneros(generos),
    familyFriendly: esFamilyFriendly(generos),
    rating: it.vote_average ?? 0,
    votos: it.vote_count ?? 0,
    popularidad: it.popularity ?? 0,
    pais: "",
    idiomaOriginal: it.original_language ?? "",
    poster: "#1f3a4d",
    gancho: ganchoDe(overview),
    descripcion: overview,
    director: "",
    actores: [],
    anio,
    runtime: null,
    posterPath: it.poster_path ?? null,
    backdropPath: null,
    imagenes: [],
    trivia: "",
    imdbId: null,
    temasSensibles: [],
    plataformas: [],
    enriquecida: false,
    procedencia: "discover",
  };
}

// Completa una peli con los datos del detail. Preserva `procedencia` y las
// plataformas que ya haya puesto el catálogo local (el detail no las trae).
function fusionarDetalle(base: Pelicula, d: TmdbDetail): Pelicula {
  const generos = d.genres ?? [];
  cacheGenerosDe(d.id, generos);
  const tono: Tono = tonoDeGeneros(generos);
  return {
    ...base,
    titulo: d.title || base.titulo,
    generos,
    tono,
    familyFriendly: esFamilyFriendly(generos),
    rating: d.vote_average ?? base.rating,
    votos: d.vote_count ?? base.votos,
    popularidad: d.popularity ?? base.popularidad,
    pais: d.production_countries?.[0] ?? base.pais,
    idiomaOriginal: d.original_language ?? base.idiomaOriginal,
    gancho: ganchoDe(d.overview) || base.gancho,
    descripcion: d.overview || base.descripcion,
    director: d.directors[0]?.name ?? "",
    actores: d.cast.slice(0, 4).map((c) => c.name),
    anio: d.year || base.anio,
    runtime: d.runtime,
    posterPath: d.poster_path ?? base.posterPath,
    backdropPath: d.backdrop_path,
    imagenes: d.images ?? [],
    trivia: generarTrivia(d),
    imdbId: d.imdb_id,
    // El detail manda sobre el catálogo local: se acaba de calcular con las
    // keywords vigentes, mientras que lo local puede ser de una importación
    // vieja. Solo se conserva lo local si el detail no trajo nada.
    temasSensibles: d.sensitive_themes?.length
      ? d.sensitive_themes
      : base.temasSensibles,
    enriquecida: true,
  };
}

// --- Purga de cache vieja (ya no cacheamos pools) ---

// One-shot al inicio de cada construirPool: barre claves de cache previas que
// se acumulan en localStorage. Idempotente.
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

function apiKey(): string {
  return localStorage.getItem("tmdb_key") ?? "";
}

async function discoverItems(
  filtros: FiltrosDiscover,
  key: string,
): Promise<TmdbItemMini[]> {
  const resp = await invoke<TmdbListResp>("tmdb_discover", {
    mediaType: "movie",
    page: filtros.page ?? 1,
    apiKey: key,
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
  key: string,
  kind: "recommendations" | "similar" = "recommendations",
  page = 1,
): Promise<TmdbItemMini[]> {
  const resp = await invoke<TmdbListResp>("tmdb_recommendations", {
    mediaType: "movie",
    id: Number(seedId),
    page,
    apiKey: key,
    kind,
  });
  return resp.results ?? [];
}

// Cache de detalles por id, viva mientras dure la sesión. Evita repetir el
// /detail de la misma peli cuando reaparece en otra ronda (pasa seguido:
// "otra ronda" con el mismo intent trae solapamiento).
const cacheDetalle = new Map<string, TmdbDetail>();

async function detalleDe(id: string, key: string): Promise<TmdbDetail | null> {
  const hit = cacheDetalle.get(id);
  if (hit) return hit;
  try {
    const d = await invoke<TmdbDetail>("tmdb_detail", {
      mediaType: "movie",
      id: Number(id),
      apiKey: key,
    });
    cacheDetalle.set(id, d);
    return d;
  } catch (e) {
    console.warn("[vera/tmdb] no se pudo enriquecer", id, e);
    return null;
  }
}

// --- API pública ---

export class TmdbNoKeyError extends Error {
  constructor() {
    super(
      "Falta API key TMDb. Configúrala desde la pantalla principal o desde /vera/catalog.",
    );
  }
}

// Enriquece un lote de pelis. Devuelve un Map id → peli completa; las que
// fallaron simplemente no están en el Map (quien llama conserva la parcial).
//
// Va de a LOTE_ENRIQUECIMIENTO en paralelo: los 20 de una no son más rápidos
// (TMDb responde 429) y bloquean más tiempo la primera carta.
export async function enriquecer(
  pelis: Pelicula[],
): Promise<Map<string, Pelicula>> {
  const out = new Map<string, Pelicula>();
  const key = apiKey();
  if (!key) return out;
  const pendientes = pelis.filter((p) => !p.enriquecida);
  for (let i = 0; i < pendientes.length; i += LOTE_ENRIQUECIMIENTO) {
    const lote = pendientes.slice(i, i + LOTE_ENRIQUECIMIENTO);
    const detalles = await Promise.all(
      lote.map((p) => detalleDe(p.id, key)),
    );
    lote.forEach((p, j) => {
      const d = detalles[j];
      if (d) out.set(p.id, fusionarDetalle(p, d));
    });
  }
  return out;
}

// Pelis del listado, sin enriquecer. Un request.
export async function fetchDiscover(
  filtros: FiltrosDiscover,
): Promise<Pelicula[]> {
  const key = apiKey();
  if (!key) throw new TmdbNoKeyError();
  const items = await discoverItems(filtros, key);
  return items.map(mapearItem);
}

export async function fetchRecommendations(
  seedIds: string[],
  kind: "recommendations" | "similar" = "recommendations",
): Promise<Pelicula[]> {
  const key = apiKey();
  if (!key) throw new TmdbNoKeyError();
  // Las semillas van en paralelo: son independientes y el for secuencial
  // anterior sumaba una latencia completa por cada una.
  const tandas = await Promise.all(
    seedIds.slice(0, 3).map((seed) =>
      recommendationsItems(seed, key, kind, 1).catch((e) => {
        console.warn("[vera/tmdb] reco falló para", seed, e);
        return [] as TmdbItemMini[];
      }),
    ),
  );
  const dedup = new Map<number, TmdbItemMini>();
  for (const it of tandas.flat()) if (!dedup.has(it.id)) dedup.set(it.id, it);
  return [...dedup.values()].map(mapearItem);
}

// ¿Esta peli choca con lo que el usuario pidió no ver?
// Devuelve el motivo (para poder explicarlo) o null si está limpia.
//
// Los temas solo se pueden juzgar cuando la peli está enriquecida: en una peli
// del listado `temasSensibles: []` significa "todavía no miramos", no "no
// tiene". Por eso el chequeo de temas se aplica recién al enriquecer, y la UI
// descarta ahí lo que aparezca.
export function motivoExclusion(
  p: Pelicula,
  setup: PerfilSetup | null,
): string | null {
  if (!setup) return null;
  if (setup.generosExcluidos.length > 0) {
    const slugs = slugsDesdeNombres(p.generos);
    const choque = slugs.find((s) => setup.generosExcluidos.includes(s));
    if (choque) return `genero:${choque}`;
  }
  if (p.enriquecida && setup.temasExcluidos.length > 0) {
    const choque = p.temasSensibles.find((t) =>
      setup.temasExcluidos.includes(t),
    );
    if (choque) return `tema:${choque}`;
  }
  if (setup.animacion === "no" && p.generos.includes("Animación")) {
    return "animacion";
  }
  // Idioma que la persona no tolera escuchar. Solo excluye si sabemos cuál es:
  // idiomaOriginal vacío significa "no lo sabemos", no "es uno de esos".
  if (
    p.idiomaOriginal.length > 0 &&
    setup.idiomasEvitados.includes(p.idiomaOriginal)
  ) {
    return `idioma:${p.idiomaOriginal}`;
  }
  // La duración NO se chequea acá a propósito: el tope del perfil es una
  // preferencia, no una prohibición. Lo que pasa el tope baja en el ranking
  // (penalDuracion en motor.ts) pero sigue disponible, que es lo que promete
  // el texto del setup. Excluirlo acá lo castigaría dos veces.
  return null;
}

// Construye el pool. SIN cache de pool: cada llamada = pool fresco desde TMDb.
// La variedad entre sesiones se garantiza con page+sort_by aleatorios en
// intenciones.ts.
//
// Orden de fuentes y respaldo:
//   discover (intent puro) + recommendations (historial) + LGBT+ garantizado
//   → si no llega al mínimo: catálogo local importado
//   → si sigue sin llegar: SEMILLA_FALLBACK
export async function construirPool(
  filtros: FiltrosDiscover,
  semillas: string[],
  setup: PerfilSetup | null,
): Promise<Pelicula[]> {
  // Limpieza one-shot de caches viejos acumulados.
  purgarCachesViejas();

  // Filtros del perfil que valen para los dos caminos (TMDb y local).
  const idsIntencion = new Set(
    (filtros.with_genres ?? "").split(/[,|]/).filter(Boolean),
  );
  const filtrosLocales = {
    generos: [...idsIntencion]
      .map((id) => GENEROS.find((g) => g.id === id)?.slug ?? "")
      .filter(Boolean),
    generosExcluidos: setup?.generosExcluidos ?? [],
    temasExcluidos: setup?.temasExcluidos ?? [],
    plataformas: setup?.plataformas ?? [],
    limite: MIN_POOL * 4,
  };

  const key = apiKey();
  if (!key) {
    // Sin API key, pero con catálogo importado, Vera igual puede proponer.
    // Antes esto era un error seco aunque hubiera miles de títulos locales.
    if ((await contarPeliculasLocales()) >= MIN_POOL) {
      const locales = await candidatosLocales(filtrosLocales);
      if (locales.length >= MIN_POOL) return locales;
    }
    throw new TmdbNoKeyError();
  }

  // Las tres fuentes salen juntas. Antes la de LGBT+ esperaba a que
  // terminaran las otras dos, sumando un round-trip completo de gratis.
  const [disc, reco, lgbt] = await Promise.all([
    fetchDiscover(filtros).catch(() => [] as Pelicula[]),
    semillas.length > 0
      ? fetchRecommendations(semillas).catch(() => [] as Pelicula[])
      : Promise.resolve([] as Pelicula[]),
    fetchDiscover({
      with_keywords: KEYWORD_LGBT,
      sort_by: "popularity.desc",
      vote_count_gte: 50,
      page: 1,
    }).catch((e) => {
      console.warn("[vera/tmdb] fuente LGBT+ falló (best-effort):", e);
      return [] as Pelicula[];
    }),
  ]);

  // Filtrar recommendations a las compatibles con el intent.
  // Decisión de producto: el intent específico manda sobre el historial. Sin
  // esto, recommendations del historial pisa al intent ("pedí comedia, me
  // trajo crimen"). Para sorpresa (idsIntencion vacío) acepta todo.
  // Split defensivo coma O pipe (intenciones.ts usa pipe) — ver idsIntencion
  // más arriba, que se calcula antes porque el camino sin API key también lo
  // necesita.
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

  // Presencia LGBT+ garantizada. Va SIN cruzar con el género del intent (la
  // intersección sería casi vacía) y por eso tampoco pasa por
  // compatibleConIntencion.
  let inyectadas = 0;
  for (const p of lgbt) {
    if (inyectadas >= PRESENCIA_GARANTIZADA_LGBT) break;
    if (pool.has(p.id)) continue;
    pool.set(p.id, { ...p, procedencia: "diversidad" });
    inyectadas++;
  }

  // Exclusiones del perfil que se pueden evaluar ya (género y duración).
  // Los temas quedan para el enriquecimiento — ver motivoExclusion.
  let completo = [...pool.values()].filter(
    (p) => motivoExclusion(p, setup) === null,
  );

  // Respaldo 1: catálogo local importado. Ya viene filtrado por el perfil.
  if (completo.length < MIN_POOL) {
    const yaEnPool = new Set(completo.map((p) => p.id));
    const locales = await candidatosLocales(filtrosLocales);
    for (const p of locales) {
      if (!yaEnPool.has(p.id)) completo.push(p);
    }
  }

  // Respaldo 2: semilla hand-picked. Último recurso.
  if (completo.length < MIN_POOL) {
    const yaEnPool = new Set(completo.map((p) => p.id));
    const faltantes = SEMILLA_FALLBACK.filter(
      (id) => !yaEnPool.has(String(id)),
    );
    const detalles = await Promise.all(
      faltantes.map((id) => detalleDe(String(id), key)),
    );
    for (const d of detalles) {
      if (!d) continue;
      const base = mapearItem({ id: d.id });
      const p = { ...fusionarDetalle(base, d), procedencia: "fallback" as const };
      if (!compatibleConIntencion(p, idsIntencion)) continue;
      if (motivoExclusion(p, setup) !== null) continue;
      completo.push(p);
    }
  }

  // Anotación con el catálogo local: temas sensibles verificados y plataformas.
  // Un SELECT, sin red. Solo rellena lo que falta — el detail, cuando llegue,
  // pisa los temas con el cálculo fresco.
  const anotaciones = await anotarConCatalogo(completo.map((p) => p.id));
  if (anotaciones.size > 0) {
    completo = completo.map((p) => {
      const a = anotaciones.get(p.id);
      if (!a) return p;
      return {
        ...p,
        imdbId: p.imdbId ?? a.imdbId,
        runtime: p.runtime ?? a.runtime,
        idiomaOriginal: p.idiomaOriginal || a.idioma,
        plataformas: p.plataformas.length ? p.plataformas : a.plataformas,
        temasSensibles: p.temasSensibles.length
          ? p.temasSensibles
          : a.temasSensibles,
      };
    });
    // Con las anotaciones puestas pueden aparecer choques que antes no eran
    // visibles: un tema sensible del catálogo, o el idioma original.
    completo = completo.filter((p) => {
      if (!setup) return true;
      if (p.temasSensibles.some((t) => setup.temasExcluidos.includes(t))) {
        return false;
      }
      return !(
        p.idiomaOriginal.length > 0 &&
        setup.idiomasEvitados.includes(p.idiomaOriginal)
      );
    });
  }

  return completo;
}

// Carga el pool para un intent ya resuelto a filtros.
// El caller computa los filtros UNA vez con filtrosParaIntent(intent) y los
// pasa acá. No recalcular acá adentro porque el sort_by de "sorpresa" es
// aleatorio y debe quedar fijo durante la ronda.
export async function cargarCatalogoPorIntent(
  filtros: FiltrosDiscover,
  setup: PerfilSetup | null,
): Promise<Pelicula[]> {
  const semillas = await getSemillas(3);
  return construirPool(filtros, semillas, setup);
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
