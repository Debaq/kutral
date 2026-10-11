import { describe, expect, it } from "vitest";
import { elegirPistaSub, esForzada, pistaDelIdioma } from "./pistas";

const p = (id: number, lang: string, title = "", forced = false) => ({ id, lang, title, forced });

describe("pistaDelIdioma", () => {
  it("reconoce el código en sus variantes", () => {
    expect(pistaDelIdioma(p(1, "spa"), "es")).toBe(true);
    expect(pistaDelIdioma(p(1, "es-419"), "es")).toBe(true);
    expect(pistaDelIdioma(p(1, "eng"), "en")).toBe(true);
    expect(pistaDelIdioma(p(1, "fre"), "fr")).toBe(true);
    expect(pistaDelIdioma(p(1, "deu"), "de")).toBe(true);
  });

  it("no confunde idiomas por prefijo", () => {
    // "est" (estonio) empieza con "es" y no es español.
    expect(pistaDelIdioma(p(1, "est"), "es")).toBe(false);
    expect(pistaDelIdioma(p(1, "spa"), "en")).toBe(false);
  });

  it("usa el título cuando no hay código", () => {
    expect(pistaDelIdioma(p(1, "", "Latino"), "es")).toBe(true);
    expect(pistaDelIdioma(p(1, "und", "English SDH"), "en")).toBe(true);
    expect(pistaDelIdioma(p(1, "", "Português (Brasil)"), "pt")).toBe(true);
  });

  it("idioma desconocido u off no matchea", () => {
    expect(pistaDelIdioma(p(1, "spa"), "off")).toBe(false);
  });
});

describe("esForzada", () => {
  it("por la marca o por el título", () => {
    expect(esForzada(p(1, "spa", "", true))).toBe(true);
    expect(esForzada(p(1, "spa", "Forced"))).toBe(true);
    expect(esForzada(p(1, "spa", "Español (forzados)"))).toBe(true);
    expect(esForzada(p(1, "eng", "Signs & Songs"))).toBe(true);
    expect(esForzada(p(1, "spa", "Latino"))).toBe(false);
  });
});

describe("elegirPistaSub", () => {
  it("prefiere la completa aunque la forzada venga primero", () => {
    const r = elegirPistaSub([p(1, "spa", "Forzados", true), p(2, "spa", "Completos")], "es");
    expect(r.completa?.id).toBe(2);
    expect(r.forzada?.id).toBe(1);
  });

  it("solo forzada: no hay completa", () => {
    const r = elegirPistaSub([p(1, "spa", "", true), p(2, "eng")], "es");
    expect(r.completa).toBeUndefined();
    expect(r.forzada?.id).toBe(1);
  });

  it("sigue el idioma de Configuración, no siempre español", () => {
    const subs = [p(1, "spa"), p(2, "eng")];
    expect(elegirPistaSub(subs, "en").completa?.id).toBe(2);
    expect(elegirPistaSub(subs, "fr")).toEqual({ completa: undefined, forzada: undefined });
  });
});
