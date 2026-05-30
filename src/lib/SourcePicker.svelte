<script lang="ts">
  // Lista de fuentes torrent (estilo Stremio) para reproducir vía RealDebrid+mpv.
  // Autocontenido: hace la búsqueda, marca cacheadas (⚡), resuelve y lanza mpv.
  // Maneja su propio teclado (flechas/Enter/Esc) porque en este modo el page
  // no procesa navegación.
  import { invoke } from "@tauri-apps/api/core";

  type Src = {
    source: string;
    title: string;
    magnet: string | null;
    info_hash: string | null;
    size_bytes: number | null;
    seeders: number | null;
    quality: string;
    rd_cached: boolean | null;
  };

  let {
    imdbId,
    mediaType,
    title,
    token,
    autoplay = false,
    onClose,
    onWeb,
  }: {
    imdbId: string;
    mediaType: "movie" | "tv";
    title: string;
    token: string;
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

  const kind = $derived(mediaType === "movie" ? "movie" : "series");

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

  function fmtSize(b: number | null): string {
    if (!b) return "";
    const gb = b / 1073741824;
    if (gb >= 1) return gb.toFixed(1) + " GB";
    return Math.round(b / 1048576) + " MB";
  }

  // Índices de navegación: filas (0..n-1), luego "web" (n), luego "volver" (n+1)
  const total = $derived(sources.length + 2);
  const webIdx = $derived(sources.length);
  const backIdx = $derived(sources.length + 1);

  async function load() {
    loading = true;
    error = "";
    try {
      if (kind !== "movie") {
        error = "Las series aún no están soportadas (próximamente).";
        return;
      }
      const srcs = await invoke<Src[]>("kodios_search", { imdbId, kind });
      if (token) {
        const hashes = srcs.map((s) => s.info_hash).filter(Boolean) as string[];
        try {
          const cached = await invoke<string[]>("rd_instant_available", { hashes, token });
          const set = new Set(cached.map((h) => h.toLowerCase()));
          for (const s of srcs) {
            if (s.info_hash) s.rd_cached = set.has(s.info_hash.toLowerCase());
          }
        } catch {
          /* cache check best-effort */
        }
      }
      srcs.sort((a, b) => {
        const c = (b.rd_cached ? 1 : 0) - (a.rd_cached ? 1 : 0);
        if (c) return c;
        const q = (QRANK[b.quality] || 0) - (QRANK[a.quality] || 0);
        if (q) return q;
        return (b.seeders || 0) - (a.seeders || 0);
      });
      sources = srcs;
      if (!srcs.length) {
        error = "No encontramos fuentes para esta película.";
      } else if (autoplay) {
        // ⚡ Ver con RealDebrid: reproduce directo la mejor (ya ordenada:
        // cacheada > calidad > seeders).
        loading = false;
        void play(srcs[0]);
        return;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function play(s: Src) {
    if (!s.magnet) {
      error = "Esta fuente no tiene magnet.";
      return;
    }
    if (!token) {
      error = "Vincula RealDebrid en Configuración para reproducir.";
      return;
    }
    resolving = true;
    error = "";
    resolvingMsg = s.rd_cached ? "Preparando (instantáneo)…" : "Resolviendo en RealDebrid…";
    try {
      const url = await invoke<string>("rd_resolve", { magnet: s.magnet, token });
      resolvingMsg = "Abriendo reproductor…";
      await invoke("mpv_play", { url, title });
      // mpv toma la pantalla; quedamos en vista "reproduciendo" (mantiene el
      // screening pausado). Cuando mpv cierra, volvemos al catálogo solos.
      resolving = false;
      playing = true;
      playingTitle = s.title;
      startMpvPoll();
    } catch (e) {
      error = "No se pudo reproducir: " + String(e);
      resolving = false;
    }
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

  function move(d: number) {
    if (!total) return;
    let i = focusIdx + d;
    if (i < 0) i = 0;
    if (i > total - 1) i = total - 1;
    focusIdx = i;
    scrollFocused();
  }

  function activate() {
    if (focusIdx === backIdx) return onClose();
    if (focusIdx === webIdx) return onWeb();
    const s = sources[focusIdx];
    if (s) void play(s);
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
  <header class="sp-head">
    <button class="sp-back" onclick={onClose}>← Volver</button>
    <h2>{title}</h2>
    <span class="sp-sub">Elige una fuente</span>
  </header>

  {#if playing}
    <div class="sp-center">
      <p class="sp-playing">▶ Reproduciendo en el reproductor</p>
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
          class="sp-row"
          class:focused={focusIdx === i}
          onclick={() => { focusIdx = i; void play(s); }}
          onmouseenter={() => (focusIdx = i)}
        >
          <span class="sp-q sp-q-{s.quality}">{QLABEL[s.quality] || "—"}</span>
          {#if s.rd_cached}
            <span class="sp-cached">⚡ Instantáneo</span>
          {/if}
          <span class="sp-title">{s.title}</span>
          <span class="sp-meta">
            {#if s.seeders}{s.seeders} 🌱{/if}
            {#if s.size_bytes}· {fmtSize(s.size_bytes)}{/if}
          </span>
        </button>
      {/each}
    </div>

    <footer class="sp-foot">
      <button
        class="sp-foot-btn"
        class:focused={focusIdx === webIdx}
        onclick={onWeb}
        onmouseenter={() => (focusIdx = webIdx)}
      >
        🌐 Ver en web
      </button>
      <button
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

<style>
  .sp-root {
    position: fixed;
    inset: 0;
    background: #0d0d12;
    color: #e6e6ec;
    display: flex;
    flex-direction: column;
    padding: 28px 40px;
    z-index: 50;
    overflow: hidden;
  }
  .sp-head {
    display: flex;
    align-items: baseline;
    gap: 16px;
    margin-bottom: 18px;
  }
  .sp-head h2 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
  }
  .sp-sub {
    color: #888892;
    font-size: 13px;
  }
  .sp-back {
    background: transparent;
    border: 1px solid #2a2a36;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 7px 14px;
    cursor: pointer;
    font-size: 13px;
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
  .sp-hint {
    color: #6e6e78;
    font-size: 12px;
  }
  .sp-playing {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    color: #f3a951;
  }
  .sp-playing-title {
    margin: 0;
    color: #c8c8d0;
    font-size: 14px;
    max-width: 70%;
    text-align: center;
  }
  .sp-spinner {
    width: 34px;
    height: 34px;
    border: 3px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: sp-spin 1s linear infinite;
  }
  @keyframes sp-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .sp-error {
    color: #ff7373;
    font-size: 14px;
    margin: 0 0 12px;
  }

  .sp-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 6px;
  }
  .sp-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    text-align: left;
    background: #15151c;
    border: 2px solid transparent;
    border-radius: 10px;
    padding: 12px 16px;
    cursor: pointer;
    color: inherit;
    font-size: 14px;
  }
  .sp-row.focused {
    border-color: #f3a951;
    background: #1a1410;
  }
  .sp-q {
    flex: none;
    min-width: 52px;
    text-align: center;
    font-weight: 700;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 6px;
    background: #2a2a36;
    color: #d8d8e0;
  }
  .sp-q-p2160 { background: #3a2a4a; color: #d9b8ff; }
  .sp-q-p1080 { background: #1f3a2a; color: #9be38a; }
  .sp-q-p720 { background: #2a3340; color: #9bc4e3; }
  .sp-cached {
    flex: none;
    font-size: 11.5px;
    font-weight: 700;
    color: #ffd76b;
    background: #2e2410;
    border: 1px solid #5a4520;
    padding: 3px 8px;
    border-radius: 6px;
  }
  .sp-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #e6e6ec;
  }
  .sp-meta {
    flex: none;
    color: #888892;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .sp-foot {
    display: flex;
    gap: 10px;
    margin-top: 16px;
  }
  .sp-foot-btn {
    background: #15151c;
    border: 2px solid transparent;
    color: #d8d8e0;
    border-radius: 10px;
    padding: 11px 18px;
    cursor: pointer;
    font-size: 13.5px;
  }
  .sp-foot-btn.focused {
    border-color: #f3a951;
    background: #1a1410;
  }
</style>
