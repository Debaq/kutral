<script lang="ts">
  // Overlay global de ayuda. Vive en +layout.svelte, escucha I-I (doble tap) y Esc.
  // Cada ruta registra su pantalla activa con ayuda.set(pantalla, ...). El catálogo
  // completo de atajos vive acá: la ayuda siempre muestra todo y resalta la sección
  // activa según `pantalla`.

  import { fade } from "svelte/transition";
  import { ayuda } from "./store.svelte";
  import { player } from "$lib/playerState.svelte";

  const DOUBLE_TAP_MS = 500;
  let lastIAt = 0;
  let scrollRef: HTMLDivElement | null = $state(null);

  type Atajo = { tecla: string; desc: string };
  type Seccion = { id: string; titulo: string; lineas: Atajo[] };

  // Catálogo único de toda la app. Mantener sincronizado con los handlers de
  // teclado de cada ruta/componente.
  const SECCIONES: Seccion[] = [
    {
      id: "globales",
      titulo: "Globales",
      lineas: [
        { tecla: "I-I", desc: "Abrir / cerrar esta ayuda" },
        { tecla: "P", desc: "Volver a la reproducción en pausa" },
        { tecla: "Esc", desc: "Paso atrás · cerrar modal" },
        { tecla: "Backspace", desc: "Salir a home · cerrar modal" },
      ],
    },
    {
      id: "inicio",
      titulo: "Inicio",
      lineas: [
        { tecla: "← → ↑ ↓", desc: "Navegar entre cards" },
        { tecla: "Home · End", desc: "Primera · última card" },
        { tecla: "Enter · Espacio", desc: "Descubrir card enfocada" },
        { tecla: "I", desc: "Saltar entre cards y panel info" },
      ],
    },
    {
      id: "playmenu",
      titulo: "Menú Descubrir",
      lineas: [
        { tecla: "↑ ↓", desc: "Mover entre acciones" },
        { tecla: "← →", desc: "Desplazar info" },
        { tecla: "PageUp · PageDown", desc: "Desplazar info más rápido" },
        { tecla: "Home · End", desc: "Inicio · fin del info" },
        { tecla: "Enter · Espacio", desc: "Activar acción" },
        { tecla: "Esc · Backspace", desc: "Cerrar menú" },
      ],
    },
    {
      id: "sources",
      titulo: "Selector de fuentes",
      lineas: [
        { tecla: "↑ ↓", desc: "Mover entre fuentes" },
        { tecla: "Enter · Espacio", desc: "Elegir fuente" },
        { tecla: "Esc · Backspace", desc: "Cancelar · volver" },
      ],
    },
    {
      id: "episodes",
      titulo: "Selector de episodios",
      lineas: [
        { tecla: "↑ ↓", desc: "Navegar temporadas · episodios" },
        { tecla: "→", desc: "Entrar a episodios" },
        { tecla: "←", desc: "Volver a temporadas" },
        { tecla: "Enter · Espacio", desc: "Elegir episodio" },
        { tecla: "Esc · Backspace", desc: "Atrás · cerrar" },
      ],
    },
    {
      id: "discover",
      titulo: "Reproducción · trailer",
      lineas: [
        { tecla: "Esc · Backspace", desc: "Salir a los menús dejando en pausa" },
        { tecla: "P", desc: "Volver a lo que quedó en pausa" },
        { tecla: "Enter · Espacio", desc: "Pausar y abrir el bar de iconos" },
        { tecla: "← →", desc: "Reproduciendo: saltar 10s · pausado: mover foco" },
        { tecla: "c · a · v", desc: "Menú de subtítulos · audio · video" },
        { tecla: "Mouse", desc: "Mover saca el bar · click activa el icono" },
        { tecla: "✕ Salir (bar)", desc: "Cerrar la reproducción del todo" },
      ],
    },
    {
      id: "entrada",
      titulo: "Vera · entrada",
      lineas: [
        { tecla: "Enter", desc: "Empezar sesión" },
      ],
    },
    {
      id: "contexto",
      titulo: "Vera · contexto",
      lineas: [
        { tecla: "← → ↑ ↓", desc: "Mover entre opciones" },
        { tecla: "Enter", desc: "Confirmar contexto" },
      ],
    },
    {
      id: "intencion",
      titulo: "Vera · intención",
      lineas: [
        { tecla: "← →", desc: "Elegir intención" },
        { tecla: "Enter", desc: "Confirmar y buscar" },
      ],
    },
    {
      id: "calificar",
      titulo: "Vera · calificar",
      lineas: [
        { tecla: "← →", desc: "Ajustar puntaje (-5..+5)" },
        { tecla: "↑", desc: "Marcar · desmarcar como vista" },
        { tecla: "↓", desc: "Saltar (puntaje -1 automático)" },
        { tecla: "Enter", desc: "Siguiente carta" },
      ],
    },
    {
      id: "ranking",
      titulo: "Vera · ranking",
      lineas: [
        { tecla: "↑ ↓", desc: "Navegar entre películas" },
        { tecla: "Enter", desc: "Descubrir película" },
      ],
    },
    {
      id: "importador TMDb",
      titulo: "Importador TMDb",
      lineas: [
        { tecla: "Tab", desc: "Cambiar de campo" },
        { tecla: "Enter", desc: "Activar botón con foco" },
      ],
    },
    {
      id: "config",
      titulo: "Configuración",
      lineas: [
        { tecla: "Esc · Backspace", desc: "Volver a home" },
        { tecla: "Esc", desc: "Cancelar vinculación Real-Debrid en curso" },
      ],
    },
    {
      id: "popovers",
      titulo: "Barra superior",
      lineas: [
        { tecla: "M", desc: "Silenciar · reactivar audio (en popover Volumen)" },
        { tecla: "Esc", desc: "Cerrar popover (Volumen · Brillo · Web · Wi-Fi)" },
      ],
    },
  ];

  // Cierre auto si entra reproducción mientras la ayuda está abierta.
  $effect(() => {
    if (player.playing && ayuda.visible) ayuda.close();
  });

  // Al abrir, scrollear a la sección activa.
  $effect(() => {
    if (!ayuda.visible) return;
    queueMicrotask(() => {
      scrollRef?.querySelector<HTMLElement>(".seccion.activa")?.scrollIntoView({
        block: "start",
        behavior: "auto",
      });
    });
  });

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    // Ignorar inputs/textareas/contenteditable para no romper escritura.
    const target = e.target as HTMLElement | null;
    if (target) {
      const tag = target.tagName;
      if (
        tag === "INPUT" ||
        tag === "TEXTAREA" ||
        target.isContentEditable
      ) {
        return;
      }
    }

    if (k === "i" || k === "I") {
      // En reproducción no abrir la ayuda: nada debe molestar.
      if (player.playing) {
        lastIAt = 0;
        return;
      }
      const now = Date.now();
      if (now - lastIAt <= DOUBLE_TAP_MS) {
        e.preventDefault();
        ayuda.toggle();
        lastIAt = 0;
      } else {
        lastIAt = now;
      }
      return;
    }
    // Cualquier otra tecla (excepto modificadores) rompe la secuencia I-I.
    if (k !== "Shift" && k !== "Control" && k !== "Alt" && k !== "Meta") {
      lastIAt = 0;
    }
    // Modales se cierran con Esc o Backspace.
    if (ayuda.visible && (k === "Escape" || k === "Backspace")) {
      e.preventDefault();
      e.stopImmediatePropagation();
      ayuda.close();
      return;
    }
    // Scroll del overlay con flechas / PageUp-Down / Home-End mientras está abierto.
    // stopImmediatePropagation: evita que PlayMenu/SourcePicker reaccionen debajo.
    if (!ayuda.visible || !scrollRef) return;
    switch (k) {
      case "ArrowDown":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollBy({ top: 60, behavior: "smooth" });
        break;
      case "ArrowUp":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollBy({ top: -60, behavior: "smooth" });
        break;
      case "PageDown":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollBy({ top: 320, behavior: "smooth" });
        break;
      case "PageUp":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollBy({ top: -320, behavior: "smooth" });
        break;
      case "Home":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollTo({ top: 0, behavior: "smooth" });
        break;
      case "End":
        e.preventDefault();
        e.stopImmediatePropagation();
        scrollRef.scrollTo({ top: scrollRef.scrollHeight, behavior: "smooth" });
        break;
      case "Enter":
      case " ":
        // Tragar Enter/Espacio para que no activen botones detrás.
        e.preventDefault();
        e.stopImmediatePropagation();
        break;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- Hint global abajo derecha — oculto en reproducción para no molestar. -->
{#if !player.playing}
  <div class="hint-global" aria-hidden="true">
    <kbd>I</kbd><kbd>I</kbd> ayuda
  </div>
{/if}

{#if ayuda.visible && !player.playing}
  <div class="overlay" transition:fade={{ duration: 180 }}>
    <div class="caja">
      <h3 class="titulo">Atajos de teclado</h3>
      {#if ayuda.pantalla}
        <p class="ctx">Estás en: <strong>{ayuda.pantalla}</strong></p>
      {/if}
      <div class="scroll" bind:this={scrollRef}>
        {#each SECCIONES as s}
          <section class="seccion" class:activa={s.id === ayuda.pantalla}>
            <h4 class="seccion-tit">{s.titulo}</h4>
            <div class="lineas">
              {#each s.lineas as a}
                <div class="linea">
                  <kbd class="tecla">{a.tecla}</kbd>
                  <span class="desc">{a.desc}</span>
                </div>
              {/each}
            </div>
          </section>
        {/each}
      </div>
      <p class="cierra"><kbd>I</kbd><kbd>I</kbd> o <kbd>Esc</kbd> para cerrar · <kbd>↑↓</kbd> para recorrer</p>
    </div>
  </div>
{/if}

<style>
  .hint-global {
    position: fixed;
    bottom: 14px;
    right: 18px;
    z-index: 90;
    color: #998878;
    font-size: 12px;
    opacity: 0.7;
    letter-spacing: 0.4px;
    pointer-events: none;
  }
  .hint-global :global(kbd) {
    margin-right: 4px;
  }

  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(10, 6, 8, 0.78);
    backdrop-filter: blur(8px);
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }
  .caja {
    background: rgba(40, 18, 10, 0.96);
    border: 1.5px solid rgba(255, 140, 60, 0.5);
    border-radius: 18px;
    padding: 24px 28px;
    max-width: 560px;
    width: 100%;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow:
      0 20px 60px rgba(0, 0, 0, 0.7),
      0 0 48px rgba(255, 140, 60, 0.18);
    color: #ffe6c8;
    font-family:
      "Inter",
      system-ui,
      sans-serif;
  }
  .titulo {
    color: #ffd6a8;
    font-size: 22px;
    margin: 0 0 6px;
    text-align: center;
  }
  .ctx {
    color: #cda37e;
    font-size: 12px;
    text-align: center;
    margin: 0 0 14px;
    letter-spacing: 0.4px;
  }
  .ctx strong {
    color: #ffd6a8;
    text-transform: capitalize;
  }
  .scroll {
    overflow-y: auto;
    padding-right: 6px;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 140, 60, 0.45) transparent;
  }
  .scroll::-webkit-scrollbar {
    width: 8px;
  }
  .scroll::-webkit-scrollbar-thumb {
    background: rgba(255, 140, 60, 0.45);
    border-radius: 4px;
  }
  .seccion {
    margin: 0 0 14px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid transparent;
  }
  .seccion.activa {
    background: rgba(255, 140, 60, 0.1);
    border-color: rgba(255, 140, 60, 0.45);
    box-shadow: 0 0 18px rgba(255, 140, 60, 0.15) inset;
  }
  .seccion-tit {
    color: #ffd6a8;
    font-size: 14px;
    margin: 0 0 8px;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    opacity: 0.85;
  }
  .seccion.activa .seccion-tit {
    opacity: 1;
  }
  .lineas {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .linea {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .tecla {
    min-width: 130px;
    text-align: center;
    font-size: 13px;
    padding: 4px 10px;
    background: rgba(255, 140, 60, 0.25);
    border: 1px solid rgba(255, 140, 60, 0.55);
    border-radius: 6px;
    color: #ffe6c8;
    flex-shrink: 0;
  }
  .desc {
    color: #ffe6c8;
    font-size: 14px;
  }
  .cierra {
    color: #998878;
    font-size: 12px;
    text-align: center;
    margin: 14px 0 0;
    border-top: 1px solid rgba(255, 140, 60, 0.18);
    padding-top: 12px;
  }
  .cierra :global(kbd) {
    margin: 0 4px;
  }
</style>
