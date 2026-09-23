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
