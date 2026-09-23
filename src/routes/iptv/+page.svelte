<script lang="ts">
  // IPTV — canales en vivo desde las playlists M3U (config.iptvLists).
  //
  // Nav: ← → ↑ ↓ / d-pad mueve foco · Enter / A reproduce DENTRO de la app
  //   (video + hls.js, sin abrir mpv) · Esc/Backspace/B salen o cierran el
  //   reproductor.
  //
  // La playlist suele traer miles de canales: filtramos por grupo + búsqueda
  // y mostramos como máximo CAP resultados (se avisa cuántos quedan fuera).

  import { onMount, onDestroy, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { setNowPlaying } from "$lib/playerState.svelte";
  import { limpiarContexto } from "$lib/historial.svelte";
  import Hls from "hls.js";
  import { config, loadConfig, IPTV_DEFAULT_LISTS } from "$lib/config.svelte";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { inhibirReposo } from "$lib/reposo";
  import { mejor } from "$lib/nav";

  type Canal = { name: string; logo: string; groups: string[]; url: string; source: string };

  const CAP = 400;

  let cargando = $state(true);
  let errorMsg = $state("");
  let canales = $state<Canal[]>([]);
  // [{ name, count }] orden por cantidad desc; el desplegable lo usa.
  let grupos = $state<{ name: string; count: number }[]>([]);
  let grupoSel = $state<string>("Todos");
  // Fuentes (nombres de listas) cuando hay más de una.
  let fuentes = $state<string[]>([]);
  let fuenteSel = $state<string>("Todas");
  let query = $state("");

  // Dropdowns key-first: reemplazan los <select> nativos (no operables con el
  // control, que solo emite flechas/Enter). Mismo patrón que el home.
  let listaOpen = $state(false);
  let catOpen = $state(false);

  // Reproducción IN-APP: video + hls.js (HLS por MSE). Sin mpv.
  let playing = $state<Canal | null>(null);
  let playingIdx = $state(-1); // índice en `visibles` para canal anterior/siguiente
  let videoEl: HTMLVideoElement | null = $state(null);
  let playError = $state("");
  let buffering = $state(false);
  let hls: Hls | null = null;
  // Estado del reproductor para el HUD de controles.
  let paused = $state(false);
  let muted = $state(false);
  let volPct = $state(100);
  let controlsOn = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  function parseM3U(text: string, source: string): Canal[] {
    const lines = text.split(/\r?\n/);
    const out: Canal[] = [];
    let cur: Partial<Canal> | null = null;
    for (const raw of lines) {
      const line = raw.trim();
      if (line.startsWith("#EXTINF")) {
        const name = line.slice(line.lastIndexOf(",") + 1).trim() || "Sin nombre";
        const logo = (line.match(/tvg-logo="([^"]*)"/) || [])[1] || "";
        const gt = (line.match(/group-title="([^"]*)"/) || [])[1] || "";
        // group-title puede traer varias categorías unidas por ";".
        const groups = gt
          .split(/[;,]/)
          .map((g) => g.trim())
          .filter(Boolean);
        cur = { name, logo, groups: groups.length ? groups : ["Otros"], source };
      } else if (line && !line.startsWith("#")) {
        if (cur) {
          cur.url = line;
          out.push(cur as Canal);
          cur = null;
        }
      }
    }
    return out;
  }

  async function cargar() {
    cargando = true;
    errorMsg = "";
    const listas = config.iptvLists.length ? config.iptvLists : IPTV_DEFAULT_LISTS;
    try {
      // Carga todas las listas en paralelo; tolera que alguna falle.
      const resultados = await Promise.all(
        listas.map(async (l) => {
          try {
            const resp = await fetch(l.url.trim());
            if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
            return parseM3U(await resp.text(), l.name);
          } catch (e) {
            console.warn("[iptv] lista falló:", l.name, e);
            return [] as Canal[];
          }
        }),
      );
      const merged = resultados.flat();
      if (!merged.length) throw new Error("Ninguna lista trajo canales (revisá las URLs en config).");
      canales = merged;
      // Fuentes con al menos un canal (para el filtro por lista).
      const conFuente = listas.filter((l, i) => resultados[i].length).map((l) => l.name);
      fuentes = conFuente.length > 1 ? conFuente : [];
      if (!fuentes.includes(fuenteSel)) fuenteSel = "Todas";
      const conteo = new Map<string, number>();
      for (const c of merged) {
        for (const g of c.groups) conteo.set(g, (conteo.get(g) ?? 0) + 1);
      }
      grupos = [...conteo.entries()]
        .map(([name, count]) => ({ name, count }))
        .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
    } catch (e) {
      errorMsg = `No se pudo cargar IPTV: ${String(e)}`;
      canales = [];
      grupos = [];
      fuentes = [];
    } finally {
      cargando = false;
    }
  }

  // Lista filtrada (fuente + grupo + búsqueda), recortada a CAP.
  const filtrados = $derived.by(() => {
    const q = query.trim().toLowerCase();
    let base = canales;
    if (fuenteSel !== "Todas") base = base.filter((c) => c.source === fuenteSel);
    if (grupoSel !== "Todos") base = base.filter((c) => c.groups.includes(grupoSel));
    if (q) base = base.filter((c) => c.name.toLowerCase().includes(q));
    return base;
  });
  const visibles = $derived(filtrados.slice(0, CAP));
  const ocultos = $derived(Math.max(0, filtrados.length - CAP));

  // --- Navegación espacial key-first sobre [data-nav] (geometría de $lib/nav) ---
  // Geométrica: elige el candidato más cercano en la dirección pedida. Cubre por
  // igual el header (volver, filtros, buscador) y la grilla, sin depender de un
  // índice. Con un dropdown abierto, atrapa el foco dentro del menú.
  function spatialNav(dir: "up" | "down" | "left" | "right") {
    let all = Array.from(
      document.querySelectorAll<HTMLElement>("[data-nav]:not([disabled])"),
    ).filter((el) => el.offsetParent !== null);
    // Dropdown abierto: navegar solo entre sus opciones (foco atrapado).
    if (listaOpen || catOpen) {
      const menu = document.querySelector<HTMLElement>(".ddmenu");
      if (menu) all = Array.from(menu.querySelectorAll<HTMLElement>("[data-nav]"));
    }
    const cur = document.activeElement as HTMLElement | null;
    if (!cur || !cur.matches?.("[data-nav]")) { all[0]?.focus(); return; }
    const best = mejor(cur, all, dir);
    if (best) {
      best.focus({ preventScroll: false });
      best.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
    }
  }

  // Acción Svelte: enfoca la primera opción al abrir un dropdown.
  function autofocusFirst(node: HTMLElement) {
    tick().then(() => node.querySelector<HTMLElement>("[data-nav]")?.focus());
  }

  function cerrarMenus() {
    listaOpen = false;
    catOpen = false;
  }
  function toggleLista() { const o = !listaOpen; cerrarMenus(); listaOpen = o; }
  function toggleCat() { const o = !catOpen; cerrarMenus(); catOpen = o; }
  function elegirFuente(f: string) {
    fuenteSel = f;
    cerrarMenus();
    void focoPrimerCanal();
  }
  function elegirGrupo(g: string) {
    grupoSel = g;
    cerrarMenus();
    void focoPrimerCanal();
  }
  async function focoPrimerCanal() {
    await tick();
    document.querySelector<HTMLElement>(".canal")?.focus();
  }

  function destroyHls() {
    if (hls) {
      try { hls.destroy(); } catch { /* tolerado */ }
      hls = null;
    }
  }

  // Reproduce un canal. PRIMARIO: mpv+uosc (reproductor profesional, un solo
  // motor para IPTV y películas) con TODA la grilla como playlist → zapping
  // con ←/→. Si mpv no está (dev), cae al player in-app <video>+hls.js.
  async function reproducir(c: Canal, idx = -1) {
    const start = idx >= 0 ? idx : visibles.indexOf(c);
    const items = visibles.map((ch) => ({ url: ch.url, title: ch.name }));
    try {
      setNowPlaying("", c.name); // canal en vivo: no hay imdb para buscar subs
      // Un canal no va al historial: sin limpiar, el tracker seguiría sumando
      // minutos a la última película abierta desde el catálogo.
      limpiarContexto();
      await invoke("mpv_play_iptv", { items, start: Math.max(0, start) });
      return;
    } catch (e) {
      console.warn("[iptv] mpv no disponible, uso player in-app:", e);
    }
    await reproducirInApp(c, idx);
  }

  // Fallback: reproduce el canal DENTRO de la app en un <video>. HLS (.m3u8)
  // vía hls.js (MSE) o nativo si el webview lo soporta; otros van directo al src.
  async function reproducirInApp(c: Canal, idx = -1) {
    playing = c;
    playingIdx = idx >= 0 ? idx : visibles.indexOf(c);
    playError = "";
    buffering = true;
    mostrarControles();
    await tick();
    const v = videoEl;
    if (!v) return;
    destroyHls();
    const esHls = /\.m3u8(\?|$)/i.test(c.url);
    try {
      if (esHls && Hls.isSupported()) {
        hls = new Hls({ enableWorker: true, lowLatencyMode: true, backBufferLength: 30 });
        hls.loadSource(c.url);
        hls.attachMedia(v);
        hls.on(Hls.Events.MANIFEST_PARSED, () => void v.play().catch(() => {}));
        hls.on(Hls.Events.ERROR, (_e, data) => {
          if (!data.fatal) return;
          // Intento de recuperación antes de rendirse.
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) hls?.startLoad();
          else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) hls?.recoverMediaError();
          else { playError = "No se pudo reproducir este canal."; buffering = false; }
        });
      } else {
        // Safari/WebKit con HLS nativo, o stream directo (mp4/ts).
        v.src = c.url;
        await v.play().catch(() => {});
      }
    } catch (e) {
      playError = `Error: ${String(e)}`;
      buffering = false;
    }
  }

  function detener() {
    destroyHls();
    const v = videoEl;
    if (v) {
      try { v.pause(); v.removeAttribute("src"); v.load(); } catch { /* tolerado */ }
    }
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
    playing = null;
    playingIdx = -1;
    playError = "";
    buffering = false;
  }

  function togglePausa() {
    const v = videoEl;
    if (!v) return;
    if (v.paused) void v.play().catch(() => {});
    else v.pause();
  }
  function volRel(d: number) {
    const v = videoEl;
    if (v) v.volume = Math.max(0, Math.min(1, v.volume + d));
  }
  function toggleMute() {
    const v = videoEl;
    if (v) v.muted = !v.muted;
  }
  // Cambia de canal dentro de la lista visible (anterior/siguiente).
  function cambiarCanal(delta: number) {
    if (!visibles.length || playingIdx < 0) return;
    const next = playingIdx + delta;
    if (next < 0 || next >= visibles.length) return;
    void reproducir(visibles[next], next);
  }
  // Refleja el estado real del <video> en el HUD.
  function syncVideo() {
    const v = videoEl;
    if (!v) return;
    paused = v.paused;
    muted = v.muted;
    volPct = Math.round(v.volume * 100);
  }
  // Muestra los controles y programa su auto-ocultado.
  function mostrarControles() {
    controlsOn = true;
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => { controlsOn = false; }, 4000);
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    const tgt = e.target as HTMLElement | null;

    // En el buscador: dejar escribir; ↑/↓ saltan fuera; Escape lo desenfoca.
    if (tgt?.tagName === "INPUT") {
      // Sliders de la barra (brillo, volumen): las flechas son del control.
      if ((tgt as HTMLInputElement).type === "range") return;
      if (k === "Escape") (tgt as HTMLInputElement).blur();
      else if (k === "ArrowUp") { e.preventDefault(); spatialNav("up"); }
      else if (k === "ArrowDown") { e.preventDefault(); spatialNav("down"); }
      return;
    }

    // Reproductor abierto: controlar el <video> in-app.
    if (playing) {
      e.preventDefault();
      mostrarControles();
      if (k === "Escape" || k === "Backspace") detener();
      else if (k === " " || k === "Enter") togglePausa();
      else if (k === "ArrowLeft" || k === "[") cambiarCanal(-1);
      else if (k === "ArrowRight" || k === "]") cambiarCanal(1);
      else if (k === "ArrowUp") volRel(0.05);
      else if (k === "ArrowDown") volRel(-0.05);
      else if (k === "m" || k === "M") toggleMute();
      return;
    }

    // Dropdown abierto: Esc/Backspace lo cierran (no salen de la página).
    if ((listaOpen || catOpen) && (k === "Escape" || k === "Backspace")) {
      e.preventDefault();
      cerrarMenus();
      return;
    }

    switch (k) {
      case "Escape":
      case "Backspace":
        e.preventDefault();
        goto("/");
        break;
      case "ArrowRight": e.preventDefault(); spatialNav("right"); break;
      case "ArrowLeft": e.preventDefault(); spatialNav("left"); break;
      case "ArrowDown": e.preventDefault(); spatialNav("down"); break;
      case "ArrowUp": e.preventDefault(); spatialNav("up"); break;
      case "Enter":
      case " ": {
        // Activa el elemento enfocado (card, volver, filtro, opción). preventDefault
        // evita doble disparo con la activación nativa del teclado físico.
        const el = document.activeElement as HTMLElement | null;
        if (el?.matches?.("[data-nav]")) { e.preventDefault(); el.click(); }
        break;
      }
    }
  }

  onMount(async () => {
    ayuda.set("iptv", [
      { tecla: "← → ↑ ↓", desc: "Navegar canales" },
      { tecla: "Enter · A", desc: "Reproducir" },
      { tecla: "Esc · Backspace", desc: "Volver / detener" },
    ]);
    if (!config.loaded) loadConfig();
    await cargar();
    await focoPrimerCanal();
  });

  onDestroy(() => {
    destroyHls();
  });

  // hls.js en un <video> del webview: el escritorio no se entera solo de que
  // hay algo en pantalla. Derivado a booleano para no soltar y volver a pedir
  // la inhibición en cada cambio de canal.
  const viendo = $derived(playing !== null);
  $effect(() => {
    if (!viendo) return;
    inhibirReposo("iptv", true);
    return () => inhibirReposo("iptv", false);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="iptv">
  <header class="top">
    <button data-nav class="back" onclick={() => goto("/")} title="Volver (Esc)">‹ Volver</button>
    <h1>📡 TV en vivo</h1>
    {#if fuentes.length}
      <div class="dropdown">
        <span class="dd-label">Lista</span>
        <button data-nav class="dd-trigger" onclick={toggleLista} title="Elegir lista">
          <span>{fuenteSel}</span>
          <svg class="chev" viewBox="0 0 10 6" width="10" height="6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        </button>
        {#if listaOpen}
          <ul class="ddmenu" role="menu" use:autofocusFirst>
            <li><button data-nav class:active={fuenteSel === "Todas"} onclick={() => elegirFuente("Todas")}>Todas</button></li>
            {#each fuentes as f}
              <li><button data-nav class:active={fuenteSel === f} onclick={() => elegirFuente(f)}>{f}</button></li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
    {#if grupos.length}
      <div class="dropdown">
        <span class="dd-label">Categoría</span>
        <button data-nav class="dd-trigger" onclick={toggleCat} title="Elegir categoría">
          <span>{grupoSel === "Todos" ? `Todas (${canales.length})` : grupoSel}</span>
          <svg class="chev" viewBox="0 0 10 6" width="10" height="6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        </button>
        {#if catOpen}
          <ul class="ddmenu ddmenu-scroll" role="menu" use:autofocusFirst>
            <li><button data-nav class:active={grupoSel === "Todos"} onclick={() => elegirGrupo("Todos")}>Todas ({canales.length})</button></li>
            {#each grupos as g}
              <li><button data-nav class:active={grupoSel === g.name} onclick={() => elegirGrupo(g.name)}>{g.name} ({g.count})</button></li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
    <div class="search-wrap">
      <input
        data-nav
        class="search"
        bind:value={query}
        placeholder="Buscar canal…"
        spellcheck="false"
      />
      {#if query}
        <button data-nav class="search-clear" onclick={() => { query = ""; void focoPrimerCanal(); }} title="Limpiar">✕</button>
      {/if}
    </div>
  </header>

  {#if cargando}
    <div class="estado">Cargando lista de canales…</div>
  {:else if errorMsg}
    <div class="estado error">
      {errorMsg}
      <button class="reintentar" onclick={cargar}>Reintentar</button>
    </div>
  {:else if !visibles.length}
    <div class="estado">Sin canales para este filtro.</div>
  {:else}
    <div class="grid">
      {#each visibles as c, i (c.url + i)}
        <button
          class="canal"
          data-nav
          onclick={() => reproducir(c, i)}
          title={c.name}
        >
          <div class="logo-wrap">
            {#if c.logo}
              <img src={c.logo} alt="" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
            {:else}
              <span class="logo-fallback">📺</span>
            {/if}
          </div>
          <span class="nombre">{c.name}</span>
        </button>
      {/each}
    </div>
    {#if ocultos > 0}
      <p class="capnote">Mostrando {visibles.length} de {filtrados.length}. Afiná la búsqueda o el grupo para ver los {ocultos} restantes.</p>
    {/if}
  {/if}
</div>

{#if playing}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- Teclas se manejan global en onKey; aquí el click/mousemove solo revela el HUD. -->
  <div
    class="player"
    class:hide-cursor={!controlsOn}
    onmousemove={mostrarControles}
    onclick={mostrarControles}
  >
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={videoEl}
      class="video"
      autoplay
      playsinline
      onplay={syncVideo}
      onpause={syncVideo}
      onvolumechange={syncVideo}
      onplaying={() => { buffering = false; syncVideo(); }}
      onwaiting={() => (buffering = true)}
      onerror={() => { playError = "No se pudo reproducir este canal."; buffering = false; }}
    ></video>

    <!-- Barra superior: volver + canal + estado -->
    <div class="player-top" class:vis={controlsOn}>
      <button class="pbtn" onclick={detener} title="Volver (Esc)">‹ Volver</button>
      <span class="pname">{playing.name}</span>
      <span class="live">● EN VIVO</span>
      {#if buffering && !playError}<span class="pstate">⏳ Cargando…</span>{/if}
    </div>

    <!-- Barra inferior: controles -->
    <div class="player-ctrls" class:vis={controlsOn}>
      <button
        class="cbtn"
        title="Canal anterior (←)"
        disabled={playingIdx <= 0}
        onclick={() => cambiarCanal(-1)}
      >⏮</button>
      <button class="cbtn big" title="Pausa / Play (Espacio)" onclick={togglePausa}>
        {paused ? "▶" : "⏸"}
      </button>
      <button
        class="cbtn"
        title="Canal siguiente (→)"
        disabled={playingIdx >= visibles.length - 1}
        onclick={() => cambiarCanal(1)}
      >⏭</button>
      <button class="cbtn" title="Silenciar (M)" onclick={toggleMute}>
        {muted || volPct === 0 ? "🔇" : "🔊"}
      </button>
      <div class="vol">
        <div class="vol-fill" style="width:{muted ? 0 : volPct}%"></div>
      </div>
      <span class="vol-num">{muted ? 0 : volPct}%</span>
      <span class="spacer"></span>
      <button class="cbtn" title="Salir (Esc)" onclick={detener}>⏹</button>
    </div>

    {#if playError}
      <div class="player-err">
        <p>{playError}</p>
        <div class="err-actions">
          <button onclick={() => playing && reproducir(playing, playingIdx)}>Reintentar</button>
          <button onclick={detener}>Cerrar</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .iptv {
    padding: 18px 24px 40px;
    min-height: 100%;
    color: #eaeaea;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 14px;
  }
  .top h1 {
    font-size: 22px;
    font-weight: 800;
    margin: 0;
    white-space: nowrap;
  }
  .back {
    background: #1a1a22;
    border: 1px solid #2a2a35;
    color: #ccc;
    border-radius: 8px;
    padding: 8px 14px;
    cursor: pointer;
    font-weight: 600;
  }
  .back:hover,
  .back:focus-visible {
    background: #24242e;
    color: #fff;
  }
  .search-wrap {
    margin-left: auto;
    position: relative;
    display: flex;
    align-items: center;
  }
  .search {
    background: #15151c;
    border: 1px solid #2a2a35;
    color: #eee;
    border-radius: 8px;
    padding: 9px 32px 9px 14px;
    width: 320px;
    font-size: 14px;
  }
  .search:focus {
    outline: none;
    border-color: #f97316;
  }
  .search-clear {
    position: absolute;
    right: 8px;
    background: none;
    border: 0;
    color: #888;
    cursor: pointer;
    font-size: 13px;
  }

  .dropdown {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: #aaa;
  }
  .dd-label { white-space: nowrap; }
  .dd-trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    background: #15151c;
    border: 1px solid #2a2a35;
    color: #eee;
    border-radius: 8px;
    padding: 9px 12px;
    font-size: 14px;
    max-width: 260px;
    cursor: pointer;
  }
  .dd-trigger span {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dd-trigger .chev { color: #888; flex: none; }
  .dd-trigger:hover,
  .dd-trigger:focus,
  .dd-trigger:focus-visible {
    outline: none;
    border-color: #f97316;
    color: #fff;
  }
  .ddmenu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 60;
    list-style: none;
    margin: 0;
    padding: 6px;
    min-width: 100%;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 10px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ddmenu-scroll {
    max-height: 60vh;
    overflow-y: auto;
  }
  .ddmenu li { margin: 0; }
  .ddmenu button {
    width: 100%;
    text-align: left;
    background: none;
    border: 0;
    color: #ddd;
    border-radius: 7px;
    padding: 9px 12px;
    font-size: 13.5px;
    cursor: pointer;
    white-space: nowrap;
  }
  .ddmenu button:hover,
  .ddmenu button:focus,
  .ddmenu button:focus-visible {
    outline: none;
    background: #f97316;
    color: #1a0a02;
  }
  .ddmenu button.active { color: #f97316; font-weight: 700; }
  .ddmenu button.active:hover,
  .ddmenu button.active:focus { color: #1a0a02; }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 16px;
  }
  .canal {
    background: #121218;
    border: 1px solid #20202a;
    border-radius: 12px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    color: #ddd;
    transition: transform 0.12s, box-shadow 0.12s, border-color 0.12s;
  }
  .canal:hover {
    transform: translateY(-3px);
    border-color: #3a3a48;
  }
  .canal:focus,
  .canal:focus-visible {
    outline: 3px solid #f97316;
    outline-offset: 2px;
    transform: translateY(-3px) scale(1.03);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
    z-index: 2;
  }
  .logo-wrap {
    width: 100%;
    aspect-ratio: 1 / 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(ellipse at 50% 40%, #1b1b24 0%, #0c0c12 100%);
    border-radius: 8px;
    overflow: hidden;
  }
  .logo-wrap img {
    max-width: 88%;
    max-height: 88%;
    object-fit: contain;
  }
  .logo-fallback {
    font-size: 34px;
    opacity: 0.6;
  }
  .nombre {
    font-size: 12.5px;
    font-weight: 600;
    text-align: center;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .estado {
    padding: 60px 0;
    text-align: center;
    color: #999;
    font-size: 15px;
  }
  .estado.error {
    color: #f88;
  }
  .reintentar {
    display: block;
    margin: 14px auto 0;
    background: #f97316;
    border: 0;
    color: #1a0a02;
    font-weight: 700;
    border-radius: 8px;
    padding: 8px 18px;
    cursor: pointer;
  }
  .capnote {
    margin-top: 18px;
    color: #888;
    font-size: 13px;
    text-align: center;
  }

  /* Reproductor in-app: llena el área de contenido (debajo del header global,
     que es .app-content { position: relative } en el layout), NO el viewport.
     Así el header de la ventana queda visible mientras se reproduce. */
  .player {
    position: absolute;
    inset: 0;
    background: #000;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .video {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
  }
  .player.hide-cursor { cursor: none; }
  /* Barra superior */
  .player-top {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 20px;
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.8), transparent);
    opacity: 0;
    transform: translateY(-8px);
    transition: opacity 0.2s, transform 0.2s;
  }
  .player-top.vis { opacity: 1; transform: none; }
  .player-top .pbtn {
    background: rgba(20, 20, 26, 0.85);
    border: 1px solid #3a3a48;
    color: #eee;
    border-radius: 8px;
    padding: 8px 14px;
    cursor: pointer;
    font-weight: 600;
  }
  .player-top .pbtn:hover,
  .player-top .pbtn:focus-visible {
    background: #24242e;
  }
  .pname {
    font-weight: 700;
    color: #fff;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8);
  }
  .live {
    color: #ff4d4d;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 1px;
  }
  .pstate {
    color: #f97316;
    font-size: 13px;
  }

  /* Barra inferior de controles */
  .player-ctrls {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 18px 22px 22px;
    background: linear-gradient(0deg, rgba(0, 0, 0, 0.85), transparent);
    opacity: 0;
    transform: translateY(8px);
    transition: opacity 0.2s, transform 0.2s;
  }
  .player-ctrls.vis { opacity: 1; transform: none; }
  .cbtn {
    background: rgba(20, 20, 26, 0.85);
    border: 1px solid #3a3a48;
    color: #eee;
    border-radius: 10px;
    width: 48px;
    height: 48px;
    font-size: 20px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .cbtn.big {
    width: 60px;
    height: 60px;
    font-size: 26px;
    background: #f97316;
    border-color: #f97316;
    color: #1a0a02;
  }
  .cbtn:hover:not(:disabled),
  .cbtn:focus-visible {
    background: #2e2e3a;
    color: #fff;
  }
  .cbtn.big:hover:not(:disabled) { background: #fb8b3c; color: #1a0a02; }
  .cbtn:disabled { opacity: 0.35; cursor: default; }
  .vol {
    width: 120px;
    height: 6px;
    background: #3a3a48;
    border-radius: 3px;
    overflow: hidden;
  }
  .vol-fill {
    height: 100%;
    background: #f97316;
    transition: width 0.15s;
  }
  .vol-num {
    font-size: 12px;
    color: #bbb;
    min-width: 38px;
  }
  .spacer { flex: 1 1 auto; }
  .player-err {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    color: #eee;
    background: rgba(0, 0, 0, 0.7);
  }
  .err-actions {
    display: flex;
    gap: 12px;
  }
  .err-actions button {
    background: #f97316;
    border: 0;
    color: #1a0a02;
    font-weight: 700;
    border-radius: 8px;
    padding: 9px 18px;
    cursor: pointer;
  }
  .err-actions button:last-child {
    background: #2a2a35;
    color: #eee;
  }
</style>
