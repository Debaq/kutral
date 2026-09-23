<script lang="ts">
  // Aviso sobre el catálogo: el título elegido no se puede abrir.
  import { autofocusFirst } from "$lib/inicio/util";

  let {
    titulo,
    motivo,
    onCerrar,
    onTrailer,
  }: {
    titulo: string;
    motivo: "404" | "no_imdb";
    onCerrar: () => void;
    onTrailer: () => void;
  } = $props();
</script>

<div class="modal-bg" onclick={onCerrar} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") onCerrar(); }} role="presentation">
  <div
    class="modal"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    use:autofocusFirst
  >
    <h3>No disponible</h3>
    <p>
      {#if motivo === "404"}
        <strong>{titulo}</strong> aún no se encuentra en playimdb.com.
      {:else}
        <strong>{titulo}</strong> no tiene IMDb ID en TMDb, así que no podemos abrirlo.
      {/if}
    </p>
    <p class="modal-sub">¿Querés ver el trailer mientras tanto?</p>
    <div class="modal-actions">
      <button data-nav class="btn-secondary" onclick={onCerrar}>
        Cancelar
      </button>
      <button data-nav class="btn-primary" onclick={onTrailer}>
        🎬 Ver trailer
      </button>
    </div>
  </div>
</div>

<style>
  .modal-bg {
    position: fixed; inset: 0; z-index: 800;
    background: rgba(0,0,0,0.7);
    display: flex; align-items: center; justify-content: center;
    backdrop-filter: blur(4px);
  }
  .modal {
    background: #15151c; border: 1px solid #2a2a35;
    border-radius: 10px; padding: 28px;
    max-width: 460px; width: 90%;
    box-shadow: 0 20px 60px rgba(0,0,0,0.7);
  }
  .modal h3 {
    margin: 0 0 14px;
    color: #f5c518; font-size: 22px;
  }
  .modal p { margin: 0 0 10px; color: #ddd; font-size: 15px; line-height: 1.5; }
  .modal .modal-sub { color: #888; font-size: 14px; margin-bottom: 22px; }
  .modal-actions { display: flex; gap: 10px; justify-content: flex-end; }
  .modal-actions button {
    padding: 10px 20px; border-radius: 6px;
    font-size: 14px; font-weight: 600; cursor: pointer;
    border: 1px solid transparent;
  }
  .btn-secondary {
    background: transparent; color: #aaa; border-color: #2a2a35;
  }
  .btn-secondary:hover { color: #fff; border-color: #444; }
  .btn-primary {
    background: #f5c518; color: #0d0d12; border: 0;
  }
  .btn-primary:hover { transform: scale(1.03); }
</style>
