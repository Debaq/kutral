import { describe, expect, it } from "vitest";
import { distancia, type Rect } from "./nav";

const rect = (left: number, top: number, width: number, height: number): Rect => ({
  left,
  top,
  width,
  height,
  right: left + width,
  bottom: top + height,
});

describe("distancia", () => {
  const actual = rect(100, 100, 100, 40);

  it("descarta lo que no está en la dirección pedida", () => {
    expect(distancia(actual, rect(0, 100, 50, 40), "right")).toBeNull();
    expect(distancia(actual, rect(300, 100, 50, 40), "left")).toBeNull();
    expect(distancia(actual, rect(100, 0, 100, 40), "down")).toBeNull();
    // Lo que está casi en el mismo lugar tampoco cuenta.
    expect(distancia(actual, rect(103, 100, 100, 40), "right")).toBeNull();
  });

  it("lo pegado al frente le gana a lo lejano bien centrado", () => {
    // Abajo, ancho y corrido a la derecha pero cruzándose en X: al frente.
    const pegado = rect(150, 160, 400, 40);
    // Abajo y centrado, pero 200 px más lejos.
    const lejano = rect(100, 360, 100, 40);
    const dPegado = distancia(actual, pegado, "down")!;
    const dLejano = distancia(actual, lejano, "down")!;
    expect(dPegado).toBeLessThan(dLejano);
  });

  it("sin cruzarse, el desvío lateral castiga fuerte", () => {
    const diagonal = rect(400, 160, 100, 40);
    const derecho = rect(100, 260, 100, 40);
    expect(distancia(actual, derecho, "down")!).toBeLessThan(distancia(actual, diagonal, "down")!);
  });
});
