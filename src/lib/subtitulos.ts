// Búsqueda de subtítulos externos (Wyzie + OpenSubtitles). La usan el
// buscador dentro de mpv (SubsService) y los controles de la TV.
//
// Series: sin temporada y capítulo las dos APIs devuelven subtítulos de
// cualquier episodio de la serie.
import { invoke } from "@tauri-apps/api/core";
import { config } from "$lib/config.svelte";

export type OpcionSub = {
  label: string;
  lang: string;
  url?: string;
  fileId?: number;
  /** Subido para este archivo exacto (moviehash de OpenSubtitles). */
  hash?: boolean;
  /** Parecido con el release que se reproduce (subs_match.rs). */
  score?: number;
  /** Para sordos (SDH): el automático lo evita si hay otro. */
  hi?: boolean;
};

export type Capitulo = { season?: number | null; episode?: number | null };

/** Qué se está reproduciendo, para preferir el subtítulo que calza. */
export type Fuente = { release?: string; videoUrl?: string };

/** Desde este puntaje es "mismo release". Igual que MISMO_RELEASE en subs_match.rs. */
const MISMO_RELEASE = 65;

function marca(hash?: boolean, score?: number): string {
  if (hash) return "✓ para esta fuente · ";
  if ((score ?? 0) >= MISMO_RELEASE) return "≈ mismo release · ";
  return "";
}

function fuenteArgs(f: Fuente) {
  // Apagado en Configuración: se busca como siempre (el más descargado).
  if (!config.subsPorFuente) return {};
  return { release: f.release || null, videoUrl: f.videoUrl || null };
}

/** Idioma de búsqueda: el elegido en Configuración, español si están apagados. */
export function idiomaSubs(): string {
  return config.subsLang && config.subsLang !== "off" ? config.subsLang : "es";
}

function args(cap: Capitulo) {
  // Temporada sola no sirve (ver arriba): van los dos o ninguno.
  return cap.season != null && cap.episode != null
    ? { season: cap.season, episode: cap.episode }
    : {};
}

/** Varias opciones, para elegir. OpenSubtitles no gasta cuota hasta `enlaceSub`. */
export async function buscarSubtitulos(
  imdbId: string,
  cap: Capitulo = {},
  fuente: Fuente = {},
): Promise<OpcionSub[]> {
  const lang = idiomaSubs();
  const opts: OpcionSub[] = [];
  const f = fuenteArgs(fuente);
  if (config.wyzieKey) {
    try {
      const subs = await invoke<{ url: string; label: string; lang: string; score: number }[]>(
        "wyzie_search",
        { imdbId, language: lang, apiKey: config.wyzieKey, release: f.release ?? null, ...args(cap) },
      );
      for (const s of subs)
        opts.push({
          url: s.url,
          label: marca(false, s.score) + (s.label || s.lang || "Subtítulo"),
          lang: s.lang || lang,
          score: s.score,
        });
    } catch (e) {
      console.warn("[subs] wyzie", e);
    }
  }
  try {
    const list = await invoke<
      {
        file_id: number;
        label: string;
        lang: string;
        downloads: number;
        hi: boolean;
        hash_match: boolean;
        score: number;
      }[]
    >("os_list", { imdbId, language: lang, ...f, ...args(cap) });
    for (const s of list)
      opts.push({
        fileId: s.file_id,
        lang: s.lang,
        label: `${marca(s.hash_match, s.score)}${s.label}  ·  ${s.downloads}⬇${s.hi ? "  ·  SDH" : ""}`,
        hash: s.hash_match,
        score: s.score,
        hi: s.hi,
      });
  } catch (e) {
    console.warn("[subs] opensubtitles", e);
  }
  // Primero el hecho para el archivo, después el mismo release. Es estable: a
  // igualdad queda Wyzie antes (no gasta cuota) y OpenSubtitles por descargas.
  if (config.subsPorFuente)
    opts.sort((a, b) => Number(!!b.hash) - Number(!!a.hash) || (b.score ?? 0) - (a.score ?? 0));
  return opts;
}

/** La que se carga sola cuando el release no trae español: la primera de la
 *  lista ordenada, evitando SDH si hay otra. */
export function mejorSubtitulo(opts: OpcionSub[]): OpcionSub | undefined {
  return opts.find((o) => !o.hi) ?? opts[0];
}

/** URL descargable de una opción. En OpenSubtitles gasta 1 de la cuota diaria. */
export async function enlaceSub(o: OpcionSub): Promise<string> {
  if (o.url) return o.url;
  if (o.fileId == null) throw new Error("Subtítulo sin enlace");
  const os = await invoke<{ url: string }>("os_download", { fileId: o.fileId });
  return os.url;
}
