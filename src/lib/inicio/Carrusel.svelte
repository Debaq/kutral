<script lang="ts">
  // Escenas (backdrops) del título en pantalla completa.
  import { IMG, img, onImgError } from "$lib/imagenes.svelte";

  let {
    imagenes,
    idx,
    onCerrar,
    onMover,
  }: {
    imagenes: string[];
    idx: number;
    onCerrar: () => void;
    onMover: (delta: number) => void;
  } = $props();
</script>

<div class="carousel-bg" onclick={onCerrar} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") onCerrar(); }} role="presentation">
  <button data-nav class="modal-close carousel-close" onclick={onCerrar} title="Cerrar (Esc)">✕</button>
  <button class="carousel-arrow left" onclick={(e) => { e.stopPropagation(); onMover(-1); }} aria-label="Anterior">‹</button>
  <div class="carousel-img-wrap" role="presentation" onclick={(e) => e.stopPropagation()}>
    <img
      class="carousel-img"
      src={img(`${IMG}/original${imagenes[idx]}`, 1280)}
      alt={`Escena ${idx + 1}`}
      onerror={onImgError}
    />
  </div>
  <button class="carousel-arrow right" onclick={(e) => { e.stopPropagation(); onMover(1); }} aria-label="Siguiente">›</button>
  <div class="carousel-counter">{idx + 1} / {imagenes.length}</div>
</div>

<style>
  .carousel-bg {
    position: fixed; inset: 0; z-index: 200;
    background: rgba(0,0,0,0.92);
    display: flex; align-items: center; justify-content: center;
    gap: 12px;
  }
  .carousel-img {
    max-width: 88vw; max-height: 82vh;
    border-radius: 10px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.8);
    object-fit: contain;
  }
  .carousel-close {
    position: absolute; top: 24px; right: 28px;
    z-index: 2;
  }
  .carousel-arrow {
    flex: 0 0 auto;
    width: 72px; height: 72px;
    border-radius: 50%;
    border: none;
    background: rgba(255,255,255,0.1);
    color: #fff; font-size: 48px; line-height: 1;
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s;
  }
  .carousel-arrow:hover { background: rgba(255,255,255,0.22); }
  .carousel-counter {
    position: absolute; bottom: 28px; left: 50%;
    transform: translateX(-50%);
    color: #ddd; font-size: 20px;
    background: rgba(0,0,0,0.5);
    padding: 6px 18px; border-radius: 999px;
  }
  .modal-close {
    position: absolute; top: 12px; right: 12px;
    width: 32px; height: 32px; border-radius: 50%;
    background: #1a1a22; border: 1px solid #2a2a35;
    color: #aaa; font-size: 14px; cursor: pointer;
    transition: all 0.12s;
  }
  .modal-close:hover { background: #f5c518; color: #000; border-color: #f5c518; }
</style>
