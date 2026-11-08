import { describe, expect, it, vi } from "vitest";

// fuentes.ts lee preferencias de config; el módulo real habla con Tauri.
vi.mock("$lib/config.svelte", () => ({
  config: { torrentMaxQuality: "1080p", torrentMaxGb: 10, subMode: "dub", lang: "es-CL" },
  idiomaContenido: () => "es",
}));

import { fmtSize, infoChips, stack, ordenarFuentes, nombreProveedor, esPack, type Src } from "./fuentes";

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

  it("HDRip es un rip de SDR, no HDR", () => {
    expect(infoChips(src({ title: "Pelicula.2024.HDRip.x264" }))).toEqual(["H264"]);
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

describe("nombreProveedor", () => {
  it("nombres legibles, sin repetir", () => {
    expect(nombreProveedor("torrentio+pelispanda")).toBe("Torrentio · PelisPanda");
    expect(nombreProveedor("torrentio-kitsu+torrentio")).toBe("Torrentio");
    expect(nombreProveedor("jkanime-rd")).toBe("JKAnime vía Real-Debrid");
    expect(nombreProveedor("otro")).toBe("otro");
  });
});

describe("ordenarFuentes", () => {
  it("doblado: el audio en español gana a 'al instante' en inglés", () => {
    const ingles = src({ source: "torrentio", title: "Superman.2025.1080p.WEB-DL", rd_cached: true, seeders: 900 });
    const latino = src({ source: "pelispanda", title: "Superman (2025) [WEB-DL 1080p] Dual [Latino/Inglés]", rd_cached: null });
    const orden = ordenarFuentes([ingles, latino], "movie");
    expect(orden[0]).toBe(latino);
  });
  it("con el mismo idioma, 'al instante' sigue primero", () => {
    const a = src({ source: "pelispanda", title: "X [Dual 720p] [Latino/Inglés]", rd_cached: null, quality: "p720" });
    const b = src({ source: "torrentio", title: "X.720p.Latino", rd_cached: true, quality: "p720" });
    expect(ordenarFuentes([a, b], "movie")[0]).toBe(b);
  });
});

describe("ordenarFuentes: lo comprobado, CAM y peso", () => {
  const GB = 1024 ** 3;
  it("la que ya se sabe sin español baja aunque el nombre diga Latino", () => {
    const mentirosa = src({ title: "X.1080p.Latino", rd_cached: true, quality: "p1080", _es: "none" });
    const otra = src({ title: "X.1080p.Latino.WEB", rd_cached: true, quality: "p1080" });
    expect(ordenarFuentes([mentirosa, otra], "movie")[0]).toBe(otra);
  });
  it("audio comprobado gana a la que solo lo promete", () => {
    const promete = src({ title: "X.1080p.Latino", rd_cached: true, quality: "p1080" });
    const comprobada = src({ title: "X.1080p", rd_cached: true, quality: "p1080", _es: "audio" });
    expect(ordenarFuentes([promete, comprobada], "movie")[0]).toBe(comprobada);
  });
  it("CAM queda bajo una de calidad desconocida", () => {
    const cam = src({ title: "X.CAM.Latino", quality: "cam" });
    const nose = src({ title: "X.Latino", quality: "unknown" });
    expect(ordenarFuentes([cam, nose], "movie")[0]).toBe(nose);
  });
  it("a igual idioma y calidad, la más liviana; no una sospechosamente chica", () => {
    const remux = src({ title: "X.1080p.Latino.REMUX", quality: "p1080", size_bytes: 30 * GB });
    const web = src({ title: "X.1080p.Latino.WEB", quality: "p1080", size_bytes: 2.5 * GB });
    const fake = src({ title: "X.1080p.Latino.mini", quality: "p1080", size_bytes: 0.3 * GB });
    const orden = ordenarFuentes([remux, fake, web], "movie");
    expect(orden[0]).toBe(web);
    expect(orden[2]).toBe(fake);
  });
});

describe("esPack", () => {
  it("temporada o batch sí; capítulo suelto no", () => {
    expect(esPack(src({ title: "Show.S01.1080p.WEB" }), "series")).toBe(true);
    expect(esPack(src({ title: "Show Temporada 2 Completa" }), "series")).toBe(true);
    expect(esPack(src({ title: "[Grupo] Show (01-12) [1080p]" }), "anime")).toBe(true);
    expect(esPack(src({ title: "Show.S01E05.1080p" }), "series")).toBe(false);
    expect(esPack(src({ title: "[SubsPlease] Show Season 2 - 05 (1080p)" }), "anime")).toBe(false);
    expect(esPack(src({ title: "Show.S01.1080p" }), "movie")).toBe(false);
  });
});
