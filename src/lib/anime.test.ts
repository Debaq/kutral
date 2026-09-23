import { describe, expect, it } from "vitest";
import { ANIME_GENRES, anilistGenresCSV, animeSeasonOptions } from "./anime";

describe("animeSeasonOptions", () => {
  it("próxima, actual y las anteriores, de más nueva a más vieja", () => {
    // Mayo: primavera.
    const ops = animeSeasonOptions(new Date(2026, 4, 10), 2);
    expect(ops.map((o) => o.id)).toEqual([
      "SUMMER-2026",
      "SPRING-2026",
      "WINTER-2026",
      "FALL-2025",
    ]);
    expect(ops[0].label).toBe("Verano 2026 · próxima");
    expect(ops[1].label).toBe("Primavera 2026 · actual");
    expect(ops[2].label).toBe("Invierno 2026");
  });

  it("cruza el año hacia adelante en otoño", () => {
    const ops = animeSeasonOptions(new Date(2026, 11, 31), 0);
    expect(ops.map((o) => [o.season, o.year])).toEqual([
      ["WINTER", 2027],
      ["FALL", 2026],
    ]);
  });
});

describe("anilistGenresCSV", () => {
  it("traduce ids locales a nombres de AniList", () => {
    const [a, b] = ANIME_GENRES;
    expect(anilistGenresCSV(new Set([a.id, b.id]))).toBe(`${a.anilist},${b.anilist}`);
    expect(anilistGenresCSV(new Set())).toBe("");
  });
});
