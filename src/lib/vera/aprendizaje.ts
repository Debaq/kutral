// Memoria de largo plazo de Vera: tablas `vera_weights` y `vera_feedback`.
//
// Por qué existe si ya hay perfil histórico:
// `historial.getPerfilHistorico()` recalcula pesos de género en cada sesión
// cruzando reacciones con `vera_generos_cache` (localStorage). Eso tiene dos
// techos: solo sabe de géneros, y se evapora si el usuario limpia el navegador
// o si la peli nunca pasó por Vera. Acá los pesos viven en SQLite, sobreviven,
// y cubren director, reparto, década y tono además del género.
//
// MODELO INCREMENTAL — la parte delicada.
// vera_weights acumula deltas, no totales. Cada vez que cambia una reacción se
// aplica (pesoNuevo - pesoAnterior) a cada etiqueta de esa peli. Por eso
// `registrarCambio` recibe los DOS pesos: si aplicáramos solo el nuevo, volver
// a calificar la misma peli la contaría dos veces y una peli editada tres
// veces pesaría el triple que una calificada una vez.
//
// Espacio de etiquetas (prefijo obligatorio, para que no colisionen entre sí):
//   g:<slug>      género v3            g:drama
//   dir:<nombre>  director             dir:Denis Villeneuve
//   act:<nombre>  actor del top 4      act:Florence Pugh
//   dec:<década>  década de estreno    dec:1990
//   tono:<tono>   liviano | denso      tono:denso

import { getDb } from "./db";
import { slugsDesdeNombres } from "./generos";
import type { Pelicula } from "./tipos";

// Cuánto pesa cada familia de etiqueta respecto al género.
// El género es la señal más gruesa y confiable (1.0). Director y actor son más
// específicos: cuando aciertan, aciertan mucho — pero hay menos muestras, así
// que un solo +5 no debería dominar el ranking entero.
const PESO_FAMILIA: Record<string, number> = {
  g: 1.0,
  dir: 0.8,
  act: 0.4,
  dec: 0.3,
  tono: 0.7,
};

export function familiaDeTag(tag: string): string {
  const i = tag.indexOf(":");
  return i === -1 ? "" : tag.slice(0, i);
}

export function pesoDeFamilia(tag: string): number {
  return PESO_FAMILIA[familiaDeTag(tag)] ?? 0;
}

// Etiquetas de una peli. Solo las que tienen dato: una peli sin enriquecer no
// conoce a su director, y meter "dir:" vacío ensuciaría la tabla.
export function etiquetasDe(p: Pelicula): string[] {
  const tags: string[] = [];
  for (const slug of slugsDesdeNombres(p.generos)) tags.push(`g:${slug}`);
  if (p.director) tags.push(`dir:${p.director}`);
  for (const a of p.actores.slice(0, 4)) tags.push(`act:${a}`);
  const anio = parseInt(p.anio, 10);
  if (Number.isFinite(anio)) tags.push(`dec:${Math.floor(anio / 10) * 10}`);
  tags.push(`tono:${p.tono}`);
  return tags;
}

interface FilaPeso {
  tag: string;
  weight: number;
}

// Todos los pesos aprendidos. Map vacío si no hay DB — el motor degrada al
// perfil de localStorage sin enterarse.
export async function getPesosAprendidos(): Promise<Map<string, number>> {
  const out = new Map<string, number>();
  const db = await getDb();
  if (!db) return out;
  try {
    const filas = await db.select<FilaPeso[]>(
      `SELECT tag, weight FROM vera_weights`,
    );
    for (const f of filas) out.set(f.tag, f.weight);
  } catch (e) {
    console.warn("[vera/aprendizaje] no se pudieron leer pesos:", e);
  }
  return out;
}

// Aplica el delta de una reacción que cambió.
// `pesoAnterior` es 0 la primera vez que se califica una peli.
// Si el delta es 0 no se toca la DB (recalificar de +3 a +3 no es un evento).
export async function registrarCambio(
  p: Pelicula,
  pesoAnterior: number,
  pesoNuevo: number,
): Promise<void> {
  const delta = pesoNuevo - pesoAnterior;
  if (delta === 0) return;
  const db = await getDb();
  if (!db) return;
  const ahora = Date.now();
  try {
    for (const tag of etiquetasDe(p)) {
      await db.execute(
        `INSERT INTO vera_weights (tag, weight, updated_at)
         VALUES ($1, $2, $3)
         ON CONFLICT(tag) DO UPDATE SET
            weight = vera_weights.weight + excluded.weight,
            updated_at = excluded.updated_at`,
        [tag, delta, ahora],
      );
    }
  } catch (e) {
    console.warn("[vera/aprendizaje] no se pudo registrar el cambio:", e);
  }
}

// Registra que el usuario eligió esta peli en el ranking ("Descubrir").
// `rating` es su interés/juicio en ese momento (-5..+5); no es la nota final,
// es qué tan convencido salió. `finished` queda null: quien sabe si terminó
// es watch_history, que escribe la home, no Vera.
export async function registrarEleccion(
  p: Pelicula,
  rating: number,
): Promise<void> {
  if (!p.imdbId) return; // la tabla tiene FK a vera_titles(imdb_id)
  const db = await getDb();
  if (!db) return;
  try {
    await db.execute(
      `INSERT INTO vera_feedback (imdb_id, response_id, rating, finished, why_not, created_at)
       VALUES ($1, NULL, $2, NULL, NULL, $3)`,
      [p.imdbId, Math.round(rating), Date.now()],
    );
  } catch (e) {
    // FK violation esperada si la peli no está en el catálogo local importado.
    console.warn("[vera/aprendizaje] no se pudo registrar la elección:", e);
  }
}
