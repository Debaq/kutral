import { describe, expect, it } from "vitest";
import { comodidadDesde } from "./idiomas";

describe("comodidadDesde", () => {
  it("se compara como conjunto, sin importar el orden", () => {
    expect(comodidadDesde(["en", "es"])).toBe("es_en");
    expect(comodidadDesde(["es", "en"])).toBe("es_en");
  });

  it("solo español", () => {
    expect(comodidadDesde(["es"])).toBe("es");
  });

  it("vacío u otra combinación es cualquiera", () => {
    expect(comodidadDesde([])).toBe("cualquiera");
    expect(comodidadDesde(["fr"])).toBe("cualquiera");
  });
});
