// Handle SQLite compartido por los módulos de Vera.
//
// Antes cada módulo abría el suyo (historial.ts tenía un singleton propio).
// Ahora que setup, catálogo local y aprendizaje también pegan a la misma DB,
// tener un solo handle evita N conexiones al mismo archivo.
//
// Si la DB no se puede abrir (fuera de Tauri, permisos), devuelve null y cada
// consumidor degrada: Vera funciona sin perfil, sin catálogo local y sin
// aprendizaje, solo con TMDb en vivo + localStorage.

import Database from "@tauri-apps/plugin-sql";

let dbCache: Database | null = null;
let dbIntentado = false;

export async function getDb(): Promise<Database | null> {
  if (dbCache) return dbCache;
  // La mayoría de las fallas (no-Tauri, permisos) son permanentes en la
  // sesión: no reintentamos en cada llamada.
  if (dbIntentado) return null;
  dbIntentado = true;
  try {
    dbCache = await Database.load("sqlite:kutral.db");
    return dbCache;
  } catch (e) {
    console.warn("[vera/db] DB no disponible:", e);
    return null;
  }
}

// Parseo tolerante de las columnas TEXT que guardan JSON arrays.
// vera_titles y vera_setup guardan '[]' por default, pero una fila escrita a
// mano o una migración a medias puede traer NULL o basura: nunca reventar.
export function jsonArray(raw: unknown): string[] {
  if (typeof raw !== "string" || raw === "") return [];
  try {
    const v = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}
