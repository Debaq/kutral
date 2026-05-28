<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import WifiManager from "$lib/WifiManager.svelte";
  import { player } from "$lib/playerState.svelte";
  import { config, isKioskActive } from "$lib/config.svelte";

  type WifiState = { online: boolean; connected_ssid: string | null };

  let maximized = $state(true);

  const kiosk = $derived(
    config.modeOverride === "kiosk" ||
    (config.modeOverride === "auto" && config.detectedKutral)
  );
  const inConfig = $derived($page.url.pathname.startsWith("/config"));
  let exitOpen = $state(false);
  let exitConfirmBtn = $state<HTMLButtonElement | null>(null);
  let now = $state(fmtTime(new Date()));
  let wifi = $state<WifiState>({ online: true, connected_ssid: null });
  let wifiOpen = $state(false);
  let revealed = $state(false);
  let hideTimer: number | null = null;

  const hidden = $derived(player.playing && !revealed && !wifiOpen && !exitOpen);

  function scheduleHide() {
    if (hideTimer !== null) clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => { revealed = false; }, 2500);
  }
  function reveal() {
    revealed = true;
    scheduleHide();
  }
  function keepShown() {
    if (hideTimer !== null) { clearTimeout(hideTimer); hideTimer = null; }
    revealed = true;
  }
  function onHeaderLeave() {
    if (player.playing) scheduleHide();
  }

  function fmtTime(d: Date): string {
    const h = d.getHours().toString().padStart(2, "0");
    const m = d.getMinutes().toString().padStart(2, "0");
    return `${h}:${m}`;
  }

  async function refreshWifi() {
    try {
      wifi = await invoke<WifiState>("wifi_status");
    } catch {
      wifi = { online: navigator.onLine, connected_ssid: null };
    }
  }

  onMount(() => {
    let un: (() => void) | null = null;
    let clockTimer: number | null = null;
    let wifiTimer: number | null = null;
    const onOnline = () => refreshWifi();
    const onOffline = () => { wifi = { ...wifi, online: false }; };

    (async () => {
      const win = getCurrentWindow();
      try {
        maximized = await win.isMaximized();
      } catch {}
      try {
        un = await win.onResized(async () => {
          try {
            maximized = await win.isMaximized();
          } catch {}
        });
      } catch {}
      await refreshWifi();
    })();

    clockTimer = window.setInterval(() => { now = fmtTime(new Date()); }, 15_000);
    wifiTimer = window.setInterval(refreshWifi, 30_000);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);

    return () => {
      un?.();
      if (clockTimer !== null) clearInterval(clockTimer);
      if (wifiTimer !== null) clearInterval(wifiTimer);
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  });

  function openWifi() {
    if (!kiosk) return;
    wifiOpen = true;
  }

  function goConfig() {
    if (inConfig) goto("/"); else goto("/config");
  }

  async function minimize() {
    try { await getCurrentWindow().minimize(); } catch (e) { console.warn(e); }
  }
  async function toggleMax() {
    try { await getCurrentWindow().toggleMaximize(); } catch (e) { console.warn(e); }
  }
  async function close() {
    try { await getCurrentWindow().close(); } catch (e) { console.warn(e); }
  }

  function openExit() {
    exitOpen = true;
    setTimeout(() => exitConfirmBtn?.focus(), 30);
  }
  function cancelExit() {
    exitOpen = false;
  }
  function onExitKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); cancelExit(); }
  }
</script>

