<script lang="ts">
  // Pill "volver a la reproducción". Esc en el video ya NO cierra: suspende
  // (película pausada / canal IPTV sonando de fondo) y devuelve el teclado a
  // los menús. Esta pill es la puerta de vuelta, visible en cualquier ruta.
  // Reproducir algo nuevo reemplaza la sesión (el backend limpia el flag).
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { player, setSession, type MpvSession } from "$lib/playerState.svelte";

  let unlisten: UnlistenFn[] = [];
  let poll: ReturnType<typeof setInterval> | null = null;
  let busy = $state(false);

  async function refresh() {
    try {
      setSession(await invoke<MpvSession>("mpv_session"));
    } catch {
      setSession(null);
    }
  }

  // Vuelve al video: muestra la superficie de mpv y despausa.
  async function volver() {
    if (busy) return;
    busy = true;
    try {
      await invoke("mpv_resume");
    } catch (e) {
      console.warn("[resume]", e);
      await refresh(); // la sesión pudo morir: que la pill se vaya sola
    }
    busy = false;
  }

  // ✕ : cerrar del todo lo que quedó esperando.
  async function cerrar() {
    try {
      await invoke("mpv_stop");
    } catch {
      /* tolerado */
    }
    await refresh();
  }

  // Tecla dedicada: P retoma desde cualquier pantalla (sin robar el foco de
  // las flechas, que siguen navegando los carruseles).
  function onKey(e: KeyboardEvent) {
    if (!player.suspended || player.playing) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    if (e.key === "p" || e.key === "P") {
      e.preventDefault();
      void volver();
    }
  }

  function fmt(s: number): string {
    if (!isFinite(s) || s <= 0) return "0:00";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
    return `${h > 0 ? `${h}:` : ""}${mm}:${String(sec).padStart(2, "0")}`;
  }

  const pct = $derived(
    player.duration > 0 ? Math.min(100, (player.pos / player.duration) * 100) : 0,
  );

  onMount(() => {
    void refresh();
    // El backend avisa al suspender/retomar/cerrar; mpv:state cubre el arranque
    // de una reproducción nueva (que borra la sesión anterior).
    void listen<boolean>("mpv:suspended", () => void refresh()).then((u) => unlisten.push(u));
    void listen<boolean>("mpv:state", () => void refresh()).then((u) => unlisten.push(u));
    // Refresco lento del tiempo mostrado (IPTV de fondo sigue avanzando).
    poll = setInterval(() => {
      if (!player.playing) void refresh();
    }, 10000);
    window.addEventListener("keydown", onKey);
  });

  onDestroy(() => {
    unlisten.forEach((u) => u());
    unlisten = [];
    if (poll) clearInterval(poll);
    window.removeEventListener("keydown", onKey);
  });
</script>

{#if player.suspended && !player.playing}
  <div class="pill-wrap">
    <div class="pill">
      <button class="main" onclick={volver} disabled={busy} title="Volver a la reproducción (P)">
        <span class="play">▶</span>
        <span class="txt">
          <span class="title">{player.title || "Reproducción en pausa"}</span>
          <span class="sub">
            {#if player.live}
              <span class="dot"></span> En vivo de fondo · vuelve al canal
            {:else}
              {fmt(player.pos)}{player.duration > 0 ? ` / ${fmt(player.duration)}` : ""} · en pausa
            {/if}
          </span>
        </span>
        <span class="key">P</span>
      </button>
      <button class="close" onclick={cerrar} title="Cerrar la reproducción">✕</button>
    </div>
    {#if !player.live && pct > 0}
      <div class="bar"><div class="fill" style:width={`${pct}%`}></div></div>
    {/if}
  </div>
{/if}

<style>
  .pill-wrap {
    position: fixed;
    left: 50%;
    bottom: 22px;
    transform: translateX(-50%);
    z-index: 900;
    width: min(560px, calc(100vw - 48px));
    border-radius: 16px;
    overflow: hidden;
    background: rgba(18, 15, 10, 0.94);
    border: 1px solid rgba(249, 115, 22, 0.45);
    box-shadow: 0 18px 40px rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(8px);
  }
  .pill {
    display: flex;
    align-items: stretch;
  }
  .main {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    background: transparent;
    border: 0;
    color: #e8e0d0;
    font: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .main:hover,
  .main:focus-visible {
    background: rgba(249, 115, 22, 0.14);
    outline: none;
  }
  .main:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .play {
    font-size: 20px;
    color: #f97316;
    line-height: 1;
  }
  .txt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-size: 15px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 12px;
    color: #b9b3a6;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #ef4444;
  }
  .key {
    margin-left: auto;
    font-size: 11px;
    color: #b9b3a6;
    border: 1px solid rgba(185, 179, 166, 0.4);
    border-radius: 5px;
    padding: 2px 7px;
    flex: 0 0 auto;
  }
  .close {
    flex: 0 0 auto;
    padding: 0 14px;
    background: transparent;
    border: 0;
    border-left: 1px solid rgba(255, 255, 255, 0.08);
    color: #b9b3a6;
    font-size: 14px;
    cursor: pointer;
  }
  .close:hover,
  .close:focus-visible {
    background: rgba(239, 68, 68, 0.18);
    color: #fff;
    outline: none;
  }
  .bar {
    height: 3px;
    background: rgba(255, 255, 255, 0.1);
  }
  .fill {
    height: 100%;
    background: #f97316;
  }
</style>
