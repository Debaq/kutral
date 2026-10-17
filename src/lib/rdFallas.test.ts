import { describe, expect, it, vi, beforeEach } from "vitest";

const almacen = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => almacen.get(k) ?? null,
  setItem: (k: string, v: string) => void almacen.set(k, v),
  removeItem: (k: string) => void almacen.delete(k),
});

const { clasificarFallaRd, anotarFallaRd, fallaRd, olvidarFallaRd, rdFallas } = await import(
  "./rdFallas.svelte"
);

const H = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";

describe("fallas de RD", () => {
  beforeEach(() => {
    for (const k of Object.keys(rdFallas)) delete rdFallas[k];
    vi.useRealTimers();
  });

  it("solo anota lo que es culpa de la fuente", () => {
    expect(clasificarFallaRd("BLOQUEADO_DMCA: RD 451 :: {}")?.tipo).toBe("dmca");
    expect(clasificarFallaRd("RD torrent falló: dead")?.tipo).toBe("muerta");
    expect(clasificarFallaRd("RD no cacheado / timeout")?.tipo).toBe("sin_cache");
    expect(clasificarFallaRd("el pack no trae T1E5")?.tipo).toBe("sin_capitulo");
    // Red, token o RD caído: nada que ver con la fuente.
    expect(clasificarFallaRd("addMagnet red: connection reset")).toBeNull();
    expect(clasificarFallaRd("addMagnet 503 Service Unavailable: x")).toBeNull();
    expect(clasificarFallaRd("Vuelve a vincular Real-Debrid")).toBeNull();
    expect(anotarFallaRd(H, "", "info 502 Bad Gateway")).toBeNull();
    expect(fallaRd(H)).toBeNull();
  });

  it("el bloqueo es del torrent; la falta de capítulo, solo del capítulo", () => {
    anotarFallaRd(H, "T1E5", "el pack no trae T1E5");
    expect(fallaRd(H, "T1E5")?.corto).toBe("sin este capítulo");
    expect(fallaRd(H, "T1E6")).toBeNull();
    anotarFallaRd(H, "T1E6", "BLOQUEADO_DMCA: RD 451 al desbloquear");
    // Mayúsculas o minúsculas, el hash es el mismo.
    expect(fallaRd(H.toLowerCase(), "T2E1")?.tipo).toBe("dmca");
    expect(JSON.parse(almacen.get("rd_fallas")!)).toHaveProperty(H.toLowerCase());
  });

  it("vence a su tiempo y se puede olvidar", () => {
    vi.useFakeTimers();
    anotarFallaRd(H, "", "RD no cacheado / timeout");
    vi.advanceTimersByTime(23 * 3600_000);
    expect(fallaRd(H)?.tipo).toBe("sin_cache");
    vi.advanceTimersByTime(2 * 3600_000);
    expect(fallaRd(H)).toBeNull();
    anotarFallaRd(H, "", "RD torrent falló: virus");
    olvidarFallaRd(H);
    expect(fallaRd(H)).toBeNull();
  });
});
