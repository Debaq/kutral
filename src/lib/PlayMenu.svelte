<script lang="ts">
  // Vista completa al entrar a un título. Paleta de la marca (oro / naranja
  // cálido sobre fondo cinematográfico). Sin iframe embebido para que el foco
  // no quede capturado y Esc/Backspace siempre puedan cerrar.

  import { onMount } from "svelte";
  import { WEB_PLAYER_ENABLED } from "$lib/features";

  type Action = {
    id: string;
    label: string;
    run: () => void;
    primary?: boolean;
  };

  type PersonMini = {
    id: number;
    name: string;
    profile_path?: string;
    character?: string;
    job?: string;
  };

  type OmdbRating = { source: string; value: string };

  type OmdbDetail = {
    plot?: string | null;
    awards?: string | null;
    rated?: string | null;
    writer?: string | null;
    country?: string | null;
    language?: string | null;
    released?: string | null;
    metascore?: string | null;
    imdb_rating?: string | null;
    imdb_votes?: string | null;
    box_office?: string | null;
    production?: string | null;
    ratings: OmdbRating[];
  };

  type DetailExt = {
    title: string;
    overview?: string;
    poster_path?: string;
    backdrop_path?: string;
    vote_average?: number;
    year?: string;
    runtime?: number;
    genres?: string[];
    directors?: PersonMini[];
    cast?: PersonMini[];
    tagline?: string | null;
    original_title?: string | null;
    original_language?: string | null;
    status?: string | null;
    budget?: number | null;
    revenue?: number | null;
    vote_count?: number | null;
    release_date?: string | null;
    production_companies?: string[];
    production_countries?: string[];
    spoken_languages?: string[];
    homepage?: string | null;
  };

  let {
    detail,
    omdb = null,
    progressLabel,
    posterUrl = null,
    backdropUrl = null,
    hasTrailer = false,
    hasRd,
    onContinue,
    onRestart,
    onRealDebrid,
    onResearch,
    onTrailer,
    onClose,
  }: {
    detail: DetailExt;
    omdb?: OmdbDetail | null;
    progressLabel: string | null;
    posterUrl?: string | null;
    backdropUrl?: string | null;
    /** Hay trailer que mostrar (se reproduce en mpv o se ofrece por QR). */
    hasTrailer?: boolean;
    hasRd: boolean;
    onContinue: () => void;
    onRestart: () => void;
    onRealDebrid: () => void;
    onResearch: () => void;
    onTrailer: () => void;
    onClose: () => void;
  } = $props();

  // Despachador estable: el botón solo pasa el id, evitando que closures
  // capturen versiones viejas de las props.
  function runId(id: string): void {
    switch (id) {
      case "cont":
      case "discover":
        onContinue();
        break;
      case "restart":
        onRestart();
        break;
      case "rd":
        onRealDebrid();
        break;
      case "research":
        onResearch();
        break;
      case "trailer":
        onTrailer();
        break;
      // "web" eliminado: era casi igual a "Descubrir" (solo saltaba el check
      // de no-disponible). Si una peli está marcada como no disponible, la
      // pantalla "Lo sentimos" ya ofrece "Intentar de todas formas".
    }
  }

  type ActionItem = { id: string; label: string; primary?: boolean };

  const actions: ActionItem[] = $derived.by(() => {
    // Acciones del player WEB (vidapi). Ver WEB_PLAYER_ENABLED: hoy apagadas.
    const web: ActionItem[] = [];
    if (WEB_PLAYER_ENABLED) {
      if (progressLabel) {
        web.push({ id: "cont", label: `▶  Continuar (${progressLabel})` });
        web.push({ id: "restart", label: "↻  Empezar de nuevo" });
      } else {
        web.push({ id: "discover", label: hasRd ? "🌐  Ver en web" : "▶  Descubrir" });
      }
    }

    const a: ActionItem[] = [];
    if (hasRd || !web.length) {
      // Prioridad: si hay RealDebrid, va debrid primero (mejor calidad, directo
      // a mpv) y el player web queda como alternativa.
      //
      // Sin debrid la misma entrada sirve igual: SourcePicker busca fuentes y
      // las baja en local si el usuario activó esa opción. Por eso el nombre
      // cambia — "Ver con debrid" mentiría cuando no hay debrid.
      a.push({
        id: "rd",
        label: progressLabel
          ? `⚡  Continuar (${progressLabel})`
          : hasRd
            ? "⚡  Ver con debrid"
            : "▶  Descubrir",
        primary: true,
      });
      // El historial guarda el minuto: si hay algo empezado, hace falta la
      // salida para verlo desde el principio otra vez.
      if (progressLabel) a.push({ id: "restart", label: "↻  Empezar de nuevo" });
      a.push({ id: "research", label: "🔄  Rebuscar fuentes" });
      a.push(...web);
    } else {
      web[0].primary = true;
      a.push(...web);
    }
    if (hasTrailer) {
      a.push({ id: "trailer", label: "🎬  Ver trailer" });
    }
    return a;
  });

  // -1 = la tarjeta grande del trailer (vive en la columna de info, no en el
  // panel de acciones). Sin ella en el anillo el botón más visible de la
  // pantalla era inalcanzable con teclado o mando: ↑↓ solo recorrían .pm-btn
  // y ←→ hacen scroll, así que no había forma de llegar.
  let focusIdx = $state(0);
  let panelRef: HTMLElement | null = $state(null);
  let scrollRef: HTMLDivElement | null = $state(null);
  let trailerCardRef: HTMLButtonElement | null = $state(null);

  // Mantener índice válido si la lista de acciones cambia.
  $effect(() => {
    if (focusIdx >= actions.length) focusIdx = Math.max(0, actions.length - 1);
  });

  // Focus inicial al primer botón → asegura que Esc/Backspace lleguen a window.
  onMount(() => {
    setTimeout(() => {
      const btn = panelRef?.querySelector<HTMLButtonElement>(".pm-btn");
      btn?.focus();
    }, 50);
  });

  /** Botones del anillo, en orden visual: tarjeta de trailer y luego acciones. */
  function anillo(): HTMLElement[] {
    const btns = panelRef
      ? Array.from(panelRef.querySelectorAll<HTMLButtonElement>(".pm-btn"))
      : [];
    return trailerCardRef ? [trailerCardRef, ...btns] : btns;
  }

  function move(d: number) {
    const ring = anillo();
    if (!ring.length) return;
    const hero = trailerCardRef ? 1 : 0;
    let i = (focusIdx < 0 ? 0 : focusIdx + hero) + d;
    if (i < 0) i = ring.length - 1;
    if (i > ring.length - 1) i = 0;
    focusIdx = hero && i === 0 ? -1 : i - hero;
    const el = ring[i];
    el?.focus();
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  function scrollInfo(dy: number) {
    scrollRef?.scrollBy({ top: dy, behavior: "smooth" });
  }

  function onKey(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "ArrowLeft":
        e.preventDefault();
        scrollInfo(-200);
        break;
      case "ArrowRight":
        e.preventDefault();
        scrollInfo(200);
        break;
      case "PageUp":
        e.preventDefault();
        scrollInfo(-450);
        break;
      case "PageDown":
        e.preventDefault();
        scrollInfo(450);
        break;
      case "Home":
        e.preventDefault();
        scrollRef?.scrollTo({ top: 0, behavior: "smooth" });
        break;
      case "End":
        e.preventDefault();
        scrollRef?.scrollTo({ top: scrollRef.scrollHeight, behavior: "smooth" });
        break;
      case "Enter":
      case " ":
        if ((e.target as HTMLElement | null)?.classList.contains("pm-btn")) {
          e.preventDefault();
          const a = actions[focusIdx];
          if (a) runId(a.id);
        }
        break;
      case "Escape":
      case "Backspace":
        // Cinturón y tirantes: +page.svelte también lo maneja.
        e.preventDefault();
        e.stopPropagation();
        onClose();
        break;
    }
  }

  // Formatos.
  function fmtMoney(n: number | null | undefined): string | null {
    if (n == null || n <= 0) return null;
    if (n >= 1_000_000_000)
      return `US$ ${(n / 1_000_000_000).toFixed(2).replace(/\.?0+$/, "")} B`;
    if (n >= 1_000_000)
      return `US$ ${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")} M`;
    if (n >= 1_000) return `US$ ${(n / 1_000).toFixed(0)} K`;
    return `US$ ${n}`;
  }

  function fmtRuntime(min: number | undefined): string | null {
    if (!min) return null;
    if (min < 60) return `${min} min`;
    const h = Math.floor(min / 60);
    const m = min % 60;
    return m === 0 ? `${h}h` : `${h}h ${m}m`;
  }

  function fmtLang(code: string | null | undefined): string {
    if (!code) return "";
    const map: Record<string, string> = {
      en: "Inglés", es: "Español", fr: "Francés", de: "Alemán",
      it: "Italiano", pt: "Portugués", ja: "Japonés", ko: "Coreano",
      zh: "Chino", ru: "Ruso", ar: "Árabe", hi: "Hindi",
    };
    return map[code] || code.toUpperCase();
  }

  const tagline = $derived(detail.tagline?.trim() || "");
  const plotLong = $derived(omdb?.plot?.trim() || detail.overview?.trim() || "");
  const genres = $derived(detail.genres || []);
  const directors = $derived(detail.directors || []);
  const cast = $derived((detail.cast || []).slice(0, 8));
  const companies = $derived((detail.production_companies || []).slice(0, 6));
  const countries = $derived(detail.production_countries || []);
  const langs = $derived(detail.spoken_languages || []);
  const ratings = $derived(omdb?.ratings || []);
  const ratingTmdb = $derived(
    detail.vote_average && detail.vote_average > 0
      ? `${detail.vote_average.toFixed(1)} / 10`
      : null,
  );
  const budget = $derived(fmtMoney(detail.budget));
  const revenue = $derived(fmtMoney(detail.revenue));
  const profit = $derived(
    detail.budget && detail.revenue && detail.budget > 0 && detail.revenue > 0
      ? fmtMoney(detail.revenue - detail.budget)
      : null,
  );
