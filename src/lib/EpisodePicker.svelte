<script lang="ts">
  // Selector de temporada/episodio para la ruta debrid (series + anime).
  // 2 paneles: temporadas (izq) | episodios (der, carga perezosa vía
  // tmdb_season). Teclado propio: ←→ cambia panel, ↑↓ navega, Enter elige,
  // Esc vuelve. Al elegir episodio → onPick(season, episode).
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { WEB_PLAYER_ENABLED } from "$lib/features";
  import { estadoEpisodio } from "$lib/historial.svelte";

  type Season = {
    season_number: number;
    episode_count: number;
    name: string;
    air_date: string | null;
    poster_path: string | null;
  };
  type Episode = {
    episode_number: number;
    name: string;
    overview: string;
    still_path: string | null;
    air_date: string | null;
    runtime: number | null;
  };

  let {
    seriesId,
    title,
    backdrop = null,
    stillBase,
    seasons,
    apiKey,
    animeId = null,
    clave = "",
    onPick,
    onWeb,
    onClose,
  }: {
    seriesId: number;
    title: string;
    backdrop?: string | null;
    stillBase: string; // prefijo para still_path (ej. https://image.tmdb.org/t/p/w300)
    seasons: Season[];
    apiKey: string;
    // ID AniList: si viene, los episodios salen de ani.zip (no TMDb) y
    // still_path llega como URL completa.
    animeId?: number | null;
    /** Clave del historial (imdb_id o `anilist:<id>`). Vacía = sin marcas. */
    clave?: string;
    onPick: (season: number, episode: number, ep?: { name: string; still_path: string | null }) => void;
    onWeb: () => void;
    onClose: () => void;
  } = $props();

  let pane = $state<"seasons" | "episodes">("seasons");
  let seasonIdx = $state(0);
  let epIdx = $state(0);
  let episodes = $state<Episode[]>([]);
  let loadingEps = $state(false);
  let epError = $state("");
  const cache = new Map<number, Episode[]>();

  async function loadEpisodes(seasonNumber: number) {
    if (cache.has(seasonNumber)) {
      episodes = cache.get(seasonNumber)!;
      epError = "";
      return;
    }
    loadingEps = true;
    epError = "";
    episodes = [];
    try {
      const eps = animeId
        ? await invoke<Episode[]>("anizip_episodes", { anilistId: animeId })
        : await invoke<Episode[]>("tmdb_season", {
            id: seriesId,
            seasonNumber,
            apiKey,
          });
      cache.set(seasonNumber, eps);
      episodes = eps;
    } catch (e) {
      epError = String(e);
      episodes = [];
    } finally {
      loadingEps = false;
    }
  }

  function setSeason(i: number) {
    if (i < 0) i = 0;
    if (i > seasons.length - 1) i = seasons.length - 1;
    seasonIdx = i;
    epIdx = 0;
    const s = seasons[seasonIdx];
    if (s) void loadEpisodes(s.season_number);
    scrollFocused();
  }

  function gotoEpisodes() {
    if (!episodes.length) return;
    pane = "episodes";
    epIdx = 0;
    scrollFocused();
  }

  function moveEp(d: number) {
    let i = epIdx + d;
    if (i < 0) i = 0;
    if (i > episodes.length - 1) i = episodes.length - 1;
    epIdx = i;
    scrollFocused();
  }

  function chooseEpisode() {
    const s = seasons[seasonIdx];
    const e = episodes[epIdx];
    if (s && e) onPick(s.season_number, e.episode_number, { name: e.name, still_path: e.still_path });
  }

  // Marca de visto del capítulo: ✓ si terminó, % si quedó a medias.
  function marca(epNum: number): { visto: boolean; pct: number } | null {
    const s = seasons[seasonIdx];
    if (!clave || !s) return null;
    const fila = estadoEpisodio(clave, s.season_number, epNum);
    if (!fila) return null;
    if (fila.completed === 1) return { visto: true, pct: 100 };
    const real =
      fila.progress_real ??
      (fila.runtime_seconds && fila.runtime_seconds > 0
        ? fila.watched_seconds / fila.runtime_seconds
        : 0);
    const pct = Math.max(0, Math.min(100, Math.round(real * 100)));
    return pct > 0 ? { visto: false, pct } : null;
  }

  function scrollFocused() {
    queueMicrotask(() => {
      document
        .querySelector<HTMLElement>(".ep-focused")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function onKey(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        if (pane === "seasons") setSeason(seasonIdx + 1);
        else moveEp(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        if (pane === "seasons") setSeason(seasonIdx - 1);
        else moveEp(-1);
        break;
      case "ArrowRight":
        e.preventDefault();
        if (pane === "seasons") gotoEpisodes();
        break;
      case "ArrowLeft":
        e.preventDefault();
        if (pane === "episodes") pane = "seasons";
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (pane === "seasons") gotoEpisodes();
        else chooseEpisode();
        break;
      case "Escape":
      case "Backspace":
        e.preventDefault();
        if (pane === "episodes") pane = "seasons";
        else onClose();
        break;
    }
  }

  // Carga inicial de la primera temporada.
  onMount(() => {
    if (seasons[0]) void loadEpisodes(seasons[0].season_number);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="ep-root">
  {#if backdrop}
    <div class="ep-bg" style:background-image="url({backdrop})"></div>
  {/if}
  <div class="ep-scrim"></div>

  <div class="ep-content">
    <header class="ep-head">
      <button class="ep-back" onclick={onClose}>← Volver</button>
      <h2>{title}</h2>
      {#if WEB_PLAYER_ENABLED}
        <button class="ep-web" onclick={onWeb}>🌐 Ver en web</button>
      {/if}
    </header>

    <div class="ep-cols">
      <!-- Temporadas -->
      <div class="ep-seasons" class:active={pane === "seasons"}>
        {#each seasons as s, i (s.season_number)}
          <button
            class="ep-season"
            class:focused={seasonIdx === i}
            class:ep-focused={pane === "seasons" && seasonIdx === i}
            onclick={() => { setSeason(i); gotoEpisodes(); }}
            onmouseenter={() => { if (seasonIdx !== i) setSeason(i); }}
          >
            <span class="ep-season-name">{s.name}</span>
            <span class="ep-season-count">{s.episode_count} ep</span>
          </button>
        {/each}
      </div>

      <!-- Episodios -->
      <div class="ep-episodes" class:active={pane === "episodes"}>
        {#if loadingEps}
          <div class="ep-center"><span class="ep-spinner"></span><p>Cargando episodios…</p></div>
        {:else if epError}
          <p class="ep-error">{epError}</p>
        {:else}
          {#each episodes as ep, i (ep.episode_number)}
            {@const m = marca(ep.episode_number)}
            <button
              class="ep-item"
              class:focused={pane === "episodes" && epIdx === i}
              class:ep-focused={pane === "episodes" && epIdx === i}
              class:ep-visto={m?.visto}
              onclick={() => { epIdx = i; chooseEpisode(); }}
              onmouseenter={() => (epIdx = i)}
            >
              <div class="ep-still-wrap">
                {#if ep.still_path}
                  <img class="ep-still" src={ep.still_path.startsWith("http") ? ep.still_path : `${stillBase}${ep.still_path}`} alt="" loading="lazy" />
                {:else}
                  <div class="ep-still ep-still-empty">{ep.episode_number}</div>
                {/if}
                {#if m?.visto}
                  <span class="ep-tick" title="Ya lo viste">✓</span>
                {:else if m}
                  <span class="ep-barra"><span class="ep-barra-fill" style:width="{m.pct}%"></span></span>
                {/if}
              </div>
              <div class="ep-text">
                <span class="ep-num">E{ep.episode_number} · {ep.name}</span>
                {#if ep.overview}<span class="ep-ov">{ep.overview}</span>{/if}
              </div>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .ep-root {
    position: fixed;
    inset: 0;
    z-index: 50;
    overflow: hidden;
    background: #0d0d12;
  }
  .ep-bg {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(10px) saturate(1.1);
    transform: scale(1.06);
  }
  .ep-scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(0deg, rgba(8, 8, 12, 0.95) 0%, rgba(8, 8, 12, 0.75) 60%, rgba(8, 8, 12, 0.5) 100%);
  }
  .ep-content {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 22px 40px 32px;
  }
  .ep-head {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 18px;
  }
  .ep-head h2 {
    margin: 0;
    flex: 1;
    font-size: 22px;
    font-weight: 600;
    color: #fff;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ep-back,
  .ep-web {
    background: rgba(20, 20, 28, 0.6);
    border: 1px solid rgba(255, 255, 255, 0.14);
    color: #d8d8e0;
    border-radius: 8px;
    padding: 8px 14px;
    cursor: pointer;
    font-size: 13px;
  }

  .ep-cols {
    flex: 1;
    display: grid;
    grid-template-columns: 240px 1fr;
    gap: 18px;
    min-height: 0;
  }
  .ep-seasons,
  .ep-episodes {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 6px;
    opacity: 0.85;
  }
  .ep-seasons.active,
  .ep-episodes.active {
    opacity: 1;
  }

  .ep-season {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    background: rgba(20, 20, 28, 0.6);
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 10px;
    padding: 11px 14px;
    cursor: pointer;
    color: #e6e6ec;
    text-align: left;
  }
  .ep-season.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
  }
  .ep-season-name {
    font-size: 14px;
    font-weight: 600;
  }
  .ep-season-count {
    font-size: 11.5px;
    color: #9a9aa4;
  }

  .ep-item {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    text-align: left;
    background: rgba(20, 20, 28, 0.55);
    border: 2px solid transparent;
    border-radius: 10px;
    padding: 8px 12px;
    cursor: pointer;
    color: inherit;
  }
  .ep-item.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.14);
  }
  .ep-still-wrap {
    position: relative;
    flex: none;
    width: 112px;
    height: 63px;
  }
  /* Ya visto: la miniatura se apaga para que el ojo salte a lo pendiente. */
  .ep-item.ep-visto .ep-still {
    opacity: 0.45;
  }
  .ep-tick {
    position: absolute;
    right: 4px;
    bottom: 4px;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: #2f9e44;
    color: #fff;
    font-size: 12px;
    font-weight: 700;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.6);
  }
  .ep-barra {
    position: absolute;
    left: 4px;
    right: 4px;
    bottom: 4px;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.25);
    overflow: hidden;
  }
  .ep-barra-fill {
    display: block;
    height: 100%;
    background: #f3a951;
  }
  .ep-still {
    flex: none;
    width: 112px;
    height: 63px;
    object-fit: cover;
    border-radius: 6px;
    background: #1a1a22;
  }
  .ep-still-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6e6e78;
    font-weight: 700;
    font-size: 18px;
  }
  .ep-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .ep-num {
    font-size: 14px;
    font-weight: 600;
    color: #fff;
  }
  .ep-ov {
    font-size: 12px;
    color: #a0a0aa;
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .ep-center {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    color: #c8c8d0;
  }
  .ep-spinner {
    width: 30px;
    height: 30px;
    border: 3px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: ep-spin 1s linear infinite;
  }
  @keyframes ep-spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ep-error {
    color: #ff7373;
    font-size: 14px;
  }
</style>
