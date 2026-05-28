<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type Network = {
    ssid: string;
    signal: number;
    secured: boolean;
    in_use: boolean;
  };

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let scanning = $state(false);
  let nets = $state<Network[]>([]);
  let selected = $state<Network | null>(null);
  let password = $state("");
  let connecting = $state(false);
  let err = $state("");
  let ok = $state("");
  let firstBtn = $state<HTMLButtonElement | null>(null);
  let pwdInput = $state<HTMLInputElement | null>(null);

  async function scan() {
    scanning = true;
    err = "";
    try {
      nets = await invoke<Network[]>("wifi_scan");
    } catch (e) {
      err = String(e);
    } finally {
      scanning = false;
    }
  }

  $effect(() => {
    if (open) {
      err = "";
      ok = "";
      selected = null;
      password = "";
      scan().then(() => setTimeout(() => firstBtn?.focus(), 30));
    }
  });

  function close() {
    open = false;
  }

  function pickNet(n: Network) {
    selected = n;
    password = "";
    err = "";
    ok = "";
    if (n.secured) {
      setTimeout(() => pwdInput?.focus(), 30);
    } else {
      doConnect();
    }
  }

  async function doConnect() {
    if (!selected) return;
    connecting = true;
    err = "";
    ok = "";
    try {
      await invoke("wifi_connect", {
        ssid: selected.ssid,
        password: selected.secured ? password : null,
      });
      ok = `Conectado a ${selected.ssid}`;
      selected = null;
      password = "";
      await scan();
    } catch (e) {
      err = String(e);
    } finally {
      connecting = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (selected) { selected = null; password = ""; }
      else close();
    }
  }

  function signalBars(s: number): string {
    if (s >= 75) return "▮▮▮▮";
    if (s >= 50) return "▮▮▮▯";
    if (s >= 25) return "▮▮▯▯";
    return "▮▯▯▯";
  }
</script>

{#if open}
  <div
    class="wifi-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Gestión de WiFi"
    onkeydown={onKey}
    tabindex="-1"
  >
    <div class="wifi-card">
      <header class="wifi-head">
        <h2>Conectarse a WiFi</h2>
        <button class="wifi-close" onclick={close} aria-label="Cerrar">✕</button>
      </header>

      {#if selected && selected.secured}
        <div class="wifi-auth">
          <p class="net-name">{selected.ssid}</p>
          <label>
            <span>Contraseña</span>
            <input
              bind:this={pwdInput}
              bind:value={password}
              type="password"
              placeholder="••••••••"
              autocomplete="off"
              onkeydown={(e) => { if (e.key === "Enter") doConnect(); }}
            />
          </label>
          <div class="wifi-actions">
            <button onclick={() => { selected = null; password = ""; }} class="btn-sec">
              Atrás
            </button>
            <button
              onclick={doConnect}
              disabled={!password || connecting}
              class="btn-pri"
            >
              {connecting ? "Conectando…" : "Conectar"}
            </button>
          </div>
        </div>
      {:else}
        <div class="wifi-toolbar">
          <button class="btn-sec" onclick={scan} disabled={scanning} bind:this={firstBtn}>
            {scanning ? "Buscando…" : "Refrescar"}
          </button>
        </div>
        <ul class="wifi-list">
          {#if scanning && nets.length === 0}
            <li class="empty">Buscando redes…</li>
          {:else if nets.length === 0}
            <li class="empty">Sin redes</li>
          {/if}
          {#each nets as n (n.ssid)}
            <li>
              <button class="net-row" onclick={() => pickNet(n)} disabled={connecting}>
                <span class="net-ssid">
                  {n.ssid}
                  {#if n.in_use}<em class="tag-current">conectada</em>{/if}
                </span>
                <span class="net-meta">
                  {#if n.secured}<span class="lock" title="Protegida">🔒</span>{/if}
                  <span class="bars" title="{n.signal}%">{signalBars(n.signal)}</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if err}<p class="err">{err}</p>{/if}
      {#if ok}<p class="ok">{ok}</p>{/if}
    </div>
  </div>
{/if}

<style>
  .wifi-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9998;
    backdrop-filter: blur(4px);
  }
  .wifi-card {
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 12px;
    width: 420px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    color: #e6e6ec;
    overflow: hidden;
  }
  .wifi-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 18px;
    border-bottom: 1px solid #22222c;
  }
  .wifi-head h2 { margin: 0; font-size: 16px; font-weight: 600; }
  .wifi-close {
    background: transparent; border: 0; color: #a0a0aa; font-size: 18px;
    cursor: pointer; padding: 4px 8px; border-radius: 6px;
  }
  .wifi-close:hover { background: #22222c; color: #fff; }
  .wifi-toolbar { padding: 12px 18px 0; }
  .wifi-list {
    list-style: none; margin: 0; padding: 8px 8px 12px;
    overflow: auto; flex: 1;
  }
  .net-row {
    width: 100%;
    display: flex; justify-content: space-between; align-items: center;
    background: transparent; border: 0; color: #e6e6ec;
    padding: 12px 14px; border-radius: 8px; cursor: pointer;
    font-size: 14px; text-align: left;
  }
  .net-row:hover, .net-row:focus {
    background: #1f1f28; outline: none;
  }
  .net-row:focus { outline: 2px solid #f3a951; outline-offset: -2px; }
  .net-row:disabled { opacity: 0.5; cursor: default; }
  .net-ssid { display: flex; align-items: center; gap: 8px; }
  .tag-current {
    font-style: normal; font-size: 11px;
    background: #2a3d22; color: #9be38a;
    padding: 2px 6px; border-radius: 4px;
  }
  .net-meta {
    display: flex; align-items: center; gap: 10px;
    color: #a0a0aa; font-size: 13px;
  }
  .bars { font-family: ui-monospace, monospace; letter-spacing: -1px; color: #f3a951; }
  .empty {
    padding: 24px; text-align: center; color: #6e6e78;
    font-size: 14px;
  }
  .wifi-auth {
    padding: 18px;
    display: flex; flex-direction: column; gap: 14px;
  }
  .net-name {
    margin: 0; font-size: 15px; font-weight: 600; color: #f3a951;
  }
  .wifi-auth label {
    display: flex; flex-direction: column; gap: 6px;
    font-size: 13px; color: #a0a0aa;
  }
  .wifi-auth input {
    background: #0d0d12; border: 1px solid #2a2a36;
    color: #fff; padding: 10px 12px; border-radius: 8px;
    font-size: 14px;
  }
  .wifi-auth input:focus { border-color: #f3a951; outline: none; }
  .wifi-actions { display: flex; gap: 10px; justify-content: flex-end; }
  .btn-pri, .btn-sec {
    border: 0; border-radius: 8px; padding: 9px 18px;
    font-size: 13px; cursor: pointer; font-weight: 500;
  }
  .btn-pri { background: #f3a951; color: #1a1208; }
  .btn-pri:hover:not(:disabled) { background: #ffb86b; }
  .btn-pri:disabled { opacity: 0.5; cursor: default; }
  .btn-sec { background: #2a2a36; color: #d8d8e0; }
  .btn-sec:hover:not(:disabled) { background: #34343f; }
  .err, .ok {
    margin: 0; padding: 10px 18px;
    border-top: 1px solid #22222c;
    font-size: 13px;
  }
  .err { color: #ff7373; }
  .ok { color: #9be38a; }
</style>
