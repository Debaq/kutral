import { describe, expect, it, beforeEach, vi } from "vitest";

// cast.svelte.ts guarda lo aprendido en localStorage: en node no existe.
const almacen = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => almacen.get(k) ?? null,
  setItem: (k: string, v: string) => void almacen.set(k, v),
  removeItem: (k: string) => void almacen.delete(k),
});

const { evaluar, aprenderExito, olvidarAprendido } = await import("./cast.svelte");

const cast = { id: "tv-cast", nombre: "Google TV", modelo: "", ip: "", puerto: 8009, tipo: "cast" };
const dlna = { ...cast, id: "tv-dlna", tipo: "dlna" };

describe("evaluar", () => {
  beforeEach(() => {
    olvidarAprendido(cast.id);
    olvidarAprendido(dlna.id);
  });

  it("Google Cast descarta DTS y TrueHD de entrada", () => {
    expect(evaluar(cast, { video: "h264", audio: "dts" })).toEqual({
      apto: false,
      motivo: "Google Cast no reproduce audio DTS",
    });
    expect(evaluar(cast, { video: "hevc", audio: "truehd" }).apto).toBe(false);
    expect(evaluar(cast, { video: "h264", audio: "eac3" }).apto).toBe(true);
  });

  it("si la TV ya reprodujo DTS, lo aprendido manda", () => {
    aprenderExito(cast.id, { video: "h264", audio: "dts" });
    expect(evaluar(cast, { video: "hevc", audio: "dts" }).apto).toBe(true);
  });

  it("DLNA no tiene la regla: depende de cada TV", () => {
    expect(evaluar(dlna, { video: "h264", audio: "dts" }).apto).toBe(true);
  });
});
