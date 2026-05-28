<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
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

  type Step =
    | "welcome"
    | "mode_io"
    | "depth"
    | "languages"
    | "dub_pref"
    | "platforms"
    | "exclusions"
    | "setup_done"
    | "intent"
    | "intent_chosen";

  let step = $state<Step>("welcome");
  let db: Awaited<ReturnType<typeof Database.load>> | null = null;

  // Setup state
  let modeIo = $state<ModeIO>("touch");
  let depth = $state<DepthProfile>("interested");
  let languages = $state<string[]>(["es"]);
  let dubPref = $state<DubPref>("subs_always");
  let platforms = $state<string[]>([]);
  let excludedGenres = $state<string[]>([]);
  let excludedThemes = $state<string[]>([]);
  let personality = $state<Personality>("warm");

  // Loaded option lists
  let intentOptions = $state<VeraOption[]>([]);
  let genreOptions = $state<VeraOption[]>([]);
  let themeOptions = $state<VeraOption[]>([]);
  let platformOptions = $state<VeraOption[]>([]);

  let chosenIntent = $state<IntentId | null>(null);
  let existingSetup = $state<VeraSetup | null>(null);
  let loading = $state(true);

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
    "mode_io",
    "depth",
    "languages",
    "dub_pref",
    "platforms",
    "exclusions",
    "setup_done",
  ];
  const setupSteps = stepOrder.slice(1, -1);
  let setupStepIndex = $derived(setupSteps.indexOf(step));
</script>

<svelte:head>
  <title>Vera — Kütral</title>
</svelte:head>

