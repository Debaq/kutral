<script lang="ts">
  // Cola de descargas locales (plan B ante el bloqueo DMCA del debrid).
  // Popover del encabezado, mismo patrón que Notifications.
  import {
    torrents,
    refreshQueue,
    startQueuePoll,
    pauseTorrent,
    resumeTorrent,
    removeTorrent,
    torrentDefaultDir,
    fmtBytes,
    fmtSpeed,
    fmtEta,
    type TorrentStatus,
  } from "$lib/torrents.svelte";
  import { config } from "$lib/config.svelte";
  import { olvidarPorHash } from "$lib/descargas.svelte";

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let card: HTMLDivElement | null = $state(null);
  let dir = $state("");
  // Id con confirmación de borrado pendiente (borrar archivo es irreversible).
  let confirmId = $state<number | null>(null);

  $effect(() => {
    if (open) {
      // Al abrir, refresca aunque el poll esté dormido (todo terminado).
      void refreshQueue();
      // La elegida por el usuario manda; si no eligió, la que propone el sistema.
      if (config.torrentDir.trim()) dir = config.torrentDir.trim();
      else void torrentDefaultDir().then((d: string) => (dir = d)).catch(() => {});
      window.addEventListener("mousedown", onOutside);
    } else {
      confirmId = null;
      window.removeEventListener("mousedown", onOutside);
    }
    return () => window.removeEventListener("mousedown", onOutside);
  });

  function onOutside(e: MouseEvent) {
    if (!card) return;
    if (card.contains(e.target as Node)) return;
    open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    }
  }

  async function togglePause(t: TorrentStatus) {
    try {
      if (t.paused) await resumeTorrent(t.id);
      else await pauseTorrent(t.id);
      startQueuePoll();
    } catch {
      /* el refresh siguiente muestra el estado real */
    }
  }

  // Borra de la cola. Si aún no terminó, borra también el archivo a medias
  // (no sirve de nada); si terminó, el archivo se conserva.
  async function quitar(t: TorrentStatus) {
    try {
      await removeTorrent(t.id, !t.finished);
    } catch {
      /* ya no existía */
    }
    // A medias se borra el archivo: ya no hay copia local que ofrecer. Si
    // terminó, el archivo se queda y la ficha lo sigue conociendo — que es
    // justo el caso de "la bajé y me olvidé".
    if (!t.finished) void olvidarPorHash(t.info_hash);
    confirmId = null;
    await refreshQueue();
  }

  function estado(t: TorrentStatus): string {
    if (t.state === "error") return t.error || "Error";
    if (t.finished) return "Lista";
    if (t.paused) return "En pausa";
    if (t.state === "initializing") return "Buscando peers…";
    return `${fmtSpeed(t.download_bps)} · ${t.peers} peers · ${fmtEta(t.eta_secs)}`;
  }
</script>

{#if open}
  <div
    class="tq-card"
    bind:this={card}
    role="dialog"
    aria-label="Descargas"
    onkeydown={onKey}
    tabindex="-1"
  >
    <header class="tq-head">
      <strong>Descargas</strong>
      {#if dir}<span class="tq-dir" title={dir}>{dir}</span>{/if}
    </header>

    {#if !torrents.list.length}
      <p class="tq-empty">
        {torrents.loaded ? "Sin descargas" : "Cargando…"}
      </p>
    {:else}
      <ul class="tq-list">
        {#each torrents.list as t (t.id)}
          <li class="tq-item" class:err={t.state === "error"}>
            <div class="tq-top">
              <span class="tq-title" title={t.name}>{t.title || t.name}</span>
              <span class="tq-pct">{t.finished ? "100%" : `${Math.round(t.pct)}%`}</span>
            </div>
            <div class="tq-bar">
              <div
                class="tq-bar-fill"
                class:done={t.finished}
                style="width: {Math.min(100, t.pct)}%"
              ></div>
            </div>
            <div class="tq-meta">
              <span>{estado(t)}</span>
              <span>{fmtBytes(t.progress_bytes)} / {fmtBytes(t.size_bytes)}</span>
            </div>
            <div class="tq-acts">
              {#if !t.finished && t.state !== "error"}
                <button data-nav class="tq-act" onclick={() => togglePause(t)}>
                  {t.paused ? "▶ Reanudar" : "⏸ Pausar"}
                </button>
              {/if}
              {#if confirmId === t.id}
                <button data-nav class="tq-act danger" onclick={() => quitar(t)}>
                  {t.finished ? "Quitar de la lista" : "Sí, borrar"}
                </button>
                <button data-nav class="tq-act" onclick={() => (confirmId = null)}>Cancelar</button>
              {:else}
                <button data-nav class="tq-act" onclick={() => (confirmId = t.id)}>
                  {t.finished ? "Quitar" : "Cancelar"}
                </button>
              {/if}
            </div>
            {#if confirmId === t.id && !t.finished}
              <p class="tq-warn">Se borra lo descargado hasta ahora.</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .tq-card {
    position: absolute;
    top: 38px;
    right: 0;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 10px;
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.6);
    z-index: 1100;
    width: 360px;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .tq-head {
    display: flex; justify-content: space-between; align-items: baseline; gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid #22222c;
    font-size: 13px;
  }
  .tq-dir {
    color: #6e6e78; font-size: 11px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    max-width: 200px; direction: rtl;
  }
  .tq-empty {
    margin: 0; padding: 24px;
    text-align: center; color: #6e6e78; font-size: 13px;
  }
  .tq-list { list-style: none; margin: 0; padding: 4px; overflow: auto; }
  .tq-item {
    display: flex; flex-direction: column; gap: 5px;
    padding: 10px 12px;
    border-radius: 8px;
  }
  .tq-item:hover { background: #1c1c26; }
  .tq-top { display: flex; justify-content: space-between; gap: 8px; align-items: baseline; }
  .tq-title {
    font-size: 13px; color: #fff; font-weight: 600;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .tq-pct { font-size: 12px; color: #f3a951; flex-shrink: 0; }
  .tq-bar { height: 5px; background: #2a2a36; border-radius: 3px; overflow: hidden; }
  .tq-bar-fill { height: 100%; background: #f3a951; transition: width .3s ease; }
  .tq-bar-fill.done { background: #6cd37a; }
  .tq-meta {
    display: flex; justify-content: space-between; gap: 8px;
    color: #9a9aa6; font-size: 11.5px;
  }
  .tq-item.err .tq-meta { color: #ff7373; }
  .tq-acts { display: flex; gap: 8px; margin-top: 2px; }
  .tq-act {
    background: transparent; border: 0; padding: 0;
    color: #888892; font-size: 11.5px; cursor: pointer;
    text-decoration: underline;
  }
  .tq-act:hover { color: #d8d8e0; }
  .tq-act.danger { color: #ff7373; }
  .tq-warn { margin: 0; color: #f3a951; font-size: 11px; }
</style>
