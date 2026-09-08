// Lectura de `vera_titles` — el catálogo local que puebla /vera/catalog.
//
// La tabla existía desde la migración 3 y NADIE la leía: el importador
// escribía imdb_id, temas sensibles y plataformas por título, y el flujo de
// Vera igual salía a TMDb en vivo e ignoraba todo eso.
//
// Dos usos, distintos:
//
//   1. ANOTAR (anotarConCatalogo) — el camino normal. El pool se arma con
//      TMDb en vivo y acá le pegamos, por tmdb_id, los datos que TMDb no da
//      barato: temas sensibles verificados y en qué plataformas está. Es un
//      SELECT con IN (...), no toca la red.
//
//   2. SEMBRAR (candidatosLocales) — el camino degradado. Si TMDb falla o no
//      hay API key pero el catálogo está importado, Vera propone igual desde
//      lo local. Sin sinopsis ni reparto (la tabla no los guarda), pero con
//      título, año, géneros y duración: alcanza para rankear y ofrecer.

import { getDb, jsonArray } from "./db";
import { generoPorSlug, tonoDeGeneros, esFamilyFriendly } from "./generos";
import type { Pelicula } from "./tipos";

export interface AnotacionLocal {
  imdbId: string | null;
  temasSensibles: string[];
  plataformas: string[];
  runtime: number | null;
  idioma: string;
}

interface FilaTitulo {
  imdb_id: string;
  languages: string;
  tmdb_id: number | null;
  title: string;
  year: number | null;
  runtime_min: number | null;
  genres: string;
  sensitive_themes: string;
  platforms: string;
  popularity: number | null;
}

// SQLite tiene un tope de parámetros por statement (999 por default). El pool
// de Vera nunca pasa de ~60 títulos, pero troceamos igual: un cambio de
// tamaño de pool no debería reventar una consulta silenciosamente.
const LOTE_IN = 400;

// Anota un lote de pelis con lo que sepa el catálogo local.
// Devuelve un Map por id TMDb (string, igual que Pelicula.id).
// Map vacío = catálogo sin importar o DB caída. No es error.
export async function anotarConCatalogo(
  idsTmdb: string[],
): Promise<Map<string, AnotacionLocal>> {
  const out = new Map<string, AnotacionLocal>();
  if (idsTmdb.length === 0) return out;
  const db = await getDb();
  if (!db) return out;

  for (let i = 0; i < idsTmdb.length; i += LOTE_IN) {
    const lote = idsTmdb.slice(i, i + LOTE_IN);
    // Placeholders posicionales ($1, $2, …) — el plugin-sql de Tauri no acepta
    // arrays como un solo parámetro, así que se arma el IN a mano. Los valores
    // siguen yendo bindeados, no interpolados.
    const ph = lote.map((_, j) => `$${j + 1}`).join(",");
    try {
      const filas = await db.select<FilaTitulo[]>(
        `SELECT imdb_id, tmdb_id, runtime_min, sensitive_themes, platforms,
                languages
           FROM vera_titles
          WHERE tmdb_id IN (${ph})`,
        lote.map((s) => Number(s)),
      );
      for (const f of filas) {
        if (f.tmdb_id === null) continue;
        out.set(String(f.tmdb_id), {
          imdbId: f.imdb_id || null,
          temasSensibles: jsonArray(f.sensitive_themes),
          plataformas: jsonArray(f.platforms),
          runtime: f.runtime_min,
          idioma: jsonArray(f.languages)[0] ?? "",
        });
      }
    } catch (e) {
      console.warn("[vera/catalogoLocal] anotar falló:", e);
      return out;
    }
  }
  return out;
}

export interface FiltrosLocales {
  // Slugs v3 que el pool quiere (del intent). Vacío = cualquiera.
  generos: string[];
  // Exclusiones duras del perfil.
  generosExcluidos: string[];
  temasExcluidos: string[];
  // Si el perfil declara plataformas, solo títulos que estén en alguna.
  // Vacío = sin filtro de plataforma.
  plataformas: string[];
  limite: number;
}

