// La voz de Vera. Tres registros, elegidos en el setup y guardados en
// vera_setup.personality.
//
// Reglas de marca que aplican a TODAS las variantes:
//   - Español neutro. Nada de voseo: "tú", "eres", "tienes".
//   - La acción de reproducir se llama "Descubrir". Nunca "Reproducir".
//   - "Con niños" implica siempre un adulto presente.
//
// La personalidad NO toca el motor: mismo ranking, distinta manera de decirlo.
// Es copy, no comportamiento.

import type { Personalidad } from "./tipos";

interface Voz {
  etiqueta: string;
  descripcion: string;
  slogan: string;
  preguntaContexto: string;
  preguntaIntencion: string;
  cargando: string;
  calibrando: string;
  propuesta: string;
  // Cuando ni la mejor del pool llega al umbral: Vera lo dice en vez de
  // empujar la menos mala como si fuera un hallazgo.
  sinMatch: string;
  sinMatchSub: string;
}

const VOCES: Record<Personalidad, Voz> = {
  calida: {
    etiqueta: "Cálida",
    descripcion: "Te acompaña",
    slogan: "Tú pones el sillón. Yo propongo qué ver.",
    preguntaContexto: "¿Quiénes están hoy en el sillón?",
    preguntaIntencion: "¿Qué buscas hoy?",
    cargando: "Buscando algo que te siente bien…",
    calibrando: "Cuéntame qué te llama y qué no.",
    propuesta: "Creo que esta te va a gustar",
    sinMatch: "Hoy no tengo la indicada",
    sinMatchSub:
      "Nada de lo que encontré te queda bien de verdad. Prefiero decírtelo a hacerte perder dos horas.",
  },
  directa: {
    etiqueta: "Directa",
    descripcion: "Va al grano",
    slogan: "Dime con quién estás y qué quieres. Te digo qué ver.",
    preguntaContexto: "¿Con quién ves?",
    preguntaIntencion: "¿Qué quieres hoy?",
    cargando: "Buscando…",
    calibrando: "Califica estas y te armo el ranking.",
    propuesta: "Esta es tu mejor opción",
    sinMatch: "No hay match",
    sinMatchSub: "Ninguna llega al mínimo. Prueba otra ronda o cambia lo que pediste.",
  },
  seca: {
    etiqueta: "Seca",
    descripcion: "Lo mínimo indispensable",
    slogan: "Qué ver, en un minuto.",
    preguntaContexto: "Compañía",
    preguntaIntencion: "Intención",
    cargando: "Cargando…",
    calibrando: "Calibración",
    propuesta: "Recomendación",
    sinMatch: "Sin resultados sobre el umbral",
    sinMatchSub: "Otra ronda o cambia la intención.",
  },
};

export function voz(p: Personalidad | null | undefined): Voz {
  return VOCES[p ?? "calida"];
}

export const PERSONALIDADES: Personalidad[] = ["calida", "directa", "seca"];

export function etiquetaPersonalidad(p: Personalidad): {
  label: string;
  desc: string;
} {
  return { label: VOCES[p].etiqueta, desc: VOCES[p].descripcion };
}
