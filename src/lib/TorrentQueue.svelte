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
  import {
    cola,
    quitarDeCola,
    vaciarCola,
    reintentar,
    arrancarMotor,
  } from "$lib/colaDescargas.svelte";
  import { invoke } from "@tauri-apps/api/core";

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let card: HTMLDivElement | null = $state(null);
  let dir = $state("");
  // Espacio libre en la partición de las descargas. Se lee al abrir el
  // popover: es justo donde el usuario decide si sigue encolando o no.
  let libre = $state<number | null>(null);
  // Id con confirmación de borrado pendiente (borrar archivo es irreversible).
  let confirmId = $state<number | null>(null);

  $effect(() => {
    if (open) {
      // Al abrir, refresca aunque el poll esté dormido (todo terminado).
      void refreshQueue();
      // La elegida por el usuario manda; si no eligió, la que propone el sistema.
      if (config.torrentDir.trim()) dir = config.torrentDir.trim();
      else void torrentDefaultDir().then((d: string) => (dir = d)).catch(() => {});
      void invoke<number>("disk_free", { dir: config.torrentDir.trim() || null })
        .then((n) => (libre = n))
        .catch(() => (libre = null));
      // Abrir la cola es buen momento para empujarla: si algo quedó esperando
      // de la sesión anterior, arranca acá.
      arrancarMotor();
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
      <span class="tq-dir" title={dir}>
        {#if libre !== null}{fmtBytes(libre)} libres{/if}{#if dir && libre !== null} · {/if}{dir}
      </span>
    </header>

    {#if !torrents.list.length && !cola.filas.length}
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

    {#if cola.filas.length}
      <div class="tq-cola">
        <div class="tq-cola-head">
          <strong>En espera ({cola.filas.length})</strong>
          <button data-nav class="tq-act" onclick={() => void vaciarCola()}>Vaciar</button>
        </div>
        <p class="tq-cola-why">
          Bajan de a {config.torrentMaxParalelas}: repartir los seeds entre todas
          no las hace más rápidas, solo retrasa a todas por igual.
        </p>
        <ul class="tq-list">
          {#each cola.filas as f (f.id)}
            <li class="tq-item" class:err={f.estado === "error"}>
              <div class="tq-top">
                <span class="tq-title">
                  {f.title}{f.etiqueta ? ` · ${f.etiqueta}` : ""}
                </span>
                <span class="tq-pct">
                  {f.estado === "buscando" ? "buscando…" : f.estado === "error" ? "error" : "en cola"}
                </span>
              </div>
              {#if f.estado === "error" && f.error}
                <p class="tq-warn">{f.error}</p>
              {/if}
              <div class="tq-acts">
                {#if f.estado === "error"}
                  <button data-nav class="tq-act" onclick={() => void reintentar(f.id)}>
                    ↻ Reintentar
                  </button>
                {/if}
                <button data-nav class="tq-act" onclick={() => void quitarDeCola(f.id)}>
                  Quitar
                </button>
              </div>
            </li>
          {/each}
        </ul>
      </div>
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
  .tq-cola {
    border-top: 1px solid #22222c;
    padding-top: 4px;
  }
  .tq-cola-head {
    display: flex; justify-content: space-between; align-items: center; gap: 10px;
    padding: 10px 14px 2px;
    font-size: 12.5px;
  }
  .tq-cola-why {
    margin: 0; padding: 0 14px 6px;
    color: #6e6e78; font-size: 11px; line-height: 1.4;
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
