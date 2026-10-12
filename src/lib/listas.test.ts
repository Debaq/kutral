import { describe, expect, it, vi, beforeEach } from "vitest";

// DB falsa: solo registra lo que se ejecuta; el estado que importa es el de
// memoria, que es lo que lee la UI.
const ejecutadas: string[] = [];
vi.mock("$lib/vera/db", () => ({
  getDb: async () => ({
    execute: async (sql: string) => {
      ejecutadas.push(sql.trim().split(/\s+/).slice(0, 3).join(" "));
    },
    select: async () => [],
  }),
}));

const { historial } = await import("./historial.svelte");
const L = await import("./listas.svelte");

function meta(clave: string, title = clave) {
  return { clave, tmdbId: 1, mediaType: "movie", title, posterPath: null };
}

function claves(listaId: string): string[] {
  return L.itemsDe(listaId).map((it) => it.clave);
}

beforeEach(() => {
  ejecutadas.length = 0;
  L.listas.todas = [{ id: L.QUIERO_VER, nombre: "Quiero ver", creada: 1, actualizada: 1 }];
  L.listas.items = { [L.QUIERO_VER]: [] };
  historial.porClave = {};
});

describe("listas", () => {
  it("agrega al final y no repite", async () => {
    await L.agregarALista(L.QUIERO_VER, meta("tt1"));
    await L.agregarALista(L.QUIERO_VER, meta("tt2"));
    await L.agregarALista(L.QUIERO_VER, meta("tt1"));
    expect(claves(L.QUIERO_VER)).toEqual(["tt1", "tt2"]);
    expect(L.estaEnLista(L.QUIERO_VER, "tt2")).toBe(true);
  });

  it("alternar quita lo que estaba", async () => {
    await L.alternarEnLista(L.QUIERO_VER, meta("tt1"));
    expect(await L.alternarEnLista(L.QUIERO_VER, meta("tt1"))).toBe(false);
    expect(claves(L.QUIERO_VER)).toEqual([]);
  });

  it("mover intercambia con el vecino y no se sale de los bordes", async () => {
    for (const c of ["a", "b", "c"]) await L.agregarALista(L.QUIERO_VER, meta(c));
    await L.moverEnLista(L.QUIERO_VER, "c", -1);
    expect(claves(L.QUIERO_VER)).toEqual(["a", "c", "b"]);
    await L.moverEnLista(L.QUIERO_VER, "a", -1);
    expect(claves(L.QUIERO_VER)).toEqual(["a", "c", "b"]);
    // Las posiciones quedan en orden: recargar desde la DB daría lo mismo.
    const pos = L.itemsDe(L.QUIERO_VER).map((it) => it.posicion);
    expect([...pos].sort((x, y) => x - y)).toEqual(pos);
  });

  it("siguiente en orden salta lo visto y lo que acaba de terminar", async () => {
    for (const c of ["a", "b", "c", "d"]) await L.agregarALista(L.QUIERO_VER, meta(c));
    const visto = { visto: true, parcial: false, pct: 0, ultimoEp: null, vistos: 1, last_watched: 1 };
    historial.porClave = { a: visto, c: visto };
    expect(L.siguienteEnOrden(L.QUIERO_VER)?.clave).toBe("b");
    expect(L.siguienteEnOrden(L.QUIERO_VER, "b")?.clave).toBe("d");
    expect(L.siguienteEnOrden(L.QUIERO_VER, "d")).toBeNull();
  });

  it("Quiero ver no se borra ni se renombra; las demás sí", async () => {
    await L.borrarLista(L.QUIERO_VER);
    await L.renombrarLista(L.QUIERO_VER, "Otra");
    expect(L.listas.todas.map((l) => l.nombre)).toEqual(["Quiero ver"]);
    const id = await L.crearLista("  Maratón Nolan  ");
    expect(id).toBeTruthy();
    await L.agregarALista(id!, meta("tt9"));
    expect(L.listasCon("tt9")).toEqual([id]);
    L.enOrden.listaId = id;
    await L.borrarLista(id!);
    expect(L.listas.todas.map((l) => l.id)).toEqual([L.QUIERO_VER]);
    expect(L.enOrden.listaId).toBeNull();
  });
});
