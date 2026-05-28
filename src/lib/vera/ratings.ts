// Almacén persistente de puntuaciones por película.
// Vive en localStorage, sobrevive entre sesiones — Vera empieza a "conocer" al usuario.
// Sin sincronización con servidor todavía.

const STORAGE_KEY = "vera_ratings_v1";

export interface RatingRec {
  tmdb: number;        // 0-10 de TMDb (referencia objetiva)
  user: number | null; // 1-5 estrellas del usuario; null si no puntuó
  visto: boolean;      // true si la marcó como vista alguna vez
}

function leer(): Record<string, RatingRec> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as Record<string, RatingRec>;
  } catch {
    return {};
  }
}

export function getRating(peliId: string): RatingRec | undefined {
  return leer()[peliId];
}

export function getRatingsMap(): Record<string, RatingRec> {
  return leer();
}

export function setRating(peliId: string, rec: Partial<RatingRec>): void {
  const all = leer();
  const prev: RatingRec = all[peliId] ?? { tmdb: 0, user: null, visto: false };
  all[peliId] = { ...prev, ...rec };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    // localStorage lleno — ignorar.
  }
}
