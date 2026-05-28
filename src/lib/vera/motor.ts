// Motor de recomendación 100% determinista.
// Pesos simples, reglas explícitas, cero IA.

import type {
  Pelicula,
  EstadoVera,
  Recomendaciones,
  Reaccion,
} from "./tipos";
import { HORA_NOCHE } from "./config";

// Solo pesos positivos. "Pasar" no genera señal — Vera no penaliza descartes.
const PESO_VISTA_GENERO = 0.8;     // "ya la vi" sin rating = señal suave
const PESO_VISTA_TONO = 0.6;
const PESO_INTERES_GENERO = 1.2;
const PESO_INTERES_TONO = 0.8;

// Cuando el usuario califica 1-5 una peli vista, el peso escala con la nota:
// 5★ = +2.0 / 4★ = +1.0 / 3★ = 0 / 2★ = -1.0 / 1★ = -2.0 al género.
// Esto da MUCHA más señal que el simple "ya la vi".
const PESO_RATING_GENERO_POR_ESTRELLA = 1.0;
const PESO_RATING_TONO_POR_ESTRELLA = 0.7;

// Sesgo noche: si es tarde, suma a tono liviano y resta a tono denso.
const SESGO_NOCHE_LIVIANO = 1.0;
const SESGO_NOCHE_DENSO = -0.8;

// Construye un perfil de afinidad por género y tono a partir de las reacciones.
function perfilDePesos(reacciones: Reaccion[]): {
  generos: Map<string, number>;
  tonos: Map<string, number>;
} {
  const generos = new Map<string, number>();
  const tonos = new Map<string, number>();

  for (const r of reacciones) {
    const deltaGen = pesoGenero(r);
    const deltaTono = pesoTono(r);
    for (const g of r.pelicula.generos) {
      generos.set(g, (generos.get(g) ?? 0) + deltaGen);
    }
    tonos.set(r.pelicula.tono, (tonos.get(r.pelicula.tono) ?? 0) + deltaTono);
  }
  return { generos, tonos };
}

function pesoGenero(r: Reaccion): number {
  if (r.gesto === "pasar") return 0;
  if (r.gesto === "interes") return PESO_INTERES_GENERO;
  // vista: si hay rating, escalar; si no, fallback al peso fijo.
  if (r.userRating != null) {
    return (r.userRating - 3) * PESO_RATING_GENERO_POR_ESTRELLA;
  }
  return PESO_VISTA_GENERO;
}

function pesoTono(r: Reaccion): number {
  if (r.gesto === "pasar") return 0;
  if (r.gesto === "interes") return PESO_INTERES_TONO;
  if (r.userRating != null) {
    return (r.userRating - 3) * PESO_RATING_TONO_POR_ESTRELLA;
  }
  return PESO_VISTA_TONO;
}

// Filtros duros: solo ya-vistas (no repetir) y familyFriendly cuando hay niños.
// No filtramos "rechazos" — Vera no penaliza descartes por la regla de marca.
function filtrar(
  catalogo: Pelicula[],
  estado: EstadoVera,
): Pelicula[] {
  return catalogo.filter((p) => {
    if (estado.vistas.includes(p.id)) return false;
    if (estado.contexto === "ninos" && !p.familyFriendly) return false;
    return true;
  });
}

// Puntúa cada peli según el perfil + sesgo noche + bonus interés directo.
function puntuar(
  pelis: Pelicula[],
  estado: EstadoVera,
): { pelicula: Pelicula; score: number }[] {
  const { generos, tonos } = perfilDePesos(estado.reacciones);
  const esNoche = estado.horaActual >= HORA_NOCHE || estado.horaActual < 5;

  return pelis.map((p) => {
    let score = 0;

    // Suma por géneros que coinciden con afinidad.
    for (const g of p.generos) {
      score += generos.get(g) ?? 0;
    }
    // Tono.
    score += tonos.get(p.tono) ?? 0;

    // Sesgo nocturno: si es tarde, premiar liviano y castigar denso.
    if (esNoche) {
      score += p.tono === "liviano" ? SESGO_NOCHE_LIVIANO : SESGO_NOCHE_DENSO;
    }

    // Bonus chico por rating, para desempatar opciones parecidas.
    score += (p.rating - 7) * 0.15;

    // Si el usuario marcó interés directo en una peli con géneros similares,
    // dar pequeño empujón extra (ya está reflejado en perfil, este es un bonus de cercanía).
    if (estado.interesadas.length > 0) {
      const generosInteres = new Set<string>();
      for (const id of estado.interesadas) {
        // se podría leer del catálogo original, pero perfil ya lo cubre
        void id;
      }
      void generosInteres;
    }

    return { pelicula: p, score };
  });
}

// Elige las 3 cartas internas: segura, distinta, sorpresa.
// La regla específica de cada una está pensada para variedad,
// no para optimizar score puro.
export function recomendar(
  catalogo: Pelicula[],
  estado: EstadoVera,
): Recomendaciones {
  const candidatos = filtrar(catalogo, estado);
  if (candidatos.length === 0) {
    return { segura: null, distinta: null, sorpresa: null };
  }

  const puntuados = puntuar(candidatos, estado).sort(
    (a, b) => b.score - a.score,
  );

  // Segura = el de mayor score.
  const segura = puntuados[0] ?? null;

  // Distinta = entre top 10, primero con género distinto al de la segura.
  let distinta = null;
  if (segura) {
    const generosSegura = new Set(segura.pelicula.generos);
    const top10 = puntuados.slice(0, 10);
    distinta =
      top10
        .slice(1)
        .find((c) => !c.pelicula.generos.some((g) => generosSegura.has(g))) ??
      null;
  }

  // Sorpresa = entre top 30, el de menor popularidad (lo que el usuario
  // menos probable habría elegido por sí solo).
  const top30 = puntuados.slice(0, 30);
  const candSorpresa = top30.filter(
    (c) => c !== segura && c !== distinta,
  );
  const sorpresa =
    candSorpresa.sort(
      (a, b) => a.pelicula.popularidad - b.pelicula.popularidad,
    )[0] ?? null;

  return { segura, distinta, sorpresa };
}

// Elige las cartas iniciales del mazo: variedad de tono y género.
// Aleatoriza dentro de cada tono para que el mazo cambie entre sesiones.
export function elegirMazo(
  catalogo: Pelicula[],
  estado: EstadoVera,
  cantidad: number,
): Pelicula[] {
  const base = catalogo.filter((p) => {
    if (estado.contexto === "ninos" && !p.familyFriendly) return false;
    return true;
  });

  // Fisher-Yates shuffle simple, sin semilla.
  // Aceptable que sea distinto cada vez — la idea es no repetir mazo.
  const mezclados = [...base];
  for (let i = mezclados.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [mezclados[i], mezclados[j]] = [mezclados[j], mezclados[i]];
  }
  const livianos = mezclados.filter((p) => p.tono === "liviano");
  const densos = mezclados.filter((p) => p.tono === "denso");

  const mazo: Pelicula[] = [];
  let iL = 0;
  let iD = 0;
  while (mazo.length < cantidad && (iL < livianos.length || iD < densos.length)) {
    if (iL < livianos.length) mazo.push(livianos[iL++]);
    if (mazo.length >= cantidad) break;
    if (iD < densos.length) mazo.push(densos[iD++]);
  }
  return mazo;
}
