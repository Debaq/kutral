// Constantes ajustables del prototipo Vera and Chill.
// Cambiar acá y se propaga a todo el flujo.

// Cantidad de cartas que se le muestran al usuario en el mazo de calibración.
// Más cartas = más señal, pero la gente se cansa. Mantener bajo.
export const CARTAS_MAZO = 8;

// Score absoluto por debajo del cual Vera prefiere ser honesta
// y decir "hoy no tengo la indicada" en vez de empujar relleno.
// Si la mejor opción no supera este umbral, salta la carta rara.
export const UMBRAL_NO_MATCH = 1.5;

// Hora a partir de la cual Vera sesga hacia tono liviano
// y puede mencionarlo en el tadán ("Once de la noche... algo liviano").
export const HORA_NOCHE = 22;

// Demo: cuánto debe pasar en pausa para que Habla aparezca.
// En producción serían minutos (>30s mínimo según specs).
// Acá lo bajamos para que se vea en demo sin esperar.
export const DEMO_PAUSA_MS = 4000;

// Demo: tiempo en pausa por debajo del cual Habla NO aparece nunca.
// Regla dura: pausas cortas (<30s reales) no disparan compañía.
export const DEMO_PAUSA_MINIMA_MS = 1500;

// Tamaño del banco de "frases recientes" que no se repiten.
// Evita que Vera diga lo mismo dos veces seguidas.
export const MEMORIA_FRASES = 3;
