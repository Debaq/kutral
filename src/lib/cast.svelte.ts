// Transmitir a la TV: Google Cast (Google TV, Chromecast, LG…) o DLNA
// (Samsung y demás). El backend (cast.rs / dlna.rs) habla con la TV; acá vive
// lo que Kütral recuerda y aprende:
//
//   - La TV elegida (se recuerda entre sesiones). Si cambió de IP por DHCP se
//     la vuelve a encontrar por su id al escanear la red.
//   - Qué formatos reproduce ESA TV. Cada TV decodifica cosas distintas (una
//     LG no suena con DTS, una Google TV sí con EAC3…) y ninguna lo avisa
//     antes: la única forma de saberlo es probar. Cada transmisión deja una
//     lección, y la próxima vez el selector salta directo las fuentes que esa
//     TV no puede reproducir.
//
// Todo en localStorage, como el resto de la configuración de Kütral.

import { invoke } from "@tauri-apps/api/core";
import { guardarProgreso, type MediaMeta } from "$lib/historial.svelte";

export type CastTv = {
  id: string;
  nombre: string;
  modelo: string;
  ip: string;
  puerto: number;
  /** "cast" | "dlna". */
  tipo: string;
  // DLNA: URLs de control (el backend las necesita de vuelta).
  av_url?: string;
  av_srv?: string;
  rc_url?: string;
  rc_srv?: string;
};

/** Qué puerto usa Kütral para la TV y si un firewall lo tapa. */
export type RedInfo = { puerto: number; firewall: string; comando: string };

export type CastStatus = {
  activa: boolean;
  tv: string;
  titulo: string;
  estado: string; // PLAYING | PAUSED | BUFFERING | IDLE | LOADING | SIN_APP | SIN_CONEXION
  motivo: string; // FINISHED | ERROR | CANCELLED | INTERRUPTED | SIN_ACCESO
  aviso: string; // SUBS_SIN_ACCESO
  pos: number;
  duracion: number;
  volumen: number;
  silencio: boolean;
};

/** Pista reportada por ffprobe (probe.rs). */
export type ProbeTrack = {
  kind: string; // video | audio | subtitle
  codec: string;
  lang: string;
  title: string;
  default: boolean;
  profile: string;
};

/**
 * Rasgos de un archivo que deciden si una TV lo reproduce: el video y el
 * audio que va a sonar. `v:hevc/main 10/dv`, `a:dts`.
 */
export type Rasgos = { video: string; audio: string };

type Leccion = { ok: boolean; veces: number; cuando: number };
/** tvId → rasgo (`v:…`, `a:…` o `c:<video>|<audio>`) → lo aprendido. */
type Aprendido = Record<string, Record<string, Leccion>>;

const K_TV = "cast_tv";
const K_USAR = "cast_usar_tv";
const K_APRENDIDO = "cast_aprendido";

export const cast = $state({
  /** TV recordada. null = nunca se eligió una. */
  tv: null as CastTv | null,
  /** Descubrir en la TV por defecto (si no, en este equipo). */
  usarTv: false,
  buscando: false,
  encontradas: [] as CastTv[],
  errorBusqueda: "",
  /** Transmisión en curso (null = nada en la TV desde Kütral). */
  estado: null as CastStatus | null,
  /** El selector de fuentes muestra los controles: la pastilla se esconde. */
  controlesAbiertos: false,
  aprendido: {} as Aprendido,
});

function leer<T>(k: string, def: T): T {
  try {
    const v = localStorage.getItem(k);
    return v ? (JSON.parse(v) as T) : def;
  } catch {
    return def;
  }
}
function escribir(k: string, v: unknown) {
  try {
    if (v === null) localStorage.removeItem(k);
    else localStorage.setItem(k, JSON.stringify(v));
  } catch {
    /* sin storage: se pierde al cerrar, nada más */
  }
}

export function cargarCast() {
  cast.tv = leer<CastTv | null>(K_TV, null);
  cast.usarTv = !!cast.tv && leer<boolean>(K_USAR, false);
  cast.aprendido = leer<Aprendido>(K_APRENDIDO, {});
}

// ---- La TV -------------------------------------------------------------------

