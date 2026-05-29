<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getVersion } from "@tauri-apps/api/app";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import {
    config,
    loadConfig,
    saveConfig,
    LANGS,
    type ModeOverride,
  } from "$lib/config.svelte";
  import { notify } from "$lib/notifications.svelte";

  type RdDeviceStart = {
    device_code: string;
    user_code: string;
    verification_url: string;
    interval: number;
    expires_in: number;
  };
  type RdLinked = {
    access_token: string;
    refresh_token: string;
    client_id: string;
    client_secret: string;
    expires_in: number;
  };

  let tmdb = $state("");
  let rd = $state("");
  let lang = $state(config.lang);
  let mode = $state<ModeOverride>(config.modeOverride);
  let saved = $state(false);
  let showTmdb = $state(false);
  let showRdAdvanced = $state(false);

  // RD device flow
  let rdStarting = $state(false);
  let rdLink = $state<RdDeviceStart | null>(null);
  let rdWaiting = $state(false);
  let rdErr = $state("");
  let rdOk = $state(false);
  let rdAbort: AbortController | null = null;
  let countdownTimer: number | null = null;
  let secondsLeft = $state(0);

  // Updater
  let currentVer = $state("");
  type UpdStage = "idle" | "checking" | "uptodate" | "available" | "installing" | "ready" | "error";
  let updStage = $state<UpdStage>("idle");
  let updPending: Update | null = null;
  let updVersion = $state("");
  let updNotes = $state("");
  let updErr = $state("");
  let updDownloaded = $state(0);
  let updTotal = $state(0);

  onMount(async () => {
    loadConfig();
    tmdb = config.tmdbKey;
    rd = config.rdKey;
    lang = config.lang;
    mode = config.modeOverride;
    try { currentVer = await getVersion(); } catch {}
  });

  onDestroy(() => {
    cancelRd();
  });

  const dirty = $derived(
    tmdb !== config.tmdbKey ||
    rd !== config.rdKey ||
    lang !== config.lang ||
    mode !== config.modeOverride
  );

  function applyAndSave() {
    config.tmdbKey = tmdb.trim();
    config.rdKey = rd.trim();
    config.lang = lang;
    config.modeOverride = mode;
    saveConfig();
    saved = true;
    setTimeout(() => { saved = false; }, 1800);
  }

  function back() {
    goto("/");
  }

  async function startRd() {
    rdErr = "";
    rdOk = false;
    rdStarting = true;
    try {
      const start = await invoke<RdDeviceStart>("rd_device_start");
      rdLink = start;
      secondsLeft = start.expires_in;
      countdownTimer = window.setInterval(() => {
        secondsLeft = Math.max(0, secondsLeft - 1);
        if (secondsLeft === 0) cancelRd();
      }, 1000);
      rdWaiting = true;
      pollRd(start);
    } catch (e) {
      rdErr = String(e);
    } finally {
      rdStarting = false;
    }
  }

  async function pollRd(start: RdDeviceStart) {
    try {
      const linked = await invoke<RdLinked>("rd_device_poll", {
        deviceCode: start.device_code,
        interval: start.interval,
        expiresIn: start.expires_in,
      });
      rd = linked.access_token;
      localStorage.setItem("realdebrid_refresh", linked.refresh_token);
      localStorage.setItem("realdebrid_client_id", linked.client_id);
      localStorage.setItem("realdebrid_client_secret", linked.client_secret);
      config.rdKey = linked.access_token;
      saveConfig();
      rdOk = true;
      rdLink = null;
      stopCountdown();
    } catch (e) {
      if (!rdWaiting) return; // cancelado por user
      rdErr = String(e);
    } finally {
      rdWaiting = false;
    }
  }

  function cancelRd() {
    rdWaiting = false;
    rdLink = null;
    rdAbort?.abort();
    rdAbort = null;
    stopCountdown();
  }
  function stopCountdown() {
    if (countdownTimer !== null) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
  }

  async function openVerify() {
    if (!rdLink) return;
    try { await openUrl(rdLink.verification_url); } catch {}
  }

  function unlinkRd() {
    rd = "";
    localStorage.removeItem("realdebrid_refresh");
    localStorage.removeItem("realdebrid_client_id");
    localStorage.removeItem("realdebrid_client_secret");
    config.rdKey = "";
    saveConfig();
  }

  function fmtMmSs(s: number): string {
    const m = Math.floor(s / 60);
    const r = (s % 60).toString().padStart(2, "0");
    return `${m}:${r}`;
  }

  async function checkUpdate() {
    updStage = "checking";
    updErr = "";
    try {
      const u = await check();
      if (!u) {
        updStage = "uptodate";
        return;
      }
      updPending = u;
      updVersion = u.version;
      updNotes = u.body || "";
      updStage = "available";
      notify("info", `Actualización ${u.version} disponible`, u.body || undefined);
    } catch (e) {
      updErr = String(e);
      updStage = "error";
    }
  }

  async function installUpdate() {
    if (!updPending) return;
    updStage = "installing";
    updDownloaded = 0;
    updTotal = 0;
    try {
      await updPending.downloadAndInstall((ev) => {
        switch (ev.event) {
          case "Started": updTotal = ev.data.contentLength ?? 0; break;
          case "Progress": updDownloaded += ev.data.chunkLength; break;
          case "Finished": updStage = "ready"; break;
        }
      });
      await relaunch();
    } catch (e) {
      updErr = String(e);
      updStage = "error";
    }
  }

  function updPct(): number {
    if (updTotal <= 0) return 0;
    return Math.min(100, Math.round((updDownloaded / updTotal) * 100));
  }
  function updMb(b: number): string {
    return (b / 1024 / 1024).toFixed(1);
  }
