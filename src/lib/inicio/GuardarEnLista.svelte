<script lang="ts">
  // Botones de la ficha: "Quiero ver" (de un toque) y "Listas" (elige en
  // cuáles está el título, o crea una nueva).
  import {
    QUIERO_VER,
    listas,
    estaEnLista,
    alternarEnLista,
    agregarALista,
    crearLista,
    listasCon,
  } from "$lib/listas.svelte";
  import type { MediaMeta } from "$lib/historial.svelte";
  import type { Detail } from "$lib/tipos";

  let { titulo, clave }: { titulo: Detail; clave: string } = $props();

  let abierto = $state(false);
  let nueva = $state("");

  // Otro título en la ficha: el panel se cierra.
  $effect(() => {
    void titulo.id;
    abierto = false;
    nueva = "";
  });

  const meta = $derived<MediaMeta>({
    clave,
    tmdbId: titulo.id,
    mediaType: titulo.is_anime ? "anime" : titulo.media_type,
    title: titulo.title,
    posterPath: titulo.poster_path ?? null,
  });

  const quiero = $derived(estaEnLista(QUIERO_VER, clave));
  const enCuantas = $derived(listasCon(clave).length);
  const propias = $derived(listas.todas.filter((l) => l.id !== QUIERO_VER));

  async function crear() {
    const id = await crearLista(nueva);
    if (!id) return;
    nueva = "";
    await agregarALista(id, meta);
  }
</script>

<button data-nav class="hist-btn" class:on={quiero} onclick={() => void alternarEnLista(QUIERO_VER, meta)}>
  {quiero ? "✓ En Quiero ver" : "＋ Quiero ver"}
</button>
<button data-nav class="hist-btn" class:on={enCuantas > 0} class:abierto onclick={() => (abierto = !abierto)}>
  ≡ {enCuantas === 0 ? "Listas" : enCuantas === 1 ? "En 1 lista" : `En ${enCuantas} listas`}
</button>
{#if abierto}
  <div class="panel">
    {#if propias.length}
      <div class="chips">
        {#each propias as l (l.id)}
          {@const esta = estaEnLista(l.id, clave)}
          <button data-nav class="chip" class:on={esta} onclick={() => void alternarEnLista(l.id, meta)}>
            {esta ? "✓ " : ""}{l.nombre}
          </button>
        {/each}
      </div>
    {:else}
      <p class="vacio">Todavía no tienes listas. Crea la primera:</p>
    {/if}
    <div class="nueva">
      <input
        data-nav
        bind:value={nueva}
        maxlength="60"
        placeholder="Nueva lista (ej.: Maratón Nolan)"
        onkeydown={(e) => e.key === "Enter" && void crear()}
      />
      <button data-nav class="crear" onclick={crear} disabled={!nueva.trim()}>Crear y agregar</button>
    </div>
  </div>
{/if}

<style>
  /* Mismo botón que el resto de la fila (FichaTitulo). */
  .hist-btn {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #d8d8e0;
    border-radius: 8px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .hist-btn:hover,
  .hist-btn:focus-visible,
  .hist-btn.abierto {
    border-color: #f3a951;
    color: #fff;
    outline: none;
  }
  .hist-btn.on {
    background: rgba(47, 158, 68, 0.18);
    border-color: #2f9e44;
    color: #7ee094;
  }
  .panel {
    flex-basis: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 10px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #d8d8e0;
    border-radius: 999px;
    padding: 5px 12px;
    font-size: 12px;
    cursor: pointer;
  }
  .chip.on {
    background: rgba(47, 158, 68, 0.18);
    border-color: #2f9e44;
    color: #7ee094;
  }
  .chip:focus-visible,
  input:focus-visible,
  .crear:focus-visible {
    outline: 2px solid #f3a951;
    outline-offset: 1px;
  }
  .vacio {
    margin: 0;
    font-size: 12px;
    color: #9a9aa4;
  }
  .nueva {
    display: flex;
    gap: 6px;
  }
  input {
    flex: 1;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    color: #fff;
    padding: 7px 10px;
    border-radius: 8px;
    font-size: 12.5px;
  }
  .crear {
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 7px 12px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .crear:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
