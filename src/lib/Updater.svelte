<script lang="ts">
  import { onMount } from "svelte";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { isKutralOs } from "$lib/os";

  type Stage = "idle" | "available" | "downloading" | "installing" | "ready" | "error" | "uptodate";

  let stage = $state<Stage>("idle");
  let update = $state<Update | null>(null);
  let kutral = $state(false);
  let downloaded = $state(0);
  let total = $state(0);
  let errMsg = $state("");
  let primaryBtn = $state<HTMLButtonElement | null>(null);

  onMount(async () => {
    try {
      kutral = await isKutralOs();
    } catch {
      kutral = false;
    }
    try {
      const u = await check();
      if (!u) {
        stage = "uptodate";
        return;
      }
      update = u;
      stage = "available";
      if (kutral) {
        await runUpdate();
      } else {
        setTimeout(() => primaryBtn?.focus(), 30);
      }
    } catch (e) {
      console.warn("[updater] check fail", e);
      stage = "error";
      errMsg = String(e);
    }
  });

  async function runUpdate() {
    if (!update) return;
    stage = "downloading";
    downloaded = 0;
    total = 0;
    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            break;
          case "Finished":
            stage = "installing";
            break;
        }
      });
      stage = "ready";
      await relaunch();
    } catch (e) {
      console.error("[updater] install fail", e);
      stage = "error";
      errMsg = String(e);
    }
  }

  function dismiss() {
    stage = "idle";
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && stage === "available" && !kutral) {
      e.preventDefault();
      dismiss();
    }
  }

  function pct(): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((downloaded / total) * 100));
  }

  function mb(bytes: number): string {
    return (bytes / 1024 / 1024).toFixed(1);
  }
</script>

{#if stage === "available" && !kutral}
  <div
    class="up-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Actualización disponible"
    onkeydown={onKey}
    tabindex="-1"
  >
    <div class="up-card">
      <h2>Actualización disponible</h2>
      <p class="ver">Kütral <strong>{update?.version}</strong></p>
      {#if update?.body}
        <pre class="notes">{update.body}</pre>
      {/if}
      <div class="actions">
        <button class="btn-later" onclick={dismiss}>Después</button>
        <button class="btn-now" bind:this={primaryBtn} onclick={runUpdate}>
          Actualizar ahora
        </button>
      </div>
    </div>
  </div>
{:else if stage === "downloading" || stage === "installing" || stage === "ready"}
  <div class="up-backdrop" role="status" aria-live="polite">
    <div class="up-card">
      <h2>
        {#if stage === "downloading"}Descargando actualización
        {:else if stage === "installing"}Instalando…
        {:else}Reiniciando…{/if}
      </h2>
      {#if stage === "downloading" && total > 0}
        <div class="bar">
          <div class="bar-fill" style="width: {pct()}%"></div>
        </div>
        <p class="mb">{mb(downloaded)} / {mb(total)} MB · {pct()}%</p>
      {:else if stage === "downloading"}
        <p class="mb">{mb(downloaded)} MB</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .up-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    backdrop-filter: blur(6px);
  }
  .up-card {
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 12px;
    padding: 32px 36px;
    min-width: 380px;
    max-width: 520px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.7);
    color: #e6e6ec;
    text-align: center;
  }
  .up-card h2 {
    margin: 0 0 10px;
    font-size: 18px;
    font-weight: 600;
  }
  .ver {
    margin: 0 0 16px;
    color: #a0a0aa;
    font-size: 14px;
  }
  .ver strong { color: #f3a951; }
  .notes {
    text-align: left;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    padding: 12px 14px;
    max-height: 220px;
    overflow: auto;
    font-size: 12px;
    color: #c8c8d0;
    margin: 0 0 20px;
    white-space: pre-wrap;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
  }
  .actions {
    display: flex;
    gap: 12px;
    justify-content: center;
  }
  .btn-later, .btn-now {
    border: 0;
    border-radius: 8px;
    padding: 10px 22px;
    font-size: 14px;
    cursor: pointer;
    font-weight: 500;
  }
  .btn-later {
    background: #2a2a36;
    color: #d8d8e0;
  }
  .btn-later:hover { background: #34343f; }
  .btn-now {
    background: #f3a951;
    color: #1a1208;
  }
  .btn-now:hover { background: #ffb86b; }
  .btn-now:focus, .btn-later:focus {
    outline: 2px solid #f3a951;
    outline-offset: 2px;
  }
  .bar {
    width: 100%;
    height: 8px;
    background: #1f1f28;
    border-radius: 4px;
    overflow: hidden;
    margin: 18px 0 10px;
  }
  .bar-fill {
    height: 100%;
    background: linear-gradient(90deg, #f3a951, #ffb86b);
    transition: width 0.2s ease;
  }
  .mb {
    margin: 6px 0 0;
    color: #a0a0aa;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
</style>
