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
  import CastPill from "$lib/CastPill.svelte";
  import { cargarCast, recuperarTv } from "$lib/cast.svelte";
  import { config, loadConfig, initDetection, initRd, refreshRdLinked } from "$lib/config.svelte";
  import { onboardingHecho, faltaLoMinimo } from "$lib/onboarding";
  import { initTorrentSession, startQueuePoll, stopQueuePoll } from "$lib/torrents.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  import { cargarHistorial } from "$lib/historial.svelte";
  import { iniciarCuenta } from "$lib/cuenta.svelte";
  import { iniciarAvisoRd } from "$lib/avisoRd";
  import { cargarListas } from "$lib/listas.svelte";
  import { cargarPerfiles } from "$lib/perfiles.svelte";
  import QuienVe from "$lib/QuienVe.svelte";
  import { cargarDescargas, reconciliarDescargas } from "$lib/descargas.svelte";
  import { cola, cargarCola, arrancarMotor } from "$lib/colaDescargas.svelte";
  import { ayuda } from "$lib/atajos/store.svelte";
  let { children } = $props();

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

  // Dispatcha un KeyboardEvent sintético en el elemento enfocado: burbujea
  // hasta document y window, así que lo ven tanto los handlers locales
  // (modals) como los globales (svelte:window onkeydown). Una sola vez: un
  // segundo dispatch en window hacía que cada tecla del control llegara doble
  // a los globales (flechas de a dos, "i" que abría y cerraba). Reemplaza
  // inyección OS-level (enigo no anda en Wayland).
  function dispatchRemoteKey(key: string) {
    const ev = new KeyboardEvent("keydown", {
      key,
      code: keyToCode(key),
      bubbles: true,
      cancelable: true,
    });
    (document.activeElement ?? document.body).dispatchEvent(ev);
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

  // Tecla "m" (también el botón Menú del control web). Es la única vía de
  // teclado para llegar a Configuración; la otra puerta es el engranaje de la
  // barra.
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
    cargarCast();
    void recuperarTv();
    initDetection();
    // Clic derecho = Volver (Esc), como el botón atrás de un control. El menú
    // del navegador (Recargar, Inspeccionar…) no sirve en la app. En los
    // campos de texto queda, para copiar y pegar.
    window.addEventListener(
      "contextmenu",
      (e) => {
        const t = e.target as HTMLElement | null;
        const texto =
          t?.closest("textarea, [contenteditable='true']") ||
          (t instanceof HTMLInputElement && !["checkbox", "radio", "range", "button", "submit"].includes(t.type));
        if (texto) return;
        e.preventDefault();
        dispatchRemoteKey("Escape");
      },
      { capture: true },
    );
    // Ctrl + rueda no amplía la interfaz (el pellizco del touchpad se corta en
    // Rust, ver lib.rs: llega por GDK y no pasa por acá).
    window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), {
      passive: false,
      capture: true,
    });
    // Historial + favoritos a memoria: el grid pinta ticks sin query por card.
    // Después, con la DB ya migrada, la sincronización con la cuenta (si hay).
    // Después del historial: getDb() abre la DB una sola vez y una segunda
    // llamada en paralelo recibiría null.
    void cargarHistorial().then(() => {
      void cargarPerfiles();
      void cargarListas();
      void iniciarCuenta();
    });
    // Qué películas ya están bajadas en este equipo: la ficha lo muestra y
    // Descubrir las reproduce sin salir a la red.
    void cargarDescargas();
    // Migra/lee credenciales RD del store seguro del backend.
    void initRd().then(abrirAsistenteSiHaceFalta);
    // Real-Debrid por vencer: aviso diario desde 7 días antes.
    iniciarAvisoRd();
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
        // Las filas "bajando" contra lo que de verdad quedó en la sesión.
        .then(() => reconciliarDescargas())
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
    // Recibe keys del control web y los dispatcha como eventos nativos.
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
    // Apagar el splash de app.html cuando la fogata terminó de encenderse:
    // splash.js deja en data-hasta cuándo (arranca con el sonido, así que
    // hasta que suena no se sabe). .ready dispara el fadeout CSS (700 ms);
    // después se quita el nodo.
    const splash = document.getElementById("kutral-splash");
    if (splash) {
      const SPLASH_FADE_MS = 700;
      const cerrarSplash = () => {
        const hasta = Number(splash.dataset.hasta);
        if (!hasta) {
          setTimeout(cerrarSplash, 100);
          return;
        }
        setTimeout(() => {
          splash.classList.add("ready");
          setTimeout(() => splash.remove(), SPLASH_FADE_MS);
        }, Math.max(0, hasta - Date.now()));
      };
      cerrarSplash();
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
    window.removeEventListener("focusin", trackFocus);
    window.removeEventListener("keydown", onMenuKey);
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
<!-- Lo que suena en la TV (Cast/DLNA): pausar o cortar desde cualquier ruta. -->
<CastPill />
<!-- ¿Quién está viendo? Al arrancar con varios perfiles, o desde el encabezado. -->
<QuienVe />

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
