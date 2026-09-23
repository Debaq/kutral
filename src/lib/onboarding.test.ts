import { describe, expect, it } from "vitest";
import { faltaLoMinimo } from "./onboarding";

describe("faltaLoMinimo", () => {
  it("sin key de TMDb falta el catálogo", () => {
    expect(faltaLoMinimo({ tmdbKey: "  ", rdLinked: true, torrentLocal: false })).toBe(true);
  });

  it("sin debrid ni descarga local no hay cómo reproducir", () => {
    expect(faltaLoMinimo({ tmdbKey: "k", rdLinked: false, torrentLocal: false })).toBe(true);
  });

  it("con key y cualquiera de las dos formas de reproducir está completo", () => {
    expect(faltaLoMinimo({ tmdbKey: "k", rdLinked: true, torrentLocal: false })).toBe(false);
    expect(faltaLoMinimo({ tmdbKey: "k", rdLinked: false, torrentLocal: true })).toBe(false);
  });
});
