<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { WebStatus } from "$lib/tipos";
  import RemoteQr from "$lib/RemoteQr.svelte";
  import { redInfo } from "$lib/cast.svelte";

  let {
    open = $bindable(false),
    running = $bindable(false),
    url = $bindable<string | null>(null),
  } = $props<{
    open?: boolean;
    running?: boolean;
    url?: string | null;
  }>();

  let st: WebStatus = $state({ running: false, ip: null, port: null, url: null });
  let portInput = $state(8080);
  let busy = $state(false);
  let copied = $state(false);
  let err = $state<string | null>(null);
  // Firewall activo en este equipo: el celular puede no llegar al puerto.
  let firewall = $state("");
  // El puerto del control ya está abierto en el firewall: sin aviso.
  let controlAbierto = $state(false);
  let copiedCmd = $state(false);

  // Comando para abrir el puerto del control, según el firewall que haya.
  const comandoFirewall = $derived.by(() => {
    const p = st.port ?? portInput;
    if (controlAbierto) return "";
    switch (firewall) {
      case "Windows Defender":
        return `netsh advfirewall firewall add rule name="Kutral control" dir=in action=allow protocol=TCP localport=${p}`;
      case "firewalld":
        return `sudo firewall-cmd --permanent --add-port=${p}/tcp && sudo firewall-cmd --reload`;
      case "ufw":
        return `sudo ufw allow ${p}/tcp`;
      default:
        return "";
    }
  });
  let card: HTMLDivElement | null = $state(null);

  async function refresh() {
    try {
      st = await invoke<WebStatus>("web_server_status");
      running = st.running;
      url = st.url;
      if (st.port) portInput = st.port;
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    void refresh();
    void redInfo().then((r) => (firewall = r?.firewall ?? ""));
  });

  $effect(() => {
    const p = st.port;
    if (!firewall || !p) return;
    invoke<boolean>("firewall_abierto", { puerto: p })
      .then((a) => (controlAbierto = a))
      .catch(() => {});
  });

  $effect(() => {
    if (open) {
      refresh();
      window.addEventListener("mousedown", onOutside);
    } else {
      window.removeEventListener("mousedown", onOutside);
    }
    return () => window.removeEventListener("mousedown", onOutside);
  });

  function onOutside(e: MouseEvent) {
    if (!card) return;
    if (card.contains(e.target as Node)) return;
    open = false;
  }

  async function start() {
    if (busy) return;
    busy = true; err = null;
    try {
      st = await invoke<WebStatus>("web_server_start", { port: portInput });
      running = st.running;
      url = st.url;
      // Cierra el popover al iniciar OK — el header refleja estado activo.
      open = false;
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    if (busy) return;
    busy = true; err = null;
    try {
      await invoke("web_server_stop");
      await refresh();
      // Cierra el popover al detener OK.
      open = false;
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }

  async function copyCmd() {
    if (!comandoFirewall) return;
    try {
      await navigator.clipboard.writeText(comandoFirewall);
      copiedCmd = true;
      setTimeout(() => (copiedCmd = false), 1500);
    } catch {}
  }

  async function copy() {
    if (!st.url) return;
    try {
      await navigator.clipboard.writeText(st.url);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {}
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); open = false; }
  }
</script>

{#if open}
  <div
    class="ws-card"
    bind:this={card}
    role="dialog"
    aria-label="Servidor web"
    onkeydown={onKey}
    tabindex="-1"
  >
    {#if st.running}
      <div class="head">
        <span class="dot on"></span>
        <span class="title">Servidor web activo</span>
      </div>
      {#if st.url}
        <!-- La URL lleva el token: a mano no se copia. El QR es la vía. -->
        <div class="qr">
          <RemoteQr url={st.url} label="Escanea con el celular" size={160} />
        </div>
        <div class="url-row">
          <code class="url">{st.url}</code>
          <button data-nav class="copy" onclick={copy} title="Copiar">
            {copied ? "✓" : "⧉"}
          </button>
        </div>
      {/if}
      {#if comandoFirewall}
        <p class="fw">
          ¿El celular no abre la página? El firewall ({firewall}) puede estar
          tapando el puerto {st.port}. Ábrelo una vez con
          {firewall === "Windows Defender" ? "este comando en una terminal de administrador" : "este comando"}:
        </p>
        <div class="url-row">
          <code class="url">{comandoFirewall}</code>
          <button data-nav class="copy" onclick={copyCmd} title="Copiar comando">
            {copiedCmd ? "✓" : "⧉"}
          </button>
        </div>
      {/if}
      <div class="actions">
        <button data-nav class="btn-stop" onclick={stop} disabled={busy}>Detener</button>
      </div>
    {:else}
      <div class="head">
        <span class="dot"></span>
        <span class="title">Servidor web</span>
      </div>
      <div class="port-row">
        <label for="ws-port">Puerto</label>
        <input
          id="ws-port"
          data-nav
          type="number"
          min="1024"
          max="65535"
          bind:value={portInput}
        />
      </div>
      <div class="actions">
        <button data-nav class="btn-start" onclick={start} disabled={busy}>Iniciar</button>
      </div>
    {/if}

    {#if err}
      <p class="err">{err}</p>
    {/if}
  </div>
{/if}

<style>
  .ws-card {
    position: absolute;
    top: 38px;
    right: 0;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 10px;
    padding: 12px 14px;
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.6);
    z-index: 1100;
    min-width: 280px;
    color: #e6e6ec;
  }
  .head {
    display: flex; align-items: center; gap: 8px;
    margin-bottom: 10px;
  }
  .dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: #c44; display: inline-block;
  }
  .dot.on { background: #6cd37a; box-shadow: 0 0 6px #6cd37a; }
  .title { font-size: 13px; font-weight: 600; flex: 1; }

  .url-row {
    display: flex; align-items: center; gap: 6px;
    background: #0b0b0f; border: 1px solid #2a2a36;
    border-radius: 6px; padding: 6px 8px;
  }
  .url {
    flex: 1; font-size: 12px; color: #f3a951;
    font-family: ui-monospace, monospace;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .copy {
    background: transparent; border: 0; color: #c8c8d0;
    cursor: pointer; font-size: 13px; padding: 2px 6px;
    border-radius: 4px;
  }
  .copy:hover { background: #1c1c26; color: #fff; }

  .port-row {
    display: flex; align-items: center; gap: 8px;
    margin-bottom: 10px;
  }
  .port-row label { font-size: 12px; color: #a0a0aa; }
  .port-row input {
    flex: 1; background: #0b0b0f; border: 1px solid #2a2a36;
    border-radius: 6px; padding: 5px 8px;
    color: #e6e6ec; font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .port-row input:focus { outline: 1px solid #f3a951; }

  .actions { display: flex; justify-content: flex-end; }
  .btn-start, .btn-stop {
    border: 0; border-radius: 6px;
    padding: 6px 14px; font-size: 12.5px;
    cursor: pointer; font-weight: 500;
  }
  .btn-start { background: #2a7a3e; color: #fff; }
  .btn-start:hover { background: #34924a; }
  .btn-stop { background: #c42b1c; color: #fff; }
  .btn-stop:hover { background: #d8392b; }
  .btn-start:disabled, .btn-stop:disabled { opacity: 0.5; cursor: not-allowed; }
  .err { margin: 8px 0 0; font-size: 11px; color: #d8392b; }
  .qr { display: flex; justify-content: center; margin-bottom: 4px; }
  .fw { margin: 10px 0 6px; font-size: 11.5px; line-height: 1.4; color: #c9c9d4; max-width: 320px; }
</style>
