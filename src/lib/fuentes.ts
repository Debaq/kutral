// Fuentes de reproducción: tipo, orden y filtros.
//
// Vivía todo dentro de SourcePicker, que era el único que elegía un release.
// Ahora también elige la cola de descargas (colaDescargas.svelte.ts), y las
// dos tienen que elegir IGUAL: si la cola bajara con otro criterio, la peli
// que te deja en el disco no sería la que la app te habría reproducido.
//
// Todo acá es puro: entra una lista de fuentes + la config del usuario, sale
// el orden. Sin estado propio.

import { config } from "$lib/config.svelte";

export type Kind = "movie" | "series" | "anime";

export type Src = {
  source: string;
  title: string;
  magnet: string | null;
  url: string | null;
  info_hash: string | null;
  size_bytes: number | null;
  seeders: number | null;
  quality: string;
  rd_cached: boolean | null;
  // Sub QUEMADO en el video (jkanime y similares): "lat"/"cast"/"en".
  // null = torrent normal. Cambia todo el flujo: no hay pistas que elegir
  // ni subtítulos que bajar, y no necesita debrid para reproducirse.
  hardsub: string | null;
  _count?: number; // cuántas fuentes equivalentes se apilaron en esta
  // Veredicto de la verificación REAL de pistas (ffprobe sobre la URL
  // resuelta): qué español embebido trae el archivo de verdad.
  _es?: "audio" | "subs" | "none" | "unknown";
  // Por qué la TV elegida no puede con esta versión (aprendido). Vacío = sí puede
  // o no se sabe.
  _tv?: string;
  // El archivo ya está en el disco (descarga terminada): `url` es una ruta
  // local. No pasa por debrid ni por el swarm — se abre y ya.
  local?: boolean;
};

export const QLABEL: Record<string, string> = {
  p2160: "4K",
  p1080: "1080p",
  p720: "720p",
  sd: "SD",
  cam: "CAM",
  unknown: "—",
};

// Etiqueta de las fuentes web: se reproducen desde su URL tal cual, sin
// pasar por el debrid ni por el swarm.
export const HARDSUB_LABEL: Record<string, string> = {
  lat: "🔗 Enlace directo",
  cast: "🔗 Enlace directo",
  en: "🔗 Enlace directo",
};

export const QRANK: Record<string, number> = {
  p2160: 5,
  p1080: 4,
  p720: 3,
  sd: 2,
  cam: 1,
  unknown: 0,
};

// NIVEL 1 (barato, ordenar). El nombre del release es solo una PISTA, no
// garantía: los nombres mienten. Sirve para priorizar; la verdad se confirma
// al abrir el archivo (mpv_tracks, nivel 3). Devuelve un score según si el
// usuario quiere doblado (audio ES) o subtitulado (VO + subs ES).
export function langScore(s: Src): number {
  const t = (s.title || "").toLowerCase();
  const latino = /\blatino\b|\blat\b|dual.?lat|español.?latino/.test(t);
  const cast = /castellano|español|espanol|\besp\b|spanish/.test(t);
  const multi = /\bmulti\b|\bdual\b/.test(t);
  const vose = /vose|v\.?o\.?s\.?e|subtitulad|spa.?subs?|\bsubs?\b.*\b(es|spa|esp)\b/.test(t);
  if (config.subMode === "dub") {
    if (latino) return 5; // audio español latino: lo ideal
    if (cast) return 4; // castellano
    if (multi) return 3; // multi/dual suele incluir pista ES
    if (vose) return 1; // solo subtitulado: peor para doblado
    return 0; // VO puro
  }
  // modo "sub": audio original + subtítulos en español
  if (vose) return 5; // explícitamente VOSE
  if (multi) return 4; // multi: trae VO + subs ES
  if (latino || cast) return 2; // doblado: menos ideal pero hay ES
  return 3; // VO puro: audio original, subs externos si hace falta
}

// POOL DE IDIOMA: qué FAMILIA de fuente conviene según el idioma de la app,
// por encima de calidad y seeders. La lógica es asimétrica a propósito:
//  - Español latino: la ruta torrent casi no trae subs latinos, así que el
//    hardsub de jkanime gana.
//  - Castellano: al revés — hardsub castellano casi no existe online, pero
//    OpenSubtitles sí lo tiene, así que el torrent va primero.
//  - Inglés y el resto: la ruta torrent (softsub EN, SubsPlease) es MEJOR que
//    cualquier hardsub español, y el hardsub además es irreversible: hunde.
export function poolScore(s: Src): number {
  const hs = s.hardsub || "";
  if (config.lang === "es-CL") {
    if (hs === "lat") return 3;
    if (hs) return 1; // hardsub de otro español: sirve, pero de última
    return 2;
  }
  if (config.lang === "es-ES") {
    if (hs === "cast") return 3;
    if (hs) return 1;
    return 2;
  }
  return hs ? 0 : 2;
}

// Fuente preferida por el usuario, SEGÚN el tipo de contenido (película/serie/
// anime tienen su propia lista en config): una o varias palabras separadas por
// coma (ej. "MeGusta, FullScrab"). Si el release o el proveedor las contiene, la
// fuente sube al tope del orden. Es una preferencia fuerte: gana incluso a las
// cacheadas, porque el usuario la eligió a propósito (p.ej. porque siempre trae
// pistas en español).
export function prefScore(s: Src, kind: Kind): number {
  const raw =
    kind === "movie" ? config.preferredSourceMovie
    : kind === "anime" ? config.preferredSourceAnime
    : config.preferredSourceSeries;
  return coincide(s, raw);
}