</script>

<svelte:head>
  <title>Configuración · Kütral</title>
</svelte:head>

<div class="cfg-root">
  <div class="cfg-wrap">
    <header class="cfg-head">
      <button class="back" onclick={back} title="Volver">← Volver</button>
      <h1>Configuración</h1>
    </header>

    <div class="cfg-grid">
      <div class="col">
        <section class="block">
          <h2>Idioma</h2>
          <p class="hint">Para títulos, descripciones e interfaz.</p>
          <div class="radio-group">
            {#each LANGS as l}
              <label class="radio">
                <input type="radio" name="lang" value={l.id} bind:group={lang} />
                <span>{l.label}</span>
              </label>
            {/each}
          </div>
        </section>

        <section class="block">
          <h2>Modo de interfaz</h2>
          <p class="hint">
            Auto = kiosko en Kütral OS, escritorio en el resto.
            Detectado: <em>{config.detectedKutral ? "Kütral OS" : "Escritorio"}</em>
          </p>
          <div class="mode-group">
            <label class="mode-card" class:sel={mode === "auto"}>
              <input type="radio" name="mode" value="auto" bind:group={mode} />
              <div><strong>Auto</strong><span>Detecta y aplica el modo correcto.</span></div>
            </label>
            <label class="mode-card" class:sel={mode === "desktop"}>
              <input type="radio" name="mode" value="desktop" bind:group={mode} />
              <div><strong>Escritorio</strong><span>Ventana con minimizar / maximizar / cerrar.</span></div>
            </label>
            <label class="mode-card" class:sel={mode === "kiosk"}>
              <input type="radio" name="mode" value="kiosk" bind:group={mode} />
              <div><strong>Kiosko / embedido</strong><span>Pantalla completa. Solo "Salir" + gestor WiFi.</span></div>
            </label>
          </div>
        </section>

        <section class="block">
          <h2>Actualizaciones</h2>
          <p class="hint">
            Versión actual: <em>{currentVer || "?"}</em>
          </p>

          {#if updStage === "idle"}
            <button class="btn-link" onclick={checkUpdate}>Buscar actualizaciones</button>
          {:else if updStage === "checking"}
            <p class="upd-line"><span class="spinner"></span> Buscando…</p>
          {:else if updStage === "uptodate"}
            <p class="upd-line upd-ok">Estás al día.</p>
            <button class="link-tiny" onclick={checkUpdate}>Volver a buscar</button>
          {:else if updStage === "available"}
            <p class="upd-line">
              Disponible: <strong>v{updVersion}</strong>
            </p>
            {#if updNotes}<pre class="upd-notes">{updNotes}</pre>{/if}
            <button class="btn-link" onclick={installUpdate}>Instalar y reiniciar</button>
            <button class="link-tiny" onclick={() => { updStage = "idle"; }}>Más tarde</button>
          {:else if updStage === "installing"}
            <p class="upd-line"><span class="spinner"></span> Descargando…</p>
            {#if updTotal > 0}
              <div class="bar"><div class="bar-fill" style="width: {updPct()}%"></div></div>
              <p class="upd-mb">{updMb(updDownloaded)} / {updMb(updTotal)} MB · {updPct()}%</p>
            {:else}
              <p class="upd-mb">{updMb(updDownloaded)} MB</p>
            {/if}
          {:else if updStage === "ready"}
            <p class="upd-line"><span class="spinner"></span> Reiniciando…</p>
          {:else if updStage === "error"}
            <p class="err">{updErr}</p>
            <button class="link-tiny" onclick={checkUpdate}>Reintentar</button>
          {/if}
        </section>
      </div>

      <div class="col">
        <section class="block">
          <h2>API key de TMDb</h2>
          <p class="hint">
            Gratis en <code>themoviedb.org/settings/api</code>. Sin esto, no hay catálogo.
          </p>
          <div class="key-row">
            <input
              type={showTmdb ? "text" : "password"}
              bind:value={tmdb}
              placeholder="tmdb_xxxxxxxxxxxxxxxxxxxxxxxx"
              autocomplete="off"
              spellcheck="false"
            />
            <button class="reveal" onclick={() => (showTmdb = !showTmdb)} title={showTmdb ? "Ocultar" : "Mostrar"}>
              {showTmdb ? "🙈" : "👁"}
            </button>
          </div>
        </section>

        <section class="block">
          <h2>RealDebrid</h2>
          <p class="hint">Vinculá tu cuenta para streaming premium sin publicidad.</p>

          {#if rd && !rdLink}
            <div class="rd-linked">
              <span class="dot ok"></span>
              <span>Cuenta vinculada</span>
              <button class="btn-sec" onclick={unlinkRd}>Desvincular</button>
            </div>
          {:else if rdLink}
            <div class="rd-flow">
              <p class="rd-step">1. Andá a <button class="rd-link" onclick={openVerify}>{rdLink.verification_url}</button></p>
              <p class="rd-step">2. Ingresá este código:</p>
              <div class="rd-code">{rdLink.user_code}</div>
              <p class="rd-wait">
                <span class="spinner"></span>
                Esperando autorización… <em>{fmtMmSs(secondsLeft)}</em>
              </p>
              <button class="btn-sec" onclick={cancelRd}>Cancelar</button>
            </div>
          {:else}
            <button class="btn-link" onclick={startRd} disabled={rdStarting}>
              {rdStarting ? "Conectando…" : "Vincular cuenta"}
            </button>
            <button class="link-tiny" onclick={() => (showRdAdvanced = !showRdAdvanced)}>
              {showRdAdvanced ? "Ocultar avanzado" : "Pegar token manualmente"}
            </button>
            {#if showRdAdvanced}
              <div class="key-row" style="margin-top: 10px;">
                <input
                  type="password"
                  bind:value={rd}
                  placeholder="access_token"
                  autocomplete="off"
                  spellcheck="false"
                />
              </div>
            {/if}
          {/if}

          {#if rdErr}<p class="err">{rdErr}</p>{/if}
          {#if rdOk}<p class="ok">¡Vinculado!</p>{/if}
        </section>
      </div>
    </div>

    <div class="actions">
      <button class="btn-save" onclick={applyAndSave} disabled={!dirty}>
        {saved ? "Guardado ✓" : "Guardar cambios"}
      </button>
      <p class="hint-actions">
        El modo kiosko activa pantalla completa al guardar.
      </p>
    </div>
  </div>
</div>

<style>
  .cfg-root {
    height: 100%;
    overflow: auto;
    background: #0d0d12;
    color: #e6e6ec;
    padding: 28px 24px 80px;
  }
  .cfg-wrap {
    max-width: 1080px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  .cfg-head {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .cfg-head h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
  }
  .back {
    background: transparent;
    border: 1px solid #2a2a36;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 7px 14px;
    cursor: pointer;
    font-size: 13px;
  }
  .back:hover { background: #1c1c26; color: #fff; }

  .cfg-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
    align-items: start;
  }
  @media (max-width: 880px) {
    .cfg-grid { grid-template-columns: 1fr; }
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .block {
    background: #15151c;
    border: 1px solid #22222c;
    border-radius: 12px;
    padding: 18px 20px;
  }
  .block h2 {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 600;
    color: #f3a951;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .hint {
    margin: 0 0 12px;
    color: #888892;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .hint code {
    background: #0d0d12;
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 11.5px;
    color: #d8d8e0;
  }
  .hint em { font-style: normal; color: #f3a951; }

  .radio-group { display: flex; flex-direction: column; gap: 6px; }
  .radio {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 8px;
    cursor: pointer;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    font-size: 13.5px;
  }
  .radio:hover { border-color: #2a2a36; }
  .radio input { accent-color: #f3a951; }

  .key-row { display: flex; gap: 8px; }
  .key-row input {
    flex: 1;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    color: #fff;
    padding: 9px 12px;
    border-radius: 8px;
    font-size: 13px;
    font-family: ui-monospace, monospace;
  }
  .key-row input:focus { border-color: #f3a951; outline: none; }
  .reveal {
    background: #1c1c26;
    border: 1px solid #2a2a36;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 0 12px;
    cursor: pointer;
    font-size: 14px;
  }
  .reveal:hover { background: #22222c; }

  .mode-group { display: flex; flex-direction: column; gap: 8px; }
  .mode-card {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 11px 14px;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 10px;
    cursor: pointer;
  }
  .mode-card:hover { border-color: #2a2a36; }
  .mode-card.sel { border-color: #f3a951; background: #1a1410; }
  .mode-card input { margin-top: 3px; accent-color: #f3a951; }
  .mode-card div { display: flex; flex-direction: column; gap: 2px; }
  .mode-card strong { font-size: 13px; color: #fff; }
  .mode-card span { font-size: 12px; color: #a0a0aa; }

  .rd-linked {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 14px;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    font-size: 13.5px;
  }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .dot.ok { background: #6cd37a; }
  .rd-linked .btn-sec { margin-left: auto; }

  .btn-link {
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    display: block;
    width: 100%;
  }
  .btn-link:hover:not(:disabled) { background: #ffb86b; }
  .btn-link:disabled { opacity: 0.5; cursor: default; }

  .link-tiny {
    margin-top: 8px;
    background: transparent;
    border: 0;
    color: #888892;
    font-size: 12px;
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }
  .link-tiny:hover { color: #d8d8e0; }

  .rd-flow {
    display: flex; flex-direction: column; gap: 10px;
    padding: 14px;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    border-radius: 10px;
  }
  .rd-step { margin: 0; font-size: 13px; color: #c8c8d0; }
  .rd-link {
    background: transparent; border: 0;
    color: #f3a951; text-decoration: underline;
    cursor: pointer; font-size: 13px; padding: 0;
  }
  .rd-link:hover { color: #ffb86b; }
  .rd-code {
    text-align: center;
    background: #1a1410;
    border: 1px solid #f3a951;
    border-radius: 10px;
    padding: 18px 12px;
    font-family: ui-monospace, "SF Mono", monospace;
    font-size: 28px;
    font-weight: 700;
    color: #ffb86b;
    letter-spacing: 0.18em;
    user-select: all;
  }
  .rd-wait {
    margin: 0;
    display: flex; align-items: center; gap: 8px;
    color: #a0a0aa; font-size: 12.5px;
  }
  .rd-wait em {
    font-style: normal; color: #d8d8e0;
    font-variant-numeric: tabular-nums;
  }
  .spinner {
    width: 12px; height: 12px;
    border: 2px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .btn-sec {
    background: #2a2a36; color: #d8d8e0;
    border: 0; border-radius: 8px;
    padding: 8px 14px; font-size: 12.5px; cursor: pointer;
  }
  .btn-sec:hover { background: #34343f; }

  .err, .ok {
    margin: 10px 0 0; font-size: 12.5px;
  }
  .err { color: #ff7373; }
  .ok { color: #9be38a; }

  .actions {
    display: flex; flex-direction: column;
    align-items: flex-start; gap: 8px;
    padding-top: 4px;
  }
  .btn-save {
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 12px 28px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-save:hover:not(:disabled) { background: #ffb86b; }
  .btn-save:disabled { opacity: 0.4; cursor: default; }
  .hint-actions {
    margin: 0; color: #6e6e78; font-size: 12px;
  }

  .upd-line {
    margin: 0 0 8px;
    font-size: 13px;
    color: #c8c8d0;
    display: flex; align-items: center; gap: 8px;
  }
  .upd-line strong { color: #f3a951; }
  .upd-ok { color: #9be38a; }
  .upd-notes {
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    padding: 10px 12px;
    max-height: 140px;
    overflow: auto;
    font-size: 11.5px;
    color: #c8c8d0;
    margin: 0 0 10px;
    white-space: pre-wrap;
    font-family: ui-monospace, monospace;
  }
  .bar {
    width: 100%;
    height: 6px;
    background: #1f1f28;
    border-radius: 3px;
    overflow: hidden;
    margin: 4px 0 6px;
  }
  .bar-fill {
    height: 100%;
    background: linear-gradient(90deg, #f3a951, #ffb86b);
    transition: width 0.2s ease;
  }
  .upd-mb {
    margin: 0;
    color: #888892;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
