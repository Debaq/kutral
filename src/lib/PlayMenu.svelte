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
  <div class="pm-card">
    <h2 class="pm-title">{title}</h2>
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

<style>
  .pm-root {
    position: fixed;
    inset: 0;
    background: rgba(8, 8, 12, 0.92);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .pm-card {
    background: #15151c;
    border: 1px solid #22222c;
    border-radius: 16px;
    padding: 28px 30px;
    width: min(440px, 90vw);
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  .pm-title {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    color: #fff;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pm-sub {
    margin: 4px 0 18px;
    color: #888892;
    font-size: 13px;
  }
  .pm-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pm-btn {
    text-align: left;
    background: #0d0d12;
    border: 2px solid #1f1f28;
    color: #e6e6ec;
    border-radius: 10px;
    padding: 13px 16px;
    font-size: 15px;
    cursor: pointer;
  }
  .pm-btn.focused {
    border-color: #f3a951;
    background: #1a1410;
    color: #fff;
  }
</style>
