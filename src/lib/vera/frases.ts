// Bancos de frases de Vera + builder del tadán dinámico.
// Todo plantillas — sin generación, sin IA.

import type { EstadoVera, Pelicula, Gesto } from "./tipos";
import { MEMORIA_FRASES, HORA_NOCHE } from "./config";

// Memoria circular global de últimas frases dichas por grupo,
// para no repetir seguido. (Suficiente para un prototipo.)
const recientes: Map<string, string[]> = new Map();

function escoger(grupo: string, frases: string[]): string {
  if (frases.length === 0) return "";
  const prev = recientes.get(grupo) ?? [];
  const disponibles = frases.filter((f) => !prev.includes(f));
  const pool = disponibles.length > 0 ? disponibles : frases;
  const elegida = pool[Math.floor(Math.random() * pool.length)];
  const nuevo = [elegida, ...prev].slice(0, MEMORIA_FRASES);
  recientes.set(grupo, nuevo);
  return elegida;
}

// --- Comentarios en vivo del mazo, según gesto + pista de la peli ---

const FRASES_VISTA = [
  "Ah, esa ya cayó.",
  "Esa ya está. Sigo.",
  "Hecha. Tomo nota del gusto.",
];

const FRASES_INTERES = [
  "Ojo a eso.",
  "Tomo nota.",
  "Ese palo te llama, claro.",
];

// "Pasar" no genera comentario: avanzamos en silencio.
// Vera no penaliza descartes (regla de marca).
export function comentarioMazo(p: Pelicula, gesto: Gesto): string {
  void p;
  if (gesto === "vista") return escoger("vista", FRASES_VISTA);
  if (gesto === "interes") return escoger("interes", FRASES_INTERES);
  return "";
}

// --- Builder del tadán: SIEMPRE menciona al menos un dato real del usuario ---

export function fraseTadan(estado: EstadoVera, p: Pelicula): string {
  const partes: string[] = [];

  // Contexto humano. "ninos" implica siempre adulto presente.
  if (estado.contexto === "pareja") partes.push("Son dos");
  else if (estado.contexto === "amigos") partes.push("Hay gente acompañando");
  else if (estado.contexto === "ninos") partes.push("Hay niños mirando");
  else if (estado.contexto === "solo") partes.push("Estás solo");

  // Patrón positivo: si marcó interés mayoritariamente en un tono, lo mencionamos.
  const tonoInteresado = patronTonoInteres(estado);
  if (tonoInteresado === "denso") partes.push("buscan algo con peso");
  else if (tonoInteresado === "liviano") partes.push("quieren algo liviano");

  // Lo que ya vio.
  if (estado.vistas.length > 0) {
    const ultima = estado.reacciones
      .slice()
      .reverse()
      .find((r) => r.gesto === "vista");
    if (ultima) partes.push(`ya vieron "${ultima.pelicula.titulo}"`);
  }

  // Sesgo noche, mencionado solo si es tarde.
  const ahora = estado.horaActual;
  const esNoche = ahora >= HORA_NOCHE || ahora < 5;
  if (esNoche) partes.push(`son las ${ahora}`);

  // Si no hay nada, fallback honesto (no debería pasar — siempre hay contexto).
  if (partes.length === 0) {
    return `Me la juego por "${p.titulo}".`;
  }

  // Une las partes y cierra con la apuesta.
  const intro = partes.join(", ") + "...";
  return `${intro} así que me la juego por "${p.titulo}".`;
}

function patronTonoInteres(estado: EstadoVera): "denso" | "liviano" | null {
  const positivos = estado.reacciones.filter((r) => r.gesto === "interes");
  if (positivos.length < 2) return null;
  const densos = positivos.filter((r) => r.pelicula.tono === "denso").length;
  const livianos = positivos.filter((r) => r.pelicula.tono === "liviano").length;
  if (densos >= 2 && densos > livianos) return "denso";
  if (livianos >= 2 && livianos > densos) return "liviano";
  return null;
}

// --- Otras frases del show ---

const FRASES_NO_CONVENCE = [
  "¿Segura? ...bueno, mi otra apuesta.",
  "Ok, mi otra carta entonces.",
  "Difícil. Va esta.",
];

const FRASES_CIERRE = [
  "Si no les gusta, es mi culpa.",
  "Confía. Si fallo, fallo yo.",
];

const FRASES_NO_MATCH = [
  "Hoy no les tengo la indicada. Pongan algo que ya conozcan de fondo, no pasa nada.",
  "Hoy paso. Mejor algo conocido y a descansar.",
];

export function fraseNoConvence(): string {
  return escoger("no-convence", FRASES_NO_CONVENCE);
}

export function fraseCierre(): string {
  return escoger("cierre", FRASES_CIERRE);
}

export function fraseNoMatch(): string {
  return escoger("no-match", FRASES_NO_MATCH);
}
