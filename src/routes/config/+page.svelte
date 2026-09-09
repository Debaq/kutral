<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { setNowPlaying } from "$lib/playerState.svelte";
  import { limpiarContexto } from "$lib/historial.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getVersion } from "@tauri-apps/api/app";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import {
    config,
    loadConfig,
    saveConfig,
    refreshRdLinked,
    refreshOsStatus,
    LANGS,
    SUB_LANGS,
    GAME_REGIONS,
    SCREENING_MIN,
    SCREENING_MAX,
    TORRENT_BUFFER_DEFAULT,
    TORRENT_BUFFER_MIN,
    TORRENT_BUFFER_MAX,
    TORRENT_QUALITY_OPTIONS,
    TORRENT_MAX_GB_DEFAULT,
    TORRENT_MAX_GB_MIN,
    TORRENT_MAX_GB_MAX,
    IPTV_DEFAULT_LISTS,
    type ModeOverride,
    type IptvList,
    type SubMode,
    type SourceSelect,
  } from "$lib/config.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  import { torrentDefaultDir, torrentCheckDir } from "$lib/torrents.svelte";
  import { notify } from "$lib/notifStore.svelte";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { navegar, enfocarPrimero } from "$lib/nav";
  import Gamepad from "$lib/Gamepad.svelte";
  import RemoteQr from "$lib/RemoteQr.svelte";
  import {
    ACCIONES,
    loadGamepadMap,
    saveGamepadMap,
    defaultGamepadMap,
    nombreBoton,
    setGamepadCapture,
    type GamepadMap,
  } from "$lib/controls";

  // --- Controles: mapeo del mando físico (los 3 métodos comparten teclas) ---
  let gpMap = $state<GamepadMap>(loadGamepadMap());
  let selBtn = $state<number | null>(null); // botón clicado, esperando acción
  let padPressed = $state<number[]>([]); // botones pulsados ahora (en vivo)
  let padRaf = 0;

  function keyLabel(key: string): string {
    const m: Record<string, string> = {
      ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→",
      Enter: "Enter", Backspace: "⌫", Escape: "Esc", " ": "Espacio",
      i: "I", m: "M", "[": "[", "]": "]",
    };
    return m[key] ?? key;
  }

  // Reverso: índice de botón → etiqueta de acción asignada.
  const actionByBtn = $derived.by(() => {
    const out: Record<number, string> = {};
    for (const a of ACCIONES) {
      const b = gpMap[a.id];
      if (b !== undefined && b >= 0) out[b] = a.label;
    }
    return out;
  });

  function aplicarGpMap() {
    saveGamepadMap(gpMap);
    window.dispatchEvent(new Event("gamepad-map-changed"));
  }
  function restaurarControles() {
    gpMap = defaultGamepadMap();
    selBtn = null;
    aplicarGpMap();
  }
  // Clic en un botón del mando → seleccionarlo para asignarle acción.
  function onpick(i: number) {
    selBtn = selBtn === i ? null : i;
  }
  // Asignar una acción al botón seleccionado (cada botón = una acción).
  function asignar(id: string) {
    if (selBtn === null) return;
    const m: GamepadMap = {};
    for (const k of Object.keys(gpMap)) if (gpMap[k] !== selBtn) m[k] = gpMap[k];
    m[id] = selBtn;
    gpMap = m;
    selBtn = null;
    aplicarGpMap();
  }
  // Loop de lectura en vivo (resaltado). Mientras estás en /config el bridge
  // global queda en captura → el mando no navega, solo se prueba aquí.
  function padLoop() {
    const gp = (navigator.getGamepads?.() ?? []).find((p) => p) ?? null;
    padPressed = gp
      ? gp.buttons.map((x, i) => (x.pressed ? i : -1)).filter((i) => i >= 0)
      : [];
    padRaf = requestAnimationFrame(padLoop);
  }

  // --- Mando para juegos: binds del retropad de RetroArch -----------------
  // Distinto del mapeo de arriba: ese es para la interfaz (Gamepad API del
  // webview). RetroArch es otro proceso y numera los botones a su manera, así
  // que la captura la hace el backend leyendo /dev/input (ver padmap.rs).
  type PadBind =
    | { kind: "btn"; n: number }
    | { kind: "axis"; n: number; dir: number }
    | { kind: "hat"; n: number; dir: string };
  type PadMapJuegos = {
    device: string;
    name: string;
    pad_index: number;
    binds: Record<string, PadBind>;
  };
  type PadDevice = { path: string; name: string; index: number };

  // Botones del retropad, con el nombre que usa RetroArch como id.
  const RETRO_BOTONES: { id: string; label: string }[] = [
    { id: "up", label: "Arriba ↑" },
    { id: "down", label: "Abajo ↓" },
    { id: "left", label: "Izquierda ←" },
    { id: "right", label: "Derecha →" },
    { id: "a", label: "A" },
    { id: "b", label: "B" },
    { id: "x", label: "X" },
    { id: "y", label: "Y" },
    { id: "l", label: "L (hombro izq.)" },
    { id: "r", label: "R (hombro der.)" },
    { id: "l2", label: "L2 (gatillo izq.)" },
    { id: "r2", label: "R2 (gatillo der.)" },
    { id: "select", label: "Select" },
    { id: "start", label: "Start" },
  ];

  let pads = $state<PadDevice[]>([]);
  let padSel = $state<string>(""); // ruta del /dev/input/eventN elegido
  let padMapJuegos = $state<PadMapJuegos>({ device: "", name: "", pad_index: 0, binds: {} });
  let capturando = $state<string | null>(null); // id del botón que espera pulsación
  let padErr = $state<string | null>(null);

  function bindLabel(b: PadBind | undefined): string {
    if (!b) return "—";
    if (b.kind === "btn") return `Botón ${b.n}`;
    if (b.kind === "axis") return `Eje ${b.n}${b.dir < 0 ? "−" : "+"}`;
    const flechas: Record<string, string> = { up: "↑", down: "↓", left: "←", right: "→" };
    return `Cruceta ${flechas[b.dir] ?? b.dir}`;
  }

  async function buscarMandos() {
    padErr = null;
    try {
      pads = await invoke<PadDevice[]>("pad_devices");
      if (!pads.some((p) => p.path === padSel)) padSel = pads[0]?.path ?? "";
    } catch (e) {
      padErr = String(e);
      pads = [];
    }
  }

  async function cargarMapaJuegos() {
    try {
      padMapJuegos = await invoke<PadMapJuegos>("pad_map_get");
      if (padMapJuegos.device) padSel = padMapJuegos.device;
    } catch (e) {
      console.warn("[pad_map_get]", e);
    }
  }

  async function guardarMapaJuegos() {
    const pad = pads.find((p) => p.path === padSel);
    padMapJuegos = {
      ...padMapJuegos,
      device: padSel,
      name: pad?.name ?? padMapJuegos.name,
      pad_index: pad?.index ?? padMapJuegos.pad_index,
    };
    try {
      await invoke("pad_map_set", { map: padMapJuegos });
    } catch (e) {
      padErr = String(e);
    }
  }

  /** Espera a que el usuario apriete algo y lo ata a `id`. */
  async function capturarBoton(id: string) {
    if (capturando || !padSel) return;
    capturando = id;
    padErr = null;
    try {
      const b = await invoke<PadBind | null>("pad_capture", { device: padSel, timeoutMs: 5000 });
      if (b) {
        // Un mismo botón físico no puede quedar en dos acciones: la vieja se
        // libera, si no el emulador recibe dos pulsaciones a la vez.
        const binds: Record<string, PadBind> = {};
        for (const [k, v] of Object.entries(padMapJuegos.binds)) {
          if (JSON.stringify(v) !== JSON.stringify(b)) binds[k] = v;
        }
        binds[id] = b;
        padMapJuegos = { ...padMapJuegos, binds };
        await guardarMapaJuegos();
      } else {
        padErr = "No se detectó ninguna pulsación.";
      }
    } catch (e) {
      padErr = String(e);
    } finally {
      capturando = null;
    }
  }

  async function borrarBoton(id: string) {
    const binds = { ...padMapJuegos.binds };
    delete binds[id];
    padMapJuegos = { ...padMapJuegos, binds };
    await guardarMapaJuegos();
  }

  async function restaurarMandoJuegos() {
    try {
      await invoke("pad_map_clear");
      padMapJuegos = { device: "", name: "", pad_index: 0, binds: {} };
    } catch (e) {
      padErr = String(e);
    }
  }

  const juegosBindsCount = $derived(Object.keys(padMapJuegos.binds).length);
  const salidaLista = $derived(
    padMapJuegos.binds.select?.kind === "btn" && padMapJuegos.binds.start?.kind === "btn",
  );

  type RdDeviceStart = {
    device_code: string;
    user_code: string;
    verification_url: string;
    interval: number;
    expires_in: number;
  };
  let tmdb = $state("");
  // Token pegado a mano (avanzado). NO se persiste en localStorage: se manda al
  // store 0600 del backend vía rd_creds_save. El vínculo se lee de config.rdLinked.
  let rdManual = $state("");
  let lang = $state(config.lang);
  let mode = $state<ModeOverride>(config.modeOverride);
  let screeningConc = $state(config.screeningConcurrency);
  let subsLang = $state(config.subsLang);
  let wyzieKey = $state(config.wyzieKey);
  let showWyzie = $state(false);
  let subSize = $state(config.subSize);
  let subMode = $state<SubMode>(config.subMode);
  let sourceSelect = $state<SourceSelect>(config.sourceSelect);
  let verifyEsTracks = $state(config.verifyEsTracks);
  let preferredSourceMovie = $state(config.preferredSourceMovie);
  let preferredSourceSeries = $state(config.preferredSourceSeries);
  let preferredSourceAnime = $state(config.preferredSourceAnime);
  let blockedSourceMovie = $state(config.blockedSourceMovie);
  let blockedSourceSeries = $state(config.blockedSourceSeries);
  let blockedSourceAnime = $state(config.blockedSourceAnime);
  // OpenSubtitles: login de cuenta gratis (sube cuota a 20/día).
  let osUserInput = $state("");
  let osPassInput = $state("");
  let osBusy = $state(false);
  let osErr = $state("");
  let webAuto = $state(config.webAutoStart);
  let webPortInput = $state(config.webPort);
  let torrentLocal = $state(config.torrentLocal);
  let torrentBufferMb = $state(config.torrentBufferMb);
  let torrentMaxQuality = $state(config.torrentMaxQuality);
  let torrentMaxGb = $state(config.torrentMaxGb);
  let torrentDirPath = $state("");
  let torrentDirInput = $state(config.torrentDir);
  let dirEstado = $state<"idle" | "probando" | "ok" | "error">("idle");
  let dirMsg = $state("");
  let gameRegions = $state<string[]>([...config.gameRegions]);
  let iptvLists = $state<IptvList[]>(config.iptvLists.map((l) => ({ ...l })));
  let nuevaListaNombre = $state("");
  let nuevaListaUrl = $state("");
  let saved = $state(false);

  function agregarLista() {
    const url = nuevaListaUrl.trim();
    if (!url) return;
    const name = nuevaListaNombre.trim() || `Lista ${iptvLists.length + 1}`;
    iptvLists = [...iptvLists, { name, url }];
    nuevaListaNombre = "";
    nuevaListaUrl = "";
  }
  function quitarLista(i: number) {
    iptvLists = iptvLists.filter((_, idx) => idx !== i);
  }
  function restaurarListaDefault() {
    iptvLists = IPTV_DEFAULT_LISTS.map((l) => ({ ...l }));
  }

  function toggleRegion(id: string) {
    gameRegions = gameRegions.includes(id)
      ? gameRegions.filter((x) => x !== id)
      : [...gameRegions, id];
  }
  // --- Teclado por celular: QR a /api para escribir/pegar/escanear claves ---
  type WebStatus = { running: boolean; ip: string | null; port: number | null; url: string | null };
  let phoneUrl = $state("");
  let phoneBusy = $state(false);
  let phoneErr = $state("");
  async function conectarCelular() {
    phoneBusy = true;
    phoneErr = "";
    try {
      let st = await invoke<WebStatus>("web_server_status");
      if (!st.running) {
        st = await invoke<WebStatus>("web_server_start", { port: config.webPort });
      }
      if (st.url) phoneUrl = `${st.url}/api`;
      else phoneErr = "No se pudo obtener la IP de la red local.";
    } catch (e) {
      phoneErr = String(e);
    } finally {
      phoneBusy = false;
    }
  }

  let showTmdb = $state(false);
  let showOmdb = $state(false);
  let omdb = $state("");
  let showRdAdvanced = $state(false);

  // RD device flow
  let rdStarting = $state(false);
  let rdLink = $state<RdDeviceStart | null>(null);
  let rdWaiting = $state(false);
  let rdErr = $state("");
  let rdOk = $state(false);
  let rdAbort: AbortController | null = null;
  let countdownTimer: number | null = null;
  let secondsLeft = $state(0);

  // Updater
  let currentVer = $state("");
  type UpdStage = "idle" | "checking" | "uptodate" | "available" | "installing" | "ready" | "error";
  let updStage = $state<UpdStage>("idle");
  let updPending: Update | null = null;
  let updVersion = $state("");
  let updNotes = $state("");
  let updErr = $state("");
  let updDownloaded = $state(0);
  let updTotal = $state(0);

  onMount(async () => {
    ayuda.set("config", [
      { tecla: "← → ↑ ↓", desc: "Moverse entre ajustes" },
      { tecla: "Enter · Espacio", desc: "Activar / marcar" },
      { tecla: "Esc · Backspace", desc: "Volver a inicio" },
      { tecla: "I-I", desc: "Ayuda" },
    ]);
    // Sin foco inicial la pantalla arrancaba muerta para mando: las flechas no
    // tenían desde dónde salir y no había forma de llegar a ningún control.
    setTimeout(() => enfocarPrimero(document.querySelector(".cfg-root") ?? document), 60);
    loadConfig();
    tmdb = config.tmdbKey;
    omdb = config.omdbKey;
    lang = config.lang;
    mode = config.modeOverride;
    screeningConc = config.screeningConcurrency;
    subsLang = config.subsLang;
    wyzieKey = config.wyzieKey;
    subSize = config.subSize;
    subMode = config.subMode;
    sourceSelect = config.sourceSelect;
    verifyEsTracks = config.verifyEsTracks;
    preferredSourceMovie = config.preferredSourceMovie;
    preferredSourceSeries = config.preferredSourceSeries;
    preferredSourceAnime = config.preferredSourceAnime;
    blockedSourceMovie = config.blockedSourceMovie;
    blockedSourceSeries = config.blockedSourceSeries;
    blockedSourceAnime = config.blockedSourceAnime;
    void refreshOsStatus();
    webAuto = config.webAutoStart;
    webPortInput = config.webPort;
    torrentLocal = config.torrentLocal;
    torrentBufferMb = config.torrentBufferMb;
    torrentMaxQuality = config.torrentMaxQuality;
    torrentMaxGb = config.torrentMaxGb;
    torrentDirInput = config.torrentDir;
    // Solo informativo; no levanta la sesión torrent si no está iniciada.
    try { torrentDirPath = await torrentDefaultDir(); } catch {}
    gameRegions = [...config.gameRegions];
    iptvLists = config.iptvLists.map((l) => ({ ...l }));
    try { currentVer = await getVersion(); } catch {}
    await cargarMapaJuegos();
    await buscarMandos();
    // Probar el mando aquí sin que navegue la app.
    setGamepadCapture(true);
    padRaf = requestAnimationFrame(padLoop);
  });

  onDestroy(() => {
    cancelRd();
    cancelAnimationFrame(padRaf);
    setGamepadCapture(false);
  });

  const dirty = $derived(
    tmdb !== config.tmdbKey ||
    omdb !== config.omdbKey ||
    lang !== config.lang ||
    mode !== config.modeOverride ||
    screeningConc !== config.screeningConcurrency ||
    subsLang !== config.subsLang ||
    wyzieKey !== config.wyzieKey ||
    subSize !== config.subSize ||
    subMode !== config.subMode ||
    sourceSelect !== config.sourceSelect ||
    verifyEsTracks !== config.verifyEsTracks ||
    preferredSourceMovie !== config.preferredSourceMovie ||
    preferredSourceSeries !== config.preferredSourceSeries ||
    preferredSourceAnime !== config.preferredSourceAnime ||
    blockedSourceMovie !== config.blockedSourceMovie ||
    blockedSourceSeries !== config.blockedSourceSeries ||
    blockedSourceAnime !== config.blockedSourceAnime ||
    webAuto !== config.webAutoStart ||
    webPortInput !== config.webPort ||
    gameRegions.join(",") !== config.gameRegions.join(",") ||
    JSON.stringify(iptvLists) !== JSON.stringify(config.iptvLists)
  );

  function applyAndSave() {
    config.tmdbKey = tmdb.trim();
    config.omdbKey = omdb.trim();
    config.lang = lang;
    config.modeOverride = mode;
    const conc = Math.min(SCREENING_MAX, Math.max(SCREENING_MIN, Math.round(screeningConc)));
    config.screeningConcurrency = conc;
    config.subsLang = subsLang;
    config.wyzieKey = wyzieKey.trim();
    config.subSize = Math.min(200, Math.max(50, Math.round(subSize)));
    config.subMode = subMode;
    config.sourceSelect = sourceSelect;
    config.verifyEsTracks = verifyEsTracks;
    config.preferredSourceMovie = preferredSourceMovie.trim();
    config.preferredSourceSeries = preferredSourceSeries.trim();
    config.preferredSourceAnime = preferredSourceAnime.trim();
    config.blockedSourceMovie = blockedSourceMovie.trim();
    config.blockedSourceSeries = blockedSourceSeries.trim();
    config.blockedSourceAnime = blockedSourceAnime.trim();
    config.webAutoStart = webAuto;
    config.webPort = Math.min(65535, Math.max(1024, Math.round(webPortInput) || 8080));
    webPortInput = config.webPort;
    aplicarTorrent();
    config.gameRegions = [...gameRegions];
    const listas = iptvLists.filter((l) => l.url.trim());
    config.iptvLists = (listas.length ? listas : IPTV_DEFAULT_LISTS).map((l) => ({
      name: (l.name || "Lista").trim(),
      url: l.url.trim(),
    }));
    iptvLists = config.iptvLists.map((l) => ({ ...l }));
    saveConfig();
    void setConcurrenciaScreening(conc);
    saved = true;
    setTimeout(() => { saved = false; }, 1800);
  }

  // Descarga local: los controles se aplican al toque, sin pasar por "Guardar".
  // El panel se despliega apenas tildas la casilla, así que se lee como que ya
  // quedó activo; dejarlo dependiendo del botón hacía que siguiera apagado.
  function aplicarTorrent() {
    config.torrentLocal = torrentLocal;
    config.torrentBufferMb = Math.min(
      TORRENT_BUFFER_MAX,
      Math.max(TORRENT_BUFFER_MIN, Math.round(torrentBufferMb) || TORRENT_BUFFER_DEFAULT),
    );
    torrentBufferMb = config.torrentBufferMb;
    config.torrentMaxQuality = torrentMaxQuality;
    config.torrentMaxGb = Math.min(
      TORRENT_MAX_GB_MAX,
      Math.max(TORRENT_MAX_GB_MIN, Number(torrentMaxGb) || TORRENT_MAX_GB_DEFAULT),
    );
    torrentMaxGb = config.torrentMaxGb;
    config.torrentDir = torrentDirInput.trim();
    saveConfig();
  }

  // Comprueba que la carpeta exista y se pueda escribir antes de dejarla puesta.
  async function probarCarpeta() {
    dirEstado = "probando";
    dirMsg = "";
    try {
      const real = await torrentCheckDir(torrentDirInput);
      dirEstado = "ok";
      dirMsg = real;
      aplicarTorrent();
    } catch (e) {
      dirEstado = "error";
      dirMsg = String(e);
    }
  }

  function updateWyzieKey(value: string) {
    wyzieKey = value;
    config.wyzieKey = value.trim();
    saveConfig();
  }

  function back() {
    goto("/");
  }

  // Esc/Backspace en /config: si hay flow RD activo, Esc lo cancela; si no,
  // ambas vuelven al home. Backspace en inputs queda intocable (borra texto).
  // Tipos de <input> donde las flechas horizontales son del control (mueven el
  // cursor o el valor) y no de la navegación.
  const TEXTO = ["text", "password", "search", "url", "email", "tel", "number"];

  function onGlobalKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    const tag = t?.tagName;
    const tipo = (t as HTMLInputElement | null)?.type ?? "";
    const inText =
      tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" ||
      (t?.isContentEditable ?? false);

    if (e.key === "Escape" || e.key === "Backspace") {
      if (e.key === "Escape" && rdLink) {
        e.preventDefault();
        cancelRd();
        return;
      }
      if (e.key === "Backspace" && inText) return;
      e.preventDefault();
      back();
      return;
    }

    if (e.key.startsWith("Arrow")) {
      // Sliders y campos de texto se quedan con lo suyo; ↑↓ igual sirven para
      // salir del campo, que es lo que uno espera con un control.
      const horizontal = e.key === "ArrowLeft" || e.key === "ArrowRight";
      if (tipo === "range") return;
      if (horizontal && (tag === "TEXTAREA" || (tag === "INPUT" && TEXTO.includes(tipo)))) return;
      if (tag === "SELECT") return;
      e.preventDefault();
      navegar(e.key.slice(5).toLowerCase() as "up" | "down" | "left" | "right");
      return;
    }

    // Enter sobre radio/checkbox: los marca. Nativamente solo responden a
    // Espacio, y en el mando el botón A manda Enter.
    if (e.key === "Enter" && tag === "INPUT" && (tipo === "radio" || tipo === "checkbox")) {
      e.preventDefault();
      (t as HTMLInputElement).click();
    }
  }

  async function startRd() {
    // Limpiar cualquier flow previo (doble click rápido o reintento) antes
    // de arrancar uno nuevo. Sin esto, varios timers corren en paralelo y
    // el secondsLeft baja al doble.
    cancelRd();
    rdErr = "";
    rdOk = false;
    rdStarting = true;
    try {
      const start = await invoke<RdDeviceStart>("rd_device_start");
      rdLink = start;
      secondsLeft = start.expires_in;
      countdownTimer = window.setInterval(() => {
        secondsLeft = Math.max(0, secondsLeft - 1);
        if (secondsLeft === 0) cancelRd();
      }, 1000);
      rdWaiting = true;
      pollRd(start);
    } catch (e) {
      rdErr = String(e);
    } finally {
      rdStarting = false;
    }
  }

  async function pollRd(start: RdDeviceStart) {
    try {
      // El backend persiste las credenciales en su store 0600; aquí no llega
      // ningún token. Solo refrescamos el indicador de vínculo.
      await invoke("rd_device_poll", {
        deviceCode: start.device_code,
        interval: start.interval,
        expiresIn: start.expires_in,
      });
      await refreshRdLinked();
      rdOk = true;
      rdLink = null;
      stopCountdown();
    } catch (e) {
      if (!rdWaiting) return; // cancelado por user
      rdErr = String(e);
    } finally {
      rdWaiting = false;
    }
  }

  function cancelRd() {
    rdWaiting = false;
    rdLink = null;
    rdAbort?.abort();
    rdAbort = null;
    stopCountdown();
  }
  function stopCountdown() {
    if (countdownTimer !== null) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
  }

  async function openVerify() {
    if (!rdLink) return;
    try { await openUrl(rdLink.verification_url); } catch {}
  }

  async function unlinkRd() {
    try { await invoke("rd_creds_clear"); } catch {}
    await refreshRdLinked();
    rdManual = "";
    rdOk = false;
  }

  // OpenSubtitles: vincular / desvincular cuenta gratis (cuota 20/día).
  async function loginOs() {
    osErr = "";
    if (!osUserInput.trim() || !osPassInput) {
      osErr = "Usuario y contraseña requeridos.";
      return;
    }
    osBusy = true;
    try {
      await invoke("os_login", { username: osUserInput.trim(), password: osPassInput });
      osPassInput = "";
      osUserInput = "";
      await refreshOsStatus();
    } catch (e) {
      osErr = String(e);
    } finally {
      osBusy = false;
    }
  }

  async function unlinkOs() {
    try { await invoke("os_clear"); } catch {}
    await refreshOsStatus();
  }

  // Guarda un token pegado a mano en el store seguro del backend.
  async function saveManualToken() {
    const tok = rdManual.trim();
    if (!tok) return;
    rdErr = "";
    try {
      await invoke("rd_creds_save", {
        accessToken: tok,
        refreshToken: "",
        clientId: "",
        clientSecret: "",
      });
      await refreshRdLinked();
      rdManual = "";
      showRdAdvanced = false;
      rdOk = true;
    } catch (e) {
      rdErr = String(e);
    }
  }

  function fmtMmSs(s: number): string {
    const m = Math.floor(s / 60);
    const r = (s % 60).toString().padStart(2, "0");
    return `${m}:${r}`;
  }

  // --- Panel de prueba torrent + RD + mpv ---
  let testImdb = $state("tt0816692");
  let testRunning = $state(false);
  let testLog = $state<string[]>([]);
  let testSrcs = $state<any[]>([]);
  let testUrl = $state("");

  function testLogPush(s: string) { testLog = [...testLog, s]; }

  async function runRdTest() {
    testRunning = true;
    testLog = [];
    testUrl = "";
    testSrcs = [];
    const log = testLogPush;
    try {
      const linked = config.rdLinked;
      log(`RD: ${linked ? "vinculado" : "NO vinculado"}`);
      log("buscando fuentes…");
      const srcs = await invoke<any[]>("kodios_search", {
        imdbId: testImdb.trim(),
        kind: "movie",
      });
      testSrcs = srcs;
      const conUrl = srcs.filter((s) => s.url).length;
      log(`fuentes: ${srcs.length} · ${conUrl} pre-resueltas (Torrentio+RD)`);
      if (!srcs.length) { log("sin fuentes, fin"); return; }
      if (!linked) { log("sin RD vinculado → no resuelvo"); return; }
      // Itera: usa url directa si existe, si no resuelve magnet. Salta bloqueadas.
      let blocked = 0;
      const max = Math.min(srcs.length, 12);
      for (let i = 0; i < max; i++) {
        const s = srcs[i];
        if (!s.magnet && !s.url) continue;
        log(`#${i + 1} ${s.quality}${s.url ? " (url)" : ""} ${(s.title || "").slice(0, 40)}…`);
        try {
          const url = s.url
            ? s.url
            : await invoke<string>("rd_resolve", { magnet: s.magnet });
          testUrl = url;
          log(`✓ REPRODUCIBLE (#${i + 1}, ${blocked} bloqueadas antes): ${url.slice(0, 70)}…`);
          return;
        } catch (e) {
          const msg = String(e);
          if (msg.includes("BLOQUEADO_DMCA")) { blocked++; log(`  ⛔ bloqueada (451)`); }
          else log(`  ✗ ${msg.slice(0, 80)}`);
        }
      }
      log(`Sin reproducibles en ${max} intentos · ${blocked} bloqueadas por DMCA.`);
    } catch (e) {
      log(`✗ error: ${String(e)}`);
    } finally {
      testRunning = false;
    }
  }

  // ⚡ Cuántas de las fuentes ya están cacheadas en RD (reproducción instantánea)
  async function runCacheTest() {
    const log = testLogPush;
    if (!config.rdLinked) { log("sin RD vinculado"); return; }
    if (!testSrcs.length) { log("primero pulsa Probar (no hay fuentes)"); return; }
    const hashes = testSrcs.map((s) => s.info_hash).filter(Boolean);
    log(`checando cache de ${hashes.length} hashes…`);
    try {
      const cached = await invoke<string[]>("rd_instant_available", { hashes });
      const set = new Set(cached.map((h) => h.toLowerCase()));
      log(`⚡ cacheadas en RD: ${set.size} / ${hashes.length}`);
      const top = testSrcs
        .filter((s) => s.info_hash && set.has(s.info_hash.toLowerCase()))
        .slice(0, 3)
        .map((s) => `  • ${s.quality} ${(s.title || "").slice(0, 50)}`);
      if (top.length) log(top.join("\n"));
    } catch (e) {
      log(`✗ cache error: ${String(e)}`);
    }
  }

  // ▶ Reproduce la URL resuelta en mpv
  async function runMpvTest() {
    const log = testLogPush;
    if (!testUrl) { log("primero resuelve una URL (Probar)"); return; }
    try {
      setNowPlaying("", "Prueba Kütral");
      // Prueba técnica: no debe contarse como "visto" de nada del catálogo.
      limpiarContexto();
      await invoke("mpv_play", { url: testUrl, title: "Prueba Kütral" });
      log("▶ mpv lanzado");
    } catch (e) {
      log(`✗ mpv error: ${String(e)}`);
    }
  }

  async function runMpvStop() {
    try {
      await invoke("mpv_stop");
      testLogPush("⏹ mpv detenido");
    } catch (e) {
      testLogPush(`✗ stop error: ${String(e)}`);
    }
  }

  // 👤 Estado de la cuenta RD (diagnóstico de 451)
  async function runAccountTest() {
    const log = testLogPush;
    if (!config.rdLinked) { log("sin RD vinculado"); return; }
    try {
      const a = await invoke<any>("rd_account");
      const secs = a.premium ?? 0;
      const dias = Math.round(secs / 86400);
      log(`👤 ${a.username} · tipo: ${a.account_type || "?"} · premium: ${dias}d · expira: ${a.expiration || "?"}`);
      if (secs > 0) {
        log("✓ Premium ACTIVO → el 451 es la lista DMCA de RD (por-torrent), no la cuenta. El auto-salto debe encontrar una fuente no bloqueada.");
      } else {
        log("⚠ Sin premium → RD bloquea torrents. Renueva premium.");
      }
    } catch (e) {
      log(`✗ cuenta error: ${String(e)}`);
    }
  }

  async function checkUpdate() {
    updStage = "checking";
    updErr = "";
    try {
      const u = await check();
      if (!u) {
        updStage = "uptodate";
        return;
      }
      updPending = u;
      updVersion = u.version;
      updNotes = u.body || "";
      updStage = "available";
      notify("info", `Actualización ${u.version} disponible`, u.body || undefined);
    } catch (e) {
      updErr = String(e);
      updStage = "error";
    }
  }

  async function installUpdate() {
    if (!updPending) return;
    updStage = "installing";
    updDownloaded = 0;
    updTotal = 0;
    try {
      await updPending.downloadAndInstall((ev) => {
        switch (ev.event) {
          case "Started": updTotal = ev.data.contentLength ?? 0; break;
          case "Progress": updDownloaded += ev.data.chunkLength; break;
          case "Finished": updStage = "ready"; break;
        }
      });
      await relaunch();
    } catch (e) {
      updErr = String(e);
      updStage = "error";
    }
  }

  function updPct(): number {
    if (updTotal <= 0) return 0;
    return Math.min(100, Math.round((updDownloaded / updTotal) * 100));
  }
  function updMb(b: number): string {
    return (b / 1024 / 1024).toFixed(1);
  }
