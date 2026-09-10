<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
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
  import { leerImgCache, guardarImgCache } from "$lib/imgcache";
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

  async function setFs(on: boolean) {
    try {
      await getCurrentWindow().setFullscreen(on);
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
          if (ev.state === "Pressed") dispatchBack();
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
            if (ev.state === "Pressed") spatialNav(map[k]);
          });
        } catch (e) {
          console.warn(`shortcut ${k} no registrado`, e);
        }
      }
    }
  }

  async function unregisterBackShortcuts() {
    for (const k of [...BACK_KEYS, ...ARROW_KEYS]) {
      try { await unregister(k); } catch { /* tolerado */ }
    }
  }

  type ListItem = {
    id: number;
    title?: string;
    name?: string;
    poster_path?: string;
    overview: string;
    vote_average: number;
    release_date?: string;
    first_air_date?: string;
  };
  type ListResp = { page: number; total_pages: number; results: ListItem[] };
  type PersonMini = {
    id: number;
    name: string;
    profile_path?: string;
    character?: string;
    job?: string;
  };
  type Detail = {
    id: number;
    media_type: "movie" | "tv";
    title: string;
    overview: string;
    poster_path?: string;
    backdrop_path?: string;
    vote_average: number;
    year: string;
    imdb_id?: string;
    runtime?: number;
    original_title?: string | null;
    genres: string[];
    directors: PersonMini[];
    cast: PersonMini[];
    images?: string[];
    number_of_seasons?: number | null;
    seasons?: SeasonMini[];
    // --- extras anime (solo cuando la fuente es AniList) ---
    is_anime?: boolean;
    mal_id?: number | null;
    kitsu_id?: number | null;
    anidb_id?: number | null;
    trailer_youtube?: string | null;
    format?: string | null;
  };
  type SeasonMini = {
    season_number: number;
    episode_count: number;
    name: string;
    air_date: string | null;
    poster_path: string | null;
  };
  type PersonFilm = {
    id: number;
    title: string;
    poster_path?: string;
    year: string;
    media_type: string;
    roles: string[];
    vote_average: number;
    popularity: number;
  };
  type PersonInfo = {
    id: number;
    name: string;
    biography: string;
    profile_path?: string;
    birthday?: string;
    deathday?: string;
    place_of_birth?: string;
    known_for_department?: string;
    filmography: PersonFilm[];
  };

  // Sepá (trivia) todavía no existe: su tarjeta ocupaba un lugar en la fila de
  // accesos y no lleva a ninguna parte. Escondida hasta que haya algo detrás —
  // el markup y los estilos quedan, poner en true la devuelve.
  const SEPA_VISIBLE = false;

  const IMG = "https://image.tmdb.org/t/p";

  // Cache local WebP — guarda URL→file path una vez bajado.
  // Se hidrata de localStorage: sin esto, cada arranque pintaba primero desde
  // image.tmdb.org y recién después cambiaba al archivo del disco, aunque el
  // archivo ya estuviera ahí. Guardamos el path crudo y convertimos al leer.
  let cachedImgs = $state<Map<string, string>>(new Map(leerImgCache()));
  const cacheInflight = new Set<string>();
  let imgPersistTimer: ReturnType<typeof setTimeout> | null = null;
  // Escribir en cada póster bajado sería un JSON.stringify por imagen: se
  // agenda una sola pasada cuando la ráfaga termina.
  function persistirImgCache() {
    if (imgPersistTimer) clearTimeout(imgPersistTimer);
    imgPersistTimer = setTimeout(() => {
      imgPersistTimer = null;
      guardarImgCache(Array.from(cachedImgs));
    }, 1500);
  }
  async function ensureCached(url: string, maxW: number) {
    const key = `${url}::${maxW}`;
    if (cachedImgs.has(key) || cacheInflight.has(key)) return;
    cacheInflight.add(key);
    try {
      const path = await invoke<string>("cache_image", { url, maxW });
      const m = new Map(cachedImgs);
      m.set(key, path);
      cachedImgs = m;
      persistirImgCache();
    } catch (e) {
      // Si falla, dejamos URL original
    } finally {
      cacheInflight.delete(key);
    }
  }
  function img(url: string | null | undefined, maxW: number): string {
    if (!url) return "";
    const key = `${url}::${maxW}`;
    const cached = cachedImgs.get(key);
    if (cached) return convertFileSrc(cached);
    ensureCached(url, maxW);
    return url;
  }
  // El archivo cacheado puede no estar (limpieza del sistema, borrado manual).
  // Sacamos la entrada y el re-render vuelve solo a la URL remota, que a su vez
  // dispara ensureCached de nuevo.
  function onImgError(e: Event) {
    const src = (e.currentTarget as HTMLImageElement | null)?.src;
    if (!src || !src.includes("asset")) return;
    for (const [k, path] of cachedImgs) {
      if (convertFileSrc(path) !== src) continue;
      const m = new Map(cachedImgs);
      m.delete(k);
      cachedImgs = m;
      persistirImgCache();
      return;
    }
  }
  // Como img() pero acepta fragmento TMDb ("/abc.jpg") O URL completa:
  // AniList/ani.zip mandan https://… directo, sin base TMDb.
  function art(path: string | null | undefined, size: string, maxW: number): string {
    if (!path) return "";
    return path.startsWith("http") ? img(path, maxW) : img(`${IMG}/${size}${path}`, maxW);
  }
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

  type AwardsSummary = { wins: number; nominations: number };
  let awardsMap = $state<Map<string, AwardsSummary | "loading">>(new Map());
  let awardsQueue: string[] = [];
  let awardsActive = 0;
  const AWARDS_CONCURRENCY = 3;

  // Una sola píldora: "🏆 3 · 5 nom.". Dos emojis de medalla apilados eran
  // indistinguibles a 10px en la esquina del póster — y el title= es tooltip
  // de mouse, que en la tele no existe. Sin premios ganados no va el trofeo:
  // mentiría. La palabra "nom." es lo que desambigua.
  function awardsLabel(aw: AwardsSummary): string {
    const partes: string[] = [];
    if (aw.wins > 0) partes.push(`🏆 ${aw.wins}`);
    if (aw.nominations > 0) partes.push(`${aw.nominations} nom.`);
    return partes.join(" · ");
  }

  function awardsTitle(aw: AwardsSummary): string {
    const partes: string[] = [];
    if (aw.wins > 0) partes.push(`${aw.wins} ${aw.wins === 1 ? "premio" : "premios"}`);
    if (aw.nominations > 0) {
      partes.push(
        `${aw.nominations} ${aw.nominations === 1 ? "nominación" : "nominaciones"}`,
      );
    }
    return partes.join(" · ");
  }

  function enqueueAwards(imdb: string) {
    if (!imdb || awardsMap.has(imdb)) return;
    const m = new Map(awardsMap);
    m.set(imdb, "loading");
    awardsMap = m;
    awardsQueue.push(imdb);
    pumpAwards();
  }

  async function pumpAwards() {
    while (awardsActive < AWARDS_CONCURRENCY && awardsQueue.length > 0) {
      const imdb = awardsQueue.shift()!;
      awardsActive++;
      void (async () => {
        try {
          const s: AwardsSummary = await invoke("wikidata_awards", { imdbId: imdb });
          const m = new Map(awardsMap);
          m.set(imdb, s);
          awardsMap = m;
        } catch (e) {
          console.warn("[awards]", imdb, e);
          const m = new Map(awardsMap);
          m.set(imdb, { wins: 0, nominations: 0 });
          awardsMap = m;
        } finally {
          awardsActive--;
          pumpAwards();
        }
      })();
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
  type OmdbRating = { source: string; value: string };
  type OmdbDetail = {
    plot?: string | null;
    awards?: string | null;
    rated?: string | null;
    writer?: string | null;
    country?: string | null;
    language?: string | null;
    released?: string | null;
    metascore?: string | null;
    imdb_rating?: string | null;
    imdb_votes?: string | null;
    box_office?: string | null;
    production?: string | null;
    ratings: OmdbRating[];
  };
  let menuOmdb = $state<OmdbDetail | null>(null);
  let menuTrailerPick = $state<TrailerPick>({ ytKey: "", apple: "", playable: false, err: "" });
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

    // Volver de otra ruta (/config, /vera, /juegos, /iptv) remonta esta página.
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
      awardsMap = new Map(snap.awardsMap);
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

  // Cleanup separado en onDestroy: onMount es async y no acepta return
  // de cleanup en ese caso. El listener postMessage del iframe player
  // tiene que limpiarse cuando home desmonta.
  onDestroy(() => {
    guardarSnapshotCatalogo();
    // Volcar ya el cache de imágenes: si el debounce estaba pendiente, los
    // pósters bajados en esta pantalla se perderían.
    if (imgPersistTimer) {
      clearTimeout(imgPersistTimer);
      imgPersistTimer = null;
      guardarImgCache(Array.from(cachedImgs));
    }
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
      awardsMap: Array.from(awardsMap).filter(
        (e): e is [string, AwardsSummary] => e[1] !== "loading",
      ),
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
    // Tab inicial vía ?tab= (cards de sección en /juegos → /?tab=tv, etc.).
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
          "*",
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

  // Resultado de la búsqueda de trailer.
  //  - `ytKey`: video de YouTube que existe (verificado contra oEmbed).
  //  - `playable`: yt-dlp lo puede resolver → mpv lo reproduce.
  //  - `apple`: mp4 de iTunes, alternativa cuando YouTube no se puede.
  type TrailerPick = { ytKey: string; apple: string; playable: boolean; err: string };

  // Los trailers se reproducen SIEMPRE en mpv, nunca en el webview:
  //  1. El iframe de YouTube da "error 153" en Tauri (el origen es
  //     `tauri://localhost`, que no es un referrer http(s) válido) — pasa con
  //     todos los videos, permitan embed o no. Invidious/Piped tampoco: sus
  //     instancias públicas están caídas o bloqueadas.
  //  2. Un <video> tampoco sirve: YouTube ya casi no entrega formatos
  //     progresivos, el video y el audio van en streams DASH separados y el
  //     webview no los puede juntar.
  // mpv sí los junta (ytdl_hook + yt-dlp de vendor/), así que la reproducción
  // va por ahí. Si ni eso, se muestra un QR para verlo en el celular.
  //
  // TMDb va antes que Apple porque se consulta por id exacto y nunca devuelve
  // otra película; iTunes solo se puede buscar por texto y para títulos fuera
  // del catálogo US (cine coreano, indio, europeo) el match es frágil.
  async function resolveTrailer(d: {
    media_type: "movie" | "tv";
    id: number;
    title: string;
    original_title?: string | null;
    year?: string;
  }): Promise<TrailerPick> {
    const out: TrailerPick = { ytKey: "", apple: "", playable: false, err: "" };
    try {
      const tk = await invoke<{ key: string; embeddable: boolean } | null>("tmdb_trailer_key", {
        mediaType: d.media_type,
        id: d.id,
        apiKey,
      });
      if (tk?.key) out.ytKey = tk.key;
    } catch (e) {
      console.warn("[tmdb_trailer_key]", e);
    }
    if (out.ytKey) {
      const chk = await ytPlayable(out.ytKey);
      out.playable = chk.ok;
      out.err = chk.err;
      if (out.playable) return out;
    }
    try {
      const a = await invoke<{ url: string } | null>("apple_trailer", {
        title: d.title,
        originalTitle: d.original_title || "",
        year: d.year || "",
        mediaType: d.media_type,
      });
      if (a?.url) out.apple = a.url;
    } catch (e) {
      console.warn("[apple_trailer]", e);
    }
    return out;
  }

  // ¿yt-dlp puede resolver el video? `err` casi siempre es que falta el binario
  // en vendor/ (lo baja vendor/fetch.sh) o que YouTube pide verificación.
  async function ytPlayable(key: string): Promise<{ ok: boolean; err: string }> {
    try {
      const ok = await invoke<boolean>("yt_playable", { key });
      return { ok, err: ok ? "" : "yt-dlp no resolvió el video" };
    } catch (e) {
      console.warn("[yt_playable]", e);
      return { ok: false, err: String(e) };
    }
  }

  // Trailer de anime: la key viene de AniList; TMDb/Apple no aplican porque el
  // id no es de TMDb.
  async function resolveAnimeTrailer(key: string): Promise<TrailerPick> {
    const out: TrailerPick = { ytKey: key, apple: "", playable: false, err: "" };
    if (!key) return out;
    const chk = await ytPlayable(key);
    out.playable = chk.ok;
    out.err = chk.err;
    return out;
  }

  // Reproduce el trailer en mpv. Devuelve false si mpv no lo aceptó.
  async function playTrailerInMpv(url: string, title: string): Promise<boolean> {
    // Un trailer no es "haber visto la película": sin limpiar el contexto, dos
    // minutos de avance marcarían el título como empezado.
    limpiarContexto();
    try {
      // mpv_play_trailer, no mpv_play: un trailer no reemplaza la película que
      // estuviera esperando ni se queda él mismo en la pill al salir.
      await invoke("mpv_play_trailer", { url, title: `Trailer — ${title}` });
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

  function autofocusFirst(node: HTMLElement) {
    setTimeout(() => {
      const btn = node.querySelector<HTMLElement>(".btn-primary, [data-nav]");
      btn?.focus();
    }, 30);
    return {};
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
  type EpisodeMini = {
    episode_number: number;
    name: string;
    overview: string;
    still_path: string | null;
    air_date: string | null;
    runtime: number | null;
  };
  // null = catálogo normal; nº = mostrando capítulos de esa temporada.
  let seriesSeason = $state<number | null>(null);
  let seriesEpisodes = $state<EpisodeMini[]>([]);
  let episodesLoading = $state(false);
  let episodesError = $state("");
  // Cache por `${tmdbId}:${season}` para no rebajar episodios al volver.
  const seasonEpCache = new Map<string, EpisodeMini[]>();

  function seasonLabel(n: number): string {
    return n === 0 ? "Especiales" : `Temporada ${n}`;
  }

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

  // Cambia de temporada sin salir del listado de capítulos (LB/RB del mando,
  // [ ] o RePág/AvPág en teclado). Con el grid abierto la lista de temporadas
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

  // Sin capítulos abiertos, LB/RB rotan la pestaña del catálogo. Antes no hacían
  // nada en la home, aunque el mando los manda igual que en /juegos e /historial.
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

  const EMPTY_PICK: TrailerPick = { ytKey: "", apple: "", playable: false, err: "" };

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
          const pick = await resolveTrailer(detail);
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

  // Reproduce lo mejor disponible del pick; QR si no hay forma de verlo dentro.
  async function playPick(pick: TrailerPick, title: string): Promise<void> {
    if (pick.playable && pick.ytKey) {
      if (await playTrailerInMpv(`https://www.youtube.com/watch?v=${pick.ytKey}`, title)) return;
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
  type SugerenciaFin = { id: number; title: string; posterUrl: string | null; year: string };
  type SiguienteFin = { season: number; episode: number; nombre: string; stillUrl: string | null };

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
  // Anime → AniList (secuela primero); películas y series → TMDb.
  async function sugerenciasDe(): Promise<SugerenciaFin[]> {
    if (!selected) return [];
    try {
      let items: ListItem[] = [];
      if (tab === "anime") {
        const r = await invoke<ListResp>("anilist_relacionados", { id: selected.id });
        items = r.results ?? [];
      } else {
        if (!apiKey) return [];
        items = await recomendacionesTmdb(selected.id);
      }
      return await filtrarSugerencias(items);
    } catch (e) {
      console.warn("[fin] sugerencias", e);
      return [];
    }
  }

  async function recomendacionesTmdb(id: number): Promise<ListItem[]> {
    const mediaType = tabToMediaType(tab);
    const pedir = async (kind: "recommendations" | "similar") => {
      const r = await invoke<ListResp>("tmdb_recommendations", {
        mediaType,
        id,
        page: 1,
        apiKey,
        kind,
      });
      return r.results ?? [];
    };
    const recs = await pedir("recommendations");
    // TMDb devuelve pocas (o ninguna) recomendación en títulos de nicho:
    // "similar" es el respaldo, con el mismo shape.
    if (recs.length >= 4) return recs;
    const sim = await pedir("similar").catch(() => [] as ListItem[]);
    return [...recs, ...sim.filter((x) => !recs.some((y) => y.id === x.id))];
  }

  const MAX_SUGERENCIAS = 8;

  // Deja fuera lo que no sirve ofrecer: lo ya visto, lo marcado como no
  // disponible y (en TMDb) lo que no tiene imdb, que es lo que necesitan los
  // scrapers para encontrar fuentes.
  async function filtrarSugerencias(items: ListItem[]): Promise<SugerenciaFin[]> {
    const out: SugerenciaFin[] = [];
    if (tab === "anime") {
      for (const it of items) {
        if (out.length >= MAX_SUGERENCIAS) break;
        if (estadoDe(claveMedio(null, it.id))?.visto) continue;
        out.push(aSugerencia(it));
      }
      return out;
    }
    // TMDb: el imdb sale de item_status (una llamada por título). Se miran solo
    // las primeras candidatas, en paralelo, para no encadenar 20 llamadas.
    const candidatas = items.slice(0, MAX_SUGERENCIAS + 6);
    const estados = await Promise.all(
      candidatas.map((it) =>
        invoke<{ has_imdb: boolean; imdb_id: string | null }>("item_status", {
          mediaType: tabToMediaType(tab),
          id: it.id,
          apiKey,
        }).catch(() => null),
      ),
    );
    for (let i = 0; i < candidatas.length && out.length < MAX_SUGERENCIAS; i++) {
      const st = estados[i];
      if (!st?.has_imdb || !st.imdb_id) continue; // sin imdb no hay fuentes
      if (unavailableSet.has(st.imdb_id)) continue;
      if (estadoDe(claveMedio(st.imdb_id))?.visto) continue;
      out.push(aSugerencia(candidatas[i]));
    }
    return out;
  }

  function aSugerencia(it: ListItem): SugerenciaFin {
    finItems.set(it.id, it);
    const fecha = it.release_date || it.first_air_date || "";
    return {
      id: it.id,
      title: it.title || it.name || "",
      posterUrl: it.poster_path ? art(it.poster_path, "w342", 342) : null,
      year: fecha.slice(0, 4),
    };
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
      : await resolveTrailer(selected);
    trailerMsg = "";
    console.log("[trailer]", title, "→", pick);

    await playPick(pick, title);
  }

  function getNavRoot(): ParentNode {
    if (personOpen || personLoading) return document.querySelector(".person-modal") || document;
    if (unavailable.open) return document.querySelector(".modal") || document;
    return document;
  }

  function getSection(el: HTMLElement | null): string | null {
    let node: HTMLElement | null = el;
    while (node && node !== document.body) {
      const s = node.dataset?.section;
      if (s) return s;
      node = node.parentElement;
    }
    return null;
  }

  function findBest(
    cur: HTMLElement,
    candidates: HTMLElement[],
    dir: "up" | "down" | "left" | "right"
  ): HTMLElement | null {
    const r = cur.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    let best: HTMLElement | null = null;
    let bestDist = Infinity;
    for (const el of candidates) {
      if (el === cur) continue;
      const er = el.getBoundingClientRect();
      const ex = er.left + er.width / 2;
      const ey = er.top + er.height / 2;
      const dx = ex - cx;
      const dy = ey - cy;
      let primary = 0, secondary = 0, valid = false;
      if (dir === "right") { valid = dx > 6; primary = dx; secondary = Math.abs(dy); }
      else if (dir === "left")  { valid = dx < -6; primary = -dx; secondary = Math.abs(dy); }
      else if (dir === "down")  { valid = dy > 6; primary = dy; secondary = Math.abs(dx); }
      else                       { valid = dy < -6; primary = -dy; secondary = Math.abs(dx); }
      if (!valid) continue;
      // ¿El candidato se cruza con el actual en el eje perpendicular? O sea,
      // ¿está literalmente al frente? Con un castigo lateral fijo (era x1.4)
      // un elemento MUY lejano pero centrado le ganaba a uno pegado y corrido:
      // en una serie, bajar desde Trailer se saltaba las temporadas (anchas,
      // centro lejos del botón) y aterrizaba en el director, 200px más abajo.
      const alFrente =
        dir === "left" || dir === "right"
          ? er.bottom > r.top + 6 && er.top < r.bottom - 6
          : er.right > r.left + 6 && er.left < r.right - 6;
      const dist = primary + secondary * (alFrente ? 0.2 : 2.5);
      if (dist < bestDist) { bestDist = dist; best = el; }
    }
    return best;
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
    const curSection = getSection(cur);
    // Pass 1: candidatos en la misma sección (cautivos)
    let best: HTMLElement | null = null;
    if (curSection) {
      const sameSec = all.filter((el) => getSection(el) === curSection);
      best = findBest(cur, sameSec, dir);
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
        best = findBest(cur, all, dir);
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
  <div class="unavail-screen">
    <div class="unavail-card">
      {#if selected.poster_path}
        <img class="unavail-poster" src={art(selected.poster_path, "w342", 342)} alt="" onerror={onImgError} />
      {/if}
      <h2>Lo sentimos muchísimo</h2>
      <p>
        <strong>{selected.title}</strong> aún no se encuentra en nuestra cartelera.
      </p>
      <p class="unavail-sub">La marcamos así no la sugerimos próximamente. Si volviera a estar disponible, podés desmarcarla.</p>
      <div class="unavail-actions">
        <button data-nav class="unavail-back btn-secondary" onclick={stopDiscover}>← Volver</button>
        {#if selected.imdb_id}
          <button data-nav class="btn-primary" onclick={watchTrailer}>🎬 Ver trailer</button>
          <button data-nav class="btn-secondary" onclick={async () => {
            if (selected?.imdb_id) {
              await clearUnavailable(selected.imdb_id);
              mode = "browse";
              setTimeout(volverAlGrid, 50);
            }
          }}>
            Marcar disponible
          </button>
        {/if}
      </div>
    </div>
  </div>
{:else if mode === "playmenu" && (selected?.imdb_id || selected?.kitsu_id)}
  <PlayMenu
    detail={selected}
    omdb={menuOmdb}
    progressLabel={progressLabelFor()}
    posterUrl={selected.poster_path ? art(selected.poster_path, "w342", 342) : null}
    backdropUrl={selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null}
    hasTrailer={!!(menuTrailerPick.ytKey || menuTrailerPick.apple)}
    hasRd={config.rdLinked}
    onContinue={menuContinue}
    onRestart={menuRestart}
    onRealDebrid={menuRealDebrid}
    onResearch={menuResearch}
    onTrailer={menuTrailer}
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
    season={sourcesSeason}
    episode={sourcesEpisode}
    title={selected.title}
    originalTitle={selected.original_title ?? null}
    backdrop={selected.backdrop_path ? art(selected.backdrop_path, "w1280", 1280) : null}
    kitsuId={selected.kitsu_id ?? null}
    rdLinked={config.rdLinked}
    autoplay={sourcesAutoplay}
    retomarSegundos={reanudarDesde}
    onClose={closeSources}
    onWeb={menuWeb}
  />
  {/key}
{:else if mode === "discover" && selected?.imdb_id}
  <div class="discover-mode">
    <button data-nav class="back-btn" onclick={stopDiscover} title="Volver (Esc / Backspace)">
      <span class="arrow">←</span> Volver
    </button>
    <button data-nav class="report-btn" onclick={reportUnavailableFromDiscover} title="Marcar como no disponible">
      ⚠ No funciona
    </button>
    <iframe
      src={discoverSrc}
      title="discover"
      referrerpolicy="no-referrer"
      sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-presentation"
      allow="autoplay; fullscreen; picture-in-picture"
    ></iframe>
  </div>
{:else if mode === "trailer" && trailerQr}
  <div class="discover-mode trailer-qr-mode">
    <div class="trailer-bar">
      <button data-nav class="bar-btn" onclick={stopDiscover} title="Volver (Esc / Backspace)">
        ← Volver
      </button>
      <div class="trailer-badge-inline">TRAILER</div>
    </div>
    <div class="trailer-qr">
      <h2>Trailer disponible en YouTube</h2>
      <p>Escanea el código con el celular, o ábrelo acá en el navegador.</p>
      <img src={trailerQr} alt="Código QR del trailer en YouTube" />
      <code>{trailerQrUrl}</code>
      <button data-nav class="trailer-open" onclick={abrirTrailerWeb}>
        🌐  Abrir en el navegador
      </button>
    </div>
  </div>
{:else}
  <main>
    <aside class="info" data-section="info">
      {#if !apiKey || showKey}
        <div class="key-box">
          <h3>TMDb API Key</h3>
          <p class="hint">
            Consíguela en <code>themoviedb.org/settings/api</code> (gratis).
          </p>
          <input
            type="password"
            bind:value={keyInput}
            placeholder="32 chars hex"
            onkeydown={(e) => e.key === "Enter" && saveKey()}
          />
          <button onclick={saveKey} disabled={!keyInput.trim()}>Guardar</button>

          {#if webKeyboardUrl}
            <div class="key-phone">
              <RemoteQr url={webKeyboardUrl} label="Teclado" size={110} />
              <div class="key-phone-txt">
                <strong>¿Difícil escribir con el control?</strong>
                <span>
                  Escanea este QR con el celular. Desde ahí puedes escribir,
                  pegar o escanear la key con la cámara: aparece sola en el campo
                  de arriba. Luego pulsa Guardar.
                </span>
              </div>
            </div>
          {:else}
            <p class="key-phone-wait">Preparando teclado por celular…</p>
          {/if}
        </div>
      {:else if detailLoading}
        <div class="empty">Cargando…</div>
      {:else if selected}
        <div class="detail">
          {#if selected.backdrop_path}
            <div class="backdrop" style:background-image="url({art(selected.backdrop_path, "w780", 780)})"></div>
          {/if}
          {#if selected.poster_path}
            <div class="poster-wrap">
              <img class="poster" src={art(selected.poster_path, "w342", 342)} alt="" onerror={onImgError} />
              {#if !disponibleSel}
                <span class="poster-stamp">NO DISPONIBLE</span>
              {/if}
            </div>
          {/if}
          <h2>{selected.title} {selected.year ? `(${selected.year})` : ""}</h2>
          <div class="meta">
            <span class="rating">★ {selected.vote_average.toFixed(1)}</span>
            {#if selected.runtime}<span>{selected.runtime} min</span>{/if}
            {#each selected.genres as g}<span class="genre">{g}</span>{/each}
          </div>
          <p class="overview">{selected.overview || "(sin sinopsis)"}</p>
          {#if claveSel}
            <div class="hist-row">
              <button
                data-nav
                class="hist-btn"
                class:on={favSel}
                onclick={() => void alternarFavorito(metaDe()!)}
              >
                {favSel ? "★ Quitar de favoritos" : "☆ Agregar a favoritos"}
              </button>
              {#if enCursoSel}
                {@const ec = enCursoSel}
                <button
                  data-nav
                  class="hist-btn"
                  onclick={() => void borrarProgreso(ec.imdb_id, ec.season, ec.episode)}
                  title="Borra el minuto guardado y lo saca de la lista"
                >
                  ✕ Quitar de seguir viendo{ec.season >= 0 ? ` (T${ec.season} E${ec.episode})` : ""}
                </button>
              {/if}
              {#if selected.media_type === "movie"}
                {@const yaVista = progressForSelected?.completed === 1}
                <button
                  data-nav
                  class="hist-btn"
                  class:on={yaVista}
                  onclick={() => void marcarVisto(metaDe()!, !yaVista)}
                  title={yaVista ? "Desmarcar" : "Marcar como vista sin reproducirla"}
                >
                  {yaVista ? "✓ Vista" : "✓ Marcar vista"}
                </button>
              {:else if estadoSel?.vistos}
                <span class="hist-info">{estadoSel.vistos} {estadoSel.vistos === 1 ? "capítulo visto" : "capítulos vistos"}</span>
              {/if}
            </div>
          {/if}
          {#if disponibleSel}
            {#if selected.media_type === "tv"}
              <!-- Series: la reproducción es por capítulo (lista de Temporadas
                   abajo). No hay "Descubrir" de título; solo Trailer. -->
              <div class="action-row">
                <button data-nav class="trailer-btn trailer-btn-row" onclick={watchTrailer}>🎬 Trailer</button>
              </div>
            {:else}
              {@const prog = progressForSelected}
              {@const pct = prog && prog.runtime_seconds && prog.runtime_seconds > 0
                ? Math.min(100, Math.round((prog.progress_real ?? prog.watched_seconds / prog.runtime_seconds) * 100))
                : null}
              {@const watchedMin = prog ? Math.floor(prog.watched_seconds / 60) : 0}
              {@const watchedSec = prog ? prog.watched_seconds % 60 : 0}
              {#if prog && prog.completed}
                <div class="watched-note">✓ Ya la viste</div>
              {:else if prog && prog.watched_seconds > 5 && pct != null}
                <div class="progress-bar"><div class="progress-fill" style:width="{pct}%"></div></div>
              {/if}
              <div class="action-row">
                {#if prog && prog.completed}
                  <button data-nav class="discover-btn discover-btn-row" onclick={goDescubrir}>▶ Descubrir de nuevo</button>
                {:else if prog && prog.watched_seconds > 5}
                  <button data-nav class="discover-btn discover-btn-row" onclick={goDescubrir} title="Opciones de reproducción">
                    ↻ Desde el inicio
                  </button>
                  <button data-nav class="discover-btn discover-btn-row" onclick={goDescubrir}>
                    ▶ Continuar {pct != null ? `(${pct}%)` : `(${watchedMin}m ${watchedSec}s)`}
                  </button>
                {:else}
                  <button data-nav class="discover-btn discover-btn-row" onclick={goDescubrir}>▶ Descubrir</button>
                {/if}
                <button data-nav class="trailer-btn trailer-btn-row" onclick={watchTrailer}>🎬 Trailer</button>
              </div>
            {/if}
          {:else}
            <div class="unavail-note">
              Este título no está disponible para reproducir.
            </div>
            <div class="action-row">
              <button data-nav class="discover-btn" onclick={watchTrailer}>
                🎬 Ver trailer
              </button>
              {#if selected.imdb_id}
                <button
                  data-nav
                  class="trailer-btn"
                  onclick={goDescubrir}
                  title="Intentar abrir aunque esté marcada como no disponible"
                >
                  ⚠ Intentar de todas formas
                </button>
              {/if}
            </div>
          {/if}
          {#if selected.media_type === "tv" && selected.seasons && selected.seasons.length}
            <div class="people-row seasons-row">
              <h4 class="people-title">Temporadas</h4>
              <div class="season-list">
                {#each selected.seasons as s (s.season_number)}
                  <button
                    data-nav
                    class="season-item"
                    class:active={seriesSeason === s.season_number}
                    onclick={() => openSeason(s.season_number)}
                  >
                    <span class="season-num">{seasonLabel(s.season_number)}</span>
                    <span class="season-count">{s.episode_count} cap.</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
          {#if selected.directors.length}
            <div class="people-row">
              <h4 class="people-title">{selected.media_type === "tv" ? "Creado por" : "Dirigido por"}</h4>
              <div class="people-strip">
                {#each selected.directors as p}
                  <button data-nav class="person-chip" onclick={() => openPerson(p.id)} title={p.name}>
                    {#if p.profile_path}
                      <img src={art(p.profile_path, "w185", 185)} alt={p.name} loading="lazy" onerror={onImgError} />
                    {:else}
                      <div class="person-noimg">{p.name.charAt(0)}</div>
                    {/if}
                    <span class="person-name">{p.name}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
          {#if selected.cast.length}
            <div class="people-row">
              <h4 class="people-title">Estelares</h4>
              <div class="people-strip">
                {#each selected.cast as p}
                  <button data-nav class="person-chip" onclick={() => openPerson(p.id)} title={p.name}>
                    {#if p.profile_path}
                      <img src={art(p.profile_path, "w185", 185)} alt={p.name} loading="lazy" onerror={onImgError} />
                    {:else}
                      <div class="person-noimg">{p.name.charAt(0)}</div>
                    {/if}
                    <span class="person-name">{p.name}</span>
                    {#if p.character}<span class="person-character">{p.character}</span>{/if}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
          {#if selected.images && selected.images.length}
            {@const scenes = selected.images}
            <div class="people-row">
              <h4 class="people-title">Escenas</h4>
              <div class="scenes-strip">
                {#each scenes.slice(0, 8) as path, i}
                  <button
                    data-nav
                    class="scene-shot"
                    style:background-image="url({img(`${IMG}/w500${path}`, 500)})"
                    onclick={() => openCarousel(scenes, i)}
                    title="Ver escena (Enter)"
                    aria-label={`Escena ${i + 1}`}
                  ></button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {:else}
        <div class="brand-empty">
          <h1 class="brand-name">Kütral</h1>
          <p class="brand-tag">elegí un título</p>
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

              <a class="card juegos-card" data-nav href="/juegos" title="Jugar">
                <div class="juegos-poster">
                  <div class="vera-title-poster">
                    <span class="juegos-marca">Juegos</span>
                    <em>retro</em>
                  </div>
                </div>
              </a>

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
                    {@const aw = awardsMap.get(itImdb)}
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
    <div class="modal-bg" onclick={closePerson} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") closePerson(); }} role="presentation">
      <div class="person-modal" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-modal="true" use:autofocusFirst>
        <button data-nav class="modal-close" onclick={closePerson} title="Cerrar (Esc)">✕</button>
        {#if personLoading}
          <div class="empty">Cargando…</div>
        {:else if personOpen}
          <div class="person-head">
            {#if personOpen.profile_path}
              <img class="person-photo" src={img(`${IMG}/w300${personOpen.profile_path}`, 300)} alt={personOpen.name} onerror={onImgError} />
            {:else}
              <div class="person-photo person-photo-empty">{personOpen.name.charAt(0)}</div>
            {/if}
            <div class="person-info">
              <h2>{personOpen.name}</h2>
              {#if personOpen.known_for_department}
                <p class="person-dept">{personOpen.known_for_department}</p>
              {/if}
              {#if personOpen.birthday}
                <p class="person-meta">
                  Nacimiento: {personOpen.birthday}
                  {#if personOpen.place_of_birth} · {personOpen.place_of_birth}{/if}
                </p>
              {/if}
              {#if personOpen.deathday}
                <p class="person-meta">Fallecimiento: {personOpen.deathday}</p>
              {/if}
            </div>
          </div>
          {#if personOpen.biography}
            <div class="person-bio">{personOpen.biography}</div>
          {/if}
          {#if personOpen.filmography.length}
            <h3 class="person-section">Filmografía destacada</h3>
            <div class="filmography-grid">
              {#each personOpen.filmography as f}
                <button
                  data-nav
                  class="film-card"
                  title={`${f.title} — ${f.roles.join(", ")}`}
                  onclick={() => openFilmographyItem(f.id, f.media_type)}
                >
                  <div class="film-poster-wrap">
                    {#if f.poster_path}
                      <img src={img(`${IMG}/w185${f.poster_path}`, 185)} alt={f.title} loading="lazy" onerror={onImgError} />
                    {:else}
                      <div class="film-noposter">sin poster</div>
                    {/if}
                    {#if f.roles.length}
                      <div class="film-pills">
                        {#each f.roles as r}
                          <span class="film-pill">{r}</span>
                        {/each}
                      </div>
                    {/if}
                  </div>
                  <div class="film-meta">
                    <span class="film-title">{f.title}</span>
                    <span class="film-sub">{f.year}</span>
                  </div>
                </button>
              {/each}
            </div>
          {/if}
          <p class="person-foot">Premios no disponibles en TMDb. Para verlos, abrí el perfil en IMDb.</p>
        {/if}
      </div>
    </div>
  {/if}

  {#if carousel}
    <div class="carousel-bg" onclick={closeCarousel} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") closeCarousel(); }} role="presentation">
      <button data-nav class="modal-close carousel-close" onclick={closeCarousel} title="Cerrar (Esc)">✕</button>
      <button class="carousel-arrow left" onclick={(e) => { e.stopPropagation(); carouselGo(-1); }} aria-label="Anterior">‹</button>
      <div class="carousel-img-wrap" role="presentation" onclick={(e) => e.stopPropagation()}>
        <img
          class="carousel-img"
          src={img(`${IMG}/original${carousel.images[carousel.idx]}`, 1280)}
          alt={`Escena ${carousel.idx + 1}`}
          onerror={onImgError}
        />
      </div>
      <button class="carousel-arrow right" onclick={(e) => { e.stopPropagation(); carouselGo(1); }} aria-label="Siguiente">›</button>
      <div class="carousel-counter">{carousel.idx + 1} / {carousel.images.length}</div>
    </div>
  {/if}

  {#if unavailable.open}
    <div class="modal-bg" onclick={() => (unavailable.open = false)} onkeydown={(e) => { if (e.key === "Escape" || e.key === "Backspace") unavailable.open = false; }} role="presentation">
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
        use:autofocusFirst
      >
        <h3>No disponible</h3>
        <p>
          {#if unavailable.reason === "404"}
            <strong>{selected?.title}</strong> aún no se encuentra en playimdb.com.
          {:else}
            <strong>{selected?.title}</strong> no tiene IMDb ID en TMDb, así que no podemos abrirlo.
          {/if}
        </p>
        <p class="modal-sub">¿Querés ver el trailer mientras tanto?</p>
        <div class="modal-actions">
          <button data-nav class="btn-secondary" onclick={() => (unavailable.open = false)}>
            Cancelar
          </button>
          <button data-nav class="btn-primary" onclick={watchTrailer}>
            🎬 Ver trailer
          </button>
        </div>
      </div>
    </div>
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

  .key-box { padding: 20px; }
  .key-box h3 { margin: 0 0 8px; }
  .key-box .hint { font-size: 12px; color: #888; margin: 0 0 12px; }
  .key-box code { background: #1a1a22; padding: 2px 5px; border-radius: 3px; font-size: 11px; }
  .key-box input { width: 100%; padding: 8px; background: #1a1a22; border: 1px solid #2a2a35; color: #eee; border-radius: 4px; margin-bottom: 8px; box-sizing: border-box; }
  .key-box button { width: 100%; padding: 8px; background: #f5c518; color: #000; border: 0; border-radius: 4px; font-weight: 700; cursor: pointer; }
  .key-box button:disabled { opacity: 0.4; cursor: not-allowed; }
  .key-phone {
    display: flex;
    gap: 14px;
    align-items: center;
    margin-top: 18px;
    padding-top: 16px;
    border-top: 1px solid #23232c;
  }
  .key-phone-txt { display: flex; flex-direction: column; gap: 4px; }
  .key-phone-txt strong { font-size: 13px; color: #f5c518; }
  .key-phone-txt span { font-size: 12px; color: #9a9aa4; line-height: 1.45; }
  .key-phone-wait { margin: 16px 0 0; font-size: 11.5px; color: #6e6e78; }

  .detail { position: relative; padding: 0 20px 20px; flex: 1; }
  .backdrop {
    position: absolute; top: 0; left: 0; right: 0; height: 240px;
    background-size: cover; background-position: center;
    opacity: 0.35;
    mask-image: linear-gradient(180deg, #000 0%, transparent 100%);
    -webkit-mask-image: linear-gradient(180deg, #000 0%, transparent 100%);
    z-index: 0;
  }
  .poster-wrap { position: relative; display: inline-block; margin-top: 60px; z-index: 1; }
  .poster { width: 180px; border-radius: 6px; box-shadow: 0 8px 24px rgba(0,0,0,0.6); display: block; }
  .poster-stamp {
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%) rotate(-18deg);
    background: rgba(220, 38, 38, 0.92);
    color: #fff;
    padding: 6px 18px;
    font-weight: 900; font-size: 14px;
    letter-spacing: 1.5px;
    border: 3px solid #fff;
    border-radius: 3px;
    box-shadow: 0 4px 14px rgba(0,0,0,0.7);
    text-transform: uppercase;
    white-space: nowrap;
    pointer-events: none;
  }
  .detail h2 { position: relative; margin: 14px 0 8px; font-size: 22px; z-index: 1; }
  .meta { display: flex; flex-wrap: wrap; gap: 8px; font-size: 12px; color: #aaa; margin-bottom: 12px; position: relative; z-index: 1; }
  .rating { color: #f5c518; font-weight: 700; }
  .genre { background: #1a1a22; padding: 2px 8px; border-radius: 999px; }
  .overview { color: #c0c0c8; font-size: 14px; line-height: 1.5; position: relative; z-index: 1; }
  .discover-btn {
    position: relative; z-index: 1;
    background: #f5c518; color: #000; border: 0;
    padding: 12px 24px; font-weight: 800; font-size: 16px;
    border-radius: 6px; cursor: pointer; width: 100%;
    margin-top: 12px;
    transition: transform 0.1s;
  }
  .discover-btn:hover:not(:disabled) { transform: scale(1.02); }
  .discover-btn:disabled { opacity: 0.6; cursor: wait; }
  .trailer-btn {
    position: relative; z-index: 1;
    background: transparent; color: #f5c518;
    border: 1.5px solid #f5c518;
    padding: 10px 20px; font-weight: 700; font-size: 14px;
    border-radius: 6px; cursor: pointer; width: 100%;
    margin-top: 8px;
    transition: background 0.12s, color 0.12s;
  }
  .trailer-btn:hover { background: #f5c518; color: #0d0d12; }
  .unavail-note {
    position: relative; z-index: 1;
    margin: 12px 0;
    padding: 10px 12px;
    background: rgba(220, 38, 38, 0.12);
    border: 1px solid rgba(220, 38, 38, 0.4);
    border-radius: 6px;
    color: #ffb4b4;
    font-size: 13px;
    text-align: center;
  }

  .action-row { display: flex; gap: 8px; position: relative; z-index: 1; margin-top: 12px; }
  .discover-btn-row { flex: 1; padding: 12px 16px; font-size: 15px; }
  .trailer-btn-row { width: auto; padding: 12px 18px; margin-top: 0 !important; font-size: 14px; }
  .people-row { position: relative; z-index: 1; margin-top: 18px; }
  .people-title {
    margin: 0 0 8px; font-size: 12px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 1px;
    color: #888;
  }
  .people-strip {
    display: flex; gap: 10px; flex-wrap: wrap;
  }
  .scenes-strip {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .scene-shot {
    width: 100%;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    border: 2px solid transparent;
    padding: 0;
    background-size: cover; background-position: center;
    background-color: #2a2a35;
    box-shadow: 0 4px 10px rgba(0,0,0,0.4);
    cursor: pointer;
    transition: transform 0.15s, border-color 0.15s;
  }
  .scene-shot:hover { transform: translateY(-2px); }
  .scene-shot:focus {
    outline: none;
    border-color: #f5c518;
    transform: translateY(-2px) scale(1.03);
    box-shadow: 0 6px 18px rgba(0,0,0,0.55), 0 0 0 2px rgba(245,197,24,0.4);
  }

  /* --- Carrusel de escenas --- */
  .carousel-bg {
    position: fixed; inset: 0; z-index: 200;
    background: rgba(0,0,0,0.92);
    display: flex; align-items: center; justify-content: center;
    gap: 12px;
  }
  .carousel-img {
    max-width: 88vw; max-height: 82vh;
    border-radius: 10px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.8);
    object-fit: contain;
  }
  .carousel-close {
    position: absolute; top: 24px; right: 28px;
    z-index: 2;
  }
  .carousel-arrow {
    flex: 0 0 auto;
    width: 72px; height: 72px;
    border-radius: 50%;
    border: none;
    background: rgba(255,255,255,0.1);
    color: #fff; font-size: 48px; line-height: 1;
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s;
  }
  .carousel-arrow:hover { background: rgba(255,255,255,0.22); }
  .carousel-counter {
    position: absolute; bottom: 28px; left: 50%;
    transform: translateX(-50%);
    color: #ddd; font-size: 20px;
    background: rgba(0,0,0,0.5);
    padding: 6px 18px; border-radius: 999px;
  }
  .person-chip {
    flex: 0 0 80px; width: 80px;
    background: transparent; border: 0; padding: 0;
    color: inherit; cursor: pointer;
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
    transition: transform 0.15s;
  }
  .person-chip:hover { transform: translateY(-2px); }
  .person-chip:focus { outline: 3px solid #f5c518; outline-offset: 2px; border-radius: 6px; }
  .person-chip img, .person-noimg {
    width: 64px; height: 64px;
    border-radius: 50%; object-fit: cover;
    background: #2a2a35;
    box-shadow: 0 4px 10px rgba(0,0,0,0.4);
  }
  .person-noimg {
    display: flex; align-items: center; justify-content: center;
    font-size: 22px; font-weight: 700; color: #888;
  }
  .person-name {
    font-size: 11px; font-weight: 600; line-height: 1.2;
    text-align: center; color: #ddd;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2; line-clamp: 2;
    -webkit-box-orient: vertical;
    max-width: 80px;
  }
  .person-character {
    font-size: 10px; color: #777; line-height: 1.1;
    text-align: center;
    overflow: hidden; text-overflow: ellipsis;
    max-width: 80px; white-space: nowrap;
  }

  /* Person modal */
  .person-modal {
    position: relative;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 12px;
    width: 90%; max-width: 720px; max-height: 85vh;
    overflow-y: auto;
    padding: 28px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.7);
  }
  .modal-close {
    position: absolute; top: 12px; right: 12px;
    width: 32px; height: 32px; border-radius: 50%;
    background: #1a1a22; border: 1px solid #2a2a35;
    color: #aaa; font-size: 14px; cursor: pointer;
    transition: all 0.12s;
  }
  .modal-close:hover { background: #f5c518; color: #000; border-color: #f5c518; }
  .person-head { display: flex; gap: 18px; align-items: flex-start; margin-bottom: 16px; }
  .person-photo {
    width: 120px; height: 180px; object-fit: cover;
    border-radius: 8px; flex: 0 0 auto;
    box-shadow: 0 8px 20px rgba(0,0,0,0.5);
  }
  .person-photo-empty {
    background: #2a2a35;
    display: flex; align-items: center; justify-content: center;
    font-size: 50px; color: #555; font-weight: 700;
  }
  .person-info { flex: 1; min-width: 0; }
  .person-info h2 { margin: 0 0 6px; color: #f5c518; font-size: 24px; }
  .person-dept { margin: 0 0 8px; color: #aaa; font-size: 13px; }
  .person-meta { margin: 2px 0; color: #ccc; font-size: 13px; }
  .person-bio {
    color: #ccc; font-size: 14px; line-height: 1.6;
    white-space: pre-wrap; margin: 0 0 16px;
  }
  .person-section { margin: 16px 0 12px; color: #f5c518; font-size: 14px; text-transform: uppercase; letter-spacing: 1.5px; }
  .filmography-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
    gap: 12px;
  }
  .film-card {
    background: #1a1a22; border: 0; border-radius: 6px; overflow: hidden;
    color: inherit; text-align: left;
    padding: 0; cursor: pointer;
    display: flex; flex-direction: column;
    transition: transform 0.12s, box-shadow 0.12s;
  }
  .film-card:hover { transform: translateY(-2px); box-shadow: 0 6px 14px rgba(0,0,0,0.5); }
  .film-card:focus, .film-card:focus-visible {
    outline: 3px solid #f5c518; outline-offset: 2px;
    transform: translateY(-2px);
  }
  .film-poster-wrap { position: relative; }
  .film-card img, .film-noposter {
    width: 100%; aspect-ratio: 2/3; object-fit: cover;
    background: #222; display: block;
  }
  .film-noposter {
    display: flex; align-items: center; justify-content: center;
    color: #555; font-size: 11px;
  }
  .film-pills {
    position: absolute; left: 4px; right: 4px; bottom: 4px;
    display: flex; flex-wrap: wrap; gap: 3px;
    justify-content: flex-start;
  }
  .film-pill {
    background: rgba(245, 197, 24, 0.92);
    color: #0d0d12;
    padding: 2px 7px;
    border-radius: 999px;
    font-size: 9px;
    font-weight: 700;
    line-height: 1.3;
    max-width: 100%;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    box-shadow: 0 2px 6px rgba(0,0,0,0.5);
  }
  .film-meta { padding: 6px 8px; }
  .film-title { display: block; font-size: 12px; font-weight: 600; line-height: 1.2;
    overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .film-sub { display: block; font-size: 10px; color: #888; margin-top: 2px; }
  .person-foot { margin: 18px 0 0; color: #666; font-size: 11px; text-align: center; }
  .progress-bar {
    position: relative; z-index: 1;
    height: 6px; background: #1f1f28; border-radius: 3px;
    overflow: hidden; margin: 4px 0 8px;
  }
  .progress-fill { height: 100%; background: linear-gradient(90deg, #f5c518, #ff9b00); transition: width 0.3s; }
  .watched-note {
    position: relative; z-index: 1;
    background: rgba(82, 181, 96, 0.15);
    border: 1px solid rgba(82, 181, 96, 0.4);
    color: #b6e9bf;
    padding: 6px 10px; border-radius: 6px;
    font-size: 12px; text-align: center;
    margin: 8px 0;
  }

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
  .juegos-card {
    text-decoration: none;
  }
  .juegos-poster {
    aspect-ratio: 2 / 1;
    background:
      radial-gradient(ellipse at 50% 35%, #1a0f3d 0%, #0a0820 65%, #050310 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .juegos-poster::before {
    content: ""; position: absolute; inset: 0;
    background:
      radial-gradient(circle at 30% 20%, rgba(125, 79, 255, 0.16), transparent 55%),
      radial-gradient(circle at 70% 80%, rgba(43, 108, 255, 0.16), transparent 55%);
    pointer-events: none;
  }
  .juegos-marca {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 800;
    background: linear-gradient(90deg, #7d4fff, #9c7bff, #2b6cff, #9c7bff, #7d4fff);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
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
  .hist-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 10px 0 4px;
  }
  .hist-btn {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #d8d8e0;
    border-radius: 8px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .hist-btn:hover,
  .hist-btn:focus-visible {
    border-color: #f3a951;
    color: #fff;
    outline: none;
  }
  .hist-btn.on {
    background: rgba(47, 158, 68, 0.18);
    border-color: #2f9e44;
    color: #7ee094;
  }
  .hist-info {
    font-size: 12px;
    color: #9a9aa4;
  }
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
  .season-list {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .season-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: rgba(20, 20, 28, 0.6);
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 10px;
    padding: 10px 14px;
    cursor: pointer;
    color: #e6e6ec;
    text-align: left;
  }
  .season-item:focus-visible,
  .season-item:hover {
    outline: none;
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
  }
  .season-item.active {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.22);
    color: #fff;
  }
  .season-num { font-size: 14px; font-weight: 600; }
  .season-count { font-size: 12px; color: #9a9aa4; }

  /* PLAY mode */
  .discover-mode { position: fixed; inset: 0; background: #000; z-index: 1500; }
  .discover-mode iframe { width: 100%; height: 100%; border: 0; background: #000; }
  .back-btn {
    position: absolute; top: 14px; right: 14px; z-index: 1100;
    display: inline-flex; align-items: center; gap: 8px;
    padding: 10px 18px 10px 14px;
    background: rgba(0,0,0,0.75);
    color: #fff;
    border: 1px solid #333;
    border-radius: 999px;
    font-size: 14px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.15s, color 0.15s, transform 0.1s;
  }
  .back-btn .arrow { font-size: 18px; line-height: 1; }
  .back-btn:hover { background: #f5c518; color: #000; border-color: #f5c518; }
  .back-btn:active { transform: scale(0.97); }
  .report-btn {
    position: absolute; top: 14px; left: 14px; z-index: 1100;
    padding: 10px 16px;
    background: rgba(20,20,28,0.75);
    color: #ffb4b4;
    border: 1px solid rgba(220,38,38,0.6);
    border-radius: 999px;
    font-size: 13px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.12s, color 0.12s;
  }
  .report-btn:hover { background: rgba(220,38,38,0.9); color: #fff; }

  /* Vista unavailable */
  .unavail-screen {
    position: fixed; inset: 0; z-index: 1500;
    background: #0d0d12;
    display: flex; align-items: center; justify-content: center;
    padding: 40px;
  }
  .unavail-card {
    max-width: 480px; text-align: center;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 12px;
    padding: 36px 32px;
    box-shadow: 0 20px 60px rgba(0,0,0,0.6);
  }
  .unavail-poster {
    width: 140px; border-radius: 8px;
    margin-bottom: 20px;
    box-shadow: 0 8px 20px rgba(0,0,0,0.5);
    filter: grayscale(0.6);
  }
  .unavail-card h2 {
    margin: 0 0 12px;
    color: #f5c518; font-size: 24px;
  }
  .unavail-card p { color: #ddd; font-size: 15px; line-height: 1.5; margin: 0 0 10px; }
  .unavail-sub { color: #888 !important; font-size: 13px !important; margin-bottom: 22px !important; }
  .unavail-actions {
    display: flex; gap: 10px; justify-content: center; flex-wrap: wrap;
    margin-top: 22px;
  }
  .unavail-actions button {
    padding: 10px 18px; border-radius: 6px;
    font-size: 14px; font-weight: 600; cursor: pointer;
  }
  .trailer-bar {
    position: absolute; top: 0; left: 0; right: 0; z-index: 1100;
    display: flex; align-items: center; gap: 10px;
    padding: 10px 14px;
    background: linear-gradient(180deg, rgba(0,0,0,0.85) 0%, rgba(0,0,0,0) 100%);
    pointer-events: none;
  }
  .trailer-bar > * { pointer-events: auto; }
  /* Último recurso del trailer: QR para verlo en el celular. */
  .trailer-qr-mode { display: grid; place-items: center; }
  .trailer-qr {
    display: flex; flex-direction: column; align-items: center; gap: 18px;
    padding: 40px; text-align: center;
  }
  .trailer-qr h2 { margin: 0; font-size: 28px; color: #fff; }
  .trailer-qr p { margin: 0; color: #9a9aa8; font-size: 15px; }
  .trailer-qr img {
    width: 300px; height: 300px;
    background: #fff; padding: 12px; border-radius: 12px;
  }
  .trailer-qr code { color: #f5c518; font-size: 14px; letter-spacing: 0.5px; }
  .trailer-open {
    background: #f5c518;
    color: #0d0d12;
    border: 2px solid transparent;
    padding: 12px 26px;
    border-radius: 999px;
    font-size: 15px;
    font-weight: 800;
    cursor: pointer;
  }
  .trailer-open:hover,
  .trailer-open:focus,
  .trailer-open:focus-visible {
    outline: none;
    border-color: #fff;
    box-shadow: 0 0 0 4px rgba(245, 197, 24, 0.28);
  }

  .trailer-badge-inline {
    padding: 5px 12px;
    background: #f5c518; color: #0d0d12;
    font-weight: 900; font-size: 11px; letter-spacing: 2px;
    border-radius: 4px;
  }
  .bar-btn {
    background: rgba(20, 20, 28, 0.85);
    color: #fff;
    border: 1px solid #333;
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 13px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }
  .bar-btn:hover { background: #f5c518; color: #0d0d12; border-color: #f5c518; }


  /* Modal */
  .modal-bg {
    position: fixed; inset: 0; z-index: 800;
    background: rgba(0,0,0,0.7);
    display: flex; align-items: center; justify-content: center;
    backdrop-filter: blur(4px);
  }
  .modal {
    background: #15151c; border: 1px solid #2a2a35;
    border-radius: 10px; padding: 28px;
    max-width: 460px; width: 90%;
    box-shadow: 0 20px 60px rgba(0,0,0,0.7);
  }
  .modal h3 {
    margin: 0 0 14px;
    color: #f5c518; font-size: 22px;
  }
  .modal p { margin: 0 0 10px; color: #ddd; font-size: 15px; line-height: 1.5; }
  .modal .modal-sub { color: #888; font-size: 14px; margin-bottom: 22px; }
  .modal-actions { display: flex; gap: 10px; justify-content: flex-end; }
  .modal-actions button {
    padding: 10px 20px; border-radius: 6px;
    font-size: 14px; font-weight: 600; cursor: pointer;
    border: 1px solid transparent;
  }
  .btn-secondary {
    background: transparent; color: #aaa; border-color: #2a2a35;
  }
  .btn-secondary:hover { color: #fff; border-color: #444; }
  .btn-primary {
    background: #f5c518; color: #0d0d12; border: 0;
  }
  .btn-primary:hover { transform: scale(1.03); }

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
