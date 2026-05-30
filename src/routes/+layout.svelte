<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Header from "$lib/Header.svelte";
  import Ayuda from "$lib/atajos/Ayuda.svelte";
  import Updater from "$lib/Updater.svelte";
  import { config, loadConfig, initDetection } from "$lib/config.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  let { children } = $props();

  onMount(() => {
    loadConfig();
    initDetection();
    // Propagar concurrencia configurada al worker Rust.
    void setConcurrenciaScreening(config.screeningConcurrency);
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
