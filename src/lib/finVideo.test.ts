import { describe, expect, it } from "vitest";
import { esFinReal } from "./finVideo";

describe("esFinReal", () => {
  it("un corte a mitad no es el final", () => {
    expect(esFinReal({ pos: 720, duration: 6000 })).toBe(false);
  });

  it("los créditos ya cuentan como final", () => {
    expect(esFinReal({ pos: 5400, duration: 6000 })).toBe(true);
    expect(esFinReal({ pos: 6000, duration: 6000 })).toBe(true);
  });

  it("sin duración conocida se toma por terminado", () => {
    expect(esFinReal({ pos: 10, duration: 0 })).toBe(true);
  });

  it("sin datos o con basura no hay final", () => {
    expect(esFinReal(null)).toBe(false);
    expect(esFinReal(undefined)).toBe(false);
    expect(esFinReal({ pos: NaN, duration: 100 })).toBe(false);
    expect(esFinReal({ pos: 10, duration: Infinity })).toBe(false);
  });
});
