// Perfiles: quién está usando la app en este equipo (src-tauri/src/perfiles.rs).
//
// Viven en SQLite (`perfiles`), así que elegir uno nunca necesita red; con
// cuenta, cuenta.rs los sincroniza (los nuevos, los cambios y los borrados).
// Cambiar de perfil recarga historial, favoritos y listas.

import { invoke } from "@tauri-apps/api/core";
import { getDb } from "$lib/vera/db";
import { cargarHistorial, avisarCambio } from "$lib/historial.svelte";
import { cargarListas } from "$lib/listas.svelte";
import { PRINCIPAL, perfilActual, fijarPerfil } from "$lib/perfilActivo";

export { PRINCIPAL };

export type Perfil = {
  id: string;
  nombre: string;
  color: string;
  /** Link a la carita (personaje o póster). Vacío = la inicial sobre el color. */
  avatar: string;
  /** Perfil de niños: el catálogo se filtra por edad (ver ninos.ts). SQLite lo da como 0/1. */
  infantil: number | boolean;
  creado: number;
  actualizado: number;
};

/** Lo que se edita de un perfil. */
export type DatosPerfil = { nombre: string; color: string; avatar: string; infantil: boolean };

export const COLORES = ["#f5c518", "#ff7a33", "#e5484d", "#d6409f", "#8e4ec6", "#3e63dd", "#12a594", "#46a758"];

export const perfiles = $state({
  cargados: false,
  lista: [] as Perfil[],
  actual: perfilActual(),
  /** Ya se eligió quién está viendo en esta sesión (o no hacía falta). */
  elegido: false,
  /** La pantalla "¿Quién está viendo?" está abierta. */
  abierta: false,
  /** Perfil a configurar al abrir (bienvenida: nombre y carita del único perfil). */
  configurar: null as string | null,
});

// Con un solo perfil no se pregunta quién ve, pero el que viene de fábrica
// ("Principal", sin carita) se configura una vez: nombre y carita. Cancelar
// también cuenta, no se vuelve a pedir.
const K_CONFIGURADO = "kutral_perfil_configurado";

function yaConfigurado(): boolean {
  try {
    return localStorage.getItem(K_CONFIGURADO) === "1";
  } catch {
    return true;
  }
}

export function marcarConfigurado(): void {
  perfiles.configurar = null;
  try {
    localStorage.setItem(K_CONFIGURADO, "1");
  } catch {
    /* sin storage: se volvería a pedir la próxima vez, nada grave */
  }
}

export function perfilDe(id: string): Perfil | null {
  return perfiles.lista.find((p) => p.id === id) ?? null;
}

/** El perfil que está viendo es de niños. Reactivo: lo leen el catálogo y las sugerencias. */
export function perfilInfantil(): boolean {
  return !!perfilDe(perfiles.actual)?.infantil;
}

/** Lee los perfiles. La primera vez decide si hay que preguntar quién ve. */
export async function cargarPerfiles(): Promise<void> {
  const db = await getDb();
  if (db) {
    try {
      perfiles.lista = await db.select<Perfil[]>("SELECT * FROM perfiles ORDER BY creado");
    } catch (e) {
      console.warn("[perfiles] no se pudieron cargar:", e);
    }
  }
  // El activo ya no existe (lo borraron en otro equipo): vuelve al principal.
  if (perfiles.lista.length && !perfilDe(perfiles.actual)) await elegirPerfil(PRINCIPAL);
  if (!perfiles.cargados) {
    perfiles.cargados = true;
    perfiles.elegido = perfiles.lista.length <= 1;
    perfiles.abierta = !perfiles.elegido;
    const unico = perfiles.lista.length === 1 ? perfiles.lista[0] : null;
    if (unico && !yaConfigurado() && unico.nombre === "Principal" && !unico.avatar) {
      perfiles.configurar = unico.id;
      perfiles.abierta = true;
    }
  }
}

export async function elegirPerfil(id: string): Promise<void> {
  const cambia = id !== perfiles.actual;
  fijarPerfil(id);
  perfiles.actual = id;
  perfiles.elegido = true;
  perfiles.abierta = false;
  if (cambia) {
    await cargarHistorial();
    await cargarListas();
  }
}

export function abrirPerfiles(): void {
  perfiles.abierta = true;
}

/** Cierra "¿Quién está viendo?" sin cambiar (solo si ya había uno elegido). */
export function cerrarPerfiles(): void {
  if (perfiles.elegido) perfiles.abierta = false;
}

function nuevoId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const b = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

export async function crearPerfil(d: DatosPerfil): Promise<string | null> {
  const n = d.nombre.trim().slice(0, 40);
  const db = await getDb();
  if (!n || !db) return null;
  const id = nuevoId();
  const ahora = Date.now();
  await db.execute(
    "INSERT INTO perfiles (id, nombre, color, avatar, infantil, creado, actualizado) VALUES ($1, $2, $3, $4, $5, $6, $6)",
    [id, n, d.color, d.avatar, d.infantil ? 1 : 0, ahora],
  );
  // Todo perfil nace con su "Quiero ver" (las listas son por perfil).
  await db.execute(
    "INSERT OR IGNORE INTO listas (perfil, id, nombre, creada, actualizada) VALUES ($1, 'quiero-ver', 'Quiero ver', $2, $2)",
    [id, ahora],
  );
  avisarCambio();
  await cargarPerfiles();
  return id;
}

export async function editarPerfil(id: string, d: DatosPerfil): Promise<void> {
  const n = d.nombre.trim().slice(0, 40);
  const db = await getDb();
  if (!n || !db) return;
  await db.execute(
    "UPDATE perfiles SET nombre = $2, color = $3, avatar = $4, infantil = $5, actualizado = $6 WHERE id = $1",
    [id, n, d.color, d.avatar, d.infantil ? 1 : 0, Date.now()],
  );
  avisarCambio();
  await cargarPerfiles();
}

/** Borra el perfil con todo lo suyo (con cuenta, también en el servidor). */
export async function borrarPerfil(id: string): Promise<void> {
  await invoke("perfil_borrar", { id });
  if (perfiles.actual === id) await elegirPerfil(PRINCIPAL);
  await cargarPerfiles();
}
