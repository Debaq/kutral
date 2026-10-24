// Perfil de niños: qué del catálogo es para ellos.
//
// El discover lo filtra en el backend (tmdb.rs `filtro_ninos`, anilist.rs
// `GENEROS_NO_NINOS`). Lo que TMDb no deja filtrar en la consulta se filtra
// aquí, con los géneros que trae cada resultado: la búsqueda (no acepta
// filtros), las series con géneros elegidos por el usuario y las sugerencias.
//
// Por géneros es grueso a propósito: mejor dejar fuera una película apta que
// colar una que no lo es.

// TMDb: 16 animación, 10751 familia, 10762 infantil (solo series).
const FAMILIA = 10751;
const INFANTIL_TV = 10762;
// Terror, suspenso, crimen, guerra, misterio, guerra y política.
const NO_APTOS = [27, 53, 80, 10752, 9648, 10768];

type ConGeneros = { genre_ids?: number[] };

/**
 * ¿Se muestra en un perfil de niños? Películas: tienen que ser familiares.
 * Series: infantiles o familiares. Nada de los géneros no aptos.
 */
export function aptoNinos(it: ConGeneros, mediaType: "movie" | "tv"): boolean {
  const g = it.genre_ids ?? [];
  if (g.some((x) => NO_APTOS.includes(x))) return false;
  return mediaType === "movie" ? g.includes(FAMILIA) : g.includes(FAMILIA) || g.includes(INFANTIL_TV);
}
