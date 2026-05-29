// Almacén persistente de reacciones del usuario por película.
// Vive en localStorage, sobrevive entre sesiones — Vera empieza a "conocer".
// Sin sincronización con servidor todavía.
//
// Modelo B5:
//   - interes: -5..+5, intuición previa (antes de ver).
//   - juicio:  -5..+5 o null. null = no la vio. Si juicio !== null, fue vista.
//
// "visto" NO es campo, es derivado: (juicio !== null).
//
// Clave nueva (vera_reacciones_v2), sin migrar la vieja (vera_ratings_v1).
// La vieja queda en localStorage pero nadie la lee. El usuario pierde el
// rating que haya puesto en B4a, aceptable según decisión de B5.

const STORAGE_KEY = "vera_reacciones_v2";

export interface ReaccionGuardada {
  tmdb: number;          // 0-10 de TMDb (referencia objetiva, para debug)
  interes: number;       // -5..+5
  juicio: number | null; // -5..+5 si vista; null si no.
}

function leer(): Record<string, ReaccionGuardada> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as Record<string, ReaccionGuardada>;
  } catch {
    return {};
  }
}

export function getReaccion(peliId: string): ReaccionGuardada | undefined {
  return leer()[peliId];
}

export function getReaccionesMap(): Record<string, ReaccionGuardada> {
  return leer();
}

// Default para pelis sin reacción previa.
function vacia(tmdb = 0): ReaccionGuardada {
  return { tmdb, interes: 0, juicio: null };
}

// Devuelve la reacción persistida o una vacía. Útil para evitar undefined.
export function getOReaccion(
  peliId: string,
  tmdbRef = 0,
): ReaccionGuardada {
  return leer()[peliId] ?? vacia(tmdbRef);
}

export function setReaccion(
  peliId: string,
  rec: Partial<ReaccionGuardada>,
): void {
  const all = leer();
  const prev: ReaccionGuardada = all[peliId] ?? vacia();
  all[peliId] = { ...prev, ...rec };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    // localStorage lleno — ignorar.
  }
}