// Lista negra por tipo (config.blockedSource*): si el release o el proveedor
// contiene alguna palabra, la fuente se HUNDE al fondo del orden. No se elimina
// (sigue elegible si no hay nada más), pero nunca se auto-reproduce de primera.
export function blockScore(s: Src, kind: Kind): number {
  const raw =
    kind === "movie" ? config.blockedSourceMovie
    : kind === "anime" ? config.blockedSourceAnime
    : config.blockedSourceSeries;
  return coincide(s, raw);
}

function coincide(s: Src, raw: string): number {
  const lista = (raw || "").toLowerCase().trim();
  if (!lista) return 0;
  const needles = lista.split(/[,;]/).map((x) => x.trim()).filter(Boolean);
  if (!needles.length) return 0;
  const hay = `${s.title || ""} ${s.source || ""}`.toLowerCase();
  return needles.some((n) => hay.includes(n)) ? 1 : 0;
}

/**
 * Orden definitivo de la lista (muta y devuelve el mismo array). El criterio
 * es el mismo para reproducir y para bajar: lo que la app te pondría es lo
 * que la app te baja.
 */
export function ordenarFuentes(srcs: Src[], kind: Kind): Src[] {
  return srcs.sort((a, b) => {
    // Lista negra: las fuentes bloqueadas se hunden al fondo, pase lo que pase.
    const blk = blockScore(a, kind) - blockScore(b, kind);
    if (blk) return blk;
    // Fuente preferida por el usuario: máxima prioridad (sube al tope).
    const p = prefScore(b, kind) - prefScore(a, kind);
    if (p) return p;
    // Pool de idioma antes que "instantáneo": de nada sirve que cargue
    // rápido si no entiendes lo que dice.
    const pool = poolScore(b) - poolScore(a);
    if (pool) return pool;
    const c = (b.rd_cached ? 1 : 0) - (a.rd_cached ? 1 : 0);
    if (c) return c;
    // Preferencia de idioma (doblado/subtitulado) por sobre la calidad: de
    // nada sirve 4K si no entiendes el audio.
    const l = langScore(b) - langScore(a);
    if (l) return l;
    const q = (QRANK[b.quality] || 0) - (QRANK[a.quality] || 0);
    if (q) return q;
    return (b.seeders || 0) - (a.seeders || 0);
  });
}

// Apila fuentes equivalentes (mismo título/calidad, distinto idioma/grupo).
// Mantiene la mejor de cada grupo (la lista llega ya ordenada) y cuenta.
export function stack(list: Src[]): Src[] {
  const groups = new Map<string, Src>();
  for (const s of list) {
    const k = normKey(s);
    const ex = groups.get(k);
    if (!ex) groups.set(k, { ...s, _count: 1 });
    else ex._count = (ex._count || 1) + 1;
  }
  return [...groups.values()];
}

function normKey(s: Src): string {
  const t = (s.title || "")
    .toLowerCase()
    .replace(/[._\-\[\]()]/g, " ")
    .replace(
      /\b(1080p?|2160p?|720p?|480p?|4k|uhd|web[- ]?dl|webrip|bluray|bdrip|hdrip|x264|x265|hevc|h ?264|h ?265|av1|10bit|hdr10?|dv|aac|ac3|dd[p+]?5?1?|dts|atmos|truehd|multi|dual|latino|castellano|espanol|español|esp|eng|english|ita|rus|fr|vff|vostfr|subs?|remux)\b/g,
      " ",
    )
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 45);
  // El hardsub va en su propia clave: apilarlo con un torrent de nombre
  // parecido escondería la única fuente con español garantizado.
  return `${s.quality}|${s.hardsub || ""}|${t}`;
}

/**
 * ¿Esta fuente cabe dentro del techo de la descarga local? Con debrid el peso
 * no importa (no lo bajas tú), pero acá sí: es tu disco y tu conexión. Sin
 * tamaño declarado se deja pasar: no vamos a descartar una fuente por falta
 * de metadatos.
 */
export function cabeEnLocal(s: Src): boolean {
  if ((QRANK[s.quality] || 0) > (QRANK[config.torrentMaxQuality] || 0)) return false;
  if (s.size_bytes && s.size_bytes > config.torrentMaxGb * 1024 ** 3) return false;
  return true;
}

export function fmtSize(b: number | null): string {
  if (!b) return "";
  const gb = b / 1073741824;
  if (gb >= 1) return gb.toFixed(1) + " GB";
  return Math.round(b / 1048576) + " MB";
}

// Chips de info técnica extraídos del nombre del release.
export function infoChips(s: Src): string[] {
  const t = (s.title || "").toLowerCase();
  const out: string[] = [];
  if (/x265|h\.?265|hevc/.test(t)) out.push("HEVC");
  else if (/av1/.test(t)) out.push("AV1");
  else if (/x264|h\.?264|avc/.test(t)) out.push("H264");
  if (/dolby ?vision|dovi|\bdv\b/.test(t)) out.push("DV");
  else if (/hdr10\+/.test(t)) out.push("HDR10+");
  else if (/hdr/.test(t)) out.push("HDR");
  if (/atmos/.test(t)) out.push("Atmos");
  else if (/truehd/.test(t)) out.push("TrueHD");
  else if (/dts/.test(t)) out.push("DTS");
  else if (/dd\+|ddp|eac3|e-ac-3/.test(t)) out.push("DD+");
  else if (/ac3|dd5|dd 5/.test(t)) out.push("AC3");
  if (/multi/.test(t)) out.push("MULTi");
  if (/dual/.test(t)) out.push("Dual");
  if (/latino/.test(t)) out.push("Latino");
  else if (/castellano|español|espanol|\besp\b/.test(t)) out.push("ES");
  return out.slice(0, 5);
}
