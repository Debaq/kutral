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
  trending: "TRENDING_DESC",
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

// ========================================================================
// Temporadas (cours): convención japonesa, hemisferio norte.
// WINTER ene-mar, SPRING abr-jun, SUMMER jul-sep, FALL oct-dic.
// ========================================================================

export type AnimeSeasonOpt = {
  id: string; // "SPRING-2026"
  label: string;
  season: string; // enum MediaSeason de AniList
  year: number;
};

const SEASON_ORDER = ["WINTER", "SPRING", "SUMMER", "FALL"] as const;
const SEASON_ES: Record<string, string> = {
  WINTER: "Invierno",
  SPRING: "Primavera",
  SUMMER: "Verano",
  FALL: "Otoño",
};

/**
 * Opciones de temporada para el dropdown: próxima, actual y `past`
 * anteriores, de más nueva a más vieja.
 */
export function animeSeasonOptions(now: Date = new Date(), past = 6): AnimeSeasonOpt[] {
  // Índice lineal año*4+cour para iterar cruzando años sin casos borde.
  const base = now.getFullYear() * 4 + Math.floor(now.getMonth() / 3);
  const out: AnimeSeasonOpt[] = [];
  for (let i = base + 1; i >= base - past; i--) {
    const year = Math.floor(i / 4);
    const season = SEASON_ORDER[i % 4];
    const extra = i === base ? " · actual" : i === base + 1 ? " · próxima" : "";
    out.push({
      id: `${season}-${year}`,
      label: `${SEASON_ES[season]} ${year}${extra}`,
      season,
      year,
    });
  }
  return out;
}