{#if player.playing && hidden}
  <div
    class="reveal-zone"
    onmouseenter={reveal}
    role="presentation"
  ></div>
{/if}

<div
  class="titlebar"
  class:hidden
  role="banner"
  data-tauri-drag-region
  onmouseenter={keepShown}
  onmouseleave={onHeaderLeave}
>
  <div class="brand" data-tauri-drag-region>
    <span class="logo" data-tauri-drag-region>✦</span>
    <span class="name" data-tauri-drag-region>Kütral</span>
  </div>

  <div class="status" data-tauri-drag-region>
    {#if kiosk}
      <button
        class="net-btn"
        onclick={openWifi}
        title={wifi.connected_ssid ?? (wifi.online ? "Red" : "Sin conexión")}
        aria-label="Gestionar WiFi"
      >
        <span class="net-icon" class:offline={!wifi.online}>{wifi.online ? "●" : "○"}</span>
        <span class="net-ssid">{wifi.connected_ssid ?? (wifi.online ? "Red" : "Sin red")}</span>
      </button>
    {:else}
      <span class="net-info" title={wifi.online ? "Conectado" : "Sin conexión"}>
        <span class="net-icon" class:offline={!wifi.online}>{wifi.online ? "●" : "○"}</span>
      </span>
    {/if}
    <span class="clock" data-tauri-drag-region>{now}</span>
    <button
      class="cfg-btn"
      onclick={goConfig}
      title={inConfig ? "Volver" : "Configuración"}
      aria-label="Configuración"
    >
      {inConfig ? "←" : "⚙"}
    </button>
  </div>

  <div class="controls">
    {#if kiosk}
      <button class="btn-exit" onclick={openExit} title="Salir">
        <span class="exit-label">Salir</span>
        <span aria-hidden="true">⏻</span>
      </button>
    {:else}
      <button class="ctrl" onclick={minimize} title="Minimizar" aria-label="Minimizar">
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <rect x="0" y="4.5" width="10" height="1" />
        </svg>
      </button>
      <button class="ctrl" onclick={toggleMax} title={maximized ? "Restaurar" : "Maximizar"} aria-label="Maximizar">
        {#if maximized}
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <rect x="0" y="2" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
            <rect x="2" y="0" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
        {:else}
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <rect x="0" y="0" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
        {/if}
      </button>
      <button class="ctrl ctrl-close" onclick={close} title="Cerrar" aria-label="Cerrar">
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0,0 L10,10 M10,0 L0,10" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    {/if}
  </div>
</div>

<WifiManager bind:open={wifiOpen} />

{#if exitOpen}
  <div
    class="exit-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Confirmar salida"
    onkeydown={onExitKey}
    tabindex="-1"
  >
    <div class="exit-card">
      <h2>¿Salir de Kütral?</h2>
      <p>Vas a cerrar la sesión.</p>
      <div class="exit-actions">
        <button class="btn-cancel" onclick={cancelExit}>Cancelar</button>
        <button
          class="btn-confirm"
          bind:this={exitConfirmBtn}
          onclick={close}
        >Salir</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .titlebar {
    height: 36px;
    flex: 0 0 36px;
    display: flex;
    align-items: stretch;
    background: #0b0b0f;
    border-bottom: 1px solid #1a1a22;
    user-select: none;
    -webkit-user-select: none;
    color: #e6e6ec;
    font-family: ui-sans-serif, system-ui, sans-serif;
    position: relative;
    z-index: 1000;
    transition: transform 220ms cubic-bezier(.4, 0, .2, 1);
    will-change: transform;
  }
  .titlebar.hidden {
    transform: translateY(-100%);
    pointer-events: none;
  }
  .reveal-zone {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 6px;
    z-index: 1001;
    background: transparent;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    flex: 1;
    min-width: 0;
  }
  .logo {
    color: #f3a951;
    font-size: 14px;
    line-height: 1;
  }
  .name {
    font-size: 13px;
    letter-spacing: 0.04em;
    color: #d8d8e0;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    color: #d8d8e0;
    font-size: 13px;
  }
  .clock {
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
    color: #d8d8e0;
  }
  .net-info {
    display: flex; align-items: center;
  }
  .net-btn {
    display: flex; align-items: center; gap: 6px;
    background: transparent; border: 0;
    color: #d8d8e0; cursor: pointer;
    padding: 4px 10px; border-radius: 6px;
    font-size: 12px;
  }
  .net-btn:hover { background: #1c1c26; color: #fff; }
  .net-btn:focus { outline: 2px solid #f3a951; outline-offset: 1px; }
  .net-icon {
    color: #6cd37a;
    font-size: 10px;
    line-height: 1;
  }
  .net-icon.offline { color: #c44; }
  .net-ssid {
    max-width: 140px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .cfg-btn {
    background: transparent;
    border: 0;
    color: #c8c8d0;
    cursor: pointer;
    font-size: 15px;
    padding: 4px 10px;
    border-radius: 6px;
    line-height: 1;
  }
  .cfg-btn:hover { background: #1c1c26; color: #fff; }
  .cfg-btn:focus { outline: 2px solid #f3a951; outline-offset: 1px; }
  .controls {
    display: flex;
    align-items: stretch;
  }
  .ctrl {
    width: 46px;
    border: 0;
    background: transparent;
    color: #c8c8d0;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }
  .ctrl:hover { background: #1c1c26; color: #fff; }
  .ctrl-close:hover { background: #c42b1c; color: #fff; }
  .ctrl svg rect[fill="none"] { stroke: currentColor; }
  .ctrl svg rect:not([fill="none"]) { fill: currentColor; }

  .btn-exit {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 18px;
    background: transparent;
    border: 0;
    color: #f3a951;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    letter-spacing: 0.04em;
  }
  .btn-exit:hover { background: #2a1a10; color: #ffb86b; }
  .exit-label { font-size: 13px; }

  .exit-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
    backdrop-filter: blur(4px);
  }
  .exit-card {
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 12px;
    padding: 28px 32px;
    min-width: 320px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    color: #e6e6ec;
    text-align: center;
  }
  .exit-card h2 {
    margin: 0 0 8px;
    font-size: 18px;
    font-weight: 600;
  }
  .exit-card p {
    margin: 0 0 20px;
    font-size: 14px;
    color: #a0a0aa;
  }
  .exit-actions {
    display: flex;
    gap: 12px;
    justify-content: center;
  }
  .btn-cancel, .btn-confirm {
    border: 0;
    border-radius: 8px;
    padding: 10px 22px;
    font-size: 14px;
    cursor: pointer;
    font-weight: 500;
  }
  .btn-cancel {
    background: #2a2a36;
    color: #d8d8e0;
  }
  .btn-cancel:hover { background: #34343f; }
  .btn-confirm {
    background: #c42b1c;
    color: #fff;
  }
  .btn-confirm:hover { background: #d8392b; }
  .btn-confirm:focus, .btn-cancel:focus {
    outline: 2px solid #f3a951;
    outline-offset: 2px;
  }
</style>
