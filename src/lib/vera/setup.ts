// Perfil persistente del usuario: tabla `vera_setup` (fila única, id=1).
//
// El esquema v3 (migración 3 en src-tauri/src/lib.rs) ya existía completo y
// nadie lo escribía. Esto lo conecta.
//
// Columnas que Vera usa hoy:
//   platforms, excluded_genres, excluded_themes, personality,
//   languages_known (idiomas cómodos), languages_avoid (idiomas que no
//   tolera), animation_pref, runtime_max
// Columnas del esquema que todavía no tienen UI — se escriben con un valor
// neutro porque son NOT NULL, y quedan listas para cuando haya pantalla:
//   mode_io ('teclado'), dub_pref ('indiferente')
//
// runtime_max llegó en la migración 6. Antes el tope de duración iba embutido
// en depth_profile como "auto:<minutos>", que era un parche para no migrar por
// un entero. La lectura acepta las dos formas para no perder los perfiles ya
// guardados con el parche; la escritura usa siempre la columna nueva.

import { invoke } from "@tauri-apps/api/core";
import { getDb, jsonArray } from "./db";
import type { PerfilSetup, Personalidad, PrefAnimacion } from "./tipos";

// Lo que devuelven vera_platform_list / vera_genre_list / vera_theme_list.
export interface OpcionCatalogo {
  id: string;
  label: string;
  description: string | null;
}

const PERSONALIDADES: Personalidad[] = ["calida", "directa", "seca"];

function comoPersonalidad(v: unknown): Personalidad {
  return PERSONALIDADES.includes(v as Personalidad)
    ? (v as Personalidad)
    : "calida";
}

const ANIMACIONES: PrefAnimacion[] = ["gusta", "indiferente", "no"];

function comoAnimacion(v: unknown): PrefAnimacion {
  return ANIMACIONES.includes(v as PrefAnimacion)
    ? (v as PrefAnimacion)
    : "indiferente";
}

// Tope de duración: columna runtime_max, o el parche viejo "auto:<min>" en
// depth_profile para los perfiles guardados antes de la migración 6.
function duracion(runtimeMax: unknown, depth: unknown): number | null {
  const n = Number(runtimeMax);
  if (Number.isFinite(n) && n > 0) return n;
  if (typeof depth !== "string") return null;
  const [, mins] = depth.split(":");
  const m = Number(mins);
  return Number.isFinite(m) && m > 0 ? m : null;
}

interface FilaSetup {
  platforms: string;
  excluded_genres: string;
  excluded_themes: string;
  personality: string;
  depth_profile: string;
  languages_known: string;
  languages_avoid: string;
  animation_pref: string;
  runtime_max: number | null;
}

// Perfil guardado, o null si el usuario nunca configuró a Vera.
// null NO es error: Vera funciona sin perfil, solo sin exclusiones.
export async function getSetup(): Promise<PerfilSetup | null> {
  const db = await getDb();
  if (!db) return null;
  try {
    const filas = await db.select<FilaSetup[]>(
      `SELECT platforms, excluded_genres, excluded_themes, personality,
              depth_profile, languages_known, languages_avoid,
              animation_pref, runtime_max
         FROM vera_setup
        WHERE id = 1`,
    );
    const f = filas[0];
    if (!f) return null;
    return {
      plataformas: jsonArray(f.platforms),
      generosExcluidos: jsonArray(f.excluded_genres),
      temasExcluidos: jsonArray(f.excluded_themes),
      personalidad: comoPersonalidad(f.personality),
      duracionMax: duracion(f.runtime_max, f.depth_profile),
      animacion: comoAnimacion(f.animation_pref),
      idiomasComodos: jsonArray(f.languages_known),
      idiomasEvitados: jsonArray(f.languages_avoid),
    };
  } catch (e) {
    console.warn("[vera/setup] no se pudo leer el perfil:", e);
    return null;
  }
}

// Guarda (o reemplaza) el perfil. Devuelve false si la DB no está disponible,
// para que la UI pueda avisar en vez de fingir que guardó.
export async function guardarSetup(p: PerfilSetup): Promise<boolean> {
  const db = await getDb();
  if (!db) return false;
  try {
    await db.execute(
      `INSERT INTO vera_setup
          (id, mode_io, depth_profile, dub_pref,
           platforms, excluded_genres, excluded_themes, personality,
           languages_known, languages_avoid, animation_pref, runtime_max,
           completed_at)
       VALUES (1, 'teclado', 'auto', 'indiferente',
               $1, $2, $3, $4, $5, $6, $7, $8, $9)
       ON CONFLICT(id) DO UPDATE SET
          platforms       = excluded.platforms,
          excluded_genres = excluded.excluded_genres,
          excluded_themes = excluded.excluded_themes,
          personality     = excluded.personality,
          languages_known = excluded.languages_known,
          languages_avoid = excluded.languages_avoid,
          animation_pref  = excluded.animation_pref,
          runtime_max     = excluded.runtime_max,
          completed_at    = excluded.completed_at`,
      [
        JSON.stringify(p.plataformas),
        JSON.stringify(p.generosExcluidos),
        JSON.stringify(p.temasExcluidos),
        p.personalidad,
        JSON.stringify(p.idiomasComodos),
        JSON.stringify(p.idiomasEvitados),
        p.animacion,
        p.duracionMax,
        Date.now(),
      ],
    );
    return true;
  } catch (e) {
    console.warn("[vera/setup] no se pudo guardar el perfil:", e);
    return false;
  }
}

// Listas canónicas de opciones. Viven en Rust (una sola fuente de verdad
// compartida con el importador de catálogo) — acá solo se leen.
// Si el invoke falla (fuera de Tauri), devolvemos [] y la UI esconde el paso.
async function listar(comando: string): Promise<OpcionCatalogo[]> {
  try {
    return await invoke<OpcionCatalogo[]>(comando);
  } catch (e) {
    console.warn(`[vera/setup] ${comando} falló:`, e);
    return [];
  }
}

export const listarPlataformas = () => listar("vera_platform_list");
export const listarGeneros = () => listar("vera_genre_list");
export const listarTemas = () => listar("vera_theme_list");
