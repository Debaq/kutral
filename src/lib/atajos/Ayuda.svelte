<script lang="ts">
  // Overlay global de ayuda. Vive en +layout.svelte, escucha I-I (doble tap) y Esc.
  // Cada ruta registra sus atajos llamando ayuda.set(pantalla, lineas).

  import { fade } from "svelte/transition";
  import { ayuda } from "./store.svelte";

  const DOUBLE_TAP_MS = 500;
  let lastIAt = 0;

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    // Ignorar inputs/textareas/contenteditable para no romper escritura.
    const target = e.target as HTMLElement | null;
    if (target) {
      const tag = target.tagName;
      if (
        tag === "INPUT" ||
        tag === "TEXTAREA" ||
        target.isContentEditable
      ) {
        return;
      }
    }

    if (k === "i" || k === "I") {
      const now = Date.now();
      if (now - lastIAt <= DOUBLE_TAP_MS) {
        e.preventDefault();
        ayuda.toggle();
        lastIAt = 0;
      } else {
        lastIAt = now;
      }
      return;
    }
    // Cualquier otra tecla (excepto modificadores) rompe la secuencia I-I.
    if (k !== "Shift" && k !== "Control" && k !== "Alt" && k !== "Meta") {
      lastIAt = 0;
    }
    // Modales se cierran con Esc o Backspace.
    if (ayuda.visible && (k === "Escape" || k === "Backspace")) {
      e.preventDefault();
      e.stopPropagation();
      ayuda.close();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- Hint global abajo derecha — visible siempre, en toda ruta. -->
<div class="hint-global" aria-hidden="true">
  <kbd>I</kbd><kbd>I</kbd> ayuda
</div>

{#if ayuda.visible}
  <div class="overlay" transition:fade={{ duration: 180 }}>
    <div class="caja">
      <h3 class="titulo">Atajos disponibles</h3>
      <div class="lineas">
        {#if ayuda.lineas.length === 0}
          <p class="vacio">Esta pantalla no tiene atajos registrados.</p>
        {/if}
        {#each ayuda.lineas as a}
          <div class="linea">
            <kbd class="tecla">{a.tecla}</kbd>
            <span class="desc">{a.desc}</span>
          </div>
        {/each}
      </div>
      <p class="cierra"><kbd>I</kbd><kbd>I</kbd> o <kbd>Esc</kbd> para cerrar</p>
    </div>
  </div>
{/if}

<style>
  .hint-global {
    position: fixed;
    bottom: 14px;
    right: 18px;
    z-index: 90;
    color: #998878;
    font-size: 12px;
    opacity: 0.7;
    letter-spacing: 0.4px;
    pointer-events: none;
  }
  .hint-global :global(kbd) {
    margin-right: 4px;
  }

  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(10, 6, 8, 0.78);
    backdrop-filter: blur(8px);
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }
  .caja {
    background: rgba(40, 18, 10, 0.96);
    border: 1.5px solid rgba(255, 140, 60, 0.5);
    border-radius: 18px;
    padding: 28px 36px;
    max-width: 480px;
    width: 100%;
    box-shadow:
      0 20px 60px rgba(0, 0, 0, 0.7),
      0 0 48px rgba(255, 140, 60, 0.18);
    color: #ffe6c8;
    font-family:
      "Inter",
      system-ui,
      sans-serif;
  }
  .titulo {
    color: #ffd6a8;
    font-size: 22px;
    margin: 0 0 18px;
    text-align: center;
  }
  .lineas {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 18px;
  }
  .linea {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .tecla {
    min-width: 90px;
    text-align: center;
    font-size: 14px;
    padding: 4px 10px;
    background: rgba(255, 140, 60, 0.25);
    border: 1px solid rgba(255, 140, 60, 0.55);
    border-radius: 6px;
    color: #ffe6c8;
  }
  .desc {
    color: #ffe6c8;
    font-size: 15px;
  }
  .vacio {
    color: #998878;
    font-style: italic;
    font-size: 14px;
    text-align: center;
    margin: 0;
  }
  .cierra {
    color: #998878;
    font-size: 12px;
    text-align: center;
    margin: 0;
    border-top: 1px solid rgba(255, 140, 60, 0.18);
    padding-top: 12px;
  }
  .cierra :global(kbd) {
    margin: 0 4px;
  }
</style>
