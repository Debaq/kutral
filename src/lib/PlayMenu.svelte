<script lang="ts">
  // Menú contextual al pulsar "Descubrir" sobre una película.
  // Muestra solo las acciones válidas según estado (progreso, RD vinculado).
  // Teclado propio (flechas/Enter/Esc) — el page no navega en este modo.

  type Action = {
    id: string;
    label: string;
    run: () => void;
  };

  let {
    title,
    progressLabel,
    backdrop = null,
    year = null,
    overview = null,
    hasRd,
    isMovie,
    onContinue,
    onRestart,
    onRealDebrid,
    onResearch,
    onWeb,
    onClose,
  }: {
    title: string;
    progressLabel: string | null; // "45%" / "12m 3s" / null si no hay progreso
    backdrop?: string | null; // URL del fanart (backdrop)
    year?: string | null;
    overview?: string | null;
    hasRd: boolean;
    isMovie: boolean;
    onContinue: () => void;
    onRestart: () => void;
    onRealDebrid: () => void;
    onResearch: () => void;
    onWeb: () => void;
    onClose: () => void;
  } = $props();

  // Construye la lista de acciones según contexto.
  const actions: Action[] = (() => {
    const a: Action[] = [];
    if (progressLabel) {
      a.push({ id: "cont", label: `▶  Continuar (${progressLabel})`, run: onContinue });
      a.push({ id: "restart", label: "↻  Empezar de nuevo", run: onRestart });
    }
    if (hasRd && isMovie) {
      a.push({ id: "rd", label: "⚡  Ver con RealDebrid", run: onRealDebrid });
      a.push({ id: "research", label: "🔄  Rebuscar fuentes", run: onResearch });
    }
    a.push({ id: "web", label: "🌐  Ver en player web", run: onWeb });
    a.push({ id: "back", label: "←  Volver", run: onClose });
    return a;
  })();

  let focusIdx = $state(0);

  function move(d: number) {
    let i = focusIdx + d;
    if (i < 0) i = actions.length - 1;
    if (i > actions.length - 1) i = 0;
    focusIdx = i;
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
      case "Enter":
      case " ":
        e.preventDefault();
        actions[focusIdx]?.run();
        break;
      case "Escape":
      case "Backspace":
        e.preventDefault();
        onClose();
        break;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="pm-root">
  {#if backdrop}
    <div class="pm-bg" style:background-image="url({backdrop})"></div>
  {/if}
  <div class="pm-scrim"></div>

  <div class="pm-content">
    <div class="pm-info">
      <h2 class="pm-title">{title}</h2>
      {#if year}<span class="pm-year">{year}</span>{/if}
      {#if overview}<p class="pm-overview">{overview}</p>{/if}
    </div>

    <div class="pm-panel">
      <p class="pm-sub">¿Cómo quieres verla?</p>
      <div class="pm-list">
        {#each actions as a, i (a.id)}
          <button
            class="pm-btn"
            class:focused={focusIdx === i}
            onclick={a.run}
            onmouseenter={() => (focusIdx = i)}
          >
            {a.label}
          </button>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .pm-root {
    position: fixed;
    inset: 0;
    z-index: 50;
    overflow: hidden;
    background: #0d0d12;
  }
  /* Fanart full-bleed, ligeramente difuminado para que el texto resalte. */
  .pm-bg {
    position: absolute;
    inset: -40px; /* margen para que el blur no muestre bordes */
    background-size: cover;
    background-position: center;
    filter: blur(8px) saturate(1.1);
    transform: scale(1.06);
  }
  /* Gradiente oscuro: legibilidad y profundidad cinematográfica. */
  .pm-scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, rgba(8, 8, 12, 0.95) 0%, rgba(8, 8, 12, 0.7) 45%, rgba(8, 8, 12, 0.35) 100%),
      linear-gradient(0deg, rgba(8, 8, 12, 0.85) 0%, rgba(8, 8, 12, 0.1) 50%);
  }

  .pm-content {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 22px;
    padding: 0 56px 56px;
    max-width: 720px;
  }
  .pm-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pm-title {
    margin: 0;
    font-size: 38px;
    font-weight: 700;
    color: #fff;
    line-height: 1.05;
    text-shadow: 0 2px 18px rgba(0, 0, 0, 0.6);
  }
  .pm-year {
    color: #c8c8d0;
    font-size: 15px;
    font-variant-numeric: tabular-nums;
  }
  .pm-overview {
    margin: 4px 0 0;
    color: #c0c0c8;
    font-size: 14px;
    line-height: 1.5;
    max-width: 560px;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-shadow: 0 1px 10px rgba(0, 0, 0, 0.5);
  }

  .pm-panel {
    width: min(420px, 100%);
  }
  .pm-sub {
    margin: 0 0 12px;
    color: #b0b0ba;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .pm-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pm-btn {
    text-align: left;
    background: rgba(20, 20, 28, 0.6);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 2px solid rgba(255, 255, 255, 0.12);
    color: #e6e6ec;
    border-radius: 10px;
    padding: 13px 18px;
    font-size: 15px;
    cursor: pointer;
    transition: border-color 0.12s, background 0.12s, transform 0.12s;
  }
  .pm-btn.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
    transform: translateX(4px);
  }
</style>
