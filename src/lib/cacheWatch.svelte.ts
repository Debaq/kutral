// Vigilancia del cacheo de red durante la reproducción.
//
// El síntoma: con red mala el video corta cada pocos segundos. La causa no es
// que el cache sea chico — es que mpv reanuda demasiado pronto. Su
// `cache-pause-wait` default es 1 segundo: se vacía el buffer, junta 1s y sigue,
// así que se vuelve a vaciar enseguida. Subirlo convierte veinte micro-cortes
// en una pausa larga y después fluidez.
//
// Pero hay dos problemas distintos que en pantalla se ven igual:
//
//   1. El caudal alcanza, pero es irregular (picos, wifi lejos). Acá subir la
//      espera ARREGLA: se junta más colchón y aguanta los baches.
//   2. El caudal NO alcanza para el bitrate del archivo (pocos seeds, ADSL).
//      Acá ningún buffer sirve: solo retrasa el corte. La única salida real es
//      bajarla antes de verla.
//
// Se distinguen midiendo: `cache-speed` (bytes/s que entran) contra el bitrate
// del archivo (tamaño / duración). Por eso no hace falta ningún ping — un ping
// bueno con pocos seeds miente, y esta comparación no.
//
// Portable a propósito: lee y escribe por comandos que existen en Linux (embed)
// y en Windows (mpv de proceso, por IPC). Ver docs/TODO.md, "Player en Windows".

import { invoke } from "@tauri-apps/api/core";
import { config, cachePresetActual } from "$lib/config.svelte";

type CacheStats = {
  running: boolean;
  paused_for_cache: boolean;
  cache_secs: number;
  cache_speed: number;
  duration: number;
  pos: number;
  file_size: number;
  pause_wait: number;
};

export type MotivoRendicion = {
  /** "caudal" = no alcanza ni con buffer. "tope" = ya se subió todo lo posible. */
  tipo: "caudal" | "tope";
  /** Bytes/s que pide el archivo. */
  necesario: number;
  /** Bytes/s medidos. */
  medido: number;
  /** Segundo en el que iba el video, para poder retomar ahí. */
  pos: number;
};

type Opts = {
  /** Viene del swarm (descarga local): el caudal lo mandan los seeds. */
  desdeTorrent: boolean;
  /** Tamaño del release, por si el stream no declara Content-Length. */
  sizeBytes: number | null;
  /** La red no da: hay que ofrecerle al usuario bajarla y verla después. */
  onRendirse: (m: MotivoRendicion) => void;
  /**
   * El usuario ya eligió "seguir igual": se pone la espera en el tope y no se
   * vuelve a interrumpir. Insistir con la misma pantalla después de que dijo
   * que no sería pelearle.
   */
  sinRendirse?: boolean;
};

/** Cortes dentro de esta ventana antes de subir la espera. */
const VENTANA_MS = 120_000;
const CORTES_PARA_SUBIR = 3;
/** Muestras de caudal (1/s) antes de dar un veredicto de "no alcanza". */
const MUESTRAS_MIN = 8;
/** Se considera que no alcanza por debajo de este múltiplo del bitrate. */
const MARGEN = 0.9;
/** El cache por debajo de esto está pasando hambre: ahí medir tiene sentido. */
const CACHE_FLACO_S = 5;

let timer: ReturnType<typeof setInterval> | null = null;
let opts: Opts | null = null;
let cortes: number[] = [];
let muestras: number[] = [];
let prevPausado = false;
let espera = 0;
let rendido = false;
let prevPos = 0;
/** Hasta cuándo ignorar cortes: después de un salto, vaciar el cache es normal. */
let graciaHasta = 0;

const mpvCmd = (args: unknown[]) => invoke("mpv_cmd", { args }).catch(() => {});

/** Segundos de espera que rige ahora (para mostrarlo en pantalla). */
export function esperaActual(): number {
  return espera;
}

/**
 * Aplica el preset del usuario y empieza a vigilar. Se llama al arrancar cada
 * reproducción; con el archivo ya en disco no hace falta (no hay red).
 */
