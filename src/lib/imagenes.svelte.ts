// Pósters y fondos servidos desde el cache WebP local.
//
// img()/art() devuelven la URL a pintar: el archivo del disco si ya está
// bajado, la remota si no (y de paso piden bajarla). El mapa vive a nivel de
// módulo, así que sobrevive a que el catálogo se desmonte al ir a otra ruta.
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { leerImgCache, guardarImgCache } from "$lib/imgcache";

export const IMG = "https://image.tmdb.org/t/p";

// Se hidrata de localStorage: sin esto, cada arranque pintaba primero desde
// image.tmdb.org y recién después cambiaba al archivo del disco, aunque el
// archivo ya estuviera ahí. Guardamos el path crudo y convertimos al leer.
let cachedImgs = $state<Map<string, string>>(new Map(leerImgCache()));
const cacheInflight = new Set<string>();
let imgPersistTimer: ReturnType<typeof setTimeout> | null = null;

// Escribir en cada póster bajado sería un JSON.stringify por imagen: se
// agenda una sola pasada cuando la ráfaga termina.
function persistirImgCache() {
  if (imgPersistTimer) clearTimeout(imgPersistTimer);
  imgPersistTimer = setTimeout(() => {
    imgPersistTimer = null;
    guardarImgCache(Array.from(cachedImgs));
  }, 1500);
}

/** Vuelca ya lo pendiente: los pósters bajados en esta pantalla no se pierden. */
export function volcarImgCache() {
  if (!imgPersistTimer) return;
  clearTimeout(imgPersistTimer);
  imgPersistTimer = null;
  guardarImgCache(Array.from(cachedImgs));
}

async function ensureCached(url: string, maxW: number) {
  const key = `${url}::${maxW}`;
  if (cachedImgs.has(key) || cacheInflight.has(key)) return;
  cacheInflight.add(key);
  try {
    const path = await invoke<string>("cache_image", { url, maxW });
    const m = new Map(cachedImgs);
    m.set(key, path);
    cachedImgs = m;
    persistirImgCache();
  } catch (e) {
    // Si falla, dejamos URL original
  } finally {
    cacheInflight.delete(key);
  }
}

export function img(url: string | null | undefined, maxW: number): string {
  if (!url) return "";
  const key = `${url}::${maxW}`;
  const cached = cachedImgs.get(key);
  if (cached) return convertFileSrc(cached);
  ensureCached(url, maxW);
  return url;
}

// El archivo cacheado puede no estar (limpieza del sistema, borrado manual).
// Sacamos la entrada y el re-render vuelve solo a la URL remota, que a su vez
// dispara ensureCached de nuevo.
export function onImgError(e: Event) {
  const src = (e.currentTarget as HTMLImageElement | null)?.src;
  if (!src || !src.includes("asset")) return;
  for (const [k, path] of cachedImgs) {
    if (convertFileSrc(path) !== src) continue;
    const m = new Map(cachedImgs);
    m.delete(k);
    cachedImgs = m;
    persistirImgCache();
    return;
  }
}

// Como img() pero acepta fragmento TMDb ("/abc.jpg") O URL completa:
// AniList/ani.zip mandan https://… directo, sin base TMDb.
export function art(path: string | null | undefined, size: string, maxW: number): string {
  if (!path) return "";
  return path.startsWith("http") ? img(path, maxW) : img(`${IMG}/${size}${path}`, maxW);
}
