<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type AudioState = { volume: number; muted: boolean; available: boolean };

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let audio: AudioState = $state({ volume: 0, muted: false, available: false });
  let poll: number | null = null;
  let card: HTMLDivElement | null = $state(null);
  let slider: HTMLInputElement | null = $state(null);

  async function refresh() {
    try {
      audio = await invoke<AudioState>("audio_get");
    } catch {
      audio = { volume: 0, muted: false, available: false };
    }
  }

  onMount(() => {
    refresh();
    poll = window.setInterval(refresh, 4000);
  });
  onDestroy(() => { if (poll !== null) clearInterval(poll); });

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

  async function setVol(v: number) {
    audio.volume = Math.max(0, Math.min(150, Math.round(v)));
    if (audio.muted && v > 0) audio.muted = false;
    try { await invoke("audio_set", { volume: audio.volume }); } catch {}
    if (!audio.muted) {
      try { await invoke("audio_set_mute", { muted: false }); } catch {}
    }
  }
  async function toggleMute() {
    audio.muted = !audio.muted;
    try { await invoke("audio_set_mute", { muted: audio.muted }); } catch {}
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); open = false; }
    if (e.key === "m" || e.key === "M") { e.preventDefault(); toggleMute(); }
  }
</script>

{#if open}
  <div
    class="vol-card"
    bind:this={card}
    role="dialog"
    aria-label="Volumen"
    onkeydown={onKey}
    tabindex="-1"
  >
    <div class="row">
      <button class="mute" onclick={toggleMute} title="Mute (M)">
        {audio.muted || audio.volume === 0 ? "🔇" : audio.volume < 33 ? "🔈" : audio.volume < 66 ? "🔉" : "🔊"}
      </button>
      <input
        bind:this={slider}
        type="range"
        min="0"
        max="100"
        step="1"
        value={audio.muted ? 0 : audio.volume}
        oninput={(e) => setVol(+(e.target as HTMLInputElement).value)}
      />
      <span class="pct">{audio.muted ? 0 : audio.volume}%</span>
    </div>
    {#if !audio.available}
      <p class="hint">Audio no detectado (wpctl)</p>
    {/if}
  </div>
{/if}

<style>
  .vol-card {
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
  .mute {
    background: transparent; border: 0;
    font-size: 18px; cursor: pointer; padding: 0; line-height: 1;
  }
  input[type="range"] {
    flex: 1;
    accent-color: #f3a951;
    cursor: pointer;
  }
  .pct {
    width: 36px; text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 12px; color: #a0a0aa;
  }
  .hint { margin: 8px 0 0; color: #6e6e78; font-size: 11.5px; }
</style>
