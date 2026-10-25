import { describe, expect, it } from "vitest";
import { candidataPara, elegirTres, parecidas } from "./tres";
import type { Pelicula, RankingPeli } from "./tipos";

function peli(id: string, generos: string[], popularidad = 50): Pelicula {
  return { id, generos, popularidad } as Pelicula;
}

function rk(p: Pelicula, score: number, calidad = 0.8): RankingPeli {
  return { pelicula: p, score, porQue: { calidad } } as RankingPeli;
}

describe("parecidas", () => {
  it("mismo género principal o la mitad en común", () => {
    expect(parecidas(peli("a", ["Drama", "Romance"]), peli("b", ["Drama"]))).toBe(true);
    expect(parecidas(peli("a", ["Comedia", "Drama"]), peli("b", ["Romance", "Drama"]))).toBe(true);
    expect(parecidas(peli("a", ["Comedia"]), peli("b", ["Terror"]))).toBe(false);
  });
});

describe("elegirTres", () => {
  const ranking = [
    rk(peli("1", ["Drama"], 90), 0.9),
    rk(peli("2", ["Drama", "Historia"], 80), 0.85),
    rk(peli("3", ["Comedia"], 70), 0.8),
    rk(peli("4", ["Terror"], 5), 0.7),
    rk(peli("5", ["Animación"], 2), 0.6, 0.4),
  ];

  it("segura la primera, distinta de otro género, sorpresa poco conocida y buena", () => {
    const [s, d, x] = elegirTres(ranking, new Set());
    expect(s?.pelicula.id).toBe("1");
    expect(d?.pelicula.id).toBe("3");
    // La 5 es menos conocida, pero mal valorada: gana la 4.
    expect(x?.pelicula.id).toBe("4");
  });

  it("no repite las usadas", () => {
    const [s] = elegirTres(ranking, new Set(["1"]));
    expect(s?.pelicula.id).toBe("2");
  });

  it("sin candidatas queda null", () => {
    expect(candidataPara("segura", [], new Set(), [])).toBeNull();
  });
});