export async function buscarTvs(): Promise<CastTv[]> {
  cast.buscando = true;
  cast.errorBusqueda = "";
  try {
    cast.encontradas = await invoke<CastTv[]>("cast_scan", { ms: 3000 });
    // La recordada pudo cambiar de IP: se actualiza sola al reaparecer.
    const misma = cast.tv && cast.encontradas.find((t) => t.id === cast.tv!.id);
    if (misma && (misma.ip !== cast.tv!.ip || misma.puerto !== cast.tv!.puerto)) {
      elegirTv(misma);
    }
    if (!cast.encontradas.length) {
      cast.errorBusqueda =
        "No apareció ninguna TV. Revisa que esté encendida y en la misma red wifi que este equipo.";
    }
  } catch (e) {
    cast.errorBusqueda = String(e);
  } finally {
    cast.buscando = false;
  }
  return cast.encontradas;
}

export function elegirTv(tv: CastTv) {
  const nueva = cast.tv?.id !== tv.id;
  cast.tv = tv;
  escribir(K_TV, tv);
  // Elegir una TV por primera vez es decir "quiero ver en la TV".
  if (nueva) setUsarTv(true);
}

export function olvidarTv() {
  cast.tv = null;
  escribir(K_TV, null);
  setUsarTv(false);
}

export function setUsarTv(v: boolean) {
  cast.usarTv = v && !!cast.tv;
  escribir(K_USAR, cast.usarTv);
}

/**
 * La TV recordada, lista para usar. Si no contesta en su IP de siempre, la
 * busca en la red por id (el router le pudo dar otra IP).
 */
export async function tvLista(): Promise<CastTv> {
  const tv = cast.tv;
  if (!tv) throw new Error("No hay TV elegida");
  if (await invoke<boolean>("cast_ping", { tv }).catch(() => false)) return tv;
  const tvs = await buscarTvs();
  const misma = tvs.find((t) => t.id === tv.id);
  if (!misma) {
    throw new Error(`${tv.nombre} no aparece en la red. ¿Está encendida?`);
  }
  return misma;
}

// ---- Aprender ----------------------------------------------------------------

/** Lo que va a sonar/verse en la TV según el probe. null = no se pudo saber. */
export function rasgosDe(tracks: ProbeTrack[]): Rasgos | null {
  const v = tracks.find((t) => t.kind === "video");
  const audios = tracks.filter((t) => t.kind === "audio");
  // El receptor Cast no deja elegir pista embebida: suena la predeterminada.
  const a = audios.find((t) => t.default) ?? audios[0];
  if (!v) return null;
  const video = [v.codec, v.profile].filter(Boolean).join("/").toLowerCase();
  return { video, audio: (a?.codec || "sin audio").toLowerCase() };
}

/** La pista de audio que sonará en la TV. */
export function audioQueSuena(tracks: ProbeTrack[]): ProbeTrack | null {
  const audios = tracks.filter((t) => t.kind === "audio");
  return audios.find((t) => t.default) ?? audios[0] ?? null;
}

const kV = (r: Rasgos) => `v:${r.video}`;
const kA = (r: Rasgos) => `a:${r.audio}`;
const kC = (r: Rasgos) => `c:${r.video}|${r.audio}`;

function leccion(tvId: string, k: string): Leccion | undefined {
  return cast.aprendido[tvId]?.[k];
}

function anotar(tvId: string, k: string, ok: boolean) {
  const tabla = (cast.aprendido[tvId] ??= {});
  const prev = tabla[k];
  // Una falla nueva manda sobre un éxito viejo y viceversa: lo último que
  // pasó es lo que vale (un update de firmware puede cambiar lo que soporta).
  tabla[k] = { ok, veces: prev && prev.ok === ok ? prev.veces + 1 : 1, cuando: Date.now() };
  escribir(K_APRENDIDO, cast.aprendido);
}

// Audio que el receptor de Google Cast no decodifica: sale el video mudo, sin
// error que lo delate. Se da por malo de entrada; si alguna TV lo reproduce
// ("aprenderExito"), lo aprendido manda.
const AUDIO_SIN_CAST = new Set(["dts", "truehd"]);

/**
 * ¿Esta TV puede con estos rasgos, según lo aprendido? `motivo` explica en
 * palabras por qué no, para mostrarlo en el selector.
 */
export function evaluar(tv: CastTv, r: Rasgos): { apto: boolean; motivo: string } {
  const tvId = tv.id;
  const audioAprendido = leccion(tvId, kA(r))?.ok;
  if (tv.tipo !== "dlna" && AUDIO_SIN_CAST.has(r.audio) && audioAprendido !== true) {
    return { apto: false, motivo: `Google Cast no reproduce audio ${nombreAudio(r.audio)}` };
  }
  if (audioAprendido === false) {
    return { apto: false, motivo: `tu TV no reproduce audio ${nombreAudio(r.audio)}` };
  }
  if (leccion(tvId, kV(r))?.ok === false) {
    return { apto: false, motivo: `tu TV no reproduce video ${nombreVideo(r.video)}` };
  }
  if (leccion(tvId, kC(r))?.ok === false) {
    return {
      apto: false,
      motivo: `tu TV no pudo con ${nombreVideo(r.video)} + ${nombreAudio(r.audio)}`,
    };
  }
  return { apto: true, motivo: "" };
}

