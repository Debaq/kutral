<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Database from "@tauri-apps/plugin-sql";
  import { onMount } from "svelte";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { setPlaying } from "$lib/playerState.svelte";

  type Progress = {
    imdb_id: string;
    tmdb_id: number;
    media_type: string;
    title: string;
    poster_path: string | null;
    watched_seconds: number;
    runtime_seconds: number | null;
    progress_real: number | null;
    completed: number;
    last_watched: number;
  };

  let db: Awaited<ReturnType<typeof Database.load>> | null = null;
  let progressForSelected = $state<Progress | null>(null);
  let discoverStartTs = 0;
  let unavailableSet = $state<Set<string>>(new Set());
  let personOpen = $state<PersonInfo | null>(null);
  let personLoading = $state(false);

  type TrailerSource = "nocookie" | "youtube" | "invidious" | "piped";
  const TRAILER_SOURCES: { id: TrailerSource; label: string; build: (k: string) => string }[] = [
    { id: "nocookie",  label: "YouTube (sin cookies)", build: (k) => `https://www.youtube-nocookie.com/embed/${k}?autoplay=1&rel=0&modestbranding=1` },
    { id: "youtube",   label: "YouTube clásico",       build: (k) => `https://www.youtube.com/embed/${k}?autoplay=1&rel=0&modestbranding=1` },
    { id: "invidious", label: "Invidious (yewtu.be)",  build: (k) => `https://yewtu.be/embed/${k}?autoplay=1` },
    { id: "piped",     label: "Piped",                 build: (k) => `https://piped.video/embed/${k}?autoplay=1` },
  ];
  let trailerSource = $state<TrailerSource>("nocookie");
  let trailerSourceOpen = $state(false);

  const BACK_KEYS = ["Escape", "Backspace"];
  const ARROW_KEYS = ["Up", "Down", "Left", "Right"];

  async function setFs(on: boolean) {
    try {
      await getCurrentWindow().setFullscreen(on);
    } catch (e) {
      console.warn("fullscreen falló", e);
    }
  }

  async function registerBackShortcuts(includeArrows = false) {
    for (const k of BACK_KEYS) {
      try {
        await register(k, (ev) => {
          if (ev.state === "Pressed") stopDiscover();
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
    genres: string[];
    directors: PersonMini[];
    cast: PersonMini[];
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

  const IMG = "https://image.tmdb.org/t/p";

  // Cache local WebP — guarda URL→file path una vez bajado
  let cachedImgs = $state<Map<string, string>>(new Map());
  const cacheInflight = new Set<string>();
  async function ensureCached(url: string, maxW: number) {
    const key = `${url}::${maxW}`;
    if (cachedImgs.has(key) || cacheInflight.has(key)) return;
    cacheInflight.add(key);
    try {
      const path = await invoke<string>("cache_image", { url, maxW });
      const m = new Map(cachedImgs);
      m.set(key, convertFileSrc(path));
      cachedImgs = m;
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
    if (cached) return cached;
    ensureCached(url, maxW);
    return url;
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
  function currentMediaTypeForSort(): "movie" | "tv" { return tabToMediaType(tab); }
  let sortId = $state<string>("popular");
  let sortOpen = $state(false);

  let genres = $state<{ id: number; name: string }[]>([]);
  let selectedGenres = $state<Set<number>>(new Set());

  let items = $state<ListItem[]>([]);
  let focusedIdx = $state(0);
  let cardEls: (HTMLButtonElement | null)[] = $state([]);
  let listLoading = $state(false);
  let listError = $state("");
  let initialFocusPending = $state(true);

  $effect(() => {
    if (!initialFocusPending) return;
    if (listLoading) return;
    if (showKey) return;
    const el = cardEls[0];
    if (!el) return;
    initialFocusPending = false;
    el.focus({ preventScroll: false });
    focusedIdx = 0;
  });

  $effect(() => {
    setPlaying(mode === "discover" || mode === "trailer");
  });

  // Cache de status por id: undefined=no chequeado, "checking", "ok"=video disponible,
  // "trailer"=solo trailer (badge), "none"=ocultar
  type CardStatus = "checking" | "ok" | "trailer" | "none";
  let statusMap = $state<Map<number, CardStatus>>(new Map());
  let imdbIdMap = $state<Map<number, string>>(new Map());
  const statusInflight = new Set<number>();
  let statusObserver: IntersectionObserver | null = null;

  let sentinelEl: HTMLDivElement | null = null;
  let observer: IntersectionObserver | null = null;

  let selected = $state<Detail | null>(null);
  let detailLoading = $state(false);

  let mode = $state<"browse" | "discover" | "trailer" | "unavailable">("browse");
  let checkingDiscover = $state(false);
  let trailerKey = $state<string>("");
  let unavailable = $state<{ open: boolean; reason: "404" | "no_imdb"; checking: boolean }>({
    open: false,
    reason: "404",
    checking: false,
  });
  type Video = { key: string; name: string; site: string; type: string; official: boolean };

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(async () => {
    // Registrar los atajos del home en la ayuda global.
    ayuda.set("inicio", [
      { tecla: "← → ↑ ↓", desc: "Navegar entre cards" },
      { tecla: "Enter", desc: "Abrir / reproducir" },
      { tecla: "Esc · Backspace", desc: "Volver / cerrar" },
      { tecla: "I", desc: "Ayuda" },
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

    const k = localStorage.getItem("tmdb_key") || "";
    apiKey = k;
    keyInput = k;
    showKey = !k;
    const ts = (localStorage.getItem("trailer_src") as TrailerSource) || "nocookie";
    if (TRAILER_SOURCES.some((s) => s.id === ts)) trailerSource = ts;
    if (k) {
      loadGenres();
      resetAndLoad();
    }
  });

  function onIframeMessage(e: MessageEvent) {
    // Log todo para diagnóstico
    console.log("[postMessage]", e.origin, e.data);
    if (typeof e.data !== "object" || !e.data) return;
    // Aceptamos { type: 'progress'|'time', t: number, d?: number }
    const t = Number(e.data.t ?? e.data.currentTime ?? e.data.position);
    const d = Number(e.data.d ?? e.data.duration);
    if (!isFinite(t) || t <= 0) return;
    if (!selected?.imdb_id) return;
    saveProgress({
      watched_seconds: Math.floor(t),
      runtime_seconds: isFinite(d) && d > 0 ? Math.floor(d) : null,
      progress_real: isFinite(d) && d > 0 ? t / d : null,
    });
  }

  async function getProgress(imdb_id: string): Promise<Progress | null> {
    if (!db) return null;
    try {
      const rows = await db.select<Progress[]>(
        "SELECT * FROM watch_history WHERE imdb_id = $1",
        [imdb_id]
      );
      return rows[0] || null;
    } catch (e) {
      console.warn("[db] get error", e);
      return null;
    }
  }

  async function saveProgress(partial: {
    watched_seconds: number;
    runtime_seconds?: number | null;
    progress_real?: number | null;
  }) {
    if (!db || !selected || !selected.imdb_id) return;
    const runtime_s = partial.runtime_seconds ?? (selected.runtime ? selected.runtime * 60 : null);
    const real = partial.progress_real ?? null;
    const completed = real != null && real >= 0.9
      ? 1
      : (runtime_s && partial.watched_seconds >= runtime_s * 0.9 ? 1 : 0);
    try {
      await db.execute(
        `INSERT INTO watch_history
          (imdb_id, tmdb_id, media_type, title, poster_path,
           watched_seconds, runtime_seconds, progress_real, completed, last_watched)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT(imdb_id) DO UPDATE SET
           watched_seconds = excluded.watched_seconds,
           runtime_seconds = COALESCE(excluded.runtime_seconds, watch_history.runtime_seconds),
           progress_real   = COALESCE(excluded.progress_real,   watch_history.progress_real),
           completed       = excluded.completed,
           last_watched    = excluded.last_watched`,
        [
          selected.imdb_id,
          selected.id,
          selected.media_type,
          selected.title,
          selected.poster_path ?? null,
          partial.watched_seconds,
          runtime_s,
          real,
          completed,
          Date.now(),
        ]
      );
      progressForSelected = await getProgress(selected.imdb_id);
      console.log("[db] guardado", selected.imdb_id, "watched=", partial.watched_seconds, "→", progressForSelected);
    } catch (e) {
      console.error("[db] save error", e);
    }
  }

  async function clearProgressFor(imdb_id: string) {
    if (!db) return;
    try {
      await db.execute("DELETE FROM watch_history WHERE imdb_id = $1", [imdb_id]);
      if (selected?.imdb_id === imdb_id) progressForSelected = null;
    } catch (e) {
      console.warn("[db] delete error", e);
    }
  }

  async function loadUnavailableSet() {
    if (!db) return;
    try {
      const rows = await db.select<{ imdb_id: string }[]>("SELECT imdb_id FROM unavailable_items");
      unavailableSet = new Set(rows.map((r) => r.imdb_id));
    } catch (e) {
      console.warn("[db] loadUnavailable error", e);
    }
  }

  async function markUnavailable(imdb_id: string) {
    if (!db) return;
    try {
      await db.execute(
        `INSERT OR REPLACE INTO unavailable_items (imdb_id, detected_at) VALUES ($1, $2)`,
        [imdb_id, Date.now()]
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

  async function loadProgressForSelected() {
    if (!selected?.imdb_id) {
      progressForSelected = null;
      return;
    }
    progressForSelected = await getProgress(selected.imdb_id);
    console.log("[db] load para", selected.imdb_id, "→", progressForSelected);
  }

  function setTrailerSource(s: TrailerSource) {
    trailerSource = s;
    localStorage.setItem("trailer_src", s);
    trailerSourceOpen = false;
  }

  function trailerUrl(key: string): string {
    return (TRAILER_SOURCES.find((s) => s.id === trailerSource) || TRAILER_SOURCES[0]).build(key);
  }

  async function openTrailerExternal() {
    if (!trailerKey) return;
    try {
      await openUrl(`https://www.youtube.com/watch?v=${trailerKey}`);
    } catch (e) {
      console.warn("openUrl falló", e);
    }
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
    selectedGenres = new Set();
    statusMap = new Map();
    loadGenres();
    resetAndLoad();
  }

  function changeSort(id: string) {
    sortId = id;
    resetAndLoad();
  }

  function toggleGenre(id: number) {
    const s = new Set(selectedGenres);
    if (s.has(id)) s.delete(id); else s.add(id);
    selectedGenres = s;
    resetAndLoad();
  }

  function clearGenres() {
    if (selectedGenres.size === 0) return;
    selectedGenres = new Set();
    resetAndLoad();
  }

  function resetAndLoad() {
    page = 1;
    items = [];
    hasMore = true;
    focusedIdx = 0;
    cardEls = [];
    loadList(false);
  }

  async function loadGenres() {
    if (!apiKey) return;
    try {
      const list = await invoke<{ id: number; name: string }[]>("tmdb_genres", {
        mediaType: tabToMediaType(tab),
        apiKey,
      });
      // En anime, sacar "Animación" del listado (ya está forzado)
      genres = tab === "anime" ? list.filter((g) => g.id !== 16) : list;
    } catch (e) {
      console.warn("genres falló", e);
      genres = [];
    }
  }

  function currentSortBy(): string {
    const s = SORTS.find((x) => x.id === sortId) || SORTS[0];
    return tabToMediaType(tab) === "movie" ? s.movie : s.tv;
  }

  async function loadList(append: boolean) {
    if (!apiKey) return;
    if (append) loadingMore = true; else listLoading = true;
    listError = "";
    try {
      const isSearch = !!debouncedQ;
      const mt = tabToMediaType(tab);
      const ext = tabExtras(tab);
      const resp: ListResp = isSearch
        ? await invoke("tmdb_search", { mediaType: mt, query: debouncedQ, page, apiKey })
        : await invoke("tmdb_discover", {
            mediaType: mt,
            page,
            apiKey,
            sortBy: currentSortBy(),
            withGenres: Array.from(selectedGenres).join(","),
            originCountry: ext.originCountry,
            forceGenres: ext.forceGenres,
          });
      totalPages = Math.min(resp.total_pages, 500);
      const newItems = resp.results;
      if (append) {
        items = [...items, ...newItems];
        cardEls = [...cardEls, ...new Array(newItems.length).fill(null)];
      } else {
        items = newItems;
        cardEls = new Array(newItems.length).fill(null);
      }
      hasMore = page < totalPages && newItems.length > 0;
    } catch (e) {
      listError = String(e);
      if (!append) items = [];
      hasMore = false;
    } finally {
      listLoading = false;
      loadingMore = false;
    }
  }

  async function loadMore() {
    if (!hasMore || loadingMore || listLoading) return;
    page += 1;
    await loadList(true);
  }

  async function checkItemStatus(id: number) {
    if (statusMap.has(id) || statusInflight.has(id)) return;
    statusInflight.add(id);
    const next = new Map(statusMap);
    next.set(id, "checking");
    statusMap = next;
    try {
      const s: { id: number; has_imdb: boolean; imdb_id: string | null; has_trailer: boolean } =
        await invoke("item_status", { mediaType: tabToMediaType(tab), id, apiKey });
      const status: CardStatus = s.has_imdb ? "ok" : s.has_trailer ? "trailer" : "none";
      const m = new Map(statusMap);
      m.set(id, status);
      statusMap = m;
      if (s.imdb_id) {
        const im = new Map(imdbIdMap);
        im.set(id, s.imdb_id);
        imdbIdMap = im;
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
    closePerson();
    try {
      const d = await invoke<Detail>("tmdb_detail", { mediaType, id, apiKey });
      selected = d;
      loadProgressForSelected();
    } catch (e) {
      console.warn("[tmdb_detail filmography] error", e);
    }
  }

  async function openPerson(id: number) {
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

  function closePerson() {
    personOpen = null;
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
    observer = new IntersectionObserver(
      async (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            await loadMore();
            // Re-armar dispatch: si tras append el sentinel sigue visible, vuelve a disparar
            if (observer && sentinelEl) {
              observer.unobserve(sentinelEl);
              observer.observe(sentinelEl);
            }
          }
        }
      },
      { rootMargin: "400px" }
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
    if (!apiKey) return null;
    if (pickPromise && pickPromise.id === it.id) return pickPromise.promise;
    detailLoading = true;
    const p = (async () => {
      try {
        const d = await invoke<Detail>("tmdb_detail", { mediaType: tabToMediaType(tab), id: it.id, apiKey });
        selected = d;
        loadProgressForSelected();
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
      if (d?.imdb_id) startDiscover();
    } catch { /* error ya seteado en pick */ }
  }

  function pickOrTrailer(it: ListItem, st: CardStatus | undefined) {
    pick(it);
  }

  async function pickAndDiscoverOrTrailer(it: ListItem, st: CardStatus | undefined) {
    if (st === "trailer") {
      const d = await pick(it);
      if (d) await watchTrailer();
      return;
    }
    await pickAndDiscover(it);
  }

  function discoverUrl(imdb_id: string) {
    return `https://www.playimdb.com/es/title/${imdb_id}/?ref_=${REF}`;
  }

  async function startDiscover() {
    if (!selected) return;
    if (!selected.imdb_id) {
      unavailable = { open: true, reason: "no_imdb", checking: false };
      return;
    }
    // Si ya está marcada como no disponible → vista directo
    if (unavailableSet.has(selected.imdb_id)) {
      mode = "unavailable";
      setTimeout(() => document.querySelector<HTMLElement>(".unavail-back")?.focus(), 50);
      return;
    }
    // Pre-check rápido: solo bloquea con evidencia clara (404/410 server-side)
    checkingDiscover = true;
    let blocked = false;
    try {
      const s: { status: number; available: boolean; reason: string } =
        await invoke("check_url", { url: discoverUrl(selected.imdb_id) });
      console.log("[check_url pre-discover]", s);
      if (!s.available) {
        blocked = true;
        if (selected.imdb_id) markUnavailable(selected.imdb_id);
      }
    } catch (e) {
      console.warn("[check_url pre-discover] error", e);
    }
    checkingDiscover = false;
    if (blocked) {
      mode = "unavailable";
      setTimeout(() => document.querySelector<HTMLElement>(".unavail-back")?.focus(), 50);
      return;
    }
    discoverStartTs = Date.now();
    mode = "discover";
    setFs(true);
    registerBackShortcuts(false);
    setTimeout(() => document.querySelector<HTMLElement>(".back-btn")?.focus(), 50);
    // Auto-inspect en background (stderr de tauri dev)
    if (selected.imdb_id) {
      invoke<string>("inspect_player", { imdbId: selected.imdb_id })
        .then(() => console.log("[inspect_player] dump enviado a stderr"))
        .catch((e) => console.warn("[inspect_player] error", e));
    }
  }

  function testDiag(e: MouseEvent) {
    (e.currentTarget as HTMLElement).blur();
    invoke("sim_diagnostic").then((r) => console.log("[diag]", r));
  }
  function testSpace(e: MouseEvent) {
    (e.currentTarget as HTMLElement).blur();
    const iframe = document.querySelector(".discover-mode iframe") as HTMLIFrameElement | null;
    iframe?.focus();
    setTimeout(() => invoke("sim_key", { key: "space" }), 100);
  }
  function testWake(e: MouseEvent) {
    (e.currentTarget as HTMLElement).blur();
    invoke("sim_mouse_wake");
  }

  function runInspectPlayer() {
    if (!selected?.imdb_id) return;
    invoke<string>("inspect_player", { imdbId: selected.imdb_id })
      .then(() => console.log("[inspect_player] dump enviado a stderr"))
      .catch((e) => console.warn("[inspect_player] error", e));
  }

  function reportUnavailableFromDiscover() {
    if (!selected?.imdb_id) return;
    markUnavailable(selected.imdb_id).then(() => {
      mode = "unavailable";
      setFs(false);
      unregisterBackShortcuts();
      setTimeout(() => document.querySelector<HTMLElement>(".unavail-back")?.focus(), 50);
    });
  }

  function restartDiscover() {
    // "Volver a empezar": resetea progreso wall-clock y abre. No salta a 0 en el iframe
    // (no podemos cross-origin) pero el contador vuelve a 0
    if (!selected?.imdb_id) return;
    clearProgressFor(selected.imdb_id).then(() => startDiscover());
  }

  async function stopDiscover() {
    const wasDiscover = mode === "discover";
    if (mode === "unavailable") {
      mode = "browse";
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
    trailerKey = "";
    setFs(false);
    unregisterBackShortcuts();
  }

  let trailerMsg = $state("");

  async function watchTrailer() {
    if (!selected) return;
    unavailable.open = false;
    trailerMsg = "";
    try {
      const vids = await invoke<Video[]>("tmdb_videos", {
        mediaType: selected.media_type,
        id: selected.id,
        apiKey,
      });
      console.log("[tmdb_videos]", selected.media_type, selected.id, "→", vids);
      if (!vids.length) {
        trailerMsg = `Sin trailer disponible para "${selected.title}" en TMDb.`;
        setTimeout(() => (trailerMsg = ""), 4000);
        return;
      }
      trailerKey = vids[0].key;
      mode = "trailer";
      setFs(true);
      registerBackShortcuts(true);
      setTimeout(() => document.querySelector<HTMLElement>(".trailer-bar .bar-btn")?.focus(), 50);
    } catch (e) {
      console.warn("[tmdb_videos] error", e);
      trailerMsg = `Error trayendo trailer: ${e}`;
      setTimeout(() => (trailerMsg = ""), 5000);
    }
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
      const dist = primary + secondary * 1.4;
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
    // Pass 2: cross-section solo si la sección lo permite en esa dirección
    if (!best) {
      // info SOLO puede salir lateralmente; vertical queda cautivo
      const allowCross =
        curSection !== "info" || dir === "right" || dir === "left";
      if (allowCross) {
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

    if (inInput) {
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
          pickAndDiscoverOrTrailer(it, st);
        }
        // En otros botones, Enter/Space disparan onclick por default
        break;
      }
      case "Backspace":
        if (sortOpen) { e.preventDefault(); sortOpen = false; }
        break;
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    // Si la ayuda global está abierta, no procesar otras teclas.
    if (ayuda.visible) return;
    if (mode === "discover") {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        stopDiscover();
        return;
      }
      // Wake controles del player ante cualquier tecla (línea de tiempo visible)
      invoke("sim_mouse_wake").catch(() => {});
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
        <img class="unavail-poster" src={`${IMG}/w342${selected.poster_path}`} alt="" />
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
            }
          }}>
            Marcar disponible
          </button>
        {/if}
      </div>
    </div>
  </div>
{:else if mode === "discover" && selected?.imdb_id}
  <div class="discover-mode">
    <button data-nav class="back-btn" onclick={stopDiscover} title="Volver (Esc / Backspace)">
      <span class="arrow">←</span> Volver
    </button>
    <button data-nav class="report-btn" onclick={reportUnavailableFromDiscover} title="Marcar como no disponible">
      ⚠ No funciona
    </button>
    <button data-nav class="inspect-btn" onclick={runInspectPlayer} title="Volcar info del player a stderr">
      🔍 Inspect
    </button>
    <button data-nav class="test-btn" onclick={testDiag} title="Diagnóstico enigo">
      🧪 Diag
    </button>
    <button data-nav class="test-btn" style:left="430px" onclick={testSpace} title="Simular Space al iframe">
      ⏯ Space
    </button>
    <button data-nav class="test-btn" style:left="520px" onclick={testWake} title="Mover mouse 8px">
      🖱 Wake
    </button>
    <iframe
      src={discoverUrl(selected.imdb_id)}
      title="discover"
      referrerpolicy="no-referrer"
      sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-presentation"
      allow="autoplay; fullscreen; picture-in-picture"
    ></iframe>
  </div>
{:else if mode === "trailer" && trailerKey}
  <div class="discover-mode">
    <div class="trailer-bar">
      <button data-nav class="bar-btn" onclick={stopDiscover} title="Volver (Esc / Backspace)">
        ← Volver
      </button>
      <div class="trailer-badge-inline">TRAILER</div>
      <div class="dropdown">
        <button data-nav class="bar-btn" onclick={() => (trailerSourceOpen = !trailerSourceOpen)}>
          {TRAILER_SOURCES.find(s => s.id === trailerSource)?.label} ▾
        </button>
        {#if trailerSourceOpen}
          <ul class="dropdown-menu menu-up" use:autofocusFirst>
            {#each TRAILER_SOURCES as s}
              <li>
                <button data-nav class:active={s.id === trailerSource} onclick={() => setTrailerSource(s.id)}>
                  {s.label}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <button data-nav class="bar-btn" onclick={openTrailerExternal} title="Abrir en navegador">
        ↗ Externo
      </button>
    </div>
    {#key trailerSource}
      <iframe
        src={trailerUrl(trailerKey)}
        title="trailer"
        referrerpolicy="no-referrer"
        allow="autoplay; fullscreen; picture-in-picture; encrypted-media"
      ></iframe>
    {/key}
  </div>
{:else}
  <main>
    <aside class="info" data-section="info">
      {#if !apiKey || showKey}
        <div class="key-box">
          <h3>TMDb API Key</h3>
          <p class="hint">
            Conseguila en <code>themoviedb.org/settings/api</code> (gratis).
          </p>
          <input
            type="password"
            bind:value={keyInput}
            placeholder="32 chars hex"
            onkeydown={(e) => e.key === "Enter" && saveKey()}
          />
          <button onclick={saveKey} disabled={!keyInput.trim()}>Guardar</button>
        </div>
      {:else if detailLoading}
        <div class="empty">Cargando…</div>
      {:else if selected}
        <div class="detail">
          {#if selected.backdrop_path}
            <div class="backdrop" style:background-image="url({img(`${IMG}/w780${selected.backdrop_path}`, 780)})"></div>
          {/if}
          {#if selected.poster_path}
            <div class="poster-wrap">
              <img class="poster" src={img(`${IMG}/w342${selected.poster_path}`, 342)} alt="" />
              {#if !selected.imdb_id}
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
          {#if selected.imdb_id}
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
                <button data-nav class="discover-btn discover-btn-row" onclick={startDiscover}>▶ Descubrir de nuevo</button>
              {:else if prog && prog.watched_seconds > 5}
                <button data-nav class="discover-btn discover-btn-row" onclick={startDiscover}>
                  ▶ Continuar {pct != null ? `(${pct}%)` : `(${watchedMin}m ${watchedSec}s)`}
                </button>
              {:else}
                <button data-nav class="discover-btn discover-btn-row" onclick={startDiscover}>▶ Descubrir</button>
              {/if}
              <button data-nav class="trailer-btn trailer-btn-row" onclick={watchTrailer}>🎬 Trailer</button>
            </div>
            {#if prog && prog.watched_seconds > 5}
              <button data-nav class="link-btn" onclick={restartDiscover}>↻ Volver a empezar</button>
            {/if}
          {:else}
            <div class="unavail-note">
              Este título no está disponible para reproducir.
            </div>
            <button data-nav class="discover-btn" onclick={watchTrailer}>
              🎬 Ver trailer
            </button>
          {/if}
          {#if selected.directors.length}
            <div class="people-row">
              <h4 class="people-title">{selected.media_type === "tv" ? "Creado por" : "Dirigido por"}</h4>
              <div class="people-strip">
                {#each selected.directors as p}
                  <button data-nav class="person-chip" onclick={() => openPerson(p.id)} title={p.name}>
                    {#if p.profile_path}
                      <img src={img(`${IMG}/w185${p.profile_path}`, 185)} alt={p.name} loading="lazy" />
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
                      <img src={img(`${IMG}/w185${p.profile_path}`, 185)} alt={p.name} loading="lazy" />
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
      <header data-section="filters">
        <div class="row1">
          <div class="tabs">
            <button data-nav class:active={tab === "movie"} onclick={() => switchTab("movie")}>Películas</button>
            <button data-nav class:active={tab === "tv"} onclick={() => switchTab("tv")}>Series</button>
            <button data-nav class:active={tab === "anime"} onclick={() => switchTab("anime")}>Anime</button>
          </div>
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
              <span>{SORTS.find(s => s.id === sortId)?.label || "Orden"}</span>
              <svg class="chev" viewBox="0 0 10 6" width="10" height="6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
            </button>
            {#if sortOpen}
              <ul class="dropdown-menu" role="menu" use:autofocusFirst>
                {#each SORTS as s}
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
      </header>

      {#if listError}<p class="err">{listError}</p>{/if}

      {#if listLoading}
        <div class="empty">Cargando…</div>
      {:else}
        <div class="grid-wrap">
          <div class="grid" data-section="gallery">
            <a class="card vera-card" data-nav href="/vera" title="Pregúntale a Vera">
              <div class="vera-poster">
                <div class="vera-icon">✦</div>
              </div>
              <div class="card-meta">
                <span class="card-title">Vera</span>
                <span class="card-sub">¿Qué ver hoy?</span>
              </div>
            </a>
            {#each items as it, i (it.id)}
              {@const title = it.title || it.name || ""}
              {@const date = it.release_date || it.first_air_date || ""}
              {@const year = date.slice(0, 4)}
              {@const st = statusMap.get(it.id)}
              {@const itImdb = imdbIdMap.get(it.id)}
              {@const unavail = itImdb ? unavailableSet.has(itImdb) : false}
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
                  ondblclick={() => { focusedIdx = i; pickAndDiscoverOrTrailer(it, st); }}
                  onfocus={() => { focusedIdx = i; triggerAutoPick(i); }}
                >
                  {#if it.poster_path}
                    <img src={img(`${IMG}/w342${it.poster_path}`, 342)} alt={title} loading="lazy" />
                  {:else}
                    <div class="no-poster">sin poster</div>
                  {/if}
                  {#if st === "trailer"}
                    <span class="card-badge badge-trailer">TRAILER</span>
                  {/if}
                  {#if unavail}
                    <span class="card-stamp">NO SE ENCUENTRA</span>
                  {/if}
                  <div class="card-meta">
                    <span class="card-title">{title}</span>
                    <span class="card-sub">{year} · ★ {it.vote_average.toFixed(1)}</span>
                  </div>
                </button>
              {/if}
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
              <img class="person-photo" src={img(`${IMG}/w300${personOpen.profile_path}`, 300)} alt={personOpen.name} />
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
                      <img src={img(`${IMG}/w185${f.poster_path}`, 185)} alt={f.title} loading="lazy" />
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

  {#if unavailable.open}
    <div class="modal-bg" onclick={() => (unavailable.open = false)} onkeydown={(e) => { if (e.key === "Escape") unavailable.open = false; }} role="presentation">
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
  .link-btn {
    position: relative; z-index: 1;
    background: transparent; color: #888;
    border: 0; padding: 6px; font-size: 12px;
    cursor: pointer; text-decoration: underline;
    margin: 4px 0;
  }
  .link-btn:hover { color: #f5c518; }

  .people-row { position: relative; z-index: 1; margin-top: 18px; }
  .people-title {
    margin: 0 0 8px; font-size: 12px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 1px;
    color: #888;
  }
  .people-strip {
    display: flex; gap: 10px; flex-wrap: wrap;
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
  .gallery > header {
    padding: 10px 16px; display: flex; flex-direction: column; gap: 8px;
    border-bottom: 1px solid #1f1f28; background: #0d0d12;
  }
  .row1 { display: flex; gap: 10px; align-items: center; }

  .tabs { display: flex; gap: 2px; background: #15151c; border-radius: 6px; padding: 3px; border: 1px solid #1f1f28; }
  .tabs button {
    background: transparent; color: #888; border: 0;
    padding: 6px 14px; border-radius: 4px; cursor: pointer;
    font-weight: 600; font-size: 13px;
    transition: background 0.12s, color 0.12s;
  }
  .tabs button.active { background: #f5c518; color: #0d0d12; }
  .tabs button:not(.active):hover { color: #eee; background: #1f1f28; }
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
  .chip.active {
    background: #f5c518; color: #0d0d12; border-color: #f5c518; font-weight: 700;
    box-shadow: 0 0 0 3px rgba(245,197,24,0.15);
  }
  .err { color: #ff6b6b; padding: 12px 16px; font-size: 12px; }

  .grid-wrap {
    flex: 1; overflow-y: auto;
    display: flex; flex-direction: column;
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
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .card:hover { transform: translateY(-3px); box-shadow: 0 12px 28px rgba(0,0,0,0.6); }
  .card.selected { box-shadow: 0 0 0 2px #f5c518; }

  .vera-card { text-decoration: none; }
  .vera-poster {
    aspect-ratio: 2 / 3;
    background: linear-gradient(160deg, #ff5722 0%, #c0392b 60%, #6a1b1b 100%);
    position: relative; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
  }
  .vera-poster::before {
    content: ""; position: absolute; inset: 0;
    background: radial-gradient(circle at 30% 20%, rgba(255,255,255,0.18), transparent 50%);
    pointer-events: none;
  }
  .vera-icon {
    font-size: 64px; line-height: 1; color: #fff;
    text-shadow: 0 2px 12px rgba(0,0,0,0.4);
    z-index: 1;
  }
  .vera-card:hover { box-shadow: 0 12px 32px rgba(255,87,34,0.4); }
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
  .card img, .no-poster { width: 100%; aspect-ratio: 2/3; object-fit: cover; background: #222; }
  .no-poster { display: flex; align-items: center; justify-content: center; color: #555; font-size: 13px; }
  .card-meta { padding: 10px 12px; display: flex; flex-direction: column; gap: 3px; }
  .card-title { font-size: 14px; font-weight: 600; line-height: 1.25; }
  .card-sub { font-size: 12px; color: #888; }

  /* PLAY mode */
  .discover-mode { position: fixed; inset: 0; background: #000; z-index: 999; }
  .discover-mode iframe { width: 100%; height: 100%; border: 0; }
  .back-btn {
    position: absolute; top: 14px; right: 14px; z-index: 1000;
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
    position: absolute; top: 14px; left: 14px; z-index: 1000;
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
  .inspect-btn {
    position: absolute; top: 14px; left: 180px; z-index: 1000;
    padding: 10px 16px;
    background: rgba(20,20,28,0.75);
    color: #88c0d0;
    border: 1px solid rgba(136,192,208,0.5);
    border-radius: 999px;
    font-size: 13px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
  }
  .inspect-btn:hover { background: rgba(136,192,208,0.9); color: #0d0d12; }
  .test-btn {
    position: absolute; top: 14px; left: 340px; z-index: 1000;
    padding: 10px 14px;
    background: rgba(20,20,28,0.75);
    color: #c0c0c8;
    border: 1px solid #444;
    border-radius: 999px;
    font-size: 12px; font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(6px);
  }
  .test-btn:hover { background: #f5c518; color: #0d0d12; border-color: #f5c518; }

  /* Vista unavailable */
  .unavail-screen {
    position: fixed; inset: 0; z-index: 990;
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
    position: absolute; top: 0; left: 0; right: 0; z-index: 1000;
    display: flex; align-items: center; gap: 10px;
    padding: 10px 14px;
    background: linear-gradient(180deg, rgba(0,0,0,0.85) 0%, rgba(0,0,0,0) 100%);
    pointer-events: none;
  }
  .trailer-bar > * { pointer-events: auto; }
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
  .menu-up { top: auto; bottom: calc(100% + 4px); right: auto; left: 0; }


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
