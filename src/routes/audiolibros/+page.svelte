<script lang="ts">
  // Audiolibros — LibriVox vía Internet Archive (ver $lib/audiolibros).
  //
  // Fase 1 del plan (docs/PLAN_AUDIOLIBROS_VIDEOCLIPS.md): catálogo, ficha con
  // capítulos y escucha en un <audio> del webview, sin mpv (no hay video que
  // mostrar, y así no se pelea con el WebView2 de Windows).
  //
  // Nav: flechas / d-pad mueven el foco · Enter / A activa · Esc / Backspace
  //   cierra la ficha o vuelve al inicio. El audio sigue sonando mientras se
  //   recorre el catálogo; se corta al salir de la página.

  import { onMount, onDestroy, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { inhibirReposo } from "$lib/reposo";
  import { mejor } from "$lib/nav";
  import { perfilInfantil } from "$lib/perfiles.svelte";
  import { limpiarContexto } from "$lib/historial.svelte";
  import {
    buscarLibros,
    etiquetaIdioma,
    fichaLibro,
    fmtDuracion,
    fmtLargo,
    POR_PAGINA,
    type Ficha,
    type FiltroIdioma,
    type Libro,
  } from "$lib/audiolibros";

  const IDIOMAS: { id: FiltroIdioma; nombre: string }[] = [
    { id: "es", nombre: "Español" },
    { id: "en", nombre: "Inglés" },
    { id: "todos", nombre: "Ambos" },
  ];
  const CLAVE_IDIOMA = "kutral.audiolibros.idioma";

  // Fase 1 sin filtro para niños: en un perfil infantil no se muestra nada.
  const ninos = $derived(perfilInfantil());

  // --- catálogo -------------------------------------------------------------
  let idioma = $state<FiltroIdioma>("es");
  let query = $state("");
  let libros = $state<Libro[]>([]);
  let total = $state(0);
  let pagina = $state(1);
  let cargando = $state(true);
  let cargandoMas = $state(false);
  let errorMsg = $state("");
  let pedido: AbortController | null = null;
  let buscarTimer: ReturnType<typeof setTimeout> | null = null;

  const hayMas = $derived(libros.length < total && pagina * POR_PAGINA < total);

  async function cargar(reset = true) {
    pedido?.abort();
    const ctl = new AbortController();
    pedido = ctl;
    const pag = reset ? 1 : pagina + 1;
    if (reset) cargando = true;
    else cargandoMas = true;
    errorMsg = "";
    try {
      const r = await buscarLibros(idioma, query, pag, ctl.signal);
      if (ctl.signal.aborted) return;
      total = r.total;
      pagina = pag;
      // Archive a veces repite un libro entre páginas: sin esto, el #each con
      // clave revienta.
      if (reset) libros = r.libros;
      else {
        const ya = new Set(libros.map((l) => l.id));
        libros = [...libros, ...r.libros.filter((l) => !ya.has(l.id))];
      }
    } catch (e) {
      if (ctl.signal.aborted) return;
      errorMsg = `No se pudo cargar el catálogo de audiolibros. (${e instanceof Error ? e.message : String(e)})`;
      if (reset) {
        libros = [];
        total = 0;
      }
    } finally {
      if (pedido === ctl) {
        cargando = false;
        cargandoMas = false;
      }
    }
  }

  function elegirIdioma(i: FiltroIdioma) {
    if (i === idioma) return;
    idioma = i;
    try { localStorage.setItem(CLAVE_IDIOMA, i); } catch { /* sin storage: no se recuerda */ }
    void cargar();
  }

  function alEscribir() {
    if (buscarTimer) clearTimeout(buscarTimer);
    buscarTimer = setTimeout(() => void cargar(), 450);
  }

  async function cargarMas() {
    const antes = libros.length;
    await cargar(false);
    await tick();
    // El foco sigue en el primer libro nuevo, no vuelve arriba.
    document.querySelectorAll<HTMLElement>(".libro")[antes]?.focus();
  }

  // --- ficha ----------------------------------------------------------------
  let ficha = $state<Ficha | null>(null);
  let fichaBase = $state<Libro | null>(null);
  let fichaCargando = $state(false);
  let fichaError = $state("");
  let fichaPedido: AbortController | null = null;
  let focoAntesDeFicha: HTMLElement | null = null;

  async function abrirFicha(l: Libro) {
    focoAntesDeFicha = document.activeElement as HTMLElement | null;
    fichaPedido?.abort();
    const ctl = new AbortController();
    fichaPedido = ctl;
    fichaBase = l;
    ficha = null;
    fichaError = "";
    fichaCargando = true;
    try {
      const f = await fichaLibro(l.id, ctl.signal);
      if (ctl.signal.aborted) return;
      if (!f.capitulos.length) fichaError = "Este audiolibro no trae capítulos en mp3.";
      ficha = f;
    } catch (e) {
      if (ctl.signal.aborted) return;
      fichaError = `No se pudo abrir el audiolibro. (${e instanceof Error ? e.message : String(e)})`;
    } finally {
      if (fichaPedido === ctl) fichaCargando = false;
    }
    await tick();
    document.querySelector<HTMLElement>(".ficha .descubrir, .ficha [data-nav]")?.focus();
  }

  function cerrarFicha() {
    fichaPedido?.abort();
    fichaBase = null;
    ficha = null;
    fichaError = "";
    fichaCargando = false;
    void tick().then(() => focoAntesDeFicha?.focus());
  }

  // --- escucha --------------------------------------------------------------
  let sonando = $state<{ ficha: Ficha; idx: number } | null>(null);
  let audioEl: HTMLAudioElement | null = $state(null);
  let pausado = $state(true);
  let actual = $state(0);
  let duracion = $state(0);
  let cargandoAudio = $state(false);
  let audioError = $state("");

  const capActual = $derived(sonando ? sonando.ficha.capitulos[sonando.idx] : null);

  async function escuchar(f: Ficha, idx: number) {
    const cap = f.capitulos[idx];
    if (!cap) return;
    // Un audiolibro todavía no va al historial (fase 2): sin limpiar, el
    // tracker seguiría sumándole minutos a la última película abierta.
    limpiarContexto();
    sonando = { ficha: f, idx };
    audioError = "";
    actual = 0;
    duracion = cap.segundos ?? 0;
    cargandoAudio = true;
    await tick();
    const a = audioEl;
    if (!a) return;
    a.src = cap.url;
    try {
      await a.play();
    } catch (e) {
      // AbortError = se pidió otro capítulo antes de que este arrancara.
      if (e instanceof DOMException && e.name === "AbortError") return;
      audioError = "No se pudo reproducir este capítulo.";
      cargandoAudio = false;
    }
    sesionMedia();
  }

  /** "Descubrir" de la ficha: si ya suena este libro, sigue; si no, empieza. */
  function descubrir() {
    if (!ficha?.capitulos.length) return;
    if (sonando?.ficha.id === ficha.id) {
      if (audioEl?.paused) void audioEl.play().catch(() => {});
      return;
    }
    void escuchar(ficha, 0);
  }

  function togglePausa() {
    const a = audioEl;
    if (!a || !sonando) return;
    if (a.paused) void a.play().catch(() => {});
    else a.pause();
  }

  function saltar(delta: number) {
    const a = audioEl;
    if (!a || !sonando) return;
    const fin = Number.isFinite(a.duration) ? a.duration : Infinity;
    a.currentTime = Math.max(0, Math.min(fin - 1, a.currentTime + delta));
  }

  function cambiarCapitulo(delta: number) {
    if (!sonando) return;
    const idx = sonando.idx + delta;
    if (idx < 0 || idx >= sonando.ficha.capitulos.length) return;
    void escuchar(sonando.ficha, idx);
  }

  function alTerminar() {
    if (!sonando) return;
    if (sonando.idx + 1 < sonando.ficha.capitulos.length) cambiarCapitulo(1);
    else pausado = true;
  }

  function buscarEn(e: MouseEvent) {
    const a = audioEl;
    if (!a || !Number.isFinite(a.duration) || a.duration <= 0) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    a.currentTime = f * a.duration;
  }

  function detener() {
    const a = audioEl;
    if (a) {
      try { a.pause(); a.removeAttribute("src"); a.load(); } catch { /* tolerado */ }
    }
    sonando = null;
    pausado = true;
    actual = 0;
    duracion = 0;
    audioError = "";
    cargandoAudio = false;
    if ("mediaSession" in navigator) navigator.mediaSession.metadata = null;
  }

  // Teclas multimedia del teclado y controles del sistema (el "ahora suena"
  // del escritorio), cuando el webview los ofrece.
  function sesionMedia() {
    if (!("mediaSession" in navigator) || !sonando) return;
    const ms = navigator.mediaSession;
    const cap = sonando.ficha.capitulos[sonando.idx];
    try {
      ms.metadata = new MediaMetadata({
        title: cap.titulo,
        artist: sonando.ficha.autor,
        album: sonando.ficha.titulo,
        artwork: [{ src: sonando.ficha.portada }],
      });
      ms.setActionHandler("play", () => togglePausa());
      ms.setActionHandler("pause", () => togglePausa());
      ms.setActionHandler("previoustrack", () => cambiarCapitulo(-1));
      ms.setActionHandler("nexttrack", () => cambiarCapitulo(1));
      ms.setActionHandler("seekbackward", () => saltar(-30));
      ms.setActionHandler("seekforward", () => saltar(30));
    } catch {
      /* acción no soportada: el resto sigue */
    }
  }

  // Suena (no pausado) = el escritorio no se duerme.
  const escuchando = $derived(sonando !== null && !pausado);
  $effect(() => {
    if (!escuchando) return;
    inhibirReposo("audiolibro", true);
    return () => inhibirReposo("audiolibro", false);
  });

  // --- teclado --------------------------------------------------------------
  function spatialNav(dir: "up" | "down" | "left" | "right") {
    // Con la ficha abierta el foco no sale de ella (ni de la barra de escucha).
    const raiz = ficha || fichaBase ? ".ficha [data-nav], .barra [data-nav]" : "[data-nav]";
    const all = Array.from(document.querySelectorAll<HTMLElement>(raiz))
      .filter((el) => !(el as HTMLButtonElement).disabled && el.offsetParent !== null);
    const cur = document.activeElement as HTMLElement | null;
    if (!cur || !all.includes(cur)) { all[0]?.focus(); return; }
    const best = mejor(cur, all, dir);
    if (best) {
      best.focus({ preventScroll: false });
      best.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
    }
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    const tgt = e.target as HTMLElement | null;

    // Teclas multimedia: siempre, estén donde estén.
    if (k === "MediaPlayPause") { e.preventDefault(); togglePausa(); return; }
    if (k === "MediaTrackNext") { e.preventDefault(); cambiarCapitulo(1); return; }
    if (k === "MediaTrackPrevious") { e.preventDefault(); cambiarCapitulo(-1); return; }

    // En el buscador: dejar escribir; ↑/↓ saltan fuera; Escape lo desenfoca.
    if (tgt?.tagName === "INPUT") {
      if ((tgt as HTMLInputElement).type === "range") return;
      if (k === "Escape") (tgt as HTMLInputElement).blur();
      else if (k === "Enter") { if (buscarTimer) clearTimeout(buscarTimer); void cargar(); }
      else if (k === "ArrowUp") { e.preventDefault(); spatialNav("up"); }
      else if (k === "ArrowDown") { e.preventDefault(); spatialNav("down"); }
      return;
    }

    switch (k) {
      case "Escape":
      case "Backspace":
        e.preventDefault();
        if (ficha || fichaBase) cerrarFicha();
        else goto("/");
        break;
      case "ArrowRight": e.preventDefault(); spatialNav("right"); break;
      case "ArrowLeft": e.preventDefault(); spatialNav("left"); break;
      case "ArrowDown": e.preventDefault(); spatialNav("down"); break;
      case "ArrowUp": e.preventDefault(); spatialNav("up"); break;
      case "Enter":
      case " ": {
        const el = document.activeElement as HTMLElement | null;
        if (el?.matches?.("[data-nav]")) { e.preventDefault(); el.click(); }
        break;
      }
    }
  }

  onMount(async () => {
    ayuda.set("audiolibros", [
      { tecla: "← → ↑ ↓", desc: "Navegar" },
      { tecla: "Enter · A", desc: "Abrir / Descubrir" },
      { tecla: "Esc · Backspace", desc: "Cerrar la ficha / volver" },
      { tecla: "⏯ ⏭ ⏮", desc: "Teclas multimedia: pausa y capítulos" },
    ]);
    try {
      const g = localStorage.getItem(CLAVE_IDIOMA);
      if (g === "es" || g === "en" || g === "todos") idioma = g;
    } catch { /* sin storage: español */ }
    if (ninos) { cargando = false; return; }
    await cargar();
    await tick();
    document.querySelector<HTMLElement>(".libro")?.focus();
  });

  onDestroy(() => {
    pedido?.abort();
    fichaPedido?.abort();
    if (buscarTimer) clearTimeout(buscarTimer);
    detener();
  });
</script>

<svelte:window onkeydown={onKey} />

<!-- El <audio> vive fuera de la barra: así el elemento no se recrea al
     mostrarla y el bind queda estable. -->
<audio
  bind:this={audioEl}
  preload="auto"
  onplay={() => (pausado = false)}
  onpause={() => (pausado = true)}
  onplaying={() => (cargandoAudio = false)}
  onwaiting={() => (cargandoAudio = true)}
  ontimeupdate={() => (actual = audioEl?.currentTime ?? 0)}
  ondurationchange={() => {
    const d = audioEl?.duration ?? 0;
    if (Number.isFinite(d) && d > 0) duracion = d;
  }}
  onended={alTerminar}
  onerror={() => {
    if (!sonando) return;
    audioError = "No se pudo reproducir este capítulo.";
    cargandoAudio = false;
  }}
></audio>

<div class="audiolibros" class:con-barra={sonando !== null}>
  <header class="top">
    <button data-nav class="back" onclick={() => goto("/")} title="Volver (Esc)">‹ Volver</button>
    <h1>🎧 Audiolibros</h1>
    {#if !ninos}
      <div class="idiomas" role="group" aria-label="Idioma">
        {#each IDIOMAS as i (i.id)}
          <button data-nav class="chip" class:active={idioma === i.id} onclick={() => elegirIdioma(i.id)}>
            {i.nombre}
          </button>
        {/each}
      </div>
      <div class="search-wrap">
        <input
          data-nav
          class="search"
          bind:value={query}
          oninput={alEscribir}
          placeholder="Buscar título o autor…"
          spellcheck="false"
        />
        {#if query}
          <button data-nav class="search-clear" onclick={() => { query = ""; void cargar(); }} title="Limpiar">✕</button>
        {/if}
      </div>
    {/if}
  </header>

  {#if ninos}
    <div class="estado">
      Los audiolibros todavía no están filtrados para niños.<br />
      Pídele a un adulto que los abra desde su perfil.
    </div>
  {:else if cargando}
    <div class="estado">Cargando audiolibros…</div>
  {:else if errorMsg && !libros.length}
    <div class="estado error">
      {errorMsg}
      <button data-nav class="reintentar" onclick={() => cargar()}>Reintentar</button>
    </div>
  {:else if !libros.length}
    <div class="estado">No hay audiolibros para esta búsqueda.</div>
  {:else}
    <p class="conteo">
      {total.toLocaleString("es")} audiolibros · clásicos de dominio público leídos por voluntarios de LibriVox
    </p>
    <div class="grid">
      {#each libros as l (l.id)}
        <button
          class="libro"
          class:sonando={sonando?.ficha.id === l.id}
          data-nav
          onclick={() => abrirFicha(l)}
          title={l.autor ? `${l.titulo} — ${l.autor}` : l.titulo}
        >
          <div class="portada">
            <img src={l.portada} alt="" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
            <span class="portada-fallback">🎧</span>
            {#if l.idioma}<span class="badge-idioma">{etiquetaIdioma(l.idioma)}</span>{/if}
            {#if sonando?.ficha.id === l.id}<span class="badge-sonando">♪</span>{/if}
          </div>
          <span class="titulo">{l.titulo}</span>
          {#if l.autor}<span class="autor">{l.autor}</span>{/if}
        </button>
      {/each}
    </div>
    {#if errorMsg}
      <p class="capnote error">{errorMsg}</p>
    {/if}
    {#if hayMas}
      <button data-nav class="mas" disabled={cargandoMas} onclick={cargarMas}>
        {cargandoMas ? "Cargando…" : "Ver más audiolibros"}
      </button>
    {/if}
  {/if}
</div>

{#if fichaBase}
  {@const f = ficha}
  {@const base = fichaBase}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- Teclas en onKey global; el click en el fondo cierra. -->
  <div class="ficha-fondo" class:con-barra={sonando !== null} onclick={(e) => e.target === e.currentTarget && cerrarFicha()}>
    <div class="ficha" role="dialog" aria-label={base.titulo}>
      <div class="ficha-cabeza">
        <img class="ficha-portada" src={base.portada} alt="" />
        <div class="ficha-datos">
          <h2>{f?.titulo ?? base.titulo}</h2>
          {#if (f?.autor ?? base.autor)}<p class="ficha-autor">{f?.autor ?? base.autor}</p>{/if}
          <p class="ficha-meta">
            {#if (f?.idioma ?? base.idioma)}
              <span class="badge-idioma enlinea">{etiquetaIdioma(f?.idioma ?? base.idioma)}</span>
            {/if}
            {#if f}
              {f.capitulos.length} {f.capitulos.length === 1 ? "capítulo" : "capítulos"}
              {#if f.segundosTotal} · {fmtLargo(f.segundosTotal)}{/if}
            {/if}
          </p>
          <div class="ficha-acciones">
            <button
              data-nav
              class="descubrir"
              disabled={!f?.capitulos.length}
              onclick={descubrir}
            >▶ Descubrir</button>
            <button data-nav class="cerrar" onclick={cerrarFicha} title="Cerrar (Esc)">Cerrar</button>
          </div>
        </div>
      </div>

      {#if fichaCargando}
        <p class="ficha-estado">Cargando capítulos…</p>
      {:else if fichaError}
        <p class="ficha-estado error">{fichaError}</p>
      {/if}

      {#if f}
        {#if f.descripcion}
          <p class="ficha-desc">{f.descripcion}</p>
        {/if}
        {#if f.capitulos.length}
          <ol class="capitulos">
            {#each f.capitulos as c, i (c.url)}
              {@const activo = sonando?.ficha.id === f.id && sonando.idx === i}
              <li>
                <button data-nav class="cap" class:activo onclick={() => escuchar(f, i)}>
                  <span class="cap-n">{activo ? (pausado ? "⏸" : "♪") : c.n}</span>
                  <span class="cap-titulo">{c.titulo}</span>
                  {#if c.segundos}<span class="cap-dur">{fmtDuracion(c.segundos)}</span>{/if}
                </button>
              </li>
            {/each}
          </ol>
        {/if}
      {/if}
    </div>
  </div>
{/if}

{#if sonando && capActual}
  {@const s = sonando}
  <div class="barra">
    <img class="barra-portada" src={s.ficha.portada} alt="" />
    <div class="barra-info">
      <span class="barra-libro">{s.ficha.titulo}</span>
      <span class="barra-cap">
        Capítulo {capActual.n} de {s.ficha.capitulos.length} · {capActual.titulo}
      </span>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="progreso" onclick={buscarEn}>
        <div class="progreso-fill" style="width:{duracion > 0 ? Math.min(100, (actual / duracion) * 100) : 0}%"></div>
      </div>
      <span class="tiempos">
        {fmtDuracion(actual)} / {fmtDuracion(duracion) || "–:––"}
        {#if cargandoAudio && !audioError} · cargando…{/if}
        {#if audioError}
          · <span class="error">{audioError}</span>
        {/if}
      </span>
    </div>
    <div class="barra-ctrls">
      <button data-nav class="cbtn" title="Capítulo anterior" disabled={s.idx <= 0} onclick={() => cambiarCapitulo(-1)}>⏮</button>
      <button data-nav class="cbtn" title="Atrás 30 s" onclick={() => saltar(-30)}>↺30</button>
      {#if audioError}
        <button data-nav class="cbtn big" title="Reintentar" onclick={() => escuchar(s.ficha, s.idx)}>↻</button>
      {:else}
        <button data-nav class="cbtn big" title="Pausa / seguir" onclick={togglePausa}>{pausado ? "▶" : "⏸"}</button>
      {/if}
      <button data-nav class="cbtn" title="Adelante 30 s" onclick={() => saltar(30)}>30↻</button>
      <button data-nav class="cbtn" title="Capítulo siguiente" disabled={s.idx >= s.ficha.capitulos.length - 1} onclick={() => cambiarCapitulo(1)}>⏭</button>
      <button data-nav class="cbtn" title="Detener" onclick={detener}>⏹</button>
    </div>
  </div>
{/if}

<style>
  .audiolibros {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    padding: 18px 24px 40px;
    color: #eaeaea;
  }
  .audiolibros.con-barra { padding-bottom: 130px; }
  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 10px;
    flex-wrap: wrap;
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
  .idiomas { display: flex; gap: 6px; }
  .chip {
    background: #15151c;
    border: 1px solid #2a2a35;
    color: #ccc;
    border-radius: 999px;
    padding: 7px 14px;
    font-size: 13.5px;
    cursor: pointer;
  }
  .chip.active {
    background: #f59e0b;
    border-color: #f59e0b;
    color: #1a0f02;
    font-weight: 700;
  }
  .chip:hover,
  .chip:focus-visible {
    outline: none;
    border-color: #f59e0b;
    color: #fff;
  }
  .chip.active:hover,
  .chip.active:focus-visible { color: #1a0f02; box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.35); }
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
    width: 300px;
    font-size: 14px;
  }
  .search:focus {
    outline: none;
    border-color: #f59e0b;
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
  .conteo {
    color: #8a8a96;
    font-size: 12.5px;
    margin: 0 0 14px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 16px;
  }
  .libro {
    background: #121218;
    border: 1px solid #20202a;
    border-radius: 12px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    cursor: pointer;
    color: #ddd;
    text-align: left;
    font: inherit;
    transition: transform 0.12s, box-shadow 0.12s, border-color 0.12s;
  }
  .libro:hover {
    transform: translateY(-3px);
    border-color: #3a3a48;
  }
  .libro:focus,
  .libro:focus-visible {
    outline: 3px solid #f59e0b;
    outline-offset: 2px;
    transform: translateY(-3px) scale(1.03);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
    z-index: 2;
  }
  .libro.sonando { border-color: #f59e0b; }
  .portada {
    position: relative;
    width: 100%;
    aspect-ratio: 1 / 1;
    border-radius: 8px;
    overflow: hidden;
    background: radial-gradient(ellipse at 50% 40%, #2a1d08 0%, #120c04 100%);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .portada img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    z-index: 1;
  }
  .portada-fallback {
    font-size: 38px;
    opacity: 0.5;
  }
  .badge-idioma {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 2;
    background: rgba(10, 8, 6, 0.85);
    color: #fcd34d;
    border: 1px solid rgba(245, 158, 11, 0.5);
    border-radius: 6px;
    padding: 2px 6px;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.5px;
  }
  .badge-idioma.enlinea {
    position: static;
    margin-right: 8px;
  }
  .badge-sonando {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
    background: #f59e0b;
    color: #1a0f02;
    border-radius: 999px;
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 800;
    font-size: 13px;
  }
  .titulo {
    font-size: 13px;
    font-weight: 700;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .autor {
    font-size: 12px;
    color: #9a9aa6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mas {
    display: block;
    margin: 22px auto 0;
    background: #1a1a22;
    border: 1px solid #2a2a35;
    color: #eee;
    border-radius: 10px;
    padding: 11px 22px;
    font-weight: 700;
    cursor: pointer;
  }
  .mas:hover,
  .mas:focus-visible {
    outline: none;
    border-color: #f59e0b;
    background: #24242e;
  }

  .estado {
    padding: 60px 0;
    text-align: center;
    color: #999;
    font-size: 15px;
    line-height: 1.6;
  }
  .error { color: #f88; }
  .reintentar {
    display: block;
    margin: 14px auto 0;
    background: #f59e0b;
    border: 0;
    color: #1a0f02;
    font-weight: 700;
    border-radius: 8px;
    padding: 8px 18px;
    cursor: pointer;
  }
  .capnote {
    margin-top: 18px;
    font-size: 13px;
    text-align: center;
  }

  /* Ficha: sobre el catálogo, dentro del área de contenido (como el player
     de IPTV). Con la barra de escucha visible deja su lugar abajo. */
  .ficha-fondo {
    position: absolute;
    inset: 0;
    z-index: 50;
    background: rgba(5, 4, 6, 0.78);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 28px 20px;
    overflow-y: auto;
  }
  .ficha-fondo.con-barra { bottom: 104px; }
  .ficha {
    width: min(820px, 100%);
    background: #121218;
    border: 1px solid #2a2a35;
    border-radius: 16px;
    padding: 22px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.7);
    color: #e6e6ea;
  }
  .ficha-cabeza {
    display: flex;
    gap: 20px;
  }
  .ficha-portada {
    width: 180px;
    height: 180px;
    object-fit: cover;
    border-radius: 10px;
    flex: none;
    background: #1d1508;
  }
  .ficha-datos {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .ficha-datos h2 {
    margin: 0;
    font-size: 22px;
    font-weight: 800;
    line-height: 1.2;
  }
  .ficha-autor {
    margin: 0;
    color: #c9c9d2;
    font-size: 15px;
  }
  .ficha-meta {
    margin: 2px 0 0;
    color: #9a9aa6;
    font-size: 13px;
    display: flex;
    align-items: center;
  }
  .ficha-acciones {
    display: flex;
    gap: 10px;
    margin-top: auto;
    padding-top: 10px;
  }
  .descubrir {
    background: #f59e0b;
    border: 0;
    color: #1a0f02;
    font-weight: 800;
    border-radius: 10px;
    padding: 11px 22px;
    font-size: 15px;
    cursor: pointer;
  }
  .descubrir:disabled { opacity: 0.4; cursor: default; }
  .descubrir:hover:not(:disabled),
  .descubrir:focus-visible {
    outline: 3px solid #fde68a;
    outline-offset: 2px;
  }
  .cerrar {
    background: #1f1f28;
    border: 1px solid #33333f;
    color: #ddd;
    border-radius: 10px;
    padding: 11px 18px;
    cursor: pointer;
    font-weight: 600;
  }
  .cerrar:hover,
  .cerrar:focus-visible {
    outline: none;
    border-color: #f59e0b;
    color: #fff;
  }
  .ficha-estado {
    margin: 16px 0 0;
    color: #9a9aa6;
    font-size: 14px;
  }
  .ficha-desc {
    margin: 16px 0 0;
    white-space: pre-line;
    color: #c4c4cc;
    font-size: 14px;
    line-height: 1.55;
    max-height: 9.5em;
    overflow-y: auto;
  }
  .capitulos {
    list-style: none;
    margin: 16px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .cap {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    background: none;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 9px 12px;
    color: #ddd;
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
  }
  .cap:hover,
  .cap:focus-visible {
    outline: none;
    background: #1f1f28;
    border-color: #f59e0b;
    color: #fff;
  }
  .cap.activo { color: #fcd34d; font-weight: 700; }
  .cap-n {
    flex: none;
    width: 28px;
    text-align: right;
    color: #8a8a96;
    font-variant-numeric: tabular-nums;
  }
  .cap.activo .cap-n { color: #f59e0b; }
  .cap-titulo {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cap-dur {
    flex: none;
    color: #8a8a96;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  /* Barra de escucha: abajo del área de contenido, por encima de todo. */
  .barra {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 60;
    height: 104px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 22px;
    background: linear-gradient(0deg, #0d0a07 0%, rgba(18, 14, 10, 0.97) 100%);
    border-top: 1px solid #2e2414;
    box-shadow: 0 -10px 30px rgba(0, 0, 0, 0.5);
  }
  .barra-portada {
    width: 76px;
    height: 76px;
    border-radius: 8px;
    object-fit: cover;
    flex: none;
    background: #1d1508;
  }
  .barra-info {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .barra-libro {
    font-weight: 800;
    font-size: 14.5px;
    color: #fff;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .barra-cap {
    font-size: 12.5px;
    color: #c9b48a;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .progreso {
    height: 6px;
    background: #3a3020;
    border-radius: 3px;
    overflow: hidden;
    cursor: pointer;
    margin-top: 2px;
  }
  .progreso-fill {
    height: 100%;
    background: #f59e0b;
    transition: width 0.25s linear;
  }
  .tiempos {
    font-size: 12px;
    color: #9a9aa6;
    font-variant-numeric: tabular-nums;
  }
  .barra-ctrls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .cbtn {
    background: rgba(28, 24, 20, 0.9);
    border: 1px solid #3a3020;
    color: #eee;
    border-radius: 10px;
    min-width: 46px;
    height: 46px;
    padding: 0 8px;
    font-size: 16px;
    font-weight: 700;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .cbtn.big {
    width: 58px;
    height: 58px;
    font-size: 24px;
    background: #f59e0b;
    border-color: #f59e0b;
    color: #1a0f02;
  }
  .cbtn:hover:not(:disabled),
  .cbtn:focus-visible {
    outline: none;
    background: #2e2618;
    border-color: #f59e0b;
    color: #fff;
  }
  .cbtn.big:hover:not(:disabled),
  .cbtn.big:focus-visible {
    background: #fbbf24;
    color: #1a0f02;
    box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.35);
  }
  .cbtn:disabled { opacity: 0.35; cursor: default; }
</style>
