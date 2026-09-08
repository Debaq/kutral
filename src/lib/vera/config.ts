// Constantes ajustables de Vera and Chill.
// Cambiar acá y se propaga a todo el flujo.
//
// Se quitaron CARTAS_MAZO, DEMO_PAUSA_MS, DEMO_PAUSA_MINIMA_MS y MEMORIA_FRASES:
// pertenecían al prototipo con mock player y burbuja Habla, que ya no existe.
// Nadie las leía.

// Cuántas cartas de calibración pedir, según cuántas reacciones reales ya
// tiene el perfil. Antes eran 8 fijas SIEMPRE, incluso para alguien con 200
// pelis calificadas: peaje puro. La curva baja hasta 3 porque a esa altura la
// calibración deja de informar al motor y solo sirve para refrescar el pool.
//
// Se lee de arriba abajo: primer tramo cuyo `hasta` supera las muestras.
export const CARTAS_POR_MADUREZ: { hasta: number; cartas: number }[] = [
  { hasta: 5, cartas: 8 },        // te conozco poco: necesito señal
  { hasta: 20, cartas: 6 },
  { hasta: 60, cartas: 4 },
  { hasta: Infinity, cartas: 3 }, // ya te conozco: no te hago perder tiempo
];

export function cartasParaMuestras(muestras: number): number {
  for (const t of CARTAS_POR_MADUREZ) {
    if (muestras < t.hasta) return t.cartas;
  }
  return 3;
}

// Score absoluto por debajo del cual Vera prefiere ser honesta y avisar que
// no tiene una buena, en vez de empujar la menos mala como si fuera un match.
// El score del motor vive en 0..1 (más los ajustes de contexto), así que este
// umbral se lee como "menos de la mitad de match".
export const UMBRAL_NO_MATCH = 0.45;

// Hora a partir de la cual Vera sesga hacia tono liviano.
// OJO: el sesgo solo aplica cuando el usuario NO pidió un tono explícito
// (intent "sorpresa"). Ver motor.ts — pedir "denso" a las 23h y que el motor
// te penalice lo denso era un bug real.
export const HORA_NOCHE = 22;

// --- Rating bayesiano ---
//
// vote_average crudo no sirve para rankear: un 8.9 con 110 votos le gana a un
// 8.2 con 12.000, y el primero es ruido. La fórmula es la de IMDb:
//
//   ponderado = (v/(v+m)) * R + (m/(v+m)) * C
//
// con R = vote_average, v = vote_count, m = votos mínimos para tomarse en
// serio la nota, C = media global del catálogo. Con pocos votos el resultado
// colapsa a C (la media); con muchos, converge a la nota real.
export const VOTOS_MINIMOS = 800;
export const MEDIA_GLOBAL_TMDB = 6.6;

// --- Diversidad del ranking (MMR) ---
//
// Sin esto el top del ranking son cinco pelis del mismo género y la misma
// década: el motor optimiza afinidad y la afinidad premia lo mismo cinco
// veces. LAMBDA_MMR reparte entre relevancia (1.0 = solo score) y variedad
// (0.0 = solo diferencia). 0.75 mantiene el orden por calidad pero rompe
// rachas de títulos gemelos.
export const LAMBDA_MMR = 0.75;

// Cuántas posiciones del ranking se rediversifican. Más abajo no importa:
// nadie navega hasta la #40.
export const TOPE_MMR = 15;

// --- Penalizaciones ---
//
// Ya vista: no se excluye (verla de nuevo es legítimo), pero baja. La que te
// gustó mucho baja menos: rever algo que calificaste +5 es un plan, rever algo
// que calificaste -3 no.
export const PENAL_VISTA_BASE = 0.35;

// Excedida de duración respecto al tope del perfil. Proporcional al exceso y
// con techo: una peli 20 minutos más larga no es lo mismo que uno de tres horas.
export const PENAL_DURACION_MAX = 0.3;

// Fuera de los idiomas en los que la persona dijo moverse cómoda. No es un
// veto (para eso están los idiomas evitados, que excluyen): es el costo real
// de leer subtítulos dos horas cuando dijiste que prefieres no hacerlo.
export const PENAL_IDIOMA_INCOMODO = 0.18;

// Empujón para la animación cuando la persona dijo que le encanta. Del mismo
// orden que el ajuste de contexto: mueve el orden, no lo dicta.
export const BONUS_ANIMACION = 0.08;

// Cuántas pelis se enriquecen (tmdb_detail) por lote en segundo plano.
// TMDb no publica un límite duro por segundo, pero tandas grandes en paralelo
// disparan 429. 6 mantiene la ficha llena sin que se caiga el lote.
export const LOTE_ENRIQUECIMIENTO = 6;
