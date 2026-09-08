<script lang="ts">
  // Lista de fuentes torrent (estilo Stremio) para reproducir vía RealDebrid+mpv.
  // Autocontenido: hace la búsqueda, marca cacheadas (⚡), resuelve y lanza mpv.
  // Maneja su propio teclado (flechas/Enter/Esc) porque en este modo el page
  // no procesa navegación.
  import { invoke } from "@tauri-apps/api/core";
  import { setNowPlaying } from "$lib/playerState.svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { config } from "$lib/config.svelte";
  import { WEB_PLAYER_ENABLED } from "$lib/features";
  import { notify } from "$lib/notifStore.svelte";
  import {
    addTorrent,
    torrentStatus,
    removeTorrent,
    startQueuePoll,
    fmtBytes,
    fmtSpeed,
    fmtEta,
    type TorrentStatus,
  } from "$lib/torrents.svelte";

  // Pista real del contenedor leída de mpv (verdad del archivo).
  type MpvTrack = {
    id: number;
    kind: string; // "audio" | "sub"
    lang: string;
    title: string;
    selected: boolean;
  };

  type Src = {
    source: string;
    title: string;
    magnet: string | null;
    url: string | null;
    info_hash: string | null;
    size_bytes: number | null;
    seeders: number | null;
    quality: string;
    rd_cached: boolean | null;
    // Sub QUEMADO en el video (jkanime y similares): "lat"/"cast"/"en".
    // null = torrent normal. Cambia todo el flujo: no hay pistas que elegir
    // ni subtítulos que bajar, y no necesita debrid para reproducirse.
    hardsub: string | null;
    _count?: number; // cuántas fuentes equivalentes se apilaron en esta
    // Veredicto de la verificación REAL de pistas (ffprobe sobre la URL
    // resuelta): qué español embebido trae el archivo de verdad.
    _es?: "audio" | "subs" | "none" | "unknown";
  };

  // Pista reportada por ffprobe (header del contenedor, sin descargar).
  type ProbeTrack = { kind: string; codec: string; lang: string; title: string };

  let {
    imdbId,
    kind,
    season = null,
    episode = null,
    title,
    originalTitle = null,
    backdrop = null,
    kitsuId = null,
    rdLinked,
    autoplay = false,
    onClose,
    onWeb,
  }: {
    imdbId: string;
    kind: "movie" | "series" | "anime";
    season?: number | null;
    episode?: number | null;
    title: string;
    // Título ROMAJI (AniList `original_title`). jkanime no indexa títulos en
    // inglés: sin esto, "Frieren: Beyond Journey's End" no encuentra nada.
    originalTitle?: string | null;
    backdrop?: string | null;
    // ID Kitsu (anime, mapeado vía ani.zip): habilita Torrentio kitsu:{id}:{ep}
    // en kodios. Permite reproducir anime SIN imdb_id.
    kitsuId?: number | null;
    rdLinked: boolean;
    autoplay?: boolean;
    onClose: () => void;
    onWeb: () => void;
  } = $props();

  let loading = $state(true);
  let error = $state("");
  let sources = $state<Src[]>([]);
  // Familia de fuente en pantalla. Un anime devuelve ~50 torrents y 1 enlace
  // directo: sin este filtro la fuente en español queda perdida entre releases
  // japoneses.
  type Fam = "all" | "hardsub" | "torrent";
  let famFilter = $state<Fam>("all");
  let focusIdx = $state(0);
  let resolving = $state(false);
  let resolvingMsg = $state("");
  let playing = $state(false);
  let playingTitle = $state("");
  let mpvPoll: ReturnType<typeof setInterval> | null = null;

  // Plan B: descarga local del torrent cuando el debrid lo bloquea (451/DMCA).
  let torrenting = $state(false);
  let torrentMsg = $state("");
  let torrentStat = $state<TorrentStatus | null>(null);
  // Swarm lento: pasado este tiempo sin buffer listo ofrecemos pasarlo a la cola.
  let torrentSlow = $state(false);
  let torrentId: number | null = null;
  let torrentSrc: Src | null = null;
  // Lo que suena viene de la descarga local, no del debrid. Se avisa en pantalla
  // y en el centro de notificaciones: el usuario tiene que saber que ahí está
  // bajando (y compartiendo) el archivo él mismo.
  let desdeTorrent = $state(false);
  let torrentCancel = false;
  const SLOW_AFTER_MS = 45_000;

  const QLABEL: Record<string, string> = {
    p2160: "4K",
    p1080: "1080p",
    p720: "720p",
    sd: "SD",
    cam: "CAM",
    unknown: "—",
  };
  // Etiqueta de las fuentes web: se reproducen desde su URL tal cual, sin
  // pasar por el debrid ni por el swarm.
  const HARDSUB_LABEL: Record<string, string> = {
    lat: "🔗 Enlace directo",
    cast: "🔗 Enlace directo",
    en: "🔗 Enlace directo",
  };

  const QRANK: Record<string, number> = {
    p2160: 5,
    p1080: 4,
    p720: 3,
    sd: 2,
    cam: 1,
    unknown: 0,
  };

  // NIVEL 1 (barato, ordenar). El nombre del release es solo una PISTA, no
  // garantía: los nombres mienten. Sirve para priorizar; la verdad se confirma
  // al abrir el archivo (mpv_tracks, nivel 3). Devuelve un score según si el
  // usuario quiere doblado (audio ES) o subtitulado (VO + subs ES).
  function langScore(s: Src): number {
    const t = (s.title || "").toLowerCase();
    const latino = /\blatino\b|\blat\b|dual.?lat|español.?latino/.test(t);
    const cast = /castellano|español|espanol|\besp\b|spanish/.test(t);
    const multi = /\bmulti\b|\bdual\b/.test(t);
    const vose = /vose|v\.?o\.?s\.?e|subtitulad|spa.?subs?|\bsubs?\b.*\b(es|spa|esp)\b/.test(t);
    if (config.subMode === "dub") {
      if (latino) return 5; // audio español latino: lo ideal
      if (cast) return 4; // castellano
      if (multi) return 3; // multi/dual suele incluir pista ES
      if (vose) return 1; // solo subtitulado: peor para doblado
      return 0; // VO puro
    }
    // modo "sub": audio original + subtítulos en español
    if (vose) return 5; // explícitamente VOSE
    if (multi) return 4; // multi: trae VO + subs ES
    if (latino || cast) return 2; // doblado: menos ideal pero hay ES
    return 3; // VO puro: audio original, subs externos si hace falta
  }

  // POOL DE IDIOMA: qué FAMILIA de fuente conviene según el idioma de la app,
  // por encima de calidad y seeders. La lógica es asimétrica a propósito:
  //  - Español latino: la ruta torrent casi no trae subs latinos, así que el
  //    hardsub de jkanime gana.
  //  - Castellano: al revés — hardsub castellano casi no existe online, pero
  //    OpenSubtitles sí lo tiene, así que el torrent va primero.
  //  - Inglés y el resto: la ruta torrent (softsub EN, SubsPlease) es MEJOR que
  //    cualquier hardsub español, y el hardsub además es irreversible: hunde.
  function poolScore(s: Src): number {
    const hs = s.hardsub || "";
    if (config.lang === "es-CL") {
      if (hs === "lat") return 3;
      if (hs) return 1; // hardsub de otro español: sirve, pero de última
      return 2;
    }
    if (config.lang === "es-ES") {
      if (hs === "cast") return 3;
      if (hs) return 1;
      return 2;
    }
    return hs ? 0 : 2;
  }

  // Fuente preferida por el usuario, SEGÚN el tipo de contenido (película/serie/
  // anime tienen su propia lista en config): una o varias palabras separadas por
  // coma (ej. "MeGusta, FullScrab"). Si el release o el proveedor las contiene, la
  // fuente sube al tope del orden. Es una preferencia
  // fuerte: gana incluso a las cacheadas, porque el usuario la eligió a propósito
  // (p.ej. porque siempre trae pistas en español).
  function prefScore(s: Src): number {
    const raw =
      kind === "movie" ? config.preferredSourceMovie
      : kind === "anime" ? config.preferredSourceAnime
      : config.preferredSourceSeries;
    const pref = (raw || "").toLowerCase().trim();
    if (!pref) return 0;
    const needles = pref.split(/[,;]/).map((x) => x.trim()).filter(Boolean);
    if (!needles.length) return 0;
    const hay = `${s.title || ""} ${s.source || ""}`.toLowerCase();
    return needles.some((n) => hay.includes(n)) ? 1 : 0;
  }

  // Lista negra por tipo (config.blockedSource*): si el release o el proveedor
  // contiene alguna palabra, la fuente se HUNDE al fondo del orden. No se elimina
  // (sigue elegible si no hay nada más), pero nunca se auto-reproduce de primera.
  function blockScore(s: Src): number {
    const raw =
      kind === "movie" ? config.blockedSourceMovie
      : kind === "anime" ? config.blockedSourceAnime
      : config.blockedSourceSeries;
    const blk = (raw || "").toLowerCase().trim();
    if (!blk) return 0;
    const needles = blk.split(/[,;]/).map((x) => x.trim()).filter(Boolean);
    if (!needles.length) return 0;
    const hay = `${s.title || ""} ${s.source || ""}`.toLowerCase();
    return needles.some((n) => hay.includes(n)) ? 1 : 0;
  }

  function fmtSize(b: number | null): string {
    if (!b) return "";
    const gb = b / 1073741824;
    if (gb >= 1) return gb.toFixed(1) + " GB";
    return Math.round(b / 1048576) + " MB";
  }

  // Chips de info técnica extraídos del nombre del release.
  function infoChips(s: Src): string[] {
    const t = (s.title || "").toLowerCase();
    const out: string[] = [];
    if (/x265|h\.?265|hevc/.test(t)) out.push("HEVC");
    else if (/av1/.test(t)) out.push("AV1");
    else if (/x264|h\.?264|avc/.test(t)) out.push("H264");
    if (/dolby ?vision|dovi|\bdv\b/.test(t)) out.push("DV");
    else if (/hdr10\+/.test(t)) out.push("HDR10+");
    else if (/hdr/.test(t)) out.push("HDR");
    if (/atmos/.test(t)) out.push("Atmos");
    else if (/truehd/.test(t)) out.push("TrueHD");
    else if (/dts/.test(t)) out.push("DTS");
    else if (/dd\+|ddp|eac3|e-ac-3/.test(t)) out.push("DD+");
    else if (/ac3|dd5|dd 5/.test(t)) out.push("AC3");
    if (/multi/.test(t)) out.push("MULTi");
    if (/dual/.test(t)) out.push("Dual");
    if (/latino/.test(t)) out.push("Latino");
    else if (/castellano|español|espanol|\besp\b/.test(t)) out.push("ES");
    return out.slice(0, 5);
  }

  // Apila fuentes equivalentes (mismo título/calidad, distinto idioma/grupo).
  // Mantiene la mejor de cada grupo (la lista llega ya ordenada) y cuenta.
  function stack(list: Src[]): Src[] {
    const groups = new Map<string, Src>();
    for (const s of list) {
      const k = normKey(s);
      const ex = groups.get(k);
      if (!ex) groups.set(k, { ...s, _count: 1 });
      else ex._count = (ex._count || 1) + 1;
    }
    return [...groups.values()];
  }

  function normKey(s: Src): string {
    const t = (s.title || "")
      .toLowerCase()
      .replace(/[._\-\[\]()]/g, " ")
      .replace(
        /\b(1080p?|2160p?|720p?|480p?|4k|uhd|web[- ]?dl|webrip|bluray|bdrip|hdrip|x264|x265|hevc|h ?264|h ?265|av1|10bit|hdr10?|dv|aac|ac3|dd[p+]?5?1?|dts|atmos|truehd|multi|dual|latino|castellano|espanol|español|esp|eng|english|ita|rus|fr|vff|vostfr|subs?|remux)\b/g,
        " ",
      )
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 45);
    // El hardsub va en su propia clave: apilarlo con un torrent de nombre
    // parecido escondería la única fuente con español garantizado.
    return `${s.quality}|${s.hardsub || ""}|${t}`;
  }

  // Índices de navegación: filas (0..n-1), luego "web" (n), luego "volver" (n+1).
  // Con el player web apagado (WEB_PLAYER_ENABLED) el pie pierde un botón, así
  // que los índices se corren: si no, quedaría un slot fantasma que el teclado
  // recorre sin nada enfocado y "volver" no respondería al Enter.
  const nHardsub = $derived(sources.filter((s) => s.hardsub).length);
  const nTorrent = $derived(sources.length - nHardsub);
  // Las pestañas solo aparecen si de verdad hay dos familias entre las que
  // elegir. Con una sola serían ruido en pantalla.
  const showTabs = $derived(nHardsub > 0 && nTorrent > 0);
  const TABS: { id: Fam; label: string }[] = [
    { id: "all", label: "Todas" },
    { id: "hardsub", label: "🔗 Enlace directo" },
    { id: "torrent", label: "🧲 Torrent" },
  ];
  function tabCount(f: Fam): number {
    return f === "all" ? sources.length : f === "hardsub" ? nHardsub : nTorrent;
  }

  // La lista que se ve Y sobre la que se reproduce. Todo lo demás (foco,
  // índices del pie, salto a la siguiente fuente) va contra esto: si el usuario
  // eligió "Enlace directo", caer a un torrent japonés traicionaría la elección.
  const view = $derived(
    famFilter === "all" ? sources
    : famFilter === "hardsub" ? sources.filter((s) => s.hardsub)
    : sources.filter((s) => !s.hardsub),
  );

  const total = $derived(view.length + (WEB_PLAYER_ENABLED ? 2 : 1));
  const webIdx = $derived(WEB_PLAYER_ENABLED ? view.length : -1);
  const backIdx = $derived(view.length + (WEB_PLAYER_ENABLED ? 1 : 0));

  function setFam(f: Fam) {
    if (famFilter === f) return;
    famFilter = f;
    focusIdx = 0; // los índices viejos apuntan a otra lista
    scrollFocused();
  }

  /// Cambia de pestaña con ←/→ (mando y teclado). Solo entre las que existen.
  function cycleFam(d: number) {
    if (!showTabs) return;
    const i = TABS.findIndex((t) => t.id === famFilter);
    const next = TABS[(i + d + TABS.length) % TABS.length];
    setFam(next.id);
  }

  async function load() {
    loading = true;
    error = "";
    try {
      // Anime sin temporada/episodio = película anime (formato MOVIE):
      // kodios usa la forma movie (kitsu:{id} / título sin nº de episodio).
      if (kind === "series" && (season == null || episode == null)) {
        error = "Falta temporada/episodio.";
        return;
      }
      const srcs = await invoke<Src[]>("kodios_search", {
        imdbId,
        kind,
        title,
        originalTitle: originalTitle ?? undefined,
        season: season ?? undefined,
        episode: episode ?? undefined,
        kitsuId: kitsuId ?? undefined,
      });
      if (rdLinked) {
        const hashes = srcs.map((s) => s.info_hash).filter(Boolean) as string[];
        try {
          const cached = await invoke<string[]>("rd_instant_available", { hashes });
          const set = new Set(cached.map((h) => h.toLowerCase()));
          for (const s of srcs) {
            if (s.info_hash) s.rd_cached = set.has(s.info_hash.toLowerCase());
          }
        } catch {
          /* cache check best-effort */
        }
      }
      srcs.sort((a, b) => {
        // Lista negra: las fuentes bloqueadas se hunden al fondo, pase lo que pase.
        const blk = blockScore(a) - blockScore(b);
        if (blk) return blk;
        // Fuente preferida por el usuario: máxima prioridad (sube al tope).
        const p = prefScore(b) - prefScore(a);
        if (p) return p;
        // Pool de idioma antes que "instantáneo": de nada sirve que cargue
        // rápido si no entiendes lo que dice.
        const pool = poolScore(b) - poolScore(a);
        if (pool) return pool;
        const c = (b.rd_cached ? 1 : 0) - (a.rd_cached ? 1 : 0);
        if (c) return c;
        // Preferencia de idioma (doblado/subtitulado) por sobre la calidad: de
        // nada sirve 4K si no entiendes el audio.
        const l = langScore(b) - langScore(a);
        if (l) return l;
        const q = (QRANK[b.quality] || 0) - (QRANK[a.quality] || 0);
        if (q) return q;
        return (b.seeders || 0) - (a.seeders || 0);
      });
      sources = stack(srcs);
      if (!srcs.length) {
        error = "No encontramos fuentes para esta película.";
      } else if (autoplay && config.sourceSelect === "auto") {
        // ⚡ Ver con RealDebrid en modo automático: reproduce directo la mejor
        // (ya ordenada: preferida > cacheada > idioma > calidad > seeders).
        loading = false;
        void playFrom(0);
        return;
      }
      // Modo manual: aunque se haya pedido autoplay, mostramos el selector con
      // la mejor fuente ya enfocada (focusIdx=0) para elegir con un Enter.
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Log a la consola de Tauri (stderr) para depurar el flujo de reproducción.
  function dbg(m: string) {
    invoke("ui_log", { msg: `[picker] ${m}` }).catch(() => {});
  }

  const MAX_TRIES = 12;
  // Tope de fuentes inspeccionadas con ffprobe por intento de reproducción:
  // cada inspección cuesta resolver el magnet + leer el header (~5-15s).
  const MAX_PROBES = 4;
  // Torrents que se intentan bajar en local antes de rendirse. Cada intento
  // fallido cuesta el timeout de metadata del backend, así que son pocos.
  const MAX_TORRENT_TRIES = 2;

  // ¿La pista del probe es español? lang ISO (spa/es/es-419…) o título
  // ("Latino", "Español (España)", "Spanish [LAS]"…).
  function isEsProbe(t: ProbeTrack): boolean {
    if (/^(es|spa)/i.test(t.lang || "")) return true;
    return /espa|spanish|latino|castellano|\blat\b/i.test(t.title || "");
  }

  // NIVEL 2: verdad del archivo ANTES de reproducir. ffprobe lee el header de
  // la URL resuelta (sin descargar) y dice qué pistas trae de verdad.
  // "unknown" = no se pudo inspeccionar (sin ffprobe, timeout) → no bloquear.
  async function probeEs(url: string): Promise<"audio" | "subs" | "none" | "unknown"> {
    try {
      const tracks = await invoke<ProbeTrack[]>("ffprobe_tracks", { url });
      if (!tracks.length) return "unknown";
      const audio = tracks.some((t) => t.kind === "audio" && isEsProbe(t));
      const subs = tracks.some((t) => t.kind === "subtitle" && isEsProbe(t));
      dbg(`probe: ${tracks.length} pistas, audioES=${audio} subsES=${subs}`);
      if (audio) return "audio";
      if (subs) return "subs";
      return "none";
    } catch (e) {
      dbg(`probe fail (no bloquea): ${String(e).slice(0, 60)}`);
      return "unknown";
    }
  }

  // Lanza mpv con una URL ya resuelta y deja el picker en estado "playing".
  async function launch(url: string, s: Src) {
    resolvingMsg = "Abriendo reproductor…";
    // Contexto para el buscador de subtítulos global (SubsService).
    setNowPlaying(imdbId, title);
    await invoke("mpv_play", { url, title });
    resolving = false;
    playing = true;
    playingTitle = s.title;
    startMpvPoll();
    // Hardsub: el subtítulo está QUEMADO en la imagen. No hay pista que
    // seleccionar ni subtítulo externo que bajar — buscarlos solo gastaría
    // cuota de OpenSubtitles y podría superponer dos textos en pantalla.
    if (s.hardsub) {
      dbg(`hardsub ${s.hardsub}: sin selección de pistas ni subs externos`);
      return;
    }
    // Niveles 2-3: confirmar audio/subs REALES del archivo y elegir la
    // pista ES; si no existe, bajar subtítulos externos. No bloquea el play.
    void applyPreferredTracks();
  }

  // Intenta reproducir desde `startIdx`, saltando fuentes que fallan
  // (bloqueadas por DMCA 451, sin video, timeout). Reproduce la primera que
  // resuelva. Así un release bloqueado en el debrid no rompe la experiencia.
  // Con config.verifyEsTracks: además INSPECCIONA las pistas reales (ffprobe)
  // y salta las fuentes sin español embebido; si ninguna de las inspeccionadas
  // trae, reproduce la mejor igual (los subs externos siguen de respaldo).
  async function playFrom(startIdx: number) {
    dbg(`playFrom start=${startIdx} n=${view.length} fam=${famFilter} rd=${rdLinked} verify=${config.verifyEsTracks}`);
    // Sin debrid solo se puede reproducir lo que YA trae URL directa (fuentes
    // web tipo jkanime). Los magnets sí lo necesitan, así que se filtran en el
    // bucle en vez de bloquear todo el picker.
    if (!rdLinked && !view.some((s) => s.url)) {
      error = "Vincula tu debrid en Configuración para reproducir.";
      return;
    }
    resolving = true;
    error = "";
    desdeTorrent = false;
    let blocked = 0;
    let tried = 0;
    let probes = 0;
    let sinEs = 0;
    let lastErr = "";
    // Mejor fuente que resolvió pero NO trae español: respaldo si ninguna trae.
    let fallback: { url: string; s: Src } | null = null;
    // Fuentes que el debrid rechazó pero que SÍ tienen magnet: candidatas al
    // plan B (bajarlas nosotros). El 451 es cumplimiento legal de RD, no falta
    // de seeds, así que la mejor bloqueada suele bajarse sin problema.
    const rechazadas: Src[] = [];

    for (let i = startIdx; i < view.length && tried < MAX_TRIES; i++) {
      const s = view[i];
      if (!s.magnet && !s.url) { dbg(`#${i} SKIP (sin magnet ni url)`); continue; }
      if (!rdLinked && !s.url) { dbg(`#${i} SKIP (magnet sin debrid)`); continue; }
      dbg(`#${i} intento url=${!!s.url} magnet=${!!s.magnet} ${(s.title || "").slice(0, 40)}`);
      tried++;
      focusIdx = i;
      resolvingMsg =
        `Probando fuente ${i + 1}/${view.length}${s.rd_cached ? " ⚡" : ""}…` +
        (blocked ? ` (${blocked} bloqueada${blocked > 1 ? "s" : ""})` : "") +
        (sinEs ? ` (${sinEs} sin español)` : "");
      try {
        // URL pre-resuelta (Torrentio+RD, método Kodi) → directo a mpv, sin
        // addMagnet del cliente (evita el 451 de RD). Si no, resuelve el magnet.
        const url = s.url
          ? s.url
          : await invoke<string>("rd_resolve", { magnet: s.magnet });

        // Verificación de español real (activable en Configuración).
        // El hardsub no expone pistas ES (está quemado en la imagen): ffprobe
        // diría "none" y la saltaríamos justo por traer el español que
        // buscábamos. Se verifica solo lo que tiene pistas de verdad.
        if (config.verifyEsTracks && !s.hardsub && probes < MAX_PROBES) {
          probes++;
          resolvingMsg = `Inspeccionando pistas de la fuente ${i + 1}… 🔎`;
          const verdict = await probeEs(url);
          s._es = verdict; // badge en la lista ($state es reactivo profundo)
          if (verdict === "none") {
            sinEs++;
            if (!fallback) fallback = { url, s };
            dbg(`#${i} sin español embebido → siguiente`);
            continue;
          }
        }
        await launch(url, s);
        return;
      } catch (e) {
        lastErr = String(e);
        if (lastErr.includes("BLOQUEADO_DMCA")) blocked++;
        if (s.magnet) rechazadas.push(s);
        // sigue con la próxima fuente
      }
    }

    // Nada con español embebido entre lo inspeccionado: reproducir la mejor
    // que SÍ resolvió. Los subtítulos externos (Wyzie/OpenSubtitles) cubren.
    if (fallback) {
      dbg(`sin ES embebido en ${sinEs} fuentes → fallback a la mejor`);
      try {
        await launch(fallback.url, fallback.s);
        void mpv(["show-text", "Sin español embebido en las fuentes: usando subtítulos externos", "4000"]);
        return;
      } catch (e) {
        lastErr = String(e);
      }
    }

    dbg(`fin RD: tried=${tried} blocked=${blocked} sinEs=${sinEs} lastErr=${lastErr.slice(0, 60)}`);

    // Plan B: ninguna fuente pasó por el debrid, pero el torrent sigue vivo.
    // Lo bajamos nosotros y lo reproducimos mientras se descarga. Acá el orden
    // que importa es OTRO: con debrid mandaba la caché de RD, bajando en local
    // manda cuánta gente está compartiendo el archivo.
    let pesadas = 0;
    if (config.torrentLocal && rechazadas.length) {
      const aptas = rechazadas.filter((c) => {
        if (cabeEnLocal(c)) return true;
        pesadas++;
        return false;
      });
      const porSeeders = aptas.sort((a, b) => (b.seeders || 0) - (a.seeders || 0));
      for (const cand of porSeeders.slice(0, MAX_TORRENT_TRIES)) {
        if (await playViaTorrent(cand)) return;
        if (torrentCancel) return; // el usuario canceló o lo mandó a la cola
      }
    }

    resolving = false;
    if (error) return; // playViaTorrent ya dejó un mensaje más específico
    if (blocked > 0 && !config.torrentLocal) {
      error = `Sin fuentes reproducibles: ${blocked} bloqueada(s) por el debrid (DMCA). Activa "Descarga local" en Configuración para bajarlas igual, o prueba otra calidad.`;
    } else if (pesadas > 0) {
      // Sí había fuentes bajables, pero pasan el techo que puso el usuario.
      error =
        `Las ${pesadas} fuente(s) que quedan superan tu tope para descarga local ` +
        `(${QLABEL[config.torrentMaxQuality]} y ${config.torrentMaxGb} GB). ` +
        `Sube el tope en Configuración o busca una versión más liviana.`;
    } else if (blocked > 0) {
      error = `Sin fuentes reproducibles: ${blocked} bloqueada(s) por el debrid (DMCA) y la descarga local tampoco pudo. Prueba "🔄 Rebuscar fuentes" u otra calidad.`;
    } else {
      error = `No se pudo reproducir ninguna fuente. ${lastErr}`;
    }
  }

  // ¿Esta fuente cabe dentro del techo de la descarga local? Con debrid el peso
  // da lo mismo (lo sirve RD a velocidad de fibra); bajándolo tú hay que
  // sostener el bitrate del archivo o el video se corta. Lo desconocido pasa:
  // no vamos a descartar una fuente por falta de metadatos.
  function cabeEnLocal(s: Src): boolean {
    if ((QRANK[s.quality] || 0) > (QRANK[config.torrentMaxQuality] || 0)) return false;
    if (s.size_bytes && s.size_bytes > config.torrentMaxGb * 1024 ** 3) return false;
    return true;
  }

  // ---- Plan B: torrent local ---------------------------------------------

  // Baja el torrent con el cliente local y arranca mpv apenas hay buffer. Lo
  // que RD niega por DMCA se baja igual del swarm; el costo es que tu IP queda
  // visible ahí (por eso config.torrentLocal es opt-in) y que la velocidad
  // depende de los seeds, no de la fibra de RD.
  async function playViaTorrent(s: Src): Promise<boolean> {
    if (!s.magnet) return false;
    resolving = false;
    error = "";
    torrenting = true;
    torrentCancel = false;
    torrentSlow = false;
    torrentStat = null;
    torrentSrc = s;
    torrentId = null;
    torrentMsg = "Buscando peers…";
    const t0 = Date.now();
    try {
      const added = await addTorrent(s.magnet, title, config.torrentBufferMb);
      if (torrentCancel) return false;
      torrentId = added.id;
      dbg(`torrent local id=${added.id} ${added.name} (${added.size_bytes} B)`);
      torrentMsg = "Descargando el inicio…";
      for (;;) {
        if (torrentCancel) return false;
        const st = await torrentStatus(added.id);
        torrentStat = st;
        if (st.state === "error") throw new Error(st.error || "el torrent falló");
        if (st.buffer_ready) break;
        torrentSlow = Date.now() - t0 > SLOW_AFTER_MS;
        await sleep(1000);
      }
      if (torrentCancel) return false;
      torrenting = false;
      desdeTorrent = true;
      // Sigue bajando de fondo mientras se ve: el poll de la cola lo muestra.
      startQueuePoll();
      await launch(added.stream_url, s);
      notify(
        "warn",
        "Reproduciendo desde descarga local",
        "El debrid bloqueó esta fuente por DMCA, así que el archivo lo estás bajando y compartiendo tú.",
      );
      void mpv([
        "show-text",
        "Desde descarga local: el debrid bloqueó esta fuente",
        "5000",
      ]);
      return true;
    } catch (e) {
      torrenting = false;
      dbg(`torrent local falló: ${String(e).slice(0, 100)}`);
      error = `Descarga local: ${String(e)}`;
      // No dejar el torrent muerto ocupando la cola y el disco.
      const id = torrentId;
      torrentId = null;
      if (id !== null) {
        try {
          await removeTorrent(id, true);
        } catch {
          /* ya no estaba */
        }
      }
      return false;
    }
  }

  // "Que siga bajando de fondo": deja el torrent en la cola y vuelve al menú.
  // Avisamos por notificación cuando termine.
  function torrentToQueue() {
    torrentCancel = true;
    torrenting = false;
    startQueuePoll();
    notify(
      "info",
      "Descargando en segundo plano",
      torrentSrc?.title || title,
    );
    onClose();
  }

  // Cancelar de verdad: saca el torrent y borra lo bajado a medias.
  async function torrentAbort() {
    torrentCancel = true;
    torrenting = false;
    const id = torrentId;
    torrentId = null;
    if (id !== null) {
      try {
        await removeTorrent(id, true);
      } catch {
        /* si ya no existe, da igual */
      }
    }
  }

  function startMpvPoll() {
    stopMpvPoll();
    mpvPoll = setInterval(async () => {
      try {
        const alive = await invoke<boolean>("mpv_running");
        if (!alive) {
          stopMpvPoll();
          onClose();
        }
      } catch {
        /* sigue intentando */
      }
    }, 1500);
  }
  function stopMpvPoll() {
    if (mpvPoll !== null) {
      clearInterval(mpvPoll);
      mpvPoll = null;
    }
  }

  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  // Manda un comando JSON-IPC a mpv (set propiedad, sub-add, etc.).
  const mpv = (args: unknown[]) => invoke("mpv_cmd", { args }).catch(() => {});

  // ¿La pista es español? Mira el lang ISO (spa/es) y, de respaldo, el título.
  function isEsTrack(t: MpvTrack): boolean {
    const l = (t.lang || "").toLowerCase();
    if (/^(es|spa|esp)/.test(l)) return true;
    return /espa|spanish|latino|castellano/i.test(t.title || "");
  }

  // NIVELES 2-3: la verdad del contenedor. Lee las pistas reales de mpv y elige
  // según la preferencia. Solo baja subs externos si el archivo NO trae ES.
  async function applyPreferredTracks() {
    // mpv tarda un momento en demuxear y exponer las pistas tras abrir.
    let tracks: MpvTrack[] = [];
    for (let i = 0; i < 15; i++) {
      tracks = await invoke<MpvTrack[]>("mpv_tracks").catch(() => []);
      if (tracks.some((t) => t.kind === "audio")) break;
      await sleep(400);
    }
    const audios = tracks.filter((t) => t.kind === "audio");
    const subs = tracks.filter((t) => t.kind === "sub");
    const esAudio = audios.find(isEsTrack);
    const esSub = subs.find(isEsTrack);

    if (config.subMode === "dub" && esAudio) {
      // Audio español embebido: selecciónalo y oculta subtítulos.
      await mpv(["set", "aid", String(esAudio.id)]);
      await mpv(["set", "sub-visibility", "no"]);
      dbg(`audio ES embebido id=${esAudio.id}`);
      return;
    }
    // Subtitulado (o doblado sin audio ES): audio original + subs ES.
    if (esSub) {
      await mpv(["set", "sid", String(esSub.id)]);
      await mpv(["set", "sub-visibility", "yes"]);
      dbg(`sub ES embebido id=${esSub.id}`);
      return;
    }
    // El release no trae ES → subtítulos externos (Wyzie o OpenSubtitles).
    await loadExternalSub();
  }

  // Subtítulos externos: Wyzie si hay key, si no OpenSubtitles (cuota diaria).
  // mpv carga la URL directo con sub-add (no hace falta bajar a disco).
  async function loadExternalSub(): Promise<boolean> {
    const lang = config.subsLang || "es";
    if (lang === "off") return false;
    let url = "";
    if (config.wyzieKey) {
      try {
        const subs = await invoke<{ url: string }[]>("wyzie_search", {
          imdbId,
          language: lang,
          apiKey: config.wyzieKey,
        });
        if (subs.length) url = subs[0].url;
      } catch (e) {
        dbg(`wyzie fail: ${String(e).slice(0, 60)}`);
      }
    }
    if (!url) {
      try {
        const os = await invoke<{ url: string; remaining: number }>("os_search", {
          imdbId,
          language: lang,
        });
        url = os.url;
        dbg(`opensubtitles ok, quedan ${os.remaining}/día`);
      } catch (e) {
        dbg(`opensubtitles fail: ${String(e).slice(0, 80)}`);
      }
    }
    if (!url) {
      dbg("sin subtítulos ES (embebidos ni externos)");
      return false;
    }
    // Guardar el .srt en Descargas (para coleccionarlos) y cargar el archivo
    // local en mpv. Si falla la descarga a disco, se carga la URL directa.
    let toLoad = url;
    try {
      const fname = `${title} [${lang}]`;
      const path = await invoke<string>("subtitle_save", { url, filename: fname });
      if (path) {
        toLoad = path;
        dbg(`sub guardado: ${path}`);
      }
    } catch (e) {
      dbg(`no se pudo guardar sub, uso URL: ${String(e).slice(0, 60)}`);
    }
    await mpv(["sub-add", toLoad, "select"]);
    await mpv(["set", "sub-visibility", "yes"]);
    return true;
  }

  async function stopMpv() {
    stopMpvPoll();
    try {
      await invoke("mpv_stop");
    } catch {
      /* ignore */
    }
    onClose();
  }

  $effect(() => () => stopMpvPoll());

  // Con libmpv embebido el video se cierra desde DENTRO (Esc en el reproductor
  // → backend hace stop() y emite "mpv:state"=false). El webview estuvo oculto
  // y su timer de poll pudo quedar suspendido, así que NO dependemos del poll:
  // escuchamos el evento y volvemos al menú al recibir false.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen<boolean>("mpv:state", (e) => {
      if (e.payload === false) {
        stopMpvPoll();
        onClose();
      }
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  function move(d: number) {
    if (!total) return;
    let i = focusIdx + d;
    if (i < 0) i = 0;
    if (i > total - 1) i = total - 1;
    focusIdx = i;
    scrollFocused();
  }

  function activate() {
    dbg(`activate focusIdx=${focusIdx} back=${backIdx} web=${webIdx}`);
    if (focusIdx === backIdx) return onClose();
    if (focusIdx === webIdx) return onWeb();
    if (view[focusIdx]) void playFrom(focusIdx);
  }

  function scrollFocused() {
    queueMicrotask(() => {
      document
        .querySelector<HTMLElement>(".sp-row.focused, .sp-foot-btn.focused")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function onKey(e: KeyboardEvent) {
    if (playing) {
      if (e.key === "Escape" || e.key === "Backspace" || e.key === "Enter") {
        e.preventDefault();
        void stopMpv();
      }
      return;
    }
    if (torrenting) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        void torrentAbort();
      } else if (e.key === "Enter" && torrentSlow) {
        e.preventDefault();
        torrentToQueue();
      }
      return;
    }
    if (resolving) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        resolving = false; // cancela visualmente; el resolve sigue en backend
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "ArrowLeft":
        e.preventDefault();
        cycleFam(-1);
        break;
      case "ArrowRight":
        e.preventDefault();
        cycleFam(1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        activate();
        break;
      case "Escape":
      case "Backspace":
        e.preventDefault();
        onClose();
        break;
    }
  }

  load();
</script>

<svelte:window onkeydown={onKey} />

<div class="sp-root">
  {#if backdrop}
    <div class="sp-bg" style:background-image="url({backdrop})"></div>
  {/if}
  <div class="sp-scrim"></div>
  <div class="sp-hint-back">Esc o Backspace para volver</div>

  <div class="sp-content">
    <header class="sp-head">
      <h2 class="sp-title-h">{title}</h2>
      <span class="sp-sub">
        {#if famFilter === "hardsub"}Enlace directo
        {:else if famFilter === "torrent"}Torrents vía debrid
        {:else}Elige una fuente{/if}
      </span>
    </header>

    {#if playing}
      <div class="sp-center">
        <p class="sp-playing">▶ Reproduciendo</p>
        <p class="sp-playing-title">{playingTitle}</p>
        {#if desdeTorrent}
          <p class="sp-desde-local">
            📥 Desde descarga local — el debrid bloqueó esta fuente por DMCA.
            Sigue bajando mientras la ves.
          </p>
        {/if}
        <button class="sp-foot-btn focused" onclick={stopMpv}>⏹ Detener y volver</button>
        <span class="sp-hint">Se cierra solo al terminar el video</span>
      </div>
    {:else if loading}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p>Buscando fuentes…</p>
      </div>
    {:else if torrenting}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p class="sp-tor-head">📥 Bajando del swarm</p>
        <p class="sp-tor-why">
          El debrid bloqueó esta fuente (DMCA). El torrent sigue vivo, así que
          lo bajamos aquí y empieza apenas haya buffer.
        </p>
        {#if torrentStat}
          {@const t = torrentStat}
          {@const bufPct = t.buffer_target
            ? Math.min(100, (t.buffered_bytes * 100) / t.buffer_target)
            : 0}
          <div class="sp-bar"><div class="sp-bar-fill" style="width: {bufPct}%"></div></div>
          <p class="sp-tor-line">
            Buffer {Math.round(bufPct)}% · {fmtBytes(t.buffered_bytes)} de
            {fmtBytes(t.buffer_target)}
          </p>
          <p class="sp-tor-meta">
            {fmtSpeed(t.download_bps)} · {t.peers} peers · listo en {fmtEta(t.eta_secs)}
          </p>
        {:else}
          <p class="sp-tor-line">{torrentMsg}</p>
        {/if}
        {#if torrentSlow}
          <p class="sp-tor-slow">Este swarm va lento.</p>
          <button class="sp-foot-btn focused" onclick={torrentToQueue}>
            📥 Seguir bajando de fondo y avisarme
          </button>
        {/if}
        <span class="sp-hint">Esc para cancelar y borrar lo bajado</span>
      </div>
    {:else if resolving}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p>{resolvingMsg}</p>
        <span class="sp-hint">Esc para cancelar</span>
      </div>
    {:else}
      {#if error}
        <p class="sp-error">{error}</p>
      {/if}

      {#if showTabs}
        <div class="sp-tabs">
          {#each TABS as t}
            <button
              class="sp-tab"
              class:on={famFilter === t.id}
              onclick={() => setFam(t.id)}
            >{t.label} <span class="sp-tab-n">{tabCount(t.id)}</span></button>
          {/each}
          <span class="sp-tab-hint">← → cambia</span>
        </div>
      {/if}

      <div class="sp-list">
        {#each view as s, i (s.info_hash || s.url || i)}
          <button
            data-nav
            class="sp-row"
            class:focused={focusIdx === i}
            onclick={() => { focusIdx = i; void playFrom(i); }}
            onmouseenter={() => (focusIdx = i)}
          >
            <span class="sp-q sp-q-{s.quality}">{QLABEL[s.quality] || "—"}</span>
            <div class="sp-mid">
              <span class="sp-title">{s.title}</span>
              <div class="sp-chips">
                {#if prefScore(s)}<span class="sp-pref">★ Preferida</span>{/if}
                {#if s.rd_cached}<span class="sp-cached">⚡ Instantáneo</span>{/if}
                {#if s.hardsub}<span class="sp-hardsub">{HARDSUB_LABEL[s.hardsub] || "🔗 Enlace directo"}</span>{/if}
                {#if s._es === "audio"}<span class="sp-es-ok">🗣 Audio ES ✓</span>
                {:else if s._es === "subs"}<span class="sp-es-ok">💬 Subs ES ✓</span>
                {:else if s._es === "none"}<span class="sp-es-no">Sin español</span>{/if}
                {#each infoChips(s) as c}<span class="sp-chip">{c}</span>{/each}
                <span class="sp-prov">{s.source}</span>
              </div>
            </div>
            <span class="sp-meta">
              {#if s._count && s._count > 1}<span class="sp-dupe">×{s._count}</span>{/if}
              {#if s.seeders}<span>{s.seeders} 🌱</span>{/if}
              {#if s.size_bytes}<span>{fmtSize(s.size_bytes)}</span>{/if}
            </span>
          </button>
        {/each}
      </div>

      <footer class="sp-foot">
        {#if WEB_PLAYER_ENABLED}
          <button
            data-nav
            class="sp-foot-btn"
            class:focused={focusIdx === webIdx}
            onclick={onWeb}
            onmouseenter={() => (focusIdx = webIdx)}
          >
            🌐 Ver en web
          </button>
        {/if}
        <button
          data-nav
          class="sp-foot-btn"
          class:focused={focusIdx === backIdx}
          onclick={onClose}
          onmouseenter={() => (focusIdx = backIdx)}
        >
          ← Volver
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  /* Frame consistente con PlayMenu: fanart difuminado + scrim + glass. */
  .sp-root {
    position: fixed;
    inset: 0;
    z-index: 50;
    overflow: hidden;
    background: #0d0d12;
    color: #e6e6ec;
  }
  .sp-bg {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(10px) saturate(1.1);
    transform: scale(1.06);
  }
  .sp-scrim {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, rgba(8, 8, 12, 0.95) 0%, rgba(8, 8, 12, 0.72) 55%, rgba(8, 8, 12, 0.88) 100%),
      linear-gradient(0deg, rgba(8, 8, 12, 0.9) 0%, rgba(8, 8, 12, 0.25) 60%);
  }
  .sp-hint-back {
    position: absolute;
    top: 18px;
    right: 24px;
    color: #9a9aa4;
    font-size: 12px;
    z-index: 2;
  }
  .sp-content {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 30px 48px 36px;
  }
  .sp-head {
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-bottom: 18px;
  }
  .sp-title-h {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
  }
  .sp-sub {
    color: #b0b0ba;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .sp-center {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    color: #c8c8d0;
  }
  .sp-hint { color: #6e6e78; font-size: 12px; }
  /* Plan B: descarga local del torrent (debrid bloqueado por DMCA). */
  .sp-tor-head { margin: 0; font-size: 20px; font-weight: 700; color: #f3a951; }
  .sp-tor-why {
    margin: 0; max-width: 460px; text-align: center;
    color: #9a9aa6; font-size: 13px; line-height: 1.45;
  }
  .sp-tor-line { margin: 0; color: #e6e6ea; font-size: 14px; }
  .sp-tor-meta { margin: 0; color: #9a9aa6; font-size: 12px; }
  .sp-tor-slow { margin: 6px 0 0; color: #f3a951; font-size: 13px; }
  .sp-desde-local {
    margin: 0; max-width: 460px; text-align: center;
    color: #cbb489; font-size: 12.5px; line-height: 1.45;
  }
  .sp-bar {
    width: min(420px, 70vw); height: 6px;
    background: #2a2a36; border-radius: 3px; overflow: hidden;
  }
  .sp-bar-fill { height: 100%; background: #f3a951; transition: width .3s ease; }
  .sp-playing { margin: 0; font-size: 22px; font-weight: 700; color: #f3a951; }
  .sp-playing-title { margin: 0; color: #c8c8d0; font-size: 14px; max-width: 70%; text-align: center; }
  .sp-spinner {
    width: 34px; height: 34px;
    border: 3px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: sp-spin 1s linear infinite;
  }
  @keyframes sp-spin { to { transform: rotate(360deg); } }
  .sp-error { color: #ff7373; font-size: 14px; margin: 0 0 12px; }

  .sp-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 8px;
    scrollbar-color: #3a3a4a transparent;
  }
  .sp-row {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    text-align: left;
    background: rgba(20, 20, 28, 0.62);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    padding: 12px 16px;
    cursor: pointer;
    color: inherit;
    transition: border-color 0.12s, background 0.12s, transform 0.12s;
  }
  .sp-row.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    transform: translateX(4px);
  }
  .sp-q {
    flex: none;
    min-width: 54px;
    text-align: center;
    font-weight: 700;
    font-size: 12px;
    padding: 5px 8px;
    border-radius: 6px;
    background: #2a2a36;
    color: #d8d8e0;
  }
  .sp-q-p2160 { background: #3a2a4a; color: #d9b8ff; }
  .sp-q-p1080 { background: #1f3a2a; color: #9be38a; }
  .sp-q-p720 { background: #2a3340; color: #9bc4e3; }
  .sp-mid {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .sp-title {
    color: #f0f0f4;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sp-chips { display: flex; flex-wrap: wrap; gap: 5px; align-items: center; }
  .sp-chip {
    font-size: 10.5px;
    font-weight: 600;
    color: #c0c0c8;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-cached {
    font-size: 10.5px;
    font-weight: 700;
    color: #ffd76b;
    background: #2e2410;
    border: 1px solid #5a4520;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-tabs {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    flex-wrap: wrap;
  }
  .sp-tab {
    font-size: 12.5px;
    font-weight: 600;
    color: #c0c0c8;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.14);
    padding: 5px 12px;
    border-radius: 999px;
    cursor: pointer;
  }
  .sp-tab.on {
    color: #0d1b12;
    background: #8ce0a6;
    border-color: #8ce0a6;
  }
  .sp-tab-n {
    opacity: 0.65;
    font-weight: 700;
    margin-left: 2px;
  }
  .sp-tab-hint {
    font-size: 11px;
    color: #7a7a85;
    margin-left: auto;
  }
  .sp-hardsub {
    font-size: 10.5px;
    font-weight: 700;
    color: #b6f0c2;
    background: #12301b;
    border: 1px solid #2c5a38;
    padding: 2px 7px;
    border-radius: 5px;
  }
  /* Veredicto de la inspección real de pistas (ffprobe). */
  .sp-es-ok {
    font-size: 10.5px;
    font-weight: 700;
    color: #8ad1ff;
    background: #102230;
    border: 1px solid #2a4a62;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-es-no {
    font-size: 10.5px;
    font-weight: 700;
    color: #ff9a9a;
    background: #2b1414;
    border: 1px solid #5a2a2a;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-pref {
    font-size: 10.5px;
    font-weight: 700;
    color: #9be38a;
    background: #14271a;
    border: 1px solid #2f5a3a;
    padding: 2px 7px;
    border-radius: 5px;
  }
  .sp-prov {
    font-size: 10.5px;
    color: #7a7a86;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .sp-meta {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
    color: #a0a0aa;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .sp-dupe {
    background: #f3a951;
    color: #1a1208;
    border-radius: 5px;
    padding: 1px 6px;
    font-weight: 700;
    font-size: 11px;
  }

  .sp-foot { display: flex; gap: 10px; margin-top: 16px; }
  .sp-foot-btn {
    background: rgba(20, 20, 28, 0.62);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 2px solid rgba(255, 255, 255, 0.12);
    color: #d8d8e0;
    border-radius: 10px;
    padding: 11px 18px;
    cursor: pointer;
    font-size: 13.5px;
  }
  .sp-foot-btn.focused {
    border-color: #f3a951;
    background: rgba(243, 169, 81, 0.16);
    color: #fff;
  }
</style>
