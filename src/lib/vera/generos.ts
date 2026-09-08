// Tabla canónica de géneros. UNA sola fuente para los tres vocabularios que
// conviven en Kütral:
//
//   id TMDb ("18")  ←→  nombre es-ES ("Drama")  ←→  slug v3 ("drama")
//
// El nombre es-ES es lo que devuelve tmdb_detail (LANG=es-ES) y lo que se
// muestra en pantalla. El slug v3 es lo que guarda `vera_titles.genres` y lo
// que devuelve `vera_genre_list` (ver map_genre_id en src-tauri/src/lib.rs).
// Sin esta tabla, filtrar el catálogo local por lo que el usuario excluyó en
// el setup exigiría comparar vocabularios distintos a mano en cada consumidor.

export interface GeneroCanonico {
  id: string;      // id numérico TMDb, como string
  nombre: string;  // nombre es-ES de TMDb
  slug: string;    // slug v3 (vera_genre_list / vera_titles.genres)
  // Aporte al tono. +N tira a liviano, -N tira a denso, 0 neutro.
  // Reemplaza los sets binarios: una peli "Comedia + Crimen + Acción" no es
  // liviana solo porque tenga Comedia entre sus etiquetas.
  peso: number;
}

export const GENEROS: GeneroCanonico[] = [
  { id: "28",    nombre: "Acción",          slug: "action",      peso: -2 },
  { id: "12",    nombre: "Aventura",        slug: "adventure",   peso: 1 },
  { id: "16",    nombre: "Animación",       slug: "animation",   peso: 2 },
  { id: "35",    nombre: "Comedia",         slug: "comedy",      peso: 2 },
  { id: "80",    nombre: "Crimen",          slug: "crime",       peso: -2 },
  { id: "99",    nombre: "Documental",      slug: "documentary", peso: -1 },
  { id: "18",    nombre: "Drama",           slug: "drama",       peso: -1 },
  { id: "10751", nombre: "Familia",         slug: "family",      peso: 2 },
  { id: "14",    nombre: "Fantasía",        slug: "fantasy",     peso: 1 },
  { id: "36",    nombre: "Historia",        slug: "historical",  peso: -1 },
  { id: "27",    nombre: "Terror",          slug: "horror",      peso: -3 },
  { id: "10402", nombre: "Música",          slug: "musical",     peso: 1 },
  { id: "9648",  nombre: "Misterio",        slug: "mystery",     peso: -1 },
  { id: "10749", nombre: "Romance",         slug: "romance",     peso: 1 },
  { id: "878",   nombre: "Ciencia ficción", slug: "scifi",       peso: -1 },
  { id: "53",    nombre: "Suspense",        slug: "thriller",    peso: -2 },
  { id: "10752", nombre: "Bélica",          slug: "war",         peso: -3 },
  { id: "37",    nombre: "Western",         slug: "western",     peso: -1 },
  // Sin equivalente en vera_genre_list: nunca se excluye desde el setup.
  { id: "10770", nombre: "Película de TV",  slug: "",            peso: 0 },
];

const PorId = new Map(GENEROS.map((g) => [g.id, g]));
const PorNombre = new Map(GENEROS.map((g) => [g.nombre, g]));
const PorSlug = new Map(
  GENEROS.filter((g) => g.slug !== "").map((g) => [g.slug, g]),
);

export const generoPorId = (id: string): GeneroCanonico | undefined =>
  PorId.get(id);
export const generoPorNombre = (n: string): GeneroCanonico | undefined =>
  PorNombre.get(n);
export const generoPorSlug = (s: string): GeneroCanonico | undefined =>
  PorSlug.get(s);

// ids TMDb → nombres es-ES. Para mapear `genre_ids` del listado de discover
// (que viene con ids, no con nombres) al mismo vocabulario que usa el detail.
export function nombresDesdeIds(ids: number[]): string[] {
  const out: string[] = [];
  for (const id of ids) {
    const g = PorId.get(String(id));
    if (g && !out.includes(g.nombre)) out.push(g.nombre);
  }
  return out;
}

// nombres es-ES → slugs v3. Para cruzar una peli de TMDb con las exclusiones
// de género del perfil, que están guardadas en slugs.
export function slugsDesdeNombres(nombres: string[]): string[] {
  const out: string[] = [];
  for (const n of nombres) {
    const g = PorNombre.get(n);
    if (g && g.slug !== "" && !out.includes(g.slug)) out.push(g.slug);
  }
  return out;
}

// Tono ponderado de un conjunto de géneros (nombres es-ES).
// Empate (0) cae en liviano: sin señal fuerte, no castigamos.
export function tonoDeGeneros(nombres: string[]): "liviano" | "denso" {
  let s = 0;
  for (const n of nombres) s += PorNombre.get(n)?.peso ?? 0;
  return s >= 0 ? "liviano" : "denso";
}

// Tono esperado de un intent, derivado del signo agregado de sus géneros.
// Intent sin géneros (sorpresa) → null = sin restricción de tono.
export function tonoDeIds(ids: Set<string>): "liviano" | "denso" | null {
  if (ids.size === 0) return null;
  let s = 0;
  for (const id of ids) s += PorId.get(id)?.peso ?? 0;
  if (s === 0) return null;
  return s > 0 ? "liviano" : "denso";
}

// familyFriendly conservador: solo true si hay género familiar/animación y
// ninguno adulto-ish. Sin señal → false.
const FAMILY_OK = new Set(["Familia", "Animación"]);
const NO_FAMILY = new Set([
  "Terror",
  "Suspense",
  "Crimen",
  "Bélica",
  "Misterio",
]);

export function esFamilyFriendly(nombres: string[]): boolean {
  if (nombres.some((g) => NO_FAMILY.has(g))) return false;
  return nombres.some((g) => FAMILY_OK.has(g));
}
