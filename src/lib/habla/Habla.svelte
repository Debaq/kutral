<script lang="ts">
  // Burbuja Habla: chiquita, ignorable, NUNCA bloqueante.
  // Aparece sola tras un delay y desaparece sola si el usuario no responde.
  // Dos caras según contexto: si está solo, es compañía;
  // si hay gente, empuja hacia los humanos reales.

  import { fade, fly } from "svelte/transition";
  import type { Contexto } from "$lib/vera/tipos";
  import { fraseHabla, type MomentoHabla } from "./frases";

  interface Props {
    contexto: Contexto;
    momento: MomentoHabla;
    visible: boolean;
    onCerrar?: () => void;
    // Si es true, la frase se ve como "joya" — más íntima, sin botones.
    joya?: boolean;
  }

  let { contexto, momento, visible, onCerrar, joya = false }: Props =
    $props();

  // Frase recalculada cuando cambia el momento o contexto.
  let frase = $derived.by(() => {
    if (joya) return fraseHabla("joya", contexto);
    return fraseHabla(momento, contexto);
  });

  function cerrar() {
    onCerrar?.();
  }
</script>

{#if visible}
  <div
    class="habla"
    class:joya
    transition:fly={{ y: 20, duration: 380 }}
  >
    <div class="avatar" aria-hidden="true"></div>
    <div class="texto">{frase}</div>
    {#if !joya}
      <button class="cerrar" onclick={cerrar} aria-label="cerrar">×</button>
    {/if}
  </div>
{/if}

<style>
  .habla {
    position: fixed;
    bottom: 24px;
    left: 24px;
    max-width: 320px;
    padding: 14px 16px;
    background: rgba(20, 30, 50, 0.92);
    color: #d6ecff;
    border: 1px solid rgba(124, 188, 255, 0.45);
    border-radius: 16px;
    box-shadow: 0 6px 24px rgba(60, 130, 220, 0.25);
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 15px;
    line-height: 1.35;
    z-index: 50;
    backdrop-filter: blur(6px);
  }

  .habla.joya {
    bottom: 32px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 480px;
    background: rgba(15, 25, 45, 0.85);
    border-color: rgba(124, 188, 255, 0.25);
    font-style: italic;
    text-align: center;
    justify-content: center;
  }

  .avatar {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: radial-gradient(circle at 30% 30%, #b3dcff, #4a9bd6);
    flex-shrink: 0;
  }

  .joya .avatar {
    display: none;
  }

  .texto {
    flex: 1;
  }

  .cerrar {
    background: transparent;
    border: none;
    color: #d6ecff;
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
    opacity: 0.6;
  }
  .cerrar:hover {
    opacity: 1;
  }
</style>
