<script lang="ts">
  // Juegos — catálogo completo por sistema (carátulas de libretro-thumbnails),
  // marca lo que no tienes y al hacer clic baja el ROM (Myrient).
  //
  // Nav: ← → ↑ ↓ / d-pad mueve foco · Enter / A abre (Descubre si la tienes,
  //   descarga si no) · LB/RB cambia sistema · Esc/Backspace/B salen a home.
  //
  // Catálogos enormes (SNES ~3700): carátula lazy por IntersectionObserver y
  // render incremental (shown crece al acercarse el foco al final).

  import { onMount, onDestroy, tick, untrack } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { config, nameInRegions, GAME_REGIONS } from "$lib/config.svelte";

  type System = "nes" | "snes" | "gba" | "gbc" | "ds";

  type Juego = {
    id: string;
    name: string; // variante elegida: nombre No-Intro (carátula + .zip)
    display: string; // título base sin tags, para mostrar
    system: System;
  };

  const SISTEMAS: { key: System; label: string; color: string }[] = [
    { key: "nes", label: "NES", color: "#e60012" },
    { key: "snes", label: "SNES", color: "#7d4fff" },
    { key: "gba", label: "GBA", color: "#d4308a" },
    { key: "gbc", label: "GB Color", color: "#00b06b" },
    { key: "ds", label: "DS", color: "#2b6cff" },
  ];

  // Repos de libretro-thumbnails por sistema (carátulas oficiales).
  const REPO: Record<System, string> = {
    nes: "Nintendo_-_Nintendo_Entertainment_System",
    snes: "Nintendo_-_Super_Nintendo_Entertainment_System",
    gba: "Nintendo_-_Game_Boy_Advance",
    gbc: "Nintendo_-_Game_Boy_Color",
    ds: "Nintendo_-_Nintendo_DS",
  };

  // libretro reemplaza estos chars por "_" en el nombre del archivo.
  function cleanName(title: string): string {
    return title.replace(/[&*/:`<>?\\|]/g, "_");
  }
  function thumbUrl(sys: System, name: string): string {
    const repo = REPO[sys];
    const file = encodeURIComponent(cleanName(name));
    return `https://raw.githubusercontent.com/libretro-thumbnails/${repo}/master/Named_Boxarts/${file}.png`;
  }

  function colorDe(sys: System): string {
    return SISTEMAS.find((s) => s.key === sys)?.color ?? "#888";
  }

  // --- Estado ---
  let filtro = $state<System>("nes");
  let catalogo = $state<Record<string, string[]>>({});
  let owned = $state<Set<string>>(new Set());
  let cargando = $state(false);
  let errorMsg = $state("");

  let focusedIdx = $state(0);
  let shown = $state(60);
  let cardEls: HTMLButtonElement[] = $state([]);

  // Carátulas resueltas: name → src local (asset://).
  let boxarts = $state<Record<string, string>>({});
  // Descargas en curso: name → {received,total}.
  let bajando = $state<Record<string, { received: number; total: number }>>({});

  // Filtros extra (estilo pelis).
  let busca = $state("");
  let soloTengo = $state(false);

  // Modal de detalle del juego seleccionado.
  let detalle = $state<Juego | null>(null);
  let modalSel = $state(0); // 0 = Descubrir, 1 = Descargar

  // Título base: nombre sin los (tags) ni [tags] → "1942 (USA) (Rev 1)" → "1942".
  function baseTitle(name: string): string {
    return name
      .replace(/\s*\([^)]*\)/g, "")
      .replace(/\s*\[[^\]]*\]/g, "")
      .trim();
  }

  // Tags que indican variante de baja prioridad (no la versión "buena").
  const MALOS = [
    "(Beta", "(Proto", "(Demo", "(Sample", "(Promo",
    "(Pirate", "(Unl", "(Aftermarket", "(Hack", "(Test",
  ];

  // Menor puntaje = mejor variante.
  function variantScore(name: string): number {
    let s = 0;
    if (MALOS.some((m) => name.includes(m))) s += 1000;
    // Prioridad por región según el orden elegido en config.
    let rp = 50;
    config.gameRegions.forEach((id, i) => {
      const r = GAME_REGIONS.find((x) => x.id === id);
      if (r && r.tags.some((t) => name.includes(t))) rp = Math.min(rp, i);
    });
    s += rp * 10;
    const m = name.match(/Rev (\d+)/); // revisión más alta = mejor
    s -= m ? parseInt(m[1], 10) : 0;
    s += name.length * 0.001; // desempate: nombre más corto
    return s;
  }

  // Catálogo filtrado por región + deduplicado por título (1 card por juego).
  // Orden: las que tengo primero, luego alfabético.
  let lista = $derived.by(() => {
    const names = (catalogo[filtro] ?? []).filter((n) =>
      nameInRegions(n, config.gameRegions),
    );
    const grupos = new Map<string, string>(); // base → mejor variante
    for (const n of names) {
      const base = baseTitle(n);
      const actual = grupos.get(base);
      if (!actual || variantScore(n) < variantScore(actual)) {
        grupos.set(base, n);
      }
    }
    const q = busca.trim().toLowerCase();
    return [...grupos.entries()]
      .map(([display, name]) => ({
        id: `${filtro}:${name}`,
        name,
        display,
        system: filtro,
      }))
      .filter((g) => !q || g.display.toLowerCase().includes(q))
      .filter((g) => !soloTengo || owned.has(g.name))
      .sort((a, b) => {
        const ta = owned.has(a.name) ? 0 : 1;
        const tb = owned.has(b.name) ? 0 : 1;
        if (ta !== tb) return ta - tb; // tenidas primero
        return a.display.localeCompare(b.display);
      }) as Juego[];
  });

  // --- Carga de catálogo + biblioteca al cambiar de sistema ---
  async function cargarSistema(sys: System) {
    cargando = !catalogo[sys];
    errorMsg = "";
    focusedIdx = 0;
    shown = 60;
    try {
      const [cat, own] = await Promise.all([
        invoke<string[]>("emu_catalog", { system: sys }),
        invoke<string[]>("emu_owned", { system: sys }),
      ]);
      catalogo = { ...catalogo, [sys]: cat };
      owned = new Set(own);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      cargando = false;
      await tick();
      // No robar el foco si estás escribiendo en el buscador.
      if (document.activeElement?.tagName !== "INPUT") cardEls[0]?.focus();
    }
  }

  // Solo al cambiar de sistema. untrack: cargarSistema lee/escribe `catalogo`,
  // sin esto el effect se auto-dispararía en bucle (y robaría el foco).
  $effect(() => {
    const sys = filtro;
    untrack(() => void cargarSistema(sys));
  });

  // --- Carátulas (lazy) ---
  async function loadBoxart(g: Juego) {
    if (boxarts[g.name]) return;
    try {
      const local = await invoke<string>("cache_image", {
        url: thumbUrl(g.system, g.name),
        maxW: 512,
      });
      boxarts = { ...boxarts, [g.name]: convertFileSrc(local) };
    } catch {
      // 404 / sin red → placeholder de color.
    }
  }

  function attachThumb(node: HTMLElement, g: Juego) {
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          void loadBoxart(g);
          io.disconnect();
        }
      },
      { rootMargin: "300px" },
    );
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  // --- Acciones ---
  // Clic en card → abre el modal de detalle (info + botones).
  function abrirJuego(g: Juego) {
    detalle = g;
    modalSel = owned.has(g.name) ? 0 : 1;
    void loadBoxart(g);
  }
  function cerrarModal() {
    detalle = null;
  }

  async function descubrir(g: Juego) {
    try {
      await invoke("emu_play", { system: g.system, rom: `${g.name}.zip` });
    } catch (e) {
      errorMsg = String(e);
    }
  }

  async function descargar(g: Juego) {
    if (bajando[g.name]) return;
    bajando = { ...bajando, [g.name]: { received: 0, total: 0 } };
    try {
      await invoke("emu_download", { system: g.system, name: g.name });
      // El evento emu_download_done confirma; por si acaso marcamos tenida.
      owned = new Set(owned).add(g.name);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      const { [g.name]: _drop, ...rest } = bajando;
      bajando = rest;
    }
  }

  // Activa el botón seleccionado del modal.
  function accionModal() {
    if (!detalle) return;
    const tengo = owned.has(detalle.name);
    if (modalSel === 0 && tengo) void descubrir(detalle);
    else if (modalSel === 1 && !tengo && !bajando[detalle.name]) void descargar(detalle);
  }

  // --- Navegación ---
  function cols(): number {
    return Math.max(1, Math.floor(window.innerWidth / 190));
  }

  async function mover(delta: number) {
    const n = lista.length;
    if (!n) return;
    let next = focusedIdx + delta;
    if (next < 0) next = 0;
    if (next >= n) next = n - 1;
    focusedIdx = next;
    if (focusedIdx >= shown - cols() * 2) shown = Math.min(n, shown + 60);
    await tick();
    cardEls[focusedIdx]?.scrollIntoView({ block: "nearest" });
    cardEls[focusedIdx]?.focus();
  }

  const ORDEN: System[] = SISTEMAS.map((s) => s.key);
  function ciclarSistema(dir: number) {
    const i = ORDEN.indexOf(filtro);
    filtro = ORDEN[(i + dir + ORDEN.length) % ORDEN.length];
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;

    // En el buscador: dejar escribir; Escape lo cierra/desenfoca.
    const tgt = e.target as HTMLElement | null;
    if (tgt?.tagName === "INPUT") {
      if (k === "Escape") (tgt as HTMLInputElement).blur();
      return;
    }

    // Modal abierto: ← → cambia botón, Enter/A activa, Esc/Backspace cierra.
    if (detalle) {
      if (k === "Escape" || k === "Backspace") {
        e.preventDefault();
        cerrarModal();
      } else if (k === "ArrowLeft" || k === "ArrowRight") {
        e.preventDefault();
        modalSel = modalSel === 0 ? 1 : 0;
      } else if (k === "Enter") {
        e.preventDefault();
        accionModal();
      }
      return;
    }

    if (k === "Escape" || k === "Backspace") {
      e.preventDefault();
      goto("/");
    } else if (k === "ArrowRight") {
      e.preventDefault();
      void mover(1);
    } else if (k === "ArrowLeft") {
      e.preventDefault();
      void mover(-1);
    } else if (k === "ArrowDown") {
      e.preventDefault();
      void mover(cols());
    } else if (k === "ArrowUp") {
      e.preventDefault();
      void mover(-cols());
    } else if (k === "Enter") {
      e.preventDefault();
      const g = lista[focusedIdx];
      if (g) abrirJuego(g);
    } else if (k === "[" || k === "PageUp") {
      e.preventDefault();
      ciclarSistema(-1);
    } else if (k === "]" || k === "PageDown") {
      e.preventDefault();
      ciclarSistema(1);
    }
  }

  // --- Mando de consola (Gamepad API) ---
  // d-pad/stick → foco · A → abrir · B → home · LB/RB → sistema.
  // Botón estándar: A=0 B=1 LB=4 RB=5 d-pad=12..15.
  let rafId = 0;
  let prev: boolean[] = [];
  let axisCooldown = 0;

  function gamepadLoop() {
    const pads = navigator.getGamepads?.() ?? [];
    const gp = pads.find((p) => p) ?? null;
    if (gp) {
      const b = gp.buttons.map((x) => x.pressed);
      const edge = (i: number) => b[i] && !prev[i];

      // Si estás escribiendo en el buscador, el mando NO navega (robaría el
      // foco del input cada frame). Solo registra estado y sale.
      if (document.activeElement?.tagName === "INPUT") {
        prev = b;
        rafId = requestAnimationFrame(gamepadLoop);
        return;
      }

      if (detalle) {
        // Modal: ←→ cambia botón · A activa · B cierra.
        if (edge(14) || edge(15)) modalSel = modalSel === 0 ? 1 : 0;
        if (edge(0)) accionModal();
        if (edge(1)) cerrarModal();
        prev = b;
        rafId = requestAnimationFrame(gamepadLoop);
        return;
      }

      if (edge(12)) void mover(-cols());
      if (edge(13)) void mover(cols());
      if (edge(14)) void mover(-1);
      if (edge(15)) void mover(1);
      if (edge(0)) {
        const g = lista[focusedIdx];
        if (g) abrirJuego(g);
      }
      if (edge(1)) goto("/");
      if (edge(4)) ciclarSistema(-1);
      if (edge(5)) ciclarSistema(1);

      // Solo stick izquierdo (axes 0/1) con zona muerta amplia, para no
      // confundir con triggers/hat que reposan en valores no-cero.
      const ax = gp.axes[0] ?? 0;
      const ay = gp.axes[1] ?? 0;
      if (axisCooldown > 0) axisCooldown--;
      else if (Math.abs(ax) > 0.7 || Math.abs(ay) > 0.7) {
        if (Math.abs(ax) > Math.abs(ay)) void mover(ax > 0 ? 1 : -1);
        else void mover(ay > 0 ? cols() : -cols());
        axisCooldown = 12;
      }
      prev = b;
    }
    rafId = requestAnimationFrame(gamepadLoop);
  }

  // --- Eventos de descarga desde el backend ---
  let unlisten: UnlistenFn[] = [];

  onMount(async () => {
    unlisten.push(
      await listen<{ name: string; received: number; total: number }>(
        "emu_download_progress",
        (e) => {
          bajando = {
            ...bajando,
            [e.payload.name]: {
              received: e.payload.received,
              total: e.payload.total,
            },
          };
        },
      ),
    );
    unlisten.push(
      await listen<{ name: string }>("emu_download_done", (e) => {
        owned = new Set(owned).add(e.payload.name);
        const { [e.payload.name]: _d, ...rest } = bajando;
        bajando = rest;
      }),
    );
    rafId = requestAnimationFrame(gamepadLoop);
  });

  onDestroy(() => {
    cancelAnimationFrame(rafId);
    unlisten.forEach((u) => u());
  });

  function pct(b: { received: number; total: number }): number {
    return b.total > 0 ? Math.round((b.received / b.total) * 100) : 0;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="juegos">
  <header class="top">
    <h1><span class="marca">Juegos</span> <em>retro</em></h1>
    <nav class="filtros">
      {#each SISTEMAS as s}
        <button
          class:active={filtro === s.key}
          style="--c:{s.color}"
          onclick={() => (filtro = s.key)}
        >
          {s.label}
        </button>
      {/each}
    </nav>
    <div class="filtros2">
      <input
        class="buscar"
        type="search"
        placeholder="Buscar juego…"
        bind:value={busca}
      />
      <button class="toggle" class:active={soloTengo} onclick={() => (soloTengo = !soloTengo)}>
        ★ Solo las que tengo
      </button>
    </div>
  </header>

  {#if cargando}
    <div class="empty">Cargando catálogo…</div>
  {:else if errorMsg}
    <div class="empty err">{errorMsg}</div>
  {:else if lista.length === 0}
    <div class="empty">Catálogo vacío.</div>
  {:else}
    <div class="grid">
      {#each lista.slice(0, shown) as g, i (g.id)}
        {@const tengo = owned.has(g.name)}
        {@const dl = bajando[g.name]}
        <button
          data-nav
          class="card"
          class:focused={focusedIdx === i}
          class:no-tengo={!tengo}
          bind:this={cardEls[i]}
          use:attachThumb={g}
          onclick={() => {
            focusedIdx = i;
            abrirJuego(g);
          }}
          onfocus={() => (focusedIdx = i)}
        >
          {#if boxarts[g.name]}
            <img src={boxarts[g.name]} alt={g.name} loading="lazy" />
          {:else}
            <div class="no-box" style="--c:{colorDe(g.system)}">
              <span>{g.display}</span>
            </div>
          {/if}

          {#if !tengo && !dl}
            <span class="card-badge falta">No la tienes</span>
          {/if}
          {#if tengo}
            <span class="card-badge tengo" style="background:{colorDe(g.system)}">
              ▶
            </span>
          {/if}

          {#if dl}
            <div class="dl">
              <div class="dl-bar" style="width:{pct(dl)}%"></div>
              <span class="dl-txt">Bajando {pct(dl)}%</span>
            </div>
          {/if}

          <div class="card-meta">
            <span class="card-title">{g.display}</span>
          </div>
        </button>
      {/each}
    </div>
    {#if shown < lista.length}
      <div class="mas">{shown} de {lista.length} · sigue navegando para ver más</div>
    {/if}
  {/if}

  <footer class="ayuda">
    <kbd>←→↑↓</kbd>/<kbd>d-pad</kbd> navega · <kbd>Enter</kbd>/<kbd>A</kbd> abre ·
    <kbd>LB</kbd>/<kbd>RB</kbd> sistema · <kbd>Esc</kbd>/<kbd>B</kbd> a home
  </footer>

  {#if detalle}
    {@const d = detalle}
    {@const tengo = owned.has(d.name)}
    {@const dl = bajando[d.name]}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="modal-bg"
      role="presentation"
      onclick={cerrarModal}
    >
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        aria-label={d.display}
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
      >
        <div class="modal-art">
          {#if boxarts[d.name]}
            <img src={boxarts[d.name]} alt={d.display} />
          {:else}
            <div class="no-box" style="--c:{colorDe(d.system)}">
              <span>{d.display}</span>
            </div>
          {/if}
        </div>
        <div class="modal-info">
          <h2>{d.display}</h2>
          <div class="chips">
            <span class="chip" style="background:{colorDe(d.system)}">
              {SISTEMAS.find((s) => s.key === d.system)?.label}
            </span>
            <span class="chip estado" class:ok={tengo}>
              {tengo ? "En tu biblioteca" : "No la tienes"}
            </span>
          </div>
          <p class="variante">{d.name}</p>

          {#if dl}
            <div class="modal-dl">
              <div class="modal-dl-bar" style="width:{pct(dl)}%"></div>
              <span>Bajando {pct(dl)}%</span>
            </div>
          {/if}

          <div class="modal-btns">
            <button
              class="btn primary"
              class:sel={modalSel === 0}
              disabled={!tengo}
              onclick={() => { modalSel = 0; accionModal(); }}
            >
              ▶ Descubrir
            </button>
            <button
              class="btn"
              class:sel={modalSel === 1}
              disabled={tengo || !!dl}
              onclick={() => { modalSel = 1; accionModal(); }}
            >
              {dl ? "Bajando…" : "⬇ Descargar"}
            </button>
          </div>

          <p class="cerrar-hint"><kbd>Esc</kbd>/<kbd>B</kbd> cerrar</p>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .juegos {
    height: 100%;
    overflow-y: auto;
    padding: 28px 36px 80px;
    color: #eee;
    background: #0d0d12;
  }

  .filtros2 {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-left: auto;
  }
  .buscar {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #eee;
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 13px;
    min-width: 200px;
  }
  .toggle {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #bbb;
    padding: 7px 14px;
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
  }
  .toggle.active {
    color: #fff;
    border-color: #f5c518;
    box-shadow: 0 0 0 1px #f5c518 inset;
  }

  /* --- Modal --- */
  .modal-bg {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    padding: 24px;
  }
  .modal {
    display: flex;
    gap: 28px;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 16px;
    padding: 24px;
    max-width: 720px;
    width: 100%;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.7);
  }
  .modal-art {
    flex: 0 0 220px;
    border-radius: 10px;
    overflow: hidden;
  }
  .modal-art img,
  .modal-art .no-box {
    width: 220px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
  }
  .modal-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .modal-info h2 {
    margin: 0 0 12px;
    font-size: 24px;
  }
  .chips {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }
  .chip {
    font-size: 12px;
    font-weight: 700;
    color: #fff;
    padding: 3px 10px;
    border-radius: 6px;
  }
  .chip.estado {
    background: #444;
    color: #ddd;
  }
  .chip.estado.ok {
    background: #1f7a3d;
    color: #fff;
  }
  .variante {
    color: #777;
    font-size: 12px;
    margin: 0 0 18px;
    word-break: break-word;
  }
  .modal-dl {
    position: relative;
    height: 26px;
    border-radius: 6px;
    overflow: hidden;
    background: #0e0e14;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 16px;
    font-size: 12px;
    font-weight: 700;
  }
  .modal-dl-bar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    background: linear-gradient(90deg, #7d4fff, #2b6cff);
  }
  .modal-dl span {
    position: relative;
  }
  .modal-btns {
    display: flex;
    gap: 12px;
    margin-top: auto;
  }
  .btn {
    flex: 1;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid #2a2a36;
    background: #1a1a22;
    color: #eee;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn.primary {
    background: linear-gradient(135deg, #7d4fff, #2b6cff);
    border: none;
  }
  .btn.sel {
    outline: none;
    box-shadow: 0 0 0 2px #f5c518;
  }
  .btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .cerrar-hint {
    color: #666;
    font-size: 11px;
    margin: 14px 0 0;
    text-align: right;
  }

  .top {
    display: flex;
    align-items: baseline;
    gap: 24px;
    margin-bottom: 22px;
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: 30px;
    font-weight: 800;
  }
  .marca {
    background: linear-gradient(135deg, #7d4fff, #2b6cff);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  h1 em {
    font-style: italic;
    color: #888;
    font-weight: 500;
  }

  .filtros {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .filtros button {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #bbb;
    padding: 6px 14px;
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
  }
  .filtros button.active {
    color: #fff;
    border-color: var(--c, #7d4fff);
    box-shadow: 0 0 0 1px var(--c, #7d4fff) inset;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 18px;
  }

  .card {
    position: relative;
    padding: 0;
    border: none;
    border-radius: 10px;
    overflow: hidden;
    background: #15151c;
    cursor: pointer;
    text-align: left;
    transition: transform 0.12s, box-shadow 0.12s;
  }
  .card:hover {
    transform: translateY(-3px);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
  }
  .card.focused,
  .card:focus,
  .card:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px #f5c518;
  }
  /* No la tienes → carátula apagada. */
  .card.no-tengo img,
  .card.no-tengo .no-box {
    filter: grayscale(0.7) brightness(0.6);
  }

  .card img,
  .no-box {
    width: 100%;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    display: block;
    background: #222;
  }
  .no-box {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 14px;
    text-align: center;
    font-weight: 700;
    font-size: 14px;
    line-height: 1.3;
    background: radial-gradient(
      circle at 50% 35%,
      color-mix(in srgb, var(--c) 55%, #000),
      #111
    );
  }

  .card-badge {
    position: absolute;
    top: 8px;
    left: 8px;
    font-size: 11px;
    font-weight: 700;
    color: #fff;
    padding: 2px 8px;
    border-radius: 6px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
  }
  .card-badge.falta {
    background: #444;
    color: #ddd;
  }
  .card-badge.tengo {
    font-size: 12px;
    padding: 2px 7px;
  }

  .dl {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 42px;
    background: rgba(0, 0, 0, 0.55);
    height: 22px;
    display: flex;
    align-items: center;
  }
  .dl-bar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    background: linear-gradient(90deg, #7d4fff, #2b6cff);
  }
  .dl-txt {
    position: relative;
    width: 100%;
    text-align: center;
    font-size: 11px;
    font-weight: 700;
  }

  .card-meta {
    padding: 10px 12px;
  }
  .card-title {
    font-size: 13px;
    font-weight: 600;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .empty {
    color: #777;
    padding: 60px 0;
    text-align: center;
  }
  .empty.err {
    color: #f87171;
  }
  .mas {
    color: #666;
    text-align: center;
    padding: 24px 0;
    font-size: 12px;
  }

  .ayuda {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 10px;
    text-align: center;
    font-size: 12px;
    color: #888;
    background: linear-gradient(transparent, #0d0d12 40%);
  }
  kbd {
    background: #222;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 11px;
  }
</style>
