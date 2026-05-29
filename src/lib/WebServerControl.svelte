<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type WebStatus = {
    running: boolean;
    ip: string | null;
    port: number | null;
    url: string | null;
  };

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

  onMount(refresh);

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
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
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
    if (e.key === "Escape") { e.preventDefault(); open = false; }
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
    <div class="head">
      <span class="dot" class:on={st.running}></span>
      <span class="title">Servidor web</span>
      <span class="state">{st.running ? "Activo" : "Detenido"}</span>
    </div>

    {#if st.running && st.url}
      <div class="url-row">
        <code class="url">{st.url}</code>
        <button class="copy" onclick={copy} title="Copiar">
          {copied ? "✓" : "⧉"}
        </button>
      </div>
      <p class="hint">Entrar desde otro equipo en la misma red.</p>
    {:else}
      <div class="port-row">
        <label for="ws-port">Puerto</label>
        <input
          id="ws-port"
          type="number"
          min="1024"
          max="65535"
          bind:value={portInput}
        />
      </div>
    {/if}

    <div class="actions">
      {#if st.running}
        <button class="btn-stop" onclick={stop} disabled={busy}>Detener</button>
      {:else}
        <button class="btn-start" onclick={start} disabled={busy}>Iniciar</button>
      {/if}
    </div>

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
  .state { font-size: 11px; color: #a0a0aa; }

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
  .hint { margin: 6px 0 10px; font-size: 11px; color: #6e6e78; }

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
</style>
