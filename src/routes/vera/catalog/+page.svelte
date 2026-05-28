<script lang="ts">
  import { onMount } from "svelte";
  import {
    importCatalog,
    catalogCount,
    type ImportProgress,
    type CatalogCount,
    type MediaType,
  } from "$lib/vera/import";

  let apiKey = $state("");
  let movie = $state(true);
  let tv = $state(true);
  let pages = $state(5);
  let region = $state("CL");
  let running = $state(false);
  let progress = $state<ImportProgress | null>(null);
  let log = $state<string[]>([]);
  let count = $state<CatalogCount>({ total: 0, movies: 0, tv: 0 });

  const REGIONS = [
    { id: "CL", label: "Chile" },
    { id: "AR", label: "Argentina" },
    { id: "MX", label: "México" },
    { id: "CO", label: "Colombia" },
    { id: "PE", label: "Perú" },
    { id: "ES", label: "España" },
    { id: "US", label: "Estados Unidos" },
    { id: "BR", label: "Brasil" },
  ];

  async function refresh() {
    try {
      count = await catalogCount();
    } catch (e) {
      console.warn("count fail", e);
    }
  }

  onMount(async () => {
    apiKey = localStorage.getItem("tmdb_key") || "";
    region = localStorage.getItem("vera_watch_region") || "CL";
    await refresh();
  });

  $effect(() => {
    localStorage.setItem("vera_watch_region", region);
  });

  function pushLog(line: string) {
    log = [...log.slice(-200), line];
  }

  async function run() {
    if (!apiKey) {
      pushLog("falta api key (configurar en home)");
      return;
    }
    if (!movie && !tv) {
      pushLog("elige al menos movie o tv");
      return;
    }
    running = true;
    progress = null;
    log = [];
    const targets: MediaType[] = [];
    if (movie) targets.push("movie");
    if (tv) targets.push("tv");

    try {
      for (const mt of targets) {
        pushLog(`--- ${mt} : ${pages} pages (region ${region}) ---`);
        const sum = await importCatalog(apiKey, mt, pages, region, (p) => {
          progress = p;
        });
        pushLog(`${mt}: insertados ${sum.inserted}, skip ${sum.skipped}`);
      }
      pushLog("listo.");
      await refresh();
    } catch (e) {
      pushLog(`error: ${e}`);
    } finally {
      running = false;
      progress = null;
    }
  }

  let pct = $derived(() => {
    if (!progress || progress.page_items === 0) return 0;
    const done = (progress.page - 1) * progress.page_items + progress.item_in_page;
    const total = progress.total_pages * progress.page_items;
    return total > 0 ? Math.round((done / total) * 100) : 0;
  });
</script>

<svelte:head><title>Catálogo Vera — Kütral</title></svelte:head>

<div class="page">
  <header>
    <a class="back" href="/vera">← Volver a Vera</a>
    <h1>Catálogo</h1>
    <p class="tagline">Importar títulos desde TMDb a la base local.</p>
  </header>

  <section class="stats">
    <div><strong>{count.total}</strong><span>total</span></div>
    <div><strong>{count.movies}</strong><span>películas</span></div>
    <div><strong>{count.tv}</strong><span>series</span></div>
  </section>

  <section>
    <h2>Importar desde TMDb</h2>
    {#if !apiKey}
      <p class="warn">Sin API key. Configurala en la home antes de importar.</p>
    {/if}
    <div class="form">
      <label><input type="checkbox" bind:checked={movie} disabled={running} /> Películas</label>
      <label><input type="checkbox" bind:checked={tv} disabled={running} /> Series</label>
      <label>
        Páginas (20 títulos c/u):
        <input type="number" min="1" max="50" bind:value={pages} disabled={running} />
      </label>
      <label>
        Región (plataformas):
        <select bind:value={region} disabled={running}>
          {#each REGIONS as r}
            <option value={r.id}>{r.label}</option>
          {/each}
        </select>
      </label>
      <button class="primary" onclick={run} disabled={running || !apiKey}>
        {running ? "Importando…" : "Importar"}
      </button>
    </div>

    {#if progress}
      <div class="progress-box">
        <div class="bar"><div class="fill" style="width: {pct()}%"></div></div>
        <p>
          Página {progress.page}/{progress.total_pages} ·
          ítem {progress.item_in_page}/{progress.page_items} ·
          ✓ {progress.inserted} · ⊘ {progress.skipped}
        </p>
        <p class="current">{progress.current}</p>
      </div>
    {/if}

    {#if log.length}
      <pre class="log">{log.join("\n")}</pre>
    {/if}
  </section>

  <p class="footnote">
    Filtros tone/use/sensitive quedan vacíos en esta etapa.
    Se llenan después manualmente o con fuente externa.
  </p>
</div>

<style>
  .page {
    max-width: 760px;
    margin: 0 auto;
    padding: 2rem 1.5rem 4rem;
    font-family: ui-sans-serif, system-ui, sans-serif;
    color: #1a1a1a;
  }
  header { margin-bottom: 2rem; }
  .back { color: #666; text-decoration: none; font-size: 0.9rem; }
  .back:hover { color: #111; }
  h1 {
    margin: 0.5rem 0 0.25rem;
    font-size: 2rem;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .tagline { margin: 0; color: #666; }
  h2 { font-size: 1.3rem; font-weight: 500; margin: 1.5rem 0 1rem; }
  .stats {
    display: flex;
    gap: 1rem;
    margin-bottom: 2rem;
  }
  .stats div {
    flex: 1;
    background: #fafafa;
    border: 1px solid #eee;
    border-radius: 10px;
    padding: 1rem;
    text-align: center;
  }
  .stats strong { display: block; font-size: 1.8rem; font-weight: 600; }
  .stats span { color: #777; font-size: 0.85rem; }
  .warn { color: #c93; }
  .form {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    align-items: center;
    background: #fafafa;
    border: 1px solid #eee;
    padding: 1rem;
    border-radius: 10px;
  }
  .form label { display: flex; align-items: center; gap: 0.5rem; font-size: 0.95rem; }
  .form input[type="number"] { width: 5rem; padding: 0.3rem 0.5rem; }
  button {
    padding: 0.6rem 1.2rem;
    font-size: 0.95rem;
    border: 1px solid #d0d0d0;
    background: white;
    border-radius: 8px;
    cursor: pointer;
    font-family: inherit;
  }
  button.primary { background: #ff5722; color: white; border-color: #ff5722; }
  button.primary:disabled { background: #ccc; border-color: #ccc; cursor: not-allowed; }
  .progress-box {
    margin-top: 1.5rem;
    background: #fafafa;
    border: 1px solid #eee;
    border-radius: 10px;
    padding: 1rem;
  }
  .bar {
    width: 100%;
    height: 8px;
    background: #e8e8e8;
    border-radius: 4px;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: #ff5722;
    transition: width 0.2s;
  }
  .progress-box p { margin: 0.5rem 0 0; font-size: 0.9rem; color: #555; }
  .current { color: #999 !important; font-style: italic; }
  .log {
    margin-top: 1.5rem;
    background: #1a1a1a;
    color: #d0d0d0;
    padding: 1rem;
    border-radius: 8px;
    font-size: 0.8rem;
    line-height: 1.5;
    max-height: 280px;
    overflow-y: auto;
    white-space: pre-wrap;
  }
  .footnote { margin-top: 2rem; color: #888; font-size: 0.85rem; }
</style>
