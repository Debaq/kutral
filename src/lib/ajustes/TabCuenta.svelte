<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { config, refreshRdLinked, refreshOsStatus } from "$lib/config.svelte";
  import { setNowPlaying } from "$lib/playerState.svelte";
  import { limpiarContexto } from "$lib/historial.svelte";
  import {
    cuenta,
    cargarCuenta,
    entrarCuenta,
    probarServidor,
    salirCuenta,
    desbloquearCuenta,
    sincronizarCuenta,
    social,
    cargarSocial,
    agregarAmigo,
    quitarAmigo,
    llavero,
    llaveroCodigo,
    llaveroVincular,
    llaveroBorrar,
    llaveroPrestar,
    llaveroDejarDePrestar,
  } from "$lib/cuenta.svelte";
  import type { RdDeviceStart } from "$lib/tipos";
  import { texto, fmtMmSs, haceCuanto } from "./guardar.svelte";

  let showTmdb = $state(false);
  let showOmdb = $state(false);
  let showWyzie = $state(false);

  // Token pegado a mano (avanzado). NO se persiste en localStorage: se manda al
  // store 0600 del backend vía rd_creds_save. El vínculo se lee de config.rdLinked.
  let rdManual = $state("");
  let showRdAdvanced = $state(false);
  let showDiag = $state(false);

  // RD device flow
  let rdStarting = $state(false);
  let rdLink = $state<RdDeviceStart | null>(null);
  let rdWaiting = $state(false);
  let rdErr = $state("");
  let rdOk = $state(false);
  let rdAbort: AbortController | null = null;
  let countdownTimer: number | null = null;
  let secondsLeft = $state(0);

  // OpenSubtitles: login de cuenta gratis (sube cuota a 20/día).
  let osUserInput = $state("");
  let osPassInput = $state("");
  let osBusy = $state(false);
  let osErr = $state("");

  onMount(() => {
    void refreshOsStatus();
    void cargarCuenta().then(() => {
      ctaServidor = cuenta.servidor;
      void cargarSocial();
    });
  });

  onDestroy(() => {
    cancelRd();
  });

  /** Esc corta la vinculación en curso. Devuelve true si había una. */
  export function cancelarFlujo(): boolean {
    if (rdLink) {
      cancelRd();
      return true;
    }
    if (llvLink) {
      llvCancelar();
      return true;
    }
    return false;
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

  // Cuenta de Kutral (opcional): servidor propio para sincronizar entre equipos.
  let ctaServidor = $state("");
  let ctaNombre = $state("");
  let ctaClave = $state("");
  let ctaInvitacion = $state("");
  let ctaCrear = $state(false);
  let ctaBusy = $state(false);
  let ctaErr = $state("");
  let ctaOk = $state("");
  let ctaClaveBoveda = $state("");

  async function probarCta() {
    ctaErr = "";
    ctaOk = "";
    ctaBusy = true;
    try {
      const v = await probarServidor(ctaServidor);
      ctaOk = `Servidor de Kutral encontrado (versión ${v}).`;
    } catch (e) {
      ctaErr = String(e);
    } finally {
      ctaBusy = false;
    }
  }

  async function entrarCta() {
    ctaErr = "";
    ctaOk = "";
    if (!ctaServidor.trim() || !ctaNombre.trim() || !ctaClave) {
      ctaErr = "Completa servidor, nombre y contraseña.";
      return;
    }
    if (ctaCrear && !ctaInvitacion.trim()) {
      ctaErr = "Para crear una cuenta necesitas un código de invitación.";
      return;
    }
    ctaBusy = true;
    try {
      await entrarCuenta(ctaServidor, ctaNombre, ctaClave, ctaCrear ? ctaInvitacion : undefined);
      ctaClave = "";
      ctaInvitacion = "";
    } catch (e) {
      ctaErr = String(e);
    } finally {
      ctaBusy = false;
    }
  }

  async function desbloquearCta() {
    ctaErr = "";
    ctaBusy = true;
    try {
      await desbloquearCuenta(ctaClaveBoveda);
      ctaClaveBoveda = "";
    } catch (e) {
      ctaErr = String(e);
    } finally {
      ctaBusy = false;
    }
  }

  async function salirCta() {
    ctaBusy = true;
    try {
      await salirCuenta();
    } finally {
      ctaBusy = false;
    }
  }

  // Amigos: se agregan por su nombre de usuario en el mismo servidor.
  let amigoNombre = $state("");
  let amigoBusy = $state(false);
  let amigoErr = $state("");

  async function accionAmigo(fn: () => Promise<void>) {
    amigoErr = "";
    amigoBusy = true;
    try {
      await fn();
    } catch (e) {
      amigoErr = String(e);
    } finally {
      amigoBusy = false;
    }
  }

  function pedirAmistad() {
    const n = amigoNombre.trim();
    if (!n) return;
    void accionAmigo(async () => {
      await agregarAmigo(n);
      amigoNombre = "";
    });
  }

  // Llavero: prestar Real-Debrid a amigos con una vinculación aparte.
  let llvLink = $state<RdDeviceStart | null>(null);
  let llvSegundos = $state(0);
  let llvTimer: number | null = null;
  let llvBusy = $state(false);
  let llvErr = $state("");
  let llvConfirmarBorrar = $state(false);

  function llvParar() {
    llvLink = null;
    if (llvTimer !== null) {
      clearInterval(llvTimer);
      llvTimer = null;
    }
  }

  // Cancelar no frena la espera en Rust: cada intento lleva número y solo el
  // último toca la pantalla.
  let llvIntento = 0;

  function llvCancelar() {
    llvIntento++;
    llvParar();
    llvBusy = false;
  }

  async function llvVincular() {
    const yo = ++llvIntento;
    llvParar();
    llvErr = "";
    llvBusy = true;
    try {
      const start = await llaveroCodigo();
      llvLink = start;
      llvSegundos = start.expires_in;
      llvTimer = window.setInterval(() => {
        llvSegundos = Math.max(0, llvSegundos - 1);
        if (llvSegundos === 0) llvCancelar();
      }, 1000);
      await llaveroVincular(start);
    } catch (e) {
      if (yo === llvIntento) llvErr = String(e);
    } finally {
      if (yo === llvIntento) {
        llvParar();
        llvBusy = false;
      }
    }
  }

  async function llvAbrir() {
    if (!llvLink) return;
    try { await openUrl(llvLink.verification_url); } catch {}
  }

  async function llvAccion(fn: () => Promise<void>) {
    llvErr = "";
    llvBusy = true;
    try {
      await fn();
    } catch (e) {
      llvErr = String(e);
    } finally {
      llvBusy = false;
      llvConfirmarBorrar = false;
    }
  }

  const llvPrestadoA = $derived(new Set(llavero.rdMia?.prestada_a.map((p) => p.nombre) ?? []));

  function llvUltimoUso(nombre: string): string {
    const p = llavero.rdMia?.prestada_a.find((x) => x.nombre === nombre);
    if (!p) return "";
    return p.ultimo_uso ? `la usó ${haceCuanto(p.ultimo_uso)}` : "todavía no la usa";
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
</script>

<section class="block">
  <h2>Claves de API</h2>
  <p class="hint">
    Si una clave es muy larga para el control, usa <em>📱 Escribir desde el celular</em>
    arriba.
  </p>
  <label class="field">
    <span class="field-label">TMDb — sin esto no hay catálogo</span>
    <div class="key-row">
      <input data-nav
        type={showTmdb ? "text" : "password"}
        value={config.tmdbKey}
        onchange={(e) => (config.tmdbKey = texto(e))}
        placeholder="tmdb_xxxxxxxxxxxxxxxxxxxxxxxx"
        autocomplete="off"
        spellcheck="false"
      />
      <button data-nav type="button" class="reveal" onclick={() => (showTmdb = !showTmdb)} title={showTmdb ? "Ocultar" : "Mostrar"}>
        {showTmdb ? "🙈" : "👁"}
      </button>
    </div>
  </label>
  <p class="hint">Gratis en <code>themoviedb.org/settings/api</code>.</p>

  <label class="field">
    <span class="field-label">OMDb <span class="badge-opt">opcional</span></span>
    <div class="key-row">
      <input data-nav
        type={showOmdb ? "text" : "password"}
        value={config.omdbKey}
        onchange={(e) => (config.omdbKey = texto(e))}
        placeholder="xxxxxxxx"
        autocomplete="off"
        spellcheck="false"
      />
      <button data-nav type="button" class="reveal" onclick={() => (showOmdb = !showOmdb)} title={showOmdb ? "Ocultar" : "Mostrar"}>
        {showOmdb ? "🙈" : "👁"}
      </button>
    </div>
  </label>
  <p class="hint">
    Gratis en <code>omdbapi.com/apikey.aspx</code>. Habilita premios, recaudación,
    ratings (IMDb/RT/Metacritic) y sinopsis extendida al abrir un título.
  </p>

  <label class="field">
    <span class="field-label">Wyzie (subtítulos) <span class="badge-opt">opcional</span></span>
    <div class="key-row">
      <input data-nav
        type={showWyzie ? "text" : "password"}
        value={config.wyzieKey}
        onchange={(e) => (config.wyzieKey = texto(e))}
        placeholder="Sin key: el reproductor usa OpenSubtitles"
        autocomplete="off"
        spellcheck="false"
      />
      <button data-nav type="button" class="reveal" onclick={() => (showWyzie = !showWyzie)} title={showWyzie ? "Ocultar" : "Mostrar"}>
        {showWyzie ? "🙈" : "👁"}
      </button>
    </div>
  </label>
  <p class="hint">
    Con ella Kütral busca subtítulos ahí (más cuota que OpenSubtitles).
    <a href="https://store.wyzie.io/redeem" target="_blank" rel="noopener">Generar key gratis →</a>
  </p>
</section>

<section class="block">
  <h2>RealDebrid</h2>
  <p class="hint">Vincula tu cuenta para streaming premium sin publicidad.</p>

  {#if config.rdLinked && !rdLink && !llavero.rdPropio && llavero.rdDe}
    <div class="rd-linked">
      <span class="dot ok"></span>
      <span>Usas el Real-Debrid que te presta <strong>{llavero.rdDe}</strong></span>
      <button data-nav class="btn-sec" onclick={startRd} disabled={rdStarting}>Vincular la mía</button>
    </div>
  {:else if config.rdLinked && !rdLink}
    <div class="rd-linked">
      <span class="dot ok"></span>
      <span>Cuenta vinculada</span>
      <button data-nav class="btn-sec" onclick={unlinkRd}>Desvincular</button>
    </div>
  {:else if rdLink}
    <div class="rd-flow">
      <p class="rd-step">1. Abre <button data-nav class="rd-link" onclick={openVerify}>{rdLink.verification_url}</button></p>
      <p class="rd-step">2. Escribe este código:</p>
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

  <div class="diag">
    <button data-nav class="link-tiny" onclick={() => (showDiag = !showDiag)}>
      {showDiag ? "Ocultar diagnóstico" : "Diagnóstico de RealDebrid"}
    </button>
    {#if showDiag}
      <p class="hint" style="margin: 10px 0 8px;">
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
    {/if}
  </div>
</section>

<section class="block">
  <h2>OpenSubtitles <span class="badge-opt">opcional</span></h2>
  {#if !config.osHasKey}
    <p class="hint">
      Subtítulos externos deshabilitados: falta la API key de la app.
      Configúrala en el código (módulo opensubtitles.rs).
    </p>
  {:else if config.osLinked}
    <div class="key-row">
      <span class="os-linked">✓ Vinculada{config.osUser ? ` (${config.osUser})` : ""} — 20 descargas/día</span>
      <button data-nav type="button" class="btn-sec" onclick={unlinkOs}>Desvincular</button>
    </div>
  {:else}
    <p class="hint">
      Sin cuenta usas 5 subtítulos/día (anónimo). Con una cuenta gratis
      suben a 20/día.
      <a href="https://www.opensubtitles.com/newuser" target="_blank" rel="noopener">Crear cuenta gratis →</a>
    </p>
    <div class="key-row">
      <input data-nav type="text" bind:value={osUserInput} placeholder="Usuario" autocomplete="off" />
      <input data-nav type="password" bind:value={osPassInput} placeholder="Contraseña" autocomplete="off" />
    </div>
    {#if osErr}<p class="err">{osErr}</p>{/if}
    <button data-nav type="button" class="btn-link" style="margin-top: 10px;" disabled={osBusy} onclick={loginOs}>
      {osBusy ? "Vinculando…" : "Vincular cuenta"}
    </button>
  {/if}
</section>

<section class="block">
  <h2>Cuenta de Kutral <span class="badge-opt">opcional</span></h2>
  <p class="hint">
    Sincroniza tus favoritos, lo que viste (con el minuto donde quedaste),
    tus listas, tus claves de API, tus ajustes y tus vinculaciones (Real-Debrid,
    OpenSubtitles) entre tus equipos, usando tu propio servidor. Las claves
    viajan cifradas con tu contraseña.
    Sin cuenta, Kutral funciona igual, solo en este equipo.
  </p>

  {#if cuenta.usuario}
    <div class="rd-linked">
      <span class="dot ok"></span>
      <span>Conectado como <strong>{cuenta.usuario}</strong></span>
      <button data-nav class="btn-sec" onclick={salirCta} disabled={ctaBusy}>Cerrar sesión</button>
    </div>
    <p class="hint" style="margin: 10px 0 8px;">
      {cuenta.servidor} · sincronizado {haceCuanto(cuenta.ultimaSync)}
    </p>
    <button data-nav class="btn-sec" onclick={() => void sincronizarCuenta()} disabled={cuenta.sincronizando}>
      {cuenta.sincronizando ? "Sincronizando…" : "Sincronizar ahora"}
    </button>
    {#if !cuenta.boveda}
      <p class="hint" style="margin: 14px 0 8px;">
        Para llevar tus claves, ajustes y Real-Debrid a tus otros equipos,
        confirma tu contraseña una vez en este equipo.
      </p>
      <div class="key-row">
        <input data-nav type="password" bind:value={ctaClaveBoveda} placeholder="contraseña" autocomplete="off" />
        <button data-nav class="btn-sec" onclick={desbloquearCta} disabled={ctaBusy || !ctaClaveBoveda}>
          Confirmar
        </button>
      </div>
    {/if}
    {#if cuenta.error}<p class="err">{cuenta.error}</p>{/if}
  {:else}
    {#if cuenta.error}<p class="err" style="margin: 0 0 10px;">{cuenta.error}</p>{/if}
    <div class="key-row">
      <input data-nav
        bind:value={ctaServidor}
        placeholder="https://mi-servidor.cl/kutral"
        autocomplete="off"
        spellcheck="false"
      />
      <button data-nav class="btn-sec" onclick={probarCta} disabled={ctaBusy || !ctaServidor.trim()}>
        Probar
      </button>
    </div>
    <div class="key-row" style="margin-top: 8px;">
      <input data-nav bind:value={ctaNombre} placeholder="nombre" autocomplete="off" spellcheck="false" />
      <input data-nav type="password" bind:value={ctaClave} placeholder="contraseña" autocomplete="off" />
    </div>
    {#if ctaCrear}
      <div class="key-row" style="margin-top: 8px;">
        <input data-nav bind:value={ctaInvitacion} placeholder="código de invitación (XXXXX-XXXXX)" autocomplete="off" spellcheck="false" />
      </div>
    {/if}
    <button data-nav class="btn-link" style="margin-top: 10px;" onclick={entrarCta} disabled={ctaBusy}>
      {ctaBusy ? "Conectando…" : ctaCrear ? "Crear cuenta" : "Entrar"}
    </button>
    <button data-nav class="link-tiny" onclick={() => (ctaCrear = !ctaCrear)}>
      {ctaCrear ? "Ya tengo cuenta" : "Tengo una invitación: crear cuenta"}
    </button>
  {/if}
  {#if ctaErr}<p class="err">{ctaErr}</p>{/if}
  {#if ctaOk}<p class="ok">{ctaOk}</p>{/if}
</section>

{#if cuenta.usuario}
  <section class="block">
    <h2>Amigos</h2>
    <p class="hint">
      Agrega a alguien por su nombre de usuario en tu mismo servidor. Cuando
      acepte, podrán recomendarse películas y series desde la ficha de cada título.
    </p>
    <div class="key-row">
      <input data-nav
        bind:value={amigoNombre}
        placeholder="nombre de tu amigo"
        autocomplete="off"
        spellcheck="false"
        onkeydown={(e) => e.key === "Enter" && pedirAmistad()}
      />
      <button data-nav class="btn-sec" onclick={pedirAmistad} disabled={amigoBusy || !amigoNombre.trim()}>
        Agregar
      </button>
    </div>

    {#if social.recibidas.length}
      <p class="subhead amigos-sub">Te quieren agregar</p>
      {#each social.recibidas as a (a.nombre)}
        <div class="amigo">
          <span>{a.nombre}</span>
          <button data-nav class="btn-link" onclick={() => accionAmigo(() => agregarAmigo(a.nombre))} disabled={amigoBusy}>
            Aceptar
          </button>
          <button data-nav class="btn-sec" onclick={() => accionAmigo(() => quitarAmigo(a.nombre))} disabled={amigoBusy}>
            Rechazar
          </button>
        </div>
      {/each}
    {/if}

    {#if social.amigos.length}
      <p class="subhead amigos-sub">Tus amigos</p>
      {#each social.amigos as a (a.nombre)}
        <div class="amigo">
          <span class="dot ok"></span>
          <span>{a.nombre}</span>
          <button data-nav class="btn-sec" onclick={() => accionAmigo(() => quitarAmigo(a.nombre))} disabled={amigoBusy}>
            Quitar
          </button>
        </div>
      {/each}
    {/if}

    {#if social.enviadas.length}
      <p class="subhead amigos-sub">Esperando respuesta</p>
      {#each social.enviadas as a (a.nombre)}
        <div class="amigo">
          <span class="tenue">{a.nombre}</span>
          <button data-nav class="btn-sec" onclick={() => accionAmigo(() => quitarAmigo(a.nombre))} disabled={amigoBusy}>
            Cancelar
          </button>
        </div>
      {/each}
    {/if}

    {#if amigoErr || social.error}<p class="err">{amigoErr || social.error}</p>{/if}
  </section>

  <section class="block">
    <h2>Prestar Real-Debrid</h2>
    <p class="hint">
      Tus amigos usan tu Real-Debrid sin conocer tu contraseña ni tu clave: el
      servidor les entrega solo un permiso temporal y puedes dejar de prestarlo
      cuando quieras. Usa una vinculación aparte, así la de tus equipos no se toca.
    </p>
    <p class="warn-box">
      Real-Debrid prohíbe compartir la cuenta y puede bloquearla si la usan
      desde varios lugares a la vez. Al dejar de prestar, a tu amigo se le
      corta en ese momento.
    </p>

    {#if llvLink}
      <div class="rd-flow">
        <p class="rd-step">1. Abre <button data-nav class="rd-link" onclick={llvAbrir}>{llvLink.verification_url}</button></p>
        <p class="rd-step">2. Escribe este código:</p>
        <div class="rd-code">{llvLink.user_code}</div>
        <p class="rd-wait">
          <span class="spinner"></span>
          Esperando autorización… <em>{fmtMmSs(llvSegundos)}</em>
        </p>
        <button data-nav class="btn-sec" onclick={llvCancelar}>Cancelar</button>
      </div>
    {:else if !llavero.rdMia}
      <button data-nav class="btn-link" onclick={llvVincular} disabled={llvBusy}>
        {llvBusy ? "Conectando…" : "Vincular para prestar"}
      </button>
    {:else}
      {#if !social.amigos.length}
        <p class="hint">Agrega amigos en el bloque Amigos para poder prestarles tu cuenta.</p>
      {/if}
      {#each social.amigos as a (a.nombre)}
        {@const prestado = llvPrestadoA.has(a.nombre)}
        <div class="amigo">
          {#if prestado}<span class="dot ok"></span>{/if}
          <span>{a.nombre}{#if prestado}<span class="tenue"> · {llvUltimoUso(a.nombre)}</span>{/if}</span>
          {#if prestado}
            <button data-nav class="btn-sec" onclick={() => llvAccion(() => llaveroDejarDePrestar(a.nombre))} disabled={llvBusy}>
              Dejar de prestar
            </button>
          {:else}
            <button data-nav class="btn-link" onclick={() => llvAccion(() => llaveroPrestar(a.nombre))} disabled={llvBusy}>
              Prestar
            </button>
          {/if}
        </div>
      {/each}
      {#if llvConfirmarBorrar}
        <p class="hint" style="margin: 14px 0 8px;">
          Se deja de prestar a todos y el servidor olvida esta vinculación.
        </p>
        <button data-nav class="btn-sec" onclick={() => llvAccion(llaveroBorrar)} disabled={llvBusy}>Sí, borrar</button>
        <button data-nav class="link-tiny" onclick={() => (llvConfirmarBorrar = false)}>Cancelar</button>
      {:else}
        <button data-nav class="link-tiny" onclick={() => (llvConfirmarBorrar = true)}>
          Borrar la vinculación del llavero
        </button>
      {/if}
    {/if}
    {#if llvErr || llavero.error}<p class="err">{llvErr || llavero.error}</p>{/if}
  </section>
{/if}
