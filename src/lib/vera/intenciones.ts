// Tabla canónica de intenciones del usuario.
// Cada intent mapea a un FiltrosDiscover concreto que arma el pool en B1.
// Mantiene la UI (orden, label, icono) en un solo lugar para no duplicar.

import type { Intencion } from "./tipos";
import type { FiltrosDiscover } from "./tmdb";

// Para "sorpresa" el sort_by se elige al azar entre estas tres variantes.
// Cada una produce un pool distinto y cacheable por su hash propio.
const SORPRESA_SORTS = [
  "popularity.desc",
  "vote_average.desc",
  "revenue.desc",
] as const;

export interface OpcionIntencion {
  id: Intencion;
  label: string;
  icono: string;
  desc: string;
}

// Orden de aparición en la pantalla. El último (sorpresa) es el "default"
// del atajo ↓ que salta sin tener que navegar.
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
//
// IMPORTANTE: llamar UNA sola vez al confirmar el intent y guardar el
// resultado en estado local. Recalcular cambia el sort_by aleatorio de
// "sorpresa" y rompe la estabilidad de la sesión + la cache (hash distinto).
//
// Separador de géneros: PIPE "|" para que TMDb interprete como OR
// (la peli debe tener AL MENOS UNO de los géneros). Con coma TMDb
// interpreta AND y deja casi todo el pool fuera, sesgando hacia
// nichos que matchean todos los géneros simultáneamente.
export function filtrosParaIntent(intent: Intencion): FiltrosDiscover {
  switch (intent) {
    case "liviano":
      // Recortado de 5 géneros a 3: Comedia (35), Animación (16), Familia (10751).
      // Aventura (12) abría la puerta a Indiana Jones / Mortal Kombat (denso);
      // Romance (10749) abría a dramas románticos pesados. Esos géneros son
      // livianos solo en combinación, no sueltos.
      return {
        with_genres: "35|16|10751",
        sort_by: "popularity.desc",
        vote_count_gte: 100,
        page: 1,
      };
    case "denso":
      return {
        with_genres: "18|99|36|9648",
        sort_by: "vote_average.desc",
        vote_count_gte: 200,
        page: 1,
      };
    case "adrenalina":
      return {
        with_genres: "28|53|27|878|10752",
        sort_by: "popularity.desc",
        vote_count_gte: 100,
        page: 1,
      };
    case "sorpresa":
      return {
        sort_by:
          SORPRESA_SORTS[Math.floor(Math.random() * SORPRESA_SORTS.length)],
        vote_count_gte: 100,
        page: 1,
      };
  }
}
