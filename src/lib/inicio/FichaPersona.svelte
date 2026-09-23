<script lang="ts">
  // Ficha de una persona del reparto (TMDb): datos, biografía y filmografía.
  // Elegir un título de la filmografía lo abre en el panel de detalle.
  import { IMG, img, onImgError } from "$lib/imagenes.svelte";
  import { autofocusFirst } from "$lib/inicio/acciones";
  import type { PersonInfo } from "$lib/tipos";

  let {
    persona,
    cargando,
    onCerrar,
    onAbrirTitulo,
  }: {
    persona: PersonInfo | null;
    cargando: boolean;
    onCerrar: () => void;
    onAbrirTitulo: (id: number, mediaType: string) => void;
  } = $props();
</script>

<div class="modal-bg" onclick={onCerrar} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") onCerrar(); }} role="presentation">
  <div class="person-modal" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-modal="true" use:autofocusFirst>
    <button data-nav class="modal-close" onclick={onCerrar} title="Cerrar (Esc)">✕</button>
    {#if cargando}
      <div class="empty">Cargando…</div>
    {:else if persona}
      <div class="person-head">
        {#if persona.profile_path}
          <img class="person-photo" src={img(`${IMG}/w300${persona.profile_path}`, 300)} alt={persona.name} onerror={onImgError} />
        {:else}
          <div class="person-photo person-photo-empty">{persona.name.charAt(0)}</div>
        {/if}
        <div class="person-info">
          <h2>{persona.name}</h2>
          {#if persona.known_for_department}
            <p class="person-dept">{persona.known_for_department}</p>
          {/if}
          {#if persona.birthday}
            <p class="person-meta">
              Nacimiento: {persona.birthday}
              {#if persona.place_of_birth} · {persona.place_of_birth}{/if}
            </p>
          {/if}
          {#if persona.deathday}
            <p class="person-meta">Fallecimiento: {persona.deathday}</p>
          {/if}
        </div>
      </div>
      {#if persona.biography}
        <div class="person-bio">{persona.biography}</div>
      {/if}
      {#if persona.filmography.length}
        <h3 class="person-section">Filmografía destacada</h3>
        <div class="filmography-grid">
          {#each persona.filmography as f}
            <button
              data-nav
              class="film-card"
              title={`${f.title} — ${f.roles.join(", ")}`}
              onclick={() => onAbrirTitulo(f.id, f.media_type)}
            >
              <div class="film-poster-wrap">
                {#if f.poster_path}
                  <img src={img(`${IMG}/w185${f.poster_path}`, 185)} alt={f.title} loading="lazy" onerror={onImgError} />
                {:else}
                  <div class="film-noposter">sin poster</div>
                {/if}
                {#if f.roles.length}
                  <div class="film-pills">
                    {#each f.roles as r}
                      <span class="film-pill">{r}</span>
                    {/each}
                  </div>
                {/if}
              </div>
              <div class="film-meta">
                <span class="film-title">{f.title}</span>
                <span class="film-sub">{f.year}</span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
      <p class="person-foot">Premios no disponibles en TMDb. Para verlos, abrí el perfil en IMDb.</p>
    {/if}
  </div>
</div>

<style>
  .empty { color: #555; padding: 40px 20px; text-align: center; }
  .person-modal {
    position: relative;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 12px;
    width: 90%; max-width: 720px; max-height: 85vh;
    overflow-y: auto;
    padding: 28px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.7);
  }
  .modal-close {
    position: absolute; top: 12px; right: 12px;
    width: 32px; height: 32px; border-radius: 50%;
    background: #1a1a22; border: 1px solid #2a2a35;
    color: #aaa; font-size: 14px; cursor: pointer;
    transition: all 0.12s;
  }
  .modal-close:hover { background: #f5c518; color: #000; border-color: #f5c518; }
  .person-head { display: flex; gap: 18px; align-items: flex-start; margin-bottom: 16px; }
  .person-photo {
    width: 120px; height: 180px; object-fit: cover;
    border-radius: 8px; flex: 0 0 auto;
    box-shadow: 0 8px 20px rgba(0,0,0,0.5);
  }
  .person-photo-empty {
    background: #2a2a35;
    display: flex; align-items: center; justify-content: center;
    font-size: 50px; color: #555; font-weight: 700;
  }
  .person-info { flex: 1; min-width: 0; }
  .person-info h2 { margin: 0 0 6px; color: #f5c518; font-size: 24px; }
  .person-dept { margin: 0 0 8px; color: #aaa; font-size: 13px; }
  .person-meta { margin: 2px 0; color: #ccc; font-size: 13px; }
  .person-bio {
    color: #ccc; font-size: 14px; line-height: 1.6;
    white-space: pre-wrap; margin: 0 0 16px;
  }
  .person-section { margin: 16px 0 12px; color: #f5c518; font-size: 14px; text-transform: uppercase; letter-spacing: 1.5px; }
  .filmography-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
    gap: 12px;
  }
  .film-card {
    background: #1a1a22; border: 0; border-radius: 6px; overflow: hidden;
    color: inherit; text-align: left;
    padding: 0; cursor: pointer;
    display: flex; flex-direction: column;
    transition: transform 0.12s, box-shadow 0.12s;
  }
  .film-card:hover { transform: translateY(-2px); box-shadow: 0 6px 14px rgba(0,0,0,0.5); }
  .film-card:focus, .film-card:focus-visible {
    outline: 3px solid #f5c518; outline-offset: 2px;
    transform: translateY(-2px);
  }
  .film-poster-wrap { position: relative; }
  .film-card img, .film-noposter {
    width: 100%; aspect-ratio: 2/3; object-fit: cover;
    background: #222; display: block;
  }
  .film-noposter {
    display: flex; align-items: center; justify-content: center;
    color: #555; font-size: 11px;
  }
  .film-pills {
    position: absolute; left: 4px; right: 4px; bottom: 4px;
    display: flex; flex-wrap: wrap; gap: 3px;
    justify-content: flex-start;
  }
  .film-pill {
    background: rgba(245, 197, 24, 0.92);
    color: #0d0d12;
    padding: 2px 7px;
    border-radius: 999px;
    font-size: 9px;
    font-weight: 700;
    line-height: 1.3;
    max-width: 100%;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    box-shadow: 0 2px 6px rgba(0,0,0,0.5);
  }
  .film-meta { padding: 6px 8px; }
  .film-title { display: block; font-size: 12px; font-weight: 600; line-height: 1.2;
    overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .film-sub { display: block; font-size: 10px; color: #888; margin-top: 2px; }
  .person-foot { margin: 18px 0 0; color: #666; font-size: 11px; text-align: center; }
  .modal-bg {
    position: fixed; inset: 0; z-index: 800;
    background: rgba(0,0,0,0.7);
    display: flex; align-items: center; justify-content: center;
    backdrop-filter: blur(4px);
  }
</style>
