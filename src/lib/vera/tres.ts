// Las tres cartas de Vera, sacadas del ranking del motor:
//
//   - segura:   lo que más calza contigo.
//   - distinta: buena, pero de otro género que la segura.
//   - sorpresa: la menos conocida entre las bien valoradas, distinta de las dos.
//
// Puro (sin red ni estado): la página decide qué se descarta y cuándo.

import type { Pelicula, RankingPeli } from "./tipos";

export type Ranura = "segura" | "distinta" | "sorpresa";
export const RANURAS: Ranura[] = ["segura", "distinta", "sorpresa"];

// Hasta dónde del ranking se busca cada una: la distinta tiene que seguir
// siendo de las mejores; la sorpresa puede venir de más abajo.
const TOPE_DISTINTA = 12;
const TOPE_SORPRESA = 30;
// Calidad mínima (bayesiana, 0..1) para que una desconocida valga la sorpresa.
const CALIDAD_SORPRESA = 0.62;

/** ¿Son "la misma recomendación"? Mismo género principal o la mitad de los géneros. */
export function parecidas(a: Pelicula, b: Pelicula): boolean {
  if (a.generos[0] && a.generos[0] === b.generos[0]) return true;
  const sa = new Set(a.generos);
  const comunes = b.generos.filter((g) => sa.has(g)).length;
  const min = Math.min(a.generos.length, b.generos.length);
  return min > 0 && comunes / min >= 0.5;
}

/**
 * La mejor del ranking para `ranura`, sin repetir las `usadas` (ya mostradas,
 * descartadas o vistas) y distinta de `otras` (las cartas que siguen en pantalla).
 */
export function candidataPara(
  ranura: Ranura,
  ranking: RankingPeli[],
  usadas: Set<string>,
  otras: Pelicula[],
): RankingPeli | null {
  const libres = ranking.filter((r) => !usadas.has(r.pelicula.id));
  if (libres.length === 0) return null;
  const distintaDeOtras = (r: RankingPeli) => otras.every((o) => !parecidas(r.pelicula, o));
  if (ranura === "segura") return libres[0];
  if (ranura === "distinta") {
    const top = libres.slice(0, TOPE_DISTINTA);
    return top.find(distintaDeOtras) ?? top[0];
  }
  const top = libres.slice(0, TOPE_SORPRESA);
  const buenas = top.filter((r) => r.porQue.calidad >= CALIDAD_SORPRESA);
  const base = buenas.filter(distintaDeOtras);
  const elegibles = base.length ? base : buenas.length ? buenas : top;
  return [...elegibles].sort((a, b) => a.pelicula.popularidad - b.pelicula.popularidad)[0];
}

/** Las tres de una vez, en orden. Una ranura sin candidata queda en null. */
export function elegirTres(ranking: RankingPeli[], usadas: Set<string>): (RankingPeli | null)[] {
  const out: (RankingPeli | null)[] = [];
  const tomadas = new Set(usadas);
  for (const ranura of RANURAS) {
    const otras = out.filter((x): x is RankingPeli => x !== null).map((x) => x.pelicula);
    const c = candidataPara(ranura, ranking, tomadas, otras);
    if (c) tomadas.add(c.pelicula.id);
    out.push(c);
  }
  return out;
}
