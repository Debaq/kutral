<script lang="ts">
  // Pastilla "en la TV": lo que se mandó a la TV sigue sonando aunque salgas
  // del selector de fuentes y navegues el catálogo. Desde acá se pausa o se
  // corta, en cualquier pantalla. El estado lo mantiene cast.svelte.ts.
  import { cast, control, detener, fmtTiempo } from "$lib/cast.svelte";
  import { notify } from "$lib/notifStore.svelte";

  let busy = $state(false);

  const st = $derived(cast.estado);
  const pausada = $derived(st?.estado === "PAUSED");
  const pct = $derived(
    st && st.duracion > 0 ? Math.min(100, (st.pos / st.duracion) * 100) : 0,
  );

  async function hacer(f: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await f();
    } catch (e) {
      notify("warn", "La TV no respondió", String(e));
    }
    busy = false;
  }
</script>

{#if st?.activa && !cast.controlesAbiertos}
  <div class="pill-wrap">
    <div class="pill">
      <span class="tv">📺</span>
      <span class="txt">
        <span class="title">{st.titulo}</span>
        <span class="sub">
          {st.tv} ·
          {#if st.estado === "LOADING" || st.estado === "BUFFERING"}cargando…
          {:else if st.estado === "SIN_CONEXION"}la TV no responde…
          {:else}{fmtTiempo(st.pos)}{st.duracion > 0 ? ` / ${fmtTiempo(st.duracion)}` : ""}{pausada ? " · en pausa" : ""}{/if}
        </span>
      </span>
      <button
        class="btn"
        disabled={busy}
        onclick={() => hacer(() => control(pausada ? "seguir" : "pausa"))}
        title={pausada ? "Seguir" : "Pausa"}
      >{pausada ? "▶" : "⏸"}</button>
      <button class="btn close" disabled={busy} onclick={() => hacer(detener)} title="Detener en la TV">⏹</button>
    </div>
    {#if pct > 0}
      <div class="bar"><div class="fill" style:width={`${pct}%`}></div></div>
    {/if}
  </div>
{/if}

<style>
  .pill-wrap {
    position: fixed;
    right: 22px;
    bottom: 22px;
    z-index: 900;
    width: min(420px, calc(100vw - 32px));
    border-radius: 16px;
    overflow: hidden;
    background: rgba(18, 15, 10, 0.94);
    border: 1px solid rgba(249, 115, 22, 0.45);
    box-shadow: 0 18px 40px rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(8px);
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 10px 10px 16px;
    color: #e8e0d0;
  }
  .tv {
    font-size: 20px;
    line-height: 1;
  }
  .txt {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-size: 14.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 12px;
    color: #b9b3a6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btn {
    flex: 0 0 auto;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #e8e0d0;
    font-size: 15px;
    cursor: pointer;
  }
  .btn:hover,
  .btn:focus-visible {
    background: rgba(249, 115, 22, 0.2);
    outline: none;
  }
  .close:hover,
  .close:focus-visible {
    background: rgba(239, 68, 68, 0.22);
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .bar {
    height: 3px;
    background: rgba(255, 255, 255, 0.1);
  }
  .fill {
    height: 100%;
    background: #f97316;
  }
</style>
