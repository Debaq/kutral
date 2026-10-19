// Guardado de /config: no hay botón, cada cambio se persiste solo.
//
// La página escucha `change` en el contenedor de las pestañas y llama a
// `guardar()`; como `change` burbujea después de que el bind del control ya
// escribió en `config`, basta con eso para radios, casillas, selects y sliders.
// Los campos de texto y números no se atan directo a `config` (cada tecla
// cambiaría la key de TMDb a medias, por ejemplo): su propio `onchange` limpia
// el valor y lo escribe, y el contenedor guarda justo después.

import { config, saveConfig, CACHE_WAIT_MAX } from "$lib/config.svelte";

export const guardado = $state({ visible: false });

let ultimo = "";
let timer: ReturnType<typeof setTimeout> | null = null;

export function guardar() {
  // El tope de la subida automática nunca por debajo de la espera base.
  config.cacheWaitMax = Math.min(CACHE_WAIT_MAX, Math.max(config.cacheWait, config.cacheWaitMax));
  // Los campos que no son de config (cuenta, IP de la TV…) también disparan
  // change: si nada cambió no se escribe ni se avisa a la sincronización.
  const ahora = JSON.stringify(config);
  if (ahora === ultimo) return;
  ultimo = ahora;
  saveConfig();
  guardado.visible = true;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => (guardado.visible = false), 1600);
}

/** Valor de un campo de texto, sin espacios en los bordes. */
export function texto(e: Event): string {
  return (e.currentTarget as HTMLInputElement).value.trim();
}

/**
 * Valor de un campo numérico dentro de [min, max]; vacío o inválido = `porDefecto`.
 * Reescribe el campo con lo que quedó, porque si el valor de config no cambia
 * Svelte no lo repinta y se vería lo que el usuario escribió.
 */
export function numero(e: Event, min: number, max: number, porDefecto: number, entero = true): number {
  const el = e.currentTarget as HTMLInputElement;
  const n = Number(el.value);
  const v = el.value.trim() !== "" && Number.isFinite(n)
    ? Math.min(max, Math.max(min, entero ? Math.round(n) : n))
    : porDefecto;
  el.value = String(v);
  return v;
}

export function fmtMmSs(s: number): string {
  const m = Math.floor(s / 60);
  const r = (s % 60).toString().padStart(2, "0");
  return `${m}:${r}`;
}

export function haceCuanto(ms: number): string {
  if (!ms) return "todavía no";
  const min = Math.round((Date.now() - ms) / 60_000);
  if (min < 1) return "hace un momento";
  if (min < 60) return `hace ${min} min`;
  return new Date(ms).toLocaleString();
}
