<script lang="ts">
  // Panel de detalle del título elegido en el catálogo: datos, historial,
  // acciones (Descubrir / trailer), temporadas, reparto y escenas.
  import { IMG, img, art, onImgError } from "$lib/imagenes.svelte";
  import { borrarProgreso, type FilaHistorial, type EstadoMedio } from "$lib/historial.svelte";
  import { seasonLabel } from "$lib/inicio/util";
  import type { Detail } from "$lib/tipos";

  let {
    titulo,
    clave,
    favorito,
    enCurso,
    estado,
    progreso,
    disponible,
    local,
    bajando,
    enCola,
    temporadaAbierta,
    onFavorito,
    onVisto,
    onDescubrir,
    onTrailer,
    onTemporada,
    onPersona,
    onEscenas,
  }: {
    titulo: Detail;
    clave: string;
    favorito: boolean;
    enCurso: FilaHistorial | null;
    estado: EstadoMedio | null;
    progreso: FilaHistorial | null;
    disponible: boolean;
    local: boolean;
    bajando: boolean;
    enCola: boolean;
    temporadaAbierta: number | null;
    onFavorito: () => void;
    onVisto: (visto: boolean) => void;
    onDescubrir: () => void;
    onTrailer: () => void;
    onTemporada: (n: number) => void;
    onPersona: (id: number) => void;
    onEscenas: (imagenes: string[], idx: number) => void;
  } = $props();
</script>

