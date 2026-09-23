<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
  import Database from "@tauri-apps/plugin-sql";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { afterNavigate, goto } from "$app/navigation";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { setPlaying } from "$lib/playerState.svelte";
  import { config } from "$lib/config.svelte";
  import SourcePicker from "$lib/SourcePicker.svelte";
  import { tieneDescarga, estadoDescarga } from "$lib/descargas.svelte";
  import { encolar, yaEnCola } from "$lib/colaDescargas.svelte";
  import { notify } from "$lib/notifStore.svelte";
  import PlayMenu from "$lib/PlayMenu.svelte";
  import QRCode from "qrcode";
  import EpisodePicker from "$lib/EpisodePicker.svelte";
  import PostCreditos from "$lib/PostCreditos.svelte";
  import { esFinReal, type FinPayload } from "$lib/finVideo";
  import { WEB_PLAYER_ENABLED } from "$lib/features";
  import { ANIME_GENRES, ANILIST_SORTS, anilistGenresCSV, animeSeasonOptions } from "$lib/anime";
  import RemoteQr from "$lib/RemoteQr.svelte";
  import {
    guardarCatalogo,
    leerCatalogo,
    guardarFiltros,
    leerFiltros,
    guardarPrimeraPagina,
    leerPrimeraPagina,
  } from "$lib/catalogo";
  import { IMG, art, onImgError, volcarImgCache } from "$lib/imagenes.svelte";
  import {
    EMPTY_PICK,
    resolveTrailer,
    resolveAnimeTrailer,
    ytTrailerSrc,
    type TrailerPick,
  } from "$lib/trailers";
  import { sugerenciasPara, type SugerenciaFin, type SiguienteFin } from "$lib/sugerenciasFin";
  import { autofocusFirst, seasonLabel } from "$lib/inicio/util";
  import FichaPersona from "$lib/inicio/FichaPersona.svelte";
  import Carrusel from "$lib/inicio/Carrusel.svelte";
  import FichaTitulo from "$lib/inicio/FichaTitulo.svelte";
  import ClaveTmdb from "$lib/inicio/ClaveTmdb.svelte";
  import AvisoNoDisponible from "$lib/inicio/AvisoNoDisponible.svelte";
  import TrailerQr from "$lib/inicio/TrailerQr.svelte";
  import Descubrir from "$lib/inicio/Descubrir.svelte";
  import PantallaNoDisponible from "$lib/inicio/PantallaNoDisponible.svelte";
  import {
    premioDe,
    premiosResueltos,
    restaurarPremios,
    enqueueAwards,
    awardsLabel,
    awardsTitle,
  } from "$lib/premios.svelte";
  import { inhibirReposo } from "$lib/reposo";
  import { mejor, seccionDe } from "$lib/nav";
  import {
    cargarNoDisponiblesIniciales,
    pausarScreening,
    suscribirScreening,
  } from "$lib/screening.svelte";
  import {
    historial,
    claveMedio,
    estadoDe,
    estadoEpisodio,
    filaPelicula,
    filaEnCurso,
    segundosParaRetomar,
    guardarProgreso,
    borrarProgreso,
    marcarVisto,
    esFavorito,
    alternarFavorito,
    setContexto,
    contextoActual,
    limpiarContexto,
    type FilaHistorial,
    type MediaMeta,
  } from "$lib/historial.svelte";
  import type {
    OmdbDetail,
    ListItem,
    ListResp,
    Detail,
    PersonInfo,
    EpisodeMini,
  } from "$lib/tipos";

  type WyzieSubtitle = { url: string; label: string; lang: string };

  let db: Awaited<ReturnType<typeof Database.load>> | null = null;
  let discoverStartTs = 0;
  let unavailableSet = $state<Set<string>>(new Set());
  let personOpen = $state<PersonInfo | null>(null);
  let personLoading = $state(false);
  // Carrusel de escenas (backdrops) en pantalla completa.
  let carousel = $state<{ images: string[]; idx: number } | null>(null);

  const BACK_KEYS = ["Escape", "Backspace"];
  const ARROW_KEYS = ["Up", "Down", "Left", "Right"];

  // Fullscreen que tenía la ventana antes de entrar a los menús de
  // reproducción. Al volver al catálogo se restaura ese estado: forzar modo
  // ventana dejaba maximizado a quien había puesto fullscreen a mano. Es una
  // promesa para que dos setFs(true) seguidos lean el estado de antes del
  // primero y no el que dejó este mismo.
  let fsPrevio: Promise<boolean> | null = null;

  async function setFs(on: boolean) {
    try {
      const win = getCurrentWindow();
      if (on) {
        fsPrevio ??= win.isFullscreen().catch(() => false);
        await win.setFullscreen(true);
      } else {
        const previo = fsPrevio;
        fsPrevio = null;
        await win.setFullscreen(previo ? await previo : false);
      }
    } catch (e) {
      console.warn("fullscreen falló", e);
    }
  }

  // Despacha el cierre correcto según el modo actual al momento de presionar Esc/Backspace.
  // Evita el bug donde Esc en playmenu llamaba a stopDiscover (que no-op) y dejaba la vista colgada.
  function dispatchBack() {
    if (mode === "playmenu" || mode === "episodes" || mode === "sources" || mode === "fin") {
      closeSources();
    } else if (mode === "discover" || mode === "trailer" || mode === "unavailable") {
      stopDiscover();
    }
  }

  async function registerBackShortcuts(includeArrows = false) {
    // Desregistrar primero: evita "HotKey already registered" si el modo
    // anterior dejó shortcuts pegados (ej. trailer → playmenu → trailer).
    await unregisterBackShortcuts();
    for (const k of BACK_KEYS) {
      try {
        await register(k, (ev) => {
          if (ev.state === "Pressed") void siEnfocada(dispatchBack);
        });
      } catch (e) {
        console.warn(`shortcut ${k} no registrado`, e);
      }
    }
    if (includeArrows) {
      const map: Record<string, "up" | "down" | "left" | "right"> = {
        Up: "up", Down: "down", Left: "left", Right: "right",
      };
      for (const k of ARROW_KEYS) {
        try {
          await register(k, (ev) => {
            if (ev.state === "Pressed") void siEnfocada(() => spatialNav(map[k]));
          });
        } catch (e) {
          console.warn(`shortcut ${k} no registrado`, e);
        }
      }
    }
  }

  // Los atajos son globales del SO: sin este filtro, un Backspace tecleado
  // en otra app cerraba el menú de Kütral (y lo sacaba de fullscreen).
  async function siEnfocada(fn: () => void) {
    try {
      if (!(await getCurrentWindow().isFocused())) return;
    } catch {
      return;
    }
    fn();
  }

  async function unregisterBackShortcuts() {
    for (const k of [...BACK_KEYS, ...ARROW_KEYS]) {
      try { await unregister(k); } catch { /* tolerado */ }
    }
  }


  // Sepá (trivia) todavía no existe: su tarjeta ocupaba un lugar en la fila de
  // accesos y no lleva a ninguna parte. Escondida hasta que haya algo detrás —
  // el markup y los estilos quedan, poner en true la devuelve.
  const SEPA_VISIBLE = false;

  const REF = "hm_tpks_i_2_pd_tp1_pbr_ic";

  let apiKey = $state("");
  let keyInput = $state("");
  let showKey = $state(false);

  type Tab = "movie" | "tv" | "anime";
  let tab = $state<Tab>("movie");
  // Mapeo tab → media_type real para TMDb
  function tabToMediaType(t: Tab): "movie" | "tv" {
    return t === "anime" ? "tv" : t;
  }
  function tabExtras(t: Tab) {
    if (t === "anime") return { originCountry: "JP", forceGenres: "16" };
    return { originCountry: "", forceGenres: "" };
  }
  let query = $state("");
  let debouncedQ = $state("");
  let page = $state(1);
  let totalPages = $state(1);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  // Efecto de chispas mágicas mientras se cargan más pelis tras ArrowDown
  // al final del grid (gallery + down + no candidato). Se autoapaga cuando
  // loadingMore vuelve a false o tras un timeout de seguridad.
  let sparkling = $state(false);
  let sparkleTimer: ReturnType<typeof setTimeout> | null = null;
  const SPARKLE_COUNT = 16;
  // Posiciones / glifos congelados al primer render para que no parpadeen
  // entre re-renders reactivos.
  // Paleta dorada con acentos mágicos (rosa, púrpura, cian, ámbar).
  const SPARKLE_COLORS = ["#f5c518", "#f5c518", "#ffd76a", "#ff8be4", "#a78bff", "#8be0ff"];
  const SPARKLES = Array.from({ length: SPARKLE_COUNT }, (_, i) => ({
    i,
    x: Math.round(5 + Math.random() * 90),
    y: Math.round(5 + Math.random() * 90),
    glyph: ["✦", "✧", "⋆", "★", "✺", "·"][Math.floor(Math.random() * 6)],
    delay: Math.round(Math.random() * 800),
    size: 14 + Math.round(Math.random() * 18),
    color: SPARKLE_COLORS[Math.floor(Math.random() * SPARKLE_COLORS.length)],
  }));
  // Mide cuántas columnas tiene el grid en tiempo real para calcular cuántos
  // ghost-slots renderizar (rellenar la fila + opcionalmente la próxima).
  let gridEl: HTMLDivElement | null = $state(null);
  let gridCols = $state(6);
  let gridResizeObs: ResizeObserver | null = null;
  // Contenedor con el scroll real del grid. Lo necesitamos para guardar y
  // restaurar la posición al salir/volver de la ruta.
  let gridWrapEl: HTMLDivElement | null = $state(null);
  // Scroll pendiente de restaurar tras volver de otra ruta (-1 = nada).
  let scrollPendiente = -1;
  // Último scroll conocido del grid. Plano (sin runa) a propósito: lo escribe
  // el onscroll y no debe disparar re-renders. Hace falta porque <main> se
  // desmonta entero al entrar al player/menú, y al volver el contenedor nace
  // con scrollTop 0.
  let scrollGrid = 0;
  function measureGridCols() {
    if (!gridEl) return;
    const tc = getComputedStyle(gridEl).gridTemplateColumns;
    const cols = tc.split(" ").filter((x) => x && x !== "0px").length;
    if (cols > 0) gridCols = cols;
  }
  // ghostCount: slots vacíos en la fila actual; si la fila está exacta,
  // mostramos una fila completa de fantasmas como "lo que viene".
  const ghostCount = $derived.by(() => {
    if (!hasMore || !items.length) return 0;
    const rem = items.length % gridCols;
    return rem === 0 ? gridCols : gridCols - rem;
  });
  function startSparkles() {
    sparkling = true;
    if (sparkleTimer) clearTimeout(sparkleTimer);
    // Safety: si loadMore se cuelga, cortar a los 4s.
    sparkleTimer = setTimeout(() => { sparkling = false; }, 4000);
  }
  $effect(() => {
    // Autoapaga cuando termina la carga.
    if (sparkling && !loadingMore) {
      if (sparkleTimer) { clearTimeout(sparkleTimer); sparkleTimer = null; }
      // Pequeño delay para que las chispas no desaparezcan en seco.
      const t = setTimeout(() => { sparkling = false; }, 350);
      return () => clearTimeout(t);
    }
  });

  type SortOpt = { id: string; label: string; movie: string; tv: string };
  const SORTS: SortOpt[] = [
    { id: "popular",   label: "Popular",         movie: "popularity.desc",          tv: "popularity.desc" },
    { id: "top",       label: "Mejor valoradas", movie: "vote_average.desc",        tv: "vote_average.desc" },
    { id: "voted",     label: "Más votadas",     movie: "vote_count.desc",          tv: "vote_count.desc" },
    { id: "new",       label: "Más recientes",   movie: "primary_release_date.desc", tv: "first_air_date.desc" },
    { id: "old",       label: "Más antiguas",    movie: "primary_release_date.asc",  tv: "first_air_date.asc" },
    { id: "az",        label: "A → Z",           movie: "original_title.asc",        tv: "name.asc" },
    { id: "za",        label: "Z → A",           movie: "original_title.desc",       tv: "name.desc" },
  ];
  // Orden extra solo-anime: TRENDING_DESC de AniList. Útil con temporada en
  // curso, donde el score aún no estabiliza. movie/tv son dummies (no se usan).
  const ANIME_SORTS: SortOpt[] = [
    { id: "trending", label: "Tendencia", movie: "popularity.desc", tv: "popularity.desc" },
  ];
  function currentMediaTypeForSort(): "movie" | "tv" { return tabToMediaType(tab); }
  let sortId = $state<string>("popular");
  let sortOpen = $state(false);
  const sortOptions = $derived(
    tab === "anime" ? [SORTS[0], ...ANIME_SORTS, ...SORTS.slice(1)] : SORTS,
  );

  // Temporada anime (cour AniList). "" = todas. Lista fija al montar.
  const SEASON_OPTS = animeSeasonOptions();
  let seasonId = $state<string>("");
  let seasonOpen = $state(false);
  const seasonSel = $derived(SEASON_OPTS.find((s) => s.id === seasonId) ?? null);

  let genres = $state<{ id: number; name: string }[]>([]);
  let selectedGenres = $state<Set<number>>(new Set());

  // Filtros recordados entre sesiones, uno por tab.
  //
  // Validar al leer no es opcional: "trending" solo existe en anime, y las
  // temporadas de anime se calculan a partir de la fecha de hoy, así que la que
  // el user dejó guardada puede haber salido de la lista. Un valor inválido
  // deja el desplegable mostrando algo que no está en sus opciones.
  function aplicarFiltrosGuardados(t: Tab) {
    const f = leerFiltros(t);
    const opciones = t === "anime" ? [...SORTS, ...ANIME_SORTS] : SORTS;
    sortId = f && opciones.some((o) => o.id === f.sortId) ? f.sortId : "popular";
    seasonId =
      t === "anime" && f && SEASON_OPTS.some((o) => o.id === f.seasonId) ? f.seasonId : "";
    selectedGenres = new Set(f?.genres ?? []);
  }

  function persistirFiltros() {
    guardarFiltros(tab, {
      sortId,
      seasonId,
      genres: Array.from(selectedGenres),
    });
  }

  let items = $state<ListItem[]>([]);
  let focusedIdx = $state(0);
  let cardEls: (HTMLButtonElement | null)[] = $state([]);
  let listLoading = $state(false);
  let listError = $state("");
  let initialFocusPending = $state(true);

  // Card real más cercana a idx. Los títulos con status "none" no se
  // renderizan, así que cardEls tiene huecos: buscamos hacia afuera antes de
  // caer en la primera.
  function cardCercana(idx: number): HTMLButtonElement | null {
    if (cardEls[idx]) return cardEls[idx];
    for (let d = 1; d < cardEls.length; d++) {
      const a = cardEls[idx - d];
      if (a) return a;
      const b = cardEls[idx + d];
      if (b) return b;
    }
    return null;
  }

  // Devuelve el foco a la card donde estaba el user. Se llama al cerrar
  // cualquier overlay (player, menú, persona, carrusel): sin esto el foco cae
  // en body, se dispara el focus-guard y el catálogo saltaba a la primera card.
  function volverAlGrid() {
    const el = cardCercana(focusedIdx);
    if (!el) return;
    el.focus({ preventScroll: true });
    // preventScroll + scrollTop guardado: la vista queda idéntica a como se
    // dejó. Si no hay scroll guardado, basta con asegurar que la card se vea.
    if (gridWrapEl && scrollGrid > 0) aplicarScrollGrid(gridWrapEl, scrollGrid);
    else el.scrollIntoView({ block: "nearest", behavior: "auto" });
  }

  // Devuelve el scroll al contenedor, con un reintento en el próximo frame:
  // si el grid todavía no tenía toda su altura, el navegador recorta el valor
  // al máximo scrolleable de ese instante y quedaríamos a medio camino.
  function aplicarScrollGrid(el: HTMLDivElement, top: number) {
    if (top <= 0) return;
    el.scrollTop = top;
    requestAnimationFrame(() => {
      if (gridWrapEl === el && el.scrollTop < top) el.scrollTop = top;
    });
  }

  // El contenedor del grid renace con scrollTop 0 cada vez que <main> deja
  // paso al player, al menú de fuentes o al listado de capítulos. Restaurarlo
  // acá — cuando el elemento aparece — cubre TODOS los caminos de vuelta de
  // una vez. Hacerlo en cada cierre se olvidaba de la mitad: closeSources y
  // closeSeasonView mandan el foco al panel de info, nunca llaman a
  // volverAlGrid, y el catálogo aparecía rebobinado al principio.
  $effect(() => {
    const el = gridWrapEl;
    if (!el) return;
    // scrollGrid es plano a propósito: acá solo se lee al montar el elemento.
    aplicarScrollGrid(el, scrollGrid);
  });

  $effect(() => {
    if (!initialFocusPending) return;
    if (listLoading) return;
    if (showKey) return;
    // Al volver de otra ruta queremos la card donde estábamos, no la primera.
    // Si esa card todavía no montó, esperamos al próximo flush.
    const idx = focusedIdx;
    const el = cardCercana(idx);
    if (!el) return;
    initialFocusPending = false;
    if (scrollPendiente >= 0) {
      // Restaurar scroll ANTES de que el foco lo mueva: preventScroll evita
      // que el navegador reposicione y el scrollTop guardado manda.
      el.focus({ preventScroll: true });
      const top = scrollPendiente;
      scrollPendiente = -1;
      if (gridWrapEl) aplicarScrollGrid(gridWrapEl, top);
    } else {
      // Con scroll guardado, enfocar sin preventScroll rebobina lo restaurado.
      el.focus({ preventScroll: scrollGrid > 0 });
      focusedIdx = idx;
      if (gridWrapEl && scrollGrid > 0) aplicarScrollGrid(gridWrapEl, scrollGrid);
    }
  });

  $effect(() => {
    const reproduciendo =
      mode === "discover" || mode === "trailer" || mode === "sources" ||
      mode === "episodes" || mode === "playmenu";
    setPlaying(reproduciendo);
    // Pausamos el screening mientras hay video: evita que el worker robe
    // red al stream del iframe y cause cortes/saltitos.
    void pausarScreening(reproduciendo);
  });

  // Cache de status por id: undefined=no chequeado, "checking", "ok"=video disponible,
  // "trailer"=solo trailer (badge), "none"=ocultar
  type CardStatus = "checking" | "ok" | "trailer" | "none";
  let statusMap = $state<Map<number, CardStatus>>(new Map());
  let imdbIdMap = $state<Map<number, string>>(new Map());
  // Nº de temporadas por id (solo series). Llega gratis en item_status.
  let seasonsMap = $state<Map<number, number>>(new Map());

  // URL del servidor web (si está activo) para mostrar QR del mando remoto
  // sobre la carátula del título seleccionado. Poll periódico para reflejar
  // auto-start, start manual o stop sin refrescar la página.
  let webRemoteUrl = $state<string | null>(null);
  let webStatusTimer: ReturnType<typeof setInterval> | null = null;
  // onMount es async: lo que registra después de un await tiene que saber si
  // el componente ya se desmontó en el medio.
  let desmontado = false;
  async function refreshWebStatus() {
    try {
      const s = await invoke<{ running: boolean; url: string | null }>("web_server_status");
      webRemoteUrl = s.running ? s.url : null;
    } catch {
      webRemoteUrl = null;
    }
  }
  // URL del teclado web (escribir/pegar/escanear la key desde el celular).
  let webKeyboardUrl = $derived(webRemoteUrl ? `${webRemoteUrl}/api` : null);
  // Primer arranque sin key: levanta el servidor web aunque el autostart esté
  // apagado, para que el QR del teclado funcione y el cliente no quede atado de
  // manos teniendo que escribir 32 chars con el control.
  async function ensureWebServer() {
    try {
      let s = await invoke<{ running: boolean; url: string | null }>("web_server_status");
      if (!s.running) {
        const wp = parseInt(localStorage.getItem("web_port") || "8080", 10);
        s = await invoke("web_server_start", { port: Number.isFinite(wp) ? wp : 8080 });
      }
      webRemoteUrl = s.url;
    } catch {
      // Puerto ocupado u otra falla: el QR simplemente no aparece.
    }
  }

  const statusInflight = new Set<number>();
  let statusObserver: IntersectionObserver | null = null;

  let sentinelEl: HTMLDivElement | null = null;
  let observer: IntersectionObserver | null = null;

  let selected = $state<Detail | null>(null);
  let detailLoading = $state(false);

  // --- Historial del título abierto -----------------------------------------
  // El anime de AniList puede no tener imdb y aun así reproducirse (kitsu):
  // claveMedio() le da una clave sintética para que igual quede registrado.
  const claveSel = $derived(
    selected ? (selected.is_anime ? claveMedio(null, selected.id) : claveMedio(selected.imdb_id)) : "",
  );
  // Derivado del store: cualquier escritura del tracker se refleja sola, sin
  // recargar la fila a mano después de cada guardado.
  const progressForSelected = $derived<FilaHistorial | null>(
    claveSel ? filaPelicula(claveSel) : null,
  );
  const estadoSel = $derived(claveSel ? estadoDe(claveSel) : null);
  // Lo que pone al título en "seguir viendo": la película a medias, o el
  // capítulo a medias si es una serie.
  const enCursoSel = $derived(claveSel ? filaEnCurso(claveSel) : null);
  const favSel = $derived(claveSel ? esFavorito(claveSel) : false);
  // ¿Este título ya está bajado en el equipo? La ficha lo dice antes de que el
  // usuario pulse nada: bajarla dos veces es el error que esto evita.
  const localSel = $derived(claveSel ? tieneDescarga(claveSel) : false);
  // Solo para películas: en series la descarga es por capítulo y el título no
  // tiene un estado único que mostrar.
  const bajandoSel = $derived(
    claveSel && selected?.media_type === "movie"
      ? estadoDescarga(claveSel) === "bajando"
      : false,
  );
  // Pedida pero todavía sin empezar: espera que se libere un lugar.
  const enColaSel = $derived(
    claveSel && selected?.media_type === "movie" ? yaEnCola(claveSel) : false,
  );

  // Metadatos que el historial necesita para escribir una fila. `season`/
  // `episode` en -1 = película (o el título entero, cuando el marcado es del
  // título y no de un capítulo).
  function metaDe(season = -1, episode = -1, ep?: EpisodeMini | null): MediaMeta | null {
    if (!selected || !claveSel) return null;
    return {
      clave: claveSel,
      tmdbId: selected.id,
      mediaType: selected.is_anime ? "anime" : selected.media_type,
      title: selected.title,
      posterPath: selected.poster_path ?? null,
      season,
      episode,
      stillPath: ep?.still_path ?? null,
      episodeTitle: ep?.name ?? null,
      runtimeSeconds: season >= 0 ? null : selected.runtime ? selected.runtime * 60 : null,
    };
  }

  let mode = $state<"browse" | "discover" | "trailer" | "unavailable" | "sources" | "playmenu" | "episodes" | "fin">("browse");
  let checkingDiscover = $state(false);
  // Pantalla de último recurso: QR al video de YouTube.
  let trailerQr = $state<string>("");
  let trailerQrUrl = $state<string>("");
  // Datos extra para PlayMenu: OMDb (premios/ratings/plot) + trailer pre-cargado.
  let menuOmdb = $state<OmdbDetail | null>(null);
  let menuTrailerPick = $state<TrailerPick>({
    ytKey: "",
    apple: "",
    playable: false,
    err: "",
    video: "",
    audio: "",
    at: 0,
  });
  const omdbCache = new Map<string, OmdbDetail>();
  const trailerCache = new Map<string, TrailerPick>();
  let unavailable = $state<{ open: boolean; reason: "404" | "no_imdb"; checking: boolean }>({
    open: false,
    reason: "404",
    checking: false,
  });

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(async () => {
    // CRÍTICO: leer la TMDb key SYNC al PRINCIPIO, antes de cualquier await.
    // afterNavigate puede correr mientras onMount sigue esperando Database.load,
    // y handoffPlay necesita la key. Si esto se hace después del await, hay
    // race: handoffPlay aborta por "sin key" aunque la key esté en localStorage.
    {
      const k0 = localStorage.getItem("tmdb_key") || "";
      apiKey = k0;
      keyInput = k0;
      showKey = !k0;
    }

    // Volver de otra ruta (/config, /vera, /iptv) remonta esta página.
    // Si hay snapshot, restauramos el catálogo tal cual estaba en vez de
    // recargarlo desde la página 1. Sync y antes de cualquier await: el
    // afterNavigate del handoff de Vera puede correr entremedio.
    const snap = leerCatalogo(apiKey);
    if (snap) {
      tab = snap.tab;
      query = snap.query;
      debouncedQ = snap.debouncedQ;
      page = snap.page;
      totalPages = snap.totalPages;
      hasMore = snap.hasMore;
      sortId = snap.sortId;
      seasonId = snap.seasonId;
      genres = snap.genres;
      selectedGenres = new Set(snap.selectedGenres);
      items = snap.items;
      cardEls = new Array(snap.items.length).fill(null);
      statusMap = new Map(snap.statusMap);
      imdbIdMap = new Map(snap.imdbIdMap);
      seasonsMap = new Map(snap.seasonsMap);
      restaurarPremios(snap.awardsMap);
      focusedIdx = Math.min(Math.max(snap.focusedIdx, 0), snap.items.length - 1);
      scrollPendiente = snap.scrollTop;
      scrollGrid = snap.scrollTop;
      listLoading = false;
      initialFocusPending = true;
    } else {
      // Arranque real de la app: los filtros salen de localStorage.
      aplicarFiltrosGuardados(tab);
      // Y la página 1 que quedó de la última vez se pinta YA, sin esperar a la
      // red. La carga de verdad corre abajo en silencio y reemplaza esto
      // cuando llega (stale-while-revalidate).
      const p1 = apiKey ? leerPrimeraPagina(firmaLista()) : null;
      if (p1) {
        items = p1;
        cardEls = new Array(p1.length).fill(null);
        listLoading = false;
        initialFocusPending = true;
      }
    }

    // Registrar los atajos del home en la ayuda global.
    // Esc y Backspace en browse no hacen nada (ya estás en la raíz).
    // En discover/trailer: Esc o Backspace cierran y vuelven a browse.
    ayuda.set("inicio", [
      { tecla: "← → ↑ ↓", desc: "Navegar entre cards" },
      { tecla: "Enter · Espacio", desc: "Abrir / descubrir" },
      { tecla: "I", desc: "Saltar a panel info / volver a cards" },
      { tecla: "[ ] · RePág AvPág", desc: "Cambiar pestaña · temporada (en capítulos)" },
      { tecla: "Home · End", desc: "Primera / última card" },
      { tecla: "Esc · Backspace", desc: "Cerrar player (en discover)" },
      { tecla: "I-I", desc: "Ayuda" },
    ]);
    try {
      db = await Database.load("sqlite:kutral.db");
      console.log("[db] kutral.db abierta OK");
      await loadUnavailableSet();
    } catch (e) {
      console.error("[db] FALLO al abrir:", e);
    }
    // Si el user salió del home mientras se abría la base, onDestroy ya corrió:
    // lo que se registre de acá en adelante no lo limpiaría nadie (y cada
    // vuelta al home sumaría otro intervalo de 4 s).
    if (desmontado) return;

    // Listener postMessage por si playimdb coopera
    window.addEventListener("message", onIframeMessage);

    // apiKey ya fue leída sync al principio del onMount para evitar race
    // con afterNavigate. Acá solo cargamos lo demás.
    // Con snapshot restaurado no se recarga nada: items, géneros y estados de
    // disponibilidad ya están en memoria.
    if (apiKey && !snap) {
      loadGenres();
      // Si ya pintamos la página 1 guardada, la recarga va silenciosa: el user
      // está viendo cards y no tiene por qué ver un estado de carga encima.
      const silenciosa = items.length > 0;
      page = 1;
      hasMore = true;
      void loadList(false, silenciosa);
    }

    // QR del mando remoto: refrescar estado del servidor cada 4s.
    // Sin key (primer uso): arranca el servidor ya, para ofrecer el teclado web.
    if (!apiKey) void ensureWebServer();
    else refreshWebStatus();
    webStatusTimer = setInterval(refreshWebStatus, 4000);

    // Observar cambios de tamaño del grid para recalcular columnas (ghost slots).
    gridResizeObs = new ResizeObserver(() => measureGridCols());

    // Si el foco se pierde (cae en body), devolverlo a la primera card real
    // (cardEls[0] = la peli a la derecha de la card de Vera). Solo en browse
    // y sin overlays activos: no robar foco a modales o inputs.
    document.addEventListener("focusout", scheduleFocusGuard);

    // El handoff de Vera (B5) NO va acá: onMount solo corre una vez en la
    // SPA, y Vera navega cliente-side con goto(). Movido a afterNavigate
    // abajo, que sí corre en cada navegación.
  });

  // El reproductor web (iframe) no le avisa al escritorio que hay video en
  // pantalla; sin esto KDE bloquea o suspende a mitad de la película.
  $effect(() => {
    if (mode !== "discover") return;
    inhibirReposo("web", true);
    return () => inhibirReposo("web", false);
  });

  // Cleanup separado en onDestroy: onMount es async y no acepta return
  // de cleanup en ese caso. El listener postMessage del iframe player
  // tiene que limpiarse cuando home desmonta.
  onDestroy(() => {
    desmontado = true;
    guardarSnapshotCatalogo();
    volcarImgCache();
    if (calentarTimer) {
      clearTimeout(calentarTimer);
      calentarTimer = null;
    }
    window.removeEventListener("message", onIframeMessage);
    if (webStatusTimer !== null) {
      clearInterval(webStatusTimer);
      webStatusTimer = null;
    }
    gridResizeObs?.disconnect();
    gridResizeObs = null;
    document.removeEventListener("focusout", scheduleFocusGuard);
  });

  // Congela el catálogo antes de desmontar (navegación a otra ruta) para que
  // al volver el user caiga donde estaba: misma página, mismo scroll, misma
  // card, y sin re-chequear disponibilidad ni premios.
  //
  // No guardamos los estados en vuelo ("checking" / "loading"): sus requests
  // mueren con la página y al restaurar quedarían clavados para siempre.
  function guardarSnapshotCatalogo() {
    guardarCatalogo({
      apiKey,
      tab,
      query,
      debouncedQ,
      page,
      totalPages,
      hasMore,
      sortId,
      seasonId,
      genres,
      selectedGenres: Array.from(selectedGenres),
      items,
      focusedIdx,
      scrollTop: gridWrapEl?.scrollTop ?? scrollGrid,
      statusMap: Array.from(statusMap).filter(([, v]) => v !== "checking"),
      imdbIdMap: Array.from(imdbIdMap),
      seasonsMap: Array.from(seasonsMap),
      awardsMap: premiosResueltos(),
    });
  }

  // Reasegura foco en la primera card del catálogo cuando se pierde.
  // setTimeout(0) para leer activeElement DESPUÉS de que el navegador haya
  // movido el foco al destino del focusout (suele ser body si nadie lo agarró).
  let focusGuardScheduled = false;
  function scheduleFocusGuard() {
    if (focusGuardScheduled) return;
    focusGuardScheduled = true;
    setTimeout(() => {
      focusGuardScheduled = false;
      if (mode !== "browse") return;
      if (carousel || personOpen || unavailable.open || sortOpen) return;
      if (showKey) return;
      if (document.visibilityState !== "visible") return;
      const cur = document.activeElement as HTMLElement | null;
      // Tiene foco navegable real → no tocar.
      if (cur && cur !== document.body && cur.matches?.("[data-nav]")) return;
      // Tampoco intervenir si está en un input no marcado data-nav.
      const tag = cur?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      // Estricto: SOLO una card del catálogo. Nada de tabs/búsqueda/info
      // como fallback — esos roles confunden al user. Si aún no hay card
      // montada, esperar (el $effect inicial la enfoca apenas se monte).
      //
      // Va a la ÚLTIMA card enfocada, no a la primera: el guard salta cada vez
      // que se cierra un overlay, y mandar el foco a la card 0 rebobinaba el
      // grid al principio y cambiaba el panel de info.
      const target = cardCercana(focusedIdx);
      target?.focus({ preventScroll: true });
    }, 0);
  }

  // Ligar/desligar el observer al elemento real cuando cambia.
  // Capturamos las refs locales para que el cleanup use el MISMO elemento
  // que se observó (gridEl puede ser null al desmontar y unobserve(null) tira
  // TypeError, lo que rompe toda la cadena reactiva de Svelte 5).
  $effect(() => {
    const el = gridEl;
    const obs = gridResizeObs;
    if (el && obs) {
      obs.observe(el);
      measureGridCols();
      return () => {
        try { obs.unobserve(el); } catch { /* tolerado */ }
      };
    }
  });

  // Handoff de Vera (B5): se dispara en CADA navegación cliente, incluyendo
  // el load inicial. SvelteKit no remonta este +page.svelte al hacer goto()
  // desde /vera, así que onMount no veía la URL nueva. afterNavigate sí.
  //
  // Usamos afterNavigate y NO un $effect sobre $page.url para evitar el
  // ciclo: replaceState abajo cambia la URL, un $effect lo vería y
  // re-dispararía. afterNavigate ignora replaceState (es un cambio sin
  // SvelteKit navigation event), entonces corre una sola vez por descubrir.
  afterNavigate(() => {
    const url = new URL(window.location.href);
    // Tab inicial vía ?tab= (las cards de sección apuntan a /?tab=tv, etc.).
    const tabParam = url.searchParams.get("tab");
    if (tabParam === "movie" || tabParam === "tv" || tabParam === "anime") {
      history.replaceState({}, "", "/");
      if (tab !== tabParam) switchTab(tabParam);
    }
    const playId = url.searchParams.get("play");
    const playType = url.searchParams.get("type");
    const playSeason = url.searchParams.get("season");
    const playEpisode = url.searchParams.get("episode");
    const desde = url.searchParams.get("from");
    if (playId && playType) {
      // CRÍTICO: limpiar ?play= ANTES de invocar tmdb_detail. Sin esto,
      // un refresh durante o tras el handoff re-dispara el play solo.
      // replaceState NO triggea afterNavigate (no es una nav real), así
      // que no entramos en loop.
      history.replaceState({}, "", "/");
      void handoffPlay(playId, playType, playSeason, playEpisode, desde);
    }
  });

  // Handoff Vera → home: resuelve un tmdb_id en Detail y entra al modo
  // discover (el flujo real de Descubrir). Reusa la misma maquinaria que
  // usa la home cuando se clickea una card (tmdb_detail → selected →
  // startDiscover). Si cualquier paso falla, el user queda en browse
  // SIN peli seleccionada — nunca con un selected viejo simulando ser la
  // peli pedida.
  async function handoffPlay(
    idStr: string,
    typeStr: string,
    seasonStr: string | null = null,
    episodeStr: string | null = null,
    desde: string | null = null,
  ) {
    // La vuelta se arma recién cuando hay algo que cerrar (ver más abajo): un
    // handoff que falla deja al user en la home, y ahí un Esc no debe saltar a
    // una ruta que nadie pidió.
    volverDesde = "";
    // Limpiar de entrada: si algo de abajo falla, no queremos mostrar
    // la peli que estaba seleccionada antes (de un click previo) como si
    // fuera la del handoff.
    selected = null;
    mode = "browse";

    if (!apiKey) {
      console.warn("[handoff] sin api key — abortando");
      return;
    }
    const id = parseInt(idStr, 10);
    if (!Number.isFinite(id)) {
      console.warn("[handoff] id inválido:", idStr);
      return;
    }
    if (typeStr !== "movie" && typeStr !== "tv" && typeStr !== "anime") {
      console.warn("[handoff] type inválido:", typeStr);
      return;
    }
    // El anime vive en otro catálogo (AniList) y currentKind() lo deduce del
    // tab: sin cambiarlo, el scraper buscaría la serie como si fuera TMDb.
    if (typeStr === "anime") switchTab("anime");
    try {
      const d =
        typeStr === "anime"
          ? await invoke<Detail>("anilist_detail", { id, apiKey })
          : await invoke<Detail>("tmdb_detail", { mediaType: typeStr, id, apiKey });
      selected = d;
      // Con capítulo en la URL (viene del historial) se salta el selector de
      // temporada y el menú apunta directo a ese episodio.
      const se = seasonStr != null ? parseInt(seasonStr, 10) : NaN;
      const ep = episodeStr != null ? parseInt(episodeStr, 10) : NaN;
      if (Number.isFinite(se) && Number.isFinite(ep) && se >= 0) {
        sourcesSeason = se;
        sourcesEpisode = ep;
        setContexto(metaDe(se, ep));
      } else {
        sourcesSeason = null;
        sourcesEpisode = null;
      }
      if (desde === "historial") volverDesde = "/historial";
      goDescubrir();
    } catch (e) {
      console.warn("[handoff] falló:", e);
      selected = null;
      mode = "browse";
      volverDesde = "";
    }
  }

  // Throttle escrituras a SQLite: el player emite player_status='playing'
  // varias veces por segundo. Guardamos como máximo cada 5s.
  let lastSaveAt = 0;

  function onIframeMessage(e: MessageEvent) {
    if (typeof e.data !== "object" || !e.data) return;
    if (!vieneDelReproductor(e.source)) return;

    // El player (VidAPI) usa parentStorage proxy. Cuando inicia pide al padre
    // STORAGE_GET_ALL. Si respondemos con STORAGE_INIT incluyendo
    // playerSubStyle, aplica nuestras preferencias (tamaño, color, fuente)
    // antes de mostrar el primer subtítulo.
    if (e.data.type === "STORAGE_GET_ALL") {
      const subStyle = JSON.stringify({
        color: "#ffffff",
        bg: "#000000",
        bgAlpha: "80",
        size: String(config.subSize),
        font: "system-ui",
      });
      try {
        (e.source as Window)?.postMessage(
          {
            type: "STORAGE_INIT",
            data: { playerSubStyle: subStyle },
          },
          // Solo a quien preguntó. "null" es un frame con sandbox sin origen
          // propio: ahí no queda otra que "*".
          e.origin && e.origin !== "null" ? e.origin : "*",
        );
      } catch (err) {
        console.warn("[storage-init] no se pudo responder:", err);
      }
      return;
    }

    if (!selected?.imdb_id) return;

    // VidAPI PLAYER_EVENT (docs vidapi.ru/api). Emite info real del <video>:
    // player_progress = currentTime en segundos, player_duration = duration.
    // player_status: 'playing' | 'paused' | 'seeked' | 'completed' | ...
    if (e.data.type === "PLAYER_EVENT" && e.data.data) {
      const { player_status, player_progress, player_duration } = e.data.data;
      const t = Number(player_progress);
      const d = Number(player_duration);
      if (!isFinite(t) || t <= 0) return;
      const now = Date.now();
      // Guarda inmediato en eventos importantes; throttle a 5s en playing.
      const importante =
        player_status === "paused" ||
        player_status === "seeked" ||
        player_status === "completed" ||
        player_status === "ended";
      if (!importante && now - lastSaveAt < 5000) return;
      lastSaveAt = now;
      saveProgress({
        watched_seconds: Math.floor(t),
        runtime_seconds: isFinite(d) && d > 0 ? Math.floor(d) : null,
        progress_real: isFinite(d) && d > 0 ? t / d : null,
      });
      return;
    }

    // Compatibilidad con otros providers que emitan {t, d} simples.
    const t = Number(e.data.t ?? e.data.currentTime ?? e.data.position);
    const d = Number(e.data.d ?? e.data.duration);
    if (!isFinite(t) || t <= 0) return;
    saveProgress({
      watched_seconds: Math.floor(t),
      runtime_seconds: isFinite(d) && d > 0 ? Math.floor(d) : null,
      progress_real: isFinite(d) && d > 0 ? t / d : null,
    });
  }

  // El player web (iframe) reporta {t,d}: se guarda por el mismo camino que
  // mpv, contra el store del historial. Ver features.ts — hoy está apagado.
  async function saveProgress(partial: {
    watched_seconds: number;
    runtime_seconds?: number | null;
    progress_real?: number | null;
  }) {
    const meta = metaDe();
    if (!meta) return;
    await guardarProgreso(meta, {
      watched: partial.watched_seconds,
      runtime: partial.runtime_seconds ?? null,
      real: partial.progress_real ?? null,
    });
  }

  async function clearProgressFor(clave: string) {
    await borrarProgreso(clave, -1, -1);
  }

  async function loadUnavailableSet() {
    // Fuente de verdad: Rust screening (lee unavailable_items y emite events
    // a medida que detecta más). Suscribimos antes de cargar para no perder
    // resultados que lleguen mientras carga.
    try {
      await suscribirScreening((ev) => {
        if (ev.disponible) return;
        const s = new Set(unavailableSet);
        s.add(ev.imdbId);
        unavailableSet = s;
      });
      const ids = await cargarNoDisponiblesIniciales();
      const s = new Set(unavailableSet);
      for (const id of ids) s.add(id);
      unavailableSet = s;
    } catch (e) {
      console.warn("[screening] init fail", e);
    }
  }

  async function markUnavailable(imdb_id: string) {
    if (!db) return;
    try {
      const kind = tabToMediaType(tab);
      await db.execute(
        `INSERT OR REPLACE INTO unavailable_items (imdb_id, kind, detected_at) VALUES ($1, $2, $3)`,
        [imdb_id, kind, Date.now()]
      );
      const s = new Set(unavailableSet);
      s.add(imdb_id);
      unavailableSet = s;
    } catch (e) {
      console.warn("[db] markUnavailable error", e);
    }
  }

  async function clearUnavailable(imdb_id: string) {
    if (!db) return;
    try {
      await db.execute("DELETE FROM unavailable_items WHERE imdb_id = $1", [imdb_id]);
      const s = new Set(unavailableSet);
      s.delete(imdb_id);
      unavailableSet = s;
    } catch (e) {
      console.warn("[db] clearUnavailable error", e);
    }
  }

  // Reproduce el trailer en mpv. Devuelve false si mpv no lo aceptó.
  async function playTrailerInMpv(url: string, title: string, audioUrl = ""): Promise<boolean> {
    // Un trailer no es "haber visto la película": sin limpiar el contexto, dos
    // minutos de avance marcarían el título como empezado.
    limpiarContexto();
    try {
      // mpv_play_trailer, no mpv_play: un trailer no reemplaza la película que
      // estuviera esperando ni se queda él mismo en la pill al salir.
      await invoke("mpv_play_trailer", { url, title: `Trailer — ${title}`, audioUrl });
      setPlaying(true);
      return true;
    } catch (e) {
      console.warn("[mpv_play trailer]", e);
      return false;
    }
  }

  // Último recurso: pantalla con QR para ver el trailer en el celular.
  async function showTrailerQr(key: string) {
    trailerQrUrl = `https://youtu.be/${key}`;
    try {
      trailerQr = await QRCode.toDataURL(trailerQrUrl, { width: 420, margin: 1 });
    } catch (e) {
      console.warn("[qr]", e);
      trailerQr = "";
      trailerMsg = "No se pudo generar el QR del trailer.";
      setTimeout(() => (trailerMsg = ""), 4000);
      return;
    }
    mode = "trailer";
    setFs(true);
    registerBackShortcuts(true);
    setTimeout(
      () =>
        document
          .querySelector<HTMLElement>(".trailer-open, .trailer-bar .bar-btn")
          ?.focus(),
      50,
    );
  }

  // Abre el trailer en el navegador del sistema. Kütral corre fullscreen: al
  // volver, el webview puede quedar sin foco y el mando dejaría de responder,
  // así que lo reponemos sobre el botón.
  async function abrirTrailerWeb() {
    if (!trailerQrUrl) return;
    try {
      await openUrl(trailerQrUrl);
    } catch (e) {
      console.warn("[openUrl trailer]", e);
      trailerMsg = "No se pudo abrir el navegador.";
      setTimeout(() => (trailerMsg = ""), 4000);
      return;
    }
    setTimeout(() => document.querySelector<HTMLElement>(".trailer-open")?.focus(), 400);
  }

  /** Flechas en la pantalla del trailer: cicla entre sus botones. */
  function moverFocoTrailer(delta: number) {
    const btns = Array.from(
      document.querySelectorAll<HTMLElement>(".trailer-qr-mode [data-nav]"),
    );
    if (!btns.length) return;
    const i = btns.indexOf(document.activeElement as HTMLElement);
    btns[(i + delta + btns.length) % btns.length].focus();
  }

  function saveKey() {
    apiKey = keyInput.trim();
    localStorage.setItem("tmdb_key", apiKey);
    showKey = false;
    loadGenres();
    resetAndLoad();
  }

  function onSearchInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debouncedQ = query.trim();
      resetAndLoad();
    }, 400);
  }

  function switchTab(t: Tab) {
    if (tab === t) return;
    tab = t;
    selected = null;
    // Cada tab recuerda sus propios filtros. La validación de aplicar…() se
    // encarga de que "Tendencia" (solo anime) no se cuele en pelis/series.
    aplicarFiltrosGuardados(t);
    statusMap = new Map();
    // Estos dos también van por id, y el id cambia de universo entre pestañas
    // (TMDb ↔ AniList). Arrastrarlos mezclaba datos de un catálogo en el otro.
    imdbIdMap = new Map();
    seasonsMap = new Map();
    loadGenres();
    resetAndLoad(false);
  }

  function changeSort(id: string) {
    sortId = id;
    persistirFiltros();
    resetAndLoad();
  }

  function changeSeason(id: string) {
    seasonId = id;
    persistirFiltros();
    resetAndLoad();
  }

  function toggleGenre(id: number) {
    const s = new Set(selectedGenres);
    if (s.has(id)) s.delete(id); else s.add(id);
    selectedGenres = s;
    persistirFiltros();
    resetAndLoad();
  }

  function clearGenres() {
    if (selectedGenres.size === 0) return;
    selectedGenres = new Set();
    persistirFiltros();
    resetAndLoad();
  }

  // `conservar`: deja las cards viejas puestas (atenuadas) hasta que lleguen
  // las nuevas. Cambiar de orden o de género ya no vacía la pantalla.
  //
  // Al cambiar de TAB no se conserva: son de otro media_type y el observer de
  // las cards viejas pediría su status contra el tipo equivocado.
  function resetAndLoad(conservar = true) {
    page = 1;
    hasMore = true;
    focusedIdx = 0;
    if (!conservar) {
      items = [];
      cardEls = [];
    }
    // Lista nueva: el scroll y el foco guardados ya no aplican. El contenedor
    // no se desmonta cuando se conservan las cards viejas, así que hay que
    // subirlo a mano o el user queda mirando el medio de la lista anterior.
    scrollGrid = 0;
    scrollPendiente = -1;
    if (gridWrapEl) gridWrapEl.scrollTop = 0;
    loadList(false);
  }

  async function loadGenres() {
    // Anime: géneros fijos de AniList (strings estables, etiqueta en español).
    if (tab === "anime") {
      genres = ANIME_GENRES.map((g) => ({ id: g.id, name: g.name }));
      return;
    }
    if (!apiKey) return;
    try {
      const list = await invoke<{ id: number; name: string }[]>("tmdb_genres", {
        mediaType: tabToMediaType(tab),
        apiKey,
      });
      genres = list;
    } catch (e) {
      console.warn("genres falló", e);
      genres = [];
    }
  }

  function currentSortBy(): string {
    const s = SORTS.find((x) => x.id === sortId) || SORTS[0];
    return tabToMediaType(tab) === "movie" ? s.movie : s.tv;
  }

  // Fecha local YYYY-MM-DD. Tope para discover: si no se estrenó, no existe.
  function hoyISO(): string {
    const d = new Date();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const dia = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${dia}`;
  }

  // Tipos de release TMDb que cuentan como "salió": 2|3 cine, 4 digital,
  // 5 físico, 6 TV. Excluye 1 (premiere de festival). Solo aplica a movie.
  const RELEASE_TYPES_DISPONIBLES = "2|3|4|5|6";

  // Token de generación. Cada loadList se queda con su número al entrar; si
  // entremedio arranca otra (el user siguió tecleando, cambió de tab, de
  // género o de orden), la vieja descarta su respuesta al volver.
  //
  // Sin esto: escribes "batman" rápido → salen 2-3 búsquedas y la que responde
  // ÚLTIMA gana, no la que corresponde al texto actual. El grid terminaba
  // mostrando resultados de "batm" o de la lista anterior.
  let loadGen = 0;

  // Firma de la consulta actual. Identifica qué lista es esta para no pintar
  // la página 1 guardada bajo otros filtros.
  function firmaLista(): string {
    return [tab, sortId, seasonId, Array.from(selectedGenres).sort().join(",")].join("|");
  }

  // `silencioso`: recarga sin mostrar estado de carga. Lo usa la revalidación
  // de la página 1 guardada — el user ya está viendo cards, avisarle que
  // estamos recargando solo le sacaría la sensación de instantáneo.
  // Hay una carga de lista base (no-append) en vuelo. `listLoading` no alcanza:
  // la revalidación silenciosa no lo prende, y sin este flag el sentinel podía
  // disparar la página 2 mientras la 1 venía en camino — la respuesta de la 1
  // llegaba después y borraba lo apilado, dejando `page` adelantado.
  let cargaBase = false;

  async function loadList(append: boolean, silencioso = false) {
    if (!apiKey && tab !== "anime") return;
    const gen = ++loadGen;
    if (append) loadingMore = true;
    else {
      cargaBase = true;
      if (!silencioso) listLoading = true;
    }
    listError = "";
    try {
      const isSearch = !!debouncedQ;
      const mt = tabToMediaType(tab);
      const ext = tabExtras(tab);
      const resp: ListResp = tab === "anime"
        ? await invoke("anilist_discover", {
            page,
            sort: ANILIST_SORTS[sortId] ?? "POPULARITY_DESC",
            genres: anilistGenresCSV(selectedGenres) || undefined,
            search: debouncedQ || undefined,
            // Temporada fuera durante búsqueda, igual que sort/géneros.
            season: !isSearch && seasonSel ? seasonSel.season : undefined,
            seasonYear: !isSearch && seasonSel ? seasonSel.year : undefined,
          })
        : isSearch
        ? await invoke("tmdb_search", { mediaType: mt, query: debouncedQ, page, apiKey })
        : await invoke("tmdb_discover", {
            mediaType: mt,
            page,
            apiKey,
            sortBy: currentSortBy(),
            withGenres: Array.from(selectedGenres).join(","),
            originCountry: ext.originCountry,
            forceGenres: ext.forceGenres,
            // Sin estreno (cine/digital/físico/TV) hasta hoy → fuera.
            primaryReleaseDateLte: hoyISO(),
            withReleaseType: mt === "movie" ? RELEASE_TYPES_DISPONIBLES : undefined,
          });
      if (gen !== loadGen) return; // respuesta obsoleta: no pisar la actual
      totalPages = Math.min(resp.total_pages, 500);
      // Sin póster = ficha fantasma. Queda un resto que el piso de votos del
      // backend no atrapa (sobre todo en "Más antiguas": cine mudo real pero
      // sin arte). Una card sin imagen no se puede ni mirar ni elegir, así
      // que fuera del grid.
      const newItems = resp.results.filter((x) => !!x.poster_path);
      if (append) {
        // TMDb repite títulos entre páginas consecutivas (la popularidad
        // cambia entre requests). Sin dedup, la key duplicada en {#each}
        // rompe el render y el grid deja de actualizarse.
        const seen = new Set(items.map((x) => x.id));
        const fresh = newItems.filter((x) => !seen.has(x.id));
        items = [...items, ...fresh];
        cardEls = [...cardEls, ...new Array(fresh.length).fill(null)];
      } else {
        items = newItems;
        cardEls = new Array(newItems.length).fill(null);
        // Sin búsqueda y en la página 1: esto es lo que el user verá al abrir
        // la app la próxima vez.
        if (!isSearch && page === 1) guardarPrimeraPagina(firmaLista(), newItems);
      }
      // hasMore mira la página CRUDA, no la filtrada: una página que quedó
      // vacía por el filtro de póster no significa fin de lista.
      hasMore = page < totalPages && resp.results.length > 0;
    } catch (e) {
      if (gen !== loadGen) return; // el error es de una carga ya descartada
      listError = String(e);
      if (append) {
        // Error transitorio (429/red): revertir página y dejar hasMore
        // intacto para que el próximo scroll reintente.
        page -= 1;
      } else {
        items = [];
        hasMore = false;
      }
    } finally {
      // Solo la carga vigente apaga los spinners: si una vieja los apagara,
      // el grid se vería "listo" con la nueva todavía en vuelo.
      if (gen === loadGen) {
        listLoading = false;
        loadingMore = false;
        if (!append) cargaBase = false;
        // Colchón: apenas termina una lista nueva, la siguiente página se pide
        // sola. Cuando el user llega al fondo ya está puesta.
        if (!append && hasMore) setTimeout(() => void loadMore(), 0);
        // Y en segundo plano se resuelve el status de las cards que todavía no
        // se ven, para que bajar no dispare una ráfaga de peticiones.
        calentarStatus();
      }
    }
  }

  // Resuelve item_status de lo que aún no se ve, despacio y después de lo
  // visible. Con el caché en disco esto se paga una sola vez por título.
  let calentarTimer: ReturnType<typeof setTimeout> | null = null;
  function calentarStatus() {
    if (calentarTimer) clearTimeout(calentarTimer);
    calentarTimer = setTimeout(() => {
      calentarTimer = null;
      // Anime va por AniList: no hay item_status que calentar.
      if (tab === "anime") return;
      const gen = loadGen;
      // Tope: la lista puede tener cientos y no vale la pena adelantarlos todos.
      const pendientes = items
        .filter((it) => !statusMap.has(it.id))
        .slice(0, 60)
        .map((it) => it.id);
      if (!pendientes.length) return;
      let i = 0;
      const obrero = async () => {
        while (i < pendientes.length) {
          // La lista cambió (otro tab, otro filtro): lo que queda ya no sirve
          // y encima se consultaría con el media_type equivocado.
          if (gen !== loadGen) return;
          await checkItemStatus(pendientes[i++]);
        }
      };
      // Concurrencia 2: TMDb tira 429 con una grilla entera en paralelo.
      void Promise.all([obrero(), obrero()]);
    }, 1200);
  }

  async function loadMore() {
    if (!hasMore || loadingMore || listLoading || cargaBase) return;
    page += 1;
    await loadList(true);
  }

  // Prefetch: pedir la página siguiente al pasar el 60% del scroll, en vez de
  // esperar al sentinel del fondo. Cuando el user llega abajo las cards ya
  // están puestas y no ve el hueco cargando.
  function prefetchSiCorresponde() {
    const el = gridWrapEl;
    if (!el || !hasMore || loadingMore || listLoading || cargaBase) return;
    const recorrible = el.scrollHeight - el.clientHeight;
    if (recorrible <= 0) return;
    if (el.scrollTop / recorrible >= 0.6) void loadMore();
  }

  async function checkItemStatus(id: number) {
    // Anime (AniList): no hay item_status TMDb ni screening por imdb — la
    // disponibilidad la da la ruta debrid vía kitsu. Todo se marca ok.
    if (tab === "anime") {
      if (!statusMap.has(id)) {
        const m = new Map(statusMap);
        m.set(id, "ok");
        statusMap = m;
      }
      return;
    }
    if (statusMap.has(id) || statusInflight.has(id)) return;
    statusInflight.add(id);
    const next = new Map(statusMap);
    next.set(id, "checking");
    statusMap = next;
    try {
      const s: { id: number; has_imdb: boolean; imdb_id: string | null; has_trailer: boolean; number_of_seasons: number | null } =
        await invoke("item_status", { mediaType: tabToMediaType(tab), id, apiKey });
      const status: CardStatus = s.has_imdb ? "ok" : s.has_trailer ? "trailer" : "none";
      const m = new Map(statusMap);
      m.set(id, status);
      statusMap = m;
      if (s.number_of_seasons && s.number_of_seasons > 0) {
        const sm = new Map(seasonsMap);
        sm.set(id, s.number_of_seasons);
        seasonsMap = sm;
      }
      if (s.imdb_id) {
        const im = new Map(imdbIdMap);
        im.set(id, s.imdb_id);
        imdbIdMap = im;
        // Screening automático (vaplayer) desactivado: esa API dejó de ser
        // confiable como fuente de verdad y marcaba pelis sanas como no
        // disponibles. Única vía de reproducción real: debrid (PlayMenu).
        // El reporte manual del user ("Marcar como no disponible") sigue activo.
        // Premios/nominaciones (Wikidata). Cache + cola con concurrencia.
        enqueueAwards(s.imdb_id);
      }
    } catch (e) {
      console.warn("[item_status] error", id, e);
      // En error de red, asumimos ok para no ocultar todo
      const m = new Map(statusMap);
      m.set(id, "ok");
      statusMap = m;
    } finally {
      statusInflight.delete(id);
    }
  }

  let lastCardEl: HTMLElement | null = null;

  function toggleInfoFocus() {
    const cur = document.activeElement as HTMLElement | null;
    const isInInfo = cur?.closest('[data-section="info"]') != null;
    if (isInInfo) {
      // Volver a la última card focuseada
      const target = lastCardEl || cardEls[focusedIdx] || cardEls[0];
      target?.focus();
      target?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    } else {
      // Saltar al panel info
      if (cur && cardEls.includes(cur as HTMLButtonElement)) {
        lastCardEl = cur;
      }
      const info = document.querySelector<HTMLElement>('[data-section="info"] [data-nav]:not([disabled])');
      info?.focus();
      info?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }

  let autoPickTimer: ReturnType<typeof setTimeout> | null = null;
  function triggerAutoPick(i: number) {
    if (autoPickTimer) clearTimeout(autoPickTimer);
    autoPickTimer = setTimeout(() => {
      const it = items[i];
      if (it && document.activeElement === cardEls[i]) pick(it);
    }, 350);
  }

  async function openFilmographyItem(id: number, mediaType: string) {
    if (!apiKey) return;
    if (mediaType !== "movie" && mediaType !== "tv") return;
    personOpen = null;
    try {
      const d = await invoke<Detail>("tmdb_detail", { mediaType, id, apiKey });
      selected = d;
    } catch (e) {
      console.warn("[tmdb_detail filmography] error", e);
    }
  }

  async function openPerson(id: number) {
    // Cast anime (AniList): sin ficha TMDb de persona (id 0).
    if (!id) return;
    if (!apiKey) return;
    personLoading = true;
    personOpen = null;
    try {
      personOpen = await invoke<PersonInfo>("tmdb_person", { id, apiKey });
    } catch (e) {
      console.warn("[tmdb_person] error", e);
      personOpen = null;
    } finally {
      personLoading = false;
    }
  }

  function openCarousel(images: string[], idx: number) {
    carousel = { images, idx };
  }
  function closeCarousel() {
    carousel = null;
    volverAlGrid();
  }
  function carouselGo(delta: number) {
    if (!carousel) return;
    const n = carousel.images.length;
    carousel.idx = (carousel.idx + delta + n) % n;
  }

  function closePerson() {
    personOpen = null;
    volverAlGrid();
  }

  function attachCardObserver(node: HTMLButtonElement, id: number) {
    const obs = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            checkItemStatus(id);
            obs.unobserve(node);
          }
        }
      },
      { rootMargin: "200px" }
    );
    obs.observe(node);
    return {
      destroy() { obs.disconnect(); }
    };
  }

  function attachObserver(node: HTMLDivElement) {
    sentinelEl = node;
    // root = .grid-wrap (scroll container real). Sin esto el observer mide
    // contra el viewport y el sentinel — que está dentro de un scroll
    // interno — nunca cuenta como intersectado por más que el user llegue
    // al fondo del grid.
    const root = node.parentElement as HTMLElement | null;
    observer = new IntersectionObserver(
      async (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            await loadMore();
            if (observer && sentinelEl) {
              observer.unobserve(sentinelEl);
              observer.observe(sentinelEl);
            }
          }
        }
      },
      { root, rootMargin: "400px" }
    );
    observer.observe(node);
    return {
      destroy() {
        observer?.disconnect();
        observer = null;
        sentinelEl = null;
      }
    };
  }

  let pickPromise: { id: number; promise: Promise<Detail> } | null = null;

  async function pick(it: ListItem): Promise<Detail | null> {
    if (!apiKey && tab !== "anime") return null;
    if (pickPromise && pickPromise.id === it.id) return pickPromise.promise;
    detailLoading = true;
    const p = (async () => {
      try {
        const d = tab === "anime"
          ? await invoke<Detail>("anilist_detail", { id: it.id, apiKey })
          : await invoke<Detail>("tmdb_detail", { mediaType: tabToMediaType(tab), id: it.id, apiKey });
        selected = d;
        // Nuevo título → resetear vista de capítulos y episodio elegido.
        seriesSeason = null;
        seriesEpisodes = [];
        sourcesSeason = null;
        sourcesEpisode = null;
        return d;
      } catch (e) {
        listError = String(e);
        throw e;
      } finally {
        detailLoading = false;
        if (pickPromise?.id === it.id) pickPromise = null;
      }
    })();
    pickPromise = { id: it.id, promise: p };
    return p;
  }

  async function pickAndDiscover(it: ListItem) {
    try {
      const d = await pick(it);
      if (d?.imdb_id || d?.kitsu_id) goDescubrir();
    } catch { /* error ya seteado en pick */ }
  }

  function pickOrTrailer(it: ListItem, _st: CardStatus | undefined) {
    void pick(it).catch((e) => console.warn("[pick]", e));
  }

  async function pickAndDiscoverOrTrailer(it: ListItem, st: CardStatus | undefined) {
    if (st === "trailer") {
      const d = await pick(it);
      if (d) await watchTrailer();
      return;
    }
    const d = await pick(it);
    if (!d) return;
    // Series con temporadas: drill-in in-place → enfocar la lista de
    // temporadas del panel derecho (el grid mostrará los capítulos).
    if (d.media_type === "tv" && d.seasons && d.seasons.length) {
      setTimeout(() => {
        document
          .querySelector<HTMLElement>('[data-section="info"] .season-item')
          ?.focus();
      }, 60);
      return;
    }
    if (d.imdb_id || d.kitsu_id) goDescubrir();
  }

  // URL de subtítulo precargada por startDiscover antes de cambiar a mode=discover.
  // Se inyecta como sub_url en discoverUrl. Limpio al volver a browse.
  let currentSubUrl = $state<string | null>(null);
  let currentSubLang = $state<string>("");

  // Src del iframe congelado al entrar a discover. Si pusiéramos
  // discoverUrl(...) directo en el atributo `src`, cada saveProgress
  // mutaría progressForSelected → Svelte re-evalúa → src nuevo → iframe
  // RECARGA → player corta y arranca de nuevo en resumeAt actualizado.
  // Solo se setea en startDiscover y se limpia en stopDiscover.
  let discoverSrc = $state<string>("");
  let discoverFrame: HTMLIFrameElement | null = $state(null);

  // ¿El mensaje viene del iframe del reproductor (o de un frame anidado
  // dentro de él)? Sin esto cualquier ventana podía inventar progreso y
  // ensuciar el historial. `parent` se puede leer aunque el frame sea de
  // otro origen.
  function vieneDelReproductor(src: MessageEventSource | null): boolean {
    const propio = discoverFrame?.contentWindow;
    if (!propio || !src) return false;
    let w = src as Window | null;
    try {
      for (let i = 0; i < 6 && w; i++) {
        if (w === propio) return true;
        if (w === window || w.parent === w) return false;
        w = w.parent;
      }
    } catch {
      // MessagePort / ServiceWorker: no son ventanas.
    }
    return false;
  }

  function discoverUrl(
    imdb_id: string,
    kind: "movie" | "tv",
    resumeAt?: number,
    season?: number | null,
    episode?: number | null,
  ) {
    // VidAPI endpoint canónico (docs: vidapi.ru/api). Acepta resumeAt en
    // segundos para seek inicial y emite PLAYER_EVENT postMessage con el
    // progreso real del <video>. Reemplaza el chain playimdb→streamimdb.
    // El path debe coincidir con el tipo: /embed/movie/ para pelis,
    // /embed/tv/ para series — si no, el provider no encuentra el título.
    let url = `https://vaplayer.ru/embed/${kind}/${imdb_id}?autoplay=1`;
    // Series con capítulo elegido: apuntar el embed a esa temporada/episodio.
    // Si el provider ignora los params, cae al selector interno (comportamiento
    // previo) — sin regresión.
    if (kind === "tv" && season != null && episode != null) {
      url += `&season=${season}&episode=${episode}`;
    }
    if (resumeAt && resumeAt > 5) {
      url += `&resumeAt=${Math.floor(resumeAt)}`;
    }
    if (currentSubUrl && currentSubLang) {
      url +=
        `&sub_url=${encodeURIComponent(currentSubUrl)}` +
        `&sub_lang=${encodeURIComponent(currentSubLang)}` +
        `&sub_label=${encodeURIComponent(currentSubLang.toUpperCase())}` +
        `&sub_default=true`;
    } else if (config.subsLang && config.subsLang !== "off") {
      // Sin wyzie disponible: dejar que el player auto-busque vía ds_lang.
      url += `&ds_lang=${encodeURIComponent(config.subsLang)}`;
    }
    return url;
  }

  async function precargarSubtitulo(imdbId: string): Promise<void> {
    currentSubUrl = null;
    currentSubLang = "";
    if (!config.wyzieKey || !config.subsLang || config.subsLang === "off") return;
    try {
      const subs = await invoke<WyzieSubtitle[]>("wyzie_search", {
        imdbId,
        language: config.subsLang,
        apiKey: config.wyzieKey,
      });
      if (subs.length > 0) {
        currentSubUrl = subs[0].url;
        currentSubLang = subs[0].lang || config.subsLang;
        console.log("[wyzie] sub cargado:", subs[0].label, subs[0].url);
      } else {
        console.log("[wyzie] sin resultados para", imdbId, config.subsLang);
      }
    } catch (e) {
      console.warn("[wyzie] búsqueda fall:", e);
    }
  }

  async function startDiscover(force = false) {
    if (!selected) return;
    if (!selected.imdb_id) {
      unavailable = { open: true, reason: "no_imdb", checking: false };
      return;
    }
    // Si está marcada como no disponible y no se forzó → vista directo
    if (!force && unavailableSet.has(selected.imdb_id)) {
      mode = "unavailable";
      setTimeout(() => document.querySelector<HTMLElement>(".unavail-back")?.focus(), 50);
      return;
    }
    // Pre-check rápido: solo bloquea con evidencia clara (404/410 server-side).
    // Si force=true, saltamos también esta verificación (user pidió intentar).
    // Sin pre-check: el screening worker ya marca no disponibles vía la
    // API real del provider (streamdata.vaplayer.ru). Si llegó hasta acá,
    // o la peli es disponible o el user forzó "Intentar de todas formas".
    // Precargar subtítulo desde Wyzie (si hay key) ANTES de mode=discover
    // para que el iframe se renderice con el sub_url ya en src.
    await precargarSubtitulo(selected.imdb_id);
    // Snapshot del src ANTES de cambiar a mode=discover. progressForSelected
    // puede mutar varias veces durante la sesión (cada PLAYER_EVENT), pero
    // discoverSrc queda fijo hasta stopDiscover.
    const playKind = selected.media_type === "tv" ? "tv" : "movie";
    discoverSrc = discoverUrl(
      selected.imdb_id,
      playKind,
      progressForSelected?.watched_seconds,
      sourcesSeason,
      sourcesEpisode,
    );
    discoverStartTs = Date.now();
    mode = "discover";
    setFs(true);
    registerBackShortcuts(false);
    setTimeout(() => document.querySelector<HTMLElement>(".back-btn")?.focus(), 50);
  }

  // Si la lista de fuentes debe auto-reproducir la mejor (⚡ Ver con debrid)
  // o mostrar la lista para elegir (🔄 Rebuscar fuentes).
  let sourcesAutoplay = $state(false);
  // La lista de fuentes abierta para BAJAR, no para ver (ver SourcePicker:
  // misma búsqueda, distinto final).
  let sourcesDescargar = $state(false);
  // Temporada/episodio elegidos (series/anime). null en películas.
  let sourcesSeason = $state<number | null>(null);
  let sourcesEpisode = $state<number | null>(null);
  // Ruta desde la que llegó el handoff (?from=). Cerrar el menú vuelve ahí en
  // vez de dejar al user tirado en la home: si entró desde Vistos, salir tiene
  // que devolverlo a Vistos. Se consume una sola vez.
  let volverDesde = $state("");

  // "Empezar de nuevo": ignora el punto guardado en el próximo lanzamiento.
  // Se apaga solo al abrir el menú de nuevo (ver openDebrid).
  let empezarDeCero = $state(false);
  const reanudarDesde = $derived(
    empezarDeCero || !claveSel
      ? 0
      : segundosParaRetomar(claveSel, sourcesSeason ?? -1, sourcesEpisode ?? -1),
  );

  // --- Navegación in-place de series: temporada elegida en el panel derecho →
  // el grid de carátulas muestra los capítulos de esa temporada. ---
  // null = catálogo normal; nº = mostrando capítulos de esa temporada.
  let seriesSeason = $state<number | null>(null);
  let seriesEpisodes = $state<EpisodeMini[]>([]);
  let episodesLoading = $state(false);
  let episodesError = $state("");
  // Cache por `${tmdbId}:${season}` para no rebajar episodios al volver.
  const seasonEpCache = new Map<string, EpisodeMini[]>();

  // Abre una temporada: carga capítulos (cacheado) y el grid los muestra.
  async function openSeason(seasonNumber: number) {
    if (!selected) return;
    seriesSeason = seasonNumber;
    episodesError = "";
    const key = `${selected.id}:${seasonNumber}`;
    if (seasonEpCache.has(key)) {
      seriesEpisodes = seasonEpCache.get(key)!;
      return;
    }
    episodesLoading = true;
    seriesEpisodes = [];
    try {
      const eps = selected.is_anime
        ? await invoke<EpisodeMini[]>("anizip_episodes", { anilistId: selected.id })
        : await invoke<EpisodeMini[]>("tmdb_season", {
            id: selected.id,
            seasonNumber,
            apiKey,
          });
      seasonEpCache.set(key, eps);
      seriesEpisodes = eps;
    } catch (e) {
      episodesError = String(e);
      seriesEpisodes = [];
    } finally {
      episodesLoading = false;
    }
  }

  // Cambia de temporada sin salir del listado de capítulos ([ ] o RePág/AvPág
  // en teclado). Con el grid abierto la lista de temporadas
  // queda al fondo del panel de info, casi siempre fuera de pantalla, así que
  // este atajo es la vía corta.
  function cambiarTemporada(d: number) {
    const ss = selected?.seasons;
    if (!ss || ss.length < 2 || seriesSeason == null) return;
    const i = ss.findIndex((x) => x.season_number === seriesSeason);
    if (i < 0) return;
    const prox = ss[(i + d + ss.length) % ss.length];
    void openSeason(prox.season_number);
    // El foco vive en el grid: dejarlo en el primer capítulo de la temporada nueva.
    setTimeout(() => {
      document
        .querySelector<HTMLElement>('[data-section="gallery"] [data-nav]:not([disabled])')
        ?.focus();
    }, 60);
  }

  // Sin capítulos abiertos, [ ] rotan la pestaña del catálogo, igual que en
  // /historial.
  function ciclarTab(d: number) {
    const orden: Tab[] = ["movie", "tv", "anime"];
    const i = orden.indexOf(tab);
    switchTab(orden[(i + d + orden.length) % orden.length]);
  }

  // Vuelve del listado de capítulos al catálogo normal.
  function closeSeasonView() {
    seriesSeason = null;
    seriesEpisodes = [];
    episodesError = "";
    sourcesSeason = null;
    sourcesEpisode = null;
    setTimeout(() => {
      document
        .querySelector<HTMLElement>('[data-section="info"] .season-item')
        ?.focus();
    }, 30);
  }

  // Elige un capítulo → abre el menú de reproducción apuntando a ese episodio.
  function chooseEpisode(ep: EpisodeMini) {
    if (seriesSeason == null) return;
    sourcesSeason = seriesSeason;
    sourcesEpisode = ep.episode_number;
    setContexto(metaDe(seriesSeason, ep.episode_number, ep));
    goDescubrir();
  }

  // Tipo para el scraper: movie / anime (tab) / series.
  function currentKind(): "movie" | "series" | "anime" {
    if (selected?.media_type === "movie") return "movie";
    return tab === "anime" ? "anime" : "series";
  }

  // Etiqueta de progreso para el menú ("45%" / "12m 3s" / null si no hay).
  function progressLabelFor(): string | null {
    const p = progressForSelected;
    if (!p || p.completed === 1 || p.watched_seconds <= 5) return null;
    if (p.runtime_seconds && p.runtime_seconds > 0) {
      return Math.round((p.watched_seconds / p.runtime_seconds) * 100) + "%";
    }
    const m = Math.floor(p.watched_seconds / 60);
    const s = p.watched_seconds % 60;
    return `${m}m ${s}s`;
  }

  // "Descubrir" abre el menú contextual de reproducción.
  function goDescubrir() {
    // Anime puede no tener imdb pero sí kitsu (ruta debrid vía Torrentio).
    if (!selected?.imdb_id && !selected?.kitsu_id) {
      void startDiscover();
      return;
    }
    mode = "playmenu";
    setFs(true);
    void prefetchMenuExtras();
    // Atajos OS-level para asegurar que Esc/Backspace cierren incluso si el
    // foco quedó atrapado en un iframe/embed.
    void registerBackShortcuts(false);
  }

  // ¿El título seleccionado se puede reproducir? Mismo criterio que usa el
  // resto de la app para abrir el menú (imdb en TMDb, kitsu en anime de
  // AniList). El panel de info miraba SOLO imdb_id, así que todo el anime salía
  // sellado "NO DISPONIBLE" aunque sus capítulos abren sin problema.
  const disponibleSel = $derived(
    !!selected &&
      (selected.imdb_id
        ? !unavailableSet.has(selected.imdb_id)
        : !!selected.kitsu_id),
  );


  // Bajamos OMDb (premios/ratings/plot largo) + trailer en paralelo cuando se
  // abre el PlayMenu. No bloquea: la UI ya mostró todo lo que tiene de TMDb.
  async function prefetchMenuExtras() {
    // Anime: el trailer ya viene de AniList (YouTube). OMDb/Apple/TMDb no
    // aplican — el id es de AniList, no de TMDb.
    if (selected?.is_anime) {
      menuOmdb = null;
      menuTrailerPick = EMPTY_PICK;
      const key = selected.trailer_youtube || "";
      const animeId = selected.id;
      const pick = await resolveAnimeTrailer(key);
      if (selected?.id === animeId) menuTrailerPick = pick;
      return;
    }
    if (!selected?.imdb_id) {
      menuOmdb = null;
      menuTrailerPick = EMPTY_PICK;
      return;
    }
    const imdb = selected.imdb_id;
    const detail = selected;
    const omdbKey = (config.omdbKey || "").trim();

    // Servir desde cache si existe.
    menuOmdb = omdbCache.get(imdb) ?? null;
    const tCache = trailerCache.get(imdb);
    menuTrailerPick = tCache ?? EMPTY_PICK;

    const tasks: Promise<unknown>[] = [];

    if (omdbKey && !omdbCache.has(imdb)) {
      tasks.push(
        invoke<OmdbDetail>("omdb_detail", { imdbId: imdb, apiKey: omdbKey })
          .then((d) => {
            omdbCache.set(imdb, d);
            if (selected?.imdb_id === imdb) menuOmdb = d;
          })
          .catch((e) => console.warn("[omdb_detail]", e)),
      );
    }

    if (!trailerCache.has(imdb)) {
      tasks.push(
        (async () => {
          const pick = await resolveTrailer(detail, apiKey);
          trailerCache.set(imdb, pick);
          if (selected?.imdb_id === imdb) menuTrailerPick = pick;
        })(),
      );
    }

    await Promise.all(tasks);
  }

  // Acción "Trailer" desde PlayMenu: reusa los datos ya bajados.
  async function menuTrailer() {
    if (!selected) return;
    const pick = menuTrailerPick;
    // Nada prefetcheado todavía → flujo completo (busca y avisa).
    if (!pick.ytKey && !pick.apple) {
      void watchTrailer();
      return;
    }
    await playPick(pick, selected.title);
  }

  // Las URLs directas de YouTube caducan: pasado este rato se vuelven a pedir.
  const TRAILER_URL_TTL_MS = 20 * 60 * 1000;

  // Reproduce lo mejor disponible del pick; QR si no hay forma de verlo dentro.
  async function playPick(pick: TrailerPick, title: string): Promise<void> {
    if (pick.playable && pick.ytKey) {
      let { video, audio } = pick;
      // Prefetch viejo: las URLs ya no sirven, se resuelve de nuevo (una sola
      // llamada, igual que la primera vez).
      if (video && Date.now() - pick.at > TRAILER_URL_TTL_MS) {
        const fresh = await ytTrailerSrc(pick.ytKey);
        video = fresh.ok ? fresh.video : "";
        audio = fresh.ok ? fresh.audio : "";
      }
      // Sin URL directa la reproduce ytdl_hook desde la URL de YouTube.
      const url = video || `https://www.youtube.com/watch?v=${pick.ytKey}`;
      if (await playTrailerInMpv(url, title, audio)) return;
    }
    if (pick.apple) {
      if (await playTrailerInMpv(pick.apple, title)) return;
    }
    if (pick.ytKey) {
      if (pick.err) console.warn("[trailer] sin reproducción local:", pick.err);
      await showTrailerQr(pick.ytKey);
      return;
    }
    trailerMsg = `Sin trailer disponible para "${title}".`;
    setTimeout(() => (trailerMsg = ""), 4000);
  }

  // --- Acciones del menú ---
  function menuContinue() { void startDiscover(); }      // web con resume
  // "Empezar de nuevo". No se borra el historial: solo se lanza desde 0.
  // Borrarlo perdería el dato de que ya la habías visto a medias. La ruta de
  // fuentes vale con y sin debrid, así que solo el player web (apagado) usa
  // el camino viejo.
  function menuRestart() {
    if (WEB_PLAYER_ENABLED && !config.rdLinked) {
      void restartDiscover();
      return;
    }
    openDebrid(true, true);
  }
  // Abre la ruta debrid. Películas → lista directa. Series/anime → elegir
  // temporada/episodio primero.
  function openDebrid(autoplay: boolean, desdeCero = false) {
    sourcesDescargar = false;
    sourcesAutoplay = autoplay;
    empezarDeCero = desdeCero;
    if (selected?.media_type === "movie") {
      sourcesSeason = null;
      sourcesEpisode = null;
      // El tracker escribe contra este contexto: sin él mpv reproduce pero
      // nada queda registrado (ver HistorialTracker.svelte).
      setContexto(metaDe());
      mode = "sources";
    } else if (sourcesSeason != null && sourcesEpisode != null) {
      // Capítulo ya elegido inline → directo a fuentes, sin EpisodePicker.
      mode = "sources";
    } else {
      mode = "episodes";
    }
  }
  function menuRealDebrid() { openDebrid(true); }  // auto mejor
  function menuResearch() { openDebrid(false); }   // elegir
  // "Bajar para después": la misma lista de fuentes, pero al elegir encola en
  // vez de reproducir. Con sourceSelect en automático ni se muestra: agarra la
  // mejor que quepa en el tope y vuelve.
  async function menuBajar() {
    if (!selected || !claveSel) return;
    // En automático no hay nada que elegir: va derecho a la cola, que la baja
    // cuando haya lugar. Así pedir cinco películas seguidas no arranca cinco
    // descargas peleándose la conexión.
    if (config.sourceSelect === "auto") {
      const n = await encolar([
        {
          clave: claveSel,
          title: selected.title,
          kind: currentKind(),
          imdbId: selected.imdb_id ?? "",
          originalTitle: selected.original_title ?? null,
          kitsuId: selected.kitsu_id ?? null,
        },
      ]);
      notify(
        "info",
        n ? "En la cola" : enColaSel ? "Ya estaba en la cola" : "Ya la tienes",
        n ? `${selected.title} — te avisamos cuando esté lista.` : selected.title,
      );
      closeSources();
      return;
    }
    // En manual el usuario quiere elegir el release: para eso está la lista.
    sourcesDescargar = true;
    sourcesSeason = null;
    sourcesEpisode = null;
    setContexto(metaDe());
    mode = "sources";
  }

  // "Cambiar fuente" desde el bar del reproductor. Con la lista montada la
  // maneja el propio SourcePicker (vuelve a ella marcando la ya probada); acá
  // solo se cubre el caso de haber llegado al video por otro camino (la pill
  // de "seguir viendo", por ejemplo), donde hay que abrir la lista de cero.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen("player:cambiar-fuente", () => {
      if (mode === "sources" || !selected) return;
      openDebrid(false);
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  // ---- Fin del video: pantalla de "qué ver después" ----------------------
  // El backend avisa con "mpv:fin" cuando el archivo se acabó solo (distinto de
  // salir con Esc). Lo que NUNCA puede seguir es otra fuente de la lista: todas
  // son el mismo archivo, así que la película se repetiría.
  //
  // En vez del corte seco al catálogo se abre una pantalla de fin: capítulo
  // siguiente (con Enter, nada arranca solo) y recomendaciones del título.
  let finTitulo = $state("");
  let finBackdrop = $state<string | null>(null);
  let finSiguiente = $state<SiguienteFin | null>(null);
  let finSugerencias = $state<SugerenciaFin[]>([]);
  let finCargando = $state(false);
  // Ítem completo de cada sugerencia: `pick()` lo necesita para abrir la ficha.
  const finItems = new Map<number, ListItem>();
  // Episodio al que apunta finSiguiente (para el contexto del historial).
  let finSiguienteEp: EpisodeMini | null = null;

  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen<FinPayload>("mpv:fin", (e) => void alTerminar(e.payload)).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  async function alTerminar(payload: FinPayload) {
    // Corte del stream a mitad: no terminó nada. Lo resuelve el SourcePicker
    // (vuelve a la lista para elegir otra fuente).
    if (!esFinReal(payload)) return;
    // Lo que terminó tiene que ser el título abierto acá. Si se lanzó desde
    // otro lado (la pill de "seguir viendo", otra ruta), `selected` puede ser
    // cualquier otra cosa y recomendaríamos "porque viste" un título que nadie
    // vio.
    if (!selected || contextoActual()?.clave !== claveSel) {
      closeSources();
      return;
    }
    finTitulo = selected.title;
    finBackdrop = selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null;
    finSiguiente = null;
    finSiguienteEp = null;
    finSugerencias = [];
    finItems.clear();
    finCargando = true;
    mode = "fin";
    setFs(true);
    void registerBackShortcuts(false);

    const esEpisodio =
      currentKind() !== "movie" && sourcesSeason != null && sourcesEpisode != null;
    if (esEpisodio) {
      const sig = await siguienteEpisodio(sourcesSeason!, sourcesEpisode!);
      if (sig) {
        finSiguienteEp = sig.ep ?? null;
        finSiguiente = {
          season: sig.season,
          episode: sig.episode,
          nombre: sig.ep?.name ?? "",
          stillUrl: sig.ep?.still_path ? art(sig.ep.still_path, "w300", 300) : null,
        };
      }
    }
    finSugerencias = await sugerenciasDe();
    finCargando = false;
    // Nada que ofrecer (sin capítulo siguiente y sin red): no dejamos al user
    // mirando una pantalla vacía.
    if (!finSiguiente && !finSugerencias.length) closeSources();
  }

  // Enter sobre "Siguiente capítulo": mismo camino que elegirlo a mano.
  function verSiguienteCapitulo() {
    const sig = finSiguiente;
    if (!sig) return;
    sourcesSeason = sig.season;
    sourcesEpisode = sig.episode;
    // El capítulo nuevo arranca desde el principio aunque el título tuviera
    // progreso guardado de otra vez.
    empezarDeCero = true;
    setContexto(
      metaDe(
        sig.season,
        sig.episode,
        finSiguienteEp ?? ({ episode_number: sig.episode, name: sig.nombre, still_path: null } as EpisodeMini),
      ),
    );
    sourcesAutoplay = true;
    mode = "sources"; // la key del SourcePicker cambia → busca fuentes del nuevo
  }

  // Enter sobre una sugerencia: abre su ficha de reproducción, igual que
  // elegirla en el catálogo.
  function verSugerencia(id: number) {
    const it = finItems.get(id);
    if (!it) return;
    void pickAndDiscover(it);
  }

  // "Porque viste X": recomendaciones del título que acaba de terminar.
  async function sugerenciasDe(): Promise<SugerenciaFin[]> {
    if (!selected) return [];
    const r = await sugerenciasPara({
      id: selected.id,
      esAnime: tab === "anime",
      mediaType: tabToMediaType(tab),
      apiKey,
      noDisponibles: unavailableSet,
    });
    for (const [id, it] of r.items) finItems.set(id, it);
    return r.sugerencias;
  }

  // Capítulo siguiente: el que sigue dentro de la temporada y, si se acabó, el
  // primero de la siguiente. Los "Especiales" (temporada 0) no entran en la
  // cadena: no son la continuación de nada.
  async function siguienteEpisodio(
    season: number,
    episode: number,
  ): Promise<{ season: number; episode: number; ep?: EpisodeMini } | null> {
    if (!selected) return null;
    const enTemporada = await cargarEpisodios(season);
    const prox = enTemporada
      .filter((e) => e.episode_number > episode)
      .sort((a, b) => a.episode_number - b.episode_number)[0];
    if (prox) return { season, episode: prox.episode_number, ep: prox };
    // El anime de AniList trae TODOS los capítulos en una lista: si ahí no hay
    // siguiente, no hay temporada que buscar.
    if (selected.is_anime) return null;
    const siguientes = (selected.seasons ?? [])
      .filter((x) => x.season_number > season && x.season_number > 0 && x.episode_count > 0)
      .sort((a, b) => a.season_number - b.season_number);
    for (const t of siguientes) {
      const eps = await cargarEpisodios(t.season_number);
      const primero = eps.sort((a, b) => a.episode_number - b.episode_number)[0];
      if (primero) {
        return { season: t.season_number, episode: primero.episode_number, ep: primero };
      }
      // Sin datos de episodios pero la temporada existe: al capítulo 1 igual.
      if (t.episode_count > 0) return { season: t.season_number, episode: 1 };
    }
    return null;
  }

  // Episodios de una temporada, con el mismo cache que usa el grid.
  async function cargarEpisodios(seasonNumber: number): Promise<EpisodeMini[]> {
    if (!selected) return [];
    const key = `${selected.id}:${seasonNumber}`;
    const hit = seasonEpCache.get(key);
    if (hit) return hit;
    try {
      const eps = selected.is_anime
        ? await invoke<EpisodeMini[]>("anizip_episodes", { anilistId: selected.id })
        : await invoke<EpisodeMini[]>("tmdb_season", {
            id: selected.id,
            seasonNumber,
            apiKey,
          });
      seasonEpCache.set(key, eps);
      return eps;
    } catch {
      return [];
    }
  }

  // Episodio elegido en EpisodePicker → a la lista de fuentes.
  function onPickEpisode(
    season: number,
    episode: number,
    ep?: { name: string; still_path: string | null },
  ) {
    sourcesSeason = season;
    sourcesEpisode = episode;
    setContexto(
      metaDe(season, episode, ep ? ({ ...ep, episode_number: episode } as EpisodeMini) : null),
    );
    mode = "sources";
  }
  function menuWeb() { void startDiscover(true); }       // iframe playimdb

  // Cierra menú/lista y vuelve al catálogo.
  function closeSources() {
    mode = "browse";
    sourcesDescargar = false;
    setFs(false);
    void unregisterBackShortcuts();
    if (volverDesde) {
      const destino = volverDesde;
      volverDesde = "";
      void goto(destino);
      return;
    }
    setTimeout(() => {
      const info = document.querySelector<HTMLElement>('[data-section="info"] [data-nav]');
      // Sin panel de info (o vacío) el foco caería en body y el focus-guard
      // rebobinaría el grid: mejor volver directo a la card.
      if (info) info.focus(); else volverAlGrid();
    }, 50);
  }

  // "Marcar disponible" desde la pantalla de no disponible.
  async function marcarDisponible() {
    if (!selected?.imdb_id) return;
    await clearUnavailable(selected.imdb_id);
    mode = "browse";
    setTimeout(volverAlGrid, 50);
  }

  async function reportUnavailableFromDiscover() {
    if (!selected?.imdb_id) return;
    try {
      await markUnavailable(selected.imdb_id);
    } catch (e) {
      console.warn("[markUnavailable]", e);
    }
    // UI se actualiza igual: si la marca falló, el user puede reintentar,
    // pero la pantalla siempre debe pasar a "unavailable" cuando él la apretó.
    mode = "unavailable";
    setFs(false);
    unregisterBackShortcuts();
    setTimeout(() => document.querySelector<HTMLElement>(".unavail-back")?.focus(), 50);
  }

  async function restartDiscover() {
    // "Desde el inicio": resetea progreso wall-clock y abre. El iframe arranca
    // sin resumeAt → player comienza en 0.
    if (!claveSel) return;
    try {
      await clearProgressFor(claveSel);
    } catch (e) {
      console.warn("[clearProgressFor]", e);
    }
    void startDiscover();
  }

  async function stopDiscover() {
    const wasDiscover = mode === "discover";
    if (mode === "unavailable") {
      mode = "browse";
      setTimeout(volverAlGrid, 50);
      return;
    }
    if (mode !== "discover" && mode !== "trailer") return;
    // Acumular tiempo wall-clock si veníamos del modo discover
    if (wasDiscover && selected?.imdb_id && discoverStartTs > 0) {
      const elapsed = Math.floor((Date.now() - discoverStartTs) / 1000);
      discoverStartTs = 0;
      if (elapsed > 5) {
        const prev = progressForSelected?.watched_seconds || 0;
        await saveProgress({
          watched_seconds: prev + elapsed,
          runtime_seconds: progressForSelected?.runtime_seconds ?? null,
          progress_real: progressForSelected?.progress_real ?? null,
        });
      }
    }
    mode = "browse";
    trailerQr = "";
    trailerQrUrl = "";
    discoverSrc = "";
    setFs(false);
    unregisterBackShortcuts();
    // El grid recién existe otra vez tras este cambio de modo: esperamos al
    // render antes de devolver el foco.
    setTimeout(volverAlGrid, 50);
  }

  let trailerMsg = $state("");

  async function watchTrailer() {
    if (!selected) return;
    unavailable.open = false;
    trailerMsg = "Buscando trailer…";
    trailerQr = "";
    trailerQrUrl = "";

    const title = selected.title;
    // Anime: la key viene de AniList; TMDb/Apple no aplican (el id no es TMDb).
    const pick = selected.is_anime
      ? await resolveAnimeTrailer(selected.trailer_youtube || "")
      : await resolveTrailer(selected, apiKey);
    trailerMsg = "";
    console.log("[trailer]", title, "→", pick);

    await playPick(pick, title);
  }

  function getNavRoot(): ParentNode {
    if (personOpen || personLoading) return document.querySelector(".person-modal") || document;
    if (unavailable.open) return document.querySelector(".modal") || document;
    return document;
  }

  function spatialNav(dir: "up" | "down" | "left" | "right") {
    const all = Array.from(
      getNavRoot().querySelectorAll<HTMLElement>('[data-nav]:not([disabled])')
    ).filter(el => el.offsetParent !== null);
    const cur = document.activeElement as HTMLElement | null;
    if (!cur || !cur.matches?.("[data-nav]")) {
      all[0]?.focus();
      return;
    }
    const curSection = seccionDe(cur);
    // Pass 1: candidatos en la misma sección (cautivos)
    let best: HTMLElement | null = null;
    if (curSection) {
      const sameSec = all.filter((el) => seccionDe(el) === curSection);
      best = mejor(cur, sameSec, dir);
    }
    // Borde izquierdo del grid de capítulos: salir vuelve SIEMPRE a la
    // temporada abierta, no a lo que quede más cerca (que era el botón de
    // Trailer, lo único que hay arriba del listado en una serie). Va después
    // del pass 1 para no romper el ← que recorre los capítulos.
    if (!best && seriesSeason != null && curSection === "gallery" && dir === "left") {
      const activa =
        document.querySelector<HTMLElement>('[data-section="info"] .season-item.active') ??
        document.querySelector<HTMLElement>('[data-section="info"] .season-item');
      if (activa) {
        activa.focus();
        activa.scrollIntoView({ block: "center", behavior: "smooth" });
        return;
      }
    }
    // Pass 2: cross-section solo si la sección lo permite en esa dirección
    if (!best) {
      // info SOLO puede salir lateralmente; vertical queda cautivo
      const allowCross =
        curSection !== "info" || dir === "right" || dir === "left";
      // gallery + ArrowDown: NUNCA saltar a info. Si quedan más pelis
      // por cargar, dispara loadMore y deja el foco donde está; el siguiente
      // ArrowDown ya encontrará candidatos válidos en la misma sección.
      const galleryDownTrapped =
        curSection === "gallery" && dir === "down";
      if (galleryDownTrapped) {
        if (hasMore) {
          startSparkles();
          if (!loadingMore) void loadMore();
        }
      } else if (allowCross) {
        best = mejor(cur, all, dir);
      }
    }
    if (best) {
      best.focus({ preventScroll: false });
      best.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
      const ci = cardEls.indexOf(best as HTMLButtonElement);
      if (ci >= 0) focusedIdx = ci;
    }
  }

  function onGlobalKey(e: KeyboardEvent) {
    if (mode !== "browse") return;
    const t = e.target as HTMLElement | null;
    const inInput = t?.tagName === "INPUT" || t?.tagName === "TEXTAREA";

    if (carousel) {
      if (e.key === "Escape" || e.key === "Backspace" || e.key === "Enter") {
        e.preventDefault(); closeCarousel(); return;
      }
      if (e.key === "ArrowRight") { e.preventDefault(); carouselGo(1); return; }
      if (e.key === "ArrowLeft")  { e.preventDefault(); carouselGo(-1); return; }
      return;
    }
    if (personOpen && (e.key === "Escape" || e.key === "Backspace")) {
      e.preventDefault();
      closePerson();
      return;
    }
    if (unavailable.open && (e.key === "Escape" || e.key === "Backspace")) {
      e.preventDefault();
      unavailable.open = false;
      return;
    }
    if (sortOpen && e.key === "Escape") { sortOpen = false; e.preventDefault(); return; }

    // Vista de capítulos abierta: Esc/Backspace vuelve al catálogo.
    if (seriesSeason != null && !inInput && (e.key === "Escape" || e.key === "Backspace")) {
      e.preventDefault();
      closeSeasonView();
      return;
    }

    if (inInput) {
      // Sliders (brillo, volumen de la barra): las cuatro flechas son del
      // control, no de la navegación. Robárselas dejaba el popover abierto y
      // sin forma de moverlo.
      if ((t as HTMLInputElement | null)?.type === "range") return;
      // Permitir edición; ↑↓ saltan fuera del input
      if (e.key === "ArrowUp")   { e.preventDefault(); spatialNav("up"); }
      if (e.key === "ArrowDown") { e.preventDefault(); spatialNav("down"); }
      return;
    }

    if (e.key.toLowerCase() === "i" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      toggleInfoFocus();
      return;
    }
    switch (e.key) {
      case "ArrowRight": e.preventDefault(); spatialNav("right"); break;
      case "ArrowLeft":  e.preventDefault(); spatialNav("left"); break;
      case "ArrowDown":  e.preventDefault(); spatialNav("down"); break;
      case "ArrowUp":    e.preventDefault(); spatialNav("up"); break;
      case "Home":
        if (items.length) { e.preventDefault(); cardEls[0]?.focus(); }
        break;
      case "End":
        if (items.length) { e.preventDefault(); cardEls[items.length - 1]?.focus(); }
        break;
      case "Enter":
      case " ": {
        const ci = cardEls.findIndex((el) => el === t);
        if (ci >= 0) {
          e.preventDefault();
          focusedIdx = ci;
          const it = items[ci];
          const st = statusMap.get(it.id);
          void pickAndDiscoverOrTrailer(it, st).catch((err) =>
            console.warn("[pickAndDiscoverOrTrailer]", err),
          );
        }
        // En otros botones, Enter/Space disparan onclick por default
        break;
      }
      case "Backspace":
        if (sortOpen) { e.preventDefault(); sortOpen = false; }
        break;
      case "[":
      case "PageUp":
        e.preventDefault();
        if (seriesSeason != null) cambiarTemporada(-1); else ciclarTab(-1);
        break;
      case "]":
      case "PageDown":
        e.preventDefault();
        if (seriesSeason != null) cambiarTemporada(1); else ciclarTab(1);
        break;
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    // Si la ayuda global está abierta, no procesar otras teclas.
    if (ayuda.visible) return;
    if (mode === "discover" || mode === "unavailable") {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        stopDiscover();
        return;
      }
      return;
    }
    if (mode === "trailer") {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        stopDiscover();
        return;
      }
      if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        moverFocoTrailer(1);
        return;
      }
      if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        moverFocoTrailer(-1);
        return;
      }
      return;
    }
    // La pantalla de fin maneja TODO su teclado (Esc incluido): si acá también
    // cerráramos, un Esc dispararía dos cierres.
    if (mode === "fin") return;
    if (mode === "playmenu" || mode === "episodes" || mode === "sources") {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        closeSources();
        return;
      }
      // Dejar que el componente maneje el resto (flechas, Enter).
      return;
    }
    onGlobalKey(e);
  }}
  onclick={(e) => {
    if (!sortOpen) return;
    const t = e.target as HTMLElement | null;
    if (t && !t.closest(".dropdown")) sortOpen = false;
  }}
