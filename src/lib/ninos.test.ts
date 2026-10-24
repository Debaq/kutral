import { describe, expect, it } from "vitest";
import { aptoNinos } from "./ninos";

describe("aptoNinos", () => {
  it("películas: solo las familiares", () => {
    expect(aptoNinos({ genre_ids: [16, 10751, 35] }, "movie")).toBe(true); // Toy Story
    // Animación para adultos (Sausage Party) no tiene familia.
    expect(aptoNinos({ genre_ids: [16, 35] }, "movie")).toBe(false);
    expect(aptoNinos({ genre_ids: [10751, 27] }, "movie")).toBe(false);
  });

  it("series: infantiles o familiares, sin crimen ni misterio", () => {
    expect(aptoNinos({ genre_ids: [10762, 16] }, "tv")).toBe(true);
    expect(aptoNinos({ genre_ids: [10751, 35] }, "tv")).toBe(true);
    // Rick and Morty: animación, comedia y ciencia ficción.
    expect(aptoNinos({ genre_ids: [16, 35, 10765] }, "tv")).toBe(false);
    expect(aptoNinos({ genre_ids: [10751, 9648] }, "tv")).toBe(false);
  });

  it("sin géneros no pasa", () => {
    expect(aptoNinos({}, "movie")).toBe(false);
    expect(aptoNinos({ genre_ids: [] }, "tv")).toBe(false);
  });
});
