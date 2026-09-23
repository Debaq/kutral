<script lang="ts">
  // El título abierto no se puede reproducir: se ofrece el trailer o
  // desmarcarlo si volvió a estar disponible.
  import { art, onImgError } from "$lib/imagenes.svelte";
  import type { Detail } from "$lib/tipos";

  let {
    titulo,
    onVolver,
    onTrailer,
    onMarcarDisponible,
  }: {
    titulo: Detail;
    onVolver: () => void;
    onTrailer: () => void;
    onMarcarDisponible: () => void;
  } = $props();
</script>

<div class="unavail-screen">
  <div class="unavail-card">
    {#if titulo.poster_path}
      <img class="unavail-poster" src={art(titulo.poster_path, "w342", 342)} alt="" onerror={onImgError} />
    {/if}
    <h2>Lo sentimos muchísimo</h2>
    <p>
      <strong>{titulo.title}</strong> aún no se encuentra en nuestra cartelera.
    </p>
    <p class="unavail-sub">La marcamos así no la sugerimos próximamente. Si volviera a estar disponible, puedes desmarcarla.</p>
    <div class="unavail-actions">
      <button data-nav class="unavail-back btn-secondary" onclick={onVolver}>← Volver</button>
      {#if titulo.imdb_id}
        <button data-nav class="btn-primary" onclick={onTrailer}>🎬 Ver trailer</button>
        <button data-nav class="btn-secondary" onclick={onMarcarDisponible}>
          Marcar disponible
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .unavail-screen {
    position: fixed; inset: 0; z-index: 1500;
    background: #0d0d12;
    display: flex; align-items: center; justify-content: center;
    padding: 40px;
  }
  .unavail-card {
    max-width: 480px; text-align: center;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 12px;
    padding: 36px 32px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.6);
  }
  .unavail-poster {
    width: 140px; border-radius: 8px;
    margin-bottom: 20px;
    box-shadow: 0 8px 20px rgba(0,0,0,0.5);
    filter: grayscale(0.6);
  }
  .unavail-card h2 {
    margin: 0 0 12px;
    color: #f5c518; font-size: 24px;
  }
  .unavail-card p { color: #ddd; font-size: 15px; line-height: 1.5; margin: 0 0 10px; }
  .unavail-sub { color: #888 !important; font-size: 13px !important; margin-bottom: 22px !important; }
  .unavail-actions {
    display: flex; gap: 10px; justify-content: center; flex-wrap: wrap;
    margin-top: 22px;
  }
  .unavail-actions button {
    padding: 10px 18px; border-radius: 6px;
    font-size: 14px; font-weight: 600; cursor: pointer;
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
