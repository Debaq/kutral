// Listas: "Quiero ver" y las que arma el usuario (colecciones temáticas que se
// pueden recorrer en orden).
//
// Fuente de verdad: SQLite (`listas`, `lista_items`), migración v11. Igual que
// historial.svelte.ts, se mantiene una copia en memoria reactiva para pintar la
// ficha y las pestañas sin una query por tarjeta.
//
// "Descubrir en orden": al empezar a recorrer una lista se anota cuál
// (`enOrden`); cuando termina un título de esa lista, la pantalla de fin ofrece
// el siguiente sin ver (ver siguienteEnOrden y alTerminar en +page.svelte).

import { getDb } from "$lib/vera/db";
import { estadoDe, avisarCambio, type MediaMeta } from "$lib/historial.svelte";

export const QUIERO_VER = "quiero-ver";

export type Lista = {
  id: string;
  nombre: string;
  creada: number;
  actualizada: number;
};

export type ItemLista = {
  lista_id: string;
  clave: string;
  tmdb_id: number;
  media_type: string;
  title: string;
  poster_path: string | null;
  posicion: number;
  agregado: number;
};

export const listas = $state({
  cargadas: false,
  /** "Quiero ver" primero; las demás, la tocada más recientemente antes. */
  todas: [] as Lista[],
  /** lista_id → ítems en orden. */
  items: {} as Record<string, ItemLista[]>,
});

/** La lista que se está recorriendo con "Descubrir en orden", o null. */
export const enOrden = $state({ listaId: null as string | null });

function ordenarListas(ls: Lista[]): Lista[] {
  return [...ls].sort((a, b) =>
    a.id === QUIERO_VER ? -1 : b.id === QUIERO_VER ? 1 : b.actualizada - a.actualizada,
  );
}

export async function cargarListas(): Promise<void> {
  const db = await getDb();
  if (!db) {
    listas.cargadas = true;
    return;
  }
  try {
    const ls = await db.select<Lista[]>("SELECT * FROM listas");
    const its = await db.select<ItemLista[]>("SELECT * FROM lista_items ORDER BY posicion, agregado");
    const items: Record<string, ItemLista[]> = {};
    for (const l of ls) items[l.id] = [];
    for (const it of its) (items[it.lista_id] ??= []).push(it);
    listas.todas = ordenarListas(ls);
    listas.items = items;
  } catch (e) {
    console.warn("[listas] no se pudieron cargar:", e);
  }
  listas.cargadas = true;
}

export function itemsDe(listaId: string): ItemLista[] {
  return listas.items[listaId] ?? [];
}

export function nombreDe(listaId: string): string {
  return listas.todas.find((l) => l.id === listaId)?.nombre ?? "";
}

export function estaEnLista(listaId: string, clave: string): boolean {
  return !!clave && itemsDe(listaId).some((it) => it.clave === clave);
}

// Marca la lista como tocada: sube en el orden de la pestaña Listas. Todo
// cambio a una lista pasa por aquí, así que también avisa para sincronizar.
async function tocar(listaId: string): Promise<void> {
  avisarCambio();
  const db = await getDb();
  if (!db) return;
  const ahora = Date.now();
  await db.execute("UPDATE listas SET actualizada = $1 WHERE id = $2", [ahora, listaId]);
  listas.todas = ordenarListas(listas.todas.map((l) => (l.id === listaId ? { ...l, actualizada: ahora } : l)));
}

// randomUUID solo existe en contexto seguro; según la plataforma, el webview
// de Tauri puede no serlo. getRandomValues está siempre.
function nuevoId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const b = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

export async function crearLista(nombre: string): Promise<string | null> {
  const n = nombre.trim().slice(0, 60);
  if (!n) return null;
  const db = await getDb();
  if (!db) return null;
  const id = nuevoId();
  const ahora = Date.now();
  try {
    await db.execute("INSERT INTO listas (id, nombre, creada, actualizada) VALUES ($1, $2, $3, $3)", [
      id,
      n,
      ahora,
    ]);
  } catch (e) {
    console.warn("[listas] crear:", e);
    return null;
  }
  listas.todas = ordenarListas([...listas.todas, { id, nombre: n, creada: ahora, actualizada: ahora }]);
  listas.items = { ...listas.items, [id]: [] };
  avisarCambio();
  return id;
}

export async function renombrarLista(listaId: string, nombre: string): Promise<void> {
  const n = nombre.trim().slice(0, 60);
  if (!n || listaId === QUIERO_VER) return;
  const db = await getDb();
  if (!db) return;
  await db.execute("UPDATE listas SET nombre = $1, actualizada = $2 WHERE id = $3", [n, Date.now(), listaId]);
  listas.todas = listas.todas.map((l) => (l.id === listaId ? { ...l, nombre: n } : l));
  avisarCambio();
}