/** Arrancó y se vio un buen rato: video y audio sirven. */
export function aprenderExito(tvId: string, r: Rasgos) {
  anotar(tvId, kV(r), true);
  anotar(tvId, kA(r), true);
  anotar(tvId, kC(r), true);
}

/**
 * La TV rechazó el archivo. No dice si fue por el video o por el audio, así
 * que se culpa al rasgo que NO se conoce: si el video ya funcionó antes con
 * otro audio, el culpable es el audio, y al revés. Si ninguno se conoce se
 * anota solo la combinación.
 */
export function aprenderFalla(tvId: string, r: Rasgos): string {
  const vOk = leccion(tvId, kV(r))?.ok === true;
  const aOk = leccion(tvId, kA(r))?.ok === true;
  anotar(tvId, kC(r), false);
  if (vOk && !aOk) {
    anotar(tvId, kA(r), false);
    return `audio ${nombreAudio(r.audio)}`;
  }
  if (aOk && !vOk) {
    anotar(tvId, kV(r), false);
    return `video ${nombreVideo(r.video)}`;
  }
  return `${nombreVideo(r.video)} + ${nombreAudio(r.audio)}`;
}

/** "No se oye": el video anda pero el audio no. Culpa sin ambigüedad. */
export function aprenderMudo(tvId: string, r: Rasgos) {
  anotar(tvId, kV(r), true);
  anotar(tvId, kA(r), false);
  anotar(tvId, kC(r), false);
}

export function olvidarAprendido(tvId: string) {
  delete cast.aprendido[tvId];
  escribir(K_APRENDIDO, cast.aprendido);
}

/** Lecciones de una TV para mostrarlas en Configuración (sin combinaciones). */
export function leccionesDe(tvId: string): { rasgo: string; ok: boolean; veces: number }[] {
  const t = cast.aprendido[tvId] ?? {};
  return Object.entries(t)
    .filter(([k]) => !k.startsWith("c:"))
    .map(([k, l]) => ({
      rasgo: k.startsWith("v:") ? `Video ${nombreVideo(k.slice(2))}` : `Audio ${nombreAudio(k.slice(2))}`,
      ok: l.ok,
      veces: l.veces,
    }))
    .sort((a, b) => Number(a.ok) - Number(b.ok) || a.rasgo.localeCompare(b.rasgo));
}

const AUDIO: Record<string, string> = {
  aac: "AAC",
  ac3: "Dolby Digital (AC3)",
  eac3: "Dolby Digital Plus (EAC3)",
  truehd: "Dolby TrueHD",
  dts: "DTS",
  mp3: "MP3",
  opus: "Opus",
  flac: "FLAC",
  vorbis: "Vorbis",
};
export function nombreAudio(a: string): string {
  return AUDIO[a] ?? a.toUpperCase();
}

export function nombreVideo(v: string): string {
  const [codec, ...perfil] = v.split("/");
  const base =
    { h264: "H.264", hevc: "HEVC", av1: "AV1", vp9: "VP9", mpeg4: "MPEG-4" }[codec] ??
    codec.toUpperCase();
  const p = perfil.join(" ");
  const extras = [
    /10/.test(p) ? "10-bit" : "",
    /\bdv\b/.test(p) ? "Dolby Vision" : "",
  ].filter(Boolean);
  return [base, ...extras].join(" ");
}

// ---- Transmisión en curso ----------------------------------------------------

type Seguimiento = {
  tvId: string;
  rasgos: Rasgos | null;
  ctx: MediaMeta | null;
  desde: number;
  aprendio: boolean;
  ultimoGuardado: number;
  sinConexion: number;
};

let seg: Seguimiento | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
// Recarga en curso (cambio de subtítulos): la TV pasa por IDLE/LOADING al
// cambiar de medio y eso no es que haya terminado.
let graciaHasta = 0;

/** Ignora los estados de fin durante `ms` o hasta que la TV vuelva a reproducir. */
export function darGracia(ms: number) {
  graciaHasta = Date.now() + ms;
}

// Un minuto de video corriendo sin que nadie diga "no se oye" = funciona.
const SEGUNDOS_PARA_APRENDER = 60;
const GUARDAR_CADA_MS = 15_000;

