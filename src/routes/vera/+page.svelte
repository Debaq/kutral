<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { fly, fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Database from "@tauri-apps/plugin-sql";
  import type {
    VeraOption,
    VeraSetup,
    ModeIO,
    DepthProfile,
    DubPref,
    Personality,
    IntentId,
  } from "$lib/vera/types";
  import { LANG_OPTIONS } from "$lib/vera/types";

  // Pasos quitados del flujo (estado se preserva con default):
  //   - mode_io: voz/comando aún no implementado.
  //   - depth: pregunta "cinéfilo o casual" sentía a encuesta de vida.
  //   - platforms: Kütral reproduce todo internamente; no tiene sentido.
  //   - exclusions: 23 géneros + 24 temas es demasiado para un wizard.
  //              Se aprenden por rechazo de carátula/sinopsis en sesión,
  //              o se editan en Ajustes (pendiente).
  type Step =
    | "welcome"
    | "languages"
    | "dub_pref"
    | "setup_done"
    | "intent"
    | "intent_chosen";

  let step = $state<Step>("welcome");
  let db: Awaited<ReturnType<typeof Database.load>> | null = null;

  let modeIo = $state<ModeIO>("touch");
  let depth = $state<DepthProfile>("interested");
  let languages = $state<string[]>(["es"]);
  let dubPref = $state<DubPref>("subs_always");
  let platforms = $state<string[]>([]);
  let excludedGenres = $state<string[]>([]);
  let excludedThemes = $state<string[]>([]);
  let personality = $state<Personality>("warm");

  let intentOptions = $state<VeraOption[]>([]);
  let genreOptions = $state<VeraOption[]>([]);
  let themeOptions = $state<VeraOption[]>([]);
  let platformOptions = $state<VeraOption[]>([]);

  let chosenIntent = $state<IntentId | null>(null);
  let existingSetup = $state<VeraSetup | null>(null);
  let loading = $state(true);

  let leftPosters = $state<string[]>([]);
  let rightPosters = $state<string[]>([]);

  const IMG = "https://image.tmdb.org/t/p";

  interface TmdbResp {
    results: { id: number; poster_path: string | null; backdrop_path: string | null }[];
  }

  async function loadPosters() {
    const apiKey = localStorage.getItem("tmdb_key") || "";
    if (!apiKey) return;
    try {
      const [m, t] = await Promise.all([
        invoke<TmdbResp>("tmdb_discover", { mediaType: "movie", page: 1, apiKey }),
        invoke<TmdbResp>("tmdb_discover", { mediaType: "tv", page: 1, apiKey }),
      ]);
      const all = [...m.results, ...t.results]
        .filter((x) => x.poster_path)
        .map((x) => `${IMG}/w342${x.poster_path}`);
      const shuffled = all.sort(() => Math.random() - 0.5);
      const half = Math.ceil(shuffled.length / 2);
      leftPosters = shuffled.slice(0, half);
      rightPosters = shuffled.slice(half);
    } catch (e) {
      console.warn("[vera] posters fail", e);
    }
  }

  onMount(async () => {
    db = await Database.load("sqlite:kutral.db");
    const rows = await db.select<VeraSetup[]>(
      "SELECT mode_io, depth_profile, languages_known, dub_pref, platforms, excluded_genres, excluded_themes, personality, completed_at FROM vera_setup WHERE id = 1"
    );
    if (rows.length) {
      const r = rows[0] as unknown as Record<string, string | number>;
      existingSetup = {
        mode_io: r.mode_io as ModeIO,
        depth_profile: r.depth_profile as DepthProfile,
        languages_known: JSON.parse(r.languages_known as string),
        dub_pref: r.dub_pref as DubPref,
        platforms: JSON.parse(r.platforms as string),
        excluded_genres: JSON.parse(r.excluded_genres as string),
        excluded_themes: JSON.parse(r.excluded_themes as string),
        personality: r.personality as Personality,
        completed_at: r.completed_at as number,
      };
      modeIo = existingSetup.mode_io;
      depth = existingSetup.depth_profile;
      languages = existingSetup.languages_known;
      dubPref = existingSetup.dub_pref;
      platforms = existingSetup.platforms;
      excludedGenres = existingSetup.excluded_genres;
      excludedThemes = existingSetup.excluded_themes;
      personality = existingSetup.personality;
      step = "intent";
    }

    [intentOptions, genreOptions, themeOptions, platformOptions] = await Promise.all([
      invoke<VeraOption[]>("vera_intent_options"),
      invoke<VeraOption[]>("vera_genre_list"),
      invoke<VeraOption[]>("vera_theme_list"),
      invoke<VeraOption[]>("vera_platform_list"),
    ]);

    loading = false;
    loadPosters();
  });

  function toggle(list: string[], id: string): string[] {
    return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
  }

  async function saveSetup() {
    if (!db) return;
    await db.execute(
      `INSERT INTO vera_setup (id, mode_io, depth_profile, languages_known, dub_pref, platforms, excluded_genres, excluded_themes, personality, completed_at)
       VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8, $9)
       ON CONFLICT(id) DO UPDATE SET
         mode_io = $1, depth_profile = $2, languages_known = $3, dub_pref = $4,
         platforms = $5, excluded_genres = $6, excluded_themes = $7,
         personality = $8, completed_at = $9`,
      [
        modeIo,
        depth,
        JSON.stringify(languages),
        dubPref,
        JSON.stringify(platforms),
        JSON.stringify(excludedGenres),
        JSON.stringify(excludedThemes),
        personality,
        Date.now(),
      ]
    );
    step = "setup_done";
  }

  function pickIntent(id: IntentId) {
    chosenIntent = id;
    step = "intent_chosen";
  }

  function resetSetup() {
    step = "welcome";
    existingSetup = null;
  }

  const stepOrder: Step[] = [
    "welcome",
    "languages",
    "dub_pref",
    "setup_done",
  ];
  const setupSteps = stepOrder.slice(1, -1);
  let setupStepIndex = $derived(setupSteps.indexOf(step));
  let progressPct = $derived(
    setupStepIndex >= 0 ? Math.round(((setupStepIndex + 1) / setupSteps.length) * 100) : 0
  );
</script>

<svelte:head>
  <title>Vera and Chill — Kütral</title>
</svelte:head>

<div class="vera-bg">
  <aside class="strip strip-left" aria-hidden="true">
    <div class="track track-up">
      {#each [...leftPosters, ...leftPosters] as src}
        <img src={src} alt="" loading="lazy" />
      {/each}
    </div>
  </aside>

  <main class="vera-stage">
    <nav class="topbar">
      <span class="brandmark">Vera <em>and Chill</em></span>
      <a class="catalog" href="/vera/catalog">Catálogo</a>
    </nav>

    {#if setupStepIndex >= 0}
      <div class="progress-bar" transition:fade={{ duration: 200 }}>
        <div class="fill" style="width: {progressPct}%"></div>
      </div>
    {/if}

    {#if loading}
      <div class="loading">Cargando…</div>
    {:else}
      {#key step}
        <div class="content" in:fly={{ y: 16, duration: 320, easing: cubicOut }} out:fade={{ duration: 120 }}>
          {#if step === "welcome"}
            <h1 class="hero">
              <span class="brand">Vera</span> <em class="and-chill">and Chill</em>
            </h1>
            <p class="lede">Tú pones el sillón. Yo pongo qué ver.</p>
            <p class="meta">Dos cosas rápidas y empezamos.</p>
            <div class="nav-row">
              <a class="btn ghost big" href="/">← Volver</a>
              <button class="primary big" onclick={() => (step = "languages")}>Vamos →</button>
            </div>

          {:else if step === "languages"}
            <h2>¿Qué idiomas entiendes de oído?</h2>
            <p class="hint">Sin subtítulos. Marca todos los que apliquen.</p>
            <div class="chips">
              {#each LANG_OPTIONS as lang}
                <button
                  class="chip"
                  class:on={languages.includes(lang.id)}
                  onclick={() => (languages = toggle(languages, lang.id))}
                >{lang.label}</button>
              {/each}
            </div>
            <div class="nav-row">
              <button class="ghost" onclick={() => (step = "welcome")}>Atrás</button>
              <button class="primary" onclick={() => (step = "dub_pref")}>Siguiente →</button>
            </div>

          {:else if step === "dub_pref"}
            <h2>Cuando está en un idioma que no entiendes…</h2>
            <div class="cards">
              <button class="opt-card" class:on={dubPref === "subs_always"} onclick={() => (dubPref = "subs_always")}>
                <strong>Subtítulos siempre</strong>
              </button>
              <button class="opt-card" class:on={dubPref === "dub_always"} onclick={() => (dubPref = "dub_always")}>
                <strong>Doblaje siempre</strong>
              </button>
              <button class="opt-card" class:on={dubPref === "depends"} onclick={() => (dubPref = "depends")}>
                <strong>Depende</strong>
              </button>
              <button class="opt-card" class:on={dubPref === "indifferent"} onclick={() => (dubPref = "indifferent")}>
                <strong>Me da igual</strong>
              </button>
            </div>
            <div class="nav-row">
              <button class="ghost" onclick={() => (step = "languages")}>Atrás</button>
              <button class="primary" onclick={saveSetup}>Listo →</button>
            </div>

          {:else if step === "setup_done"}
            <h1 class="hero">Listo.</h1>
            <p class="lede">Vera ya te conoce. ¿Empezamos?</p>
            <div class="nav-row">
              <button class="primary big" onclick={() => (step = "intent")}>Pedir recomendación →</button>
            </div>

          {:else if step === "intent"}
            <h2>¿Qué buscás hoy?</h2>
            {#if existingSetup}
              <p class="hint">
                Setup del {new Date(existingSetup.completed_at).toLocaleDateString()}.
                <button class="link" onclick={resetSetup}>Reconfigurar</button>
              </p>
            {/if}
            <div class="intent-grid">
              {#each intentOptions as it}
                <button class="intent-card" onclick={() => pickIntent(it.id as IntentId)}>
                  <strong>{it.label}</strong>
                  {#if it.description}<span>{it.description}</span>{/if}
                </button>
              {/each}
            </div>

          {:else if step === "intent_chosen"}
            <h2>Elegiste</h2>
            <p class="lede"><em>{intentOptions.find((o) => o.id === chosenIntent)?.label}</em></p>
            <p class="hint">Capa 2 (Forma) y motor de coincidencia: próxima iteración.</p>
            <div class="nav-row">
              <button class="ghost" onclick={() => (step = "intent")}>← Otra intención</button>
            </div>
          {/if}
        </div>
      {/key}
    {/if}
  </main>

  <aside class="strip strip-right" aria-hidden="true">
    <div class="track track-down">
      {#each [...rightPosters, ...rightPosters] as src}
        <img src={src} alt="" loading="lazy" />
      {/each}
    </div>
  </aside>
</div>

<style>
  :global(html), :global(body) {
    margin: 0; padding: 0; height: 100%;
    background: #0a0a12;
  }
  .vera-bg {
    min-height: 100vh;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(420px, 720px) minmax(0, 1fr);
    background:
      radial-gradient(circle at 50% 0%, rgba(255,87,34,0.10), transparent 60%),
      linear-gradient(180deg, #0a0a12 0%, #15080a 100%);
    color: #e8e8ea;
    font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
    overflow: hidden;
    position: relative;
  }
  .strip {
    overflow: hidden;
    position: relative;
    height: 100vh;
    mask-image: linear-gradient(180deg, transparent 0%, black 12%, black 88%, transparent 100%);
  }
  .strip::after {
    content: ""; position: absolute; inset: 0;
    background: linear-gradient(90deg, rgba(10,10,18,0.7), transparent 30%, transparent 70%, rgba(10,10,18,0.7));
    pointer-events: none;
  }
  .strip-left::after  { background: linear-gradient(90deg, transparent 0%, transparent 60%, rgba(10,10,18,0.85) 100%); }
  .strip-right::after { background: linear-gradient(90deg, rgba(10,10,18,0.85) 0%, transparent 40%, transparent 100%); }
  .track {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px;
    will-change: transform;
  }
  .track img {
    width: 100%;
    border-radius: 6px;
    display: block;
    opacity: 0.55;
    filter: saturate(0.85) brightness(0.8);
    transition: opacity 0.4s;
  }
  .track-up   { animation: drift-up   90s linear infinite; }
  .track-down { animation: drift-down 110s linear infinite; }
  @keyframes drift-up   { from { transform: translateY(0); } to { transform: translateY(-50%); } }
  @keyframes drift-down { from { transform: translateY(-50%); } to { transform: translateY(0); } }
  @media (max-width: 1100px) {
    .vera-bg { grid-template-columns: 1fr; }
    .strip { display: none; }
  }

  .vera-stage {
    display: flex;
    flex-direction: column;
    padding: 24px 32px 64px;
    height: 100vh;
    overflow-y: auto;
    position: relative;
    z-index: 1;
  }
  .topbar {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 32px;
  }
  .topbar a {
    color: #888; text-decoration: none; font-size: 13px;
    padding: 6px 10px; border-radius: 6px;
    transition: color 0.15s, background 0.15s;
  }
  .topbar a:hover { color: #fff; background: rgba(255,255,255,0.05); }

  .progress-bar {
    height: 3px; background: rgba(255,255,255,0.08); border-radius: 2px;
    overflow: hidden; margin-bottom: 48px;
  }
  .progress-bar .fill {
    height: 100%; background: linear-gradient(90deg, #ff5722, #ff8a65);
    transition: width 0.35s cubic-bezier(.2,.7,.2,1);
  }

  .loading { color: #666; text-align: center; padding: 4rem 0; }

  .content {
    flex: 1;
    display: flex; flex-direction: column;
    justify-content: center;
    min-height: 60vh;
  }
  .hero {
    font-size: clamp(2.5rem, 6vw, 4rem);
    font-weight: 300;
    line-height: 1.05;
    margin: 0 0 1rem;
    letter-spacing: -0.02em;
  }
  .brand {
    background: linear-gradient(90deg, #ff5722, #ffab40);
    -webkit-background-clip: text; background-clip: text;
    color: transparent;
    font-weight: 600;
  }
  .lede {
    font-size: 1.4rem;
    color: #c8c8cc;
    margin: 0 0 0.5rem;
    font-weight: 300;
  }
  .meta { color: #888; font-size: 1rem; margin: 0 0 2.5rem; }
  h2 {
    font-size: clamp(1.6rem, 3vw, 2.2rem);
    font-weight: 400;
    margin: 0 0 1.5rem;
    line-height: 1.2;
    letter-spacing: -0.01em;
  }
  .sub-h {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #888;
    margin: 1.75rem 0 0.75rem;
    font-weight: 600;
  }
  .hint {
    color: #888; font-size: 0.95rem; margin: -0.5rem 0 1.5rem;
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
    margin-bottom: 2rem;
  }
  .opt-card {
    background: rgba(255,255,255,0.04);
    border: 1px solid rgba(255,255,255,0.08);
    border-radius: 14px;
    padding: 1.25rem 1.25rem;
    color: #eee;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    display: flex; flex-direction: column; gap: 4px;
    transition: transform 0.15s, border-color 0.15s, background 0.15s;
  }
  .opt-card:hover {
    background: rgba(255,255,255,0.07);
    border-color: rgba(255,255,255,0.15);
    transform: translateY(-2px);
  }
  .opt-card.on {
    background: rgba(255,87,34,0.12);
    border-color: #ff5722;
    box-shadow: 0 0 0 1px #ff5722, 0 8px 24px rgba(255,87,34,0.2);
  }
  .opt-card strong { font-size: 1.05rem; font-weight: 600; }
  .opt-card span { font-size: 0.85rem; color: #999; }

  .chips {
    display: flex; flex-wrap: wrap; gap: 8px;
    margin-bottom: 2rem;
  }
  .chips.tight { gap: 6px; }
  .chip {
    background: rgba(255,255,255,0.04);
    border: 1px solid rgba(255,255,255,0.1);
    color: #ccc;
    padding: 0.55rem 1rem;
    border-radius: 999px;
    cursor: pointer;
    font-size: 0.92rem;
    font-family: inherit;
    transition: all 0.15s;
  }
  .chip:hover {
    background: rgba(255,255,255,0.08);
    color: #fff;
  }
  .chip.on {
    background: #ff5722;
    border-color: #ff5722;
    color: #fff;
    font-weight: 500;
  }

  .intent-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
  }
  .intent-card {
    background: rgba(255,255,255,0.04);
    border: 1px solid rgba(255,255,255,0.08);
    border-radius: 14px;
    padding: 1.4rem;
    color: #eee;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    display: flex; flex-direction: column; gap: 6px;
    transition: transform 0.15s, border-color 0.15s, background 0.15s;
  }
  .intent-card:hover {
    background: rgba(255,87,34,0.08);
    border-color: rgba(255,87,34,0.4);
    transform: translateY(-3px);
  }
  .intent-card strong { font-size: 1.1rem; font-weight: 600; }
  .intent-card span { font-size: 0.88rem; color: #999; }

  .nav-row {
    display: flex; justify-content: space-between; gap: 1rem;
    margin-top: 2.5rem;
  }
  .nav-row > :only-child { margin-left: auto; }

  button {
    padding: 0.75rem 1.5rem;
    font-size: 0.95rem;
    border-radius: 10px;
    cursor: pointer;
    font-family: inherit;
    font-weight: 500;
    transition: all 0.15s;
    border: 1px solid transparent;
  }
  button.primary {
    background: #ff5722;
    color: #fff;
    border-color: #ff5722;
  }
  button.primary:hover { background: #ff7043; border-color: #ff7043; }
  button.primary.big {
    padding: 1rem 2rem; font-size: 1.05rem; border-radius: 12px;
  }
  button.ghost {
    background: transparent; color: #aaa;
    border-color: rgba(255,255,255,0.15);
  }
  button.ghost:hover { color: #fff; border-color: rgba(255,255,255,0.3); }
  button.link {
    border: none; background: none; color: #ff8a65;
    padding: 0; font-size: inherit; text-decoration: underline;
    margin-left: 0.4rem;
  }
</style>
