<script lang="ts">
  // Pantalla de fin: lo que se ve cuando la película o el capítulo termina solo
  // (evento "mpv:fin"). Antes acá había un corte seco al catálogo.
  //
  // Dos cosas, en este orden:
  //   1. Siguiente capítulo (series/anime). SIEMPRE con Enter: nada arranca
  //      solo, encadenar capítulos sin pedirlos es justo lo que molesta de
  //      otros reproductores.
  //   2. "Porque viste X": qué ver después. En películas es lo único que hay.
  //
  // Teclado propio (el page no navega en este modo): ←→ dentro de la fila,
  // ↑↓ entre secciones, Enter elige, Esc/Backspace vuelve al catálogo.

  type Sugerencia = {
    id: number;
    title: string;
    posterUrl: string | null;
    year: string;
  };

  type Siguiente = {
    season: number;
    episode: number;
    nombre: string;
    stillUrl: string | null;
  };

  let {
    title,
    backdrop = null,
    siguiente = null,
    sugerencias = [],
    cargando = false,
    onSiguiente,
    onElegir,
    onClose,
  }: {
    /** Título que acaba de terminar. */
    title: string;
    backdrop?: string | null;
    /** Capítulo que sigue, o null (película / último capítulo). */
    siguiente?: Siguiente | null;
    sugerencias?: Sugerencia[];
    /** Las sugerencias todavía se están buscando. */
    cargando?: boolean;
    onSiguiente: () => void;
    onElegir: (id: number) => void;
    onClose: () => void;
  } = $props();

  type Fila = "siguiente" | "sugerencias" | "salir";
  let focoFila = $state<Fila>("salir");
  let focoSug = $state(0);
  // El capítulo siguiente y las sugerencias llegan por red DESPUÉS de que la
  // pantalla ya está montada. Hasta que el usuario toque algo, el foco sigue
  // solo a lo mejor que haya aparecido; después se respeta donde lo dejó.
  let tocado = $state(false);
  $effect(() => {
    if (tocado) return;
    focoFila = siguiente ? "siguiente" : sugerencias.length ? "sugerencias" : "salir";
  });

  function enfocar(f: Fila, i = focoSug) {
    tocado = true;
    focoFila = f;
    focoSug = i;
  }

  function filasDisponibles(): Fila[] {
    const f: Fila[] = [];
    if (siguiente) f.push("siguiente");
    if (sugerencias.length) f.push("sugerencias");
    f.push("salir");
    return f;
  }

  function moverFila(d: number) {
    const filas = filasDisponibles();
    const i = filas.indexOf(focoFila);
    const prox = filas[Math.min(filas.length - 1, Math.max(0, (i < 0 ? 0 : i) + d))];
    if (prox) enfocar(prox);
    scrollFoco();
  }

  function moverSug(d: number) {
    if (focoFila !== "sugerencias" || !sugerencias.length) return;
    enfocar("sugerencias", Math.min(sugerencias.length - 1, Math.max(0, focoSug + d)));
    scrollFoco();
  }

  function activar() {
    if (focoFila === "siguiente") return onSiguiente();
    if (focoFila === "salir") return onClose();
    const s = sugerencias[focoSug];
    if (s) onElegir(s.id);
  }

  function scrollFoco() {
    queueMicrotask(() => {
      document
        .querySelector<HTMLElement>(".pc-focused")
        ?.scrollIntoView({ block: "nearest", inline: "center" });
    });
  }

  function onKey(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowRight":
        e.preventDefault();
        moverSug(1);
        break;
      case "ArrowLeft":
        e.preventDefault();
        moverSug(-1);
        break;
      case "ArrowDown":
        e.preventDefault();
        moverFila(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        moverFila(-1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        activar();
        break;
      case "Escape":
      case "Backspace":
        e.preventDefault();
        onClose();
        break;
    }
  }

  function epLabel(s: Siguiente): string {
    const n = `T${s.season} E${s.episode}`;
    return s.nombre ? `${n} — ${s.nombre}` : n;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="pc-root">
  {#if backdrop}
    <div class="pc-bg" style:background-image="url({backdrop})"></div>
  {/if}
  <div class="pc-scrim"></div>

  <div class="pc-content">
    <header class="pc-head">
      <span class="pc-kicker">Terminaste</span>
      <h2 class="pc-title">{title}</h2>
    </header>

    {#if siguiente}
      <button
        class="pc-next"
        class:pc-focused={focoFila === "siguiente"}
        onclick={onSiguiente}
        onmouseenter={() => enfocar("siguiente")}
      >
        {#if siguiente.stillUrl}
          <img class="pc-next-img" src={siguiente.stillUrl} alt="" loading="lazy" />
        {:else}
          <div class="pc-next-img pc-next-sinimg">▶</div>
        {/if}
        <span class="pc-next-txt">
          <span class="pc-next-label">Siguiente capítulo</span>
          <span class="pc-next-ep">{epLabel(siguiente)}</span>
        </span>
      </button>
    {/if}

    {#if cargando}
      <p class="pc-cargando">Buscando qué ver después…</p>
    {:else if sugerencias.length}
      <section class="pc-sec">
        <h3 class="pc-sec-h">Porque viste {title}</h3>
        <div class="pc-fila">
          {#each sugerencias as s, i (s.id)}
            <button
              class="pc-card"
              class:pc-focused={focoFila === "sugerencias" && focoSug === i}
              onclick={() => onElegir(s.id)}
              onmouseenter={() => enfocar("sugerencias", i)}
            >
              {#if s.posterUrl}
                <img class="pc-poster" src={s.posterUrl} alt="" loading="lazy" />
              {:else}
                <div class="pc-poster pc-poster-sin">{s.title.slice(0, 1)}</div>
              {/if}
              <span class="pc-card-t">{s.title}</span>
              {#if s.year}<span class="pc-card-y">{s.year}</span>{/if}
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <button
      class="pc-salir"
      class:pc-focused={focoFila === "salir"}
      onclick={onClose}
      onmouseenter={() => enfocar("salir")}
    >
      Volver al catálogo
    </button>
    <span class="pc-hint">Enter elige · Esc vuelve al catálogo</span>
  </div>
</div>

<style>
  .pc-root {
    position: fixed;
    inset: 0;
    z-index: 50;
    overflow: hidden;
    background: #0d0d12;
    color: #e6e6ec;
  }
  .pc-bg {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(14px) saturate(1.1);
    transform: scale(1.06);
  }
  .pc-scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, rgba(8, 8, 12, 0.95) 0%, rgba(8, 8, 12, 0.7) 55%, rgba(8, 8, 12, 0.9) 100%),
      linear-gradient(0deg, rgba(8, 8, 12, 0.92) 0%, rgba(8, 8, 12, 0.3) 60%);
  }
  .pc-content {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 22px;
    padding: 40px 48px 32px;
    overflow-y: auto;
  }
  .pc-head {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .pc-kicker {
    color: #b0b0ba;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .pc-title {
    margin: 0;
    font-size: 30px;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
  }

  .pc-next {
    display: flex;
    align-items: center;
    gap: 18px;
    width: min(720px, 100%);
    padding: 12px 18px 12px 12px;
    border: 2px solid transparent;
    border-radius: 14px;
    background: rgba(20, 20, 28, 0.7);
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.12s, background 0.12s, transform 0.12s;
  }
  .pc-next.pc-focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    transform: translateY(-2px);
  }
  .pc-next-img {
    width: 176px;
    height: 99px;
    object-fit: cover;
    border-radius: 9px;
    background: #1b1b24;
    flex: none;
  }
  .pc-next-sinimg {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 30px;
    color: #f3a951;
  }
  .pc-next-txt {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .pc-next-label {
    color: #f5c518;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .pc-next-ep {
    font-size: 19px;
    font-weight: 600;
    color: #fff;
  }

  .pc-cargando {
    margin: 0;
    color: #9a9aa4;
    font-size: 14px;
  }
  .pc-sec {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
  }
  .pc-sec-h {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: #d8c4a8;
  }
  .pc-fila {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    padding-bottom: 6px;
  }
  .pc-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 150px;
    flex: none;
    padding: 6px;
    border: 2px solid transparent;
    border-radius: 12px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.12s, background 0.12s, transform 0.12s;
  }
  .pc-card.pc-focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.14);
    transform: translateY(-3px);
  }
  .pc-poster {
    width: 100%;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 8px;
    background: #1b1b24;
  }
  .pc-poster-sin {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 34px;
    color: #5a5a68;
  }
  .pc-card-t {
    font-size: 13px;
    color: #e6e6ec;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .pc-card-y {
    font-size: 12px;
    color: #8d8d99;
  }

  .pc-salir {
    align-self: flex-start;
    margin-top: auto;
    padding: 10px 22px;
    border: 2px solid rgba(255, 255, 255, 0.16);
    border-radius: 10px;
    background: rgba(20, 20, 28, 0.7);
    color: #e6e6ec;
    font-size: 14px;
    cursor: pointer;
  }
  .pc-salir.pc-focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
  }
  .pc-hint {
    color: #7d7d88;
    font-size: 12px;
  }
</style>