/**
 * Empieza a seguir lo que se mandó a la TV: progreso al historial, lección
 * de éxito y fin. El contexto del historial se captura AHORA: si el usuario
 * sigue navegando y abre otra ficha, el progreso no se cruza de título.
 */
export function seguir(s: { tvId: string; rasgos: Rasgos | null; ctx: MediaMeta | null; desde: number }) {
  seg = { ...s, aprendio: false, ultimoGuardado: Date.now(), sinConexion: 0 };
  if (timer) clearInterval(timer);
  timer = setInterval(() => void muestrear(), 2000);
  void muestrear();
}

async function muestrear() {
  let st: CastStatus;
  try {
    st = await invoke<CastStatus>("cast_status");
  } catch {
    return;
  }
  if (!st.activa) {
    terminar(false);
    return;
  }
  if (Date.now() < graciaHasta) {
    if (st.estado === "PLAYING" || st.estado === "PAUSED") graciaHasta = 0;
    else {
      cast.estado = { ...st, estado: "LOADING" };
      return;
    }
  }
  cast.estado = st;
  if (!seg) return;

  if (st.estado === "SIN_CONEXION") {
    // La TV se apagó o salió de la red. Unos cuantos intentos antes de soltar.
    if (++seg.sinConexion >= 8) terminar(true);
    return;
  }
  seg.sinConexion = 0;

  if (st.estado === "SIN_APP" || st.estado === "IDLE") {
    // Terminó, alguien cambió de app en la TV, la detuvo con su control, o
    // se cortó a mitad (IDLE + ERROR ya pasado el arranque).
    await guardar(st, st.motivo === "FINISHED");
    terminar(true);
    return;
  }

  if (!seg.aprendio && seg.rasgos && st.estado === "PLAYING" && st.pos - seg.desde >= SEGUNDOS_PARA_APRENDER) {
    seg.aprendio = true;
    aprenderExito(seg.tvId, seg.rasgos);
  }
  if (Date.now() - seg.ultimoGuardado >= GUARDAR_CADA_MS) {
    seg.ultimoGuardado = Date.now();
    await guardar(st, false);
  }
}

async function guardar(st: CastStatus, termino: boolean) {
  const ctx = seg?.ctx;
  if (!ctx) return;
  const dur = st.duracion > 0 ? st.duracion : null;
  const pos = termino && dur ? dur : st.pos;
  if (!(pos > 0)) return;
  await guardarProgreso(ctx, {
    watched: pos,
    runtime: dur ? Math.floor(dur) : null,
    real: dur ? pos / dur : null,
  }).catch(() => {});
}

function terminar(soltar: boolean) {
  if (timer) clearInterval(timer);
  timer = null;
  seg = null;
  cast.estado = null;
  if (soltar) void invoke("cast_soltar").catch(() => {});
}

/** Tipo de TV legible: "Google Cast" / "DLNA". */
export function protocolo(tv: CastTv): string {
  return tv.tipo === "dlna" ? "DLNA" : "Google Cast";
}

export async function redInfo(): Promise<RedInfo | null> {
  return invoke<RedInfo>("cast_red_info").catch(() => null);
}

/** Rasgos de lo que está sonando ahora (para "No se oye"). */
export function rasgosActuales(): { tvId: string; rasgos: Rasgos | null } | null {
  return seg ? { tvId: seg.tvId, rasgos: seg.rasgos } : null;
}

export async function control(accion: string, valor?: number) {
  await invoke("cast_control", { accion, valor: valor ?? null });
  // Reflejarlo al tiro en la UI; el próximo muestreo trae la verdad.
  if (cast.estado) {
    if (accion === "pausa") cast.estado.estado = "PAUSED";
    if (accion === "seguir") cast.estado.estado = "PLAYING";
    if (accion === "saltar" && valor != null) cast.estado.pos = valor;
  }
}

export async function detener() {
  const st = cast.estado;
  if (st) await guardar(st, false);
  try {
    await invoke("cast_control", { accion: "detener", valor: null });
  } finally {
    terminar(false);
  }
}

/** Salta ±segundos desde la posición actual. */
export async function saltar(delta: number) {
  const st = cast.estado;
  if (!st) return;
  const tope = st.duracion > 0 ? st.duracion - 5 : Infinity;
  await control("saltar", Math.max(0, Math.min(tope, st.pos + delta)));
}

export function fmtTiempo(s: number): string {
  if (!isFinite(s) || s <= 0) return "0:00";
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = Math.floor(s % 60);
  const mm = h ? String(m).padStart(2, "0") : String(m);
  return `${h ? h + ":" : ""}${mm}:${String(ss).padStart(2, "0")}`;
}
