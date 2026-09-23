<script lang="ts">
  // Reproductor web (iframe del proveedor) a pantalla completa.
  let {
    src,
    frame = $bindable(null),
    onVolver,
    onReportar,
  }: {
    src: string;
    frame?: HTMLIFrameElement | null;
    onVolver: () => void;
    onReportar: () => void;
  } = $props();
</script>

<div class="discover-mode">
  <button data-nav class="back-btn" onclick={onVolver} title="Volver (Esc / Backspace)">
    <span class="arrow">←</span> Volver
  </button>
  <button data-nav class="report-btn" onclick={onReportar} title="Marcar como no disponible">
    ⚠ No funciona
  </button>
  <iframe
    bind:this={frame}
    {src}
    title="discover"
    referrerpolicy="no-referrer"
    sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-presentation"
    allow="autoplay; fullscreen; picture-in-picture"
  ></iframe>
</div>

<style>
  .discover-mode { position: fixed; inset: 0; background: #000; z-index: 1500; }
  .discover-mode iframe { width: 100%; height: 100%; border: 0; background: #000; }
  .back-btn {
    position: absolute; top: 14px; right: 14px; z-index: 1100;
    display: inline-flex; align-items: center; gap: 8px;
    padding: 10px 18px 10px 14px;
    background: rgba(0,0,0,0.75);
    color: #fff;
    border: 1px solid #333;
    border-radius: 999px;
    font-size: 14px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.15s, color 0.15s, transform 0.1s;
  }
  .back-btn .arrow { font-size: 18px; line-height: 1; }
  .back-btn:hover { background: #f5c518; color: #000; border-color: #f5c518; }
  .back-btn:active { transform: scale(0.97); }
  .report-btn {
    position: absolute; top: 14px; left: 14px; z-index: 1100;
    padding: 10px 16px;
    background: rgba(20,20,28,0.75);
    color: #ffb4b4;
    border: 1px solid rgba(220,38,38,0.6);
    border-radius: 999px;
    font-size: 13px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.12s, color 0.12s;
  }
  .report-btn:hover { background: rgba(220,38,38,0.9); color: #fff; }
</style>
