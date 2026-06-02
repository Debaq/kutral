<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Header from "$lib/Header.svelte";
  import Ayuda from "$lib/atajos/Ayuda.svelte";
  import Updater from "$lib/Updater.svelte";
  import { config, loadConfig, initDetection } from "$lib/config.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  import { ACCIONES, loadGamepadMap, gamepadCaptured, type GamepadMap } from "$lib/controls";
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

  onMount(() => {
    loadConfig();
    initDetection();
    // Propagar concurrencia configurada al worker Rust.
    void setConcurrenciaScreening(config.screeningConcurrency);
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

  const kiosk = $derived(
    config.loaded && (
      config.modeOverride === "kiosk" ||
      (config.modeOverride === "auto" && config.detectedKutral)
    )
  );

  onDestroy(() => {
    unlistenRemoteKey?.();
    unlistenRemoteKey = null;
    cancelAnimationFrame(padRaf);
    window.removeEventListener("gamepad-map-changed", onGamepadMapChanged);
  });

  let lastApplied: boolean | null = null;
  $effect(() => {
    if (!config.loaded) return;
    const target = kiosk;
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
