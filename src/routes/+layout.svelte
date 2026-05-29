<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Header from "$lib/Header.svelte";
  import Ayuda from "$lib/atajos/Ayuda.svelte";
  import Updater from "$lib/Updater.svelte";
  import { config, loadConfig, initDetection } from "$lib/config.svelte";
  let { children } = $props();

  onMount(() => {
    loadConfig();
    initDetection();
    // Apagar el splash de app.html una vez que el layout monta:
    // .ready dispara el fadeout CSS (0.5s); después removemos el nodo
    // del DOM para que no quede ocupando memoria ni capturando algo.
    const splash = document.getElementById("kutral-splash");
    if (splash) {
      splash.classList.add("ready");
      setTimeout(() => splash.remove(), 600);
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
