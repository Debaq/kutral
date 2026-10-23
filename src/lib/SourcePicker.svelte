<script lang="ts">
  // Lista de fuentes torrent (estilo Stremio) para reproducir vía RealDebrid+mpv.
  // Autocontenido: hace la búsqueda, marca cacheadas (⚡), resuelve y lanza mpv.
  // Maneja su propio teclado (flechas/Enter/Esc) porque en este modo el page
  // no procesa navegación.
  import { invoke } from "@tauri-apps/api/core";
  import { player, setNowPlaying } from "$lib/playerState.svelte";
  import { elegirPistaSub, pistaDelIdioma } from "$lib/pistas";
  import { esFinReal, type FinPayload } from "$lib/finVideo";
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
  import {
    QLABEL,
    HARDSUB_LABEL,
    ordenarFuentes,
    stack,
    cabeEnLocal,
    prefScore,
    infoChips,
    fmtSize,
    type Src,
  } from "$lib/fuentes";
  import { encolar as encolarPendiente } from "$lib/colaDescargas.svelte";
  import {
    iniciarVigilancia,
    detenerVigilancia,
    fmtBps,
    type MotivoRendicion,
  } from "$lib/cacheWatch.svelte";
  import {
    fallaRd,
    anotarFallaRd,
    olvidarFallaRd,
    okRd,
    anotarOkRd,
    type FallaRd,
  } from "$lib/rdFallas.svelte";
  import {
    descargaUsable,
    descargaEnCurso,
    adoptarDeSesion,
    registrarDescarga,
    olvidarDescarga,
    estadoDescarga,
  } from "$lib/descargas.svelte";
  import {
    cast,
    tvLista,
    rasgosDe,
    audioQueSuena,
    evaluar,
    aprenderFalla,
    aprenderMudo,
    rasgosActuales,
    seguir as seguirEnTv,
    cambiarRasgos,
    control as castControl,
    darGracia,
    detener as castDetener,
    saltar as castSaltar,
    nombreAudio,
    fmtTiempo,
    type CastStatus,
    type ProbeTrack,
    type ParaTv,
    type CastTv,
  } from "$lib/cast.svelte";
  import {
    buscarSubtitulos,
    claveSub,
    enlaceSub,
    idiomaSubs,
    mejorSubtitulo,
    type OpcionSub,
  } from "$lib/subtitulos";
  import { contextoActual } from "$lib/historial.svelte";
  import {
    pistaDe,
    recordarPack,
    capituloDelPack,
    hayPack,
    type ArchivoPack,
    type Resuelto,
  } from "$lib/pack";

  // Pista real del contenedor leída de mpv (verdad del archivo).
  type MpvTrack = {
    id: number;
    kind: string; // "audio" | "sub"
    lang: string;
    title: string;
    selected: boolean;
    forced: boolean;
  };


  let {
    imdbId,
    kind,
    clave = "",
    season = null,
    episode = null,
    title,
    originalTitle = null,
    backdrop = null,
    kitsuId = null,
    rdLinked,
    autoplay = false,
    descargarSolo = false,
    retomarSegundos = 0,
    onClose,
    onWeb,
    onLanzado,
  }: {
    imdbId: string;
    kind: "movie" | "series" | "anime";
    // Clave del título en el historial/descargas (imdb, o `anilist:<id>` en
    // anime). Sin ella no se puede saber si la copia local es de esta película.
    clave?: string;
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
    /**
     * Modo "bajar para después": la lista no reproduce nada, encola la fuente
     * elegida y vuelve. Misma búsqueda y mismo orden que al reproducir — lo
     * único que cambia es qué pasa al elegir.
     */
    descargarSolo?: boolean;
    /** Segundo desde el que arranca mpv (historial). 0 = desde el principio. */
    retomarSegundos?: number;
    onClose: () => void;
    onWeb: () => void;
    /** Justo antes de entregarle el video a mpv (o a la TV). */
    onLanzado?: () => void;
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
  // El botón "Cambiar fuente" del reproductor cierra mpv igual que salir, pero
  // no significa lo mismo: hay que quedarse en la lista para elegir otra, no
  // devolver al catálogo. El backend avisa antes de cerrar (ver mpv_embed.rs).
  let cambiandoFuente = false;
  // El video llegó al final SOLO (evento "mpv:fin" del backend). Quien decide
  // qué sigue es la página (menú en películas, próximo capítulo en series), así
  // que acá solo hay que no tratarlo como un cierre normal.
  let terminoSolo = false;
  // Se cerró a propósito el capítulo anterior que seguía en pantalla (ver
  // desdePack): ese cierre no es "salir".
  let soltandoVideo = false;
  let mpvPoll: ReturnType<typeof setInterval> | null = null;

  // Plan B: descarga local del torrent cuando el debrid lo bloquea (451/DMCA).
  let torrenting = $state(false);
  let torrentMsg = $state("");
  let torrentStat = $state<TorrentStatus | null>(null);
  // Swarm lento: pasado este tiempo sin buffer listo ofrecemos pasarlo a la cola.
  let torrentSlow = $state(false);
  let torrentId: number | null = null;
  // El torrent de la espera ya estaba en la sesión: cancelar no lo borra.
  let torrentReutilizado = false;
  let torrentHash = "";
  let torrentSrc: Src | null = null;
  // Lo que suena viene de la descarga local, no del debrid. Se avisa en pantalla
  // y en el centro de notificaciones: el usuario tiene que saber que ahí está
  // bajando (y compartiendo) el archivo él mismo.
  let desdeTorrent = $state(false);

  // ---- Descubrir en la TV (ver cast.svelte.ts) ----
  // Destino de ESTA reproducción: siempre empieza en este equipo (ahí se
  // elige audio y subtítulos) y T desde la lista lo cambia. Lo normal es ver
  // aquí y después "Pasar a la TV" desde el reproductor.
  let destinoTv = $state(false);
  // Lo que está sonando en mpv, para "Pasar a la TV".
  let enMpv: { url: string; s: Src } | null = null;
  // Pantalla "Pasar a la TV": qué se lleva y qué cambia allá. null = cerrada.
  let aTv = $state<{ info: ParaTv; avisos: string[] } | null>(null);
  let aTvFoco = $state(0);
  let aTvBuscando = $state(false);
  // Pista de audio y corrimiento de subtítulos en la TV (para recargar igual).
  let castAudio = $state(1);
  let castDesfase = $state(0);
  // Pistas del archivo en la TV (menú Audio) y el subtítulo que lleva.
  let castTracks = $state<ProbeTrack[]>([]);
  let castSubs: string | null = null;
  // Submenú de los controles de la TV. null = los botones de siempre.
  let menuTv = $state<"audio" | "sync" | null>(null);
  // Corrimiento elegido que todavía no se mandó (se junta: cada envío recarga).
  let syncPendiente = $state<number | null>(null);
  let syncTimer: ReturnType<typeof setTimeout> | null = null;
  // Transmitiendo: el picker muestra los controles de la TV en vez de mpv.
  let casting = $state(false);
  let castFoco = $state(0);
  let castTvNombre = $state("");
  // Fuente que está en la TV: "No se oye" sigue con la de abajo.
  let castSrc: Src | null = null;
  // Esc mientras la TV carga: cortar y no seguir probando.
  let castCancel = false;
  // Lo que se mandó a la TV, para recargarlo en el mismo minuto con otros
  // subtítulos (el receptor no deja cambiar la pista sin volver a cargar).
  let castEnvio: Record<string, unknown> | null = null;
  // Menú de subtítulos en la TV: null = cerrado.
  let subsTv = $state<OpcionSub[] | null>(null);
  let buscandoSubsTv = $state(false);
  let subsTvActual = $state("");
  // La transmisión terminó desde la TV (su control, otra app, se apagó): el
  // segundo en que iba, para ofrecer seguir aquí. null = sigue en la TV.
  let tvTermino = $state<number | null>(null);
  // Última posición y duración que informó la TV, y si ya respondió alguna vez
  // (recién lanzada, cast.estado todavía no llegó y no es que haya terminado).
  let tvPos = 0;
  let tvDur = 0;
  let tvVista = false;
  // Segundo desde el que retomar al cambiar de versión ("No se oye").
  let desdeForzado: number | null = null;
  // Nombre del archivo del torrent local: su stream (127.0.0.1) no dice la
  // extensión y la TV necesita saber si es MKV o MP4.
  let archivoTorrent = "";
  // Está sonando la copia que ya estaba en el disco: ni debrid ni swarm. Se
  // avisa en pantalla porque explica por qué arrancó al instante.
  let desdeLocal = $state(false);
  // Modo descarga: mensaje de la pantalla de "encolando".
  let encolando = $state(false);
  // La red no da para ver esto en vivo: pantalla con las dos salidas reales
  // (bajarla y verla después, o seguir igual). Ver cacheWatch.
  let rendicion = $state<MotivoRendicion | null>(null);
  // Se eligió una fuente que RD ya no pudo abrir (rdFallas): se explica y se
  // ofrece verla por torrent en vez de volver a gastar el intento.
  let avisoRd = $state<{ i: number; s: Src; falla: FallaRd } | null>(null);
  let avisoRdFoco = $state(0);
  // Antes de reproducir solo (sin que se haya elegido la fuente) se ofrece lo
  // que hay si: otras versiones están bloqueadas por DMCA en RD (por torrent
  // sí se ven y pueden traer el audio o los subtítulos que esta no tiene), o
  // la que abrió no trae el idioma elegido. Sin respuesta, a los OFERTA_SEG
  // arranca la que abrió.
  const OFERTA_SEG = 10;
  type Oferta = { url: string; s: Src; n: number; sinIdioma: boolean; revisadas: number };
  let oferta = $state<Oferta | null>(null);
  let ofertaSeg = $state(0);
  let ofertaFoco = $state(0);
  let ofertaTimer: ReturnType<typeof setInterval> | null = null;
  // Aviso sobre la lista (no es un error): "ninguna abrió en RD, elige una".
  let avisoLista = $state("");
  // Lo que estaba sonando cuando la red se rindió, para poder retomarlo.
  let rendicionSrc: { url: string; s: Src } | null = null;
  // El usuario eligió seguir viendo igual: no se le vuelve a ofrecer nada.
  let sinRendirse = false;
  // Fuentes que ya se vieron y se descartaron desde el reproductor (por título:
  // es lo que identifica al release en la lista). Se marcan como "ya probada".
  // (array y no Set: $state no rastrea mutaciones de Set sin svelte/reactivity)
  let descartadas = $state<string[]>([]);
  let torrentCancel = false;
  // Cerrado el selector, lo que quedaba en vuelo (probar fuentes en el debrid,
  // esperar un torrent) no puede terminar abriendo el reproductor: volver a
  // entrar arrancaba otra búsqueda y las dos lanzaban mpv una encima de otra.
  let vivo = true;
  $effect(() => () => {
    vivo = false;
    torrentCancel = true;
    if (ofertaTimer) clearInterval(ofertaTimer);
  });
  const SLOW_AFTER_MS = 45_000;

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

  // La copia que ya está en el disco, como una fuente más de la lista. Es la
  // mejor de todas: sin red, sin debrid, sin swarm y arranca al instante.
  function srcDeDescarga(f: {
    release_title: string;
    ruta: string;
    info_hash: string;
    quality: string;
    size_bytes: number;
  }): Src {
    return {
      source: "En tu equipo",
      title: f.release_title,
      magnet: null,
      url: f.ruta, // mpv abre rutas locales igual que URLs
      info_hash: f.info_hash,
      size_bytes: f.size_bytes || null,
      seeders: null,
      quality: f.quality || "unknown",
      rd_cached: null,
      hardsub: null,
      local: true,
    };
  }

  // La que todavía está bajando: se ve desde el MISMO torrent de la sesión
  // (el magnet con solo el hash basta, ya está gestionado), que sigue bajando
  // en orden mientras se mira. Sin `url`: playFrom la manda a playViaTorrent.
  function srcDeBajada(f: {
    release_title: string;
    ruta: string;
    info_hash: string;
    quality: string;
    size_bytes: number;
  }): Src {
    return {
      ...srcDeDescarga(f),
      source: "En tu equipo (bajando)",
      url: null,
      magnet: `magnet:?xt=urn:btih:${f.info_hash}`,
      filename: f.ruta.split(/[\\/]/).pop() || null,
    };
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

      // La que quedó bajando de antes, mirada contra la sesión torrent: si
      // terminó la marca completa, si ya no existe la olvida. Va antes que
      // todo lo demás para que lo de abajo vea el estado real.
      const enCurso = await descargaEnCurso(clave, season ?? -1, episode ?? -1);

      if (descargarSolo) {
        // Bajar dos veces lo mismo es exactamente lo que esto viene a evitar.
        const est = estadoDescarga(clave, season ?? -1, episode ?? -1);
        if (est) {
          notify(
            "info",
            est === "completa" ? "Ya está en tu equipo" : "Ya se está bajando",
            title,
          );
          onClose();
          return;
        }
        if (!config.torrentLocal) {
          error =
            'Para bajar títulos activa "Descarga local" en Configuración.';
          return;
        }
      }

      // ¿Ya la bajaste antes? La copia en disco gana a cualquier fuente de la
      // red, así que se busca ANTES de scrapear: en modo automático ni
      // siquiera hace falta salir a buscar nada.
      // A medias también cuenta: se ve desde el torrent que ya baja en vez
      // de arrancar otro.
      let local: Src | null = null;
      const fila = await descargaUsable(clave, season ?? -1, episode ?? -1);
      if (fila) {
        local = srcDeDescarga(fila);
        dbg(`copia local: ${fila.ruta}`);
      } else if (enCurso) {
        local = srcDeBajada(enCurso.fila);
        dbg(`bajando en local (${Math.round(enCurso.torrent.pct)}%): ${enCurso.fila.ruta}`);
      }
      // El botón del menú ya avisó que es la versión del equipo: se abre aunque
      // la selección de fuentes esté en manual. Para elegir otra está
      // "Buscar otras fuentes" (sin autoplay).
      if (local && autoplay) {
        sources = [local];
        loading = false;
        await playFrom(0);
        if (playing || casting || torrentCancel) return;
        dbg("la copia local no abrió → a buscar fuentes");
        // No abrió (archivo corrupto, códec imposible): se olvida y se sigue
        // por el camino normal. Un archivo que no se puede ver no es una
        // copia local. Salvo en la TV: ahí puede ser solo que ESA TV no la
        // reproduce, y en el equipo sí se ve. La que baja no se olvida: lo
        // que falló puede ser solo que todavía no hay peers.
        if (fila && !destinoTv) {
          await olvidarDescarga(fila.clave, fila.season, fila.episode, fila.info_hash);
          local = null;
        } else if (!fila) {
          local = null;
        }
        loading = true;
        error = "";
      }

      // Capítulo del pack que venía sonando: sale del mismo torrent, sin
      // buscar ni elegir de nuevo.
      if (autoplay && !descargarSolo && !local && (await desdePack())) return;

      let srcs: Src[];
      try {
        srcs = await invoke<Src[]>("kodios_search", {
          imdbId,
          kind,
          title,
          originalTitle: originalTitle ?? undefined,
          season: season ?? undefined,
          episode: episode ?? undefined,
          kitsuId: kitsuId ?? undefined,
        });
      } catch (e) {
        // Sin internet no hay scrapers, pero la copia del disco sí se puede
        // ver: sería absurdo mostrar "no hay fuentes" teniéndola acá al lado.
        if (!local) throw e;
        dbg(`búsqueda falló pero hay copia local: ${String(e).slice(0, 60)}`);
        sources = [local];
        loading = false;
        if (autoplay && config.sourceSelect === "auto") void playFrom(0);
        return;
      }
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
        // La que RD ya abrió hace poco sigue en la cuenta: abre al instante.
        for (const s of srcs) if (okRd(hashDe(s))) s.rd_cached = true;
      }
      ordenarFuentes(srcs, kind);
      if (!vivo) return;

      // El torrent de una de estas fuentes ya está en la sesión aunque no
      // haya fila que lo ate al título: se ve desde ahí, no desde el debrid.
      if (!local && !descargarSolo && season == null && episode == null) {
        const adoptada = await adoptarDeSesion(
          clave,
          srcs.map((s) => ({ info_hash: hashDe(s), title: s.title, quality: s.quality })),
        );
        if (adoptada) {
          local = adoptada.torrent.finished
            ? srcDeDescarga(adoptada.fila)
            : srcDeBajada(adoptada.fila);
          dbg(`ya estaba en la sesión torrent: ${adoptada.fila.ruta}`);
        }
      }

      if (descargarSolo) {
        // Solo se puede bajar lo que es torrent: un enlace directo de web no
        // tiene magnet que darle al cliente.
        sources = stack(srcs).filter((x) => x.magnet);
        if (!sources.length) {
          error = "No hay ningún torrent para bajar de este título.";
        } else if (config.sourceSelect === "auto") {
          // Mismo criterio que el plan B al reproducir: la mejor que quepa
          // dentro del techo que el usuario puso en Configuración.
          const mejor = sources.find(cabeEnLocal);
          if (mejor) {
            loading = false;
            void encolar(mejor);
          } else {
            error =
              `Todas las fuentes superan tu tope de descarga ` +
              `(${QLABEL[config.torrentMaxQuality]} y ${config.torrentMaxGb} GB). ` +
              `Súbelo en Configuración, o elige una a mano.`;
          }
        }
        return;
      }

      // La copia local va SIEMPRE primera: ninguna fuente de la red le gana.
      sources = local ? [local, ...stack(srcs)] : stack(srcs);
      agrupar();
      if (!sources.length) {
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
  // fallido cuesta el timeout de metadata del backend, así que son pocos. Sin
  // debrid la descarga local es la única vía, no un plan B: ahí vale insistir.
  const MAX_TORRENT_TRIES = 2;
  const MAX_TORRENT_TRIES_SOLO = 4;

  // ¿La pista del probe es español? lang ISO (spa/es/es-419…) o título
  // ("Latino", "Español (España)", "Spanish [LAS]"…).
  function isEsProbe(t: ProbeTrack): boolean {
    if (/^(es|spa)/i.test(t.lang || "")) return true;
    return /espa|spanish|latino|castellano|\blat\b/i.test(t.title || "");
  }

  // El idioma que tiene que traer la fuente (audio o subtítulos): el de los
  // subtítulos en Configuración, español si están apagados.
  const IDIOMA_PROBE: Record<string, { iso: RegExp; nombre: RegExp; texto: string }> = {
    es: { iso: /^(es|spa)/i, nombre: /espa|spanish|latino|castellano|\blat\b/i, texto: "español" },
    en: { iso: /^(en|eng)/i, nombre: /english|ingl/i, texto: "inglés" },
    fr: { iso: /^(fr|fra|fre)/i, nombre: /fran|french/i, texto: "francés" },
    pt: { iso: /^(pt|por)/i, nombre: /portug|brazil/i, texto: "portugués" },
    it: { iso: /^(it|ita)/i, nombre: /ital/i, texto: "italiano" },
    de: { iso: /^(de|deu|ger)/i, nombre: /deutsch|german|alem/i, texto: "alemán" },
    ja: { iso: /^(ja|jpn)/i, nombre: /japan|japon/i, texto: "japonés" },
    ko: { iso: /^(ko|kor)/i, nombre: /korea|corea/i, texto: "coreano" },
    zh: { iso: /^(zh|zho|chi)/i, nombre: /chin/i, texto: "chino" },
  };
  function idiomaProbe() {
    return IDIOMA_PROBE[idiomaSubs()] ?? IDIOMA_PROBE.es;
  }
  function esDelIdioma(t: ProbeTrack): boolean {
    const i = idiomaProbe();
    return i.iso.test(t.lang || "") || i.nombre.test(t.title || "");
  }

  // ffprobe una vez por URL: el filtro de español y la TV preguntan lo mismo.
  const probeCache = new Map<string, Promise<ProbeTrack[]>>();
  function probeTracks(url: string): Promise<ProbeTrack[]> {
    let p = probeCache.get(url);
    if (!p) {
      p = invoke<ProbeTrack[]>("ffprobe_tracks", { url });
      probeCache.set(url, p);
    }
    return p;
  }

  // NIVEL 2: verdad del archivo ANTES de reproducir. ffprobe lee el header de
  // la URL resuelta (sin descargar) y dice qué pistas trae de verdad.
  // "unknown" = no se pudo inspeccionar (sin ffprobe, timeout) → no bloquear.
  async function probeEs(url: string): Promise<"audio" | "subs" | "none" | "unknown"> {
    try {
      const tracks = await probeTracks(url);
      if (!tracks.length) return "unknown";
      const audio = tracks.some((t) => t.kind === "audio" && esDelIdioma(t));
      const subs = tracks.some((t) => t.kind === "subtitle" && esDelIdioma(t));
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
  // Con qué se recuerda la sincronía de subtítulos (sub_sync.rs): qué se ve,
  // de qué fuente y con qué preferencia de Configuración.
  function contextoSync(s: Src) {
    // Anime puede venir sin imdb (AniList/Kitsu) y sin temporada.
    const base = imdbId || clave || (kitsuId ? `kitsu:${kitsuId}` : "");
    const cap = episode != null;
    const temporada = season ?? 0;
    return {
      media: base ? (cap ? `${base}:${temporada}:${episode}` : base) : `t:${title}`,
      serie: base && cap ? `${base}:${temporada}` : "",
      fuente: s.info_hash || s.title || "",
      release: s.title || "",
      modo: config.syncModo,
      capituloAnterior: config.syncCapAnterior,
      auto: config.syncAuto,
    };
  }

  async function launch(url: string, s: Src, desdeSegundos?: number) {
    if (!vivo) return;
    resolvingMsg = "Abriendo reproductor…";
    desdeLocal = !!s.local;
    // Contexto para el buscador de subtítulos global (SubsService).
    setNowPlaying(imdbId, title, season ?? null, episode ?? null, s.title || "", url);
    // Retomar donde quedó. 5 s de colchón: caer justo en el corte desorienta,
    // un poco de contexto previo hace que se retome la escena, no el frame.
    const base = desdeSegundos ?? desdeForzado ?? retomarSegundos;
    const desde = base > 5 ? Math.floor(base) - 5 : 0;
    if (desde > 0) dbg(`retomando en ${desde}s`);
    onLanzado?.();
    if (destinoTv) {
      await lanzarEnTv(url, s, desde);
      return;
    }
    await invoke("mpv_play", {
      url,
      title,
      startSecs: desde > 0 ? desde : null,
      sync: contextoSync(s),
    });
    enMpv = { url, s };
    void invoke("mpv_tv", { on: !!cast.tv }).catch(() => {});
    // Capítulo de un pack: el bar ofrece saltar a otro del mismo torrent.
    void invoke("mpv_capitulos", { on: kind !== "movie" && hayPack(clave) }).catch(() => {});
    // El archivo del disco no pasa por la red: no hay cache que vigilar.
    if (!s.local) {
      void iniciarVigilancia({
        desdeTorrent,
        sizeBytes: s.size_bytes,
        sinRendirse,
        onRendirse: (m) => void redNoDa(m, url, s),
      });
    }
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

  // El capítulo pedido está en el pack que venía sonando: se abre del mismo
  // torrent (en RD, el que ya está en la cuenta; en local, el de la sesión).
  // Si no se puede, sigue la búsqueda normal.
  async function desdePack(): Promise<boolean> {
    const cap = capituloDelPack(clave, season ?? null, episode ?? null);
    if (!cap) return false;
    const s = cap.src;
    dbg(`capítulo del pack (${cap.via}): ${s.filename}`);
    loading = false;
    try {
      if (cap.via === "rd" && rdLinked && s.magnet) {
        resolving = true;
        resolvingMsg = "Abriendo el capítulo del mismo pack…";
        const r = await invoke<Resuelto>("rd_resolver", {
          magnet: s.magnet,
          pista: pistaDe(s, season ?? null, episode ?? null),
        });
        recordarPack(clave, season ?? null, s, "rd", r.pack);
        await launch(r.url, s);
        return true;
      }
      if (cap.via === "local" && config.torrentLocal && s.magnet) {
        if (await playViaTorrent(s)) return true;
      }
    } catch (e) {
      dbg(`el pack no dio: ${String(e).slice(0, 80)}`);
    }
    resolving = false;
    torrenting = false;
    error = "";
    loading = true;
    // Saltando de capítulo desde el reproductor, el anterior sigue en
    // pantalla (pausado) y taparía la búsqueda: se cierra.
    if (await invoke<boolean>("mpv_running").catch(() => false)) {
      soltandoVideo = true;
      await invoke("mpv_stop").catch(() => {});
    }
    return false;
  }

  // Manda la fuente a la TV en vez de a mpv. Tira un error "TV_…" cuando esta
  // versión no le sirve a la TV, para que playFrom pruebe la siguiente:
  //   TV_INCOMPATIBLE  ya se sabe (aprendido) que la TV no puede con esto
  //   TV_FALLA         la TV lo rechazó ahora (y se aprende)
  //   TV_RED           la TV no está o no alcanza a Kütral: cortar la búsqueda
  //   TV_CORTADO       el usuario canceló o alguien tomó la TV
  //
  // `como`: lo que traía el reproductor al pasar a la TV (audio en el orden
  // del archivo, subtítulo puesto y su corrimiento). Sin eso, el audio sale de
  // la preferencia de Configuración y los subtítulos se buscan solos.
  async function lanzarEnTv(
    url: string,
    s: Src,
    desde: number,
    como?: { audio: number; subs?: string | null; desfase: number },
  ) {
    castCancel = false;
    const tv = await esperarTv();
    castTvNombre = tv.nombre;
    resolvingMsg = `Revisando si ${tv.nombre} puede con esta versión… 🔎`;
    const tracks = await probeTracks(url).catch(() => [] as ProbeTrack[]);
    const audioN = como?.audio ?? audioPreferido(tracks);
    const r = rasgosDe(tracks, audioN);
    if (r) {
      const ev = evaluar(tv, r);
      if (!ev.apto) {
        s._tv = ev.motivo; // badge en la lista
        throw new Error(`TV_INCOMPATIBLE: ${ev.motivo}`);
      }
    }
    // Los subs embebidos no se pueden prender en la TV: externos si hacen
    // falta (o el que venía puesto en el reproductor).
    const audio = audioQueSuena(tracks, audioN);
    const conSubs =
      !s.hardsub &&
      config.subsLang !== "off" &&
      (config.subMode === "sub" || !(audio && isEsProbe(audio)));
    let subs: string | null = como?.subs ?? null;
    if (como?.subs === undefined && conSubs) {
      resolvingMsg = "Buscando subtítulos… 💬";
      subs = (await buscarSubUrl()).url || null;
    }
    if (castCancel) throw new Error("TV_CORTADO: cancelado");
    resolvingMsg = `Enviando a ${tv.nombre}… 📺`;
    const envio = {
      tv,
      url,
      titulo: title,
      subtitulo: s.title || null,
      imagen: backdrop,
      subsIdioma: config.subsLang || "es",
      archivo: archivoTorrent || null,
      audio: audioN,
    };
    const desfase = subs ? (como?.desfase ?? 0) : 0;
    try {
      const audioOk = await invoke<boolean>("cast_play", { ...envio, desde, subs, subsDesfase: desfase });
      if (!audioOk) {
        notify(
          "info",
          "La TV suena con el primer audio",
          "Esta versión no deja elegir la pista de audio en la TV.",
        );
      }
    } catch (e) {
      const m = String(e);
      if (m.includes("LOAD_FAILED") && r) {
        const culpa = aprenderFalla(tv.id, r);
        dbg(`TV rechazó el LOAD → aprendido: ${culpa}`);
        throw new Error(`TV_FALLA: ${culpa}`);
      }
      throw new Error(`TV_RED: ${m}`);
    }
    const res = await vigilarArranque(desde);
    dbg(`TV arranque: ${res} (${r ? `${r.video} + ${r.audio}` : "sin probe"})`);
    if (res === "ok") {
      castSrc = s;
      seguirEnTv({ tvId: tv.id, rasgos: r, ctx: contextoActual(), desde });
      resolving = false;
      castEnvio = envio;
      castAudio = audioN;
      castDesfase = desfase;
      castTracks = tracks;
      castSubs = subs;
      menuTv = null;
      syncPendiente = null;
      subsTv = null;
      subsTvActual = subs ? "automáticos" : "";
      tvTermino = null;
      tvVista = false;
      tvPos = desde;
      tvDur = 0;
      casting = true;
      castFoco = 0;
      playingTitle = s.title;
      return;
    }
    if (res === "error") {
      await invoke("cast_soltar").catch(() => {});
      if (r) {
        const culpa = aprenderFalla(tv.id, r);
        s._tv = `tu TV no pudo con ${culpa}`;
        notify(
          "info",
          `${tv.nombre} no pudo con esta versión`,
          `Aprendido: no reproduce ${culpa}. La próxima vez Kütral la salta sola.`,
        );
        throw new Error(`TV_FALLA: ${culpa}`);
      }
      throw new Error("TV_FALLA: la TV no pudo abrir el video");
    }
    if (res === "tiempo") {
      // Tardó demasiado: puede ser la red, no el formato. No se aprende nada.
      await castDetener().catch(() => {});
      throw new Error("TV_FALLA: la TV no arrancó a tiempo");
    }
    await invoke("cast_soltar").catch(() => {});
    if (res === "red") {
      throw new Error(
        `TV_RED: ${tv.nombre} no alcanza a Kütral. Revisa el firewall en Configuración → Transmitir a la TV.`,
      );
    }
    throw new Error("TV_CORTADO: se interrumpió");
  }

  // La TV elegida, esperándola un rato: es común mandar a la TV y recién
  // después prenderla. Esc (castCancel) corta la espera.
  async function esperarTv(): Promise<CastTv> {
    const t0 = Date.now();
    for (let intento = 1; ; intento++) {
      try {
        return await tvLista();
      } catch (e) {
        const m = e instanceof Error ? e.message : String(e);
        if (castCancel) throw new Error("TV_CORTADO: cancelado");
        if (!cast.tv || Date.now() - t0 > 75_000) throw new Error(`TV_RED: ${m}`);
        resolvingMsg = `${cast.tv.nombre} no contesta. ¿Está encendida? Reintentando (${intento})… 📺`;
        for (let i = 0; i < 8 && !castCancel; i++) await sleep(500);
      }
    }
  }

  // Audio para la TV cuando no viene del reproductor: el doblaje en español
  // si Configuración pide doblado (lo mismo que elige mpv), si no la primera.
  function audioPreferido(tracks: ProbeTrack[]): number {
    if (config.subMode !== "dub") return 1;
    const i = tracks.filter((t) => t.kind === "audio").findIndex(isEsProbe);
    return i >= 0 ? i + 1 : 1;
  }

  // ---- Pasar a la TV (desde el bar del reproductor) ----

  // Lo que cambia en la TV respecto de verlo aquí.
  function avisosTv(info: ParaTv, s: Src): string[] {
    const v = ["Cambiar el audio o correr los subtítulos recarga el video en el mismo minuto: tarda unos segundos."];
    if (info.sub_imagen) {
      v.push("Los subtítulos que trae el archivo son imágenes y la TV no los muestra: baja otros en Subtítulos antes de pasar, o Kütral buscará unos.");
    } else if (info.sub_interno) {
      v.push("Los subtítulos que trae el archivo no pasan a la TV: Kütral buscará unos descargables.");
    }
    const codec = info.audio_codec.toLowerCase();
    if (cast.tv?.tipo === "cast" && (codec.startsWith("dts") || codec === "truehd")) {
      v.push(`Este audio (${nombreAudio(codec)}) puede no sonar en la TV. Si no se oye, elige otro en Audio antes de pasar.`);
    }
    if (info.audio > 1 || s.local || desdeTorrent || cast.tv?.tipo === "dlna") {
      v.push("El video pasa por este equipo: déjalo prendido mientras lo ves.");
    }
    return v;
  }

  async function pasarATv() {
    if (!enMpv || !cast.tv || aTv || casting) return;
    const info = await invoke<ParaTv>("mpv_para_tv").catch(() => null);
    if (!info) return;
    await invoke("mpv_suspend").catch(() => {});
    aTv = { info, avisos: avisosTv(info, enMpv.s) };
    aTvFoco = 0;
  }

  async function aTvAccion(id: "transmitir" | "aqui") {
    const p = aTv;
    if (!p || !enMpv || aTvBuscando) return;
    if (id === "aqui") {
      aTv = null;
      await invoke("mpv_resume").catch(() => {});
      return;
    }
    const { url, s } = enMpv;
    const { info } = p;
    aTvBuscando = true;
    resolvingMsg = `Buscando ${cast.tv?.nombre ?? "la TV"}… 📺`;
    try {
      await lanzarEnTv(url, s, Math.max(0, Math.floor(info.pos) - 2), {
        audio: info.audio > 0 ? info.audio : 1,
        // Externo puesto: ese mismo. Interno: que busque uno. Ninguno: sin.
        subs: info.sub_archivo || (info.sub_interno ? undefined : null),
        desfase: info.sub_desfase,
      });
      // Ya suena en la TV: el reproductor de aquí se cierra sin volver al menú.
      aTv = null;
      enMpv = null;
      playing = false;
      stopMpvPoll();
      detenerVigilancia();
      soltandoVideo = true;
      await invoke("mpv_stop").catch(() => {});
    } catch (e) {
      const m = String(e instanceof Error ? e.message : e).replace(/^TV_\w+: /, "");
      if (aTv) aTv = { ...aTv, avisos: [`No se pudo: ${m}`, ...aTv.avisos.filter((a) => !a.startsWith("No se pudo"))] };
    } finally {
      aTvBuscando = false;
    }
  }

  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen("player:a-tv", () => void pasarATv()).then((u) => (un = u));
    return () => {
      if (un) un();
      void invoke("mpv_tv", { on: false }).catch(() => {});
    };
  });

  // Espera a que la TV arranque de verdad (el tiempo avanza) o avise que no
  // puede. Con 4K en wifi la carga inicial puede tardar bastante.
  async function vigilarArranque(
    desde: number,
  ): Promise<"ok" | "error" | "red" | "tiempo" | "cortado"> {
    const t0 = Date.now();
    while (Date.now() - t0 < 45_000) {
      if (castCancel) {
        await castDetener().catch(() => {});
        return "cortado";
      }
      await sleep(1000);
      const st = await invoke<CastStatus>("cast_status").catch(() => null);
      if (!st) continue;
      if (st.estado === "LOADING" || st.estado === "BUFFERING") {
        resolvingMsg = `${castTvNombre} está cargando el video… 📺`;
      }
      if (st.estado === "PLAYING" && st.pos >= desde + 2) return "ok";
      // Alguien la pausó con el control de la TV: ya había arrancado.
      if (st.estado === "PAUSED" && st.pos > desde) return "ok";
      if (st.estado === "IDLE" && st.motivo === "ERROR") return "error";
      if (st.estado === "IDLE" && st.motivo === "SIN_ACCESO") return "red";
      if (st.estado === "SIN_APP") return "cortado";
      if (st.estado === "IDLE" && st.motivo && st.motivo !== "ERROR") return "cortado";
    }
    return "tiempo";
  }

  // ---- Controles con la película en la TV ----

  const audiosTv = $derived(castTracks.filter((t) => t.kind === "audio"));

  type CastBtn = { id: string; label: string };
  const castBtns = $derived<CastBtn[]>(
    tvTermino != null
      ? [
          { id: "aqui", label: `💻 Seguir aquí desde ${fmtTiempo(tvTermino)}` },
          { id: "volver", label: "↩ Volver" },
        ]
      : subsTv != null
      ? [
          { id: "sub:off", label: "🚫 Sin subtítulos" },
          ...subsTv.map((o, i) => ({ id: `sub:${i}`, label: o.label })),
          { id: "sub:volver", label: "↩ Volver" },
        ]
      : menuTv === "audio"
      ? [
          ...audiosTv.map((a, i) => ({
            id: `aud:${i + 1}`,
            label: `${i + 1 === castAudio ? "● " : ""}${etiquetaAudio(a, i)}`,
          })),
          { id: "menu:volver", label: "↩ Volver" },
        ]
      : menuTv === "sync"
      ? [
          { id: "sync:-1", label: "⏪ Antes 1 s" },
          { id: "sync:-0.25", label: "◀ Antes 0,25 s" },
          { id: "sync:0.25", label: "▶ Después 0,25 s" },
          { id: "sync:1", label: "⏩ Después 1 s" },
          { id: "sync:0", label: "↺ Sin corrimiento" },
          { id: "menu:volver", label: "↩ Volver" },
        ]
      : [
          { id: "pausa", label: cast.estado?.estado === "PAUSED" ? "▶ Seguir" : "⏸ Pausa" },
          { id: "atras", label: "⏪ 30 s" },
          { id: "adelante", label: "⏩ 30 s" },
          { id: "vol-", label: "🔉 Bajar volumen" },
          { id: "vol+", label: "🔊 Subir volumen" },
          ...(audiosTv.length > 1 ? [{ id: "audio", label: "🎧 Audio" }] : []),
          { id: "subs", label: buscandoSubsTv ? "💬 Buscando…" : "💬 Subtítulos" },
          ...(castSubs ? [{ id: "sync", label: "⏱ Sincronía" }] : []),
          { id: "mudo", label: "🔇 No se oye" },
          { id: "aqui", label: "💻 Seguir aquí" },
          { id: "detener", label: "⏹ Detener" },
          { id: "navegar", label: "↩ Seguir navegando" },
        ],
  );

  // "Latino · E-AC3 5.1" / "Pista 2 · AAC". Sin idioma ni título, el número.
  function etiquetaAudio(a: ProbeTrack, i: number): string {
    const nombre = [a.lang && a.lang !== "und" ? a.lang.toUpperCase() : "", a.title]
      .filter(Boolean)
      .join(" — ");
    return `${nombre || `Pista ${i + 1}`} · ${nombreAudio(a.codec)}`;
  }

  // Vuelve a cargar lo mismo en la TV, en el mismo minuto, con otro audio,
  // subtítulo o corrimiento (la TV no deja cambiarlos en caliente: guarda el
  // subtítulo del principio y solo toca la primera pista de audio).
  async function recargarTv(
    cambio: { audio?: number; subs?: string | null; desfase?: number },
    titulo: string,
  ): Promise<boolean> {
    if (!castEnvio) return false;
    const audio = cambio.audio ?? castAudio;
    const subs = cambio.subs !== undefined ? cambio.subs : castSubs;
    const desfase = cambio.desfase ?? castDesfase;
    const desde = Math.max(0, Math.floor(cast.estado?.pos ?? tvPos) - 3);
    darGracia(45_000);
    try {
      const audioOk = await invoke<boolean>("cast_play", {
        ...castEnvio,
        audio,
        desde,
        subs,
        subsDesfase: subs ? desfase : 0,
      });
      castEnvio = { ...castEnvio, audio };
      if (audio !== castAudio) cambiarRasgos(rasgosDe(castTracks, audio), desde);
      castAudio = audio;
      castSubs = subs;
      castDesfase = desfase;
      notify(
        "info",
        titulo,
        audioOk ? "La TV retoma en el mismo minuto." : "Esta versión no deja elegir el audio en la TV: suena el primero.",
      );
      return true;
    } catch (e) {
      notify("warn", "La TV no aceptó el cambio", String(e));
      return false;
    }
  }

  function corrimientoTxt(s: number): string {
    if (Math.abs(s) < 0.001) return "sin corrimiento";
    const v = Math.abs(s).toFixed(2).replace(".", ",");
    return s > 0 ? `${v} s después` : `${v} s antes`;
  }

  // Cada toque suma; se manda a la TV cuando se deja de tocar (una recarga,
  // no una por toque).
  function correrSubs(d: number) {
    const base = syncPendiente ?? castDesfase;
    syncPendiente = d === 0 ? 0 : Math.round((base + d) * 100) / 100;
    if (syncTimer) clearTimeout(syncTimer);
    syncTimer = setTimeout(() => {
      syncTimer = null;
      const v = syncPendiente;
      syncPendiente = null;
      if (v == null || v === castDesfase) return;
      void recargarTv({ desfase: v }, `Subtítulos: ${corrimientoTxt(v)}`);
    }, 1500);
  }

  async function volumenTv(d: number) {
    const st = cast.estado;
    if (!st) return;
    const v = Math.max(0, Math.min(1, Math.round((st.volumen + d) * 10) / 10));
    if (st.silencio) await castControl("silencio", 0);
    await castControl("volumen", v);
    cast.estado = { ...st, volumen: v, silencio: false };
  }

  function saltarConBarra(e: MouseEvent) {
    const st = cast.estado;
    if (!st || !(st.duracion > 0)) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    void castControl("saltar", Math.floor(f * st.duracion));
  }

  // Seguimiento de la TV mientras está en pantalla: si deja de reportar (la
  // cortaron desde la TV) se pasa a "terminó" en vez de quedar en "Cargando…".
  $effect(() => {
    if (!casting || tvTermino != null) return;
    const st = cast.estado;
    if (st) {
      tvVista = true;
      if (st.pos > 0) tvPos = st.pos;
      if (st.duracion > 0) tvDur = st.duracion;
      return;
    }
    if (!tvVista) return;
    // Llegó al final: nada que seguir aquí.
    if (tvDur > 0 && tvPos >= tvDur - 90) {
      casting = false;
      onClose();
      return;
    }
    tvTermino = tvPos;
    castFoco = 0;
  });

  // Lista de subtítulos para lo que está en la TV (del capítulo, si es serie).
  async function abrirSubsTv() {
    if (buscandoSubsTv) return;
    if (!imdbId) {
      notify("info", "Sin subtítulos", "Este título no tiene con qué buscarlos.");
      return;
    }
    buscandoSubsTv = true;
    try {
      const opts = await buscarSubtitulos(imdbId, { season, episode }, {
        release: player.nowPlaying.release,
        videoUrl: player.nowPlaying.videoUrl,
      });
      subsTv = opts;
      castFoco = 0;
      if (!opts.length) notify("info", "Sin alternativas", "No se encontraron subtítulos; solo puedes apagarlos.");
    } finally {
      buscandoSubsTv = false;
    }
  }

  // Recarga el video en la TV en el mismo minuto con otro subtítulo (o sin).
  async function elegirSubTv(id: string) {
    const opts = subsTv;
    subsTv = null;
    castFoco = 0;
    if (id === "sub:volver" || !opts || !castEnvio) return;
    const o = id === "sub:off" ? null : opts[Number(id.slice(4))];
    let subs: string | null = null;
    try {
      if (o) subs = await enlaceSub(o);
    } catch (e) {
      notify("warn", "No se pudo bajar el subtítulo", String(e));
      return;
    }
    // Otro subtítulo viene con su propio tiempo: el corrimiento vuelve a 0.
    if (await recargarTv({ subs, desfase: 0 }, o ? "Subtítulos cambiados" : "Subtítulos apagados")) {
      subsTvActual = o ? o.label : "";
    }
  }

  // Corta la TV y abre la misma versión en este equipo, desde donde iba.
  async function seguirAqui() {
    const pos = tvTermino ?? cast.estado?.pos ?? tvPos;
    const src = castSrc;
    const seguiaEnTv = tvTermino == null;
    // casting en false ANTES de detener: si no, el seguimiento de arriba vería
    // la TV apagarse y lo tomaría por un corte desde la TV.
    casting = false;
    tvTermino = null;
    castSrc = null;
    if (seguiaEnTv) await castDetener().catch(() => {});
    destinoTv = false;
    const idx = src ? view.indexOf(src) : -1;
    desdeForzado = pos;
    try {
      await playFrom(idx >= 0 ? idx : focusIdx);
    } finally {
      desdeForzado = null;
    }
  }

  async function castAccion(id: string) {
    if (id.startsWith("sub:")) return void elegirSubTv(id);
    if (id === "menu:volver") {
      menuTv = null;
      castFoco = 0;
      return;
    }
    if (id.startsWith("aud:")) {
      const n = Number(id.slice(4));
      menuTv = null;
      castFoco = 0;
      if (n !== castAudio) void recargarTv({ audio: n }, `Audio: ${etiquetaAudio(audiosTv[n - 1], n - 1)}`);
      return;
    }
    if (id.startsWith("sync:")) return correrSubs(Number(id.slice(5)));
    try {
      switch (id) {
        case "audio":
          menuTv = "audio";
          castFoco = Math.max(0, castAudio - 1);
          break;
        case "sync":
          menuTv = "sync";
          castFoco = 0;
          break;
        case "vol-":
          await volumenTv(-0.1);
          break;
        case "vol+":
          await volumenTv(0.1);
          break;
        case "subs":
          await abrirSubsTv();
          break;
        case "pausa":
          await castControl(cast.estado?.estado === "PAUSED" ? "seguir" : "pausa");
          break;
        case "atras":
          await castSaltar(-30);
          break;
        case "adelante":
          await castSaltar(30);
          break;
        case "mudo":
          await noSeOye();
          break;
        case "aqui":
          await seguirAqui();
          break;
        case "detener":
          casting = false;
          await castDetener().catch(() => {});
          onClose();
          break;
        case "volver":
          casting = false;
          tvTermino = null;
          onClose();
          break;
        case "navegar":
          // La película sigue en la TV; la pastilla de abajo la controla.
          onClose();
          break;
      }
    } catch (e) {
      notify("warn", "La TV no respondió", String(e));
    }
  }

  // El video anda pero no hay sonido: la TV no decodifica ESE audio. Se
  // aprende (sin ambigüedad, a diferencia de un rechazo) y se sigue con la
  // próxima versión desde el mismo minuto.
  async function noSeOye() {
    const act = rasgosActuales();
    const pos = cast.estado?.pos ?? 0;
    if (act?.rasgos) {
      aprenderMudo(act.tvId, act.rasgos);
      notify(
        "info",
        "Aprendido",
        `${castTvNombre} no reproduce audio ${nombreAudio(act.rasgos.audio)}. Buscando otra versión…`,
      );
    }
    casting = false;
    await castDetener().catch(() => {});
    if (castSrc) {
      castSrc._tv = act?.rasgos ? `tu TV no reproduce audio ${nombreAudio(act.rasgos.audio)}` : "sin sonido en tu TV";
      if (!descartadas.includes(castSrc.title)) descartadas = [...descartadas, castSrc.title];
    }
    const sig = castSrc ? view.indexOf(castSrc) + 1 : focusIdx + 1;
    castSrc = null;
    desdeForzado = pos;
    try {
      await playFrom(Math.max(0, sig));
    } finally {
      desdeForzado = null;
    }
  }

  // Con los controles en pantalla la pastilla global sobra.
  $effect(() => {
    cast.controlesAbiertos = casting;
    return () => {
      cast.controlesAbiertos = false;
    };
  });

  function toggleDestino() {
    if (!cast.tv) {
      notify("info", "Sin TV elegida", "Busca tu TV en Configuración → Transmitir a la TV.");
      return;
    }
    destinoTv = !destinoTv;
  }

  // Intenta reproducir desde `startIdx`, saltando fuentes que fallan
  // (bloqueadas por DMCA 451, sin video, timeout). Reproduce la primera que
  // resuelva. Así un release bloqueado en el debrid no rompe la experiencia.
  // Con config.verifyEsTracks: además INSPECCIONA las pistas reales (ffprobe)
  // y salta las fuentes sin español embebido; si ninguna de las inspeccionadas
  // trae, reproduce la mejor igual (los subs externos siguen de respaldo).
  // Capítulo para las marcas de RD ("" en películas): un pack puede no traer
  // un capítulo y sí el siguiente.
  const capRd = $derived(season != null && episode != null ? `T${season}E${episode}` : "");

  // Hash de la fuente: el del scraper o, si no vino, el del magnet.
  function hashDe(s: Src): string | null {
    return s.info_hash || /btih:([0-9a-z]{40})/i.exec(s.magnet || "")?.[1] || null;
  }

  // Lo que RD ya no pudo abrir de esta fuente (solo cuenta si se resuelve por RD).
  function marcaRd(s: Src): FallaRd | null {
    return rdLinked && s.magnet && !s.url ? fallaRd(hashDe(s), capRd) : null;
  }

  // `elegida`: el usuario tocó ESTA fuente (no es el autoplay ni el paso a la
  // siguiente). Si RD ya no pudo con ella, se le pregunta en vez de saltarla.
  async function playFrom(startIdx: number, elegida = false) {
    dbg(`playFrom start=${startIdx} n=${view.length} fam=${famFilter} rd=${rdLinked} local=${config.torrentLocal} verify=${config.verifyEsTracks}`);
    // Sin debrid quedan dos vías: las fuentes que ya traen URL directa (web
    // tipo jkanime) y la descarga local. Sin ninguna de las dos no hay nada
    // que intentar.
    if (!rdLinked && !config.torrentLocal && !view.some((s) => s.url)) {
      error =
        'Sin debrid y sin descarga local no hay forma de reproducir. ' +
        'Activa "Descarga local" en Configuración o vincula un debrid.';
      return;
    }
    resolving = true;
    error = "";
    avisoLista = "";
    desdeTorrent = false;
    let blocked = 0;
    let tried = 0;
    let probes = 0;
    let sinEs = 0;
    // Saltadas porque RD ya no pudo con ellas antes (rdFallas).
    let marcadas = 0;
    let lastErr = "";
    // TV: versiones que ya se sabe que no le sirven, y las que rechazó ahora.
    let tvIncompatibles = 0;
    let tvFallas = 0;
    let tvMotivo = "";
    // Mejor fuente que resolvió pero NO trae español: respaldo si ninguna trae.
    let fallback: { url: string; s: Src; archivos: ArchivoPack[] } | null = null;
    // Candidatas a bajar en local: las que el debrid rechazó (el 451 es
    // cumplimiento legal de RD, no falta de seeds, así que suelen bajarse sin
    // problema) y, sin debrid vinculado, todas las que traen magnet.
    const rechazadas: Src[] = [];

    for (let i = startIdx; i < view.length && tried < MAX_TRIES; i++) {
      if (!vivo) return;
      const s = view[i];
      if (!s.magnet && !s.url) { dbg(`#${i} SKIP (sin magnet ni url)`); continue; }
      // La que ya baja en este equipo: del torrent de la sesión, sin debrid.
      if (s.local && !s.url) {
        if (await playViaTorrent(s)) return;
        if (torrentCancel) return;
        error = "";
        continue;
      }
      // Sin debrid no hay a quién pedirle que resuelva el magnet: la fuente
      // pasa directo a la lista de descarga local, sin gastar un intento.
      if (!rdLinked && !s.url) {
        if (s.magnet) rechazadas.push(s);
        dbg(`#${i} -> local (magnet sin debrid)`);
        continue;
      }
      const marca = marcaRd(s);
      if (marca) {
        if (elegida && i === startIdx) {
          resolving = false;
          // Está en el grupo de "se ven por torrent": un DMCA no se levanta y
          // lo que RD no tenía en caché tampoco va a estar, así que va derecho.
          if (s.magnet && (marca.tipo === "dmca" || marca.tipo === "sin_cache")) {
            await verPorTorrent(i, s);
            return;
          }
          avisoRd = { i, s, falla: marca };
          avisoRdFoco = 0;
          return;
        }
        // Sigue siendo candidata a bajarse en local: lo que falló es RD.
        if (s.magnet) rechazadas.push(s);
        marcadas++;
        dbg(`#${i} SKIP (RD ya no pudo: ${marca.corto})`);
        continue;
      }
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
        let url = s.url;
        // Capítulos del torrent si es un pack: el siguiente sale de ahí.
        let archivos: ArchivoPack[] = [];
        if (!url) {
          const r = await invoke<Resuelto>("rd_resolver", {
            magnet: s.magnet,
            pista: pistaDe(s, season ?? null, episode ?? null),
          });
          url = r.url;
          archivos = r.pack;
          olvidarFallaRd(hashDe(s), capRd);
          anotarOkRd(hashDe(s));
        }

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
            if (!fallback) fallback = { url, s, archivos };
            dbg(`#${i} sin español embebido → siguiente`);
            continue;
          }
        }
        recordarPack(clave, season ?? null, s, "rd", archivos);
        if (await ofrecerOpciones(url, s, elegida, sinEs)) return;
        await launch(url, s);
        return;
      } catch (e) {
        lastErr = String(e);
        // La TV no está / no alcanza a Kütral / el usuario canceló: probar
        // más versiones no cambia nada.
        if (lastErr.includes("TV_RED") || lastErr.includes("TV_CORTADO")) {
          resolving = false;
          error = lastErr.includes("TV_RED")
            ? lastErr.replace(/^.*TV_RED:\s*/, "")
            : "";
          return;
        }
        // No le sirve a la TV, pero es una fuente sana: no va a la descarga
        // local (bajarla no cambia que la TV no la pueda reproducir).
        if (lastErr.includes("TV_INCOMPATIBLE")) {
          tvIncompatibles++;
          tvMotivo ||= lastErr.replace(/^.*TV_INCOMPATIBLE:\s*/, "");
          resolvingMsg = `Saltando versiones que ${castTvNombre} no reproduce…`;
          continue;
        }
        if (lastErr.includes("TV_FALLA")) {
          tvFallas++;
          continue;
        }
        if (lastErr.includes("BLOQUEADO_DMCA")) blocked++;
        // Queda marcada en la lista (y no se vuelve a intentar sola).
        if (s.magnet && !s.url) {
          const f = anotarFallaRd(hashDe(s), capRd, lastErr);
          if (f) dbg(`#${i} RD no pudo: ${f.corto}`);
        }
        if (s.magnet) rechazadas.push(s);
        // sigue con la próxima fuente
      }
    }

    // Nada con español embebido entre lo inspeccionado: reproducir la mejor
    // que SÍ resolvió. Los subtítulos externos (Wyzie/OpenSubtitles) cubren.
    if (fallback) {
      dbg(`sin ES embebido en ${sinEs} fuentes → fallback a la mejor`);
      try {
        recordarPack(clave, season ?? null, fallback.s, "rd", fallback.archivos);
        if (await ofrecerOpciones(fallback.url, fallback.s, elegida, sinEs)) return;
        await launch(fallback.url, fallback.s);
        avisoSinIdioma();
        return;
      } catch (e) {
        lastErr = String(e);
      }
    }

    dbg(`fin RD: tried=${tried} blocked=${blocked} sinEs=${sinEs} lastErr=${lastErr.slice(0, 60)}`);

    // Descarga local: con debrid es el plan B (nada pasó, pero el torrent
    // sigue vivo); sin debrid es la vía principal. Acá el orden que importa es
    // OTRO: con debrid mandaba la caché de RD, bajando en local manda cuánta
    // gente está compartiendo el archivo.
    let pesadas = 0;
    // Con debrid, ninguna abrió en RD: no se baja nada por cuenta propia. Se
    // vuelve a la lista con las de torrent agrupadas y la de más gente
    // compartiendo ya enfocada; un Enter y se ve mientras baja.
    if (rdLinked && rechazadas.length && !destinoTv) {
      resolving = false;
      agrupar();
      const mejor = [...rechazadas].sort((a, b) => (b.seeders || 0) - (a.seeders || 0))[0];
      const iMejor = view.indexOf(mejor);
      if (iMejor >= 0) focusIdx = iMejor;
      avisoLista =
        `Ninguna de las ${tried} fuentes que se probaron abrió en Real-Debrid` +
        (blocked ? ` (${blocked} bloqueada${blocked > 1 ? "s" : ""} por DMCA)` : "") +
        `. Elige una de abajo para verla por torrent: empieza apenas haya buffer y sigue bajando mientras la ves.`;
      scrollFocused();
      return;
    }
    if (config.torrentLocal && rechazadas.length) {
      const aptas = rechazadas.filter((c) => {
        if (cabeEnLocal(c)) return true;
        pesadas++;
        return false;
      });
      const porSeeders = aptas.sort((a, b) => (b.seeders || 0) - (a.seeders || 0));
      // Sin debrid esta es la vía principal, así que se respeta la fuente que
      // el usuario marcó en la lista: va primero aunque otra tenga más seeds.
      const elegida = view[startIdx];
      const iSel = elegida ? porSeeders.indexOf(elegida) : -1;
      if (!rdLinked && iSel > 0) {
        porSeeders.splice(iSel, 1);
        porSeeders.unshift(elegida);
      }
      const tope = rdLinked ? MAX_TORRENT_TRIES : MAX_TORRENT_TRIES_SOLO;
      for (const cand of porSeeders.slice(0, tope)) {
        if (await playViaTorrent(cand)) return;
        if (torrentCancel) return; // el usuario canceló o lo mandó a la cola
      }
    }

    resolving = false;
    agrupar();
    if (error) return; // playViaTorrent ya dejó un mensaje más específico
    if (destinoTv && (tvIncompatibles || tvFallas)) {
      const partes = [
        tvIncompatibles ? `${tvIncompatibles} con formatos que no reproduce (${tvMotivo})` : "",
        tvFallas ? `${tvFallas} que no arrancaron` : "",
      ].filter(Boolean);
      error =
        `Ninguna versión sirvió para ${castTvNombre}: ${partes.join(" y ")}. ` +
        "Presiona T para verla en este equipo.";
      return;
    }
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
    } else if (marcadas > 0 && tried === 0) {
      error =
        `Real-Debrid ya no pudo con ${marcadas === 1 ? "la fuente que queda" : `las ${marcadas} fuentes que quedan`} ` +
        "(están marcadas ⛔). Elige una para verla por torrent, o prueba \"🔄 Rebuscar fuentes\".";
    } else if (!rdLinked && rechazadas.length) {
      error =
        "La descarga local no pudo arrancar ninguna fuente. Casi siempre es falta de " +
        'gente compartiendo: prueba otra calidad o "🔄 Rebuscar fuentes".';
    } else {
      error = `No se pudo reproducir ninguna fuente. ${lastErr}`;
    }
  }

  // ---- Grupos y oferta de las bloqueadas ------------------------------------

  // Con debrid, la lista va en dos grupos: lo que abre (o puede abrir) en RD y
  // lo que RD ya no pudo, que se ve por torrent. Se reordena solo en momentos
  // fijos (al cargar, al volver a la lista): reordenar mientras playFrom
  // recorre la lista le correría los índices.
  function agrupar() {
    if (!rdLinked) return;
    const abren = sources.filter((x) => !marcaRd(x));
    const no = sources.filter((x) => marcaRd(x));
    if (abren.length && no.length) sources = [...abren, ...no];
  }

  const hayGrupos = $derived(rdLinked && view.some((x) => marcaRd(x)) && view.some((x) => !marcaRd(x)));

  // Las DMCA que se pueden ver por torrent, sin contar la que RD abrió.
  function bloqueadasDmca(sin: Src): Src[] {
    return view.filter((x) => x !== sin && x.magnet && marcaRd(x)?.tipo === "dmca");
  }

  // Hashes que el sondeo ya vio pasar en RD: volver a la lista y a Descubrir
  // no lo repite.
  const sondeadas = new Set<string>();
  const MAX_SONDEO = 15;

  // ¿Cuáles de las otras bloquea RD por DMCA? Sin esto solo se sabía de las
  // que se habían intentado, y la búsqueda se para en la primera que abre: la
  // oferta casi nunca salía. Las bloqueadas quedan anotadas en rdFallas (chip
  // y grupo "por torrent") igual que si se hubieran intentado.
  async function sondearDmca(sin: Src) {
    if (!rdLinked) return;
    const cand = view
      .filter((x) => x !== sin && x.magnet && !x.url && !x.local && !marcaRd(x))
      .filter((x) => {
        const h = hashDe(x);
        return !!h && !okRd(h) && !sondeadas.has(h.toLowerCase());
      })
      .slice(0, MAX_SONDEO);
    if (!cand.length) return;
    try {
      const r = await invoke<boolean[]>("rd_sondear_dmca", { magnets: cand.map((x) => x.magnet) });
      cand.forEach((x, k) => {
        if (r[k]) anotarFallaRd(hashDe(x), capRd, "BLOQUEADO_DMCA: sondeo");
        else sondeadas.add(hashDe(x)!.toLowerCase());
      });
      dbg(`sondeo DMCA: ${r.filter(Boolean).length}/${cand.length} bloqueadas`);
    } catch (e) {
      dbg(`sondeo DMCA falló (no bloquea): ${String(e).slice(0, 60)}`);
    }
  }

  // true = quedó la oferta en pantalla (el que llama no lanza). No se ofrece
  // si el usuario eligió esa fuente a mano, ni al pasar de la TV al equipo.
  // `revisadas`: cuántas se inspeccionaron sin el idioma (verifyEsTracks).
  async function ofrecerOpciones(url: string, s: Src, elegida: boolean, revisadas: number): Promise<boolean> {
    if (elegida || desdeForzado != null || !vivo) return false;
    resolvingMsg = "Revisando las otras versiones…";
    // Sin la verificación activada no se inspeccionó nada: se mira al menos
    // la que va a sonar (el hardsub no expone pistas, ver playFrom).
    const inspeccionar = s._es === undefined && !s.hardsub && !s.local;
    await Promise.all([
      sondearDmca(s),
      inspeccionar ? probeEs(url).then((v) => (s._es = v)) : null,
    ]);
    if (!vivo) return false;
    const n = bloqueadasDmca(s).length;
    const sinIdioma = s._es === "none";
    if (!n && !sinIdioma) return false;
    resolving = false;
    oferta = { url, s, n, sinIdioma, revisadas: Math.max(revisadas, 1) };
    ofertaFoco = 0;
    ofertaSeg = OFERTA_SEG;
    if (ofertaTimer) clearInterval(ofertaTimer);
    ofertaTimer = setInterval(() => {
      ofertaSeg--;
      if (ofertaSeg <= 0) void accionOferta("ver");
    }, 1000);
    dbg(`oferta: ${s.title.slice(0, 40)} sinIdioma=${sinIdioma}, ${n} DMCA para torrent`);
    return true;
  }

  function avisoSinIdioma() {
    void mpv(["show-text", `Sin ${idiomaProbe().texto} en el archivo: usando subtítulos externos`, "4000"]);
  }

  async function accionOferta(id: "ver" | "otras") {
    const o = oferta;
    if (ofertaTimer) clearInterval(ofertaTimer);
    ofertaTimer = null;
    if (!o) return;
    oferta = null;
    if (id === "otras") {
      // A la lista, parado en la primera bloqueada (o en otra que no se sepa
      // sin el idioma): ahí se elige cuál.
      agrupar();
      const otra = bloqueadasDmca(o.s)[0] ?? view.find((x) => x !== o.s && x._es !== "none");
      const i = otra ? view.indexOf(otra) : -1;
      if (i >= 0) focusIdx = i;
      avisoLista = [
        o.sinIdioma ? `La que abrió no trae ${idiomaProbe().texto}.` : "",
        o.n
          ? "Las bloqueadas por DMCA en Real-Debrid se ven por torrent: elige una."
          : "Elige otra de la lista.",
      ]
        .filter(Boolean)
        .join(" ");
      scrollFocused();
      return;
    }
    resolving = true;
    resolvingMsg = "Abriendo reproductor…";
    try {
      await launch(o.url, o.s);
      if (o.sinIdioma) avisoSinIdioma();
    } catch (e) {
      error = String(e);
    } finally {
      resolving = false;
    }
  }

  // Por torrent, pedida a mano: vale aunque la descarga local no esté
  // activada (es lo único que abre la sesión torrent sin esa opción).
  async function verPorTorrent(i: number, s: Src) {
    focusIdx = i;
    avisoLista = "";
    if (!(await playViaTorrent(s)) && !error && !torrentCancel) {
      error = "El torrent no arrancó: casi siempre es falta de gente compartiéndolo. Prueba otra fuente.";
    }
  }

  // ---- RD ya no pudo con la fuente elegida ----------------------------------

  type BotonRd = { id: "torrent" | "reintentar" | "otra"; label: string };
  const botonesRd = $derived<BotonRd[]>(
    avisoRd
      ? [
          ...(avisoRd.s.magnet ? [{ id: "torrent" as const, label: "🧲 Verla por torrent" }] : []),
          { id: "reintentar", label: "🔁 Probar RD otra vez" },
          { id: "otra", label: "↩ Elegir otra" },
        ]
      : [],
  );

  async function accionRd(id: BotonRd["id"]) {
    const a = avisoRd;
    if (!a) return;
    avisoRd = null;
    if (id === "otra") return;
    if (id === "reintentar") {
      olvidarFallaRd(hashDe(a.s), capRd);
      await playFrom(a.i);
      return;
    }
    await verPorTorrent(a.i, a.s);
  }

  // ---- La red no da ---------------------------------------------------------

  // El vigilante concluyó que esto no se puede ver en vivo. Se corta el video
  // (con el archivo cortándose cada diez segundos igual no se veía) y se
  // ofrecen las dos salidas honestas.
  async function redNoDa(m: MotivoRendicion, url: string, s: Src) {
    dbg(`red no da (${m.tipo}): necesita ${m.necesario} B/s, mide ${m.medido} B/s`);
    rendicionSrc = { url, s };
    rendicion = m;
    // El cierre de mpv emite "mpv:state"=false, que por defecto cierra el
    // picker entero. Esta bandera dice que no: la pantalla que sigue es esta.
    cambiandoFuente = true;
    stopMpvPoll();
    try {
      await invoke("mpv_stop");
    } catch {
      /* ya estaba cerrado */
    }
    playing = false;
    desdeTorrent = false;
  }

  /** "Bajarla y verla después": a la cola y a otra cosa. */
  async function rendicionBajar() {
    const pos = rendicion?.pos ?? 0;
    rendicion = null;
    const n = await encolarPendiente([
      {
        clave,
        season,
        episode,
        title,
        etiqueta: season != null && episode != null ? `T${season} E${episode}` : "",
        imdbId,
        kind,
        originalTitle,
        kitsuId,
      },
    ]);
    notify(
      "info",
      n ? "En la cola" : "Ya estaba pedida",
      `${title} — te avisamos cuando esté lista${pos > 60 ? " (retoma donde ibas)" : ""}.`,
    );
    onClose();
  }

  /** "Seguir igual": retoma donde iba, con la espera en el tope y sin más avisos. */
  async function rendicionSeguir() {
    const m = rendicion;
    const r = rendicionSrc;
    rendicion = null;
    if (!r) return;
    sinRendirse = true;
    resolving = true;
    resolvingMsg = "Volviendo al video…";
    try {
      await launch(r.url, r.s, m?.pos ?? 0);
    } catch (e) {
      error = `No se pudo retomar: ${String(e)}`;
      resolving = false;
    }
  }

  // ---- Bajar para después --------------------------------------------------

  // Encola la descarga y vuelve: acá NO se reproduce nada. El aviso de que
  // terminó lo da el poll de la cola (torrents.svelte.ts) y a partir de ahí la
  // ficha muestra "ya está en tu equipo".
  async function encolar(s: Src) {
    if (!s.magnet) {
      error = "Esa fuente es un enlace directo, no un torrent: no se puede bajar.";
      return;
    }
    encolando = true;
    error = "";
    try {
      // Sin buffer de arranque: nadie está esperando para ver. El cliente baja
      // igual de forma secuencial, así que si después la abres a mitad de
      // descarga el inicio ya va a estar.
      const added = await addTorrent(
        s.magnet,
        title,
        config.torrentBufferMb,
        pistaDe(s, season ?? null, episode ?? null),
      );
      await registrarDescarga({
        clave,
        season,
        episode,
        infoHash: added.info_hash,
        ruta: added.path,
        releaseTitle: s.title || added.name,
        quality: s.quality,
        sizeBytes: added.size_bytes,
      });
      startQueuePoll();
      notify(
        "info",
        "Bajando para después",
        `${title} · ${QLABEL[s.quality] || ""} · ${fmtBytes(added.size_bytes)}`,
      );
      dbg(`encolada ${added.name} (${added.size_bytes} B)`);
      onClose();
    } catch (e) {
      encolando = false;
      dbg(`encolar falló: ${String(e).slice(0, 100)}`);
      error = `No se pudo encolar: ${String(e)}`;
    }
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
    torrentReutilizado = false;
    torrentHash = "";
    torrentMsg = "Buscando peers…";
    const t0 = Date.now();
    // El torrent ya estaba (otro capítulo del pack): si este falla, se queda.
    let reutilizado = false;
    let hashNuevo = "";
    try {
      const added = await addTorrent(
        s.magnet,
        title,
        config.torrentBufferMb,
        pistaDe(s, season ?? null, episode ?? null),
      );
      reutilizado = added.reutilizado;
      torrentReutilizado = reutilizado;
      hashNuevo = torrentHash = added.info_hash;
      if (torrentCancel) return false;
      torrentId = added.id;
      // Queda anotado a QUÉ título pertenece este archivo. Se registra ya (con
      // completa=0), no al terminar: si la app se cierra a mitad, la fila
      // existe y el poll de la cola la marca completa cuando llegue al final.
      void registrarDescarga({
        clave,
        season,
        episode,
        infoHash: added.info_hash,
        ruta: added.path,
        releaseTitle: s.title || added.name,
        quality: s.quality,
        sizeBytes: added.size_bytes,
      });
      dbg(`torrent local id=${added.id} ${added.name} (${added.size_bytes} B)`);
      archivoTorrent = added.path || added.name || "";
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
      recordarPack(clave, season ?? null, s, "local", added.pack);
      await launch(added.stream_url, s);
      notify(
        "warn",
        "Reproduciendo desde descarga local",
        rdLinked
          ? "El debrid bloqueó esta fuente por DMCA. La estás bajando y compartiendo tú."
          : "Sin debrid: la estás bajando y compartiendo tú. Tu IP queda visible para el resto del torrent.",
      );
      void mpv([
        "show-text",
        rdLinked
          ? "Descarga local: el debrid bloqueó esta fuente"
          : "Descarga local: sin debrid",
        "5000",
      ]);
      return true;
    } catch (e) {
      torrenting = false;
      dbg(`torrent local falló: ${String(e).slice(0, 100)}`);
      error = `Descarga local: ${String(e)}`;
      // No dejar el torrent muerto ocupando la cola y el disco. Salvo que ya
      // estuviera: lleva los otros capítulos del pack.
      const id = torrentId;
      torrentId = null;
      if (id !== null && !reutilizado) {
        try {
          await removeTorrent(id, true);
        } catch {
          /* ya no estaba */
        }
      }
      // Se borró el archivo: no hay copia local de este título que ofrecer.
      // Solo la fila de ESTE torrent, y solo si se borró: si ya estaba en la
      // sesión sigue bajando y la fila sigue valiendo.
      if (hashNuevo && !reutilizado) {
        void olvidarDescarga(clave, season ?? -1, episode ?? -1, hashNuevo);
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
    // Ya bajaba de antes (la descarga de este título, u otro capítulo del
    // pack): cancelar la espera no es tirar lo bajado. Queda en la cola.
    if (id !== null && torrentReutilizado) {
      startQueuePoll();
      return;
    }
    if (id !== null) {
      const hash = torrentHash || undefined;
      try {
        await removeTorrent(id, true);
      } catch {
        /* si ya no existe, da igual */
      }
      void olvidarDescarga(clave, season ?? -1, episode ?? -1, hash);
    }
  }

  function startMpvPoll() {
    stopMpvPoll();
    mpvPoll = setInterval(async () => {
      try {
        const alive = await invoke<boolean>("mpv_running");
        if (!alive) {
          stopMpvPoll();
          if (!cambiandoFuente) onClose();
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

  // ¿La pista de audio es español? (el doblaje de "Doblado" es al español).
  const isEsTrack = (t: MpvTrack) => pistaDelIdioma(t, "es");

  // NIVELES 2-3: la verdad del contenedor. Lee las pistas reales de mpv y elige
  // según la preferencia. Solo baja subs externos si el archivo no trae una
  // pista COMPLETA en el idioma de Configuración.
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
    const lang = config.subsLang || "es";

    if (config.subMode === "dub" && esAudio) {
      // Audio español embebido: selecciónalo y oculta subtítulos.
      await mpv(["set", "aid", String(esAudio.id)]);
      await mpv(["set", "sub-visibility", "no"]);
      dbg(`audio ES embebido id=${esAudio.id}`);
      return;
    }
    // "Sin subtítulos": ni externos ni los que traiga el archivo (mpv puede
    // activar uno solo, por la marca "default" del contenedor).
    if (lang === "off") {
      await mpv(["set", "sid", "no"]);
      dbg("subtítulos apagados en Configuración");
      return;
    }
    // Subtitulado (o doblado sin audio ES): audio original + subs del idioma.
    const { completa, forzada } = elegirPistaSub(subs, lang);
    if (completa) {
      await mpv(["set", "sid", String(completa.id)]);
      await mpv(["set", "sub-visibility", "yes"]);
      dbg(`sub ${lang} embebido id=${completa.id}`);
      return;
    }
    // No trae una completa en el idioma → externo (Wyzie o OpenSubtitles).
    // Una forzada solo traduce carteles: no alcanza para ver la película,
    // pero es mejor que nada si no aparece un externo.
    if (await loadExternalSub()) return;
    if (forzada) {
      await mpv(["set", "sid", String(forzada.id)]);
      await mpv(["set", "sub-visibility", "yes"]);
      dbg(`solo forzado ${lang} embebido id=${forzada.id}`);
    }
  }

  // Series: sin capítulo, las APIs devuelven subtítulos de cualquier episodio.
  function capituloSubs() {
    return season != null && episode != null ? { season, episode } : {};
  }

  // URL de un subtítulo externo. "" = no hay. Lista Wyzie + OpenSubtitles
  // (listar no gasta cuota), ordenada para que primero vaya el que calza con
  // esta fuente, y solo resuelve el enlace del elegido.
  async function buscarSubUrl(): Promise<{ url: string; key: string }> {
    const nada = { url: "", key: "" };
    const lang = config.subsLang || "es";
    if (lang === "off") return nada;
    const { release, videoUrl } = player.nowPlaying;
    const elegido = mejorSubtitulo(
      await buscarSubtitulos(imdbId, capituloSubs(), { release, videoUrl }),
    );
    if (!elegido) return nada;
    try {
      const url = await enlaceSub(elegido);
      dbg(`sub externo: ${elegido.label.slice(0, 70)}`);
      return { url, key: claveSub(elegido) };
    } catch (e) {
      dbg(`sub externo fail: ${String(e).slice(0, 80)}`);
      return nada;
    }
  }

  // Subtítulos externos en mpv. Carga la URL directo con sub-add (no hace
  // falta bajar a disco).
  async function loadExternalSub(): Promise<boolean> {
    const lang = config.subsLang || "es";
    const { url, key } = await buscarSubUrl();
    if (!url) {
      dbg(`sin subtítulos ${lang} (embebidos ni externos)`);
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
    // Antes del sub-add: así la pista ya tiene identidad cuando el
    // reproductor la ve aparecer y busca su sincronía recordada.
    if (key) await invoke("mpv_sub_key", { path: toLoad, key }).catch(() => {});
    await mpv(["sub-add", toLoad, "select"]);
    await mpv(["set", "sub-visibility", "yes"]);
    return true;
  }

  async function stopMpv() {
    stopMpvPoll();
    detenerVigilancia();
    try {
      await invoke("mpv_stop");
    } catch {
      /* ignore */
    }
    onClose();
  }

  $effect(() => () => {
    stopMpvPoll();
    detenerVigilancia();
  });

  // Con libmpv embebido el video se cierra desde DENTRO (Esc en el reproductor
  // → backend hace stop() y emite "mpv:state"=false). El webview estuvo oculto
  // y su timer de poll pudo quedar suspendido, así que NO dependemos del poll:
  // escuchamos el evento y volvemos al menú al recibir false.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen<boolean>("mpv:state", (e) => {
      if (e.payload === false) {
        stopMpvPoll();
        if (soltandoVideo) {
          soltandoVideo = false;
          return;
        }
        if (terminoSolo) {
          terminoSolo = false;
          return; // la página ya decidió: menú o siguiente capítulo
        }
        if (cambiandoFuente) {
          cambiandoFuente = false;
          volverALista();
          return;
        }
        onClose();
      }
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  // Fin de reproducción: el backend avisa antes de cerrar. Dos casos distintos:
  //   - terminó de verdad → decide la página (menú o siguiente capítulo);
  //   - se cortó a mitad (stream muerto) → esta fuente no sirve, se vuelve a la
  //     lista con ella marcada para elegir otra.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen<FinPayload>("mpv:fin", (e) => {
      terminoSolo = true;
      stopMpvPoll();
      if (!esFinReal(e.payload)) {
        dbg("corte a mitad: vuelvo a la lista de fuentes");
        volverALista();
        error = "Esa fuente se cortó antes de terminar. Prueba con otra.";
      }
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  // "Cambiar fuente" desde el reproductor: vuelve a la lista con la fuente que
  // estaba sonando marcada, para no volver a elegir la misma sin querer.
  $effect(() => {
    let un: UnlistenFn | undefined;
    void listen("player:cambiar-fuente", () => {
      cambiandoFuente = true;
      stopMpvPoll();
      volverALista();
    }).then((u) => (un = u));
    return () => {
      if (un) un();
    };
  });

  function volverALista() {
    detenerVigilancia();
    // Otra fuente es otra red y otro peso: la decisión anterior no la ata.
    sinRendirse = false;
    if (playing && playingTitle && !descartadas.includes(playingTitle)) {
      descartadas = [...descartadas, playingTitle];
    }
    playing = false;
    playingTitle = "";
    desdeTorrent = false;
    desdeLocal = false;
    resolving = false;
    resolvingMsg = "";
    setTimeout(() => scrollFocused(), 50);
  }

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
    if (!view[focusIdx]) return;
    if (descargarSolo) return void encolar(view[focusIdx]);
    void playFrom(focusIdx, true);
  }

  function scrollFocused() {
    queueMicrotask(() => {
      document
        .querySelector<HTMLElement>(".sp-row.focused, .sp-foot-btn.focused")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function onKey(e: KeyboardEvent) {
    if (casting) {
      const n = castBtns.length;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        castFoco = (castFoco + 1) % n;
        scrollFocused();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        castFoco = (castFoco - 1 + n) % n;
        scrollFocused();
      } else if (e.key === "Enter") {
        e.preventDefault();
        void castAccion(castBtns[castFoco].id);
      } else if (e.key === " ") {
        e.preventDefault();
        if (tvTermino == null) void castAccion("pausa");
      } else if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        if (subsTv != null) {
          subsTv = null;
          castFoco = 0;
        } else if (menuTv != null) {
          menuTv = null;
          castFoco = 0;
        } else void castAccion("navegar");
      }
      return;
    }
    if (aTv) {
      e.preventDefault();
      if (aTvBuscando) {
        if (e.key === "Escape" || e.key === "Backspace") castCancel = true;
      } else if (["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"].includes(e.key)) {
        aTvFoco = 1 - aTvFoco;
      } else if (e.key === "Enter") {
        void aTvAccion(aTvFoco === 0 ? "transmitir" : "aqui");
      } else if (e.key === "Escape" || e.key === "Backspace") {
        void aTvAccion("aqui");
      }
      return;
    }
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
    // Encolando: addTorrent está esperando la metadata del magnet y no se puede
    // cortar a mitad. Se ignora el teclado hasta que responda.
    if (encolando) return;
    if (avisoRd) {
      e.preventDefault();
      const n = botonesRd.length;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") avisoRdFoco = (avisoRdFoco + 1) % n;
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp") avisoRdFoco = (avisoRdFoco - 1 + n) % n;
      else if (e.key === "Enter" || e.key === " ") void accionRd(botonesRd[avisoRdFoco].id);
      else if (e.key === "Escape" || e.key === "Backspace") void accionRd("otra");
      return;
    }
    if (oferta) {
      e.preventDefault();
      if (["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp"].includes(e.key)) ofertaFoco = 1 - ofertaFoco;
      else if (e.key === "Enter" || e.key === " ") void accionOferta(ofertaFoco === 0 ? "ver" : "otras");
      else if (e.key === "Escape" || e.key === "Backspace") void accionOferta("otras");
      return;
    }
    if (rendicion) {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        void rendicionBajar();
      } else if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        rendicion = null;
        volverALista();
      }
      return;
    }
    if (resolving) {
      if (e.key === "Escape" || e.key === "Backspace") {
        e.preventDefault();
        resolving = false; // cancela visualmente; el resolve sigue en backend
        castCancel = true; // si la TV estaba cargando, se corta
      }
      return;
    }
    if ((e.key === "t" || e.key === "T") && !descargarSolo) {
      e.preventDefault();
      toggleDestino();
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
        {#if descargarSolo}Elige qué versión bajar
        {:else if famFilter === "hardsub"}Enlace directo
        {:else if famFilter === "torrent"}Torrents vía debrid
        {:else}Elige una fuente{/if}
      </span>
      {#if cast.tv && !descargarSolo}
        <button
          class="sp-dest"
          class:tv={destinoTv}
          onclick={toggleDestino}
          title="Cambiar dónde se ve (tecla T)"
        >
          {destinoTv ? `📺 En ${cast.tv.nombre}` : "💻 En este equipo"}
          <kbd>T</kbd>
        </button>
      {/if}
    </header>

    {#if casting}
      {@const st = cast.estado}
      {@const pct = st && st.duracion > 0 ? Math.min(100, (st.pos * 100) / st.duracion) : 0}
      <div class="sp-center">
        {#if tvTermino != null}
          <p class="sp-playing">📺 La transmisión en {castTvNombre} terminó</p>
          <p class="sp-playing-title">{playingTitle}</p>
          <p class="sp-tor-line">Iba en {fmtTiempo(tvTermino)}</p>
        {:else}
        <p class="sp-playing">📺 En {castTvNombre}</p>
        <p class="sp-playing-title">{playingTitle}</p>
        <button
          class="sp-bar sp-cast-bar"
          aria-label="Saltar a otro minuto"
          title="Clic para saltar a ese minuto"
          onclick={saltarConBarra}
        ><div class="sp-bar-fill" style="width: {pct}%"></div></button>
        <p class="sp-tor-line">
          {#if !st || st.estado === "LOADING" || st.estado === "BUFFERING"}Cargando…
          {:else if st.estado === "PAUSED"}En pausa · {fmtTiempo(st.pos)} de {fmtTiempo(st.duracion)}
          {:else if st.estado === "SIN_CONEXION"}La TV no responde…
          {:else}{fmtTiempo(st.pos)} de {fmtTiempo(st.duracion)}{/if}
        </p>
        {#if audiosTv.length > 1 && audiosTv[castAudio - 1]}
          <p class="sp-tor-line">🎧 Audio: {etiquetaAudio(audiosTv[castAudio - 1], castAudio - 1)}</p>
        {/if}
        {#if subsTvActual}
          <p class="sp-tor-line">💬 Subtítulos: {subsTvActual}</p>
        {/if}
        {#if st && st.estado !== "LOADING"}
          <p class="sp-tor-meta">
            {st.silencio ? "🔇 En silencio" : `🔊 Volumen ${Math.round(st.volumen * 100)}%`}
          </p>
        {/if}
        {#if st?.aviso === "SUBS_SIN_ACCESO"}
          <p class="sp-tor-slow">
            Los subtítulos no llegan a la TV: revisa el firewall en Configuración → Transmitir a la TV.
          </p>
        {/if}
        {/if}
        {#if subsTv != null}
          <p class="sp-tor-line">Elige subtítulos · la TV retoma en el mismo minuto</p>
        {:else if menuTv === "audio"}
          <p class="sp-tor-line">Elige el audio · la TV retoma en el mismo minuto</p>
        {:else if menuTv === "sync"}
          <p class="sp-tor-line">
            Subtítulos {corrimientoTxt(syncPendiente ?? castDesfase)}{syncPendiente != null
              ? " · se aplica al dejar de tocar"
              : ""}
          </p>
        {/if}
        <div class="sp-cast-btns" class:sp-cast-subs={subsTv != null}>
          {#each castBtns as b, i (b.id)}
            <button
              class="sp-foot-btn"
              class:focused={castFoco === i}
              onclick={() => void castAccion(b.id)}
              onmouseenter={() => (castFoco = i)}
            >{b.label}</button>
          {/each}
        </div>
        {#if tvTermino == null}
          <span class="sp-hint">
            Espacio pausa · Esc sigue navegando (la película sigue en la TV)
          </span>
        {/if}
      </div>
    {:else if aTv}
      <div class="sp-center">
        <p class="sp-playing">📺 Pasar a {cast.tv?.nombre ?? "la TV"}</p>
        <p class="sp-playing-title">{playingTitle}</p>
        {#if aTvBuscando}
          <p class="sp-tor-line">{resolvingMsg}</p>
          <span class="sp-hint">Esc cancela y sigues viendo aquí</span>
        {:else}
          <p class="sp-tor-line">
            Sigue en {fmtTiempo(aTv.info.pos)} con el audio y los subtítulos que tienes puestos.
          </p>
          <ul class="sp-tv-avisos">
            {#each aTv.avisos as a (a)}<li>{a}</li>{/each}
          </ul>
          <div class="sp-cast-btns">
            <button
              class="sp-foot-btn"
              class:focused={aTvFoco === 0}
              onclick={() => void aTvAccion("transmitir")}
              onmouseenter={() => (aTvFoco = 0)}
            >📺 Transmitir</button>
            <button
              class="sp-foot-btn"
              class:focused={aTvFoco === 1}
              onclick={() => void aTvAccion("aqui")}
              onmouseenter={() => (aTvFoco = 1)}
            >💻 Seguir aquí</button>
          </div>
        {/if}
      </div>
    {:else if playing}
      <div class="sp-center">
        <p class="sp-playing">▶ Reproduciendo</p>
        <p class="sp-playing-title">{playingTitle}</p>
        {#if desdeLocal}
          <p class="sp-desde-disco">
            📁 Desde tu equipo — ya la tenías bajada. No se usó internet.
          </p>
        {:else if desdeTorrent}
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
    {:else if avisoRd}
      {@const a = avisoRd}
      <div class="sp-center">
        <p class="sp-tor-head">⛔ Real-Debrid no pudo con esta fuente</p>
        <p class="sp-playing-title">{a.s.title}</p>
        <p class="sp-tor-why">
          Ya se intentó: {a.falla.motivo}. Por eso quedó marcada y no se vuelve
          a probar sola.
          {#if a.s.magnet}
            Por torrent se baja directo de la gente que la comparte, sin pasar
            por RD (se baja y se comparte desde tu equipo).
          {/if}
        </p>
        <div class="sp-cast-btns">
          {#each botonesRd as b, k (b.id)}
            <button
              class="sp-foot-btn"
              class:focused={avisoRdFoco === k}
              onclick={() => void accionRd(b.id)}
              onmouseenter={() => (avisoRdFoco = k)}
            >{b.label}</button>
          {/each}
        </div>
      </div>
    {:else if oferta}
      {@const o = oferta}
      {@const idioma = idiomaProbe().texto}
      <div class="sp-center">
        <p class="sp-tor-head">{o.sinIdioma ? `🔎 Sin ${idioma} en esta versión` : "⚡ Lista en Real-Debrid"}</p>
        <p class="sp-playing-title">{o.s.title}</p>
        {#if o.sinIdioma}
          <p class="sp-tor-why">
            {o.revisadas > 1 ? `Ninguna de las ${o.revisadas} versiones revisadas trae` : "No trae"}
            audio ni subtítulos en {idioma} dentro del archivo. Si la ves igual, se buscan
            subtítulos aparte.
          </p>
        {/if}
        {#if o.n}
          <p class="sp-tor-why">
            {o.n === 1 ? "Otra versión quedó bloqueada" : `Otras ${o.n} versiones quedaron bloqueadas`}
            por DMCA en Real-Debrid. Por torrent sí se pueden ver, por si alguna trae
            mejor audio o subtítulos.
          </p>
        {/if}
        <div class="sp-cast-btns">
          <button
            class="sp-foot-btn"
            class:focused={ofertaFoco === 0}
            onclick={() => void accionOferta("ver")}
            onmouseenter={() => (ofertaFoco = 0)}
          >▶ Ver esta ({ofertaSeg})</button>
          <button
            class="sp-foot-btn"
            class:focused={ofertaFoco === 1}
            onclick={() => void accionOferta("otras")}
            onmouseenter={() => (ofertaFoco = 1)}
          >{o.n ? "🧲 Ver las bloqueadas" : "📋 Ver las otras opciones"}</button>
        </div>
        <span class="sp-hint">Arranca sola en {ofertaSeg} s · Esc para ver la lista</span>
      </div>
    {:else if rendicion}
      {@const m = rendicion}
      <div class="sp-center">
        <p class="sp-tor-head">🐢 La red no da para verla en vivo</p>
        <p class="sp-tor-why">
          {#if m.tipo === "caudal"}
            Este archivo pide {fmtBps(m.necesario)} y la conexión está dando
            {fmtBps(m.medido)}. Con esa diferencia ningún buffer alcanza: se
            cortaría igual, solo que más espaciado.
          {:else}
            Ya subimos el buffer al máximo ({config.cacheWaitMax} s) y sigue
            cortándose.
          {/if}
          {#if desdeTorrent}
            Puede ser que este torrent tenga poca gente compartiéndolo.
          {/if}
        </p>
        <button class="sp-foot-btn focused" onclick={() => void rendicionBajar()}>
          📥 Bajarla y verla después
        </button>
        <button class="sp-foot-btn" onclick={() => void rendicionSeguir()}>
          ▶ Seguir igual, con cortes
        </button>
        <button class="sp-foot-btn" onclick={volverALista}>
          🔄 Probar otra fuente
        </button>
        <span class="sp-hint">
          Bajarla la deja lista para verse sin internet, retomando donde ibas.
        </span>
      </div>
    {:else if encolando}
      <div class="sp-center">
        <span class="sp-spinner"></span>
        <p class="sp-tor-head">📥 Preparando la descarga</p>
        <p class="sp-tor-why">
          Buscando peers del torrent. Cuando termine te avisamos y queda lista
          para verla sin internet.
        </p>
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
      {:else if avisoLista}
        <p class="sp-baja-hint">{avisoLista}</p>
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

      {#if descargarSolo}
        <p class="sp-baja-hint">
          📥 Se baja de fondo: puedes seguir usando la app. Tope actual:
          {QLABEL[config.torrentMaxQuality]} y {config.torrentMaxGb} GB.
        </p>
      {/if}

      <div class="sp-list">
        {#each view as s, i (s.info_hash || s.url || i)}
          {#if hayGrupos && i === 0 && !marcaRd(s)}
            <p class="sp-grupo">⚡ Real-Debrid</p>
          {:else if hayGrupos && marcaRd(s) && !marcaRd(view[i - 1])}
            <p class="sp-grupo">🧲 Real-Debrid no las abre · se ven por torrent</p>
          {/if}
          <button
            data-nav
            class="sp-row"
            class:focused={focusIdx === i}
            class:probada={descartadas.includes(s.title)}
            onclick={() => {
              focusIdx = i;
              if (descargarSolo) void encolar(s);
              else void playFrom(i, true);
            }}
            onmouseenter={() => (focusIdx = i)}
          >
            <span class="sp-q sp-q-{s.quality}">{QLABEL[s.quality] || "—"}</span>
            <div class="sp-mid">
              <span class="sp-title">{s.title}</span>
              <div class="sp-chips">
                {#if descartadas.includes(s.title)}<span class="sp-probada">↩ Ya probada</span>{/if}
                {#if marcaRd(s)}{@const f = marcaRd(s)}<span class="sp-es-no" title="Real-Debrid: {f?.motivo}">⛔ RD: {f?.corto}</span>{/if}
                {#if s.local}<span class="sp-en-disco">📁 En tu equipo</span>{/if}
                {#if prefScore(s, kind)}<span class="sp-pref">★ Preferida</span>{/if}
                {#if s.rd_cached}<span class="sp-cached">⚡ Instantáneo</span>{/if}
                {#if s.hardsub}<span class="sp-hardsub">{HARDSUB_LABEL[s.hardsub] || "🔗 Enlace directo"}</span>{/if}
                {#if s._es === "audio"}<span class="sp-es-ok">🗣 Audio {idiomaSubs().toUpperCase()} ✓</span>
                {:else if s._es === "subs"}<span class="sp-es-ok">💬 Subs {idiomaSubs().toUpperCase()} ✓</span>
                {:else if s._es === "none"}<span class="sp-es-no">Sin {idiomaProbe().texto}</span>{/if}
                {#if destinoTv && s._tv}<span class="sp-es-no" title={s._tv}>📺✗ {s._tv}</span>{/if}
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
  .sp-grupo {
    margin: 10px 4px 4px;
    color: #b59770;
    font-size: 12px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
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
  .sp-baja-hint {
    margin: 0 0 8px; color: #9a9aa6; font-size: 12px;
  }
  .sp-desde-disco {
    margin: 0; max-width: 460px; text-align: center;
    color: #8fe3a8; font-size: 12.5px; line-height: 1.45;
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
  /* Fuente que ya se vio y se cambió desde el reproductor: se atenúa para no
     volver a caer en la misma, pero sigue elegible. */
  .sp-row.probada {
    opacity: 0.55;
  }
  .sp-row.probada.focused {
    opacity: 1;
  }
  .sp-probada {
    font-size: 10.5px;
    font-weight: 700;
    color: #e0c08a;
    background: #33291c;
    border: 1px solid #6b5227;
    padding: 2px 7px;
    border-radius: 5px;
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
  /* Copia ya bajada: verde, el único chip que promete cero espera. */
  .sp-en-disco {
    font-size: 10.5px;
    font-weight: 700;
    color: #8fe3a8;
    background: #102417;
    border: 1px solid #27512f;
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
  .sp-dest {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: rgba(20, 20, 28, 0.62);
    border: 1px solid rgba(255, 255, 255, 0.14);
    color: #c8c8d0;
    border-radius: 999px;
    padding: 6px 12px;
    font-size: 12.5px;
    cursor: pointer;
  }
  .sp-dest.tv {
    border-color: #f3a951;
    color: #fff;
    background: rgba(243, 169, 81, 0.16);
  }
  .sp-dest kbd {
    font: inherit;
    font-size: 10.5px;
    color: #8a8a96;
    border: 1px solid #3a3a46;
    border-radius: 4px;
    padding: 0 5px;
  }
  .sp-cast-bar {
    width: min(460px, 80vw); height: 10px; padding: 0; border: 0;
    display: block; cursor: pointer;
  }
  .sp-tv-avisos {
    margin: 2px 0 6px; max-width: 520px; padding-left: 1.2em;
    text-align: left; color: #9a9aa6; font-size: 13px; line-height: 1.5;
  }
  .sp-cast-btns {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
    max-width: 720px;
  }
  /* Lista de subtítulos: una columna con scroll, los nombres de release son largos. */
  .sp-cast-btns.sp-cast-subs {
    flex-direction: column;
    flex-wrap: nowrap;
    align-items: stretch;
    width: min(720px, 90vw);
    max-height: 46vh;
    overflow-y: auto;
    padding: 4px;
  }
  .sp-cast-subs .sp-foot-btn {
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex-shrink: 0;
  }
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