// Convierte una fila de vera_titles en una Pelicula parcial.
// `enriquecida: false` es la señal para el resto del flujo de que faltan
// sinopsis, reparto y backdrop — la UI los pide después con tmdb_detail.
function filaAPelicula(f: FilaTitulo): Pelicula | null {
  if (f.tmdb_id === null) return null;
  const nombres = jsonArray(f.genres)
    .map((s) => generoPorSlug(s)?.nombre)
    .filter((n): n is string => n !== undefined);
  return {
    id: String(f.tmdb_id),
    titulo: f.title,
    generos: nombres,
    tono: tonoDeGeneros(nombres),
    familyFriendly: esFamilyFriendly(nombres),
    // vera_titles no guarda vote_average. 0 votos hace que el rating
    // bayesiano del motor caiga entero a la media global, que es exactamente
    // lo correcto: no sabemos nada de su calidad.
    rating: 0,
    votos: 0,
    popularidad: f.popularity ?? 0,
    pais: "",
    // El importador guarda original_language como array de un elemento.
    idiomaOriginal: jsonArray(f.languages)[0] ?? "",
    poster: "#1f3a4d",
    gancho: "",
    descripcion: "",
    director: "",
    actores: [],
    anio: f.year === null ? "" : String(f.year),
    runtime: f.runtime_min,
    posterPath: null,
    backdropPath: null,
    imagenes: [],
    trivia: "",
    imdbId: f.imdb_id || null,
    temasSensibles: jsonArray(f.sensitive_themes),
    plataformas: jsonArray(f.platforms),
    enriquecida: false,
    procedencia: "local",
  };
}

// Candidatos desde el catálogo local, ya filtrados por el perfil.
// Devuelve [] si no hay DB o el catálogo está vacío — quien llama decide.
//
// El filtro de géneros/temas se hace en JS y no en SQL a propósito: las
// columnas guardan JSON arrays y un LIKE '%"drama"%' sobre TEXT es a la vez
// frágil (matchea subcadenas) y no indexable. Se traen filas por popularidad
// y se filtran en memoria; el catálogo son miles de filas, no millones.
export async function candidatosLocales(
  f: FiltrosLocales,
): Promise<Pelicula[]> {
  const db = await getDb();
  if (!db) return [];

  const generosOk = new Set(f.generos);
  const generosNo = new Set(f.generosExcluidos);
  const temasNo = new Set(f.temasExcluidos);
  const platsOk = new Set(f.plataformas);

  try {
    // Se leen hasta 20x el límite pedido porque el filtrado real ocurre
    // después, en memoria: pedir justo `limite` dejaría el pool corto apenas
    // el perfil excluya algo.
    // Sin filtro de duración: igual que en el camino de TMDb, el tope del
    // perfil es preferencia y la aplica el motor penalizando, no excluyendo.
    const filas = await db.select<FilaTitulo[]>(
      `SELECT imdb_id, tmdb_id, title, year, runtime_min, genres,
              sensitive_themes, platforms, popularity, languages
         FROM vera_titles
        WHERE format = 'movie'
          AND tmdb_id IS NOT NULL
        ORDER BY popularity DESC
        LIMIT $1`,
      [f.limite * 20],
    );

    const out: Pelicula[] = [];
    for (const fila of filas) {
      const slugs = jsonArray(fila.genres);
      if (slugs.some((g) => generosNo.has(g))) continue;
      if (generosOk.size > 0 && !slugs.some((g) => generosOk.has(g))) continue;
      const temas = jsonArray(fila.sensitive_themes);
      if (temas.some((t) => temasNo.has(t))) continue;
      if (platsOk.size > 0) {
        const plats = jsonArray(fila.platforms);
        if (!plats.some((p) => platsOk.has(p))) continue;
      }
      const p = filaAPelicula(fila);
      if (p) out.push(p);
      if (out.length >= f.limite) break;
    }
    return out;
  } catch (e) {
    console.warn("[vera/catalogoLocal] candidatos falló:", e);
    return [];
  }
}

// Cuántas películas hay importadas. La UI la usa para decidir si el camino
// local es viable y para explicar por qué no lo es.
export async function contarPeliculasLocales(): Promise<number> {
  const db = await getDb();
  if (!db) return 0;
  try {
    const filas = await db.select<{ n: number }[]>(
      `SELECT COUNT(*) AS n FROM vera_titles WHERE format = 'movie'`,
    );
    return filas[0]?.n ?? 0;
  } catch {
    return 0;
  }
}