/>

{#if mode === "unavailable" && selected}
  <PantallaNoDisponible
    titulo={selected}
    onVolver={stopDiscover}
    onTrailer={watchTrailer}
    onMarcarDisponible={marcarDisponible}
  />
{:else if mode === "playmenu" && (selected?.imdb_id || selected?.kitsu_id)}
  <PlayMenu
    detail={selected}
    omdb={menuOmdb}
    progressLabel={progressLabelFor()}
    posterUrl={selected.poster_path ? art(selected.poster_path, "w342", 342) : null}
    backdropUrl={selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null}
    hasTrailer={!!(menuTrailerPick.ytKey || menuTrailerPick.apple)}
    hasRd={config.rdLinked}
    hasLocal={localSel && selected.media_type !== "tv"}
    bajando={bajandoSel || enColaSel}
    puedeBajar={config.torrentLocal && selected.media_type === "movie"}
    onContinue={menuContinue}
    onRestart={menuRestart}
    onRealDebrid={menuRealDebrid}
    onResearch={menuResearch}
    onTrailer={menuTrailer}
    onDownload={() => void menuBajar()}
    onClose={closeSources}
  />
{:else if mode === "episodes" && (selected?.imdb_id || selected?.kitsu_id)}
  <EpisodePicker
    seriesId={selected.id}
    title={selected.title}
    backdrop={selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null}
    poster={selected.poster_path ? art(selected.poster_path, "w300", 300) : null}
    stillBase={`${IMG}/w300`}
    seasons={selected.seasons ?? []}
    apiKey={apiKey}
    animeId={selected.is_anime ? selected.id : null}
    clave={claveSel}
    imdbId={selected.imdb_id ?? ""}
    kind={currentKind()}
    originalTitle={selected.original_title ?? null}
    kitsuId={selected.kitsu_id ?? null}
    onPick={onPickEpisode}
    onWeb={menuWeb}
    onClose={closeSources}
  />
{:else if mode === "fin"}
  <PostCreditos
    title={finTitulo}
    backdrop={finBackdrop}
    siguiente={finSiguiente}
    sugerencias={finSugerencias}
    cargando={finCargando}
    onSiguiente={verSiguienteCapitulo}
    onElegir={verSugerencia}
    onClose={closeSources}
  />
{:else if mode === "sources" && (selected?.imdb_id || selected?.kitsu_id)}
  <!-- key: al saltar al capítulo siguiente hay que rehacer la búsqueda de
       fuentes desde cero; sin remontar, el picker seguiría con las del anterior. -->
  {#key `${selected.id}:${sourcesSeason}:${sourcesEpisode}`}
  <SourcePicker
    imdbId={selected.imdb_id ?? ""}
    kind={currentKind()}
    clave={claveSel}
    season={sourcesSeason}
    episode={sourcesEpisode}
    title={selected.title}
    originalTitle={selected.original_title ?? null}
    backdrop={selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null}
    kitsuId={selected.kitsu_id ?? null}
    rdLinked={config.rdLinked}
    autoplay={sourcesAutoplay}
    descargarSolo={sourcesDescargar}
    retomarSegundos={reanudarDesde}
    onClose={closeSources}
    onWeb={menuWeb}
  />
  {/key}
{:else if mode === "discover" && selected?.imdb_id}
  <Descubrir
    src={discoverSrc}
    bind:frame={discoverFrame}
    onVolver={stopDiscover}
    onReportar={reportUnavailableFromDiscover}
  />
{:else if mode === "trailer" && trailerQr}
  <TrailerQr qr={trailerQr} url={trailerQrUrl} onVolver={stopDiscover} onAbrir={abrirTrailerWeb} />
{:else}
  <main>
    <aside class="info" data-section="info">
      {#if !apiKey || showKey}
        <ClaveTmdb bind:valor={keyInput} urlTeclado={webKeyboardUrl} onGuardar={saveKey} />
      {:else if detailLoading}
        <div class="empty">Cargando…</div>
      {:else if selected}
        <FichaTitulo
          titulo={selected}
          clave={claveSel}
          favorito={favSel}
          enCurso={enCursoSel}
          estado={estadoSel}
          progreso={progressForSelected}
          disponible={disponibleSel}
          local={localSel}
          bajando={bajandoSel}
          enCola={enColaSel}
          temporadaAbierta={seriesSeason}
          onFavorito={() => void alternarFavorito(metaDe()!)}
          onVisto={(v) => void marcarVisto(metaDe()!, v)}
          onDescubrir={goDescubrir}
          onTrailer={watchTrailer}
          onTemporada={openSeason}
          onPersona={openPerson}
          onEscenas={openCarousel}
        />
      {:else}
        <div class="brand-empty">
          <h1 class="brand-name">Kütral</h1>
          <p class="brand-tag">elige un título</p>
        </div>
      {/if}
      <div class="key-edit">
        <button class="link" onclick={() => (showKey = !showKey)}>
          {showKey ? "Cerrar" : "Cambiar API key"}
        </button>
      </div>
    </aside>

    <section class="gallery">
      {#if seriesSeason != null}
        <header class="ep-head-bar" data-section="filters">
          <button data-nav class="ep-back-inline" onclick={closeSeasonView}>← Volver</button>
          <span class="ep-crumb">
            {selected?.title} · {seasonLabel(seriesSeason)}
          </span>
        </header>
        {#if episodesError}<p class="err">{episodesError}</p>{/if}
        {#if episodesLoading}
          <div class="empty">Cargando capítulos…</div>
        {:else}
          <div class="grid-wrap">
            <div class="grid" data-section="gallery">
              {#each seriesEpisodes as ep (ep.episode_number)}
                {@const fila = seriesSeason != null ? estadoEpisodio(claveSel, seriesSeason, ep.episode_number) : null}
                {@const epPct = fila
                  ? Math.round(
                      (fila.progress_real ??
                        (fila.runtime_seconds ? fila.watched_seconds / fila.runtime_seconds : 0)) * 100,
                    )
                  : 0}
                <button
                  data-nav
                  class="card ep-card"
                  class:ep-visto={fila?.completed === 1}
                  onclick={() => chooseEpisode(ep)}
                  title={ep.name}
                >
                  {#if ep.still_path}
                    <img src={art(ep.still_path, "w300", 300)} alt={ep.name} loading="lazy" onerror={onImgError} />
                  {:else if selected?.poster_path}
                    <!-- Sin captura del capítulo: la portada de la serie antes
                         que un cuadro gris con el número. -->
                    <img
                      class="ep-fallback"
                      src={art(selected.poster_path, "w300", 300)}
                      alt={ep.name}
                      loading="lazy"
                      onerror={onImgError}
                    />
                    <span class="ep-num">E{ep.episode_number}</span>
                  {:else}
                    <div class="no-poster ep-noimg">E{ep.episode_number}</div>
                  {/if}
                  {#if fila?.completed === 1}
                    <span class="card-tick" title="Ya lo viste">✓</span>
                  {:else if fila && epPct > 0}
                    <span class="card-barra"><span class="card-barra-fill" style:width="{Math.max(3, epPct)}%"></span></span>
                  {/if}
                </button>
              {/each}
              {#if !seriesEpisodes.length}
                <div class="empty">Sin capítulos</div>
              {/if}
            </div>
          </div>
        {/if}
      {:else}
      <header data-section="filters">
        {#if webRemoteUrl}
          <div class="qr-col">
            <RemoteQr url={webRemoteUrl} size={70} />
          </div>
        {/if}
        <div class="filters-col">
        <div class="row1">
          <div class="search-wrap">
            <svg class="search-icon" viewBox="0 0 16 16" width="14" height="14">
              <circle cx="7" cy="7" r="5" fill="none" stroke="currentColor" stroke-width="1.6"/>
              <path d="M11 11l3.5 3.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
            </svg>
            <input
              data-nav
              class="search"
              bind:value={query}
              oninput={onSearchInput}
              placeholder="Buscar…"
              disabled={!apiKey}
            />
            {#if query}
              <button class="search-clear" onclick={() => { query = ""; debouncedQ = ""; resetAndLoad(); }} title="Limpiar">✕</button>
            {/if}
          </div>
          <div class="dropdown">
            <button
              data-nav
              class="dropdown-trigger"
              onclick={() => (sortOpen = !sortOpen)}
              disabled={!apiKey || !!debouncedQ}
              title={debouncedQ ? "Orden no disponible durante búsqueda" : "Ordenar por…"}
            >
              <span>{sortOptions.find(s => s.id === sortId)?.label || "Orden"}</span>
              <svg class="chev" viewBox="0 0 10 6" width="10" height="6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
            </button>
            {#if sortOpen}
              <ul class="dropdown-menu" role="menu" use:autofocusFirst>
                {#each sortOptions as s}
                  <li>
                    <button
                      data-nav
                      class:active={s.id === sortId}
                      onclick={() => { changeSort(s.id); sortOpen = false; }}
                    >
                      {s.label}
                      {#if s.id === sortId}<span class="dot">●</span>{/if}
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
          {#if tab === "anime"}
            <div class="dropdown">
              <button
                data-nav
                class="dropdown-trigger"
                onclick={() => (seasonOpen = !seasonOpen)}
                disabled={!!debouncedQ}
                title={debouncedQ ? "Temporada no disponible durante búsqueda" : "Filtrar por temporada…"}
              >
                <span>{seasonSel?.label || "Temporada"}</span>
                <svg class="chev" viewBox="0 0 10 6" width="10" height="6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
              </button>
              {#if seasonOpen}
                <ul class="dropdown-menu" role="menu" use:autofocusFirst>
                  <li>
                    <button
                      data-nav
                      class:active={seasonId === ""}
                      onclick={() => { changeSeason(""); seasonOpen = false; }}
                    >
                      Todas
                      {#if seasonId === ""}<span class="dot">●</span>{/if}
                    </button>
                  </li>
                  {#each SEASON_OPTS as s}
                    <li>
                      <button
                        data-nav
                        class:active={s.id === seasonId}
                        onclick={() => { changeSeason(s.id); seasonOpen = false; }}
                      >
                        {s.label}
                        {#if s.id === seasonId}<span class="dot">●</span>{/if}
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </div>
        {#if genres.length && !debouncedQ}
          <div class="genres">
            <button
              data-nav
              class="chip"
              class:active={selectedGenres.size === 0}
              onclick={clearGenres}
            >Todos</button>
            {#each genres as g}
              <button
                data-nav
                class="chip"
                class:active={selectedGenres.has(g.id)}
                onclick={() => toggleGenre(g.id)}
              >{g.name}</button>
            {/each}
          </div>
        {/if}
        </div>
      </header>

      {#if listError}<p class="err">{listError}</p>{/if}

      {#if listLoading && !items.length}
        <div class="empty">Cargando…</div>
      {:else}
        <!-- Con cards ya puestas no se vacía la pantalla: se atenúan hasta que
             llega la lista nueva. El "Cargando…" queda para la primera vez,
             cuando no hay nada que atenuar. -->
        <div
          class="grid-wrap"
          class:cargando={listLoading}
          bind:this={gridWrapEl}
          onscroll={() => {
            if (!gridWrapEl) return;
            scrollGrid = gridWrapEl.scrollTop;
            prefetchSiCorresponde();
          }}
        >
          <div class="grid" data-section="gallery" bind:this={gridEl}>
            <!-- Accesos de la casa. Van en una fila propia que ocupa el ancho
                 completo del grid y se reparte a partes iguales: con el grid
                 normal, sumar una tarjeta empujaba la última a la fila de
                 abajo. Acá se comprimen, nunca envuelven. -->
            <div class="accesos">
              <a class="card vera-card" data-nav href="/vera" title="Pregúntale a Vera">
                <div class="vera-poster">
                  <div class="vera-title-poster">
                    <span class="vera-marca">Vera</span>
                    <em>and Chill</em>
                  </div>
                </div>
              </a>

              <a class="card lista-card" data-nav href="/historial" title="Lo que viste y tus favoritos">
                <div class="lista-poster">
                  <div class="vera-title-poster">
                    <span class="lista-marca">Vistos</span>
                    <em class="lista-em">y favoritos</em>
                  </div>
                </div>
              </a>

              {#if SEPA_VISIBLE}
                <div
                  class="card sepa-card"
                  role="presentation"
                  aria-label="Sepá — próximamente"
                  title="Sepá — próximamente"
                >
                  <span class="card-badge badge-coming">Próximamente</span>
                  <div class="sepa-poster">
                    <div class="vera-title-poster">
                      <span class="sepa-marca">Sepá</span>
                      <em class="sepa-em">trivia</em>
                    </div>
                  </div>
                </div>
              {/if}

              <a class="card iptv-card" data-nav href="/iptv" title="TV en vivo">
                <div class="iptv-poster">
                  <div class="vera-title-poster">
                    <span class="iptv-marca">IPTV</span>
                    <em>en vivo</em>
                  </div>
                </div>
              </a>

              {#if tab !== "movie"}
                <button class="card cat-card cat-movie" data-nav onclick={() => switchTab("movie")} title="Películas">
                  <div class="cat-poster">
                    <div class="vera-title-poster">
                      <span class="cat-marca">Pelis</span>
                      <em>cine</em>
                    </div>
                  </div>
                </button>
              {/if}
              {#if tab !== "tv"}
                <button class="card cat-card cat-tv" data-nav onclick={() => switchTab("tv")} title="Series">
                  <div class="cat-poster">
                    <div class="vera-title-poster">
                      <span class="cat-marca">Series</span>
                      <em>tv</em>
                    </div>
                  </div>
                </button>
              {/if}
              {#if tab !== "anime"}
                <button class="card cat-card cat-anime" data-nav onclick={() => switchTab("anime")} title="Anime">
                  <div class="cat-poster">
                    <div class="vera-title-poster">
                      <span class="cat-marca">Anime</span>
                      <em>日本</em>
                    </div>
                  </div>
                </button>
              {/if}
            </div>

            {#each items as it, i (it.id)}
              {@const title = it.title || it.name || ""}
              {@const date = it.release_date || it.first_air_date || ""}
              {@const year = date.slice(0, 4)}
              {@const st = statusMap.get(it.id)}
              <!-- imdbIdMap y seasonsMap se llenan con ids de TMDb. Los del
                   catálogo de anime son de AniList y caen en el mismo rango
                   numérico: sin este guardo, una card de anime heredaba el
                   imdb (y el sello "NO DISPONIBLE") de una peli cualquiera. -->
              {@const itImdb = tab === "anime" ? undefined : imdbIdMap.get(it.id)}
              {@const unavail = itImdb ? unavailableSet.has(itImdb) : false}
              {@const nseasons = tab === "anime" ? undefined : seasonsMap.get(it.id)}
              {@const clv = tab === "anime" ? claveMedio(null, it.id) : claveMedio(itImdb)}
              {@const vis = clv ? estadoDe(clv) : null}
              {#if st !== "none"}
                <button
                  data-nav
                  class="card"
                  class:selected={selected?.id === it.id}
                  class:focused={focusedIdx === i}
                  class:status-checking={st === "checking" || !st}
                  bind:this={cardEls[i]}
                  use:attachCardObserver={it.id}
                  onclick={() => { focusedIdx = i; pickOrTrailer(it, st); }}
                  ondblclick={() => { focusedIdx = i; void pickAndDiscoverOrTrailer(it, st).catch((e) => console.warn("[pickAndDiscoverOrTrailer]", e)); }}
                  onfocus={() => { focusedIdx = i; triggerAutoPick(i); }}
                >
                  {#if it.poster_path}
                    <img src={art(it.poster_path, "w342", 342)} alt={title} loading="lazy" onerror={onImgError} />
                  {:else}
                    <div class="no-poster">sin poster</div>
                  {/if}
                  {#if st === "trailer"}
                    <span class="card-badge badge-trailer">TRAILER</span>
                  {/if}
                  {#if nseasons}
                    <span class="card-badge badge-seasons" class:stacked={st === "trailer"}
                      >{nseasons} {nseasons === 1 ? "temporada" : "temporadas"}</span
                    >
                  {/if}
                  {#if it.vote_average > 0}
                    <span
                      class="card-rating"
                      title="Valoración {tab === 'anime' ? 'AniList' : 'TMDb'}: {it.vote_average.toFixed(1)} / 10"
                      >★ {it.vote_average.toFixed(1)}</span
                    >
                  {/if}
                  {#if unavail}
                    <span class="card-stamp">NO DISPONIBLE</span>
                  {/if}
                  {#if esFavorito(clv)}
                    <span class="card-fav" title="En favoritos">★</span>
                  {/if}
                  {#if vis?.visto}
                    <span class="card-tick" title={vis.ultimoEp ? `Al día · ${vis.vistos} capítulos vistos` : "Ya la viste"}>✓</span>
                  {:else if vis?.parcial}
                    <span
                      class="card-medias"
                      title={vis.ultimoEp
                        ? `A medias · T${vis.ultimoEp.season} E${vis.ultimoEp.episode}`
                        : `A medias · ${vis.pct}%`}
                    >
                      {vis.ultimoEp ? `T${vis.ultimoEp.season} E${vis.ultimoEp.episode}` : `${vis.pct}%`}
                    </span>
                    <span class="card-barra"><span class="card-barra-fill" style:width="{Math.max(3, vis.pct)}%"></span></span>
                  {/if}
                  {#if itImdb}
                    {@const aw = premioDe(itImdb)}
                    {#if aw && aw !== "loading" && (aw.wins > 0 || aw.nominations > 0)}
                      <div class="card-awards" class:stacked={it.vote_average > 0}>
                        <span
                          class="award-pill"
                          class:win={aw.wins > 0}
                          title={awardsTitle(aw)}
                          aria-label={awardsTitle(aw)}>{awardsLabel(aw)}</span
                        >
                      </div>
                    {/if}
                  {/if}
                </button>
              {/if}
            {/each}
            {#each Array(ghostCount) as _, g (g)}
              <div class="ghost-card" class:loading={loadingMore || sparkling} aria-hidden="true">
                {#each SPARKLES as s (s.i)}
                  <span
                    class="sparkle"
                    style:left="{(s.x + g * 7) % 100}%"
                    style:top="{(s.y + g * 11) % 100}%"
                    style:font-size="{s.size}px"
                    style:animation-delay="{s.delay + g * 90}ms"
                    style:color={s.color}
                    style:text-shadow="0 0 6px {s.color}, 0 0 14px {s.color}"
                  >{s.glyph}</span>
                {/each}
              </div>
            {/each}
            {#if !items.length && apiKey}
              <div class="empty">Sin resultados</div>
            {/if}
          </div>
          <div class="sentinel" use:attachObserver></div>
          {#if loadingMore}
            <div class="loading-more">
              <span class="spinner"></span>
              <span>Cargando más…</span>
            </div>
          {:else if !hasMore && items.length > 0}
            <div class="end-mark">
              <span class="dash"></span>
              <span>Fin de la lista</span>
              <span class="dash"></span>
            </div>
          {/if}
        </div>
      {/if}
      {/if}
    </section>
  </main>

  {#if trailerMsg}
    <div class="toast" role="status">{trailerMsg}</div>
  {/if}

  {#if personOpen || personLoading}
    <FichaPersona
      persona={personOpen}
      cargando={personLoading}
      onCerrar={closePerson}
      onAbrirTitulo={openFilmographyItem}
    />
  {/if}

  {#if carousel}
    <Carrusel
      imagenes={carousel.images}
      idx={carousel.idx}
      onCerrar={closeCarousel}
      onMover={carouselGo}
    />
  {/if}

  {#if unavailable.open}
    <AvisoNoDisponible
      titulo={selected?.title ?? ""}
      motivo={unavailable.reason}
      onCerrar={() => (unavailable.open = false)}
      onTrailer={watchTrailer}
    />
  {/if}
{/if}

<style>
  :global(html), :global(body) {
    margin: 0; padding: 0; height: 100%;
    background: #0d0d12; color: #e8e8ea;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    overflow: hidden;
    font-size: 17px;
  }
  main { display: grid; grid-template-columns: 480px 1fr; height: 100%; }
  /* Focus visible global para D-pad */
  :global(button:focus-visible),
  :global([data-nav]:focus-visible) {
    outline: 3px solid #f5c518;
    outline-offset: 2px;
  }

  /* INFO column */
  .info {
    border-right: 1px solid #1f1f28;
    overflow-y: auto;
    position: relative;
    display: flex; flex-direction: column;
    box-shadow: inset 0 0 0 0 transparent;
    transition: box-shadow 0.2s;
  }
  .info:focus-within {
    box-shadow: inset 0 0 0 3px #f5c518;
  }
  .empty { color: #555; padding: 40px 20px; text-align: center; }

  .brand-empty {
    flex: 1;
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    padding: 40px 20px;
    text-align: center;
  }
  .brand-name {
    margin: 0;
    font-size: 72px;
    font-weight: 900;
    letter-spacing: -2px;
    color: #f5c518;
    text-shadow: 0 4px 24px rgba(245, 197, 24, 0.35);
    line-height: 1;
  }
  .brand-tag {
    margin: 16px 0 0;
    color: #666;
    font-size: 14px;
    letter-spacing: 4px;
    text-transform: uppercase;
  }




  /* --- Carrusel de escenas --- */

  /* Person modal */
  /* Mismo lenguaje visual que "ya la viste", en verde más frío: las dos son
     cosas que la app YA sabe del título, no acciones. */



  .key-edit { padding: 12px 20px; border-top: 1px solid #1f1f28; }
  .link { background: none; border: 0; color: #888; font-size: 11px; cursor: pointer; padding: 0; }
  .link:hover { color: #ccc; }

  /* GALLERY */
  .gallery { display: flex; flex-direction: column; min-height: 0; }
  /* Header = caja horizontal: QR (izq) + filtros stacked (der). */
  .gallery > header {
    padding: 10px 16px;
    display: flex;
    flex-direction: row;
    align-items: stretch;
    gap: 12px;
    border-bottom: 1px solid #1f1f28;
    background: #0d0d12;
  }
  .qr-col {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
  }
  .qr-col > :global(.qr-badge) { margin: 0; }
  .filters-col {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    justify-content: center;
  }
  .row1 { display: flex; gap: 10px; align-items: center; }

  .search-wrap {
    flex: 1; position: relative;
    display: flex; align-items: center;
    background: #15151c; border: 1px solid #2a2a35; border-radius: 6px;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .search-wrap:focus-within { border-color: #f5c518; box-shadow: 0 0 0 3px rgba(245,197,24,0.15); }
  .search-icon { position: absolute; left: 10px; color: #666; pointer-events: none; }
  .search {
    flex: 1; padding: 8px 30px 8px 32px;
    background: transparent; border: 0; color: #eee; font-size: 13px;
    outline: none;
  }
  .search::placeholder { color: #555; }
  .search:disabled { opacity: 0.5; }
  .search-clear {
    position: absolute; right: 6px;
    width: 22px; height: 22px; border-radius: 50%;
    background: transparent; border: 0; color: #666; cursor: pointer;
    font-size: 12px; line-height: 1;
    transition: background 0.12s, color 0.12s;
  }
  .search-clear:hover { background: #2a2a35; color: #f5c518; }

  .dropdown { position: relative; }
  .dropdown-trigger {
    display: inline-flex; align-items: center; gap: 8px;
    background: #15151c; border: 1px solid #2a2a35; color: #ddd;
    padding: 8px 12px; border-radius: 6px; cursor: pointer;
    font-size: 13px; font-weight: 500;
    transition: border-color 0.12s, color 0.12s;
  }
  .dropdown-trigger:hover:not(:disabled) { border-color: #f5c518; color: #fff; }
  .dropdown-trigger:disabled { opacity: 0.4; cursor: not-allowed; }
  .dropdown-trigger .chev { transition: transform 0.15s; color: #666; }
  .dropdown-menu {
    position: absolute; top: calc(100% + 4px); right: 0; z-index: 50;
    list-style: none; margin: 0; padding: 4px;
    background: #15151c; border: 1px solid #2a2a35;
    border-radius: 6px; min-width: 170px;
    box-shadow: 0 10px 28px rgba(0,0,0,0.5);
  }
  .dropdown-menu li { margin: 0; }
  .dropdown-menu button {
    width: 100%; text-align: left;
    background: transparent; color: #ddd; border: 0;
    padding: 8px 12px; border-radius: 4px;
    font-size: 13px; cursor: pointer;
    display: flex; justify-content: space-between; align-items: center;
  }
  .dropdown-menu button:hover { background: #1f1f28; color: #fff; }
  .dropdown-menu button.active { color: #f5c518; }
  .dropdown-menu .dot { color: #f5c518; font-size: 8px; }

  .genres {
    display: flex; flex-wrap: wrap; gap: 6px;
    max-height: 78px; overflow-y: auto;
    padding: 2px 0;
  }
  .genres::-webkit-scrollbar { width: 6px; }
  .genres::-webkit-scrollbar-thumb { background: #2a2a35; border-radius: 3px; }
  .chip {
    background: #15151c; border: 1px solid #2a2a35; color: #aaa;
    padding: 4px 11px; border-radius: 999px; font-size: 11px;
    cursor: pointer; white-space: nowrap;
    transition: all 0.12s;
  }
  .chip:hover { color: #fff; border-color: #f5c518; }
  .chip:focus, .chip:focus-visible {
    outline: none;
    border-color: #f5c518;
    color: #fff;
  }
  .chip.active,
  .chip.active:focus,
  .chip.active:focus-visible {
    outline: none;
    background: #f5c518; color: #0d0d12; border-color: #f5c518; font-weight: 700;
  }
  .err { color: #ff6b6b; padding: 12px 16px; font-size: 12px; }

  .grid-wrap {
    flex: 1; overflow-y: auto;
    display: flex; flex-direction: column;
    position: relative;
  }
  /* Lista vieja mientras carga la nueva: se ve que algo está pasando sin
     perder la estructura de la pantalla. */
  .grid-wrap.cargando .grid {
    opacity: 0.42;
    transition: opacity 0.18s ease;
    pointer-events: none;
  }
  /* Ghost cards = placeholders del slot de la próxima card mientras carga. */
  .ghost-card {
    aspect-ratio: 2 / 3;
    background:
      radial-gradient(ellipse at 50% 40%, rgba(245, 197, 24, 0.08), transparent 70%),
      linear-gradient(160deg, rgba(255, 255, 255, 0.02), rgba(0, 0, 0, 0.25));
    border: 1px dashed rgba(245, 197, 24, 0.35);
    border-radius: 4px;
    position: relative;
    overflow: hidden;
    pointer-events: none;
  }
  .sparkle {
    position: absolute;
    display: inline-block;
    transform-origin: center;
    animation: sparkle-pop 1.5s cubic-bezier(.2, .8, .2, 1) infinite;
    will-change: transform, opacity;
    line-height: 1;
  }
  @keyframes sparkle-pop {
    0%   { transform: scale(0) rotate(0deg) translateY(0); opacity: 0; }
    35%  { transform: scale(1.25) rotate(180deg) translateY(-6px); opacity: 1; }
    70%  { transform: scale(0.85) rotate(300deg) translateY(-14px); opacity: 0.8; }
    100% { transform: scale(0) rotate(420deg) translateY(-26px); opacity: 0; }
  }
  .grid {
    padding: 20px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 20px;
    align-content: start;
  }
  .sentinel { height: 1px; }
  .loading-more, .end-mark {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    padding: 20px; color: #666; font-size: 12px;
    letter-spacing: 0.5px; text-transform: uppercase;
  }
  .spinner {
    width: 14px; height: 14px; border-radius: 50%;
    border: 2px solid #2a2a35; border-top-color: #f5c518;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .end-mark .dash { width: 30px; height: 1px; background: #333; }
  .card {
    background: #15151c; border: 0; border-radius: 6px;
    overflow: hidden; cursor: pointer; padding: 0;
    color: inherit; text-align: left;
    display: flex; flex-direction: column;
    /* Sin transición en box-shadow: el halo amarillo del foco queda
       "pegado" al cambiar de card si dejamos que se desvanezca. */
    transition: transform 0.15s;
  }
  .card:hover { transform: translateY(-3px); box-shadow: 0 12px 28px rgba(0,0,0,0.6); }
  .card.selected { box-shadow: 0 0 0 2px #f5c518; }

  /* Fila de accesos: una sola línea, siempre. Ocupa todas las columnas del
     grid y reparte el ancho en partes iguales, así sumar o sacar una tarjeta
     encoge las demás en vez de mandar la última a la fila de abajo. */
  .accesos {
    grid-column: 1 / -1;
    display: flex;
    gap: 20px;
    align-items: start;
  }
  .accesos > .card {
    flex: 1 1 0;
    min-width: 0;
  }
  /* Con 7 accesos en una pantalla angosta el texto es lo primero que revienta:
     escala con el ancho de la tarjeta y nunca corta en dos líneas. */
  .accesos .vera-title-poster {
    max-width: 100%;
  }
  .accesos .vera-title-poster span,
  .accesos .vera-title-poster em {
    font-size: clamp(11px, 1.5vw, 24px);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .vera-card { text-decoration: none; }
  .vera-poster {
    aspect-ratio: 2 / 1;
    background:
      radial-gradient(ellipse at 50% 35%, #1a1326 0%, #0c0810 65%, #050307 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .vera-poster::before {
    content: ""; position: absolute; inset: 0;
    background:
      radial-gradient(circle at 30% 20%, rgba(255, 174, 92, 0.08), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(117, 7, 135, 0.10), transparent 55%);
    pointer-events: none;
  }
  .vera-card:hover { box-shadow: 0 12px 32px rgba(255,87,34,0.4); }

  .vera-title-poster {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 0 10px;
    line-height: 1.05;
    letter-spacing: -0.5px;
  }
  .vera-title-poster .vera-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 700;
    background: linear-gradient(
      90deg,
      #e40303,
      #ff8c00,
      #ffed00,
      #008026,
      #004dff,
      #750787
    );
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  .vera-title-poster em {
    font-size: clamp(16px, 2.4vw, 22px);
    color: #ffae5c;
    font-style: italic;
    font-weight: 300;
    text-shadow: 0 0 14px rgba(255, 140, 60, 0.45);
    margin-top: 4px;
  }

  /* Vistos: entra al historial. Va pegada a Vera porque las dos hablan del
     gusto de la casa — una lo predice, la otra lo recuerda. */
  .lista-card { text-decoration: none; }
  .lista-poster {
    aspect-ratio: 2 / 1;
    background: radial-gradient(ellipse at 50% 35%, #06251a 0%, #04140f 65%, #020807 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .lista-poster::before {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background:
      radial-gradient(circle at 30% 20%, rgba(47, 158, 68, 0.18), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(126, 224, 148, 0.12), transparent 55%);
  }
  .lista-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 800;
    background-image: linear-gradient(90deg, #2f9e44, #7ee094, #d6ffe0, #7ee094, #2f9e44);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  .lista-em {
    color: #7ee094 !important;
    -webkit-text-fill-color: #7ee094 !important;
    text-shadow: 0 0 14px rgba(47, 158, 68, 0.45) !important;
  }

  /* Sepá: misma fila que Vera, esquina derecha. Placeholder "Próximamente". */
  .sepa-card {
    cursor: default;
    text-decoration: none;
  }
  .sepa-poster {
    aspect-ratio: 2 / 1;
    background:
      radial-gradient(ellipse at 50% 35%, #0d1530 0%, #060916 65%, #03030a 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .sepa-poster::before {
    content: ""; position: absolute; inset: 0;
    background:
      radial-gradient(circle at 30% 20%, rgba(110, 193, 255, 0.10), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(74, 142, 216, 0.10), transparent 55%);
    pointer-events: none;
  }
  .sepa-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 700;
    background: linear-gradient(90deg, #4a8ed8, #6ec1ff, #b3d9ff, #6ec1ff, #4a8ed8);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  .sepa-em {
    color: #9cc8ff !important;
    -webkit-text-fill-color: #9cc8ff !important;
    text-shadow: 0 0 14px rgba(110, 193, 255, 0.45) !important;
  }
  /* IPTV — rojo/naranjo señal en vivo. */
  .iptv-card { text-decoration: none; }
  .iptv-poster {
    aspect-ratio: 2 / 1;
    background: radial-gradient(ellipse at 50% 35%, #2a0d08 0%, #160806 65%, #0a0303 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .iptv-poster::before {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background:
      radial-gradient(circle at 30% 20%, rgba(239, 68, 68, 0.16), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(249, 115, 22, 0.14), transparent 55%);
  }
  .iptv-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 800;
    background-image: linear-gradient(90deg, #ef4444, #f97316, #fbbf24, #f97316, #ef4444);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  .iptv-card em { color: #fca56b !important; text-shadow: 0 0 14px rgba(249, 115, 22, 0.45); }

  /* Cards de categoría (Películas / Series / Anime) — reemplazan los tabs. */
  .cat-card {
    text-decoration: none;
    border: none;
    padding: 0;
    cursor: pointer;
    font: inherit;
    color: inherit;
    text-align: left;
  }
  .cat-poster {
    aspect-ratio: 2 / 1;
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .cat-poster::before {
    content: ""; position: absolute; inset: 0;
    pointer-events: none;
  }
  .cat-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 800;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  /* Películas — dorado/ámbar */
  .cat-movie .cat-poster {
    background: radial-gradient(ellipse at 50% 35%, #2a2008 0%, #14100a 65%, #070503 100%);
  }
  .cat-movie .cat-poster::before {
    background:
      radial-gradient(circle at 30% 20%, rgba(245, 197, 24, 0.14), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(255, 140, 60, 0.12), transparent 55%);
  }
  .cat-movie .cat-marca {
    background-image: linear-gradient(90deg, #f5c518, #ffd76a, #fff0b3, #ffd76a, #f5c518);
  }
  .cat-movie em { color: #ffd76a !important; text-shadow: 0 0 14px rgba(245, 197, 24, 0.45); }

  /* Series — verde/teal */
  .cat-tv .cat-poster {
    background: radial-gradient(ellipse at 50% 35%, #07261c 0%, #051613 65%, #03090a 100%);
  }
  .cat-tv .cat-poster::before {
    background:
      radial-gradient(circle at 30% 20%, rgba(52, 211, 153, 0.14), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(16, 185, 129, 0.12), transparent 55%);
  }
  .cat-tv .cat-marca {
    background-image: linear-gradient(90deg, #10b981, #34d399, #a7f3d0, #34d399, #10b981);
  }
  .cat-tv em { color: #6ee7b7 !important; text-shadow: 0 0 14px rgba(52, 211, 153, 0.45); }

  /* Anime — rosa sakura */
  .cat-anime .cat-poster {
    background: radial-gradient(ellipse at 50% 35%, #2e0f24 0%, #1a0814 65%, #0d0309 100%);
  }
  .cat-anime .cat-poster::before {
    background:
      radial-gradient(circle at 30% 20%, rgba(244, 114, 182, 0.16), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(217, 70, 160, 0.12), transparent 55%);
  }
  .cat-anime .cat-marca {
    background-image: linear-gradient(90deg, #f472b6, #ff9ad4, #ffd1ec, #ff9ad4, #f472b6);
  }
  .cat-anime em { color: #ff9ad4 !important; text-shadow: 0 0 14px rgba(244, 114, 182, 0.45); }
  .badge-coming {
    background: #4a8ed8;
    color: #04101e;
  }
  .card:focus, .card:focus-visible {
    outline: 4px solid #f5c518;
    outline-offset: 3px;
    transform: translateY(-3px) scale(1.03);
    box-shadow: 0 14px 36px rgba(0,0,0,0.7), 0 0 0 6px rgba(245,197,24,0.18);
    z-index: 5;
  }
  .card { position: relative; }
  .card.status-checking { opacity: 0.85; }
  .card-badge {
    position: absolute; top: 8px; right: 8px;
    padding: 4px 10px; border-radius: 4px;
    font-size: 10px; font-weight: 900; letter-spacing: 1.5px;
    text-transform: uppercase; z-index: 2;
    box-shadow: 0 4px 12px rgba(0,0,0,0.5);
  }
  .badge-trailer {
    background: #f5c518; color: #0d0d12;
  }
  .badge-seasons {
    background: rgba(13, 13, 18, 0.82);
    color: #fff;
    letter-spacing: 0.5px;
    backdrop-filter: blur(2px);
  }
  /* Cuando la card también muestra el badge TRAILER (top-right), bajamos. */
  .badge-seasons.stacked { top: 38px; }
  .card-stamp {
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%) rotate(-15deg);
    background: rgba(220, 38, 38, 0.92);
    color: #fff;
    padding: 5px 14px;
    font-weight: 900; font-size: 12px;
    letter-spacing: 1.5px;
    border: 2.5px solid #fff;
    border-radius: 3px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.6);
    text-transform: uppercase;
    pointer-events: none;
    z-index: 3;
    white-space: nowrap;
  }
  /* Fila de acciones de historial del panel de info. */
  .ep-card.ep-visto img {
    opacity: 0.45;
  }
  /* --- Marcas de historial en la tarjeta -------------------------------
     Tick verde = terminado. Píldora + barra naranja = a medias. La estrella
     de favorito va abajo a la izquierda porque arriba viven los premios. */
  .card-tick {
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
    z-index: 3;
    pointer-events: none;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.65);
  }
  .card-medias {
    position: absolute;
    right: 8px;
    bottom: 12px;
    padding: 3px 8px;
    border-radius: 4px;
    background: rgba(13, 13, 18, 0.85);
    color: #f3a951;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.4px;
    z-index: 3;
    pointer-events: none;
    backdrop-filter: blur(2px);
  }
  .card-barra {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 5px;
    background: rgba(0, 0, 0, 0.55);
    z-index: 3;
    pointer-events: none;
  }
  .card-barra-fill {
    display: block;
    height: 100%;
    background: #f3a951;
  }
  .card-fav {
    position: absolute;
    left: 8px;
    bottom: 8px;
    color: #f5c518;
    font-size: 17px;
    line-height: 1;
    z-index: 3;
    pointer-events: none;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.8);
  }
  /* Nota (TMDb / AniList, ambas /10) arriba a la izquierda del póster. La
     esquina la comparte con los premios, que bajan con .stacked. */
  .card-rating {
    position: absolute;
    top: 8px;
    left: 8px;
    padding: 3px 8px;
    border-radius: 4px;
    background: rgba(13, 13, 18, 0.85);
    color: #f5c518;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.3px;
    z-index: 3;
    pointer-events: none;
    backdrop-filter: blur(2px);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.6);
  }
  .card-awards {
    position: absolute;
    top: 8px;
    left: 8px;
    display: flex;
    z-index: 2;
    pointer-events: none;
    /* Deja aire a la derecha: la píldora no debe pisar el borde del póster. */
    max-width: calc(100% - 16px);
  }
  /* Cuando la card también muestra la nota (top-left), bajamos. */
  .card-awards.stacked { top: 34px; }
  .award-pill {
    background: rgba(0, 0, 0, 0.72);
    /* Gris por defecto = solo nominaciones. El dorado lo pone .win. */
    color: #d8d8d8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 999px;
    letter-spacing: 0.4px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    backdrop-filter: blur(4px);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.6);
  }
  .award-pill.win {
    color: #ffd76a;
    text-shadow: 0 0 6px rgba(255, 215, 0, 0.5);
  }
  .card img, .no-poster { width: 100%; aspect-ratio: 2/3; object-fit: cover; background: #222; }
  .no-poster { display: flex; align-items: center; justify-content: center; color: #555; font-size: 13px; }

  /* Capítulos: thumbnail apaisado 16/9 en vez del poster 2/3. */
  .ep-card img, .ep-card .ep-noimg { aspect-ratio: 16/9; }
  /* Sin captura propia usamos la portada, recortada al mismo 16/9 y algo
     apagada para que no compita con los capítulos que sí tienen imagen. El
     número va encima, que si no todos los capítulos se ven idénticos. */
  .ep-card .ep-fallback { object-position: center 30%; filter: brightness(0.6); }
  .ep-card .ep-num {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 20px;
    font-weight: 800;
    color: #fff;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.9);
    pointer-events: none;
  }

  /* Barra de encabezado del listado de capítulos (back + breadcrumb). */
  .ep-head-bar {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .ep-back-inline {
    background: rgba(20, 20, 28, 0.6);
    border: 1px solid rgba(255, 255, 255, 0.14);
    color: #d8d8e0;
    border-radius: 8px;
    padding: 8px 14px;
    cursor: pointer;
    font-size: 13px;
  }
  .ep-back-inline:focus-visible {
    outline: none;
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
  }
  .ep-crumb {
    font-size: 15px;
    font-weight: 600;
    color: #e6e6ec;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Lista vertical de temporadas en el panel derecho. */

  /* PLAY mode */

  /* Vista unavailable */
  /* Último recurso del trailer: QR para verlo en el celular. */



  /* Modal */

  .toast {
    position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%);
    z-index: 850;
    background: #1a1a22; color: #f5c518;
    border: 1px solid #f5c518;
    padding: 12px 20px; border-radius: 8px;
    font-size: 14px; font-weight: 500;
    box-shadow: 0 10px 30px rgba(0,0,0,0.6);
    max-width: 80vw;
    animation: toast-in 0.25s ease-out;
  }
  @keyframes toast-in {
    from { transform: translate(-50%, 20px); opacity: 0; }
    to   { transform: translate(-50%, 0);    opacity: 1; }
  }
</style>