export async function iniciarVigilancia(o: Opts): Promise<void> {
  detenerVigilancia();
  opts = o;
  cortes = [];
  muestras = [];
  prevPausado = false;
  rendido = false;
  prevPos = 0;
  graciaHasta = 0;
  const preset = cachePresetActual();
  espera = o.sinRendirse
    ? config.cacheWaitMax
    : config.cacheWait || preset.wait;
  await aplicar(espera, preset.readahead, preset.maxMb);
  // Manual, o ya avisado: el preset queda puesto y no se vigila nada más.
  if (!config.cacheAuto || o.sinRendirse) return;
  timer = setInterval(() => void tick(), 1000);
}

export function detenerVigilancia(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
  opts = null;
}

async function aplicar(wait: number, readahead: number, maxMb: number): Promise<void> {
  try {
    await invoke("mpv_set_cache", {
      waitSecs: wait,
      readaheadSecs: readahead,
      maxMb,
    });
  } catch (e) {
    console.warn("[cache] no se pudo ajustar:", e);
  }
}

/** Bytes/s que pide el archivo para verse sin cortes. */
function bitrate(st: CacheStats): number {
  const size = st.file_size || opts?.sizeBytes || 0;
  if (!size || st.duration <= 0) return 0;
  return size / st.duration;
}

function mediana(xs: number[]): number {
  if (!xs.length) return 0;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

async function tick(): Promise<void> {
  if (!opts || rendido) return;
  let st: CacheStats;
  try {
    st = await invoke<CacheStats>("mpv_cache_stats");
  } catch {
    return;
  }
  if (!st.running) return;

  const ahora = Date.now();

  // Salto manual: el cache se vacía de golpe y mpv pausa a llenarlo. Eso no es
  // red mala, es el usuario buscando una escena. Contarlo llevaría a subir el
  // buffer (o peor, a decir "la red no da") por adelantar tres veces seguidas.
  const salto = prevPos > 0 && Math.abs(st.pos - prevPos) > 3;
  if (salto) graciaHasta = ahora + 8000;
  prevPos = st.pos;
  const enGracia = ahora < graciaHasta;

  // Flanco de subida: el corte se cuenta una vez, no una por segundo que dure.
  if (st.paused_for_cache && !prevPausado && !enGracia) cortes.push(ahora);
  prevPausado = st.paused_for_cache;
  cortes = cortes.filter((t) => ahora - t < VENTANA_MS);

  // Medir el caudal SOLO con el cache flaco: lleno, mpv frena la lectura a
  // propósito y `cache-speed` baja al bitrate. Medir ahí daría un falso
  // "no alcanza" justo cuando todo va bien.
  if (!enGracia && st.cache_secs < CACHE_FLACO_S && st.cache_speed > 0) {
    muestras.push(st.cache_speed);
    if (muestras.length > 60) muestras.shift();
  }

  if (cortes.length < CORTES_PARA_SUBIR) return;
  cortes = [];

  const necesario = bitrate(st);
  const medido = mediana(muestras);
  // Veredicto duro: la red no da para este archivo. Ningún buffer lo arregla.
  if (
    necesario > 0 &&
    muestras.length >= MUESTRAS_MIN &&
    medido > 0 &&
    medido < necesario * MARGEN
  ) {
    return rendirse({ tipo: "caudal", necesario, medido, pos: st.pos });
  }

  const tope = config.cacheWaitMax;
  if (espera >= tope) {
    return rendirse({ tipo: "tope", necesario, medido, pos: st.pos });
  }

  const preset = cachePresetActual();
  espera = Math.min(tope, Math.max(2, espera * 2));
  await aplicar(espera, preset.readahead, preset.maxMb);
  void mpvCmd([
    "show-text",
    `Red inestable: juntando ${Math.round(espera)} s de video antes de seguir`,
    4000,
  ]);
}

function rendirse(m: MotivoRendicion): void {
  rendido = true;
  const cb = opts?.onRendirse;
  detenerVigilancia();
  cb?.(m);
}

/** "1,4 MB/s" para los mensajes. Mismo formato que la cola de descargas. */
export function fmtBps(bps: number): string {
  if (!bps || bps <= 0) return "—";
  const mb = bps / 1_048_576;
  if (mb >= 1) return `${mb.toFixed(1)} MB/s`;
  return `${Math.round(bps / 1024)} KB/s`;
}
