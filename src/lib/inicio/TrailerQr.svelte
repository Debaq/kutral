<script lang="ts">
  // Último recurso cuando el trailer no se puede reproducir acá: QR al video
  // de YouTube para verlo en el celular, o abrirlo en el navegador.
  let {
    qr,
    url,
    onVolver,
    onAbrir,
  }: {
    qr: string;
    url: string;
    onVolver: () => void;
    onAbrir: () => void;
  } = $props();
</script>

<div class="discover-mode trailer-qr-mode">
  <div class="trailer-bar">
    <button data-nav class="bar-btn" onclick={onVolver} title="Volver (Esc / Backspace)">
      ← Volver
    </button>
    <div class="trailer-badge-inline">TRAILER</div>
  </div>
  <div class="trailer-qr">
    <h2>Trailer disponible en YouTube</h2>
    <p>Escanea el código con el celular, o ábrelo acá en el navegador.</p>
    <img src={qr} alt="Código QR del trailer en YouTube" />
    <code>{url}</code>
    <button data-nav class="trailer-open" onclick={onAbrir}>
      🌐  Abrir en el navegador
    </button>
  </div>
</div>

<style>
  .discover-mode { position: fixed; inset: 0; background: #000; z-index: 1500; }
  .trailer-bar {
    position: absolute; top: 0; left: 0; right: 0; z-index: 1100;
    display: flex; align-items: center; gap: 10px;
    padding: 10px 14px;
    background: linear-gradient(180deg, rgba(0,0,0,0.85) 0%, rgba(0,0,0,0) 100%);
    pointer-events: none;
  }
  .trailer-bar > * { pointer-events: auto; }
  .trailer-qr-mode { display: grid; place-items: center; }
  .trailer-qr {
    display: flex; flex-direction: column; align-items: center; gap: 18px;
    padding: 40px; text-align: center;
  }
  .trailer-qr h2 { margin: 0; font-size: 28px; color: #fff; }
  .trailer-qr p { margin: 0; color: #9a9aa8; font-size: 15px; }
  .trailer-qr img {
    width: 300px; height: 300px;
    background: #fff; padding: 12px; border-radius: 12px;
  }
  .trailer-qr code { color: #f5c518; font-size: 14px; letter-spacing: 0.5px; }
  .trailer-open {
    background: #f5c518;
    color: #0d0d12;
    border: 2px solid transparent;
    padding: 12px 26px;
    border-radius: 999px;
    font-size: 15px;
    font-weight: 800;
    cursor: pointer;
  }
  .trailer-open:hover,
  .trailer-open:focus,
  .trailer-open:focus-visible {
    outline: none;
    border-color: #fff;
    box-shadow: 0 0 0 4px rgba(245, 197, 24, 0.28);
  }
  .trailer-badge-inline {
    padding: 5px 12px;
    background: #f5c518; color: #0d0d12;
    font-weight: 900; font-size: 11px; letter-spacing: 2px;
    border-radius: 4px;
  }
  .bar-btn {
    background: rgba(20, 20, 28, 0.85);
    color: #fff;
    border: 1px solid #333;
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 13px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }
  .bar-btn:hover { background: #f5c518; color: #0d0d12; border-color: #f5c518; }
</style>
