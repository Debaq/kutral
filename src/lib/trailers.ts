// Búsqueda de trailers: TMDb/AniList dan la key de YouTube, yt-dlp dice si se
// puede reproducir y, si no, Apple (iTunes) como alternativa.
import { invoke } from "@tauri-apps/api/core";

// Resultado de la búsqueda de trailer.
//  - `ytKey`: video de YouTube que existe (verificado contra oEmbed).
//  - `playable`: yt-dlp lo puede resolver → mpv lo reproduce.
//  - `apple`: mp4 de iTunes, alternativa cuando YouTube no se puede.
// `video`/`audio`: URLs directas que devolvió yt-dlp (audio va aparte porque
// YouTube entrega DASH). `at` es cuándo se resolvieron: caducan en unas horas,
// así que un pick viejo se vuelve a resolver antes de reproducir.
export type TrailerPick = {
  ytKey: string;
  apple: string;
  playable: boolean;
  err: string;
  video: string;
  audio: string;
  at: number;
};

export const EMPTY_PICK: TrailerPick = {
  ytKey: "",
  apple: "",
  playable: false,
  err: "",
  video: "",
  audio: "",
  at: 0,
};

// Los trailers se reproducen SIEMPRE en mpv, nunca en el webview:
//  1. El iframe de YouTube da "error 153" en Tauri (el origen es
//     `tauri://localhost`, que no es un referrer http(s) válido) — pasa con
//     todos los videos, permitan embed o no. Invidious/Piped tampoco: sus
//     instancias públicas están caídas o bloqueadas.
//  2. Un <video> tampoco sirve: YouTube ya casi no entrega formatos
//     progresivos, el video y el audio van en streams DASH separados y el
//     webview no los puede juntar.
// mpv sí los junta (ytdl_hook + yt-dlp de vendor/), así que la reproducción
// va por ahí. Si ni eso, se muestra un QR para verlo en el celular.
//
// TMDb va antes que Apple porque se consulta por id exacto y nunca devuelve
// otra película; iTunes solo se puede buscar por texto y para títulos fuera
// del catálogo US (cine coreano, indio, europeo) el match es frágil.
export async function resolveTrailer(
  d: {
    media_type: "movie" | "tv";
    id: number;
    title: string;
    original_title?: string | null;
    year?: string;
  },
  apiKey: string,
): Promise<TrailerPick> {
  const out: TrailerPick = { ...EMPTY_PICK };
  try {
    const tk = await invoke<{ key: string; embeddable: boolean } | null>("tmdb_trailer_key", {
      mediaType: d.media_type,
      id: d.id,
      apiKey,
    });
    if (tk?.key) out.ytKey = tk.key;
  } catch (e) {
    console.warn("[tmdb_trailer_key]", e);
  }
  if (out.ytKey) {
    const src = await ytTrailerSrc(out.ytKey);
    out.playable = src.ok;
    out.err = src.err;
    out.video = src.video;
    out.audio = src.audio;
    out.at = Date.now();
    if (out.playable) return out;
  }
  try {
    const a = await invoke<{ url: string } | null>("apple_trailer", {
      title: d.title,
      originalTitle: d.original_title || "",
      year: d.year || "",
      mediaType: d.media_type,
    });
    if (a?.url) out.apple = a.url;
  } catch (e) {
    console.warn("[apple_trailer]", e);
  }
  return out;
}

// Resuelve el trailer con yt-dlp: la misma llamada dice si se puede
// reproducir y devuelve las URLs para mpv (una sola ejecución de yt-dlp por
// trailer). `video` vacío con ok=true significa reproducible pero por
// ytdl_hook. `err` casi siempre es que falta el binario en vendor/ (lo baja
// vendor/fetch.sh) o que YouTube pide verificación.
export async function ytTrailerSrc(
  key: string,
): Promise<{ ok: boolean; err: string; video: string; audio: string }> {
  try {
    const src = await invoke<{ video: string; audio: string }>("yt_trailer_src", { key });
    return { ok: true, err: "", video: src?.video || "", audio: src?.audio || "" };
  } catch (e) {
    console.warn("[yt_trailer_src]", e);
    return { ok: false, err: String(e), video: "", audio: "" };
  }
}

// Trailer de anime: la key viene de AniList; TMDb/Apple no aplican porque el
// id no es de TMDb.
export async function resolveAnimeTrailer(key: string): Promise<TrailerPick> {
  const out: TrailerPick = { ...EMPTY_PICK, ytKey: key };
  if (!key) return out;
  const src = await ytTrailerSrc(key);
  out.playable = src.ok;
  out.err = src.err;
  out.video = src.video;
  out.audio = src.audio;
  out.at = Date.now();
  return out;
}
