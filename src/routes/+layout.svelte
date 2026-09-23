<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Header from "$lib/Header.svelte";
  import Ayuda from "$lib/atajos/Ayuda.svelte";
  import Updater from "$lib/Updater.svelte";
  import ResumePill from "$lib/ResumePill.svelte";
  import SubsService from "$lib/SubsService.svelte";
  import HistorialTracker from "$lib/HistorialTracker.svelte";
  import { config, loadConfig, initDetection, initRd, refreshRdLinked } from "$lib/config.svelte";
  import { onboardingHecho, faltaLoMinimo } from "$lib/onboarding";
  import { initTorrentSession, startQueuePoll, stopQueuePoll } from "$lib/torrents.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  import { cargarHistorial } from "$lib/historial.svelte";
  import { cargarDescargas } from "$lib/descargas.svelte";
  import { cola, cargarCola, arrancarMotor } from "$lib/colaDescargas.svelte";
  import { ACCIONES, loadGamepadMap, gamepadCaptured, type GamepadMap } from "$lib/controls";
  import { ayuda } from "$lib/atajos/store.svelte";
  let { children } = $props();

  // --- Mando físico global: dispatcha las mismas teclas que el web/teclado ---
  let padMap: GamepadMap = loadGamepadMap();
  let btnToKey: Record<number, string> = {};
  function recomputeBtnToKey() {
    btnToKey = {};
    for (const a of ACCIONES) {
      const b = padMap[a.id];
      if (b !== undefined && b >= 0) btnToKey[b] = a.key;
    }
  }
  recomputeBtnToKey();
  let padRaf = 0;
  let padPrev: boolean[] = [];
  let padCooldown = 0;
  function onGamepadMapChanged() {
    padMap = loadGamepadMap();
    recomputeBtnToKey();
  }
  function gamepadBridge() {
    const pads = navigator.getGamepads?.() ?? [];
    const gp = pads.find((p) => p) ?? null;
    // No interferir mientras se escribe en un campo.
    if (gp && document.activeElement?.tagName !== "INPUT" && !gamepadCaptured()) {
      const b = gp.buttons.map((x) => x.pressed);
      for (let i = 0; i < b.length; i++) {
        if (b[i] && !padPrev[i] && btnToKey[i]) dispatchRemoteKey(btnToKey[i]);
      }
      padPrev = b;
      // Stick izquierdo = flechas (con cooldown anti-repetición).
      const ax = gp.axes[0] ?? 0;
      const ay = gp.axes[1] ?? 0;
      if (padCooldown > 0) padCooldown--;
      else if (Math.abs(ax) > 0.7 || Math.abs(ay) > 0.7) {
        if (Math.abs(ax) > Math.abs(ay)) dispatchRemoteKey(ax > 0 ? "ArrowRight" : "ArrowLeft");
        else dispatchRemoteKey(ay > 0 ? "ArrowDown" : "ArrowUp");
        padCooldown = 12;
      }
    }
    padRaf = requestAnimationFrame(gamepadBridge);
  }

  let unlistenRemoteKey: UnlistenFn | null = null;
  let unlistenRemoteText: UnlistenFn | null = null;
  let unlistenRemoteOpen: UnlistenFn | null = null;
  let unlistenMpvState: UnlistenFn | null = null;

  // Último input/textarea enfocado: si al llegar el texto del celular el foco ya
  // se movió (p.ej. el webview perdió foco al escanear), igual sabemos a qué
  // campo va. Lo rastreamos con focusin global.
  let lastInput: HTMLInputElement | HTMLTextAreaElement | null = null;
  function trackFocus(e: FocusEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) {
      lastInput = t as HTMLInputElement | HTMLTextAreaElement;
    }
  }

  // ¿Es un campo de texto utilizable (input/textarea visible y habilitado)?
  function isUsableField(n: Element | null): n is HTMLInputElement | HTMLTextAreaElement {
    if (!n) return false;
    const tag = n.tagName;
    if (tag !== "INPUT" && tag !== "TEXTAREA") return false;
    const el = n as HTMLInputElement | HTMLTextAreaElement;
    if (el.disabled || (el as HTMLInputElement).readOnly) return false;
    if (tag === "INPUT") {
      const t = (el as HTMLInputElement).type;
      if (["checkbox", "radio", "range", "button", "submit", "hidden", "file", "color"].includes(t))
        return false;
    }
    // Visible en pantalla (offsetParent null = display:none o sin layout).
    return el.offsetParent !== null;
  }

  // Escribe el texto que llega del teclado web (POST /text) en un campo de texto.
  // Orden de preferencia: el campo enfocado → el último enfocado → el primer
  // campo de texto visible en pantalla (p.ej. el buscador del home, donde el
  // foco suele estar en una tarjeta y no en el input). Usa el setter nativo de
  // value + evento "input" para que Svelte (bind:value) y los handlers reaccionen.
  function injectRemoteText(text: string) {
    let el: Element | null = document.activeElement;
    if (!isUsableField(el)) el = lastInput;
    if (!isUsableField(el)) {
      el = Array.from(document.querySelectorAll("input, textarea"))
        .find((n) => isUsableField(n)) ?? null;
    }
    if (!isUsableField(el)) return;
    const input = el;
    const proto = input.tagName === "TEXTAREA"
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
    setter?.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    input.focus();
  }

  // Dispatcha un KeyboardEvent sintético en window y document para que
  // tanto handlers globales (svelte:window onkeydown) como locales (modals)
  // reaccionen. Reemplaza inyección OS-level (enigo no anda en Wayland).
  function dispatchRemoteKey(key: string) {
    const ev = new KeyboardEvent("keydown", {
      key,
      code: keyToCode(key),
      bubbles: true,
      cancelable: true,
    });
    (document.activeElement ?? document.body).dispatchEvent(ev);
    window.dispatchEvent(new KeyboardEvent("keydown", { key, code: keyToCode(key), bubbles: true }));
  }
  function keyToCode(k: string): string {
    switch (k) {
      case "ArrowUp": return "ArrowUp";
      case "ArrowDown": return "ArrowDown";
      case "ArrowLeft": return "ArrowLeft";
      case "ArrowRight": return "ArrowRight";
      case "Enter": return "Enter";
      case "Escape": return "Escape";
      case "Backspace": return "Backspace";
      case " ": return "Space";
      case "i": case "I": return "KeyI";
      case "m": case "M": return "KeyM";
      default: return "";
    }
  }

  // Botón Menú del mando (Start → "m"). Antes se dispatchaba y no lo escuchaba
  // nadie: es la única vía de teclado/mando para llegar a Configuración, porque
  // la única otra puerta es el engranaje de la barra.
  function onMenuKey(e: KeyboardEvent) {
    if (e.key !== "m" && e.key !== "M") return;
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    const tag = t?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || (t?.isContentEditable ?? false)) return;
    if (ayuda.visible) return;
    const ruta = window.location.pathname;
    if (ruta.startsWith("/config")) return;
    e.preventDefault();
    void goto("/config");
  }

  onMount(() => {
    loadConfig();
    initDetection();
    // Historial + favoritos a memoria: el grid pinta ticks sin query por card.
    void cargarHistorial();
    // Qué películas ya están bajadas en este equipo: la ficha lo muestra y
    // Descubrir las reproduce sin salir a la red.
    void cargarDescargas();
    // Migra/lee credenciales RD del store seguro del backend.
    void initRd().then(abrirAsistenteSiHaceFalta);
    // Propagar concurrencia configurada al worker Rust.
    void setConcurrenciaScreening(config.screeningConcurrency);
    // Descarga local activada: levanta la sesión torrent para recuperar la cola
    // que quedó a medias del arranque anterior. Sin esta opción la app nunca
    // abre un socket de BitTorrent.
    if (config.torrentLocal) {
      void initTorrentSession()
        .then((n) => { if (n > 0) startQueuePoll(); })
        // La cola de pendientes se retoma después de levantar la sesión: el
        // motor necesita saber cuántas descargas hay vivas para decidir si
        // puede arrancar la siguiente.
        .then(() => cargarCola())
        .then(() => { if (cola.filas.length) arrancarMotor(); })
        .catch((e) => console.warn("[torrent init]", e));
    }
    // Auto-arranque del servidor web si el user lo activó en config.
    // Falla silencioso (ej. puerto ocupado) — no rompe la app.
    if (config.webAutoStart) {
      void invoke("web_server_start", { port: config.webPort }).catch((e) => {
        console.warn("[web autostart]", e);
      });
    }
    // Recibe keys del mando web y los dispatcha como eventos nativos.
    // Reemplaza enigo (no compatible Wayland).
    void listen<string>("remote_key", (e) => {
      dispatchRemoteKey(e.payload);
    }).then((un) => { unlistenRemoteKey = un; });
    // mpv corre como proceso externo fullscreen aparte. Al cerrarse, KWin
    // (visto en Wayland) a veces no recalcula bien el strut del panel y
    // este queda montado sobre la ventana aunque siga en fullscreen state.
    // Reafirmar el fullscreen fuerza a KWin a recomputar geometría.
    void listen<boolean>("mpv:state", (e) => {
      if (e.payload) return;
      if (!kiosk) return;
      getCurrentWindow().setFullscreen(true).catch(() => {});
    }).then((un) => { unlistenMpvState = un; });
    // Texto largo (API key) tecleado/pegado/escaneado en el celular → campo activo.
    void listen<string>("remote_text", (e) => {
      injectRemoteText(e.payload);
    }).then((un) => { unlistenRemoteText = un; });
    // Título elegido en el catálogo del control web. Va por el mismo handoff
    // que usa Vera: la home resuelve el detalle y entra a Descubrir.
    void listen<{ id: number; tipo: string }>("remote_open", (e) => {
      const { id, tipo } = e.payload;
      // Si venía algo reproduciéndose, se corta: el handoff abre otro título y
      // dejar mpv vivo encima taparía la pantalla a la que estamos yendo.
      void invoke("mpv_stop")
        .catch(() => {})
        .finally(() => goto(`/?play=${id}&type=${tipo}`));
    }).then((un) => { unlistenRemoteOpen = un; });
    window.addEventListener("focusin", trackFocus);
    // El listener del menú va acá (y no en <svelte:window>) para quedar
    // REGISTRADO ÚLTIMO: así ve el defaultPrevented de la ruta y no pisa la
    // "m" de mute del reproductor IPTV ni la del panel de volumen.
    window.addEventListener("keydown", onMenuKey);
    // Mando físico global + recarga de mapeo al cambiarlo en /config.
    window.addEventListener("gamepad-map-changed", onGamepadMapChanged);
    padRaf = requestAnimationFrame(gamepadBridge);
    // Apagar el splash de app.html. Mantenemos un piso mínimo de tiempo
    // (1800 ms) para que se aprecie el banner aunque Svelte monte rápido.
    // .ready dispara el fadeout CSS (700 ms); después removemos el nodo.
    const splash = document.getElementById("kutral-splash");
    if (splash) {
      const SPLASH_MIN_MS = 1800;
      const SPLASH_FADE_MS = 700;
      setTimeout(() => {
        splash.classList.add("ready");
        setTimeout(() => splash.remove(), SPLASH_FADE_MS);
      }, SPLASH_MIN_MS);
    }
  });

  // Primera vez: si no hay catálogo NI forma de reproducir, el asistente. Se
  // consulta después de initRd porque rdLinked llega del backend, no de
  // localStorage. Si ya se pasó por el asistente no vuelve a aparecer, aunque
  // falte todo: fue una decisión del usuario.
  async function abrirAsistenteSiHaceFalta() {
    if (onboardingHecho()) return;
    if (window.location.pathname.startsWith("/bienvenida")) return;
    await refreshRdLinked();
    const falta = faltaLoMinimo({
      tmdbKey: config.tmdbKey,
      rdLinked: config.rdLinked,
      torrentLocal: config.torrentLocal,
    });
    if (falta) void goto("/bienvenida");
  }

  const kiosk = $derived(
    config.loaded && (
      config.modeOverride === "kiosk" ||
      (config.modeOverride === "auto" && config.detectedKutral)
    )
  );

  onDestroy(() => {
    unlistenRemoteKey?.();
    unlistenRemoteKey = null;
    unlistenRemoteText?.();
    unlistenRemoteText = null;
    unlistenRemoteOpen?.();
    unlistenRemoteOpen = null;
    unlistenMpvState?.();
    unlistenMpvState = null;
    cancelAnimationFrame(padRaf);
    window.removeEventListener("focusin", trackFocus);
    window.removeEventListener("keydown", onMenuKey);
    window.removeEventListener("gamepad-map-changed", onGamepadMapChanged);
    stopQueuePoll();
  });

  // El servidor web busca en TMDb por su cuenta (el celular no tiene la key),
  // así que se la empujamos: al cargar la config y cada vez que cambie.
  let ultimaKeyWeb: string | null = null;
  $effect(() => {
    if (!config.loaded) return;
    const k = config.tmdbKey ?? "";
    if (k === ultimaKeyWeb) return;
    ultimaKeyWeb = k;
    void invoke("web_set_tmdb_key", { key: k }).catch((e) => {
      console.warn("[web key]", e);
    });
  });

  let lastApplied: boolean | null = null;
  $effect(() => {
    if (!config.loaded) return;
    const target = kiosk || config.pantallaCompleta;
    if (target === lastApplied) return;
    lastApplied = target;
    getCurrentWindow().setFullscreen(target).catch((e) => {
      console.warn("[fullscreen]", e);
    });
  });
</script>

<div class="app-shell">
  <Header />
  <div class="app-content">
    {@render children()}
  </div>
</div>

<!-- Overlay global: I abre/cierra desde cualquier ruta de Kütral. -->
<Ayuda />
<Updater />
<!-- Esc en el video no cierra: suspende. Esta pill lo retoma (o P). -->
<ResumePill />
<!-- Subtítulos del reproductor: vive acá para sobrevivir al Esc y al IPTV. -->
<SubsService />
<!-- Escribe el progreso de mpv en watch_history mientras reproduce. -->
<HistorialTracker />

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    height: 100%;
    background: #0b0b0f;
    /* Controles nativos (desplegables de <select>, scrollbars) en oscuro:
       evita el item blanco sobre fondo blanco. */
    color-scheme: dark;
  }
  /* Refuerzo para los <option> del desplegable nativo. */
  :global(option) {
    background: #15151c;
    color: #e6e6ec;
  }
  .app-shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
  .app-content {
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
    position: relative;
  }
</style>
