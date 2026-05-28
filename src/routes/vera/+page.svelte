<script lang="ts">
  // Vera and Chill — prototipo 100% teclado.
  // Cero ratón: la app se navega entera con teclas físicas.
  // Cada pantalla mueve el foco a su acción primaria al montarse.
  // Catálogo viene de TMDb vía comandos Tauri (ver $lib/vera/tmdb.ts).

  import { fade, fly, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { tick } from "svelte";
  import { goto } from "$app/navigation";

  import {
    CARTAS_MAZO,
    UMBRAL_NO_MATCH,
    DEMO_PAUSA_MS,
    DEMO_PAUSA_MINIMA_MS,
  } from "$lib/vera/config";
  import type {
    Pelicula,
    Contexto,
    Intencion,
    EstadoVera,
    Gesto,
    Recomendaciones,
  } from "$lib/vera/tipos";
  import { recomendar, elegirMazo } from "$lib/vera/motor";
  import {
    comentarioMazo,
    fraseTadan,
    fraseNoConvence,
    fraseCierre,
    fraseNoMatch,
  } from "$lib/vera/frases";
  import {
    cargarCatalogoPorIntent,
    posterUrl,
    backdropUrl,
    TmdbNoKeyError,
    type FiltrosDiscover,
  } from "$lib/vera/tmdb";
  import { INTENCIONES, filtrosParaIntent } from "$lib/vera/intenciones";
  import { setRating } from "$lib/vera/ratings";
  import { ayuda, type AtajoLinea } from "$lib/atajos/store.svelte";
  import Habla from "$lib/habla/Habla.svelte";

  // --- Carga del catálogo ---
  // B3: el catálogo NO se precarga en onMount. Se carga al confirmar
  // un intent en la pantalla "intencion", con los filtros del intent.
  // La entrada queda instantánea.
  let catalogo = $state<Pelicula[]>([]);
  let cargandoPool = $state(false);
  let errorPool = $state<string | null>(null);
  let errorPoolEsKey = $state(false);

  // Precarga las imágenes del pool en el HTTP cache del navegador para que
  // el mazo aparezca sin esperar. Async, no bloqueante.
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

  // Pantallas del flujo.
  type Pantalla =
    | "entrada"
    | "contexto"
    | "intencion"
    | "mazo"
    | "tadan"
    | "noMatch"
    | "reproduciendo"
    | "pausa"
    | "fin";

  let pantalla = $state<Pantalla>("entrada");

  // Estado global Vera.
  let estado = $state<EstadoVera>({
    contexto: null,
    intencion: null,
    reacciones: [],
    vistas: [],
    interesadas: [],
    horaActual: new Date().getHours(),
  });

  // Mazo y carta en pantalla.
  let mazo = $state<Pelicula[]>([]);
  let indiceMazo = $state(0);
  let comentarioVivo = $state<string>("");

  // Recomendaciones del motor.
  let recs = $state<Recomendaciones>({
    segura: null,
    distinta: null,
    sorpresa: null,
  });
  let cartaActual = $state<"segura" | "distinta" | "sorpresa">("segura");

  // Habla state.
  let hablaVisible = $state(false);
  let hablaMomento = $state<"bienvenida" | "pausa" | "fin" | "joya">(
    "bienvenida",
  );
  let hablaJoya = $state(false);
  let pausaTimer: ReturnType<typeof setTimeout> | null = null;
  let pausaInicio = 0;

  // Selección activa de contexto (navegable con flechas).
  // Regla de marca: "ninos" SIEMPRE implica un adulto presente.
  // No existe "niños solos" en Kütral.
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

  // Selección activa de intent (navegable con ←/→).
  let idxIntencion = $state(0);
  // Filtros fijados una sola vez al confirmar el intent. NO recalcular ni
  // derivar — el sort_by de "sorpresa" es random y debe mantenerse estable.
  let filtrosIntent = $state<FiltrosDiscover | null>(null);

  // Drift de fondo: tomamos las 12 primeras pelis ya cargadas.
  let driftPosters = $derived(catalogo.slice(0, 12));

  // Refs para mover el foco.
  let refEntrada = $state<HTMLButtonElement | null>(null);
  let refContexto = $state<HTMLButtonElement | null>(null);
  let refIntencion = $state<HTMLButtonElement | null>(null);
  let refMazo = $state<HTMLElement | null>(null);
  let refTadan = $state<HTMLButtonElement | null>(null);
  let refNoMatch = $state<HTMLButtonElement | null>(null);
  let refPlayer = $state<HTMLButtonElement | null>(null);
  let refFin = $state<HTMLButtonElement | null>(null);

  // Foco al cambiar de pantalla.
  $effect(() => {
    void pantalla;
    void idxContexto;
    void idxIntencion;
    tick().then(() => {
      if (pantalla === "entrada") refEntrada?.focus();
      else if (pantalla === "contexto") refContexto?.focus();
      else if (pantalla === "intencion") refIntencion?.focus();
      else if (pantalla === "mazo") refMazo?.focus();
      else if (pantalla === "tadan") refTadan?.focus();
      else if (pantalla === "noMatch") refNoMatch?.focus();
      else if (pantalla === "reproduciendo" || pantalla === "pausa")
        refPlayer?.focus();
      else if (pantalla === "fin") refFin?.focus();
    });
  });

  // --- Navegación ---

  function elegirContextoActual() {
    const c = contextos[idxContexto];
    estado.contexto = c.id;
    pantalla = "intencion";
  }

  // Handler de selección del intent. CRÍTICO:
  //   1. Fija filtrosIntent UNA sola vez con filtrosParaIntent(id) y lo guarda
  //      en $state local. No usar $derived: el sort_by de "sorpresa" es random
  //      y debe quedar estable para que la cache sea consistente.
  //   2. Carga el pool con esos filtros (B1 + B2 fusión historial).
  //   3. Si OK, arma mazo y navega. Si falla, muestra error dentro de "intencion"
  //      sin perder el flujo. Reusa TmdbNoKeyError del módulo tmdb (no duplica).
  async function elegirIntencionActual() {
    const id = INTENCIONES[idxIntencion].id;
    estado.intencion = id;

    // Resolver filtros una sola vez y persistir.
    const filtros = filtrosParaIntent(id);
    filtrosIntent = filtros;

    cargandoPool = true;
    errorPool = null;
    errorPoolEsKey = false;
    try {
      const pool = await cargarCatalogoPorIntent(filtros);
      catalogo = pool;
      mazo = elegirMazo(pool, estado, CARTAS_MAZO);
      indiceMazo = 0;
      comentarioVivo = "";
      // Precarga en background — no esperar.
      precargarImagenes(pool);
      pantalla = "mazo";
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

  function saltarASorpresa() {
    const i = INTENCIONES.findIndex((o) => o.id === "sorpresa");
    if (i >= 0) idxIntencion = i;
    void elegirIntencionActual();
  }

  // Cuando el usuario marca "ya la vi", entramos en modo rating:
  // se pausa el avance hasta que califique con flechas + Enter (o saltee con I).
  let ratingPendiente = $state<Pelicula | null>(null);
  // Selección actual del rating (1-5). Las flechas la mueven.
  let ratingSel = $state<number>(3);

  function reaccionar(gesto: Gesto) {
    const p = mazo[indiceMazo];
    if (!p) return;
    if (gesto === "vista") {
      ratingPendiente = p;
      ratingSel = 3;
      return;
    }
    completarReaccion(p, gesto, undefined);
  }

  function completarReaccion(
    p: Pelicula,
    gesto: Gesto,
    userRating: number | undefined,
  ) {
    estado.reacciones = [
      ...estado.reacciones,
      { pelicula: p, gesto, userRating: userRating ?? null },
    ];
    if (gesto === "vista") {
      estado.vistas = [...estado.vistas, p.id];
      setRating(p.id, {
        tmdb: p.rating,
        user: userRating ?? null,
        visto: true,
      });
    }
    if (gesto === "interes")
      estado.interesadas = [...estado.interesadas, p.id];

    comentarioVivo = comentarioMazo(p, gesto);

    setTimeout(() => {
      if (indiceMazo + 1 >= mazo.length) {
        cerrarMazo();
      } else {
        indiceMazo++;
        comentarioVivo = "";
      }
    }, 700);
  }

  function aplicarRatingMazo(stars: number | null) {
    const p = ratingPendiente;
    if (!p) return;
    ratingPendiente = null;
    completarReaccion(p, "vista", stars ?? undefined);
  }

  // Rating final (en pantalla fin): el usuario califica la peli que vio.
  // null = aún no decidió. 0 (sentinel) = saltó. 1-5 = puntuó.
  let ratingFinal = $state<number | null>(null);
  let ratingFinalDecidido = $state(false);
  let ratingSelFinal = $state<number>(3);

  function aplicarRatingFinal(stars: number | null) {
    ratingFinal = stars;
    ratingFinalDecidido = true;
    const p = peliMostrada();
    if (p) {
      setRating(p.id, {
        tmdb: p.rating,
        user: stars,
        visto: true,
      });
    }
  }

  function cerrarMazo() {
    recs = recomendar(catalogo, estado);
    const mejor = recs.segura;
    if (!mejor || mejor.score < UMBRAL_NO_MATCH) {
      pantalla = "noMatch";
      return;
    }
    cartaActual = "segura";
    pantalla = "tadan";
  }

  let textoNoConvence = $state("");
  function noMeConvence() {
    textoNoConvence = fraseNoConvence();
    if (cartaActual === "segura" && recs.distinta) {
      cartaActual = "distinta";
      return;
    }
    if (cartaActual !== "sorpresa" && recs.sorpresa) {
      cartaActual = "sorpresa";
      return;
    }
    pantalla = "noMatch";
  }

  function peliMostrada(): Pelicula | null {
    if (cartaActual === "segura") return recs.segura?.pelicula ?? null;
    if (cartaActual === "distinta") return recs.distinta?.pelicula ?? null;
    return recs.sorpresa?.pelicula ?? null;
  }

  function reproducir() {
    pantalla = "reproduciendo";
    hablaMomento = "bienvenida";
    hablaJoya = false;
    hablaVisible = true;
    setTimeout(() => (hablaVisible = false), 5500);
  }

  function pausar() {
    pantalla = "pausa";
    pausaInicio = Date.now();
    pausaTimer = setTimeout(() => {
      const transcurrido = Date.now() - pausaInicio;
      if (transcurrido < DEMO_PAUSA_MINIMA_MS) return;
      hablaMomento = "pausa";
      hablaJoya = false;
      hablaVisible = true;
    }, DEMO_PAUSA_MS);
  }

  function reanudar() {
    if (pausaTimer) {
      clearTimeout(pausaTimer);
      pausaTimer = null;
    }
    hablaVisible = false;
    pantalla = "reproduciendo";
  }

  function terminar() {
    if (pausaTimer) clearTimeout(pausaTimer);
    pantalla = "fin";
    const esJoya = estado.contexto === "solo" && Math.random() < 0.45;
    hablaMomento = "fin";
    hablaJoya = esJoya;
    hablaVisible = true;
  }

  function reiniciar() {
    estado = {
      contexto: null,
      intencion: null,
      reacciones: [],
      vistas: [],
      interesadas: [],
      horaActual: new Date().getHours(),
    };
    catalogo = [];
    mazo = [];
    indiceMazo = 0;
    comentarioVivo = "";
    recs = { segura: null, distinta: null, sorpresa: null };
    cartaActual = "segura";
    textoNoConvence = "";
    hablaVisible = false;
    idxContexto = 0;
    idxIntencion = 0;
    filtrosIntent = null;
    cargandoPool = false;
    errorPool = null;
    errorPoolEsKey = false;
    ratingPendiente = null;
    ratingSel = 3;
    ratingFinal = null;
    ratingFinalDecidido = false;
    ratingSelFinal = 3;
    pantalla = "entrada";
  }

  function salirDeVera() {
    if (pausaTimer) clearTimeout(pausaTimer);
    hablaVisible = false;
    goto("/");
  }

  // La ayuda vive en src/lib/atajos/Ayuda.svelte (overlay global).
  // Acá solo registramos los atajos de cada pantalla en el store compartido.
  function atajosActuales(): AtajoLinea[] {
    if (pantalla === "entrada") {
      return [
        { tecla: "Enter", desc: "Empezar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "contexto") {
      return [
        { tecla: "← → ↑ ↓", desc: "Mover entre opciones" },
        { tecla: "Enter", desc: "Confirmar contexto" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "intencion") {
      return [
        { tecla: "← →", desc: "Elegir intención" },
        { tecla: "Enter", desc: "Confirmar y buscar" },
        { tecla: "↓", desc: "Saltar a sorpresa" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "mazo" && ratingPendiente) {
      return [
        { tecla: "← →", desc: "Mover estrella" },
        { tecla: "Enter", desc: "Confirmar puntuación" },
        { tecla: "↓", desc: "Saltar sin puntuar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "mazo") {
      return [
        { tecla: "←", desc: "Pasar (sin opinar)" },
        { tecla: "↑", desc: "Ya la vi (te pide puntuar)" },
        { tecla: "→", desc: "Me llama" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "tadan") {
      return [
        { tecla: "Enter", desc: "Play" },
        { tecla: "↓", desc: "No me convence (otra carta)" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "noMatch") {
      return [
        { tecla: "Enter", desc: "Volver a empezar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "reproduciendo") {
      return [
        { tecla: "Enter", desc: "Pausar" },
        { tecla: "↓", desc: "Terminar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "pausa") {
      return [
        { tecla: "Enter", desc: "Reanudar" },
        { tecla: "↓", desc: "Terminar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "fin" && !ratingFinalDecidido) {
      return [
        { tecla: "← →", desc: "Mover estrella" },
        { tecla: "Enter", desc: "Confirmar puntuación" },
        { tecla: "↓", desc: "Saltar sin puntuar" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    if (pantalla === "fin") {
      return [
        { tecla: "Enter", desc: "Otra vuelta" },
        { tecla: "Esc", desc: "Salir de Vera" },
        { tecla: "I", desc: "Ayuda" },
      ];
    }
    return [];
  }

  // Sincroniza los atajos visibles en el overlay de ayuda global
  // con la pantalla y el modo actual (rating, etc.).
  $effect(() => {
    void pantalla;
    void ratingPendiente;
    void ratingFinalDecidido;
    void cargandoPool;
    void errorPool;
    ayuda.set(pantalla, atajosActuales());
  });

  // Atajos limitados a 7 teclas: ← → ↑ ↓ Enter Esc I
  // I la maneja el overlay global (Ayuda.svelte). Acá lo ignoramos.
  function onKey(e: KeyboardEvent) {
    // Si la ayuda está abierta, no interpretamos otras teclas.
    if (ayuda.visible) return;

    const k = e.key;

    if (hablaVisible && k === "Escape") {
      hablaVisible = false;
      e.preventDefault();
      return;
    }
    if (k === "Escape") {
      e.preventDefault();
      salirDeVera();
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
      // Mientras carga el pool, las teclas no hacen nada (excepto Esc global).
      if (cargandoPool) return;
      // Si hubo error, Enter reintenta el intent seleccionado.
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
      } else if (k === "ArrowDown") {
        // ↓ siempre = saltar al default (sorpresa). Consistente con mazo/fin.
        e.preventDefault();
        saltarASorpresa();
      }
      return;
    }

    if (pantalla === "mazo") {
      // Modo rating: flechas mueven, Enter confirma, ↓ salta.
      if (ratingPendiente) {
        if (k === "ArrowLeft") {
          e.preventDefault();
          ratingSel = Math.max(1, ratingSel - 1);
        } else if (k === "ArrowRight") {
          e.preventDefault();
          ratingSel = Math.min(5, ratingSel + 1);
        } else if (k === "Enter") {
          e.preventDefault();
          aplicarRatingMazo(ratingSel);
        } else if (k === "ArrowDown") {
          e.preventDefault();
          aplicarRatingMazo(null);
        }
        return;
      }
      if (comentarioVivo) return;
      if (k === "ArrowLeft") {
        e.preventDefault();
        reaccionar("pasar");
      } else if (k === "ArrowUp") {
        e.preventDefault();
        reaccionar("vista");
      } else if (k === "ArrowRight") {
        e.preventDefault();
        reaccionar("interes");
      }
      return;
    }

    if (pantalla === "tadan") {
      if (k === "Enter") {
        e.preventDefault();
        reproducir();
      } else if (k === "ArrowDown") {
        e.preventDefault();
        noMeConvence();
      }
      return;
    }

    if (pantalla === "noMatch") {
      if (k === "Enter") {
        e.preventDefault();
        reiniciar();
      }
      return;
    }

    if (pantalla === "reproduciendo") {
      if (k === "Enter") {
        e.preventDefault();
        pausar();
      } else if (k === "ArrowDown") {
        e.preventDefault();
        terminar();
      }
      return;
    }

    if (pantalla === "pausa") {
      if (k === "Enter") {
        e.preventDefault();
        reanudar();
      } else if (k === "ArrowDown") {
        e.preventDefault();
        terminar();
      }
      return;
    }

    if (pantalla === "fin") {
      if (!ratingFinalDecidido) {
        if (k === "ArrowLeft") {
          e.preventDefault();
          ratingSelFinal = Math.max(1, ratingSelFinal - 1);
        } else if (k === "ArrowRight") {
          e.preventDefault();
          ratingSelFinal = Math.min(5, ratingSelFinal + 1);
        } else if (k === "Enter") {
          e.preventDefault();
          aplicarRatingFinal(ratingSelFinal);
        } else if (k === "ArrowDown") {
          e.preventDefault();
          aplicarRatingFinal(null);
        }
        return;
      }
      if (k === "Enter") {
        e.preventDefault();
        reiniciar();
      }
      return;
    }
  }

  // Memos del tadán.
  let peliTadan = $derived(peliMostrada());
  let textoTadan = $derived(
    peliTadan ? fraseTadan(estado, peliTadan) : "",
  );
  let cierre = $derived(fraseCierre());
</script>

<svelte:window onkeydown={onKey} />

<!-- Fondo cinematográfico común -->
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
      Vera <em>and Chill</em>
    </h1>
    <p class="slogan">Tú pones el sillón. Yo propongo qué ver.</p>
    <p class="sub">Sesenta segundos. Una sola película. Sin vueltas.</p>
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
          {#if c.nota}
            <span class="nota">{c.nota}</span>
          {/if}
        </button>
      {/each}
    </div>
    <p class="atajo">
      <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar
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
          <p class="error-sub">
            <kbd>Enter</kbd> para reintentar.
          </p>
        {/if}
      </div>
    {:else}
      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar ·
        <kbd>↓</kbd> sorpresa
      </p>
    {/if}
  </section>
{:else if pantalla === "mazo"}
  <section class="pantalla mazo" bind:this={refMazo} tabindex="-1">
    <p class="contador">Carta {indiceMazo + 1} / {mazo.length}</p>

    {#if mazo[indiceMazo]}
      {@const p = mazo[indiceMazo]}
      {#key p.id}
        <div class="ficha-mazo" in:fly={{ y: 16, duration: 280 }}>
          <div
            class="carta-poster"
            style="background-color:{p.poster}; background-image:url('{posterUrl(p, 'w342')}')"
          >
            <div class="poster-overlay"></div>
          </div>

          <div class="carta-info">
            <h2 class="info-titulo">{p.titulo}</h2>
            <p class="info-meta">
              {p.anio} · {p.director}
              {#if p.runtime} · {p.runtime} min{/if}
              {#if p.rating}
                <span class="tmdb-rating">★ {p.rating.toFixed(1)} TMDb</span>
              {/if}
            </p>
            <p class="info-generos">{p.generos.join(" · ")}</p>
            <p class="info-descripcion">{p.descripcion || p.gancho}</p>
            {#if p.actores.length}
              <p class="info-actores">
                <span class="etiqueta">Con</span>
                {p.actores.slice(0, 4).join(", ")}
              </p>
            {/if}
            {#if p.trivia}
              <p class="info-trivia">
                <span>💡</span>
                {p.trivia}
              </p>
            {/if}
          </div>
        </div>
      {/key}
    {/if}

    {#if ratingPendiente}
      <div class="prompt-rating" in:fade={{ duration: 220 }}>
        <p class="prompt-titulo">¿Cómo la calificas?</p>
        <div class="estrellas-elegir">
          {#each [1, 2, 3, 4, 5] as n}
            <span class="estrella" class:activa={n <= ratingSel}>★</span>
          {/each}
          <span class="rating-num">{ratingSel}/5</span>
        </div>
        <p class="prompt-ayuda">
          <kbd>←</kbd> <kbd>→</kbd> mover · <kbd>Enter</kbd> confirmar ·
          <kbd>↓</kbd> saltar
        </p>
      </div>
    {:else if comentarioVivo}
      <p class="comentario" in:fade={{ duration: 200 }}>
        Vera: {comentarioVivo}
      </p>
    {:else}
      <p class="ayuda">Reacciona con las flechas</p>
    {/if}

    <div class="atajos-mazo">
      <div class="atajo-box">
        <kbd>←</kbd>
        <span>pasar</span>
      </div>
      <div class="atajo-box">
        <kbd>↑</kbd>
        <span>ya la vi</span>
      </div>
      <div class="atajo-box">
        <kbd>→</kbd>
        <span>me llama</span>
      </div>
    </div>
  </section>
{:else if pantalla === "tadan" && peliTadan}
  <section class="pantalla tadan" in:fade={{ duration: 400 }}>
    {#if peliTadan.backdropPath}
      {#key peliTadan.id}
        <div
          class="backdrop-tadan"
          style="background-image:url('{backdropUrl(peliTadan, 'w1280')}')"
          aria-hidden="true"
        ></div>
      {/key}
    {/if}

    {#if textoNoConvence}
      <p class="vera-line dura" in:fade={{ duration: 200 }}>
        Vera: {textoNoConvence}
      </p>
    {/if}
    <p class="vera-line">{textoTadan}</p>

    {#key peliTadan.id}
      <div class="ficha-tadan" in:scale={{ start: 0.94, duration: 360, easing: cubicOut }}>
        <div
          class="poster-tadan"
          style="background-color:{peliTadan.poster}; background-image:url('{posterUrl(peliTadan, 'w500')}')"
        ></div>
        <div class="info-tadan">
          <h2 class="tadan-titulo">{peliTadan.titulo}</h2>
          <p class="info-meta">
            {peliTadan.anio} · {peliTadan.director}
            {#if peliTadan.runtime} · {peliTadan.runtime} min{/if}
            {#if peliTadan.rating}
              <span class="tmdb-rating">★ {peliTadan.rating.toFixed(1)} TMDb</span>
            {/if}
          </p>
          <p class="info-generos">{peliTadan.generos.join(" · ")}</p>
          <p class="info-descripcion">{peliTadan.descripcion}</p>
          {#if peliTadan.actores.length}
            <p class="info-actores">
              <span class="etiqueta">Con</span>
              {peliTadan.actores.slice(0, 4).join(", ")}
            </p>
          {/if}
          {#if peliTadan.trivia}
            <p class="info-trivia">
              <span>💡</span>
              {peliTadan.trivia}
            </p>
          {/if}
        </div>
      </div>
    {/key}

    <p class="gancho">{peliTadan.gancho}</p>

    <button
      bind:this={refTadan}
      class="btn-grande naranja"
      onclick={reproducir}
    >
      ▶ Play
    </button>

    <p class="cierre">{cierre}</p>

    <p class="atajo">
      <kbd>Enter</kbd> play · <kbd>↓</kbd> no me convence
    </p>
  </section>
{:else if pantalla === "noMatch"}
  <section class="pantalla no-match" in:fade={{ duration: 400 }}>
    <p class="vera-line grande">{fraseNoMatch()}</p>
    <button
      bind:this={refNoMatch}
      class="btn-grande naranja"
      onclick={reiniciar}
    >
      Volver a empezar
    </button>
    <p class="atajo"><kbd>Enter</kbd> para volver a empezar</p>
  </section>
{:else if pantalla === "reproduciendo" && peliTadan}
  <section class="pantalla reproduciendo">
    <div
      class="player"
      style="background-color:{peliTadan.poster}; background-image:url('{backdropUrl(peliTadan, 'w1280') || posterUrl(peliTadan, 'w780')}')"
    >
      <div class="player-overlay"></div>
      <div class="player-titulo">▶ {peliTadan.titulo}</div>
    </div>
    <div class="player-controles">
      <button bind:this={refPlayer} class="btn-control" onclick={pausar}>
        ⏸ Pausar
      </button>
      <button class="btn-control" onclick={terminar}>⏹ Terminar</button>
    </div>
    <p class="atajo">
      <kbd>Enter</kbd> pausar · <kbd>↓</kbd> terminar
    </p>
  </section>
{:else if pantalla === "pausa" && peliTadan}
  <section class="pantalla pausa">
    <div
      class="player en-pausa"
      style="background-color:{peliTadan.poster}; background-image:url('{backdropUrl(peliTadan, 'w1280') || posterUrl(peliTadan, 'w780')}')"
    >
      <div class="player-overlay"></div>
      <div class="player-titulo">⏸ {peliTadan.titulo}</div>
      <div class="badge-pausa">En pausa</div>
    </div>
    <div class="player-controles">
      <button bind:this={refPlayer} class="btn-control" onclick={reanudar}>
        ▶ Reanudar
      </button>
      <button class="btn-control" onclick={terminar}>⏹ Terminar</button>
    </div>
    <p class="atajo">
      <kbd>Enter</kbd> reanudar · <kbd>↓</kbd> terminar
    </p>
  </section>
{:else if pantalla === "fin"}
  <section class="pantalla fin" in:fade={{ duration: 380 }}>
    {#if !ratingFinalDecidido}
      <p class="vera-line grande">¿Cómo te dejó?</p>
      {#if peliTadan}
        <p class="fin-peli">{peliTadan.titulo}</p>
      {/if}
      <div class="estrellas-elegir big">
        {#each [1, 2, 3, 4, 5] as n}
          <span class="estrella" class:activa={n <= ratingSelFinal}>★</span>
        {/each}
        <span class="rating-num">{ratingSelFinal}/5</span>
      </div>
      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> mover · <kbd>Enter</kbd> confirmar ·
        <kbd>↓</kbd> saltar
      </p>
    {:else}
      <p class="vera-line grande">
        {#if ratingFinal != null}
          {ratingFinal} {ratingFinal === 1 ? "estrella" : "estrellas"}. Gracias.
        {:else}
          Listo.
        {/if}
      </p>
      <button
        bind:this={refFin}
        class="btn-grande naranja"
        onclick={reiniciar}
      >
        Otra vuelta
      </button>
      <p class="atajo"><kbd>Enter</kbd> para otra vuelta</p>
    {/if}
  </section>
{/if}

<!--
  Hint global y overlay de ayuda viven en +layout.svelte (Ayuda.svelte).
  Acá solo registramos los atajos por pantalla vía $effect → ayuda.set().
-->

{#if estado.contexto}
  <Habla
    contexto={estado.contexto}
    momento={hablaMomento}
    visible={hablaVisible}
    joya={hablaJoya}
    onCerrar={() => (hablaVisible = false)}
  />
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
  :global(.mazo:focus-visible) {
    outline: none;
  }

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
    opacity: 0.18;
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
    0% {
      transform: translateY(-20%);
    }
    100% {
      transform: translateY(120vh);
    }
  }

  .pantalla {
    position: relative;
    z-index: 1;
    min-height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 36px 20px;
    text-align: center;
  }
  .pantalla:focus {
    outline: none;
  }

  /* --- Entrada --- */
  .marca {
    font-size: 64px;
    margin: 0 0 8px;
    color: #ffd6a8;
    text-shadow:
      0 0 24px rgba(255, 140, 60, 0.55),
      0 0 56px rgba(255, 100, 40, 0.35);
    letter-spacing: -1px;
  }
  .marca em {
    color: #ffae5c;
    font-style: italic;
    font-weight: 300;
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

  /* --- Botones grandes --- */
  .btn-grande {
    font-size: 22px;
    padding: 16px 36px;
    border-radius: 999px;
    border: none;
    cursor: pointer;
    font-weight: 600;
    transition:
      transform 0.15s,
      box-shadow 0.15s;
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

  /* --- Contexto + Intención (mismo layout) --- */
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
  .icono {
    font-size: 38px;
  }
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

  /* --- Atajos --- */
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

  /* --- Mazo: ficha con poster + info al lado --- */
  .contador {
    color: #d9c0a4;
    font-size: 13px;
    margin: 0 0 14px;
    letter-spacing: 1px;
  }
  .ficha-mazo {
    display: flex;
    gap: 24px;
    align-items: stretch;
    max-width: 820px;
    width: 100%;
    margin: 0 auto 16px;
    text-align: left;
  }
  .carta-poster {
    width: 220px;
    min-width: 220px;
    height: 330px;
    border-radius: 14px;
    background-size: cover;
    background-position: center;
    box-shadow:
      0 16px 48px rgba(0, 0, 0, 0.6),
      0 0 32px rgba(255, 140, 60, 0.18);
    position: relative;
    overflow: hidden;
  }
  .poster-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      180deg,
      transparent 60%,
      rgba(0, 0, 0, 0.5) 100%
    );
    pointer-events: none;
  }
  .carta-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: #ffe6c8;
  }
  .info-titulo {
    font-size: 26px;
    margin: 0;
    color: #ffe6c8;
    line-height: 1.15;
  }
  .info-meta {
    font-size: 13px;
    color: #d9c0a4;
    margin: 0;
    letter-spacing: 0.3px;
  }
  .info-generos {
    font-size: 12px;
    color: #ffae5c;
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.8px;
  }
  .info-descripcion {
    font-size: 14px;
    line-height: 1.5;
    color: #e6d4bd;
    margin: 4px 0 0;
    max-height: 7em;
    overflow: hidden;
  }
  .info-actores {
    font-size: 13px;
    color: #d9c0a4;
    margin: 0;
  }
  .etiqueta {
    color: #998878;
    margin-right: 4px;
  }
  .info-trivia {
    font-size: 13px;
    color: #ffd6a8;
    background: rgba(255, 140, 60, 0.12);
    padding: 8px 12px;
    border-radius: 10px;
    border-left: 3px solid #ff7a2e;
    margin: 6px 0 0;
    font-style: italic;
  }
  .info-trivia span {
    margin-right: 4px;
    font-style: normal;
  }
  .comentario {
    color: #ffd6a8;
    font-size: 18px;
    font-style: italic;
    min-height: 26px;
    margin: 4px 0 10px;
  }
  .ayuda {
    color: #998878;
    font-size: 13px;
    min-height: 26px;
    margin: 4px 0 10px;
  }
  /* --- Prompt de rating (en mazo cuando "ya la vi") --- */
  .prompt-rating {
    background: rgba(80, 32, 14, 0.75);
    border: 1.5px solid rgba(255, 140, 60, 0.55);
    border-radius: 16px;
    padding: 14px 22px;
    margin: 4px 0 12px;
  }
  .prompt-titulo {
    color: #ffe6c8;
    font-size: 17px;
    margin: 0 0 8px;
  }
  .prompt-ayuda {
    color: #998878;
    font-size: 12px;
    margin: 8px 0 0;
  }
  /* Estrellas seleccionables: las activas brillan, las inactivas son tenues. */
  .estrellas-elegir {
    display: flex;
    gap: 8px;
    justify-content: center;
    align-items: center;
    margin: 6px 0;
  }
  .estrellas-elegir.big {
    gap: 14px;
    margin: 18px 0;
  }
  .estrellas-elegir .estrella {
    color: rgba(255, 174, 92, 0.25);
    font-size: 32px;
    transition:
      color 0.15s,
      text-shadow 0.15s,
      transform 0.15s;
  }
  .estrellas-elegir.big .estrella {
    font-size: 48px;
  }
  .estrellas-elegir .estrella.activa {
    color: #ffd66a;
    text-shadow: 0 0 14px rgba(255, 200, 80, 0.65);
    transform: translateY(-1px);
  }
  .estrellas-elegir .rating-num {
    margin-left: 14px;
    color: #ffd6a8;
    font-size: 16px;
    font-weight: 600;
    min-width: 36px;
    text-align: left;
  }
  .estrellas-elegir.big .rating-num {
    font-size: 22px;
  }
  /* --- Rating TMDb destacado --- */
  .tmdb-rating {
    background: rgba(255, 170, 80, 0.18);
    border: 1px solid rgba(255, 170, 80, 0.55);
    color: #ffd66a;
    padding: 2px 10px;
    border-radius: 999px;
    font-weight: 600;
    margin-left: 8px;
    font-size: 13px;
    letter-spacing: 0.3px;
  }
  .fin-peli {
    color: #ffd6a8;
    font-size: 20px;
    font-style: italic;
    margin: 0 0 6px;
  }
  .atajos-mazo {
    display: flex;
    gap: 14px;
  }
  .atajo-box {
    background: rgba(50, 22, 12, 0.6);
    border: 1.5px solid rgba(255, 140, 60, 0.3);
    border-radius: 12px;
    padding: 10px 16px;
    color: #ffe6c8;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    min-width: 92px;
  }
  .atajo-box kbd {
    font-size: 20px;
    padding: 4px 12px;
  }
  .atajo-box span {
    font-size: 12px;
    color: #d9c0a4;
  }

  /* --- Tadán --- */
  .vera-line {
    color: #ffe6c8;
    font-size: 22px;
    max-width: 720px;
    margin: 0 0 14px;
    line-height: 1.35;
    position: relative;
    z-index: 2;
  }
  .vera-line.dura {
    color: #ffae5c;
    font-style: italic;
  }
  .vera-line.grande {
    font-size: 28px;
    margin-bottom: 30px;
  }
  .backdrop-tadan {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center top;
    opacity: 0.35;
    filter: blur(2px);
    z-index: 0;
    /* Crítico: si no es transparente a clicks, tapa los botones del tadán. */
    pointer-events: none;
  }
  .backdrop-tadan::after {
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
  /* Garantiza que el botón Play y links queden encima del backdrop. */
  .tadan .btn-grande,
  .tadan .atajo,
  .tadan .cierre {
    position: relative;
    z-index: 2;
  }
  .ficha-tadan {
    display: flex;
    gap: 24px;
    max-width: 880px;
    width: 100%;
    margin: 8px 0 16px;
    text-align: left;
    position: relative;
    z-index: 2;
  }
  .poster-tadan {
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
  .info-tadan {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: #ffe6c8;
  }
  .tadan-titulo {
    font-size: 30px;
    margin: 0;
    color: #ffe6c8;
    line-height: 1.1;
  }
  .gancho {
    color: #ffd6a8;
    font-size: 19px;
    max-width: 600px;
    margin: 6px 0 16px;
    font-style: italic;
    position: relative;
    z-index: 2;
  }
  .cierre {
    color: #998878;
    font-size: 14px;
    margin: 18px 0 0;
    position: relative;
    z-index: 2;
  }

  /* --- Player mock --- */
  .player {
    width: min(80vw, 720px);
    height: min(50vw, 420px);
    border-radius: 18px;
    box-shadow: 0 24px 56px rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
    margin-bottom: 24px;
    overflow: hidden;
    background-size: cover;
    background-position: center;
  }
  .player-overlay {
    position: absolute;
    inset: 0;
    background: radial-gradient(
      ellipse at center,
      rgba(0, 0, 0, 0.25) 30%,
      rgba(0, 0, 0, 0.7) 100%
    );
    pointer-events: none;
  }
  .player.en-pausa::after {
    content: "";
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    border-radius: 18px;
  }
  .player-titulo {
    font-size: 32px;
    color: #fff;
    z-index: 1;
    text-shadow: 0 4px 12px rgba(0, 0, 0, 0.9);
  }
  .badge-pausa {
    position: absolute;
    top: 16px;
    left: 16px;
    background: rgba(255, 170, 80, 0.85);
    color: #1a0a04;
    padding: 4px 12px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 700;
    z-index: 2;
  }
  .player-controles {
    display: flex;
    gap: 14px;
  }
  .btn-control {
    padding: 12px 22px;
    border-radius: 999px;
    border: 1.5px solid rgba(255, 140, 60, 0.4);
    background: rgba(50, 22, 12, 0.7);
    color: #ffe6c8;
    font-size: 16px;
    cursor: pointer;
  }
  .btn-control:hover,
  .btn-control:focus-visible {
    border-color: #ff7a2e;
    background: rgba(80, 32, 14, 0.9);
  }
</style>
