// Tabla canónica de intenciones del usuario.
// Cada intent mapea a un FiltrosDiscover concreto que arma el pool.
// Mantiene la UI (orden, label, icono) en un solo lugar para no duplicar.

import type { Intencion } from "./tipos";
import type { FiltrosDiscover } from "./tmdb";

// B6: variedad por aleatoriedad de page + sort_by.
// Sin cache de pool, cada llamada arma filtros nuevos. Eso garantiza
// que entrar a Vera dos veces seguidas con el mismo intent traiga
// pelis distintas (no repetición).
//
// PAGE_MAX 5: TMDb tiene cientos de páginas pero las primeras 5 cubren
// el grueso de pelis con vote_count_gte 100+ (no caemos en obscuridad).
const PAGE_MAX = 5;
function pageAleatoria(): number {
  return 1 + Math.floor(Math.random() * PAGE_MAX);
}

// Sorts disponibles. Distintos sort_by traen subconjuntos distintos del
// mismo género, lo que multiplica la variedad.
const SORTS_GENERICOS = [
  "popularity.desc",
  "vote_average.desc",
  "revenue.desc",
] as const;
const SORTS_DENSO = [
  "vote_average.desc",
  "popularity.desc",
] as const;
function sortAleatorio<T extends readonly string[]>(opciones: T): T[number] {
  return opciones[Math.floor(Math.random() * opciones.length)];
}

export interface OpcionIntencion {
  id: Intencion;
  label: string;
  icono: string;
  desc: string;
}

export const INTENCIONES: OpcionIntencion[] = [
  {
    id: "liviano",
    label: "Algo liviano",
    icono: "🌤",
    desc: "Reír, distender",
  },
  {
    id: "denso",
    label: "Algo denso",
    icono: "🌑",
    desc: "Pensar, sentir",
  },
  {
    id: "adrenalina",
    label: "Adrenalina",
    icono: "⚡",
    desc: "Ritmo, acción",
  },
  {
    id: "sorpresa",
    label: "Sorpresa",
    icono: "🎲",
    desc: "Tirá lo que quieras",
  },
];

// Resuelve los filtros de discover para un intent dado.
// B6: cada llamada produce filtros con page y sort_by random — sin cache,
// cada entrada a Vera trae pool fresco. Llamar UNA sola vez al confirmar
// el intent (no recalcular en cada render, los random cambiarían).
//
// Separador de géneros: PIPE "|" = OR en TMDb.
export function filtrosParaIntent(intent: Intencion): FiltrosDiscover {
  switch (intent) {
    case "liviano":
      // Géneros recortados a Comedia/Animación/Familia (los que son
      // livianos por sí solos, sin combinación). Aventura y Romance
      // sueltos abren puerta a denso por accidente.
      return {
        with_genres: "35|16|10751",
        sort_by: sortAleatorio(SORTS_GENERICOS),
        vote_count_gte: 100,
        page: pageAleatoria(),
      };
    case "denso":
      return {
        with_genres: "18|99|36|9648",
        sort_by: sortAleatorio(SORTS_DENSO),
        vote_count_gte: 200,
        page: pageAleatoria(),
      };
    case "adrenalina":
      return {
        with_genres: "28|53|27|878|10752",
        sort_by: sortAleatorio(SORTS_GENERICOS),
        vote_count_gte: 100,
        page: pageAleatoria(),
      };
    case "sorpresa":
      return {
        sort_by: sortAleatorio(SORTS_GENERICOS),
        vote_count_gte: 100,
        page: pageAleatoria(),
      };
  }
}
