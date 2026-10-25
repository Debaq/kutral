// Tabla canónica de intenciones del usuario.
// Cada intent mapea a un conjunto de géneros TMDb, y de ahí salen dos cosas:
//   - los filtros de /discover que arman el pool (filtrosParaIntent)
//   - el tono que el usuario pidió, que el motor respeta (idsDeIntent)
// Mantiene la UI (orden, label, icono) en un solo lugar para no duplicar.

import type { Intencion } from "./tipos";
import type { FiltrosDiscover } from "./tmdb";

// Variedad por aleatoriedad de page + sort_by.
// Sin cache de pool, cada llamada arma filtros nuevos. Eso garantiza que
// entrar a Vera dos veces seguidas con el mismo intent traiga pelis distintas.
//
// PAGE_MAX 5: TMDb tiene cientos de páginas pero las primeras 5 cubren el
// grueso de pelis con vote_count_gte 100+ (no caemos en obscuridad).
const PAGE_MAX = 5;
function pageAleatoria(): number {
  return 1 + Math.floor(Math.random() * PAGE_MAX);
}

// Sorts disponibles. Distintos sort_by traen subconjuntos distintos del mismo
// género, lo que multiplica la variedad.
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

// Géneros TMDb (ids) por intent. UNA sola definición: de acá salen tanto el
// `with_genres` que se le manda a TMDb como el tono que el motor respeta.
// Antes estaban duplicados en dos archivos y se podían desincronizar.
//
// "reir" está recortado a Comedia/Animación/Familia — los que son livianos
// por sí solos. "emocion" (Drama + Romance) suma tono neutro a propósito: un
// drama que emociona es denso y una comedia romántica es liviana, y las dos
// sirven. "pensar" tira a denso.
const GENEROS_DE_INTENT: Record<Intencion, string[]> = {
  reir: ["35", "16", "10751"],
  emocion: ["18", "10749"],
  adrenalina: ["28", "53", "27", "878", "10752"],
  pensar: ["99", "36", "9648", "878", "18"],
  sorpresa: [],
};

// Ids de género que representa un intent. Vacío para "sorpresa" = sin
// restricción (ni de género ni de tono).
export function idsDeIntent(intent: Intencion): Set<string> {
  return new Set(GENEROS_DE_INTENT[intent]);
}

export interface OpcionIntencion {
  id: Intencion;
  label: string;
  icono: string;
  desc: string;
}

export const INTENCIONES: OpcionIntencion[] = [
  { id: "reir", label: "Reírme", icono: "😄", desc: "Comedia, algo liviano" },
  { id: "emocion", label: "Emocionarme", icono: "🥹", desc: "Drama, romance" },
  { id: "adrenalina", label: "Adrenalina", icono: "⚡", desc: "Acción, suspenso" },
  { id: "pensar", label: "Algo para pensar", icono: "🧠", desc: "Misterio, historia, documental" },
  { id: "sorpresa", label: "Sorpréndeme", icono: "🎲", desc: "Lo que sea, pero bueno" },
];

// Resuelve los filtros de discover para un intent dado.
// Cada llamada produce filtros con page y sort_by random — sin cache, cada
// entrada a Vera trae pool fresco. Llamar UNA sola vez al confirmar el intent
// (no recalcular en cada render: los random cambiarían).
//
// Separador de géneros: PIPE "|" = OR en TMDb.
export function filtrosParaIntent(intent: Intencion): FiltrosDiscover {
  const generos = GENEROS_DE_INTENT[intent];
  const base: FiltrosDiscover = {
    sort_by: sortAleatorio(
      intent === "pensar" ? SORTS_DENSO : SORTS_GENERICOS,
    ),
    vote_count_gte: intent === "pensar" ? 200 : 100,
    page: pageAleatoria(),
  };
  if (generos.length > 0) base.with_genres = generos.join("|");
  return base;
}
