// Plantillas de Habla. Cálidas, breves, español neutro (tú, no voseo).
// Sin LLM. Modular para enchufar uno después.
// FUTURO: aquí va integración LLM + lectura de subtítulos.

import type { Contexto } from "$lib/vera/tipos";

export type MomentoHabla = "bienvenida" | "pausa" | "fin" | "joya";

const SALUDO_SOLO = [
  "Llegaste. ¿Cómo te fue hoy?",
  "Hola. Cuéntame algo de tu día antes de partir.",
  "Aquí estoy. ¿Algo de hoy que te haya quedado dando vueltas?",
];

const SALUDO_PAREJA = [
  "¿Ya le contaste cómo te fue hoy?",
  "Antes de empezar: ¿hablaron del día?",
];

const SALUDO_AMIGOS = [
  "Antes del play: ¿qué cuentan?",
  "Hace tiempo que no se ven. Aprovechen los minutos antes.",
  "¿Comieron algo? Hablen un rato primero.",
];

// "ninos" implica siempre adulto presente — la regla de marca lo garantiza.
const SALUDO_NINOS = [
  "Antes del play, ¿ya les diste un beso?",
  "Los niños también tienen cosas para contar. ¿Hablaron?",
];

const PAUSA_SOLO = [
  "¿Sigues enganchado o cambio el rumbo?",
  "Si quieres, charlamos cinco minutos.",
];

const PAUSA_PAREJA = ["¿Pausa para comentar? Aprovechen."];

const PAUSA_AMIGOS = [
  "Aprovechen la pausa para hablar.",
  "¿Tema en común? Ahora es el momento.",
];

const PAUSA_NINOS = ["¿Baño de los niños o solo descanso?"];

const FIN_SOLO = [
  "¿Cómo te dejó? Si quieres, te escucho.",
  "Listo. ¿Te sirvió la elección?",
];

const FIN_PAREJA = ["¿Les gustó? Que el que diga 'sí' explique por qué."];

const FIN_AMIGOS = [
  "¿Quién la habría elegido? Saquen cuentas.",
  "Buena vuelta. Cuéntense lo que pensaron.",
];

const FIN_NINOS = ["A dormir. Buen rato."];

// Frase joya — íntima, raro, no es un saludo. Para final/calma.
const JOYA = [
  "¿Cuál fue tu propia película hoy? ¿Le contaste a alguien qué te hizo feliz?",
  "De todo lo que pasó hoy, ¿qué guardas?",
];

export function fraseHabla(
  momento: MomentoHabla,
  contexto: Contexto,
): string {
  const pool = elegirPool(momento, contexto);
  return pool[Math.floor(Math.random() * pool.length)];
}

function elegirPool(momento: MomentoHabla, contexto: Contexto): string[] {
  if (momento === "joya") return JOYA;
  if (momento === "bienvenida") {
    if (contexto === "solo") return SALUDO_SOLO;
    if (contexto === "pareja") return SALUDO_PAREJA;
    if (contexto === "amigos") return SALUDO_AMIGOS;
    return SALUDO_NINOS;
  }
  if (momento === "pausa") {
    if (contexto === "solo") return PAUSA_SOLO;
    if (contexto === "pareja") return PAUSA_PAREJA;
    if (contexto === "amigos") return PAUSA_AMIGOS;
    return PAUSA_NINOS;
  }
  if (momento === "fin") {
    if (contexto === "solo") return FIN_SOLO;
    if (contexto === "pareja") return FIN_PAREJA;
    if (contexto === "amigos") return FIN_AMIGOS;
    return FIN_NINOS;
  }
  return [""];
}

// Stub para integración futura.
// FUTURO: leer últimas líneas de subtítulos en pausa,
// generar comentario con LLM, devolver string corto.
export async function comentarioEscenaFuturo(
  subtitulosRecientes: string[],
): Promise<string> {
  void subtitulosRecientes;
  return "";
}