export async function borrarLista(listaId: string): Promise<void> {
  if (listaId === QUIERO_VER) return;
  const db = await getDb();
  if (!db) return;
  await db.execute("DELETE FROM lista_items WHERE lista_id = $1", [listaId]);
  await db.execute("DELETE FROM listas WHERE id = $1", [listaId]);
  listas.todas = listas.todas.filter((l) => l.id !== listaId);
  const copia = { ...listas.items };
  delete copia[listaId];
  listas.items = copia;
  if (enOrden.listaId === listaId) enOrden.listaId = null;
  avisarCambio();
}

/** Agrega al final de la lista. */
export async function agregarALista(listaId: string, meta: MediaMeta): Promise<void> {
  if (!meta.clave || estaEnLista(listaId, meta.clave)) return;
  const db = await getDb();
  if (!db) return;
  const actuales = itemsDe(listaId);
  const it: ItemLista = {
    lista_id: listaId,
    clave: meta.clave,
    tmdb_id: meta.tmdbId,
    media_type: meta.mediaType,
    title: meta.title,
    poster_path: meta.posterPath ?? null,
    posicion: actuales.length ? actuales[actuales.length - 1].posicion + 1 : 1,
    agregado: Date.now(),
  };
  try {
    await db.execute(
      `INSERT OR IGNORE INTO lista_items
         (lista_id, clave, tmdb_id, media_type, title, poster_path, posicion, agregado)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
      [it.lista_id, it.clave, it.tmdb_id, it.media_type, it.title, it.poster_path, it.posicion, it.agregado],
    );
  } catch (e) {
    console.warn("[listas] agregar:", e);
    return;
  }
  listas.items = { ...listas.items, [listaId]: [...actuales, it] };
  await tocar(listaId);
}

export async function quitarDeLista(listaId: string, clave: string): Promise<void> {
  const db = await getDb();
  if (!db) return;
  await db.execute("DELETE FROM lista_items WHERE lista_id = $1 AND clave = $2", [listaId, clave]);
  listas.items = { ...listas.items, [listaId]: itemsDe(listaId).filter((it) => it.clave !== clave) };
  await tocar(listaId);
}

export async function alternarEnLista(listaId: string, meta: MediaMeta): Promise<boolean> {
  if (estaEnLista(listaId, meta.clave)) {
    await quitarDeLista(listaId, meta.clave);
    return false;
  }
  await agregarALista(listaId, meta);
  return true;
}

/** Cambia de lugar con el vecino (`delta` -1 = antes, +1 = después). */
export async function moverEnLista(listaId: string, clave: string, delta: -1 | 1): Promise<void> {
  const its = [...itemsDe(listaId)];
  const i = its.findIndex((it) => it.clave === clave);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= its.length) return;
  const db = await getDb();
  if (!db) return;
  const a = { ...its[i], posicion: its[j].posicion };
  const b = { ...its[j], posicion: its[i].posicion };
  // Posiciones repetidas (no debería, pero una lista vieja podría tenerlas):
  // sin esto el intercambio no movería nada.
  if (a.posicion === b.posicion) a.posicion += delta * 0.5;
  await db.execute("UPDATE lista_items SET posicion = $1 WHERE lista_id = $2 AND clave = $3", [
    a.posicion,
    listaId,
    a.clave,
  ]);
  await db.execute("UPDATE lista_items SET posicion = $1 WHERE lista_id = $2 AND clave = $3", [
    b.posicion,
    listaId,
    b.clave,
  ]);
  its[j] = a;
  its[i] = b;
  listas.items = { ...listas.items, [listaId]: its };
  avisarCambio();
}

/** Listas (sin "Quiero ver") que contienen el título. */
export function listasCon(clave: string): string[] {
  return listas.todas.filter((l) => l.id !== QUIERO_VER && estaEnLista(l.id, clave)).map((l) => l.id);
}

/**
 * Lo que toca descubrir en una lista: el primer título sin ver después de
 * `despuesDe` (o desde el principio). Los ya vistos se saltan.
 */
export function siguienteEnOrden(listaId: string, despuesDe?: string): ItemLista | null {
  const its = itemsDe(listaId);
  const desde = despuesDe ? its.findIndex((it) => it.clave === despuesDe) + 1 : 0;
  return its.slice(desde).find((it) => it.clave !== despuesDe && !estadoDe(it.clave)?.visto) ?? null;
}