</script>

<svelte:window onkeydown={onKey} />

<div class="pm-root">
  {#if backdropUrl}
    <div class="pm-bg" style:background-image="url({backdropUrl})"></div>
  {/if}
  <div class="pm-scrim"></div>

  <div class="pm-hint-back">Esc o Backspace para volver</div>

  <div class="pm-layout">
    <div class="pm-scroll" bind:this={scrollRef}>
      <header class="pm-hero">
        {#if posterUrl}
          <img class="pm-poster" src={posterUrl} alt="" />
        {/if}
        <div class="pm-hero-text">
          <h1 class="pm-title">{detail.title}</h1>
          {#if detail.original_title && detail.original_title !== detail.title}
            <p class="pm-original">{detail.original_title}</p>
          {/if}
          {#if tagline}
            <p class="pm-tagline">«{tagline}»</p>
          {/if}
          <div class="pm-meta">
            {#if detail.year}<span class="pm-year">{detail.year}</span>{/if}
            {#if omdb?.rated}<span class="chip">{omdb.rated}</span>{/if}
            {#if fmtRuntime(detail.runtime)}<span>{fmtRuntime(detail.runtime)}</span>{/if}
            {#if detail.status && detail.status !== "Released"}
              <span class="chip warn">{detail.status}</span>
            {/if}
          </div>
          {#if genres.length}
            <div class="pm-genres">
              {#each genres as g}<span class="genre-chip">{g}</span>{/each}
            </div>
          {/if}
        </div>
      </header>

      {#if hasTrailer}
        <button
          bind:this={trailerCardRef}
          class="pm-trailer-card"
          class:focused={focusIdx === -1}
          onclick={onTrailer}
          onfocus={() => (focusIdx = -1)}
          title="Ver trailer (pantalla completa)"
          style:background-image={backdropUrl ? `url(${backdropUrl})` : undefined}
        >
          <div class="pm-trailer-scrim"></div>
          <div class="pm-trailer-content">
            <span class="pm-trailer-play">▶</span>
            <span class="pm-trailer-label">Ver trailer</span>
          </div>
        </button>
      {/if}

      {#if ratings.length || ratingTmdb || omdb?.imdb_rating}
        <section class="pm-section pm-ratings">
          {#if ratingTmdb}
            <div class="rating">
              <span class="rating-label">TMDb</span>
              <span class="rating-value">★ {ratingTmdb}</span>
              {#if detail.vote_count}<span class="rating-sub">{detail.vote_count.toLocaleString()} votos</span>{/if}
            </div>
          {/if}
          {#if omdb?.imdb_rating}
            <div class="rating">
              <span class="rating-label">IMDb</span>
              <span class="rating-value">★ {omdb.imdb_rating}</span>
              {#if omdb.imdb_votes}<span class="rating-sub">{omdb.imdb_votes} votos</span>{/if}
            </div>
          {/if}
          {#each ratings as r}
            {#if r.source !== "Internet Movie Database"}
              {@const isRT = r.source === "Rotten Tomatoes"}
              {@const pct = isRT ? parseInt(r.value, 10) : NaN}
              {@const fresh = !Number.isNaN(pct) && pct >= 60}
              <div class="rating" class:rt-fresh={isRT && fresh} class:rt-rotten={isRT && !fresh && !Number.isNaN(pct)}>
                <span class="rating-label">
                  {#if isRT}🍅 Rotten Tomatoes{:else if r.source === "Metacritic"}Ⓜ Metacritic{:else}{r.source}{/if}
                </span>
                <span class="rating-value">{r.value}</span>
              </div>
            {/if}
          {/each}
        </section>
      {/if}

      {#if plotLong}
        <section class="pm-section">
          <h3 class="pm-h3">Sinopsis</h3>
          <p class="pm-plot">{plotLong}</p>
        </section>
      {/if}

      {#if omdb?.awards}
        <section class="pm-section">
          <h3 class="pm-h3">🏆 Premios</h3>
          <p class="pm-text">{omdb.awards}</p>
        </section>
      {/if}

      {#if budget || revenue}
        <section class="pm-section">
          <h3 class="pm-h3">💰 Recaudación</h3>
          <div class="pm-grid">
            {#if budget}<div><span class="lbl">Presupuesto</span><span class="val">{budget}</span></div>{/if}
            {#if revenue}<div><span class="lbl">Taquilla</span><span class="val">{revenue}</span></div>{/if}
            {#if profit}
              <div>
                <span class="lbl">Ganancia</span>
                <span class="val" class:loss={profit.startsWith("US$ -")}>{profit}</span>
              </div>
            {/if}
            {#if omdb?.box_office && (!revenue || omdb.box_office !== revenue)}
              <div><span class="lbl">Box office (USA)</span><span class="val">{omdb.box_office}</span></div>
            {/if}
          </div>
        </section>
      {/if}

      {#if directors.length || cast.length || omdb?.writer}
        <section class="pm-section">
          {#if directors.length}
            <h3 class="pm-h3">{directors[0].job === "Creador" ? "Creado por" : "Dirigido por"}</h3>
            <p class="pm-people">{directors.map((d) => d.name).join(", ")}</p>
          {/if}
          {#if omdb?.writer}
            <h3 class="pm-h3">Guion</h3>
            <p class="pm-people">{omdb.writer}</p>
          {/if}
          {#if cast.length}
            <h3 class="pm-h3">Reparto</h3>
            <p class="pm-people">{cast.map((c) => c.name).join(", ")}</p>
          {/if}
        </section>
      {/if}

      {#if companies.length || countries.length || langs.length || omdb?.released || detail.release_date || omdb?.production}
        <section class="pm-section">
          <h3 class="pm-h3">Ficha técnica</h3>
          <div class="pm-facts">
            {#if omdb?.released || detail.release_date}
              <div><span class="lbl">Estreno</span><span class="val">{omdb?.released || detail.release_date}</span></div>
            {/if}
            {#if detail.original_language}
              <div><span class="lbl">Idioma original</span><span class="val">{fmtLang(detail.original_language)}</span></div>
            {/if}
            {#if langs.length}
              <div><span class="lbl">Idiomas</span><span class="val">{langs.join(", ")}</span></div>
            {/if}
            {#if countries.length}
              <div><span class="lbl">País</span><span class="val">{countries.join(", ")}</span></div>
            {/if}
            {#if companies.length}
              <div><span class="lbl">Producción</span><span class="val">{companies.join(", ")}</span></div>
            {/if}
            {#if omdb?.production && !companies.length}
              <div><span class="lbl">Productora</span><span class="val">{omdb.production}</span></div>
            {/if}
          </div>
        </section>
      {/if}
    </div>

    <aside class="pm-panel" bind:this={panelRef}>
      <p class="pm-sub">¿Cómo quieres verla?</p>
      <p class="pm-keyhint">↑↓ opciones y trailer · ←→ scroll info · Esc cerrar</p>
      <div class="pm-list">
        {#each actions as a, i (a.id)}
          <button
            class="pm-btn"
            class:focused={focusIdx === i}
            class:primary={a.primary}
            onclick={() => runId(a.id)}
            onmouseenter={() => (focusIdx = i)}
            onfocus={() => (focusIdx = i)}
          >
            {a.label}
          </button>
        {/each}
      </div>
    </aside>
  </div>
</div>

<style>
  .pm-root {
    position: fixed;
    inset: 0;
    z-index: 1500;
    overflow: hidden;
    background: #0d0d12;
    color: #ffe6c8;
    font-family: "Inter", system-ui, sans-serif;
  }
  .pm-bg {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(12px) saturate(1.15);
    transform: scale(1.08);
    opacity: 0.7;
  }
  .pm-scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, rgba(13, 6, 4, 0.94) 0%, rgba(13, 6, 4, 0.78) 55%, rgba(13, 6, 4, 0.9) 100%),
      linear-gradient(0deg, rgba(13, 6, 4, 0.85) 0%, rgba(13, 6, 4, 0.15) 50%);
  }

  .pm-hint-back {
    position: absolute;
    top: 14px;
    left: 18px;
    z-index: 10;
    color: #998878;
    font-size: 12px;
    letter-spacing: 0.04em;
    pointer-events: none;
  }

  .pm-layout {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 380px;
    gap: 40px;
    padding: 40px 48px 32px;
    box-sizing: border-box;
  }
  .pm-scroll {
    overflow-y: auto;
    padding-right: 18px;
    scrollbar-width: thin;
    scrollbar-color: #5a3a1a #14141a;
  }
  .pm-scroll::-webkit-scrollbar { width: 8px; }
  .pm-scroll::-webkit-scrollbar-thumb { background: #5a3a1a; border-radius: 4px; }

  .pm-hero {
    display: flex;
    gap: 24px;
    margin-bottom: 24px;
  }
  .pm-poster {
    width: 170px;
    height: 255px;
    object-fit: cover;
    border-radius: 10px;
    box-shadow:
      0 12px 32px rgba(0, 0, 0, 0.65),
      0 0 0 1px rgba(245, 197, 24, 0.3);
    flex-shrink: 0;
  }
  .pm-hero-text { flex: 1; min-width: 0; }
  .pm-title {
    margin: 0;
    font-size: 38px;
    font-weight: 700;
    color: #fff;
    line-height: 1.05;
    letter-spacing: -0.01em;
    text-shadow: 0 2px 18px rgba(0, 0, 0, 0.75);
  }
  .pm-original {
    margin: 6px 0 0;
    color: #b59770;
    font-size: 14px;
    font-style: italic;
  }
  .pm-tagline {
    margin: 14px 0 0;
    color: #f5c518;
    font-size: 17px;
    font-style: italic;
    line-height: 1.4;
    text-shadow: 0 2px 12px rgba(0, 0, 0, 0.6);
  }
  .pm-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 18px;
    margin: 16px 0 12px;
    color: #ffe6c8;
    font-size: 14px;
    align-items: center;
  }
  .pm-year {
    color: #f5c518;
    font-weight: 600;
    font-size: 16px;
  }
  .pm-meta .chip {
    background: rgba(255, 230, 200, 0.1);
    border: 1px solid rgba(255, 230, 200, 0.25);
    padding: 3px 10px;
    border-radius: 4px;
    font-size: 12px;
    letter-spacing: 0.04em;
    color: #ffe6c8;
  }
  .pm-meta .chip.warn {
    background: rgba(243, 169, 81, 0.18);
    border-color: rgba(243, 169, 81, 0.55);
    color: #ffb86b;
  }
  .pm-genres { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .genre-chip {
    background: rgba(245, 197, 24, 0.15);
    border: 1px solid rgba(245, 197, 24, 0.45);
    color: #f5c518;
    padding: 3px 11px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 500;
  }

  .pm-trailer-card {
    display: block;
    position: relative;
    width: 100%;
    height: 110px;
    margin: 0 0 24px;
    padding: 0;
    border: 1.5px solid rgba(245, 197, 24, 0.5);
    border-radius: 14px;
    background: #1a1108;
    background-size: cover;
    background-position: center 30%;
    cursor: pointer;
    overflow: hidden;
    color: inherit;
    text-align: left;
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.5),
      0 0 24px rgba(245, 197, 24, 0.18);
    transition: transform 0.15s, box-shadow 0.15s, border-color 0.15s;
  }
  .pm-trailer-card:hover, .pm-trailer-card:focus-visible, .pm-trailer-card.focused {
    transform: translateY(-2px);
    border-color: #f5c518;
    box-shadow:
      0 12px 32px rgba(0, 0, 0, 0.65),
      0 0 32px rgba(245, 197, 24, 0.4);
    outline: none;
  }
  .pm-trailer-scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, rgba(13, 6, 4, 0.85) 0%, rgba(13, 6, 4, 0.35) 100%);
  }
  .pm-trailer-content {
    position: relative;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 26px;
    height: 100%;
  }
  .pm-trailer-play {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 54px;
    height: 54px;
    border-radius: 50%;
    background: #f5c518;
    color: #1a0e06;
    font-size: 22px;
    font-weight: 700;
    padding-left: 4px;
    box-shadow: 0 4px 18px rgba(245, 197, 24, 0.5);
  }
  .pm-trailer-label {
    font-size: 18px;
    font-weight: 600;
    color: #fff;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.7);
  }

  .pm-section {
    margin-bottom: 18px;
    padding: 18px 22px;
    background: rgba(28, 16, 8, 0.62);
    border: 1px solid rgba(245, 197, 24, 0.18);
    border-radius: 12px;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
  }
  .pm-h3 {
    margin: 0 0 10px;
    color: #f5c518;
    font-size: 13px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .pm-h3:not(:first-child) { margin-top: 18px; }
  .pm-text, .pm-plot {
    margin: 0;
    color: #ffe6c8;
    font-size: 14.5px;
    line-height: 1.6;
  }
  .pm-people {
    margin: 0;
    color: #d8c4a8;
    font-size: 14px;
    line-height: 1.55;
  }

  .pm-ratings {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 32px;
    padding: 16px 22px;
  }
  .rating {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .rating-label {
    color: #b59770;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .rating-value {
    color: #f5c518;
    font-size: 19px;
    font-weight: 700;
  }
  .rating-sub {
    color: #998878;
    font-size: 11px;
  }
  .rating.rt-fresh .rating-value { color: #6dd16d; }
  .rating.rt-rotten .rating-value { color: #4f9b4f; }

  .pm-grid, .pm-facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: 14px 28px;
  }
  .pm-grid > div, .pm-facts > div {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .lbl {
    color: #b59770;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.07em;
  }
  .val {
    color: #ffe6c8;
    font-size: 14.5px;
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  .val.loss { color: #e57c7c; }

  .pm-panel {
    width: 380px;
    align-self: end;
    padding-bottom: 4px;
  }
  .pm-sub {
    margin: 0 0 4px;
    color: #b59770;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .pm-keyhint {
    margin: 0 0 12px;
    color: #998878;
    font-size: 11px;
    letter-spacing: 0.02em;
  }
  .pm-list {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .pm-btn {
    text-align: left;
    background: rgba(28, 16, 8, 0.85);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1.5px solid rgba(255, 230, 200, 0.18);
    color: #ffe6c8;
    border-radius: 10px;
    padding: 13px 18px;
    font-size: 15px;
    font-weight: 500;
    cursor: pointer;
    transition: border-color 0.12s, background 0.12s, transform 0.12s, color 0.12s;
  }
  .pm-btn:hover { border-color: rgba(245, 197, 24, 0.5); }
  .pm-btn.focused {
    border-color: #f5c518;
    background: rgba(245, 197, 24, 0.18);
    color: #fff;
    transform: translateX(4px);
    box-shadow: 0 0 18px rgba(245, 197, 24, 0.25);
  }
  .pm-btn.primary {
    background: rgba(245, 197, 24, 0.9);
    color: #1a0e06;
    border-color: #f5c518;
    font-weight: 700;
  }
  .pm-btn.primary.focused {
    background: #f5c518;
    color: #0d0d12;
    box-shadow: 0 0 24px rgba(245, 197, 24, 0.55);
  }
  .pm-btn:focus-visible { outline: none; }

  @media (max-width: 980px) {
    .pm-layout {
      grid-template-columns: 1fr;
      padding: 56px 24px 24px;
      gap: 20px;
    }
    .pm-panel { width: 100%; align-self: stretch; }
    .pm-poster { width: 110px; height: 165px; }
    .pm-title { font-size: 28px; }
  }
</style>
