// Búsqueda de subtítulos externos (Wyzie + OpenSubtitles). La usan el
// buscador dentro de mpv (SubsService) y los controles de la TV.
//
// Series: sin temporada y capítulo las dos APIs devuelven subtítulos de
// cualquier episodio de la serie.
import { invoke } from "@tauri-apps/api/core";
import { config } from "$lib/config.svelte";

export type OpcionSub = { label: string; lang: string; url?: string; fileId?: number };

export type Capitulo = { season?: number | null; episode?: number | null };

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
export async function buscarSubtitulos(imdbId: string, cap: Capitulo = {}): Promise<OpcionSub[]> {
  const lang = idiomaSubs();
  const opts: OpcionSub[] = [];
  if (config.wyzieKey) {
    try {
      const subs = await invoke<{ url: string; label: string; lang: string }[]>("wyzie_search", {
        imdbId,
        language: lang,
        apiKey: config.wyzieKey,
        ...args(cap),
      });
      for (const s of subs)
        opts.push({ url: s.url, label: s.label || s.lang || "Subtítulo", lang: s.lang || lang });
    } catch (e) {
      console.warn("[subs] wyzie", e);
    }
  }
  try {
    const list = await invoke<
      { file_id: number; label: string; lang: string; downloads: number; hi: boolean }[]
    >("os_list", { imdbId, language: lang, ...args(cap) });
    for (const s of list)
      opts.push({
        fileId: s.file_id,
        lang: s.lang,
        label: `${s.label}  ·  ${s.downloads}⬇${s.hi ? "  ·  SDH" : ""}`,
      });
  } catch (e) {
    console.warn("[subs] opensubtitles", e);
  }
  return opts;
}

/** URL descargable de una opción. En OpenSubtitles gasta 1 de la cuota diaria. */
export async function enlaceSub(o: OpcionSub): Promise<string> {
  if (o.url) return o.url;
  if (o.fileId == null) throw new Error("Subtítulo sin enlace");
  const os = await invoke<{ url: string }>("os_download", { fileId: o.fileId });
  return os.url;
}
