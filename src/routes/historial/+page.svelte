<script lang="ts">
  // Lo que viste, agrupado por fecha, y tus favoritos.
  //
  // Misma gramática visual que el catálogo: grid de pósters, foco amarillo,
  // navegación espacial con flechas ($lib/nav). Nada acá necesita el mouse:
  //   ←→↑↓  mover        Enter  reproducir
  //   Supr  quitar       Esc / Backspace  volver
  // Las pestañas son chips en la fila de filtros: se llega con ↑ desde la
  // primera fila del grid, igual que en la home.
  //
  // Los datos salen del store del historial, ya en memoria: sin queries acá.
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { navegar, enfocarPrimero } from "$lib/nav";
  import {
    historial,
    seguirViendo,
    listaFavoritos,
    borrarProgreso,
    alternarFavorito,
    cargarHistorial,
    type FilaHistorial,
    type FilaFavorito,
  } from "$lib/historial.svelte";

  type Pestana = "seguir" | "vistos" | "favoritos";
  const PESTANAS: { id: Pestana; label: string }[] = [
    { id: "seguir", label: "Seguir viendo" },
    { id: "vistos", label: "Historial" },
    { id: "favoritos", label: "Favoritos" },
  ];

  // Qué pestaña estaba abierta al salir a reproducir. Sobrevive al viaje de
  // ida y vuelta a la home; no debe sobrevivir al cierre de la app.
  const PESTANA_GUARDADA = "kutral_historial_pestana";

  let pestana = $state<Pestana>("seguir");
  let confirmar = $state<FilaHistorial | null>(null);
  let confirmarBtn = $state<HTMLButtonElement | null>(null);

  const IMG = "https://image.tmdb.org/t/p";

  // AniList entrega URLs completas; TMDb, rutas relativas.
  function img(path: string | null | undefined, size: string): string {
    if (!path) return "";
    return path.startsWith("http") ? path : `${IMG}/${size}${path}`;
  }

  function pct(f: FilaHistorial): number {
    const real =
      f.progress_real ??
      (f.runtime_seconds && f.runtime_seconds > 0 ? f.watched_seconds / f.runtime_seconds : 0);
    return Math.max(0, Math.min(100, Math.round(real * 100)));
  }

  function capitulo(f: FilaHistorial): string {
    return f.season < 0 ? "" : `T${f.season} E${f.episode}`;
  }

  function detalle(f: FilaHistorial): string {
    const cap = capitulo(f);
    if (cap && f.episode_title) return `${cap} · ${f.episode_title}`;
    if (cap) return cap;
    if (f.completed === 1) return "Vista";
    if (f.runtime_seconds && f.runtime_seconds > f.watched_seconds) {
      const min = Math.round((f.runtime_seconds - f.watched_seconds) / 60);
      if (min > 0) return `quedan ${min} min`;
    }
    return `${pct(f)}% visto`;
  }

  // --- Agrupado por fecha ---------------------------------------------------
  // Contra el arranque de HOY, no contra "hace 24 h": algo visto anoche a las
  // 23:50 tiene que decir "Ayer", no "Hoy".
  const DIA_MS = 86_400_000;

  function inicioDelDia(ts: number): number {
    const d = new Date(ts);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }

  function grupoDe(ts: number): string {
    const dias = Math.round((inicioDelDia(Date.now()) - inicioDelDia(ts)) / DIA_MS);
    if (dias <= 0) return "Hoy";
    if (dias === 1) return "Ayer";
    if (dias < 7) return "Esta semana";
    if (dias < 30) return "Este mes";
    return new Date(ts)
      .toLocaleDateString("es-ES", { month: "long", year: "numeric" })
      .replace(/^./, (c) => c.toUpperCase());
  }

  function fechaCorta(ts: number): string {
    return new Date(ts).toLocaleDateString("es-ES", { day: "2-digit", month: "short" });
  }

  // `historial.filas` ya viene ordenado por fecha descendente: agrupar en orden
  // preserva ese orden dentro de cada bloque.
  const grupos = $derived.by(() => {
    const out: { titulo: string; filas: FilaHistorial[] }[] = [];
    for (const f of historial.filas) {
      const g = grupoDe(f.last_watched);
      const ultimo = out[out.length - 1];
      if (ultimo && ultimo.titulo === g) ultimo.filas.push(f);
      else out.push({ titulo: g, filas: [f] });
    }
    return out;
  });

  const pendientes = $derived(seguirViendo(60));
  const favoritos = $derived(listaFavoritos());

  // --- Acciones -------------------------------------------------------------

  function reproducir(item: {
    tmdb_id: number;
    media_type: string;
    season?: number;
    episode?: number;
  }) {
    const tipo = item.media_type === "anime" ? "anime" : item.media_type;
    if (tipo !== "movie" && tipo !== "tv" && tipo !== "anime") return;
    const q = new URLSearchParams({ play: String(item.tmdb_id), type: tipo });
    if (item.season != null && item.season >= 0 && item.episode != null) {
      q.set("season", String(item.season));
      q.set("episode", String(item.episode));
    }
    // `from` le dice a la home a dónde volver al cerrar el menú, y la pestaña
    // queda guardada para volver a la misma lista, no a la primera.
    q.set("from", "historial");
    try {
      sessionStorage.setItem(PESTANA_GUARDADA, pestana);
    } catch {
      /* sin sessionStorage (modo privado): se vuelve a la pestaña por defecto */
    }
    goto(`/?${q.toString()}`);
  }

  async function quitar(f: FilaHistorial) {
    confirmar = null;
    await borrarProgreso(f.imdb_id, f.season, f.episode);
    setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 30);
  }

  async function quitarFavorito(f: FilaFavorito) {
    await alternarFavorito({
      clave: f.imdb_id,
      tmdbId: f.tmdb_id,
      mediaType: f.media_type,
      title: f.title,
      posterPath: f.poster_path,
    });
  }

  // Supr sobre una tarjeta: la tarjeta enfocada dice a quién borrar. Sin esto
  // el quitar sería un botón que solo alcanza el mouse.
  function borrarEnfocado() {
    const el = document.activeElement as HTMLElement | null;
    const idx = el?.dataset?.idx;
    if (idx == null) return;
    if (pestana === "favoritos") {
      const f = favoritos[Number(idx)];
      if (f) void quitarFavorito(f);
      return;
    }
    const lista = pestana === "seguir" ? pendientes : historial.filas;
    const f = lista[Number(idx)];
    if (f) confirmar = f;
  }

  function irAPestana(id: Pestana) {
    pestana = id;
    setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 40);
  }

  function cambiarPestana(d: number) {
    const i = PESTANAS.findIndex((p) => p.id === pestana);
    irAPestana(PESTANAS[(i + d + PESTANAS.length) % PESTANAS.length].id);
  }

  function onKey(e: KeyboardEvent) {
    if (confirmar) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        confirmar = null;
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        navegar(e.key === "ArrowLeft" ? "left" : "right", document.querySelector(".modal") ?? document);
      }
      return;
    }
    switch (e.key) {
      case "Escape":
      case "Backspace":
        e.preventDefault();
        goto("/");
        break;
      case "ArrowDown":
        e.preventDefault();
        navegar("down");
        break;
      case "ArrowUp":
        e.preventDefault();
        navegar("up");
        break;
      case "ArrowRight":
        e.preventDefault();
        navegar("right");
        break;
      case "ArrowLeft":
        e.preventDefault();
        navegar("left");
        break;
      case "Delete":
      case "Supr":
        e.preventDefault();
        borrarEnfocado();
        break;
      case "[":
      case "PageUp":
        e.preventDefault();
        cambiarPestana(-1);
        break;
      case "]":
      case "PageDown":
        e.preventDefault();
        cambiarPestana(1);
        break;
    }
  }

  onMount(() => {
    // El layout carga el historial al arrancar; recargamos por si se vio algo
    // en esta sesión desde otra ruta.
    void cargarHistorial();
    setTimeout(() => {
      // Volver de reproducir cae en la misma pestaña de la que salió.
      let guardada: string | null = null;
      try {
        guardada = sessionStorage.getItem(PESTANA_GUARDADA);
        sessionStorage.removeItem(PESTANA_GUARDADA);
      } catch {
        /* tolerado */
      }
      if (guardada === "seguir" || guardada === "vistos" || guardada === "favoritos") {
        pestana = guardada;
      } else if (!seguirViendo(1).length && historial.filas.length) {
        // Si no quedó nada a medias, la pestaña útil es el historial completo.
        pestana = "vistos";
      }
      setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 40);
    }, 60);
  });

  $effect(() => {
    if (confirmar) setTimeout(() => confirmarBtn?.focus(), 30);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="hist">
  <header class="barra" data-section="filtros">
    <h1>Lo que viste</h1>
    <div class="chips">
      {#each PESTANAS as p}
        <button
          data-nav
          class="chip"
          class:active={pestana === p.id}
          onclick={() => irAPestana(p.id)}
        >
          {p.label}
          {#if p.id === "seguir" && pendientes.length}<span class="cuenta">{pendientes.length}</span>{/if}
          {#if p.id === "favoritos" && favoritos.length}<span class="cuenta">{favoritos.length}</span>{/if}
        </button>
      {/each}
    </div>
    <span class="ayuda-teclas">
      <b>Enter</b> reproducir · <b>Supr</b> quitar · <b>Esc</b> volver
    </span>
  </header>

  <div class="galeria" data-section="galeria">
    {#if pestana === "seguir"}
      {#if !pendientes.length}
        <p class="vacio">No dejaste nada a medias. Todo lo que empezaste, lo terminaste.</p>
      {:else}
        <div class="grid">
          {#each pendientes as f, i (f.imdb_id + f.season + f.episode)}
            <button
              data-nav
              data-idx={i}
              class="card"
              onclick={() => reproducir({ ...f, season: f.season, episode: f.episode })}
              title={f.title}
            >
              <span class="poster">
                {#if img(f.poster_path ?? f.still_path, "w342")}
                  <img src={img(f.poster_path ?? f.still_path, "w342")} alt="" loading="lazy" />
                {:else}
                  <span class="sin-poster">{f.title}</span>
                {/if}
                {#if capitulo(f)}<span class="badge-cap">{capitulo(f)}</span>{/if}
                <span class="pct">{pct(f)}%</span>
                <span class="progreso"><span class="progreso-fill" style:width="{Math.max(3, pct(f))}%"></span></span>
              </span>
              <span class="pie">
                <span class="titulo">{f.title}</span>
                <span class="sub">{detalle(f)}</span>
              </span>
            </button>
          {/each}
        </div>
      {/if}
    {:else if pestana === "vistos"}
      {#if !historial.filas.length}
        <p class="vacio">
          Todavía no hay nada registrado. Lo que reproduzcas aparece acá, con la fecha.
        </p>
      {:else}
        {#each grupos as g (g.titulo)}
          <h2 class="grupo">{g.titulo}</h2>
          <div class="grid">
            {#each g.filas as f (f.imdb_id + f.season + f.episode)}
              {@const i = historial.filas.indexOf(f)}
              <button
                data-nav
                data-idx={i}
                class="card"
                class:terminada={f.completed === 1}
                onclick={() => reproducir({ ...f, season: f.season, episode: f.episode })}
                title={f.title}
              >
                <span class="poster">
                  {#if img(f.poster_path ?? f.still_path, "w342")}
                    <img src={img(f.poster_path ?? f.still_path, "w342")} alt="" loading="lazy" />
                  {:else}
                    <span class="sin-poster">{f.title}</span>
                  {/if}
                  {#if capitulo(f)}<span class="badge-cap">{capitulo(f)}</span>{/if}
                  {#if f.completed === 1}
                    <span class="tick">✓</span>
                  {:else}
                    <span class="pct">{pct(f)}%</span>
                    <span class="progreso"><span class="progreso-fill" style:width="{Math.max(3, pct(f))}%"></span></span>
                  {/if}
                </span>
                <span class="pie">
                  <span class="titulo">{f.title}</span>
                  <span class="sub">{fechaCorta(f.last_watched)} · {detalle(f)}</span>
                </span>
              </button>
            {/each}
          </div>
        {/each}
      {/if}
    {:else if !favoritos.length}
      <p class="vacio">
        Sin favoritos todavía. En la ficha de una película o serie, el botón ☆ la guarda acá.
      </p>
    {:else}
      <div class="grid">
        {#each favoritos as f, i (f.imdb_id)}
          <button
            data-nav
            data-idx={i}
            class="card"
            onclick={() => reproducir(f)}
            title={f.title}
          >
            <span class="poster">
              {#if img(f.poster_path, "w342")}
                <img src={img(f.poster_path, "w342")} alt="" loading="lazy" />
              {:else}
                <span class="sin-poster">{f.title}</span>
              {/if}
              <span class="estrella">★</span>
            </span>
            <span class="pie">
              <span class="titulo">{f.title}</span>
              <span class="sub">{f.media_type === "movie" ? "Película" : f.media_type === "anime" ? "Anime" : "Serie"}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if confirmar}
  <div class="modal-fondo">
    <div class="modal" data-section="modal">
      <h3>¿Quitar del historial?</h3>
      <p class="modal-tit">{confirmar.title}{capitulo(confirmar) ? ` · ${capitulo(confirmar)}` : ""}</p>
      <p class="modal-nota">
        Se borra la marca de visto y el minuto guardado. El título sigue en el catálogo.
      </p>
      <div class="modal-btns">
        <button data-nav class="btn-cancelar" onclick={() => (confirmar = null)}>Cancelar</button>
        <button
          data-nav
          class="btn-borrar"
          bind:this={confirmarBtn}
          onclick={() => void quitar(confirmar!)}
        >
          Quitar
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .hist {
    height: 100%;
    display: flex;
    flex-direction: column;
    color: #e6e6ec;
    overflow: hidden;
  }

  /* Barra superior: mismo lenguaje que los filtros del catálogo. */
  .barra {
    display: flex;
    align-items: center;
    gap: 18px;
    flex-wrap: wrap;
    padding: 14px 20px;
    border-bottom: 1px solid #1e1e28;
    background: #0d0d12;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    letter-spacing: 2px;
    text-transform: uppercase;
    color: #f5c518;
    font-weight: 800;
  }
  .chips {
    display: flex;
    gap: 8px;
  }
  .chip {
    background: transparent;
    border: 1px solid #2a2a35;
    color: #9a9aa4;
    border-radius: 999px;
    padding: 7px 16px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.4px;
    cursor: pointer;
    transition: all 0.12s;
  }
  .chip:hover {
    color: #fff;
    border-color: #f5c518;
  }
  .chip:focus,
  .chip:focus-visible {
    outline: none;
    border-color: #f5c518;
    color: #fff;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.25);
  }
  .chip.active,
  .chip.active:focus {
    background: #f5c518;
    border-color: #f5c518;
    color: #0d0d12;
    font-weight: 800;
  }
  .cuenta {
    margin-left: 7px;
    opacity: 0.75;
    font-weight: 700;
  }
  .ayuda-teclas {
    margin-left: auto;
    font-size: 11px;
    color: #6a6a74;
    letter-spacing: 0.3px;
  }
  .ayuda-teclas b {
    color: #9a9aa4;
    font-weight: 700;
  }

  .galeria {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 40px;
  }
  .grupo {
    margin: 22px 20px 0;
    font-size: 11px;
    letter-spacing: 2px;
    text-transform: uppercase;
    color: #6a6a74;
    font-weight: 700;
    border-bottom: 1px solid #1e1e28;
    padding-bottom: 8px;
  }

  /* Mismo grid que el catálogo: la pantalla se siente parte de la misma app. */
  .grid {
    padding: 20px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 20px;
    align-content: start;
  }
  .card {
    position: relative;
    background: #15151c;
    border: 0;
    border-radius: 6px;
    overflow: hidden;
    cursor: pointer;
    padding: 0;
    color: inherit;
    text-align: left;
    display: flex;
    flex-direction: column;
    transition: transform 0.15s;
  }
  .card:hover {
    transform: translateY(-3px);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
  }
  .card:focus,
  .card:focus-visible {
    outline: 4px solid #f5c518;
    outline-offset: 3px;
    transform: translateY(-3px) scale(1.03);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.7), 0 0 0 6px rgba(245, 197, 24, 0.18);
    z-index: 5;
  }
  .poster {
    position: relative;
    display: block;
    aspect-ratio: 2 / 3;
    background: #101018;
  }
  .poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .card.terminada .poster img {
    opacity: 0.5;
  }
  .sin-poster {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 12px;
    text-align: center;
    font-size: 12px;
    color: #55555f;
  }
  .badge-cap {
    position: absolute;
    top: 8px;
    left: 8px;
    background: rgba(13, 13, 18, 0.85);
    color: #fff;
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.8px;
    backdrop-filter: blur(2px);
  }
  .tick {
    position: absolute;
    right: 8px;
    bottom: 8px;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: #2f9e44;
    color: #fff;
    font-size: 15px;
    font-weight: 900;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.65);
  }
  .estrella {
    position: absolute;
    right: 8px;
    bottom: 8px;
    color: #f5c518;
    font-size: 18px;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.8);
  }
  .pct {
    position: absolute;
    right: 8px;
    bottom: 12px;
    background: rgba(13, 13, 18, 0.85);
    color: #f3a951;
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 11px;
    font-weight: 800;
    backdrop-filter: blur(2px);
  }
  .progreso {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 5px;
    background: rgba(0, 0, 0, 0.6);
  }
  .progreso-fill {
    display: block;
    height: 100%;
    background: #f3a951;
  }
  .pie {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 9px 10px 11px;
    min-width: 0;
  }
  .titulo {
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 11px;
    color: #8f8f9a;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .vacio {
    color: #6a6a74;
    font-size: 14px;
    padding: 40px 22px;
  }

  .modal-fondo {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: grid;
    place-items: center;
    z-index: 1000;
  }
  .modal {
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 14px;
    padding: 24px 28px;
    max-width: 430px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.7);
  }
  .modal h3 {
    margin: 0 0 12px;
    font-size: 16px;
    letter-spacing: 0.4px;
  }
  .modal-tit {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 700;
    color: #fff;
  }
  .modal-nota {
    margin: 0;
    font-size: 12px;
    color: #8f8f9a;
  }
  .modal-btns {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    margin-top: 20px;
  }
  .modal-btns button {
    border-radius: 8px;
    padding: 9px 18px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: 1px solid #2a2a35;
    background: transparent;
    color: #d8d8e0;
  }
  .modal-btns button:focus,
  .modal-btns button:focus-visible {
    outline: none;
    border-color: #f5c518;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.25);
  }
  .btn-borrar {
    background: rgba(220, 38, 38, 0.22);
    border-color: #dc2626;
    color: #fff;
  }
  .btn-borrar:focus,
  .btn-borrar:focus-visible {
    border-color: #ef4444;
    box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.3);
  }
</style>
