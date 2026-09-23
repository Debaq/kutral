// Formas de las respuestas del backend que usan varias pantallas. Tienen que
// coincidir con los structs Serialize de src-tauri (anotados encima de cada
// una).

// webserver.rs WebStatus
export type WebStatus = { running: boolean; ip: string | null; port: number | null; url: string | null };

// sistema.rs WifiStatus
export type WifiState = { online: boolean; connected_ssid: string | null };

// rd.rs RdDeviceStart
export type RdDeviceStart = {
  device_code: string;
  user_code: string;
  verification_url: string;
  interval: number;
  expires_in: number;
};

// tmdb.rs PersonMini
export type PersonMini = {
  id: number;
  name: string;
  profile_path?: string;
  character?: string;
  job?: string;
};

// omdb.rs OmdbRating
export type OmdbRating = { source: string; value: string };

// omdb.rs OmdbDetail
export type OmdbDetail = {
  plot?: string | null;
  awards?: string | null;
  rated?: string | null;
  writer?: string | null;
  country?: string | null;
  language?: string | null;
  released?: string | null;
  metascore?: string | null;
  imdb_rating?: string | null;
  imdb_votes?: string | null;
  box_office?: string | null;
  production?: string | null;
  ratings: OmdbRating[];
};

// tmdb.rs TmdbItem (también lo devuelven los listados de AniList)
export type ListItem = {
  id: number;
  title?: string;
  name?: string;
  poster_path?: string;
  overview: string;
  vote_average: number;
  release_date?: string;
  first_air_date?: string;
};

// tmdb.rs TmdbListResp
export type ListResp = { page: number; total_pages: number; results: ListItem[] };

// tmdb.rs SeasonMini
export type SeasonMini = {
  season_number: number;
  episode_count: number;
  name: string;
  air_date: string | null;
  poster_path: string | null;
};

// tmdb.rs TmdbDetail (tmdb_detail y anilist_detail)
export type Detail = {
  id: number;
  media_type: "movie" | "tv";
  title: string;
  overview: string;
  poster_path?: string;
  backdrop_path?: string;
  vote_average: number;
  year: string;
  imdb_id?: string;
  runtime?: number;
  original_title?: string | null;
  genres: string[];
  directors: PersonMini[];
  cast: PersonMini[];
  images?: string[];
  number_of_seasons?: number | null;
  seasons?: SeasonMini[];
  // --- extras anime (solo cuando la fuente es AniList) ---
  is_anime?: boolean;
  mal_id?: number | null;
  kitsu_id?: number | null;
  anidb_id?: number | null;
  trailer_youtube?: string | null;
  format?: string | null;
};

// tmdb.rs EpisodeMini (tmdb_season y anizip_episodes)
export type EpisodeMini = {
  episode_number: number;
  name: string;
  overview: string;
  still_path: string | null;
  air_date: string | null;
  runtime: number | null;
};

// tmdb.rs PersonFilmography
export type PersonFilm = {
  id: number;
  title: string;
  poster_path?: string;
  year: string;
  media_type: string;
  roles: string[];
  vote_average: number;
  popularity: number;
};

// tmdb.rs PersonInfo
export type PersonInfo = {
  id: number;
  name: string;
  biography: string;
  profile_path?: string;
  birthday?: string;
  deathday?: string;
  place_of_birth?: string;
  known_for_department?: string;
  filmography: PersonFilm[];
};
