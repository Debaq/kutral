<script lang="ts">
  import { notifs, markAllRead, clearAll } from "$lib/notifStore.svelte";

  let { open = $bindable(false) } = $props<{ open?: boolean }>();

  let card: HTMLDivElement | null = $state(null);

  $effect(() => {
    if (open) {
      markAllRead();
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

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); open = false; }
  }

  function fmtAgo(at: number): string {
    const s = Math.floor((Date.now() - at) / 1000);
    if (s < 60) return "ahora";
    const m = Math.floor(s / 60);
    if (m < 60) return `hace ${m} min`;
    const h = Math.floor(m / 60);
    if (h < 24) return `hace ${h} h`;
    const d = Math.floor(h / 24);
    return `hace ${d} d`;
  }
</script>

{#if open}
  <div
    class="nt-card"
    bind:this={card}
    role="dialog"
    aria-label="Notificaciones"
    onkeydown={onKey}
    tabindex="-1"
  >
    <header class="nt-head">
      <strong>Notificaciones</strong>
      {#if notifs.list.length > 0}
        <button class="nt-clear" onclick={clearAll}>Limpiar</button>
      {/if}
    </header>
    {#if notifs.list.length === 0}
      <p class="nt-empty">Sin avisos</p>
    {:else}
      <ul class="nt-list">
        {#each notifs.list as n (n.id)}
          <li class="nt-item kind-{n.kind}">
            <span class="dot"></span>
            <div class="nt-body">
              <strong>{n.title}</strong>
              {#if n.body}<span>{n.body}</span>{/if}
              <em>{fmtAgo(n.at)}</em>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .nt-card {
    position: absolute;
    top: 38px;
    right: 0;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 10px;
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.6);
    z-index: 1100;
    width: 320px;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .nt-head {
    display: flex; justify-content: space-between; align-items: center;
    padding: 12px 14px;
    border-bottom: 1px solid #22222c;
    font-size: 13px;
  }
  .nt-clear {
    background: transparent; border: 0;
    color: #888892; font-size: 11.5px; cursor: pointer;
    text-decoration: underline; padding: 0;
  }
  .nt-clear:hover { color: #d8d8e0; }
  .nt-empty {
    margin: 0; padding: 24px;
    text-align: center; color: #6e6e78; font-size: 13px;
  }
  .nt-list { list-style: none; margin: 0; padding: 4px; overflow: auto; }
  .nt-item {
    display: flex; gap: 10px;
    padding: 10px 12px;
    border-radius: 8px;
    font-size: 12.5px;
  }
  .nt-item:hover { background: #1c1c26; }
  .dot {
    width: 8px; height: 8px; border-radius: 50%;
    margin-top: 6px; flex-shrink: 0;
  }
  .kind-info .dot { background: #6cb1d3; }
  .kind-success .dot { background: #6cd37a; }
  .kind-warn .dot { background: #f3a951; }
  .kind-error .dot { background: #c44; }
  .nt-body {
    display: flex; flex-direction: column; gap: 2px;
    flex: 1; min-width: 0;
  }
  .nt-body strong { font-size: 13px; color: #fff; font-weight: 600; }
  .nt-body span {
    color: #a0a0aa; font-size: 12px;
    overflow-wrap: anywhere;
  }
  .nt-body em {
    font-style: normal; color: #6e6e78; font-size: 11px; margin-top: 2px;
  }
</style>
