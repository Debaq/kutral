import { describe, expect, it, vi } from "vitest";

// fuentes.ts lee preferencias de config; el módulo real habla con Tauri.
vi.mock("$lib/config.svelte", () => ({
  config: { torrentMaxQuality: "1080p", torrentMaxGb: 10 },
}));

import { fmtSize, infoChips, stack, type Src } from "./fuentes";

const src = (over: Partial<Src>): Src => ({ quality: "1080p", title: "", ...over }) as Src;

describe("fmtSize", () => {
  it("GB con un decimal, MB redondeado", () => {
    expect(fmtSize(null)).toBe("");
    expect(fmtSize(0)).toBe("");
    expect(fmtSize(1.5 * 1024 ** 3)).toBe("1.5 GB");
    expect(fmtSize(700 * 1024 ** 2)).toBe("700 MB");
  });
});

describe("infoChips", () => {
  it("lee códec, HDR y audio del nombre del release", () => {
    expect(infoChips(src({ title: "Pelicula.2024.2160p.WEB-DL.x265.HDR10.Atmos" }))).toEqual([
      "HEVC",
      "HDR",
      "Atmos",
    ]);
    expect(infoChips(src({ title: "Pelicula 2024 1080p DV DDP5.1 Latino" }))).toEqual([
      "DV",
      "DD+",
      "Latino",
    ]);
  });
});

describe("stack", () => {
  it("junta releases equivalentes y cuenta cuántos hay", () => {
    const out = stack([
      src({ title: "Pelicula.2024.1080p.WEB-DL.x264.ENG" }),
      src({ title: "Pelicula 2024 1080p WEBRip x265 Latino" }),
      src({ title: "Otra.Cosa.2024.1080p" }),
    ]);
    expect(out).toHaveLength(2);
    expect(out[0]._count).toBe(2);
  });

  it("no apila un hardsub con uno que no lo es", () => {
    const out = stack([
      src({ title: "Pelicula 2024 1080p", hardsub: "es" } as Partial<Src>),
      src({ title: "Pelicula 2024 1080p" }),
    ]);
    expect(out).toHaveLength(2);
  });
});
