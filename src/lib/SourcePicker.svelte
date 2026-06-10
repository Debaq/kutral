<script lang="ts">
  // Lista de fuentes torrent (estilo Stremio) para reproducir vía RealDebrid+mpv.
  // Autocontenido: hace la búsqueda, marca cacheadas (⚡), resuelve y lanza mpv.
  // Maneja su propio teclado (flechas/Enter/Esc) porque en este modo el page
  // no procesa navegación.
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { config } from "$lib/config.svelte";

  // Pista real del contenedor leída de mpv (verdad del archivo).
  type MpvTrack = {
    id: number;
    kind: string; // "audio" | "sub"
    lang: string;
    title: string;
    selected: boolean;
  };

  type Src = {
    source: string;
    title: string;
    magnet: string | null;
    url: string | null;
    info_hash: string | null;
    size_bytes: number | null;
    seeders: number | null;
    quality: string;
    rd_cached: boolean | null;
    _count?: number; // cuántas fuentes equivalentes se apilaron en esta
  };

  let {
    imdbId,
    kind,
    season = null,
    episode = null,
    title,
    backdrop = null,
    rdLinked,
    autoplay = false,
    onClose,
    onWeb,
  }: {
    imdbId: string;
    kind: "movie" | "series" | "anime";
    season?: number | null;
    episode?: number | null;
    title: string;
    backdrop?: string | null;
    rdLinked: boolean;
    autoplay?: boolean;
    onClose: () => void;
    onWeb: () => void;
  } = $props();

  let loading = $state(true);
  let error = $state("");
  let sources = $state<Src[]>([]);
  let focusIdx = $state(0);
  let resolving = $state(false);
  let resolvingMsg = $state("");
  let playing = $state(false);
  let playingTitle = $state("");
  let mpvPoll: ReturnType<typeof setInterval> | null = null;

  const QLABEL: Record<string, string> = {
    p2160: "4K",
    p1080: "1080p",
    p720: "720p",
    sd: "SD",
    cam: "CAM",
    unknown: "—",
  };
  const QRANK: Record<string, number> = {
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
  function langScore(s: Src): number {
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

  // Fuente preferida por el usuario (config.preferredSource): una o varias
  // palabras separadas por coma (ej. "fullscrabe, sigloxx"). Si el release o el
  // proveedor las contiene, la fuente sube al tope del orden. Es una preferencia
  // fuerte: gana incluso a las cacheadas, porque el usuario la eligió a propósito
  // (p.ej. porque siempre trae pistas en español).
  function prefScore(s: Src): number {
    const pref = (config.preferredSource || "").toLowerCase().trim();
    if (!pref) return 0;
    const needles = pref.split(/[,;]/).map((x) => x.trim()).filter(Boolean);
    if (!needles.length) return 0;
    const hay = `${s.title || ""} ${s.source || ""}`.toLowerCase();
    return needles.some((n) => hay.includes(n)) ? 1 : 0;
  }

  function fmtSize(b: number | null): string {
    if (!b) return "";
    const gb = b / 1073741824;
    if (gb >= 1) return gb.toFixed(1) + " GB";
    return Math.round(b / 1048576) + " MB";
  }

  // Chips de info técnica extraídos del nombre del release.
  function infoChips(s: Src): string[] {
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

  // Apila fuentes equivalentes (mismo título/calidad, distinto idioma/grupo).
  // Mantiene la mejor de cada grupo (la lista llega ya ordenada) y cuenta.
  function stack(list: Src[]): Src[] {
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
    return `${s.quality}|${t}`;
  }

  // Índices de navegación: filas (0..n-1), luego "web" (n), luego "volver" (n+1)
  const total = $derived(sources.length + 2);
  const webIdx = $derived(sources.length);
  const backIdx = $derived(sources.length + 1);

  async function load() {
    loading = true;
    error = "";
    try {
      if (kind !== "movie" && (season == null || episode == null)) {
        error = "Falta temporada/episodio.";
        return;
      }
      const srcs = await invoke<Src[]>("kodios_search", {
        imdbId,
        kind,
        title,
        season: season ?? undefined,
        episode: episode ?? undefined,
      });
      if (rdLinked) {
        const hashes = srcs.map((s) => s.info_hash).filter(Boolean) as string[];
        try {
          const cached = await invoke<string[]>("rd_instant_available", { hashes });
          const set = new Set(cached.map((h) => h.toLowerCase()));
          for (const s of srcs) {
            if (s.info_hash) s.rd_cached = set.has(s.info_hash.toLowerCase());
          }
        } catch {
          /* cache check best-effort */
        }
      }
      srcs.sort((a, b) => {
        // Fuente preferida por el usuario: máxima prioridad (sube al tope).
        const p = prefScore(b) - prefScore(a);
        if (p) return p;
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
      sources = stack(srcs);
      if (!srcs.length) {
        error = "No encontramos fuentes para esta película.";
      } else if (autoplay && config.sourceSelect === "auto") {
        // ⚡ Ver con RealDebrid en modo automático: reproduce directo la mejor
        // (ya ordenada: preferida > cacheada > idioma > calidad > seeders).
        loading = false;
        void playFrom(0);
        return;
      }
      // Modo manual: aunque se haya pedido autoplay, mostramos el selector con
      // la mejor fuente ya enfocada (focusIdx=0) para elegir con un Enter.
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Log a la consola de Tauri (stderr) para depurar el flujo de reproducción.
  function dbg(m: string) {
    invoke("ui_log", { msg: `[picker] ${m}` }).catch(() => {});
  }

  const MAX_TRIES = 12;

  // Intenta reproducir desde `startIdx`, saltando fuentes que fallan
  // (bloqueadas por DMCA 451, sin video, timeout). Reproduce la primera que
  // resuelva. Así un release bloqueado en el debrid no rompe la experiencia.
  async function playFrom(startIdx: number) {
    dbg(`playFrom start=${startIdx} n=${sources.length} rd=${rdLinked}`);
    if (!rdLinked) {
      error = "Vincula tu debrid en Configuración para reproducir.";
      return;
    }
    resolving = true;
    error = "";
    let blocked = 0;
    let tried = 0;
    let lastErr = "";

    for (let i = startIdx; i < sources.length && tried < MAX_TRIES; i++) {
      const s = sources[i];
      if (!s.magnet && !s.url) { dbg(`#${i} SKIP (sin magnet ni url)`); continue; }
      dbg(`#${i} intento url=${!!s.url} magnet=${!!s.magnet} ${(s.title || "").slice(0, 40)}`);
      tried++;
      focusIdx = i;
      resolvingMsg =
        `Probando fuente ${i + 1}/${sources.length}${s.rd_cached ? " ⚡" : ""}…` +
        (blocked ? ` (${blocked} bloqueada${blocked > 1 ? "s" : ""})` : "");
      try {
        // URL pre-resuelta (Torrentio+RD, método Kodi) → directo a mpv, sin
        // addMagnet del cliente (evita el 451 de RD). Si no, resuelve el magnet.
        const url = s.url
          ? s.url
          : await invoke<string>("rd_resolve", { magnet: s.magnet });
        resolvingMsg = "Abriendo reproductor…";
        await invoke("mpv_play", { url, title });
        resolving = false;
        playing = true;
        playingTitle = s.title;
        startMpvPoll();
        // Niveles 2-3: confirmar audio/subs REALES del archivo y elegir la
        // pista ES; si no existe, bajar subtítulos externos. No bloquea el play.
        void applyPreferredTracks();
        return;
      } catch (e) {
        lastErr = String(e);
        if (lastErr.includes("BLOQUEADO_DMCA")) blocked++;
        // sigue con la próxima fuente
      }
    }

    resolving = false;
    dbg(`fin: tried=${tried} blocked=${blocked} lastErr=${lastErr.slice(0, 60)}`);
    error =
      blocked > 0
        ? `Sin fuentes reproducibles: ${blocked} bloqueada(s) por el debrid (DMCA). Probá "🔄 Rebuscar fuentes" u otra calidad.`
        : `No se pudo reproducir ninguna fuente. ${lastErr}`;
  }

  function startMpvPoll() {
    stopMpvPoll();
    mpvPoll = setInterval(async () => {
      try {
        const alive = await invoke<boolean>("mpv_running");
        if (!alive) {
          stopMpvPoll();
          onClose();
        }
      } catch {
        /* sigue intentando */
      }
    }, 1500);
  }
  function stopMpvPoll() {
    if (mpvPoll !== null) {
      clearInterval(mpvPoll);
      mpvPoll = null;
    }
  }

  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  // Manda un comando JSON-IPC a mpv (set propiedad, sub-add, etc.).
  const mpv = (args: unknown[]) => invoke("mpv_cmd", { args }).catch(() => {});

  // ¿La pista es español? Mira el lang ISO (spa/es) y, de respaldo, el título.
  function isEsTrack(t: MpvTrack): boolean {
    const l = (t.lang || "").toLowerCase();
    if (/^(es|spa|esp)/.test(l)) return true;
    return /espa|spanish|latino|castellano/i.test(t.title || "");
  }

  // NIVELES 2-3: la verdad del contenedor. Lee las pistas reales de mpv y elige
  // según la preferencia. Solo baja subs externos si el archivo NO trae ES.
  async function applyPreferredTracks() {
    // mpv tarda un momento en demuxear y exponer las pistas tras abrir.
    let tracks: MpvTrack[] = [];
    for (let i = 0; i < 15; i++) {
      tracks = await invoke<MpvTrack[]>("mpv_tracks").catch(() => []);
      if (tracks.some((t) => t.kind === "audio")) break;
      await sleep(400);
    }
    const audios = tracks.filter((t) => t.kind === "audio");
    const subs = tracks.filter((t) => t.kind === "sub");
    const esAudio = audios.find(isEsTrack);
    const esSub = subs.find(isEsTrack);

    if (config.subMode === "dub" && esAudio) {
      // Audio español embebido: selecciónalo y oculta subtítulos.
      await mpv(["set", "aid", String(esAudio.id)]);
      await mpv(["set", "sub-visibility", "no"]);
      dbg(`audio ES embebido id=${esAudio.id}`);
      return;
    }
    // Subtitulado (o doblado sin audio ES): audio original + subs ES.
    if (esSub) {
      await mpv(["set", "sid", String(esSub.id)]);
      await mpv(["set", "sub-visibility", "yes"]);
      dbg(`sub ES embebido id=${esSub.id}`);
      return;
    }
    // El release no trae ES → subtítulos externos (Wyzie o OpenSubtitles).
    await loadExternalSub();
  }

  // Subtítulos externos: Wyzie si hay key, si no OpenSubtitles (cuota diaria).
  // mpv carga la URL directo con sub-add (no hace falta bajar a disco).
  async function loadExternalSub(): Promise<boolean> {
    const lang = config.subsLang || "es";
    if (lang === "off") return false;
    let url = "";
    if (config.wyzieKey) {
      try {
        const subs = await invoke<{ url: string }[]>("wyzie_search", {
          imdbId,
          language: lang,
          apiKey: config.wyzieKey,
        });
        if (subs.length) url = subs[0].url;
      } catch (e) {
        dbg(`wyzie fail: ${String(e).slice(0, 60)}`);
      }
    }
    if (!url) {
      try {
        const os = await invoke<{ url: string; remaining: number }>("os_search", {
          imdbId,
          language: lang,
        });
        url = os.url;
        dbg(`opensubtitles ok, quedan ${os.remaining}/día`);
      } catch (e) {
        dbg(`opensubtitles fail: ${String(e).slice(0, 80)}`);
      }
    }
    if (!url) {
      dbg("sin subtítulos ES (embebidos ni externos)");
      return false;
    }
    // Guardar el .srt en Descargas (para coleccionarlos) y cargar el archivo
    // local en mpv. Si falla la descarga a disco, se carga la URL directa.
    let toLoad = url;
    try {
      const fname = `${title} [${lang}]`;
      const path = await invoke<string>("subtitle_save", { url, filename: fname });
      if (path) {
        toLoad = path;
        dbg(`sub guardado: ${path}`);
      }
    } catch (e) {
      dbg(`no se pudo guardar sub, uso URL: ${String(e).slice(0, 60)}`);
    }
    await mpv(["sub-add", toLoad, "select"]);
    await mpv(["set", "sub-visibility", "yes"]);
    return true;
  }

  async function stopMpv() {
    stopMpvPoll();
    try {
      await invoke("mpv_stop");
    } catch {
      /* ignore */
    }
    onClose();
  }

  $effect(() => () => stopMpvPoll());

  // Con libmpv embebido el video se cierra desde DENTRO (Esc en el reproductor
  // → backend hace stop() y emite "mpv:state"=false). El webview estuvo oculto
  // y su timer de poll pudo quedar suspendido, así que NO dependemos del poll:
  // escuchamos el evento y volvemos al menú al recibir false.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen<boolean>("mpv:state", (e) => {
      if (e.payload === false) {
        stopMpvPoll();
        onClose();
      }
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  // "Descargar subtítulos…" desde el menú del reproductor (bar pausa). El
  // backend emite el evento; buscamos/bajamos ES (Wyzie→OpenSubtitles), lo
  // guardamos en Descargas y lo cargamos en mpv. Feedback por OSD del video.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen("player:download-subs", async () => {
      await mpv(["show-text", "Buscando subtítulos…", "4000"]);
      try {
        const ok = await loadExternalSub();
        await mpv([
          "show-text",
          ok ? "✓ Subtítulos cargados" : "No se encontraron subtítulos",
          "2500",
        ]);
      } catch {
        await mpv(["show-text", "Error al descargar subtítulos", "2500"]);
      }
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  function move(d: number) {
    if (!total) return;
    let i = focusIdx + d;
    if (i < 0) i = 0;
    if (i > total - 1) i = total - 1;
    focusIdx = i;
    scrollFocused();
  }

  function activate() {
    dbg(`activate focusIdx=${focusIdx} back=${backIdx} web=${webIdx}`);
    if (focusIdx === backIdx) return onClose();
    if (focusIdx === webIdx) return onWeb();
    if (sources[focusIdx]) void playFrom(focusIdx);
  }

  function scrollFocused() {
    queueMicrotask(() => {
      document
        .querySelector<HTMLElement>(".sp-row.focused, .sp-foot-btn.focused")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function onKey(e: KeyboardEvent) {
    if (playing) {
      if (e.key === "Escape" || e.key === "Backspace" || e.key === "Enter") {
        e.preventDefault();
        void stopMpv();
      }
      return;
    }
    if (resolving) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        resolving = false; // cancela visualmente; el resolve sigue en backend
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        activate();
        break;
      case "Escape":
      case "Backspace":
        e.preventDefault();
        onClose();
        break;
    }
  }

  load();
</script>

<svelte:window onkeydown={onKey} />

<div class="sp-root">
  {#if backdrop}
    <div class="sp-bg" style:background-image="url({backdrop})"></div>
  {/if}
  <div class="sp-scrim"></div>
  <div class="sp-hint-back">Esc o Backspace para volver</div>

  <div class="sp-content">
    <header class="sp-head">
      <h2 class="sp-title-h">{title}</h2>
      <span class="sp-sub">Elige una fuente</span>
    </header>

    {#if playing}
      <div class="sp-center">
        <p class="sp-playing">▶ Reproduciendo</p>
        <p class="sp-playing-title">{playingTitle}</p>
        <button class="sp-foot-btn focused" onclick={stopMpv}>⏹ Detener y volver</button>
        <span class="sp-hint">Se cierra solo al terminar el video</span>
      </div>
    {:else if loading}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p>Buscando fuentes…</p>
      </div>
    {:else if resolving}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p>{resolvingMsg}</p>
        <span class="sp-hint">Esc para cancelar</span>
      </div>
    {:else}
      {#if error}
        <p class="sp-error">{error}</p>
      {/if}

      <div class="sp-list">
        {#each sources as s, i (s.info_hash || i)}
          <button
            data-nav
            class="sp-row"
            class:focused={focusIdx === i}
            onclick={() => { focusIdx = i; void playFrom(i); }}
            onmouseenter={() => (focusIdx = i)}
          >
            <span class="sp-q sp-q-{s.quality}">{QLABEL[s.quality] || "—"}</span>
            <div class="sp-mid">
              <span class="sp-title">{s.title}</span>
              <div class="sp-chips">
                {#if prefScore(s)}<span class="sp-pref">★ Preferida</span>{/if}
                {#if s.rd_cached}<span class="sp-cached">⚡ Instantáneo</span>{/if}
                {#each infoChips(s) as c}<span class="sp-chip">{c}</span>{/each}
                <span class="sp-prov">{s.source}</span>
              </div>
            </div>
            <span class="sp-meta">
              {#if s._count && s._count > 1}<span class="sp-dupe">×{s._count}</span>{/if}
              {#if s.seeders}<span>{s.seeders} 🌱</span>{/if}
              {#if s.size_bytes}<span>{fmtSize(s.size_bytes)}</span>{/if}
            </span>
          </button>
        {/each}
      </div>

      <footer class="sp-foot">
        <button
          data-nav
          class="sp-foot-btn"
          class:focused={focusIdx === webIdx}
          onclick={onWeb}
          onmouseenter={() => (focusIdx = webIdx)}
        >
          🌐 Ver en web
        </button>
        <button
          data-nav
          class="sp-foot-btn"
          class:focused={focusIdx === backIdx}
          onclick={onClose}
          onmouseenter={() => (focusIdx = backIdx)}
        >
          ← Volver
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  /* Frame consistente con PlayMenu: fanart difuminado + scrim + glass. */
  .sp-root {
    position: fixed;
    inset: 0;
    z-index: 50;
    overflow: hidden;
    background: #0d0d12;
    color: #e6e6ec;
  }
  .sp-bg {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(10px) saturate(1.1);
    transform: scale(1.06);
  }
  .sp-scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, rgba(8, 8, 12, 0.95) 0%, rgba(8, 8, 12, 0.72) 55%, rgba(8, 8, 12, 0.88) 100%),
      linear-gradient(0deg, rgba(8, 8, 12, 0.9) 0%, rgba(8, 8, 12, 0.25) 60%);
  }
  .sp-hint-back {
    position: absolute;
    top: 18px;
    right: 24px;
    color: #9a9aa4;
    font-size: 12px;
    z-index: 2;
  }
  .sp-content {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 30px 48px 36px;
  }
  .sp-head {
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-bottom: 18px;
  }
  .sp-title-h {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
  }
  .sp-sub {
    color: #b0b0ba;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .sp-center {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    color: #c8c8d0;
  }
  .sp-hint { color: #6e6e78; font-size: 12px; }
  .sp-playing { margin: 0; font-size: 22px; font-weight: 700; color: #f3a951; }
  .sp-playing-title { margin: 0; color: #c8c8d0; font-size: 14px; max-width: 70%; text-align: center; }
  .sp-spinner {
    width: 34px; height: 34px;
    border: 3px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: sp-spin 1s linear infinite;
  }
  @keyframes sp-spin { to { transform: rotate(360deg); } }
  .sp-error { color: #ff7373; font-size: 14px; margin: 0 0 12px; }

  .sp-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 8px;
    scrollbar-color: #3a3a4a transparent;
  }
  .sp-row {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    text-align: left;
    background: rgba(20, 20, 28, 0.62);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    padding: 12px 16px;
    cursor: pointer;
    color: inherit;
    transition: border-color 0.12s, background 0.12s, transform 0.12s;
  }
  .sp-row.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    transform: translateX(4px);
  }
  .sp-q {
    flex: none;
    min-width: 54px;
    text-align: center;
    font-weight: 700;
    font-size: 12px;
    padding: 5px 8px;
    border-radius: 6px;
    background: #2a2a36;
    color: #d8d8e0;
  }
  .sp-q-p2160 { background: #3a2a4a; color: #d9b8ff; }
  .sp-q-p1080 { background: #1f3a2a; color: #9be38a; }
  .sp-q-p720 { background: #2a3340; color: #9bc4e3; }
  .sp-mid {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .sp-title {
    color: #f0f0f4;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sp-chips { display: flex; flex-wrap: wrap; gap: 5px; align-items: center; }
  .sp-chip {
    font-size: 10.5px;
    font-weight: 600;
    color: #c0c0c8;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-cached {
    font-size: 10.5px;
    font-weight: 700;
    color: #ffd76b;
    background: #2e2410;
    border: 1px solid #5a4520;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-pref {
    font-size: 10.5px;
    font-weight: 700;
    color: #9be38a;
    background: #14271a;
    border: 1px solid #2f5a3a;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-prov {
    font-size: 10.5px;
    color: #7a7a86;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .sp-meta {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
    color: #a0a0aa;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .sp-dupe {
    background: #f3a951;
    color: #1a1208;
    border-radius: 5px;
    padding: 1px 6px;
    font-weight: 700;
    font-size: 11px;
  }

  .sp-foot { display: flex; gap: 10px; margin-top: 16px; }
  .sp-foot-btn {
    background: rgba(20, 20, 28, 0.62);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 2px solid rgba(255, 255, 255, 0.12);
    color: #d8d8e0;
    border-radius: 10px;
    padding: 11px 18px;
    cursor: pointer;
    font-size: 13.5px;
  }
  .sp-foot-btn.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
  }
</style>