<div class="vera">
  <header>
    <a class="back" href="/">← Volver</a>
    <h1>Vera</h1>
    <p class="tagline">Te ayudo a elegir qué ver cuando no sabes qué quieres ver.</p>
  </header>

  {#if loading}
    <p class="loading">Cargando…</p>
  {:else}
    {#if setupStepIndex >= 0}
      <div class="progress">
        Setup {setupStepIndex + 1} / {setupSteps.length}
      </div>
    {/if}

    {#if step === "welcome"}
      <section>
        <h2>Hola. Soy Vera.</h2>
        <p>Siete preguntas rápidas y empezamos.</p>
        <button class="primary" onclick={() => (step = "mode_io")}>Comenzar</button>
      </section>
    {:else if step === "mode_io"}
      <section>
        <h2>¿Cómo prefieres usarme?</h2>
        <div class="opts">
          <label><input type="radio" bind:group={modeIo} value="touch" /> Tocando y leyendo</label>
          <label><input type="radio" bind:group={modeIo} value="voice" /> Hablando y escuchando (comandos)</label>
          <label><input type="radio" bind:group={modeIo} value="both" /> Las dos, según el momento</label>
        </div>
        <div class="nav">
          <button onclick={() => (step = "welcome")}>Atrás</button>
          <button class="primary" onclick={() => (step = "depth")}>Siguiente</button>
        </div>
      </section>
    {:else if step === "depth"}
      <section>
        <h2>¿Qué lugar ocupan las películas y series en tu vida?</h2>
        <div class="opts">
          <label><input type="radio" bind:group={depth} value="casual" /> Es mi entretenimiento, nada más</label>
          <label><input type="radio" bind:group={depth} value="interested" /> Me interesa, pero sin pretensiones</label>
          <label><input type="radio" bind:group={depth} value="demanding" /> Soy exigente, me importa la calidad</label>
          <label><input type="radio" bind:group={depth} value="cinephile" /> Soy cinéfilo, dame opciones raras</label>
        </div>
        <div class="nav">
          <button onclick={() => (step = "mode_io")}>Atrás</button>
          <button class="primary" onclick={() => (step = "languages")}>Siguiente</button>
        </div>
      </section>
    {:else if step === "languages"}
      <section>
        <h2>¿Qué idiomas entiendes de oído?</h2>
        <p class="hint">Sin subtítulos. Marca todos los que apliquen.</p>
        <div class="opts grid">
          {#each LANG_OPTIONS as lang}
            <label class:checked={languages.includes(lang.id)}>
              <input
                type="checkbox"
                checked={languages.includes(lang.id)}
                onchange={() => (languages = toggle(languages, lang.id))}
              />
              {lang.label}
            </label>
          {/each}
        </div>
        <div class="nav">
          <button onclick={() => (step = "depth")}>Atrás</button>
          <button class="primary" onclick={() => (step = "dub_pref")}>Siguiente</button>
        </div>
      </section>
    {:else if step === "dub_pref"}
      <section>
        <h2>Cuando algo está en un idioma que no entiendes…</h2>
        <div class="opts">
          <label><input type="radio" bind:group={dubPref} value="subs_always" /> Subtítulos siempre</label>
          <label><input type="radio" bind:group={dubPref} value="dub_always" /> Doblaje siempre</label>
          <label><input type="radio" bind:group={dubPref} value="depends" /> Depende</label>
          <label><input type="radio" bind:group={dubPref} value="indifferent" /> Me da igual</label>
        </div>
        <div class="nav">
          <button onclick={() => (step = "languages")}>Atrás</button>
          <button class="primary" onclick={() => (step = "platforms")}>Siguiente</button>
        </div>
      </section>
    {:else if step === "platforms"}
      <section>
        <h2>¿A qué plataformas tienes acceso?</h2>
        <div class="opts grid">
          {#each platformOptions as plat}
            <label class:checked={platforms.includes(plat.id)}>
              <input
                type="checkbox"
                checked={platforms.includes(plat.id)}
                onchange={() => (platforms = toggle(platforms, plat.id))}
              />
              {plat.label}
            </label>
          {/each}
        </div>
        <div class="nav">
          <button onclick={() => (step = "dub_pref")}>Atrás</button>
          <button class="primary" onclick={() => (step = "exclusions")}>Siguiente</button>
        </div>
      </section>
    {:else if step === "exclusions"}
      <section>
        <h2>¿Hay algo que nunca quieras ver, bajo ninguna circunstancia?</h2>
        <p class="hint">Saltable. Lo marcado actúa como filtro duro permanente.</p>
        <h3>Géneros</h3>
        <div class="opts grid compact">
          {#each genreOptions as g}
            <label class:checked={excludedGenres.includes(g.id)}>
              <input
                type="checkbox"
                checked={excludedGenres.includes(g.id)}
                onchange={() => (excludedGenres = toggle(excludedGenres, g.id))}
              />
              {g.label}
            </label>
          {/each}
        </div>
        <h3>Temas sensibles</h3>
        <div class="opts grid compact">
          {#each themeOptions as t}
            <label class:checked={excludedThemes.includes(t.id)}>
              <input
                type="checkbox"
                checked={excludedThemes.includes(t.id)}
                onchange={() => (excludedThemes = toggle(excludedThemes, t.id))}
              />
              {t.label}
            </label>
          {/each}
        </div>
        <div class="nav">
          <button onclick={() => (step = "platforms")}>Atrás</button>
          <button class="primary" onclick={saveSetup}>Guardar y terminar</button>
        </div>
      </section>
    {:else if step === "setup_done"}
      <section class="centered">
        <h2>Listo. Setup guardado.</h2>
        <p>Vera ya conoce tus preferencias. Empecemos.</p>
        <button class="primary" onclick={() => (step = "intent")}>Pedir recomendación</button>
      </section>
    {:else if step === "intent"}
      <section>
        <h2>¿Qué buscas hoy?</h2>
        {#if existingSetup}
          <p class="hint">
            Setup guardado el {new Date(existingSetup.completed_at).toLocaleDateString()}.
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
      </section>
    {:else if step === "intent_chosen"}
      <section class="centered">
        <h2>Elegiste: <em>{intentOptions.find((o) => o.id === chosenIntent)?.label}</em></h2>
        <p class="hint">
          Capa 2 (Forma) y motor de coincidencia: próximo iteración.
        </p>
        <button onclick={() => (step = "intent")}>Volver a intenciones</button>
      </section>
    {/if}
  {/if}
</div>

<style>
  .vera {
    max-width: 760px;
    margin: 0 auto;
    padding: 2rem 1.5rem 4rem;
    font-family: ui-sans-serif, system-ui, sans-serif;
    color: #1a1a1a;
  }
  header {
    margin-bottom: 2rem;
  }
  .back {
    color: #666;
    text-decoration: none;
    font-size: 0.9rem;
  }
  .back:hover { color: #111; }
  h1 {
    margin: 0.5rem 0 0.25rem;
    font-size: 2.4rem;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .tagline {
    margin: 0;
    color: #666;
    font-size: 1rem;
  }
  .progress {
    color: #888;
    font-size: 0.85rem;
    margin-bottom: 1.5rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  h2 {
    font-size: 1.6rem;
    font-weight: 500;
    margin: 0 0 1rem;
    line-height: 1.3;
  }
  h3 {
    font-size: 1rem;
    margin: 1.5rem 0 0.75rem;
    font-weight: 600;
    color: #444;
  }
  .hint {
    color: #777;
    font-size: 0.9rem;
    margin: -0.5rem 0 1rem;
  }
  .loading { color: #888; }
  .centered { text-align: center; padding: 3rem 0; }
  .opts {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin: 1rem 0 2rem;
  }
  .opts.grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 0.5rem;
  }
  .opts.compact label {
    font-size: 0.9rem;
    padding: 0.5rem 0.75rem;
  }
  .opts label {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.75rem 1rem;
    border: 1px solid #e0e0e0;
    border-radius: 10px;
    cursor: pointer;
    transition: border-color 0.1s, background 0.1s;
  }
  .opts label:hover { border-color: #b0b0b0; background: #fafafa; }
  .opts label.checked { border-color: #ff5722; background: #fff3ef; }
  .opts input { accent-color: #ff5722; }
  .nav {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    margin-top: 2rem;
  }
  button {
    padding: 0.7rem 1.4rem;
    font-size: 1rem;
    border: 1px solid #d0d0d0;
    background: white;
    border-radius: 10px;
    cursor: pointer;
    font-family: inherit;
  }
  button:hover { border-color: #888; }
  button.primary {
    background: #ff5722;
    color: white;
    border-color: #ff5722;
  }
  button.primary:hover { background: #e64a19; border-color: #e64a19; }
  button.link {
    border: none;
    background: none;
    color: #ff5722;
    padding: 0;
    font-size: inherit;
    text-decoration: underline;
  }
  .intent-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 0.75rem;
    margin-top: 1rem;
  }
  .intent-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    text-align: left;
    padding: 1.25rem 1.25rem;
    border: 1px solid #e0e0e0;
    border-radius: 12px;
    background: white;
    cursor: pointer;
    transition: transform 0.08s, border-color 0.08s;
  }
  .intent-card:hover {
    border-color: #ff5722;
    transform: translateY(-1px);
  }
  .intent-card strong {
    font-size: 1.05rem;
    margin-bottom: 0.35rem;
    font-weight: 600;
  }
  .intent-card span {
    color: #777;
    font-size: 0.88rem;
  }
</style>
