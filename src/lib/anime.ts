// Constantes del tab Anime (fuente: AniList).
// Los géneros de AniList son strings fijos en inglés; acá viven con su
// etiqueta en español para la UI. El id numérico es local (la UI filtra
// con Set<number> igual que con TMDb) y se traduce a nombre al consultar.

export type AnimeGenre = { id: number; name: string; anilist: string };

export const ANIME_GENRES: AnimeGenre[] = [
  { id: 1, name: "Acción", anilist: "Action" },
  { id: 2, name: "Aventura", anilist: "Adventure" },
  { id: 3, name: "Comedia", anilist: "Comedy" },
  { id: 4, name: "Drama", anilist: "Drama" },
  { id: 5, name: "Fantasía", anilist: "Fantasy" },
  { id: 6, name: "Terror", anilist: "Horror" },
  { id: 7, name: "Mecha", anilist: "Mecha" },
  { id: 8, name: "Música", anilist: "Music" },
  { id: 9, name: "Misterio", anilist: "Mystery" },
  { id: 10, name: "Psicológico", anilist: "Psychological" },
  { id: 11, name: "Romance", anilist: "Romance" },
  { id: 12, name: "Ciencia ficción", anilist: "Sci-Fi" },
  { id: 13, name: "Recuentos de la vida", anilist: "Slice of Life" },
  { id: 14, name: "Deportes", anilist: "Sports" },
  { id: 15, name: "Sobrenatural", anilist: "Supernatural" },
  { id: 16, name: "Suspenso", anilist: "Thriller" },
];

// Orden TMDb (sortId local) → MediaSort de AniList.
export const ANILIST_SORTS: Record<string, string> = {
  popular: "POPULARITY_DESC",
  top: "SCORE_DESC",
  voted: "FAVOURITES_DESC",
  new: "START_DATE_DESC",
  old: "START_DATE",
  az: "TITLE_ENGLISH",
  za: "TITLE_ENGLISH_DESC",
};

/** Set de ids locales seleccionados → CSV de géneros AniList ("Action,Romance"). */
export function anilistGenresCSV(selected: Set<number>): string {
  return ANIME_GENRES.filter((g) => selected.has(g.id))
    .map((g) => g.anilist)
    .join(",");
}
