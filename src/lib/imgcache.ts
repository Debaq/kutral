// Mapa persistente url→archivo local de pósters ya bajados.
//
// El backend (`cache_image`) guarda cada póster en disco y devuelve el path al
// toque si ya está. El problema era que el mapa vivía solo en memoria: en cada
// arranque la primera pintada volvía a apuntar a image.tmdb.org y recién
// después cambiaba al archivo. Con esto, la primera pintada ya sale del disco.
//
// Guardamos el PATH, no la url convertida: convertFileSrc depende del runtime
// y no tiene por qué ser estable entre versiones de Tauri.

const CLAVE = "kutral:imgs";
// Techo de entradas. localStorage anda por los 5 MB y cada entrada pesa ~120
// bytes; 3000 pósters entran sobrados y evitan que crezca sin fin.
const MAX = 3000;

export function leerImgCache(): [string, string][] {
  try {
    const raw = localStorage.getItem(CLAVE);
    if (!raw) return [];
    const v = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v.filter(
      (e): e is [string, string] =>
        Array.isArray(e) && typeof e[0] === "string" && typeof e[1] === "string",
    );
  } catch {
    return [];
  }
}

export function guardarImgCache(entradas: [string, string][]) {
  try {
    // Map conserva orden de inserción: recortamos por la cola vieja.
    const recortado = entradas.length > MAX ? entradas.slice(-MAX) : entradas;
    localStorage.setItem(CLAVE, JSON.stringify(recortado));
  } catch {
    /* lleno o bloqueado: se sigue usando el mapa en memoria */
  }
}
