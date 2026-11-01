import { describe, expect, it } from "vitest";
import {
  capitulosDe,
  fichaDeMetadata,
  fmtDuracion,
  fmtLargo,
  libroDeDoc,
  limpiarBusqueda,
  limpiarHtml,
  normalizarIdioma,
  parseDuracion,
  urlCatalogo,
} from "./audiolibros";

describe("normalizarIdioma", () => {
  it("unifica las variantes de Archive", () => {
    expect(normalizarIdioma("spa")).toBe("es");
    expect(normalizarIdioma("Spanish")).toBe("es");
    expect(normalizarIdioma("Español")).toBe("es");
    expect(normalizarIdioma("eng")).toBe("en");
    expect(normalizarIdioma(["English"])).toBe("en");
  });
  it("lo desconocido queda en null", () => {
    expect(normalizarIdioma("ger")).toBeNull();
    expect(normalizarIdioma(undefined)).toBeNull();
    expect(normalizarIdioma(["Multilingual", "spa"])).toBe("es");
  });
});

describe("urlCatalogo", () => {
  it("sin búsqueda: LibriVox del idioma, lo más descargado primero", () => {
    const u = new URL(urlCatalogo("es", "", 2));
    const q = u.searchParams.get("q")!;
    expect(q).toContain("collection:librivoxaudio");
    expect(q).toContain("spa");
    expect(q).not.toContain("eng");
    expect(u.searchParams.getAll("sort[]")).toEqual(["downloads desc"]);
    expect(u.searchParams.get("page")).toBe("2");
  });
  it("con búsqueda: sin orden fijo y sin sintaxis de Lucene", () => {
    const u = new URL(urlCatalogo("todos", 'Quijote: "vol 1"', 1));
    const q = u.searchParams.get("q")!;
    expect(q).toContain("AND (Quijote vol 1)");
    expect(q).toContain("spa");
    expect(q).toContain("eng");
    expect(u.searchParams.getAll("sort[]")).toEqual([]);
  });
});

describe("limpiarBusqueda", () => {
  it("quita operadores y signos", () => {
    expect(limpiarBusqueda("  a AND b (c) -d  ")).toBe("a b c d");
    expect(limpiarBusqueda("title:x*")).toBe("title x");
  });
});

describe("libroDeDoc", () => {
  it("arma el libro con portada e idioma", () => {
    const l = libroDeDoc({
      identifier: "don_quijote_vol1_0706_librivox",
      title: "Don Quijote, Volume 1",
      creator: ["Miguel de Cervantes Saavedra"],
      language: "spa",
    })!;
    expect(l.titulo).toBe("Don Quijote, Volume 1");
    expect(l.autor).toBe("Miguel de Cervantes Saavedra");
    expect(l.idioma).toBe("es");
    expect(l.portada).toBe("https://archive.org/services/img/don_quijote_vol1_0706_librivox");
  });
  it("sin identifier no hay libro", () => {
    expect(libroDeDoc({ title: "x" })).toBeNull();
  });
});

describe("duraciones", () => {
  it("parseDuracion", () => {
    expect(parseDuracion("07:00")).toBe(420);
    expect(parseDuracion("1:02:03")).toBe(3723);
    expect(parseDuracion("420.5")).toBe(420.5);
    expect(parseDuracion("")).toBeNull();
    expect(parseDuracion("x:10")).toBeNull();
    expect(parseDuracion(undefined)).toBeNull();
  });
  it("fmtDuracion y fmtLargo", () => {
    expect(fmtDuracion(3723)).toBe("1:02:03");
    expect(fmtDuracion(420)).toBe("7:00");
    expect(fmtDuracion(null)).toBe("");
    expect(fmtLargo(3723)).toBe("1 h 2 min");
    expect(fmtLargo(7200)).toBe("2 h");
    expect(fmtLargo(20)).toBe("1 min");
  });
});

describe("limpiarHtml", () => {
  it("deja texto con párrafos y entidades resueltas", () => {
    const html = "<p>El ingenioso <i>hidalgo</i>&nbsp;de la Mancha.</p><p>Segunda&#160;parte &amp; m&aacute;s</p>";
    expect(limpiarHtml(html)).toBe("El ingenioso hidalgo de la Mancha.\n\nSegunda parte & más");
  });
});

describe("capitulosDe", () => {
  const archivos = [
    { name: "quijote_02_64kb.mp3", format: "64Kbps MP3", title: "02: Prólogo", length: "27:50", track: "2/3" },
    { name: "quijote_01_64kb.mp3", format: "64Kbps MP3", title: "01: Tasa", length: "07:00", track: "1/3" },
    { name: "quijote_01.mp3", format: "VBR MP3", title: "01: Tasa", length: "07:00" },
    { name: "quijote_03_64kb.mp3", format: "64Kbps MP3", length: "43:41" },
    { name: "cover.jpg", format: "JPEG" },
  ];
  it("un solo formato, en orden de pista", () => {
    const c = capitulosDe("don_quijote", archivos);
    expect(c.map((x) => x.n)).toEqual([1, 2, 3]);
    expect(c[0].titulo).toBe("01: Tasa");
    expect(c[0].segundos).toBe(420);
    expect(c[0].url).toBe("https://archive.org/download/don_quijote/quijote_01_64kb.mp3");
    // Sin título: el nombre del archivo, legible.
    expect(c[2].titulo).toBe("quijote 03 64kb");
  });
  it("sin el formato preferido usa el que haya", () => {
    const c = capitulosDe("x", [
      { name: "b 10.mp3", format: "VBR MP3" },
      { name: "b 9.mp3", format: "VBR MP3" },
    ]);
    expect(c.map((x) => x.titulo)).toEqual(["b 9", "b 10"]);
    expect(c[0].url).toBe("https://archive.org/download/x/b%209.mp3");
  });
});

describe("fichaDeMetadata", () => {
  it("suma la duración y limpia la descripción", () => {
    const f = fichaDeMetadata("libro", {
      metadata: { title: "Libro", creator: "Autor", language: "eng", description: "<p>Hola</p>" },
      files: [
        { name: "a_64kb.mp3", format: "64Kbps MP3", length: "10:00", track: "1" },
        { name: "b_64kb.mp3", format: "64Kbps MP3", length: "5:00", track: "2" },
      ],
    });
    expect(f.idioma).toBe("en");
    expect(f.descripcion).toBe("Hola");
    expect(f.segundosTotal).toBe(900);
    expect(f.capitulos).toHaveLength(2);
  });
});
