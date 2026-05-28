// Semilla de respaldo: lista mínima de pelis populares hand-picked
// que `construirPool()` usa SOLO si discover + recommendations no
// logran juntar el mínimo de cartas para el mazo (8). En operación
// normal nunca debería tocarse — el catálogo dinámico cubre todo.
//
// IDs son de TMDb. Si TMDb cambia/borra alguna, el fetch falla
// silencioso y queda con menos pelis.

export const SEMILLA_FALLBACK: number[] = [
  545611, // Everything Everywhere All At Once
  496243, // Parasite
  693134, // Dune: Part Two
  346698, // Barbie
  872585, // Oppenheimer
  569094, // Spider-Man: Across the Spider-Verse
  354912, // Coco
  546554, // Knives Out
  419430, // Get Out
  508442, // Soul
  // 837405 // Past Lives — retirada/movida en TMDb (404), 2026
  361743, // Top Gun: Maverick
  840430, // The Holdovers
  915935, // Anatomy of a Fall
  680,    // Pulp Fiction (el clásico de guiño)
];
