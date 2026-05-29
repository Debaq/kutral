<script lang="ts">
  // Vera and Chill — flujo B6.
  // Cinco pantallas: entrada → contexto → intencion → CALIFICAR → RANKING.
  //
  // CALIFICAR: 8 cartas, una por vez. ←→ ajusta -5..+5. ↑ toggle vista
  //   (juicio dual). ↓ saltar (interes=-1). Enter pasa a la siguiente.
  //   Backspace salta al ranking ya.
  // RANKING: lista navegable, #1 destacada. ↑↓ navega, Enter Descubre,
  //   Backspace empieza otra ronda (pool fresco, mismo contexto+intent).
  //
  // SIN cache de pool: cada entrada = pool fresco vía page+sort_by random.
  // Persistencia entre sesiones: vera_reacciones_v2 alimenta perfil histórico.

  import { fade, fly } from "svelte/transition";
  import { tick, onMount } from "svelte";
  import { goto } from "$app/navigation";

  import type {
    Pelicula,
    Contexto,
    EstadoVera,
    Reaccion,
    RankingPeli,
    PerfilHistorico,
  } from "$lib/vera/tipos";
  import { recomendar } from "$lib/vera/motor";
  import {
    cargarCatalogoPorIntent,
    posterUrl,
    backdropUrl,
    imageUrl,
    TmdbNoKeyError,
    type FiltrosDiscover,
  } from "$lib/vera/tmdb";
  import { INTENCIONES, filtrosParaIntent } from "$lib/vera/intenciones";
  import { setReaccion, getReaccionesMap } from "$lib/vera/ratings";
  import { getPerfilHistorico } from "$lib/vera/historial";
  import { ayuda, type AtajoLinea } from "$lib/atajos/store.svelte";

  // --- Pantallas ---
  type Pantalla =
    | "entrada"
    | "contexto"
    | "intencion"
    | "calificar"
    | "ranking";
  let pantalla = $state<Pantalla>("entrada");

  // --- Estado global Vera ---
  let estado = $state<EstadoVera>({
    contexto: null,
    intencion: null,
    reacciones: [],
    horaActual: new Date().getHours(),
  });

  // --- Catálogo (pool) ---
  let catalogo = $state<Pelicula[]>([]);
  let cargandoPool = $state(false);
  let errorPool = $state<string | null>(null);
  let errorPoolEsKey = $state(false);

  // --- Contexto ---
  // Regla de marca: "ninos" SIEMPRE implica adulto presente.
  const contextos: {
    id: Contexto;
    label: string;
    icono: string;
    nota?: string;
  }[] = [
    { id: "solo", label: "Solo", icono: "🧍" },
    { id: "pareja", label: "En pareja", icono: "👥" },
    { id: "amigos", label: "Con amigos o familia", icono: "👨‍👩‍👦‍👦" },
    {
      id: "ninos",
      label: "Con niños",
      icono: "👶",
      nota: "con un adulto cerca",
    },
  ];
  let idxContexto = $state(0);

  // --- Intencion ---
  let idxIntencion = $state(0);
  // Filtros fijados al confirmar el intent. NO derivar: el sort_by + page
  // random deben quedar estables durante la ronda (variar en cada llamada
  // los reshufflería). "Otra ronda" sí los re-calcula para traer pool fresco.
  let filtrosIntent = $state<FiltrosDiscover | null>(null);

  // --- Calificar (B6, 8 cartas 1x1) ---
  // CARTAS_OBJETIVO: cuántas pelis del pool se calificán. Si el pool tiene
  // menos por accidente (TMDb devolvió poco), calificamos lo que haya.
  const CARTAS_OBJETIVO = 8;
  let pelisACalificar = $state<Pelicula[]>([]);
  let idxCalificar = $state(0);
  // Flag de "saltada" para mostrar feedback chico ("Saltada · -1 automático").
  let ultimaSaltada = $state(false);

  // --- Ranking (B6, lista navegable, ex "lista" de B5) ---
  let ranking = $state<RankingPeli[]>([]);
  let idxActiva = $state(0);
  // Cache del perfil histórico: cargado UNA vez al entrar a calificar y
  // reusado en cada rerankear() para evitar I/O a SQLite por flecha.
  let historicoCache: PerfilHistorico | null = null;
  // State local de reacciones del catálogo actual. Mirror reactivo de
  // localStorage para que el template re-renderice cuando cambia.
  let reaccionesPorPeli = $state<
    Map<string, { interes: number; juicio: number | null }>
  >(new Map());

  // --- Refs para foco ---
  let refEntrada = $state<HTMLButtonElement | null>(null);
  let refContexto = $state<HTMLButtonElement | null>(null);
  let refIntencion = $state<HTMLButtonElement | null>(null);
  let refCalificar = $state<HTMLElement | null>(null);
  let refRanking = $state<HTMLElement | null>(null);
  let refDescubrir = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    void pantalla;
    void idxContexto;
    void idxIntencion;
    tick().then(() => {
      if (pantalla === "entrada") refEntrada?.focus();
      else if (pantalla === "contexto") refContexto?.focus();
      else if (pantalla === "intencion") refIntencion?.focus();
      else if (pantalla === "calificar") refCalificar?.focus();
      else if (pantalla === "ranking")
        (refDescubrir ?? refRanking)?.focus();
    });
  });

  // --- Helpers ---
  const clamp = (lo: number, hi: number, v: number) =>
    Math.max(lo, Math.min(hi, v));

  function obtenerReaccion(id: string): {
    interes: number;
    juicio: number | null;
  } {
    return reaccionesPorPeli.get(id) ?? { interes: 0, juicio: null };
  }

  function actualizarReaccionLocal(
    id: string,
    rec: { interes: number; juicio: number | null },
  ): void {
    const m = new Map(reaccionesPorPeli);
    m.set(id, rec);
    reaccionesPorPeli = m;
  }

  // Persiste reacción local + en localStorage. Single source of mutation.
  function guardarReaccion(
    p: Pelicula,
    rec: { interes: number; juicio: number | null },
  ): void {
    setReaccion(p.id, { tmdb: p.rating, ...rec });
    actualizarReaccionLocal(p.id, rec);
  }

  // Convierte reaccionesPorPeli + catalogo en estado.reacciones para el motor.
  function reaccionesDesdeCatalogo(): Reaccion[] {
    const out: Reaccion[] = [];
    for (const p of catalogo) {
      const r = reaccionesPorPeli.get(p.id);
      if (!r) continue;
      if (r.interes === 0 && r.juicio === null) continue;
      out.push({ pelicula: p, interes: r.interes, juicio: r.juicio });
    }
    return out;
  }

  // --- Navegación ---

  function elegirContextoActual(): void {
    estado.contexto = contextos[idxContexto].id;
    pantalla = "intencion";
  }

  // Confirma el intent y dispara la carga del pool + entrada a calificar.
  async function elegirIntencionActual(): Promise<void> {
    const id = INTENCIONES[idxIntencion].id;
    estado.intencion = id;
    filtrosIntent = filtrosParaIntent(id);

    cargandoPool = true;
    errorPool = null;
    errorPoolEsKey = false;
    try {
      const pool = await cargarCatalogoPorIntent(filtrosIntent);
      catalogo = pool;
      await entrarACalificar();
    } catch (e: unknown) {
      if (e instanceof TmdbNoKeyError) {
        errorPoolEsKey = true;
        errorPool = e.message;
      } else {
        errorPool = e instanceof Error ? e.message : String(e);
      }
    } finally {
      cargandoPool = false;
    }
  }

  // Prepara la ronda de calificación: carga histórico, hidrata reacciones,
  // toma las primeras N pelis del pool.
  async function entrarACalificar(): Promise<void> {
    historicoCache = await getPerfilHistorico();

    // Hidratar reacciones locales con lo persistido.
    const mapaPersistido = getReaccionesMap();
    const nuevoMap = new Map<
      string,
      { interes: number; juicio: number | null }
    >();
    for (const p of catalogo) {
      const rec = mapaPersistido[p.id];
      if (rec)
        nuevoMap.set(p.id, { interes: rec.interes, juicio: rec.juicio });
    }
    reaccionesPorPeli = nuevoMap;

    // Tomamos las primeras CARTAS_OBJETIVO pelis del pool (ya están en
    // orden de armado: discover → reco → diversidad → fallback).
    pelisACalificar = catalogo.slice(0, CARTAS_OBJETIVO);
    idxCalificar = 0;
    ultimaSaltada = false;

    precargarImagenes(catalogo);
    pantalla = "calificar";
  }

  function precargarImagenes(pelis: Pelicula[]): void {
    for (const p of pelis) {
      const url = posterUrl(p, "w500");
      if (url) {
        const img = new Image();
        img.src = url;
      }
      const back = backdropUrl(p, "w780");
      if (back) {
        const img = new Image();
        img.src = back;
      }
    }
  }

  // --- Acciones en pantalla "calificar" ---

  function ajustarCarta(delta: -1 | 1): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    ultimaSaltada = false;
    const actual = obtenerReaccion(p.id);
    let nueva: { interes: number; juicio: number | null };
    if (actual.juicio !== null) {
      nueva = {
        interes: actual.interes,
        juicio: clamp(-5, 5, actual.juicio + delta),
      };
    } else {
      nueva = {
        interes: clamp(-5, 5, actual.interes + delta),
        juicio: null,
      };
    }
    guardarReaccion(p, nueva);
  }

  function toggleVistaCarta(): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    ultimaSaltada = false;
    const actual = obtenerReaccion(p.id);
    guardarReaccion(p, {
      interes: actual.interes,
      juicio: actual.juicio === null ? 0 : null,
    });
  }

  // ↓ Saltar carta: asigna interes=-1 (puntaje bajo automático, no descarta).
  // El silencio cuenta — quien salta enseña al sistema que no le interesó
  // tanto como para opinar. La peli aparece más abajo en el ranking pero
  // no desaparece. Mensaje educativo en pantalla.
  function saltarCarta(): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    const actual = obtenerReaccion(p.id);
    // Solo asignar -1 si todavía no calificó (no pisar si ya puso algo).
    if (actual.interes === 0 && actual.juicio === null) {
      guardarReaccion(p, { interes: -1, juicio: null });
    }
    ultimaSaltada = true;
    avanzarCarta();
  }

  function avanzarCarta(): void {
    if (idxCalificar + 1 < pelisACalificar.length) {
      idxCalificar++;
      ultimaSaltada = false;
    } else {
      // Última carta — pasar a ranking.
      void irAlRanking();
    }
  }

  // --- Ranking ---

  // Pasa de calificar a ranking. Computa el ranking en base a reacciones
  // (de sesión + persistidas) y el histórico cacheado.
  async function irAlRanking(): Promise<void> {
    if (!historicoCache) {
      historicoCache = await getPerfilHistorico();
    }
    estado.reacciones = reaccionesDesdeCatalogo();
    ranking = await recomendar(catalogo, estado, historicoCache);
    idxActiva = 0;
    pantalla = "ranking";
  }

  // Otra ronda: nuevos filtros (page+sort random), nuevo pool, vuelve a calificar.
  // Mantiene contexto + intent (pero re-resuelve los filtros).
  async function otraRonda(): Promise<void> {
    if (estado.intencion === null) {
      pantalla = "intencion";
      return;
    }
    cargandoPool = true;
    errorPool = null;
    errorPoolEsKey = false;
    try {
      // Re-resolver filtros: page + sort_by random nuevos.
      filtrosIntent = filtrosParaIntent(estado.intencion);
      catalogo = await cargarCatalogoPorIntent(filtrosIntent);
      await entrarACalificar();
    } catch (e: unknown) {
      if (e instanceof TmdbNoKeyError) {
        errorPoolEsKey = true;
        errorPool = e.message;
      } else {
        errorPool = e instanceof Error ? e.message : String(e);
      }
      // Caer en intencion para reintentar.
      pantalla = "intencion";
    } finally {
      cargandoPool = false;
    }
  }

  // Handoff al player de la home con la peli activa del ranking.
  // Palabra de marca: "Descubrir", no "Reproducir" ni "Play".
  function descubrir(): void {
    if (ranking.length === 0) return;
    const activa = ranking[idxActiva];
    if (!activa) return;
    goto(`/?play=${activa.pelicula.id}&type=movie`);
  }

  function salirDeVera(): void {
    goto("/");
  }

  // Backspace tiene tres semánticas según pantalla:
  //   - calificar: saltar al ranking ya (con lo calificado).
  //   - ranking: otra ronda (pool fresco).
  //   - otras: paso atrás.
  function backspaceMultiple(): void {
    if (pantalla === "calificar") {
      void irAlRanking();
      return;
    }
    if (pantalla === "ranking") {
      void otraRonda();
      return;
    }
    if (pantalla === "intencion") pantalla = "contexto";
    else if (pantalla === "contexto") pantalla = "entrada";
    else if (pantalla === "entrada") salirDeVera();
  }

  // --- Atajos ---

  function atajosActuales(): AtajoLinea[] {
    if (pantalla === "entrada") {
      return [
        { tecla: "Enter", desc: "Empezar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I-I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "contexto") {
      return [
        { tecla: "← → ↑ ↓", desc: "Mover entre opciones" },
        { tecla: "Enter", desc: "Confirmar contexto" },
        { tecla: "Backspace", desc: "Volver a entrada" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I-I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "intencion") {
      return [
        { tecla: "← →", desc: "Elegir intención" },
        { tecla: "Enter", desc: "Confirmar y buscar" },
        { tecla: "Backspace", desc: "Volver a contexto" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I-I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "calificar") {
      return [
        { tecla: "← →", desc: "Ajustar puntaje (-5..+5)" },
        { tecla: "↑", desc: "Marcar / desmarcar como vista" },
        { tecla: "↓", desc: "Saltar (puntaje -1 automático)" },
        { tecla: "Enter", desc: "Siguiente carta" },
        { tecla: "Backspace", desc: "Ir al ranking ya" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I-I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "ranking") {
      return [
        { tecla: "↑ ↓", desc: "Navegar entre pelis" },
        { tecla: "Enter", desc: "Descubrir esta peli" },
        { tecla: "Backspace", desc: "Otra ronda (pelis nuevas)" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I-I", desc: "Ayuda" },
      ];
    }
    return [];
  }

  $effect(() => {
    void pantalla;
    void cargandoPool;
    void errorPool;
    ayuda.set(pantalla, atajosActuales());
  });

  // --- Handler único de teclado (8 teclas: ← → ↑ ↓ Enter Esc I Backspace) ---
  function onKey(e: KeyboardEvent): void {
    if (ayuda.visible) return;

    const k = e.key;

    if (k === "Escape") {
      e.preventDefault();
      salirDeVera();
      return;
    }

    if (k === "Backspace") {
      e.preventDefault();
      backspaceMultiple();
      return;
    }

    if (pantalla === "entrada") {
      if (k === "Enter") {
        e.preventDefault();
        pantalla = "contexto";
      }
      return;
    }

    if (pantalla === "contexto") {
      if (k === "ArrowRight" || k === "ArrowDown") {
        e.preventDefault();
        idxContexto = (idxContexto + 1) % contextos.length;
      } else if (k === "ArrowLeft" || k === "ArrowUp") {
        e.preventDefault();
        idxContexto =
          (idxContexto - 1 + contextos.length) % contextos.length;
      } else if (k === "Enter") {
        e.preventDefault();
        elegirContextoActual();
      }
      return;
    }

    if (pantalla === "intencion") {
      if (cargandoPool) return;
      if (errorPool) {
        if (k === "Enter") {
          e.preventDefault();
          void elegirIntencionActual();
        }
        return;
      }
      if (k === "ArrowRight") {
        e.preventDefault();
        idxIntencion = (idxIntencion + 1) % INTENCIONES.length;
      } else if (k === "ArrowLeft") {
        e.preventDefault();
        idxIntencion =
          (idxIntencion - 1 + INTENCIONES.length) % INTENCIONES.length;
      } else if (k === "Enter") {
        e.preventDefault();
        void elegirIntencionActual();
      }
      return;
    }

    if (pantalla === "calificar") {
      if (pelisACalificar.length === 0) return;
      if (k === "ArrowLeft") {
        e.preventDefault();
        ajustarCarta(-1);
      } else if (k === "ArrowRight") {
        e.preventDefault();
        ajustarCarta(1);
      } else if (k === "ArrowUp") {
        e.preventDefault();
        toggleVistaCarta();
      } else if (k === "ArrowDown") {
        e.preventDefault();
        saltarCarta();
      } else if (k === "Enter") {
        e.preventDefault();
        avanzarCarta();
      }
      return;
    }

    if (pantalla === "ranking") {
      if (ranking.length === 0) return;
      if (k === "ArrowUp") {
        e.preventDefault();
        idxActiva = Math.max(0, idxActiva - 1);
      } else if (k === "ArrowDown") {
        e.preventDefault();
        idxActiva = Math.min(ranking.length - 1, idxActiva + 1);
      } else if (k === "Enter") {
        e.preventDefault();
        descubrir();
      }
      return;
    }
  }

  // Drift de fondo desde el pool (vacío hasta que se carga).
  let driftPosters = $derived(catalogo.slice(0, 12));
</script>

<svelte:window onkeydown={onKey} />

<!-- Fondo cinematográfico (drift desde catálogo cuando existe) -->
<div class="fondo" aria-hidden="true">
  {#each driftPosters as p, i (p.id)}
    <div
      class="drift"
      style="background-color:{p.poster}; background-image:url('{posterUrl(p, 'w342')}'); left:{(i * 9) % 100}%; animation-delay:-{i * 2.3}s;"
    ></div>
  {/each}
  <div class="vignette"></div>
</div>

{#if pantalla === "entrada"}
  <section class="pantalla entrada" in:fade={{ duration: 380 }}>
    <h1 class="marca">
      <span class="marca-vera">Vera</span> <em>and Chill</em>
    </h1>
    <p class="slogan">Tú pones el sillón. Yo propongo qué ver.</p>
    <p class="sub">Calificás 8, te muestro el ranking, elegís.</p>
    <button
      bind:this={refEntrada}
      class="btn-grande naranja"
      onclick={() => (pantalla = "contexto")}
    >
      Empezar
    </button>
    <p class="atajo">Enter para empezar</p>
  </section>
{:else if pantalla === "contexto"}
  <section class="pantalla contexto" in:fly={{ y: 12, duration: 320 }}>
    <h2>¿Quiénes están hoy en el sillón?</h2>
    <div class="opciones" role="radiogroup" aria-label="contexto">
      {#each contextos as c, i}
        <button
          class="opcion"
          class:activa={i === idxContexto}
          aria-checked={i === idxContexto}
          role="radio"
          tabindex={i === idxContexto ? 0 : -1}
          bind:this={refContexto}
          onclick={() => {
            idxContexto = i;
            elegirContextoActual();
          }}
        >
          <span class="icono">{c.icono}</span>
          <span class="label">{c.label}</span>
          {#if c.nota}<span class="nota">{c.nota}</span>{/if}
        </button>
      {/each}
    </div>
    <p class="atajo">
      <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar ·
      <kbd>Backspace</kbd> volver
    </p>
  </section>
{:else if pantalla === "intencion"}
  <section class="pantalla intencion" in:fly={{ y: 12, duration: 320 }}>
    <h2>¿Qué buscas hoy?</h2>
    <div class="opciones" role="radiogroup" aria-label="intencion">
      {#each INTENCIONES as o, i}
        <button
          class="opcion"
          class:activa={i === idxIntencion}
          aria-checked={i === idxIntencion}
          role="radio"
          tabindex={i === idxIntencion ? 0 : -1}
          bind:this={refIntencion}
          disabled={cargandoPool}
          onclick={() => {
            idxIntencion = i;
            void elegirIntencionActual();
          }}
        >
          <span class="icono">{o.icono}</span>
          <span class="label">{o.label}</span>
          <span class="nota">{o.desc}</span>
        </button>
      {/each}
    </div>

    {#if cargandoPool}
      <div class="overlay-cargando" in:fade={{ duration: 200 }}>
        <p>Buscando lo que pediste…</p>
      </div>
    {:else if errorPool}
      <div class="estado error">
        <p>⚠ {errorPool}</p>
        {#if errorPoolEsKey}
          <p class="error-sub">
            Configura la API key TMDb desde la pantalla principal de Kütral
            o desde <code>/vera/catalog</code>. Después <kbd>Enter</kbd> para
            reintentar.
          </p>
        {:else}
          <p class="error-sub"><kbd>Enter</kbd> para reintentar.</p>
        {/if}
      </div>
    {:else}
      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar ·
        <kbd>Backspace</kbd> volver
      </p>
    {/if}
  </section>
{:else if pantalla === "calificar"}
  <section
    class="pantalla calificar"
    bind:this={refCalificar}
    tabindex="-1"
  >
    {#if pelisACalificar.length === 0}
      <p class="vacio">No hay películas para calificar.</p>
    {:else}
      {@const carta = pelisACalificar[idxCalificar]}
      {@const reac = obtenerReaccion(carta.id)}
      {@const visto = reac.juicio !== null}
      {@const valor = visto ? (reac.juicio ?? 0) : reac.interes}

      <p class="contador">
        Carta {idxCalificar + 1} / {pelisACalificar.length}
      </p>

      {#key carta.id}
        <div class="ficha-carta" in:fade={{ duration: 240 }}>
          <div
            class="poster-carta"
            style="background-color:{carta.poster}; background-image:url('{posterUrl(carta, 'w500')}')"
          ></div>
          <div class="info-carta">
            <h2 class="titulo">
              {carta.titulo}
              {#if visto}<span class="badge-visto" title="Ya vista">✓</span>{/if}
            </h2>
            <p class="meta">
              {carta.anio} · {carta.director}
              {#if carta.runtime} · {carta.runtime} min{/if}
              {#if carta.rating}
                <span class="tmdb-rating">★ {carta.rating.toFixed(1)} TMDb</span>
              {/if}
            </p>
            <p class="generos">{carta.generos.join(" · ")}</p>
            <p class="descripcion">{carta.descripcion}</p>
            {#if carta.actores.length}
              <p class="actores">
                <span class="etiqueta">Con</span>
                {carta.actores.slice(0, 4).join(", ")}
              </p>
            {/if}
            {#if carta.imagenes.length}
              <div class="fotogramas">
                {#each carta.imagenes.slice(0, 3) as img}
                  <div
                    class="fotograma"
                    style="background-image:url('{imageUrl(img, 'w300')}')"
                    aria-hidden="true"
                  ></div>
                {/each}
              </div>
            {/if}
            {#if carta.trivia}
              <p class="trivia">💡 {carta.trivia}</p>
            {/if}

            <div class="control-interes">
              <p class="control-etiqueta">
                {visto ? "Tu juicio" : "Tu interés"}:
                <strong>{valor > 0 ? `+${valor}` : valor}</strong>
              </p>
              <div class="escala">
                {#each [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5] as v}
                  <span
                    class="celda"
                    class:activa={v === valor}
                    class:visto
                    class:negativa={v < 0}
                    class:positiva={v > 0}
                  >{v > 0 ? `+${v}` : v}</span>
                {/each}
              </div>
              {#if ultimaSaltada}
                <p class="saltada-msg">
                  Saltada · −1 automático. Si querés que cuente, calificá.
                </p>
              {/if}
            </div>
          </div>
        </div>
      {/key}

      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> puntaje · <kbd>↑</kbd> ya la vi ·
        <kbd>↓</kbd> saltar · <kbd>Enter</kbd> siguiente ·
        <kbd>Backspace</kbd> ir al ranking
      </p>
    {/if}
  </section>
{:else if pantalla === "ranking"}
  <section class="pantalla ranking" bind:this={refRanking} tabindex="-1">
    {#if ranking.length === 0}
      <p class="vacio">No hay películas para ofrecer.</p>
    {:else}
      {@const activa = ranking[idxActiva]}
      {@const reac = obtenerReaccion(activa.pelicula.id)}
      {@const visto = reac.juicio !== null}

      <!-- Backdrop -->
      {#if activa.pelicula.backdropPath}
        {#key activa.pelicula.id}
          <div
            class="backdrop-ranking"
            style="background-image:url('{backdropUrl(activa.pelicula, 'w1280')}')"
            aria-hidden="true"
          ></div>
        {/key}
      {/if}

      <!-- #1 destacada -->
      {#key activa.pelicula.id}
        <div class="ficha-activa" in:fade={{ duration: 240 }}>
          <div
            class="poster-activa"
            style="background-color:{activa.pelicula.poster}; background-image:url('{posterUrl(activa.pelicula, 'w500')}')"
          ></div>
          <div class="info-activa">
            <h2 class="titulo">
              <span class="rango">#{idxActiva + 1}</span>
              {activa.pelicula.titulo}
              {#if visto}<span class="badge-visto" title="Ya vista">✓</span>{/if}
            </h2>
            <p class="meta">
              {activa.pelicula.anio} · {activa.pelicula.director}
              {#if activa.pelicula.runtime} · {activa.pelicula.runtime} min{/if}
              {#if activa.pelicula.rating}
                <span class="tmdb-rating">★ {activa.pelicula.rating.toFixed(1)} TMDb</span>
              {/if}
            </p>
            <p class="generos">{activa.pelicula.generos.join(" · ")}</p>
            <p class="descripcion">{activa.pelicula.descripcion}</p>
            {#if activa.pelicula.actores.length}
              <p class="actores">
                <span class="etiqueta">Con</span>
                {activa.pelicula.actores.slice(0, 4).join(", ")}
              </p>
            {/if}
            {#if activa.pelicula.imagenes.length}
              <div class="fotogramas">
                {#each activa.pelicula.imagenes.slice(0, 3) as img}
                  <div
                    class="fotograma"
                    style="background-image:url('{imageUrl(img, 'w300')}')"
                    aria-hidden="true"
                  ></div>
                {/each}
              </div>
            {/if}
            {#if activa.pelicula.trivia}
              <p class="trivia">💡 {activa.pelicula.trivia}</p>
            {/if}
            <button
              bind:this={refDescubrir}
              class="btn-grande naranja"
              onclick={descubrir}
            >
              ▶ Descubrir
            </button>
          </div>
        </div>
      {/key}

      <!-- Resto del ranking -->
      {#if ranking.length > 1}
        <div class="resto">
          <p class="resto-titulo">El resto del ranking</p>
          <ul class="resto-lista">
            {#each ranking as r, i (r.pelicula.id)}
              {#if i !== idxActiva}
                {@const recR = obtenerReaccion(r.pelicula.id)}
                <li class:visto={recR.juicio !== null}>
                  <span class="posicion">#{i + 1}</span>
                  <span class="resto-titulo-peli">{r.pelicula.titulo}</span>
                  <span class="resto-anio">{r.pelicula.anio}</span>
                  <span class="resto-rating">★ {r.pelicula.rating.toFixed(1)}</span>
                  {#if recR.juicio !== null}<span class="resto-badge">✓</span>{/if}
                </li>
              {/if}
            {/each}
          </ul>
        </div>
      {/if}

      <p class="atajo ranking-atajo">
        <kbd>↑</kbd> <kbd>↓</kbd> navegar · <kbd>Enter</kbd> descubrir ·
        <kbd>Backspace</kbd> otra ronda
      </p>
    {/if}
  </section>
{/if}

<style>
  :global(body) {
    margin: 0;
    background: #0a0608;
    color: #fff8e7;
    font-family:
      "Inter",
      system-ui,
      -apple-system,
      sans-serif;
    overflow-x: hidden;
  }

  :global(button:focus-visible),
  :global([role="radio"]:focus-visible) {
    outline: 3px solid #ffae5c;
    outline-offset: 4px;
  }
  :global(.ranking:focus-visible),
  :global(.calificar:focus-visible) {
    outline: none;
  }

  /* --- Fondo drift --- */
  .fondo {
    position: fixed;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
  }
  .drift {
    position: absolute;
    width: 160px;
    height: 240px;
    border-radius: 12px;
    top: -40px;
    opacity: 0.16;
    filter: blur(3px);
    background-size: cover;
    background-position: center;
    animation: drift 38s linear infinite;
  }
  .vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(
      ellipse at center,
      transparent 30%,
      rgba(10, 6, 8, 0.92) 90%
    );
  }
  @keyframes drift {
    0% { transform: translateY(-20%); }
    100% { transform: translateY(120vh); }
  }

  .pantalla {
    position: relative;
    z-index: 1;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 36px 20px;
    text-align: center;
  }
  .pantalla:focus { outline: none; }

  /* --- Entrada (marca arcoíris) --- */
  .marca {
    font-size: 64px;
    margin: 0 0 8px;
    letter-spacing: -1px;
    line-height: 1.1;
  }
  .marca-vera {
    background: linear-gradient(
      90deg,
      #e40303,
      #ff8c00,
      #ffed00,
      #008026,
      #004dff,
      #750787
    );
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
    font-weight: 700;
  }
  .marca em {
    color: #ffae5c;
    -webkit-text-fill-color: #ffae5c;
    font-style: italic;
    font-weight: 300;
    text-shadow: 0 0 24px rgba(255, 140, 60, 0.4);
  }
  .slogan {
    color: #ffd6a8;
    margin: 0 0 4px;
    font-size: 22px;
    font-style: italic;
    text-shadow: 0 0 16px rgba(255, 140, 60, 0.3);
  }
  .sub {
    color: #d9c0a4;
    margin: 0 0 36px;
    font-size: 16px;
  }

  /* --- Botones grandes --- */
  .btn-grande {
    font-size: 22px;
    padding: 16px 36px;
    border-radius: 999px;
    border: none;
    cursor: pointer;
    font-weight: 600;
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .btn-grande.naranja {
    background: linear-gradient(135deg, #ff7a2e, #ff4a1c);
    color: #1a0a04;
    box-shadow:
      0 8px 24px rgba(255, 100, 40, 0.45),
      0 0 32px rgba(255, 140, 60, 0.35);
  }
  .btn-grande.naranja:hover,
  .btn-grande.naranja:focus-visible {
    transform: translateY(-2px);
    box-shadow:
      0 12px 32px rgba(255, 100, 40, 0.55),
      0 0 48px rgba(255, 140, 60, 0.5);
  }

  /* --- Contexto + Intención --- */
  .contexto h2,
  .intencion h2 {
    font-size: 32px;
    margin: 0 0 32px;
    color: #ffe6c8;
  }
  .opciones {
    display: flex;
    gap: 18px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .opcion {
    background: rgba(50, 22, 12, 0.65);
    border: 1.5px solid rgba(255, 140, 60, 0.35);
    color: #ffe6c8;
    padding: 22px 22px;
    border-radius: 18px;
    font-size: 18px;
    min-width: 150px;
    max-width: 180px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    transition: all 0.18s;
    position: relative;
  }
  .opcion .label {
    font-size: 19px;
    text-align: center;
    line-height: 1.2;
  }
  .opcion .nota {
    font-size: 11px;
    color: #ffae5c;
    font-style: italic;
    opacity: 0.85;
    margin-top: 2px;
  }
  .opcion.activa {
    border-color: #ff7a2e;
    background: rgba(80, 32, 14, 0.95);
    transform: translateY(-3px);
    box-shadow: 0 0 32px rgba(255, 140, 60, 0.35);
  }
  .opcion:disabled {
    opacity: 0.5;
    cursor: wait;
    transform: none;
  }
  .icono { font-size: 38px; }

  .overlay-cargando {
    margin-top: 24px;
    color: #ffd6a8;
    font-style: italic;
  }
  .overlay-cargando::after {
    content: "";
    display: inline-block;
    width: 10px;
    height: 10px;
    margin-left: 10px;
    border: 2px solid #ffae5c;
    border-radius: 50%;
    border-right-color: transparent;
    animation: vera-spin 0.8s linear infinite;
    vertical-align: middle;
  }
  @keyframes vera-spin {
    to { transform: rotate(360deg); }
  }

  .estado {
    margin-top: 16px;
    color: #d9c0a4;
    font-size: 17px;
  }
  .estado.error {
    color: #ffae5c;
    max-width: 520px;
  }
  .error-sub {
    color: #d9c0a4;
    font-size: 14px;
    margin-top: 8px;
  }
  .error-sub code {
    background: rgba(255, 140, 60, 0.18);
    padding: 1px 6px;
    border-radius: 4px;
  }

  /* --- Calificar (B6): 1 carta a la vez con ficha completa --- */
  .calificar,
  .ranking {
    justify-content: flex-start;
    padding: 24px 20px 60px;
  }
  .vacio {
    color: #ffd6a8;
    font-size: 18px;
    margin-top: 60px;
  }
  .contador {
    color: #d9c0a4;
    font-size: 13px;
    margin: 0 0 14px;
    letter-spacing: 1px;
  }
  .ficha-carta,
  .ficha-activa {
    display: flex;
    gap: 28px;
    max-width: 900px;
    width: 100%;
    margin: 0 0 16px;
    text-align: left;
    position: relative;
    z-index: 2;
  }
  .poster-carta,
  .poster-activa {
    width: 240px;
    min-width: 240px;
    height: 360px;
    border-radius: 16px;
    background-size: cover;
    background-position: center;
    box-shadow:
      0 24px 56px rgba(0, 0, 0, 0.7),
      0 0 64px rgba(255, 100, 40, 0.25);
  }
  .info-carta,
  .info-activa {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: #ffe6c8;
  }
  .titulo {
    font-size: 28px;
    margin: 0;
    color: #ffe6c8;
    line-height: 1.1;
  }
  .rango {
    color: #ff7a2e;
    font-weight: 700;
    margin-right: 8px;
  }
  .badge-visto {
    color: #8ab87a;
    margin-left: 8px;
    font-size: 22px;
  }
  .meta {
    font-size: 14px;
    color: #d9c0a4;
    margin: 0;
    letter-spacing: 0.3px;
  }
  .tmdb-rating {
    background: rgba(255, 170, 80, 0.18);
    border: 1px solid rgba(255, 170, 80, 0.55);
    color: #ffd66a;
    padding: 2px 10px;
    border-radius: 999px;
    font-weight: 600;
    margin-left: 8px;
    font-size: 13px;
  }
  .generos {
    font-size: 12px;
    color: #ffae5c;
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.8px;
  }
  .descripcion {
    font-size: 14px;
    line-height: 1.5;
    color: #e6d4bd;
    margin: 4px 0 0;
    max-height: 7em;
    overflow: hidden;
  }
  .actores {
    font-size: 13px;
    color: #d9c0a4;
    margin: 0;
  }
  .etiqueta {
    color: #998878;
    margin-right: 4px;
  }
  .fotogramas {
    display: flex;
    gap: 8px;
    margin: 4px 0 0;
  }
  .fotograma {
    flex: 1 1 0;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    background-size: cover;
    background-position: center;
    background-color: #1f1410;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }
  .trivia {
    font-size: 13px;
    color: #ffd6a8;
    background: rgba(255, 140, 60, 0.12);
    padding: 8px 12px;
    border-radius: 10px;
    border-left: 3px solid #ff7a2e;
    margin: 6px 0 0;
    font-style: italic;
  }

  /* --- Control de interés (calificar) --- */
  .control-interes {
    margin-top: 14px;
    padding: 12px 16px;
    background: rgba(50, 22, 12, 0.6);
    border-radius: 12px;
    border: 1px solid rgba(255, 140, 60, 0.3);
  }
  .control-etiqueta {
    font-size: 13px;
    color: #d9c0a4;
    margin: 0 0 8px;
    letter-spacing: 0.3px;
  }
  .control-etiqueta strong {
    color: #ffe6c8;
    font-size: 18px;
    margin-left: 4px;
  }
  .escala {
    display: flex;
    gap: 4px;
    justify-content: space-between;
  }
  .celda {
    flex: 1;
    text-align: center;
    padding: 6px 0;
    border-radius: 6px;
    font-size: 12px;
    color: #998878;
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid transparent;
    font-variant-numeric: tabular-nums;
    transition: all 0.12s;
  }
  .celda.negativa { color: rgba(255, 120, 120, 0.55); }
  .celda.positiva { color: rgba(180, 220, 140, 0.65); }
  .celda.activa {
    background: rgba(255, 140, 60, 0.25);
    border-color: #ff7a2e;
    color: #ffe6c8;
    font-weight: 700;
    transform: translateY(-1px);
  }
  .celda.activa.visto {
    background: rgba(138, 184, 122, 0.25);
    border-color: #8ab87a;
  }
  .saltada-msg {
    margin: 8px 0 0;
    color: #ffae5c;
    font-size: 12px;
    font-style: italic;
  }

  /* --- Ranking (B6) --- */
  .backdrop-ranking {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center top;
    opacity: 0.28;
    filter: blur(2px);
    z-index: 0;
    pointer-events: none;
  }
  .backdrop-ranking::after {
    content: "";
    position: absolute;
    inset: 0;
    background: radial-gradient(
      ellipse at center,
      transparent 20%,
      rgba(10, 6, 8, 0.85) 75%
    );
    pointer-events: none;
  }

  .resto {
    max-width: 900px;
    width: 100%;
    margin: 8px 0 16px;
    position: relative;
    z-index: 2;
  }
  .resto-titulo {
    color: #998878;
    font-size: 13px;
    margin: 0 0 8px;
    text-align: left;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }
  .resto-lista {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .resto-lista li {
    display: grid;
    grid-template-columns: 36px 1fr 80px 80px 24px;
    gap: 12px;
    align-items: center;
    padding: 8px 12px;
    border-radius: 8px;
    background: rgba(50, 22, 12, 0.4);
    border: 1px solid rgba(255, 140, 60, 0.1);
    color: #d9c0a4;
    font-size: 14px;
    text-align: left;
  }
  .resto-lista li.visto {
    opacity: 0.65;
  }
  .posicion {
    color: #ffae5c;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .resto-titulo-peli {
    color: #ffe6c8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .resto-anio,
  .resto-rating {
    color: #998878;
    font-size: 12px;
    text-align: right;
  }
  .resto-badge {
    color: #8ab87a;
    font-size: 16px;
    text-align: center;
  }
  .ranking-atajo {
    position: relative;
    z-index: 2;
    margin-top: 16px;
  }

  /* --- Atajos hint --- */
  .atajo {
    color: #998878;
    font-size: 13px;
    margin-top: 18px;
    letter-spacing: 0.5px;
  }
  :global(kbd) {
    background: rgba(255, 140, 60, 0.18);
    border: 1px solid rgba(255, 140, 60, 0.4);
    border-radius: 6px;
    padding: 2px 8px;
    font-family: inherit;
    font-size: 13px;
    color: #ffe6c8;
    margin: 0 2px;
  }
</style>
