import { describe, expect, it } from "vitest";
import { avisoVencimiento } from "./avisoRd";

const DIA = 86_400;

describe("avisoVencimiento", () => {
  it("calla con más de 7 días", () => {
    expect(avisoVencimiento("premium", 8 * DIA)).toBeNull();
    expect(avisoVencimiento("premium", 7 * DIA + 1)).toBeNull();
  });

  it("avisa desde 7 días antes", () => {
    expect(avisoVencimiento("premium", 7 * DIA)?.titulo).toBe("Tu Real-Debrid vence en 7 días");
    expect(avisoVencimiento("premium", 2 * DIA + 5)?.titulo).toBe("Tu Real-Debrid vence en 3 días");
    expect(avisoVencimiento("premium", DIA + 5)?.titulo).toBe("Tu Real-Debrid vence en 2 días");
    expect(avisoVencimiento("premium", DIA)?.titulo).toBe("Tu Real-Debrid vence mañana");
    expect(avisoVencimiento("premium", 3600)?.titulo).toBe("Tu Real-Debrid vence hoy");
  });

  it("sigue avisando ya vencido", () => {
    expect(avisoVencimiento("free", 0)?.titulo).toBe("Tu Real-Debrid no tiene premium");
    expect(avisoVencimiento("premium", 0)?.titulo).toBe("Tu Real-Debrid no tiene premium");
  });
});