<div class="detail">
  {#if titulo.backdrop_path}
    <div class="backdrop" style:background-image="url({art(titulo.backdrop_path, "w780", 780)})"></div>
  {/if}
  {#if titulo.poster_path}
    <div class="poster-wrap">
      <img class="poster" src={art(titulo.poster_path, "w342", 342)} alt="" onerror={onImgError} />
      {#if !disponible}
        <span class="poster-stamp">NO DISPONIBLE</span>
      {/if}
    </div>
  {/if}
  <h2>{titulo.title} {titulo.year ? `(${titulo.year})` : ""}</h2>
  <div class="meta">
    <span class="rating">★ {titulo.vote_average.toFixed(1)}</span>
    {#if titulo.runtime}<span>{titulo.runtime} min</span>{/if}
    {#each titulo.genres as g}<span class="genre">{g}</span>{/each}
  </div>
  <p class="overview">{titulo.overview || "(sin sinopsis)"}</p>
  {#if clave}
    <div class="hist-row">
      <button
        data-nav
        class="hist-btn"
        class:on={favorito}
        onclick={onFavorito}
      >
        {favorito ? "★ Quitar de favoritos" : "☆ Agregar a favoritos"}
      </button>
      {#if enCurso}
        {@const ec = enCurso}
        <button
          data-nav
          class="hist-btn"
          onclick={() => void borrarProgreso(ec.imdb_id, ec.season, ec.episode)}
          title="Borra el minuto guardado y lo saca de la lista"
        >
          ✕ Quitar de seguir viendo{ec.season >= 0 ? ` (T${ec.season} E${ec.episode})` : ""}
        </button>
      {/if}
      {#if titulo.media_type === "movie"}
        {@const yaVista = progreso?.completed === 1}
        <button
          data-nav
          class="hist-btn"
          class:on={yaVista}
          onclick={() => onVisto(!yaVista)}
          title={yaVista ? "Desmarcar" : "Marcar como vista sin reproducirla"}
        >
          {yaVista ? "✓ Vista" : "✓ Marcar vista"}
        </button>
      {:else if estado?.vistos}
        <span class="hist-info">{estado.vistos} {estado.vistos === 1 ? "capítulo visto" : "capítulos vistos"}</span>
      {/if}
    </div>
  {/if}
  {#if disponible}
    {#if titulo.media_type === "tv"}
      <!-- Series: la reproducción es por capítulo (lista de Temporadas
           abajo). No hay "Descubrir" de título; solo Trailer. -->
      {#if local}
        <div class="local-note">📁 Tienes capítulos bajados en este equipo</div>
      {/if}
      <div class="action-row">
        <button data-nav class="trailer-btn trailer-btn-row" onclick={onTrailer}>🎬 Trailer</button>
      </div>
    {:else}
      {@const prog = progreso}
      {@const pct = prog && prog.runtime_seconds && prog.runtime_seconds > 0
        ? Math.min(100, Math.round((prog.progress_real ?? prog.watched_seconds / prog.runtime_seconds) * 100))
        : null}
      {@const watchedMin = prog ? Math.floor(prog.watched_seconds / 60) : 0}
      {@const watchedSec = prog ? prog.watched_seconds % 60 : 0}
      {#if prog && prog.completed}
        <div class="watched-note">✓ Ya la viste</div>
      {:else if prog && prog.watched_seconds > 5 && pct != null}
        <div class="progress-bar"><div class="progress-fill" style:width="{pct}%"></div></div>
      {/if}
      {#if local}
        <div class="local-note">📁 Ya está en tu equipo — arranca al instante, sin internet</div>
      {:else if bajando}
        <div class="local-note bajando">📥 Bajándola ahora — te avisamos cuando esté lista</div>
      {:else if enCola}
        <div class="local-note bajando">⏳ En la cola de descargas — arranca cuando haya lugar</div>
      {/if}
      <div class="action-row">
        {#if prog && prog.completed}
          <button data-nav class="discover-btn discover-btn-row" onclick={onDescubrir}>▶ Descubrir de nuevo</button>
        {:else if prog && prog.watched_seconds > 5}
          <button data-nav class="discover-btn discover-btn-row" onclick={onDescubrir} title="Opciones de reproducción">
            ↻ Desde el inicio
          </button>
          <button data-nav class="discover-btn discover-btn-row" onclick={onDescubrir}>
            ▶ Continuar {pct != null ? `(${pct}%)` : `(${watchedMin}m ${watchedSec}s)`}
          </button>
        {:else}
          <button data-nav class="discover-btn discover-btn-row" onclick={onDescubrir}>▶ Descubrir</button>
        {/if}
        <button data-nav class="trailer-btn trailer-btn-row" onclick={onTrailer}>🎬 Trailer</button>
      </div>
    {/if}
  {:else}
    <div class="unavail-note">
      Este título no está disponible para reproducir.
    </div>
    <div class="action-row">
      <button data-nav class="discover-btn" onclick={onTrailer}>
        🎬 Ver trailer
      </button>
      {#if titulo.imdb_id}
        <button
          data-nav
          class="trailer-btn"
          onclick={onDescubrir}
          title="Intentar abrir aunque esté marcada como no disponible"
        >
          ⚠ Intentar de todas formas
        </button>
      {/if}
    </div>
  {/if}
  {#if titulo.media_type === "tv" && titulo.seasons && titulo.seasons.length}
    <div class="people-row seasons-row">
      <h4 class="people-title">Temporadas</h4>
      <div class="season-list">
        {#each titulo.seasons as s (s.season_number)}
          <button
            data-nav
            class="season-item"
            class:active={temporadaAbierta === s.season_number}
            onclick={() => onTemporada(s.season_number)}
          >
            <span class="season-num">{seasonLabel(s.season_number)}</span>
            <span class="season-count">{s.episode_count} cap.</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#if titulo.directors.length}
    <div class="people-row">
      <h4 class="people-title">{titulo.media_type === "tv" ? "Creado por" : "Dirigido por"}</h4>
      <div class="people-strip">
        {#each titulo.directors as p}
          <button data-nav class="person-chip" onclick={() => onPersona(p.id)} title={p.name}>
            {#if p.profile_path}
              <img src={art(p.profile_path, "w185", 185)} alt={p.name} loading="lazy" onerror={onImgError} />
            {:else}
              <div class="person-noimg">{p.name.charAt(0)}</div>
            {/if}
            <span class="person-name">{p.name}</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#if titulo.cast.length}
    <div class="people-row">
      <h4 class="people-title">Estelares</h4>
      <div class="people-strip">
        {#each titulo.cast as p}
          <button data-nav class="person-chip" onclick={() => onPersona(p.id)} title={p.name}>
            {#if p.profile_path}
              <img src={art(p.profile_path, "w185", 185)} alt={p.name} loading="lazy" onerror={onImgError} />
            {:else}
              <div class="person-noimg">{p.name.charAt(0)}</div>
            {/if}
            <span class="person-name">{p.name}</span>
            {#if p.character}<span class="person-character">{p.character}</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#if titulo.images && titulo.images.length}
    {@const scenes = titulo.images}
    <div class="people-row">
      <h4 class="people-title">Escenas</h4>
      <div class="scenes-strip">
        {#each scenes.slice(0, 8) as path, i}
          <button
            data-nav
            class="scene-shot"
            style:background-image="url({img(`${IMG}/w500${path}`, 500)})"
            onclick={() => onEscenas(scenes, i)}
            title="Ver escena (Enter)"
            aria-label={`Escena ${i + 1}`}
          ></button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .detail { position: relative; padding: 0 20px 20px; flex: 1; }
  .backdrop {
    position: absolute; top: 0; left: 0; right: 0; height: 240px;
    background-size: cover; background-position: center;
    opacity: 0.35;
    mask-image: linear-gradient(180deg, #000 0%, transparent 100%);
    -webkit-mask-image: linear-gradient(180deg, #000 0%, transparent 100%);
    z-index: 0;
  }
  .poster-wrap { position: relative; display: inline-block; margin-top: 60px; z-index: 1; }
  .poster { width: 180px; border-radius: 6px; box-shadow: 0 8px 24px rgba(0,0,0,0.6); display: block; }
  .poster-stamp {
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%) rotate(-18deg);
    background: rgba(220, 38, 38, 0.92);
    color: #fff;
    padding: 6px 18px;
    font-weight: 900; font-size: 14px;
    letter-spacing: 1.5px;
    border: 3px solid #fff;
    border-radius: 3px;
    box-shadow: 0 4px 14px rgba(0,0,0,0.7);
    text-transform: uppercase;
    white-space: nowrap;
    pointer-events: none;
  }
  .detail h2 { position: relative; margin: 14px 0 8px; font-size: 22px; z-index: 1; }
  .meta { display: flex; flex-wrap: wrap; gap: 8px; font-size: 12px; color: #aaa; margin-bottom: 12px; position: relative; z-index: 1; }
  .rating { color: #f5c518; font-weight: 700; }
  .genre { background: #1a1a22; padding: 2px 8px; border-radius: 999px; }
  .overview { color: #c0c0c8; font-size: 14px; line-height: 1.5; position: relative; z-index: 1; }
  .discover-btn {
    position: relative; z-index: 1;
    background: #f5c518; color: #000; border: 0;
    padding: 12px 24px; font-weight: 800; font-size: 16px;
    border-radius: 6px; cursor: pointer; width: 100%;
    margin-top: 12px;
    transition: transform 0.1s;
  }
  .discover-btn:hover:not(:disabled) { transform: scale(1.02); }
  .discover-btn:disabled { opacity: 0.6; cursor: wait; }
  .trailer-btn {
    position: relative; z-index: 1;
    background: transparent; color: #f5c518;
    border: 1.5px solid #f5c518;
    padding: 10px 20px; font-weight: 700; font-size: 14px;
    border-radius: 6px; cursor: pointer; width: 100%;
    margin-top: 8px;
    transition: background 0.12s, color 0.12s;
  }
  .trailer-btn:hover { background: #f5c518; color: #0d0d12; }
  .unavail-note {
    position: relative; z-index: 1;
    margin: 12px 0;
    padding: 10px 12px;
    background: rgba(220, 38, 38, 0.12);
    border: 1px solid rgba(220, 38, 38, 0.4);
    border-radius: 6px;
    color: #ffb4b4;
    font-size: 13px;
    text-align: center;
  }
  .action-row { display: flex; gap: 8px; position: relative; z-index: 1; margin-top: 12px; }
  .discover-btn-row { flex: 1; padding: 12px 16px; font-size: 15px; }
  .trailer-btn-row { width: auto; padding: 12px 18px; margin-top: 0 !important; font-size: 14px; }
  .people-row { position: relative; z-index: 1; margin-top: 18px; }
  .people-title {
    margin: 0 0 8px; font-size: 12px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 1px;
    color: #888;
  }
  .people-strip {
    display: flex; gap: 10px; flex-wrap: wrap;
  }
  .scenes-strip {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .scene-shot {
    width: 100%;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    border: 2px solid transparent;
    padding: 0;
    background-size: cover; background-position: center;
    background-color: #2a2a35;
    box-shadow: 0 4px 10px rgba(0,0,0,0.4);
    cursor: pointer;
    transition: transform 0.15s, border-color 0.15s;
  }
  .scene-shot:hover { transform: translateY(-2px); }
  .scene-shot:focus {
    outline: none;
    border-color: #f5c518;
    transform: translateY(-2px) scale(1.03);
    box-shadow: 0 6px 18px rgba(0,0,0,0.55), 0 0 0 2px rgba(245,197,24,0.4);
  }
  .person-chip {
    flex: 0 0 80px; width: 80px;
    background: transparent; border: 0; padding: 0;
    color: inherit; cursor: pointer;
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
    transition: transform 0.15s;
  }
  .person-chip:hover { transform: translateY(-2px); }
  .person-chip:focus { outline: 3px solid #f5c518; outline-offset: 2px; border-radius: 6px; }
  .person-chip img, .person-noimg {
    width: 64px; height: 64px;
    border-radius: 50%; object-fit: cover;
    background: #2a2a35;
    box-shadow: 0 4px 10px rgba(0,0,0,0.4);
  }
  .person-noimg {
    display: flex; align-items: center; justify-content: center;
    font-size: 22px; font-weight: 700; color: #888;
  }
  .person-name {
    font-size: 11px; font-weight: 600; line-height: 1.2;
    text-align: center; color: #ddd;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2; line-clamp: 2;
    -webkit-box-orient: vertical;
    max-width: 80px;
  }
  .person-character {
    font-size: 10px; color: #777; line-height: 1.1;
    text-align: center;
    overflow: hidden; text-overflow: ellipsis;
    max-width: 80px; white-space: nowrap;
  }
  .progress-bar {
    position: relative; z-index: 1;
    height: 6px; background: #1f1f28; border-radius: 3px;
    overflow: hidden; margin: 4px 0 8px;
  }
  .progress-fill { height: 100%; background: linear-gradient(90deg, #f5c518, #ff9b00); transition: width 0.3s; }
  .local-note {
    position: relative; z-index: 1;
    background: rgba(64, 160, 120, 0.15);
    border: 1px solid rgba(64, 160, 120, 0.4);
    color: #8fe3a8;
    padding: 6px 10px; border-radius: 6px;
    font-size: 12px; text-align: center;
    margin: 8px 0;
  }
  .local-note.bajando {
    background: rgba(243, 169, 81, 0.13);
    border-color: rgba(243, 169, 81, 0.4);
    color: #f3c489;
  }
  .watched-note {
    position: relative; z-index: 1;
    background: rgba(82, 181, 96, 0.15);
    border: 1px solid rgba(82, 181, 96, 0.4);
    color: #b6e9bf;
    padding: 6px 10px; border-radius: 6px;
    font-size: 12px; text-align: center;
    margin: 8px 0;
  }
  .hist-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 10px 0 4px;
  }
  .hist-btn {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #d8d8e0;
    border-radius: 8px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .hist-btn:hover,
  .hist-btn:focus-visible {
    border-color: #f3a951;
    color: #fff;
    outline: none;
  }
  .hist-btn.on {
    background: rgba(47, 158, 68, 0.18);
    border-color: #2f9e44;
    color: #7ee094;
  }
  .hist-info {
    font-size: 12px;
    color: #9a9aa4;
  }
  .season-list {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .season-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: rgba(20, 20, 28, 0.6);
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 10px;
    padding: 10px 14px;
    cursor: pointer;
    color: #e6e6ec;
    text-align: left;
  }
  .season-item:focus-visible,
  .season-item:hover {
    outline: none;
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
  }
  .season-item.active {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.22);
    color: #fff;
  }
  .season-num { font-size: 14px; font-weight: 600; }
  .season-count { font-size: 12px; color: #9a9aa4; }
</style>