</script>

<svelte:head>
  <title>Configuración · Kütral</title>
</svelte:head>

<svelte:window onkeydown={onGlobalKey} />

<div class="cfg-root">
  <div class="cfg-wrap">
    <header class="cfg-head">
      <button data-nav class="back" onclick={back} title="Volver">← Volver</button>
      <h1>Configuración</h1>
    </header>

    <div class="cfg-grid">
      <div class="col">
        <section class="block phone-block">
          <h2>Escribir desde el celular</h2>
          <p class="hint">
            ¿La API es muy larga para el control? Conecta tu celular por WiFi,
            escanea el QR y escribe, pega o <strong>escanea otro QR</strong> con
            la clave desde el teléfono. Llega al campo que tengas seleccionado aquí.
          </p>
          {#if !phoneUrl}
            <button data-nav class="btn-link" onclick={conectarCelular} disabled={phoneBusy}>
              {phoneBusy ? "Conectando…" : "📱 Conectar celular"}
            </button>
          {:else}
            <div class="phone-qr">
              <RemoteQr url={phoneUrl} label="Teclado" size={132} />
              <div class="phone-steps">
                <p class="hint" style="margin:0 0 8px;">
                  1. Escanea con la cámara del celular.<br />
                  2. Toca aquí el campo a llenar (TMDb, OMDb…).<br />
                  3. Escribe/pega/escanea en el cel y pulsa <em>Enviar</em>.
                </p>
                <code class="phone-url">{phoneUrl}</code>
              </div>
            </div>
          {/if}
          {#if phoneErr}<p class="err">{phoneErr}</p>{/if}
        </section>

        <section class="block">
          <h2>API key de TMDb</h2>
          <p class="hint">
            Gratis en <code>themoviedb.org/settings/api</code>. Sin esto, no hay catálogo.
          </p>
          <div class="key-row">
            <input data-nav
              type={showTmdb ? "text" : "password"}
              bind:value={tmdb}
              placeholder="tmdb_xxxxxxxxxxxxxxxxxxxxxxxx"
              autocomplete="off"
              spellcheck="false"
            />
            <button data-nav class="reveal" onclick={() => (showTmdb = !showTmdb)} title={showTmdb ? "Ocultar" : "Mostrar"}>
              {showTmdb ? "🙈" : "👁"}
            </button>
          </div>
        </section>

        <section class="block">
          <h2>API key de OMDb <span class="badge-opt">opcional</span></h2>
          <p class="hint">
            Gratis en <code>omdbapi.com/apikey.aspx</code>. Habilita premios, recaudación,
            ratings (IMDb/RT/Metacritic) y sinopsis extendida al abrir un título.
          </p>
          <div class="key-row">
            <input data-nav
              type={showOmdb ? "text" : "password"}
              bind:value={omdb}
              placeholder="xxxxxxxx"
              autocomplete="off"
              spellcheck="false"
            />
            <button data-nav class="reveal" onclick={() => (showOmdb = !showOmdb)} title={showOmdb ? "Ocultar" : "Mostrar"}>
              {showOmdb ? "🙈" : "👁"}
            </button>
          </div>
        </section>

        <section class="block">
          <h2>RealDebrid</h2>
          <p class="hint">Vinculá tu cuenta para streaming premium sin publicidad.</p>

          {#if config.rdLinked && !rdLink}
            <div class="rd-linked">
              <span class="dot ok"></span>
              <span>Cuenta vinculada</span>
              <button data-nav class="btn-sec" onclick={unlinkRd}>Desvincular</button>
            </div>
          {:else if rdLink}
            <div class="rd-flow">
              <p class="rd-step">1. Andá a <button data-nav class="rd-link" onclick={openVerify}>{rdLink.verification_url}</button></p>
              <p class="rd-step">2. Ingresá este código:</p>
              <div class="rd-code">{rdLink.user_code}</div>
              <p class="rd-wait">
                <span class="spinner"></span>
                Esperando autorización… <em>{fmtMmSs(secondsLeft)}</em>
              </p>
              <button data-nav class="btn-sec" onclick={cancelRd}>Cancelar</button>
            </div>
          {:else}
            <button data-nav class="btn-link" onclick={startRd} disabled={rdStarting}>
              {rdStarting ? "Conectando…" : "Vincular cuenta"}
            </button>
            <button data-nav class="link-tiny" onclick={() => (showRdAdvanced = !showRdAdvanced)}>
              {showRdAdvanced ? "Ocultar avanzado" : "Pegar token manualmente"}
            </button>
            {#if showRdAdvanced}
              <div class="key-row" style="margin-top: 10px;">
                <input data-nav
                  type="password"
                  bind:value={rdManual}
                  placeholder="access_token"
                  autocomplete="off"
                  spellcheck="false"
                />
                <button data-nav class="btn-sec" onclick={saveManualToken} disabled={!rdManual.trim()}>
                  Guardar
                </button>
              </div>
            {/if}
          {/if}

          {#if rdErr}<p class="err">{rdErr}</p>{/if}
          {#if rdOk}<p class="ok">¡Vinculado!</p>{/if}

          <div class="rd-test">
            <p class="hint" style="margin: 14px 0 8px;">
              Prueba: busca fuentes torrent y resuelve la 1ª vía RD.
            </p>
            <div class="key-row">
              <input data-nav
                bind:value={testImdb}
                placeholder="tt0816692"
                autocomplete="off"
                spellcheck="false"
              />
              <button data-nav class="btn-sec" onclick={runRdTest} disabled={testRunning}>
                {testRunning ? "Probando…" : "Probar"}
              </button>
            </div>
            <div class="test-btns">
              <button data-nav class="btn-sec" onclick={runAccountTest}>
                👤 Cuenta RD
              </button>
              <button data-nav class="btn-sec" onclick={runCacheTest} disabled={testRunning || !testSrcs.length}>
                ⚡ Cache
              </button>
              <button data-nav class="btn-sec" onclick={runMpvTest} disabled={!testUrl}>
                ▶ mpv
              </button>
              <button data-nav class="btn-sec" onclick={runMpvStop}>
                ⏹ Detener
              </button>
            </div>
            {#if testLog.length}
              <pre class="test-log">{testLog.join("\n")}</pre>
            {/if}
          </div>
        </section>

        <section class="block">
          <h2>Actualizaciones</h2>
          <p class="hint">
            Versión actual: <em>{currentVer || "?"}</em>
          </p>

          {#if updStage === "idle"}
            <button data-nav class="btn-link" onclick={checkUpdate}>Buscar actualizaciones</button>
          {:else if updStage === "checking"}
            <p class="upd-line"><span class="spinner"></span> Buscando…</p>
          {:else if updStage === "uptodate"}
            <p class="upd-line upd-ok">Estás al día.</p>
            <button data-nav class="link-tiny" onclick={checkUpdate}>Volver a buscar</button>
          {:else if updStage === "available"}
            <p class="upd-line">
              Disponible: <strong>v{updVersion}</strong>
            </p>
            {#if updNotes}<pre class="upd-notes">{updNotes}</pre>{/if}
            <button data-nav class="btn-link" onclick={installUpdate}>Instalar y reiniciar</button>
            <button data-nav class="link-tiny" onclick={() => { updStage = "idle"; }}>Más tarde</button>
          {:else if updStage === "installing"}
            <p class="upd-line"><span class="spinner"></span> Descargando…</p>
            {#if updTotal > 0}
              <div class="bar"><div class="bar-fill" style="width: {updPct()}%"></div></div>
              <p class="upd-mb">{updMb(updDownloaded)} / {updMb(updTotal)} MB · {updPct()}%</p>
            {:else}
              <p class="upd-mb">{updMb(updDownloaded)} MB</p>
            {/if}
          {:else if updStage === "ready"}
            <p class="upd-line"><span class="spinner"></span> Reiniciando…</p>
          {:else if updStage === "error"}
            <p class="err">{updErr}</p>
            <button data-nav class="link-tiny" onclick={checkUpdate}>Reintentar</button>
          {/if}
        </section>
      </div>

      <div class="col">
        <section class="block">
          <h2>Idioma</h2>
          <p class="hint">Para títulos, descripciones e interfaz.</p>
          <div class="radio-group">
            {#each LANGS as l}
              <label class="radio">
                <input data-nav type="radio" name="lang" value={l.id} bind:group={lang} />
                <span>{l.label}</span>
              </label>
            {/each}
          </div>
        </section>

        <section class="block">
          <h2>Modo de interfaz</h2>
          <p class="hint">
            Auto = kiosko en Kütral OS, escritorio en el resto.
            Detectado: <em>{config.detectedKutral ? "Kütral OS" : "Escritorio"}</em>
          </p>
          <div class="mode-group">
            <label class="mode-card" class:sel={mode === "auto"}>
              <input data-nav type="radio" name="mode" value="auto" bind:group={mode} />
              <div><strong>Auto</strong><span>Detecta y aplica el modo correcto.</span></div>
            </label>
            <label class="mode-card" class:sel={mode === "desktop"}>
              <input data-nav type="radio" name="mode" value="desktop" bind:group={mode} />
              <div><strong>Escritorio</strong><span>Ventana con minimizar / maximizar / cerrar.</span></div>
            </label>
            <label class="mode-card" class:sel={mode === "kiosk"}>
              <input data-nav type="radio" name="mode" value="kiosk" bind:group={mode} />
              <div><strong>Kiosko / embedido</strong><span>Pantalla completa. Solo "Salir" + gestor WiFi.</span></div>
            </label>
          </div>
        </section>

        <section class="block">
          <h2>Audio y subtítulos</h2>
          <p class="hint">
            Cómo prefieres ver. Kütral ordena las fuentes y elige la pista
            correcta del archivo. Si el release no la trae, busca subtítulos.
          </p>
          <div class="mode-group">
            <label class="mode-card" class:sel={subMode === "dub"}>
              <input data-nav type="radio" name="subMode" value="dub" bind:group={subMode} />
              <div>
                <strong>Doblado</strong>
                <span>Audio en español primero (Latino / Castellano).</span>
              </div>
            </label>
            <label class="mode-card" class:sel={subMode === "sub"}>
              <input data-nav type="radio" name="subMode" value="sub" bind:group={subMode} />
              <div>
                <strong>Subtitulado</strong>
                <span>Audio original con subtítulos en español.</span>
              </div>
            </label>
          </div>
          <label class="toggle-row">
            <input data-nav type="checkbox" bind:checked={verifyEsTracks} />
            <span>Verificar español real antes de reproducir</span>
          </label>
          <p class="hint">
            Inspecciona las pistas del archivo (sin descargarlo) y confirma si
            trae audio o subtítulos en español. Si no trae, prueba la siguiente
            fuente. Agrega unos segundos por fuente inspeccionada.
          </p>
        </section>

        <section class="block">
          <h2>Selección de fuente</h2>
          <p class="hint">
            Al "Ver con debrid", cómo se elige qué fuente reproducir.
          </p>
          <div class="mode-group">
            <label class="mode-card" class:sel={sourceSelect === "auto"}>
              <input data-nav type="radio" name="sourceSelect" value="auto" bind:group={sourceSelect} />
              <div>
                <strong>Automática</strong>
                <span>Reproduce la mejor al instante, sin preguntar.</span>
              </div>
            </label>
            <label class="mode-card" class:sel={sourceSelect === "manual"}>
              <input data-nav type="radio" name="sourceSelect" value="manual" bind:group={sourceSelect} />
              <div>
                <strong>Manual</strong>
                <span>Siempre muestra el selector para elegir tú la fuente.</span>
              </div>
            </label>
          </div>
          <p class="hint">
            Texto de interés en el nombre del torrent (release/grupo). Si el nombre
            o el proveedor lo contiene, esa fuente sube al tope (en modo automático,
            es la que se reproduce). Útil para grupos que suelen traer audio en
            español. Varias separadas por coma. Se configura por tipo.
          </p>
          <label class="field">
            <span class="field-label">Películas</span>
            <input data-nav
              type="text"
              bind:value={preferredSourceMovie}
              placeholder="ej. FullScrab, YIFY"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
          <label class="field">
            <span class="field-label">Series</span>
            <input data-nav
              type="text"
              bind:value={preferredSourceSeries}
              placeholder="ej. MeGusta, FLUX"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
          <label class="field">
            <span class="field-label">Anime</span>
            <input data-nav
              type="text"
              bind:value={preferredSourceAnime}
              placeholder="ej. Erai-raws, SubsPlease"
              autocomplete="off"
              spellcheck="false"
            />
          </label>

          <h3 class="subhead">Lista negra (opcional)</h3>
          <p class="hint">
            Texto que NO quieres. Si el nombre del torrent o el proveedor lo contiene,
            esa fuente se hunde al fondo del orden (nunca se auto-reproduce salvo que
            no haya otra). Varias separadas por coma. Se configura por tipo.
          </p>
          <label class="field">
            <span class="field-label">Películas</span>
            <input data-nav
              type="text"
              bind:value={blockedSourceMovie}
              placeholder="ej. CAM, HDTS, TELESYNC"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
          <label class="field">
            <span class="field-label">Series</span>
            <input data-nav
              type="text"
              bind:value={blockedSourceSeries}
              placeholder="ej. HDTV, x265"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
          <label class="field">
            <span class="field-label">Anime</span>
            <input data-nav
              type="text"
              bind:value={blockedSourceAnime}
              placeholder="ej. HEVC, 480p"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
        </section>

        <section class="block">
          <h2>Subtítulos</h2>
          <p class="hint">
            Idioma preferido. Si pones API key de Wyzie, Kütral busca
            subtítulos ahí (más cuotas que OpenSubtitles).
            <a href="https://store.wyzie.io/redeem" target="_blank" rel="noopener">Generar key gratis →</a>
          </p>
          <label class="field">
            <span class="field-label">Idioma</span>
            <select data-nav bind:value={subsLang}>
              {#each SUB_LANGS as sl}
                <option value={sl.id}>{sl.label}</option>
              {/each}
            </select>
          </label>
          <label class="field">
            <span class="field-label">Wyzie API key (opcional)</span>
            <div class="key-row">
              <input data-nav
                type={showWyzie ? "text" : "password"}
                value={wyzieKey}
                oninput={(e) => updateWyzieKey(e.currentTarget.value)}
                placeholder="Sin key: el player usa OpenSubtitles"
                autocomplete="off"
                spellcheck="false"
              />
              <button data-nav type="button" class="btn-ghost" onclick={() => (showWyzie = !showWyzie)}>
                {showWyzie ? "Ocultar" : "Mostrar"}
              </button>
            </div>
          </label>
          <label class="field">
            <span class="field-label">Tamaño de letra ({subSize}%)</span>
            <input data-nav
              type="range"
              min="50"
              max="200"
              step="10"
              bind:value={subSize}
            />
          </label>

          <div class="field">
            <span class="field-label">Cuenta OpenSubtitles (opcional)</span>
            {#if !config.osHasKey}
              <p class="hint">
                Subtítulos externos deshabilitados: falta la API key de la app.
                Configúrala en el código (módulo opensubtitles.rs).
              </p>
            {:else if config.osLinked}
              <div class="key-row">
                <span class="os-linked">✓ Vinculada{config.osUser ? ` (${config.osUser})` : ""} — 20 descargas/día</span>
                <button data-nav type="button" class="btn-ghost" onclick={unlinkOs}>Desvincular</button>
              </div>
            {:else}
              <p class="hint">
                Sin cuenta usas 5 subtítulos/día (anónimo). Con una cuenta gratis
                suben a 20/día.
                <a href="https://www.opensubtitles.com/newuser" target="_blank" rel="noopener">Crear cuenta gratis →</a>
              </p>
              <input data-nav type="text" bind:value={osUserInput} placeholder="Usuario" autocomplete="off" />
              <input data-nav type="password" bind:value={osPassInput} placeholder="Contraseña" autocomplete="off" />
              {#if osErr}<p class="err">{osErr}</p>{/if}
              <button data-nav type="button" class="btn-ghost" disabled={osBusy} onclick={loginOs}>
                {osBusy ? "Vinculando…" : "Vincular cuenta"}
              </button>
            {/if}
          </div>
        </section>
      </div>

      <div class="col">
        <section class="block">
          <h2>Verificación de disponibilidad</h2>
          <p class="hint">
            Verificaciones simultáneas al detectar si una película se puede
            reproducir. Más alto = los badges "no disponible" aparecen
            más rápido, pero puede saturar la red y cortar el video.
          </p>
          <div class="mode-group">
            {#each Array.from({ length: SCREENING_MAX - SCREENING_MIN + 1 }, (_, i) => i + SCREENING_MIN) as n}
              <label class="mode-card" class:sel={screeningConc === n}>
                <input data-nav type="radio" name="screeningConc" value={n} bind:group={screeningConc} />
                <div>
                  <strong>{n}</strong>
                  <span>
                    {#if n === 1}Recomendado. No interrumpe el video.
                    {:else if n === SCREENING_MAX}Máximo. Solo si tu red es buena.
                    {:else}Intermedio.
                    {/if}
                  </span>
                </div>
              </label>
            {/each}
          </div>
        </section>

        <section class="block">
          <h2>Servidor web (mando remoto)</h2>
          <p class="hint">
            Levanta un servidor HTTP en la red local para usar el celular como
            mando. Aplica al próximo inicio de Kütral.
          </p>
          <label class="toggle-row">
            <input data-nav type="checkbox" bind:checked={webAuto} />
            <span>Activar al iniciar la app</span>
          </label>
          <label class="field">
            <span class="field-label">Puerto</span>
            <input data-nav
              type="number"
              min="1024"
              max="65535"
              bind:value={webPortInput}
            />
          </label>
        </section>

        <section class="block">
          <h2>Descarga local (si el debrid bloquea)</h2>
          <p class="hint">
            Real-Debrid rechaza ciertos torrents por DMCA (error 451). No es que
            el torrent esté muerto: es cumplimiento legal de ellos. Con esto
            activado, Kütral baja esa fuente por su cuenta y la reproduce
            mientras se descarga.
          </p>
          <p class="warn-box">
            <strong>Ojo con esto:</strong> el debrid funcionaba como intermediario
            — los demás usuarios del torrent veían la IP de Real-Debrid, no la
            tuya. Bajando en local tu IP queda visible para todos los que
            comparten ese archivo. Actívalo solo si sabes lo que implica en tu
            país y con tu proveedor de internet.
          </p>
          <label class="toggle-row">
            <input data-nav type="checkbox" bind:checked={torrentLocal} onchange={aplicarTorrent} />
            <span>Bajar en local cuando el debrid bloquee la fuente</span>
          </label>
          <p class="hint">Esta sección se aplica al instante, sin pulsar Guardar.</p>
          {#if torrentLocal}
            <p class="hint">
              Con el debrid el peso del archivo daba lo mismo: lo servía él a
              velocidad de fibra. Bajándolo tú hay que sostener el bitrate o el
              video se corta — un 4K de 60 GB pide unos 67 Mbps constantes; un
              1080p normal de 4 GB, unos 4,5 Mbps. Estos topes valen
              <strong>solo</strong> para la descarga local.
            </p>
            <div class="mode-group">
              {#each TORRENT_QUALITY_OPTIONS as q}
                <label class="mode-card" class:sel={torrentMaxQuality === q.id}>
                  <input data-nav
                    type="radio"
                    name="torrentMaxQuality"
                    value={q.id}
                    bind:group={torrentMaxQuality}
                    onchange={aplicarTorrent}
                  />
                  <div>
                    <strong>{q.label}</strong>
                    <span>{q.hint}</span>
                  </div>
                </label>
              {/each}
            </div>
            <label class="field">
              <span class="field-label">Peso máximo por archivo (GB)</span>
              <input data-nav
                type="number"
                min={TORRENT_MAX_GB_MIN}
                max={TORRENT_MAX_GB_MAX}
                step="0.5"
                bind:value={torrentMaxGb}
                onchange={aplicarTorrent}
              />
            </label>
            <label class="field">
              <span class="field-label">Buffer antes de reproducir (MB)</span>
              <input data-nav
                type="number"
                min={TORRENT_BUFFER_MIN}
                max={TORRENT_BUFFER_MAX}
                bind:value={torrentBufferMb}
                onchange={aplicarTorrent}
              />
            </label>
            <p class="hint">
              Más buffer = arranca más lento pero se corta menos si el torrent
              tiene pocos usuarios compartiendo.
            </p>
            <label class="field">
              <span class="field-label">Carpeta de descarga</span>
              <input data-nav
                type="text"
                placeholder={torrentDirPath}
                bind:value={torrentDirInput}
                onblur={probarCarpeta}
              />
            </label>
            <div class="dir-row">
              <button data-nav class="link-tiny" onclick={probarCarpeta}>Comprobar carpeta</button>
              {#if torrentDirInput.trim()}
                <button data-nav
                  class="link-tiny"
                  onclick={() => { torrentDirInput = ""; dirEstado = "idle"; dirMsg = ""; aplicarTorrent(); }}
                >
                  Usar la de siempre
                </button>
              {/if}
            </div>
            {#if dirEstado === "probando"}
              <p class="hint"><span class="spinner"></span> Comprobando…</p>
            {:else if dirEstado === "ok"}
              <p class="hint dir-ok">✓ Se puede escribir en <em>{dirMsg}</em></p>
            {:else if dirEstado === "error"}
              <p class="err">{dirMsg}</p>
            {:else}
              <p class="hint">
                Vacío = <em>{torrentDirPath}</em>. Escribe una ruta completa para
                cambiarla (por ejemplo un disco externo). Vale desde la próxima
                descarga; las que ya están en la cola siguen donde estaban.
              </p>
            {/if}
          {/if}
        </section>

        <section class="block">
          <h2>Juegos — Regiones aceptadas</h2>
          <p class="hint">
            Qué versiones de cada juego se muestran en el catálogo, según la
            región del nombre. Por defecto Europa y USA.
          </p>
          <div class="radio-group">
            {#each GAME_REGIONS as r}
              <label class="radio">
                <input data-nav
                  type="checkbox"
                  checked={gameRegions.includes(r.id)}
                  onchange={() => toggleRegion(r.id)}
                />
                <span>{r.label}</span>
              </label>
            {/each}
          </div>
        </section>

        <section class="block">
          <h2>IPTV — Listas de canales</h2>
          <p class="hint">
            Playlists M3U que alimentan los canales en vivo. Podés tener varias
            (iptv-org, tu proveedor, listas propias). Se cargan todas juntas en
            la sección TV.
          </p>

          {#if iptvLists.length}
            <ul class="iptv-listas">
              {#each iptvLists as lista, i}
                <li class="iptv-item">
                  <input data-nav
                    class="iptv-nombre"
                    type="text"
                    bind:value={lista.name}
                    placeholder="Nombre"
                    spellcheck="false"
                  />
                  <input data-nav
                    class="iptv-url"
                    type="text"
                    bind:value={lista.url}
                    placeholder="https://…/lista.m3u"
                    spellcheck="false"
                  />
                  <button data-nav class="iptv-del" title="Quitar" onclick={() => quitarLista(i)}>✕</button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="hint">No hay listas. Agregá una abajo.</p>
          {/if}

          <div class="iptv-add">
            <input data-nav
              class="iptv-nombre"
              type="text"
              bind:value={nuevaListaNombre}
              placeholder="Nombre (opcional)"
              spellcheck="false"
            />
            <input data-nav
              class="iptv-url"
              type="text"
              bind:value={nuevaListaUrl}
              placeholder="https://…/lista.m3u"
              spellcheck="false"
              onkeydown={(e) => e.key === "Enter" && agregarLista()}
            />
            <button data-nav class="iptv-add-btn" onclick={agregarLista} disabled={!nuevaListaUrl.trim()}>
              + Agregar
            </button>
          </div>

          <button data-nav class="btn-ghost" onclick={restaurarListaDefault}>
            Restaurar listas por defecto (Español + global)
          </button>
        </section>

      </div>
    </div>

    <!-- Controles: a todo el ancho, fuera de las columnas. -->
    <section class="block ancho">
      <h2>Controles</h2>
      <p class="hint">
        Los tres mandos comparten las mismas teclas: <strong>teclado</strong>,
        <strong>mando web</strong> (celular) y <strong>mando físico</strong>.
        Pulsa el mando para probarlo (se ilumina) y toca un botón para
        reasignarle una acción.
      </p>

      <Gamepad {actionByBtn} pressed={padPressed} selected={selBtn} {onpick} />

      {#if selBtn !== null}
        <div class="asignar">
          <span>Asignar <b>{nombreBoton(selBtn)}</b> a:</span>
          {#each ACCIONES as a}
            <button data-nav class="acc-chip" onclick={() => asignar(a.id)}>{a.label}</button>
          {/each}
          <button data-nav class="acc-chip cancel" onclick={() => (selBtn = null)}>cancelar</button>
        </div>
      {/if}

      <!-- Referencia teclado/web (fijos) -->
      <div class="ctrl-ref">
        {#each ACCIONES as a}
          <div class="ref-fila">
            <span class="ref-acc">{a.label}</span>
            <kbd>{keyLabel(a.key)}</kbd>
            <span class="ref-web">{a.web ? "web ✓" : ""}</span>
            <span class="ref-btn">{gpMap[a.id] !== undefined ? nombreBoton(gpMap[a.id]) : "—"}</span>
          </div>
        {/each}
      </div>
      <button data-nav class="ctrl-reset" onclick={restaurarControles}>Restaurar por defecto</button>
    </section>

    <!-- Mando dentro del emulador: otro mapeo, otro proceso. -->
    <section class="block ancho">
      <h2>Mando para juegos (emulador)</h2>
      <p class="hint">
        Estos botones son los del <strong>emulador</strong>, no los de la
        interfaz. RetroArch lee el mando por su cuenta, así que la asignación se
        captura acá abajo. Si no tocas nada, se usa la detección automática de
        RetroArch.
      </p>

      {#if padErr}
        <p class="pad-err">{padErr}</p>
      {/if}

      {#if !pads.length}
        <p class="pad-vacio">
          No se detectó ningún mando conectado.
          {#if !padErr}
            Enchúfalo y vuelve a buscar (si sigue sin aparecer, tu usuario tiene
            que estar en el grupo <code>input</code>).
          {/if}
        </p>
        <button data-nav class="btn-ghost" onclick={buscarMandos}>Buscar mandos</button>
      {:else}
        <div class="pad-fila-top">
          <label class="pad-dev">
            Mando:
            <select data-nav bind:value={padSel} onchange={guardarMapaJuegos}>
              {#each pads as p (p.path)}
                <option value={p.path}>{p.name}</option>
              {/each}
            </select>
          </label>
          <button data-nav class="btn-ghost" onclick={buscarMandos}>Buscar de nuevo</button>
        </div>

        <div class="pad-grid">
          {#each RETRO_BOTONES as b (b.id)}
            <div class="pad-row" class:capturando={capturando === b.id}>
              <span class="pad-acc">{b.label}</span>
              <span class="pad-bind">
                {capturando === b.id ? "Presiona un botón…" : bindLabel(padMapJuegos.binds[b.id])}
              </span>
              <button
                data-nav
                class="pad-btn"
                disabled={!!capturando}
                onclick={() => capturarBoton(b.id)}
              >
                {padMapJuegos.binds[b.id] ? "Cambiar" : "Asignar"}
              </button>
              <button
                data-nav
                class="pad-btn borrar"
                disabled={!!capturando || !padMapJuegos.binds[b.id]}
                onclick={() => borrarBoton(b.id)}
              >
                Quitar
              </button>
            </div>
          {/each}
        </div>

        <p class="hint">
          {#if juegosBindsCount === 0}
            Sin asignaciones: manda la detección automática de RetroArch.
          {:else}
            En cuanto asignas un botón, RetroArch deja de usar su detección
            automática: asigna todos los que vayas a usar. Los cambios entran al
            abrir el próximo juego.
            {#if salidaLista}
              Dentro del juego, <strong>Select + Start</strong> cierran el emulador.
            {:else}
              Asigna <strong>Select</strong> y <strong>Start</strong> para poder
              salir del juego con el mando.
            {/if}
          {/if}
        </p>

        <button data-nav class="ctrl-reset" onclick={restaurarMandoJuegos}>
          Restaurar por defecto (borrar asignaciones)
        </button>
      {/if}
    </section>

    <div class="actions">
      <button data-nav class="btn-save" onclick={applyAndSave} disabled={!dirty}>
        {saved ? "Guardado ✓" : "Guardar cambios"}
      </button>
      <p class="hint-actions">
        El modo kiosko activa pantalla completa al guardar.
      </p>
    </div>
  </div>
</div>

<style>
  /* Mando del emulador */
  .pad-fila-top {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }
  .pad-dev {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #b9b9c6;
    font-size: 13px;
  }
  .pad-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 8px;
  }
  .pad-row {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 8px;
  }
  .pad-row.capturando {
    border-color: #f5c518;
    box-shadow: 0 0 0 2px rgba(245, 197, 24, 0.22);
  }
  .pad-acc { font-size: 13px; font-weight: 600; }
  .pad-bind {
    min-width: 96px;
    text-align: right;
    color: #f5c518;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .pad-btn {
    background: #22222c;
    color: #e6e6ec;
    border: 1px solid #33333f;
    border-radius: 6px;
    padding: 5px 10px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .pad-btn:hover:not(:disabled) { background: #f5c518; color: #0d0d12; border-color: #f5c518; }
  .pad-btn:disabled { opacity: 0.45; cursor: default; }
  .pad-btn.borrar { color: #d98a8a; }
  .pad-err { color: #ff8a8a; font-size: 13px; }
  .pad-vacio { color: #b9b9c6; font-size: 13px; }

  .cfg-root {
    height: 100%;
    overflow: auto;
    background: #0d0d12;
    color: #e6e6ec;
    padding: 28px 24px 80px;
  }
  .cfg-wrap {
    max-width: 1480px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  .cfg-head {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .cfg-head h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
  }
  .back {
    background: transparent;
    border: 1px solid #2a2a36;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 7px 14px;
    cursor: pointer;
    font-size: 13px;
  }
  .back:hover { background: #1c1c26; color: #fff; }

  .cfg-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 20px;
    align-items: stretch; /* columnas de igual alto → bottoms parejos */
  }
  /* Sección a todo el ancho (fuera de las columnas): el mando. */
  .block.ancho {
    margin-top: 20px;
  }
  .block.ancho .ctrl-ref {
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  }
  @media (max-width: 1180px) {
    .cfg-grid { grid-template-columns: 1fr 1fr; }
  }
  @media (max-width: 720px) {
    .cfg-grid { grid-template-columns: 1fr; }
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  /* Las cajas crecen para llenar la columna → las 3 columnas terminan al
     mismo nivel (look parejo, "pro"). */
  .col > :global(.block) {
    flex: 1 1 auto;
  }

  .block {
    background: #15151c;
    border: 1px solid #22222c;
    border-radius: 12px;
    padding: 18px 20px;
  }
  .block h2 {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 600;
    color: #f3a951;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .subhead {
    margin: 16px 0 4px;
    font-size: 12px;
    font-weight: 600;
    color: #c98a8a;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .hint {
    margin: 0 0 12px;
    color: #888892;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .warn-box {
    margin: 0 0 12px;
    padding: 10px 12px;
    border: 1px solid #4a3a1c;
    border-left: 3px solid #f3a951;
    border-radius: 6px;
    background: #1b160c;
    color: #cbb489;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .warn-box strong { color: #f3a951; }
  .dir-row { display: flex; gap: 12px; margin: 0 0 8px; }
  .dir-ok { color: #6cd37a; }
  .hint code {
    background: #0d0d12;
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 11.5px;
    color: #d8d8e0;
  }
  .hint em { font-style: normal; color: #f3a951; }
  .badge-opt {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 8px;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #888892;
    background: #1c1c26;
    border: 1px solid #2a2a36;
    border-radius: 999px;
    vertical-align: middle;
  }

  .radio-group { display: flex; flex-direction: column; gap: 6px; }
  .radio {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 8px;
    cursor: pointer;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    font-size: 13.5px;
  }
  .radio:hover { border-color: #2a2a36; }
  .radio input { accent-color: #f3a951; }

  .phone-block { border-color: #3a2c12; background: #15130d; }
  .phone-qr { display: flex; gap: 16px; align-items: flex-start; margin-top: 6px; }
  .phone-steps { flex: 1; min-width: 0; }
  .phone-steps .hint em { color: #f3a951; }
  .phone-url {
    display: inline-block;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    padding: 4px 8px;
    border-radius: 6px;
    font-size: 11.5px;
    color: #d8d8e0;
    word-break: break-all;
  }

  .key-row { display: flex; gap: 8px; }
  .key-row input {
    flex: 1;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    color: #fff;
    padding: 9px 12px;
    border-radius: 8px;
    font-size: 13px;
    font-family: ui-monospace, monospace;
  }
  .key-row input:focus { border-color: #f3a951; outline: none; }
  .reveal {
    background: #1c1c26;
    border: 1px solid #2a2a36;
    color: #d8d8e0;
    border-radius: 8px;
    padding: 0 12px;
    cursor: pointer;
    font-size: 14px;
  }
  .reveal:hover { background: #22222c; }

  .mode-group { display: flex; flex-direction: column; gap: 8px; }
  .mode-card {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 11px 14px;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 10px;
    cursor: pointer;
  }
  .mode-card:hover { border-color: #2a2a36; }
  .mode-card.sel { border-color: #f3a951; background: #1a1410; }
  .mode-card input { margin-top: 3px; accent-color: #f3a951; }
  .mode-card div { display: flex; flex-direction: column; gap: 2px; }
  .mode-card strong { font-size: 13px; color: #fff; }
  .mode-card span { font-size: 12px; color: #a0a0aa; }

  .rd-linked {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 14px;
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    font-size: 13.5px;
  }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .dot.ok { background: #6cd37a; }
  .rd-linked .btn-sec { margin-left: auto; }

  .btn-link {
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    display: block;
    width: 100%;
  }
  .btn-link:hover:not(:disabled) { background: #ffb86b; }
  .btn-link:disabled { opacity: 0.5; cursor: default; }

  .link-tiny {
    margin-top: 8px;
    background: transparent;
    border: 0;
    color: #888892;
    font-size: 12px;
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }
  .link-tiny:hover { color: #d8d8e0; }

  .rd-flow {
    display: flex; flex-direction: column; gap: 10px;
    padding: 14px;
    background: #0d0d12;
    border: 1px solid #2a2a36;
    border-radius: 10px;
  }
  .rd-step { margin: 0; font-size: 13px; color: #c8c8d0; }
  .rd-link {
    background: transparent; border: 0;
    color: #f3a951; text-decoration: underline;
    cursor: pointer; font-size: 13px; padding: 0;
  }
  .rd-link:hover { color: #ffb86b; }
  .rd-code {
    text-align: center;
    background: #1a1410;
    border: 1px solid #f3a951;
    border-radius: 10px;
    padding: 18px 12px;
    font-family: ui-monospace, "SF Mono", monospace;
    font-size: 28px;
    font-weight: 700;
    color: #ffb86b;
    letter-spacing: 0.18em;
    user-select: all;
  }
  .rd-wait {
    margin: 0;
    display: flex; align-items: center; gap: 8px;
    color: #a0a0aa; font-size: 12.5px;
  }
  .rd-wait em {
    font-style: normal; color: #d8d8e0;
    font-variant-numeric: tabular-nums;
  }
  .spinner {
    width: 12px; height: 12px;
    border: 2px solid #2a2a36;
    border-top-color: #f3a951;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .btn-sec {
    background: #2a2a36; color: #d8d8e0;
    border: 0; border-radius: 8px;
    padding: 8px 14px; font-size: 12.5px; cursor: pointer;
  }
  .btn-sec:hover { background: #34343f; }

  .err, .ok {
    margin: 10px 0 0; font-size: 12.5px;
  }
  .err { color: #ff7373; }
  .ok { color: #9be38a; }
  .os-linked { color: #9be38a; font-size: 13px; flex: 1; }

  .actions {
    display: flex; flex-direction: column;
    align-items: flex-start; gap: 8px;
    padding-top: 4px;
  }
  .btn-save {
    background: #f3a951;
    color: #1a1208;
    border: 0;
    border-radius: 8px;
    padding: 12px 28px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-save:hover:not(:disabled) { background: #ffb86b; }
  .btn-save:disabled { opacity: 0.4; cursor: default; }
  .hint-actions {
    margin: 0; color: #6e6e78; font-size: 12px;
  }

  .upd-line {
    margin: 0 0 8px;
    font-size: 13px;
    color: #c8c8d0;
    display: flex; align-items: center; gap: 8px;
  }
  .upd-line strong { color: #f3a951; }
  .upd-ok { color: #9be38a; }
  .upd-notes {
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    padding: 10px 12px;
    max-height: 140px;
    overflow: auto;
    font-size: 11.5px;
    color: #c8c8d0;
    margin: 0 0 10px;
    white-space: pre-wrap;
    font-family: ui-monospace, monospace;
  }
  .bar {
    width: 100%;
    height: 6px;
    background: #1f1f28;
    border-radius: 3px;
    overflow: hidden;
    margin: 4px 0 6px;
  }
  .bar-fill {
    height: 100%;
    background: linear-gradient(90deg, #f3a951, #ffb86b);
    transition: width 0.2s ease;
  }
  .upd-mb {
    margin: 0;
    color: #888892;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .test-btns {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .test-btns .btn-sec { flex: 1; }

  .field { display: flex; flex-direction: column; gap: 4px; margin-top: 8px; }
  .field-label { font-size: 12px; color: #a0a0aa; }
  .field input[type="number"],
  .field input[type="text"],
  .field input[type="password"],
  .field select {
    background: #0b0b0f;
    border: 1px solid #2a2a36;
    border-radius: 6px;
    padding: 6px 8px;
    color: #e6e6ec;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .field input[type="range"] { accent-color: #f3a951; }
  .field input:focus, .field select:focus { border-color: #f3a951; outline: none; }

  /* --- Gestor de listas IPTV --- */
  .iptv-listas { list-style: none; margin: 10px 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .iptv-item, .iptv-add { display: flex; gap: 8px; align-items: center; }
  .iptv-add { margin-top: 12px; }
  .iptv-nombre, .iptv-url {
    background: #0b0b0f;
    border: 1px solid #2a2a36;
    border-radius: 6px;
    padding: 7px 9px;
    color: #e6e6ec;
    font-size: 13px;
  }
  .iptv-nombre { flex: 0 0 150px; }
  .iptv-url { flex: 1 1 auto; min-width: 0; }
  .iptv-nombre:focus, .iptv-url:focus { border-color: #f3a951; outline: none; }
  .iptv-del {
    flex: 0 0 auto;
    background: #2a1518;
    border: 1px solid #4a2a2e;
    color: #f88;
    border-radius: 6px;
    padding: 7px 11px;
    cursor: pointer;
  }
  .iptv-del:hover { background: #3a1d22; }
  .iptv-add-btn {
    flex: 0 0 auto;
    background: #f3a951;
    border: 0;
    color: #1a1206;
    font-weight: 700;
    border-radius: 6px;
    padding: 8px 14px;
    cursor: pointer;
    white-space: nowrap;
  }
  .iptv-add-btn:disabled { opacity: 0.5; cursor: default; }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
    cursor: pointer;
    font-size: 13px;
    color: #e6e6ec;
  }
  .toggle-row input { accent-color: #f3a951; width: 16px; height: 16px; }

  .test-log {
    background: #0d0d12;
    border: 1px solid #1f1f28;
    border-radius: 8px;
    padding: 10px 12px;
    margin: 10px 0 0;
    max-height: 200px;
    overflow: auto;
    font-size: 11.5px;
    line-height: 1.55;
    color: #c8c8d0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: ui-monospace, monospace;
  }

  /* --- Controles --- */
  .asignar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 12px;
    background: #15151c;
    border: 1px solid #f5c518;
    border-radius: 10px;
    margin-bottom: 12px;
  }
  .asignar > span {
    font-size: 14px;
    margin-right: 4px;
  }
  .acc-chip {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #eee;
    padding: 6px 12px;
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
  }
  .acc-chip:hover {
    border-color: #6ec1ff;
  }
  .acc-chip.cancel {
    color: #888;
  }
  .ctrl-ref {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 4px 16px;
    margin-top: 8px;
  }
  .ref-fila {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: 8px;
    padding: 5px 0;
    font-size: 13px;
    border-bottom: 1px solid #1d1d26;
  }
  .ref-acc {
    color: #ddd;
  }
  .ref-web {
    color: #4ade80;
    font-size: 11px;
  }
  .ref-btn {
    color: #9c7bff;
    font-weight: 700;
    min-width: 56px;
    text-align: right;
  }
  .ctrl-reset {
    margin-top: 14px;
    background: transparent;
    border: 1px solid #2a2a36;
    color: #aaa;
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
  }
</style>
