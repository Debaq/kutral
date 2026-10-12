<script lang="ts">
  // Botón "Recomendar" de la ficha: elige amigos, nota opcional y envía. Le
  // llega a cada uno en Lo que viste → De amigos. Solo aparece con cuenta y
  // al menos un amigo aceptado.
  import { cuenta, social, recomendar } from "$lib/cuenta.svelte";
  import type { Detail } from "$lib/tipos";

  let { titulo }: { titulo: Detail } = $props();

  let abierto = $state(false);
  let elegidos = $state<string[]>([]);
  let nota = $state("");
  let enviando = $state(false);
  let error = $state("");
  let enviado = $state("");

  // Otro título en la ficha: el panel vuelve a empezar.
  $effect(() => {
    void titulo.id;
    abierto = false;
    elegidos = [];
    nota = "";
    error = "";
    enviado = "";
  });

  function alternar(nombre: string) {
    elegidos = elegidos.includes(nombre) ? elegidos.filter((n) => n !== nombre) : [...elegidos, nombre];
  }

  async function enviar() {
    if (!elegidos.length) return;
    error = "";
    enviando = true;
    try {
      await recomendar(
        elegidos,
        {
          tmdb_id: titulo.id,
          media_type: titulo.is_anime ? "anime" : titulo.media_type,
          title: titulo.title,
          poster_path: titulo.poster_path ?? null,
          year: Number(titulo.year) || null,
        },
        nota,
      );
      enviado = elegidos.length === 1 ? `Se la recomendaste a ${elegidos[0]}.` : `Se la recomendaste a ${elegidos.length} amigos.`;
      abierto = false;
      elegidos = [];
      nota = "";
    } catch (e) {
      error = String(e);
    } finally {
      enviando = false;
    }
  }
</script>

{#if cuenta.usuario && social.amigos.length}
  <button data-nav class="hist-btn" class:on={abierto} onclick={() => ((abierto = !abierto), (enviado = ""))}>
    ✉ Recomendar
  </button>
  {#if enviado}<span class="hist-info">{enviado}</span>{/if}
  {#if abierto}
    <div class="panel">
      <div class="amigos">
        {#each social.amigos as a (a.nombre)}
          <button data-nav class="chip" class:on={elegidos.includes(a.nombre)} onclick={() => alternar(a.nombre)}>
            {elegidos.includes(a.nombre) ? "✓ " : ""}{a.nombre}
          </button>
        {/each}
      </div>
      <input
        data-nav
        bind:value={nota}
        maxlength="200"
        placeholder="Nota (opcional): por qué la recomiendas"
        onkeydown={(e) => e.key === "Enter" && void enviar()}
      />
      <button data-nav class="enviar" onclick={enviar} disabled={enviando || !elegidos.length}>
        {enviando ? "Enviando…" : "Enviar"}
      </button>
      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {/if}
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
  .hist-btn.on {
    border-color: #f3a951;
    color: #fff;
    outline: none;
  }
  .hist-info {
    font-size: 12px;
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
  .amigos {
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
    background: rgba(243, 169, 81, 0.18);
    border-color: #f3a951;
    color: #fff;
  }
  .chip:focus-visible,
  input:focus-visible,
  .enviar:focus-visible {
    outline: 2px solid #f3a951;
    outline-offset: 1px;
  }
  input {
    background: #0d0d12;
    border: 1px solid #2a2a36;
    color: #fff;
    padding: 7px 10px;
    border-radius: 8px;
    font-size: 12.5px;
  }
  .enviar {
    align-self: flex-start;
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 7px 16px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .enviar:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .err {
    margin: 0;
    font-size: 12px;
    color: #ff7373;
  }
</style>
