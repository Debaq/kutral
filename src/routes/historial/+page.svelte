<script lang="ts">
  // Lo que viste, agrupado por fecha, tus favoritos, "Quiero ver", tus listas
  // y, con cuenta, lo que te recomendaron tus amigos.
  //
  // Misma gramática visual que el catálogo: grid de pósters, foco amarillo,
  // navegación espacial con flechas ($lib/nav). Nada acá necesita el mouse:
  //   ←→↑↓  mover        Enter  reproducir
  //   Supr  quitar       Esc / Backspace  volver
  //   Shift+←→  mover un título dentro de una lista
  // Las pestañas son chips en la fila de filtros: se llega con ↑ desde la
  // primera fila del grid, igual que en la home.
  //
  // Los datos salen del store del historial, ya en memoria: sin queries acá.
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { navegar, enfocarPrimero } from "$lib/nav";
  import {
    historial,
    seguirViendo,
    listaFavoritos,
    borrarProgreso,
    alternarFavorito,
    cargarHistorial,
    estadoDe,
    type FilaHistorial,
    type FilaFavorito,
  } from "$lib/historial.svelte";
  import {
    cuenta,
    social,
    recomendacionesSinVer,
    marcarRecomendacionesVistas,
    quitarRecomendacion,
  } from "$lib/cuenta.svelte";
  import {
    QUIERO_VER,
    listas,
    enOrden,
    itemsDe,
    crearLista,
    renombrarLista,
    borrarLista,
    quitarDeLista,
    moverEnLista,
    siguienteEnOrden,
    type Lista,
    type ItemLista,
  } from "$lib/listas.svelte";

  type Pestana = "seguir" | "vistos" | "favoritos" | "quiero" | "listas" | "amigos";
  const PESTANAS_TODAS: Pestana[] = ["seguir", "vistos", "favoritos", "quiero", "listas", "amigos"];
  // "De amigos" solo existe con cuenta.
  const PESTANAS = $derived<{ id: Pestana; label: string }[]>([
    { id: "seguir", label: "Seguir viendo" },
    { id: "vistos", label: "Historial" },
    { id: "favoritos", label: "Favoritos" },
    { id: "quiero", label: "Quiero ver" },
    { id: "listas", label: "Listas" },
    ...(cuenta.usuario ? [{ id: "amigos" as Pestana, label: "De amigos" }] : []),
  ]);

  // Qué pestaña estaba abierta al salir a reproducir. Sobrevive al viaje de
  // ida y vuelta a la home; no debe sobrevivir al cierre de la app.
  const PESTANA_GUARDADA = "kutral_historial_pestana";

  let pestana = $state<Pestana>("seguir");
  let confirmar = $state<FilaHistorial | null>(null);
  let confirmarBtn = $state<HTMLButtonElement | null>(null);

  const IMG = "https://image.tmdb.org/t/p";

  // AniList entrega URLs completas; TMDb, rutas relativas.
  function img(path: string | null | undefined, size: string): string {
    if (!path) return "";
    return path.startsWith("http") ? path : `${IMG}/${size}${path}`;
  }

  function pct(f: FilaHistorial): number {
    const real =
      f.progress_real ??
      (f.runtime_seconds && f.runtime_seconds > 0 ? f.watched_seconds / f.runtime_seconds : 0);
    return Math.max(0, Math.min(100, Math.round(real * 100)));
  }

  function capitulo(f: FilaHistorial): string {
    return f.season < 0 ? "" : `T${f.season} E${f.episode}`;
  }

  function detalle(f: FilaHistorial): string {
    const cap = capitulo(f);
    if (cap && f.episode_title) return `${cap} · ${f.episode_title}`;
    if (cap) return cap;
    if (f.completed === 1) return "Vista";
    if (f.runtime_seconds && f.runtime_seconds > f.watched_seconds) {
      const min = Math.round((f.runtime_seconds - f.watched_seconds) / 60);
      if (min > 0) return `quedan ${min} min`;
    }
    return `${pct(f)}% visto`;
  }

  // --- Agrupado por fecha ---------------------------------------------------
  // Contra el arranque de HOY, no contra "hace 24 h": algo visto anoche a las
  // 23:50 tiene que decir "Ayer", no "Hoy".
  const DIA_MS = 86_400_000;

  function inicioDelDia(ts: number): number {
    const d = new Date(ts);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }

  function grupoDe(ts: number): string {
    const dias = Math.round((inicioDelDia(Date.now()) - inicioDelDia(ts)) / DIA_MS);
    if (dias <= 0) return "Hoy";
    if (dias === 1) return "Ayer";
    if (dias < 7) return "Esta semana";
    if (dias < 30) return "Este mes";
    return new Date(ts)
      .toLocaleDateString("es-ES", { month: "long", year: "numeric" })
      .replace(/^./, (c) => c.toUpperCase());
  }

  function fechaCorta(ts: number): string {
    return new Date(ts).toLocaleDateString("es-ES", { day: "2-digit", month: "short" });
  }

  // `historial.filas` ya viene ordenado por fecha descendente: agrupar en orden
  // preserva ese orden dentro de cada bloque.
  const grupos = $derived.by(() => {
    const out: { titulo: string; filas: FilaHistorial[] }[] = [];
    for (const f of historial.filas) {
      const g = grupoDe(f.last_watched);
      const ultimo = out[out.length - 1];
      if (ultimo && ultimo.titulo === g) ultimo.filas.push(f);
      else out.push({ titulo: g, filas: [f] });
    }
    return out;
  });

  const pendientes = $derived(seguirViendo(60));
  const favoritos = $derived(listaFavoritos());
  const recomendaciones = $derived(social.recomendaciones);
  // Las que estaban sin ver al abrir la pestaña: siguen marcadas como "nueva"
  // mientras se mira, aunque el servidor ya las dé por vistas.
  let nuevas = $state(new Set<number>());

  // --- Listas ---------------------------------------------------------------
  const quieroVer = $derived(itemsDe(QUIERO_VER));
  const propias = $derived(listas.todas.filter((l) => l.id !== QUIERO_VER));
  /** Lista abierta dentro de la pestaña Listas; null = se ven todas. */
  let abierta = $state<string | null>(null);
  const listaAbierta = $derived(abierta ? (listas.todas.find((l) => l.id === abierta) ?? null) : null);
  const itemsAbierta = $derived(abierta ? itemsDe(abierta) : []);
  // Ítems de la vista actual: lo que Supr y Shift+←→ tocan.
  const itemsVista = $derived(pestana === "quiero" ? quieroVer : pestana === "listas" ? itemsAbierta : []);
  const listaVista = $derived(pestana === "quiero" ? QUIERO_VER : pestana === "listas" ? abierta : null);
  let nuevaLista = $state("");
  let renombrando = $state(false);
  let nombreNuevo = $state("");
  let borrarListaConfirm = $state<Lista | null>(null);
  let borrarListaBtn = $state<HTMLButtonElement | null>(null);
  let avisoLista = $state("");

  function abrirLista(id: string | null) {
    abierta = id;
    renombrando = false;
    avisoLista = "";
    setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 40);
  }

  async function crearNueva() {
    const id = await crearLista(nuevaLista);
    if (!id) return;
    nuevaLista = "";
    abrirLista(id);
  }

  async function guardarNombre() {
    if (abierta) await renombrarLista(abierta, nombreNuevo);
    renombrando = false;
  }

  async function confirmarBorrarLista() {
    const l = borrarListaConfirm;
    borrarListaConfirm = null;
    if (!l) return;
    await borrarLista(l.id);
    abrirLista(null);
  }

  // Descubre el primer título sin ver y deja anotada la lista: al terminar,
  // la pantalla de fin ofrece el siguiente.
  function descubrirEnOrden(listaId: string) {
    const it = siguienteEnOrden(listaId);
    if (!it) {
      avisoLista = "Ya viste todo lo de esta lista.";
      return;
    }
    enOrden.listaId = listaId;
    reproducir(it);
  }

  function portadas(listaId: string): string[] {
    return itemsDe(listaId)
      .map((it) => it.poster_path)
      .filter((p): p is string => !!p)
      .slice(0, 4);
  }

  function tipoDe(it: { media_type: string }): string {
    return it.media_type === "movie" ? "Película" : it.media_type === "anime" ? "Anime" : "Serie";
  }

  function haceDias(ms: number): string {
    const dias = Math.round((inicioDelDia(Date.now()) - inicioDelDia(ms)) / DIA_MS);
    if (dias <= 0) return "hoy";
    if (dias === 1) return "ayer";
    return fechaCorta(ms);
  }

  // --- Acciones -------------------------------------------------------------

  function reproducir(item: {
    tmdb_id: number;
    media_type: string;
    season?: number;
    episode?: number;
  }) {
    const tipo = item.media_type === "anime" ? "anime" : item.media_type;
    if (tipo !== "movie" && tipo !== "tv" && tipo !== "anime") return;
    const q = new URLSearchParams({ play: String(item.tmdb_id), type: tipo });
    if (item.season != null && item.season >= 0 && item.episode != null) {
      q.set("season", String(item.season));
      q.set("episode", String(item.episode));
    }
    // `from` le dice a la home a dónde volver al cerrar el menú, y la pestaña
    // queda guardada para volver a la misma lista, no a la primera.
    q.set("from", "historial");
    try {
      sessionStorage.setItem(PESTANA_GUARDADA, pestana);
    } catch {
      /* sin sessionStorage (modo privado): se vuelve a la pestaña por defecto */
    }
    goto(`/?${q.toString()}`);
  }

  async function quitar(f: FilaHistorial) {
    confirmar = null;
    await borrarProgreso(f.imdb_id, f.season, f.episode);
    setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 30);
  }

  async function quitarFavorito(f: FilaFavorito) {
    await alternarFavorito({
      clave: f.imdb_id,
      tmdbId: f.tmdb_id,
      mediaType: f.media_type,
      title: f.title,
      posterPath: f.poster_path,
    });
  }

  // Supr sobre una tarjeta: la tarjeta enfocada dice a quién borrar. Sin esto
  // el quitar sería un botón que solo alcanza el mouse.
  function borrarEnfocado() {
    const el = document.activeElement as HTMLElement | null;
    const idx = el?.dataset?.idx;
    if (idx == null) return;
    if (pestana === "favoritos") {
      const f = favoritos[Number(idx)];
      if (f) void quitarFavorito(f);
      return;
    }
    if (pestana === "quiero" || pestana === "listas") {
      // En la vista de todas las listas Supr no hace nada: borrar una lista
      // entera va por su botón, con confirmación.
      const it = itemsVista[Number(idx)];
      if (it && listaVista) void quitarDeLista(listaVista, it.clave);
      return;
    }
    if (pestana === "amigos") {
      const r = recomendaciones[Number(idx)];
      if (r) void quitarRecomendacion(r.id).catch((e) => console.warn("[historial] quitar recomendación:", e));
      return;
    }
    const lista = pestana === "seguir" ? pendientes : historial.filas;
    const f = lista[Number(idx)];
    if (f) confirmar = f;
  }

  // Shift+←→ sobre una tarjeta de lista: la cambia de lugar y el foco la sigue.
  function moverEnfocado(delta: -1 | 1) {
    const el = document.activeElement as HTMLElement | null;
    const idx = el?.dataset?.idx;
    if (idx == null || !listaVista) return;
    const it = itemsVista[Number(idx)];
    if (!it) return;
    const destino = Number(idx) + delta;
    if (destino < 0 || destino >= itemsVista.length) return;
    void moverEnLista(listaVista, it.clave, delta).then(() =>
      setTimeout(() => document.querySelector<HTMLElement>(`.galeria [data-idx="${destino}"]`)?.focus(), 20),
    );
  }

  function irAPestana(id: Pestana) {
    pestana = id;
    abierta = null;
    renombrando = false;
    avisoLista = "";
    // Verlas en la pestaña es lo que las da por vistas (el contador se apaga).
    if (id === "amigos") {
      nuevas = new Set(recomendaciones.filter((r) => !r.vista).map((r) => r.id));
      void marcarRecomendacionesVistas();
    }
    setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 40);
  }

  function cambiarPestana(d: number) {
    const i = PESTANAS.findIndex((p) => p.id === pestana);
    irAPestana(PESTANAS[(i + d + PESTANAS.length) % PESTANAS.length].id);
  }

  function onKey(e: KeyboardEvent) {
    const tgt = e.target as HTMLElement | null;
    // Inputs (sliders de la barra superior, sobre todo): las flechas son suyas.
    if (tgt?.tagName === "INPUT" || tgt?.tagName === "TEXTAREA") {
      if (e.key === "Escape") (tgt as HTMLInputElement).blur();
      return;
    }
    if (borrarListaConfirm) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        borrarListaConfirm = null;
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        navegar(e.key === "ArrowLeft" ? "left" : "right", document.querySelector(".modal") ?? document);
      }
      return;
    }
    if (e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      moverEnfocado(e.key === "ArrowLeft" ? -1 : 1);
      return;
    }
    if (confirmar) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        confirmar = null;
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        navegar(e.key === "ArrowLeft" ? "left" : "right", document.querySelector(".modal") ?? document);
      }
      return;
    }
    switch (e.key) {
      case "Escape":
      case "Backspace":
        e.preventDefault();
        // Dentro de una lista, Esc vuelve a ver todas las listas.
        if (pestana === "listas" && abierta) abrirLista(null);
        else goto("/");
        break;
      case "ArrowDown":
        e.preventDefault();
        navegar("down");
        break;
      case "ArrowUp":
        e.preventDefault();
        navegar("up");
        break;
      case "ArrowRight":
        e.preventDefault();
        navegar("right");
        break;
      case "ArrowLeft":
        e.preventDefault();
        navegar("left");
        break;
      case "Delete":
      case "Supr":
        e.preventDefault();
        borrarEnfocado();
        break;
      case "[":
      case "PageUp":
        e.preventDefault();
        cambiarPestana(-1);
        break;
      case "]":
      case "PageDown":
        e.preventDefault();
        cambiarPestana(1);
        break;
    }
  }

  onMount(() => {
    // El layout carga el historial al arrancar; recargamos por si se vio algo
    // en esta sesión desde otra ruta.
    void cargarHistorial();
    setTimeout(() => {
      // Volver de reproducir cae en la misma pestaña de la que salió.
      let guardada: string | null = null;
      try {
        guardada = sessionStorage.getItem(PESTANA_GUARDADA);
        sessionStorage.removeItem(PESTANA_GUARDADA);
      } catch {
        /* tolerado */
      }
      if (guardada && (PESTANAS_TODAS as string[]).includes(guardada)) {
        pestana = guardada as Pestana;
      } else if (!seguirViendo(1).length && historial.filas.length) {
        // Si no quedó nada a medias, la pestaña útil es el historial completo.
        pestana = "vistos";
      }
      setTimeout(() => enfocarPrimero(document.querySelector(".galeria") ?? document), 40);
    }, 60);
  });

  $effect(() => {
    if (confirmar) setTimeout(() => confirmarBtn?.focus(), 30);
  });

  $effect(() => {
    if (borrarListaConfirm) setTimeout(() => borrarListaBtn?.focus(), 30);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="hist">
  <header class="barra" data-section="filtros">
    <h1>Lo que viste</h1>
    <div class="chips">
      {#each PESTANAS as p}
        <button
          data-nav
          class="chip"
          class:active={pestana === p.id}
          onclick={() => irAPestana(p.id)}
        >
          {p.label}
          {#if p.id === "seguir" && pendientes.length}<span class="cuenta">{pendientes.length}</span>{/if}
          {#if p.id === "favoritos" && favoritos.length}<span class="cuenta">{favoritos.length}</span>{/if}
          {#if p.id === "quiero" && quieroVer.length}<span class="cuenta">{quieroVer.length}</span>{/if}
          {#if p.id === "amigos" && recomendacionesSinVer()}<span class="cuenta">{recomendacionesSinVer()}</span>{/if}
        </button>
      {/each}
    </div>
    <span class="ayuda-teclas">
      <b>Enter</b> descubrir · <b>Supr</b> quitar{#if listaVista} · <b>Shift+←→</b> mover{/if} · <b>Esc</b> volver
    </span>
  </header>

  <div class="galeria" data-section="galeria">
    {#if pestana === "seguir"}
      {#if !pendientes.length}
        <p class="vacio">No dejaste nada a medias. Todo lo que empezaste, lo terminaste.</p>
      {:else}
        <div class="grid">
          {#each pendientes as f, i (f.imdb_id + f.season + f.episode)}
            <button
              data-nav
              data-idx={i}
              class="card"
              onclick={() => reproducir({ ...f, season: f.season, episode: f.episode })}
              title={f.title}
            >
              <span class="poster">
                {#if img(f.poster_path ?? f.still_path, "w342")}
                  <img src={img(f.poster_path ?? f.still_path, "w342")} alt="" loading="lazy" />
                {:else}
                  <span class="sin-poster">{f.title}</span>
                {/if}
                {#if capitulo(f)}<span class="badge-cap">{capitulo(f)}</span>{/if}
                <span class="pct">{pct(f)}%</span>
                <span class="progreso"><span class="progreso-fill" style:width="{Math.max(3, pct(f))}%"></span></span>
              </span>
              <span class="pie">
                <span class="titulo">{f.title}</span>
                <span class="sub">{detalle(f)}</span>
              </span>
            </button>
          {/each}
        </div>
      {/if}
    {:else if pestana === "vistos"}
      {#if !historial.filas.length}
        <p class="vacio">
          Todavía no hay nada registrado. Lo que reproduzcas aparece acá, con la fecha.
        </p>
      {:else}
        {#each grupos as g (g.titulo)}
          <h2 class="grupo">{g.titulo}</h2>
          <div class="grid">
            {#each g.filas as f (f.imdb_id + f.season + f.episode)}
              {@const i = historial.filas.indexOf(f)}
              <button
                data-nav
                data-idx={i}
                class="card"
                class:terminada={f.completed === 1}
                onclick={() => reproducir({ ...f, season: f.season, episode: f.episode })}
                title={f.title}
              >
                <span class="poster">
                  {#if img(f.poster_path ?? f.still_path, "w342")}
                    <img src={img(f.poster_path ?? f.still_path, "w342")} alt="" loading="lazy" />
                  {:else}
                    <span class="sin-poster">{f.title}</span>
                  {/if}
                  {#if capitulo(f)}<span class="badge-cap">{capitulo(f)}</span>{/if}
                  {#if f.completed === 1}
                    <span class="tick">✓</span>
                  {:else}
                    <span class="pct">{pct(f)}%</span>
                    <span class="progreso"><span class="progreso-fill" style:width="{Math.max(3, pct(f))}%"></span></span>
                  {/if}
                </span>
                <span class="pie">
                  <span class="titulo">{f.title}</span>
                  <span class="sub">{fechaCorta(f.last_watched)} · {detalle(f)}</span>
                </span>
              </button>
            {/each}
          </div>
        {/each}
      {/if}
    {:else if pestana === "quiero"}
      {#if !quieroVer.length}
        <p class="vacio">
          Nada guardado para después. En la ficha de una película o serie, "＋ Quiero ver" la deja aquí.
        </p>
      {:else}
        <div class="acciones-lista">
          <button data-nav class="btn-lista principal" onclick={() => descubrirEnOrden(QUIERO_VER)}>
            ▶ Descubrir en orden
          </button>
          {#if avisoLista}<span class="aviso-lista">{avisoLista}</span>{/if}
        </div>
        <div class="grid">
          {#each quieroVer as it, i (it.clave)}
            {@render tarjetaItem(it, i)}
          {/each}
        </div>
      {/if}
    {:else if pestana === "listas"}
      {#if !listaAbierta}
        <div class="grid">
          {#each propias as l, i (l.id)}
            {@const portas = portadas(l.id)}
            <button data-nav data-idx={i} class="card" onclick={() => abrirLista(l.id)} title={l.nombre}>
              <span class="poster mosaico" class:uno={portas.length < 4}>
                {#if !portas.length}
                  <span class="sin-poster">{l.nombre}</span>
                {:else if portas.length < 4}
                  <img src={img(portas[0], "w342")} alt="" loading="lazy" />
                {:else}
                  {#each portas as p}<img src={img(p, "w185")} alt="" loading="lazy" />{/each}
                {/if}
              </span>
              <span class="pie">
                <span class="titulo">{l.nombre}</span>
                <span class="sub">{itemsDe(l.id).length} {itemsDe(l.id).length === 1 ? "título" : "títulos"}</span>
              </span>
            </button>
          {/each}
          <div class="card nueva-lista">
            <span class="nueva-tit">＋ Nueva lista</span>
            <input
              data-nav
              bind:value={nuevaLista}
              maxlength="60"
              placeholder="Nombre (ej.: Maratón Nolan)"
              onkeydown={(e) => e.key === "Enter" && void crearNueva()}
            />
            <button data-nav class="btn-lista principal" onclick={crearNueva} disabled={!nuevaLista.trim()}>Crear</button>
            <span class="sub">También puedes crearlas desde la ficha de un título, con "≡ Listas".</span>
          </div>
        </div>
      {:else}
        <div class="acciones-lista">
          <button data-nav class="btn-lista" onclick={() => abrirLista(null)}>← Listas</button>
          {#if renombrando}
            <input
              data-nav
              class="renombrar"
              bind:value={nombreNuevo}
              maxlength="60"
              onkeydown={(e) => {
                if (e.key === "Enter") void guardarNombre();
                if (e.key === "Escape") renombrando = false;
              }}
            />
            <button data-nav class="btn-lista" onclick={guardarNombre}>Guardar</button>
          {:else}
            <h2 class="nombre-lista">{listaAbierta.nombre}</h2>
            {#if itemsAbierta.length}
              <button data-nav class="btn-lista principal" onclick={() => descubrirEnOrden(listaAbierta.id)}>
                ▶ Descubrir en orden
              </button>
            {/if}
            <button
              data-nav
              class="btn-lista"
              onclick={() => {
                nombreNuevo = listaAbierta.nombre;
                renombrando = true;
              }}
            >
              Renombrar
            </button>
            <button data-nav class="btn-lista" onclick={() => (borrarListaConfirm = listaAbierta)}>Borrar lista</button>
          {/if}
          {#if avisoLista}<span class="aviso-lista">{avisoLista}</span>{/if}
        </div>
        {#if !itemsAbierta.length}
          <p class="vacio">
            Lista vacía. En la ficha de una película o serie, "≡ Listas" la agrega aquí.
          </p>
        {:else}
          <div class="grid">
            {#each itemsAbierta as it, i (it.clave)}
              {@render tarjetaItem(it, i)}
            {/each}
          </div>
        {/if}
      {/if}
    {:else if pestana === "amigos"}
      {#if !recomendaciones.length}
        <p class="vacio">
          Nadie te recomendó nada todavía. Cuando un amigo te recomiende una película
          o serie desde su ficha, aparece aquí.
        </p>
      {:else}
        <div class="grid">
          {#each recomendaciones as r, i (r.id)}
            <button
              data-nav
              data-idx={i}
              class="card"
              onclick={() => reproducir(r.datos)}
              title={r.nota ? `${r.datos.title}: "${r.nota}"` : r.datos.title}
            >
              <span class="poster">
                {#if img(r.datos.poster_path, "w342")}
                  <img src={img(r.datos.poster_path, "w342")} alt="" loading="lazy" />
                {:else}
                  <span class="sin-poster">{r.datos.title}</span>
                {/if}
                {#if !r.vista || nuevas.has(r.id)}<span class="badge-cap">nueva</span>{/if}
              </span>
              <span class="pie">
                <span class="titulo">{r.datos.title}</span>
                <span class="sub">de {r.de} · {haceDias(r.creada)}</span>
                {#if r.nota}<span class="sub nota">"{r.nota}"</span>{/if}
              </span>
            </button>
          {/each}
        </div>
      {/if}
    {:else if !favoritos.length}
      <p class="vacio">
        Sin favoritos todavía. En la ficha de una película o serie, el botón ☆ la guarda acá.
      </p>
    {:else}
      <div class="grid">
        {#each favoritos as f, i (f.imdb_id)}
          <button
            data-nav
            data-idx={i}
            class="card"
            onclick={() => reproducir(f)}
            title={f.title}
          >
            <span class="poster">
              {#if img(f.poster_path, "w342")}
                <img src={img(f.poster_path, "w342")} alt="" loading="lazy" />
              {:else}
                <span class="sin-poster">{f.title}</span>
              {/if}
              <span class="estrella">★</span>
            </span>
            <span class="pie">
              <span class="titulo">{f.title}</span>
              <span class="sub">{f.media_type === "movie" ? "Película" : f.media_type === "anime" ? "Anime" : "Serie"}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#snippet tarjetaItem(it: ItemLista, i: number)}
  {@const visto = estadoDe(it.clave)?.visto}
  <button data-nav data-idx={i} class="card" class:terminada={visto} onclick={() => reproducir(it)} title={it.title}>
    <span class="poster">
      {#if img(it.poster_path, "w342")}
        <img src={img(it.poster_path, "w342")} alt="" loading="lazy" />
      {:else}
        <span class="sin-poster">{it.title}</span>
      {/if}
      <span class="badge-cap">{i + 1}</span>
      {#if visto}<span class="tick">✓</span>{/if}
    </span>
    <span class="pie">
      <span class="titulo">{it.title}</span>
      <span class="sub">{tipoDe(it)}{visto ? " · vista" : ""}</span>
    </span>
  </button>
{/snippet}

{#if borrarListaConfirm}
  <div class="modal-fondo">
    <div class="modal" data-section="modal">
      <h3>¿Borrar la lista?</h3>
      <p class="modal-tit">{borrarListaConfirm.nombre}</p>
      <p class="modal-nota">Se borra la lista. Los títulos siguen en el catálogo y en tu historial.</p>
      <div class="modal-btns">
        <button data-nav class="btn-cancelar" onclick={() => (borrarListaConfirm = null)}>Cancelar</button>
        <button data-nav class="btn-borrar" bind:this={borrarListaBtn} onclick={() => void confirmarBorrarLista()}>
          Borrar
        </button>
      </div>
    </div>
  </div>
{/if}

{#if confirmar}
  <div class="modal-fondo">
    <div class="modal" data-section="modal">
      <h3>¿Quitar del historial?</h3>
      <p class="modal-tit">{confirmar.title}{capitulo(confirmar) ? ` · ${capitulo(confirmar)}` : ""}</p>
      <p class="modal-nota">
        Se borra la marca de visto y el minuto guardado. El título sigue en el catálogo.
      </p>
      <div class="modal-btns">
        <button data-nav class="btn-cancelar" onclick={() => (confirmar = null)}>Cancelar</button>
        <button
          data-nav
          class="btn-borrar"
          bind:this={confirmarBtn}
          onclick={() => void quitar(confirmar!)}
        >
          Quitar
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .hist {
    height: 100%;
    display: flex;
    flex-direction: column;
    color: #e6e6ec;
    overflow: hidden;
  }

  /* Barra superior: mismo lenguaje que los filtros del catálogo. */
  .barra {
    display: flex;
    align-items: center;
    gap: 18px;
    flex-wrap: wrap;
    padding: 14px 20px;
    border-bottom: 1px solid #1e1e28;
    background: #0d0d12;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    letter-spacing: 2px;
    text-transform: uppercase;
    color: #f5c518;
    font-weight: 800;
  }
  .chips {
    display: flex;
    gap: 8px;
  }
  .chip {
    background: transparent;
    border: 1px solid #2a2a35;
    color: #9a9aa4;
    border-radius: 999px;
    padding: 7px 16px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.4px;
    cursor: pointer;
    transition: all 0.12s;
  }
  .chip:hover {
    color: #fff;
    border-color: #f5c518;
  }
  .chip:focus,
  .chip:focus-visible {
    outline: none;
    border-color: #f5c518;
    color: #fff;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.25);
  }
  .chip.active,
  .chip.active:focus {
    background: #f5c518;
    border-color: #f5c518;
    color: #0d0d12;
    font-weight: 800;
  }
  .cuenta {
    margin-left: 7px;
    opacity: 0.75;
    font-weight: 700;
  }
  .ayuda-teclas {
    margin-left: auto;
    font-size: 11px;
    color: #6a6a74;
    letter-spacing: 0.3px;
  }
  .ayuda-teclas b {
    color: #9a9aa4;
    font-weight: 700;
  }

  .galeria {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 40px;
  }
  .grupo {
    margin: 22px 20px 0;
    font-size: 11px;
    letter-spacing: 2px;
    text-transform: uppercase;
    color: #6a6a74;
    font-weight: 700;
    border-bottom: 1px solid #1e1e28;
    padding-bottom: 8px;
  }

  /* Mismo grid que el catálogo: la pantalla se siente parte de la misma app. */
  .grid {
    padding: 20px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 20px;
    align-content: start;
  }
  .card {
    position: relative;
    background: #15151c;
    border: 0;
    border-radius: 6px;
    overflow: hidden;
    cursor: pointer;
    padding: 0;
    color: inherit;
    text-align: left;
    display: flex;
    flex-direction: column;
    transition: transform 0.15s;
  }
  .card:hover {
    transform: translateY(-3px);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
  }
  .card:focus,
  .card:focus-visible {
    outline: 4px solid #f5c518;
    outline-offset: 3px;
    transform: translateY(-3px) scale(1.03);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.7), 0 0 0 6px rgba(245, 197, 24, 0.18);
    z-index: 5;
  }
  .poster {
    position: relative;
    display: block;
    aspect-ratio: 2 / 3;
    background: #101018;
  }
  .poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .card.terminada .poster img {
    opacity: 0.5;
  }
  .sin-poster {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 12px;
    text-align: center;
    font-size: 12px;
    color: #55555f;
  }
  .badge-cap {
    position: absolute;
    top: 8px;
    left: 8px;
    background: rgba(13, 13, 18, 0.85);
    color: #fff;
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.8px;
    backdrop-filter: blur(2px);
  }
  .tick {
    position: absolute;
    right: 8px;
    bottom: 8px;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: #2f9e44;
    color: #fff;
    font-size: 15px;
    font-weight: 900;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.65);
  }
  .estrella {
    position: absolute;
    right: 8px;
    bottom: 8px;
    color: #f5c518;
    font-size: 18px;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.8);
  }
  .pct {
    position: absolute;
    right: 8px;
    bottom: 12px;
    background: rgba(13, 13, 18, 0.85);
    color: #f3a951;
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 11px;
    font-weight: 800;
    backdrop-filter: blur(2px);
  }
  .progreso {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 5px;
    background: rgba(0, 0, 0, 0.6);
  }
  .progreso-fill {
    display: block;
    height: 100%;
    background: #f3a951;
  }
  .pie {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 9px 10px 11px;
    min-width: 0;
  }
  .titulo {
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 11px;
    color: #8f8f9a;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub.nota {
    font-style: italic;
    color: #b8b8c2;
  }
  /* Listas */
  .acciones-lista {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    padding: 18px 20px 0;
  }
  .nombre-lista {
    margin: 0 8px 0 4px;
    font-size: 17px;
    font-weight: 800;
  }
  .btn-lista {
    background: transparent;
    border: 1px solid #2a2a35;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 8px 14px;
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn-lista.principal {
    background: #f5c518;
    border-color: #f5c518;
    color: #0d0d12;
  }
  .btn-lista:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-lista:focus,
  .btn-lista:focus-visible,
  .renombrar:focus,
  .nueva-lista input:focus {
    outline: none;
    border-color: #f5c518;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.3);
  }
  .renombrar,
  .nueva-lista input {
    background: #0d0d12;
    border: 1px solid #2a2a35;
    color: #fff;
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 13px;
  }
  .aviso-lista {
    font-size: 12.5px;
    color: #9a9aa4;
  }
  .mosaico {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  }
  .mosaico.uno {
    display: block;
  }
  .nueva-lista {
    cursor: default;
    gap: 10px;
    padding: 16px;
    justify-content: center;
    border: 1px dashed #2a2a35;
  }
  .nueva-lista:hover {
    transform: none;
    box-shadow: none;
  }
  .nueva-tit {
    font-size: 14px;
    font-weight: 800;
    color: #f5c518;
  }
  .nueva-lista .sub {
    white-space: normal;
  }
  .vacio {
    color: #6a6a74;
    font-size: 14px;
    padding: 40px 22px;
  }

  .modal-fondo {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: grid;
    place-items: center;
    z-index: 1000;
  }
  .modal {
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 14px;
    padding: 24px 28px;
    max-width: 430px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.7);
  }
  .modal h3 {
    margin: 0 0 12px;
    font-size: 16px;
    letter-spacing: 0.4px;
  }
  .modal-tit {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 700;
    color: #fff;
  }
  .modal-nota {
    margin: 0;
    font-size: 12px;
    color: #8f8f9a;
  }
  .modal-btns {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    margin-top: 20px;
  }
  .modal-btns button {
    border-radius: 8px;
    padding: 9px 18px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: 1px solid #2a2a35;
    background: transparent;
    color: #d8d8e0;
  }
  .modal-btns button:focus,
  .modal-btns button:focus-visible {
    outline: none;
    border-color: #f5c518;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.25);
  }
  .btn-borrar {
    background: rgba(220, 38, 38, 0.22);
    border-color: #dc2626;
    color: #fff;
  }
  .btn-borrar:focus,
  .btn-borrar:focus-visible {
    border-color: #ef4444;
    box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.3);
  }
</style>
