<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type BrightnessState = { percent: number; available: boolean };

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let br: BrightnessState = $state({ percent: 100, available: false });
  let card: HTMLDivElement | null = $state(null);
  let slider: HTMLInputElement | null = $state(null);

  async function refresh() {
    try {
      br = await invoke<BrightnessState>("brightness_get");
    } catch {
      br = { percent: 100, available: false };
    }
  }

  onMount(refresh);

  $effect(() => {
    if (open) {
      refresh();
      setTimeout(() => slider?.focus(), 30);
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

  async function setPct(v: number) {
    br.percent = Math.max(5, Math.min(100, Math.round(v)));
    try { await invoke("brightness_set", { percent: br.percent }); } catch {}
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); open = false; }
  }
</script>

{#if open}
  <div
    class="br-card"
    bind:this={card}
    role="dialog"
    aria-label="Brillo"
    onkeydown={onKey}
    tabindex="-1"
  >
    <div class="row">
      <span class="ico">{br.percent < 33 ? "🌒" : br.percent < 66 ? "🌓" : "🌕"}</span>
      <input
        bind:this={slider}
        type="range"
        min="5"
        max="100"
        step="1"
        value={br.percent}
        disabled={!br.available}
        oninput={(e) => setPct(+(e.target as HTMLInputElement).value)}
      />
      <span class="pct">{br.percent}%</span>
    </div>
    {#if !br.available}
      <p class="hint">Brillo no controlable (brightnessctl)</p>
    {/if}
  </div>
{/if}

<style>
  .br-card {
    position: absolute;
    top: 38px;
    right: 0;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 10px;
    padding: 12px 14px;
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.6);
    z-index: 1100;
    min-width: 260px;
  }
  .row { display: flex; align-items: center; gap: 10px; }
  .ico { font-size: 16px; line-height: 1; }
  input[type="range"] {
    flex: 1;
    accent-color: #f3a951;
    cursor: pointer;
  }
  input:disabled { opacity: 0.4; cursor: not-allowed; }
  .pct {
    width: 36px; text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 12px; color: #a0a0aa;
  }
  .hint { margin: 8px 0 0; color: #6e6e78; font-size: 11.5px; }
</style>
