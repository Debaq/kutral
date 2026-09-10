<script lang="ts">
  // Asistente inicial. Se abre solo la primera vez (o desde Configuración →
  // "Repetir asistente") y deja la app en un estado donde algo se puede ver:
  // catálogo y una forma de reproducir.
  //
  // Todo paso es salteable. El único que no se puede saltar de verdad es el de
  // reproducción, porque sin debrid ni descarga local no hay nada que hacer —
  // pero incluso ahí se deja seguir, con el aviso puesto.

  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import RemoteQr from "$lib/RemoteQr.svelte";
  import WifiManager from "$lib/WifiManager.svelte";
  import { navegar, enfocarPrimero } from "$lib/nav";
  import { marcarOnboarding } from "$lib/onboarding";
  import {
    config,
    loadConfig,
    saveConfig,
    refreshRdLinked,
    LANGS,
    TORRENT_QUALITY_OPTIONS,
    TORRENT_BUFFER_OPTIONS,
    type Lang,
    type ModeOverride,
    type SubMode,
  } from "$lib/config.svelte";

  type RdDeviceStart = {
    device_code: string;
    user_code: string;
    verification_url: string;
    interval: number;
    expires_in: number;
  };
  type WebStatus = { running: boolean; ip: string | null; port: number | null; url: string | null };
  type WifiState = { online: boolean; connected_ssid: string | null };

  // --- Pasos ---------------------------------------------------------------
  // El de red solo aparece si al arrancar no hay internet: en un escritorio
  // normal preguntar por wifi es ruido.
  type PasoId = "idioma" | "que-es" | "red" | "catalogo" | "reproducir" | "opcionales" | "listo";

  let hayRed = $state(true);
  const pasos = $derived<PasoId[]>(
    hayRed
      ? ["idioma", "que-es", "catalogo", "reproducir", "opcionales", "listo"]
      : ["idioma", "que-es", "red", "catalogo", "reproducir", "opcionales", "listo"],
  );
  let i = $state(0);
  const paso = $derived(pasos[Math.min(i, pasos.length - 1)]);

  function siguiente() {
    if (i < pasos.length - 1) i++;
    else terminar();
  }
  function atras() {
    if (i > 0) i--;
  }
  function terminar() {
    guardar();
    marcarOnboarding();
    goto("/");
  }
  function saltarTodo() {
    guardar();
    marcarOnboarding();
    goto("/");
  }

  // Los campos escriben directo en el store y se persisten al avanzar: si la
  // app se cierra a medias, lo que ya se contestó quedó guardado.
  function guardar() {
    config.lang = lang;
    config.modeOverride = mode;
    config.tmdbKey = tmdb.trim();
    config.omdbKey = omdb.trim();
    config.subMode = subMode;
    saveConfig();
  }

  // --- Estado de los campos -------------------------------------------------
  let lang = $state<Lang>("es-CL");
  let mode = $state<ModeOverride>("auto");
  let tmdb = $state("");
  let showTmdb = $state(false);
  let omdb = $state("");
  let subMode = $state<SubMode>("dub");
  let wifiOpen = $state(false);

  // Teclado por celular: el QR abre /api del servidor web, que inyecta el texto
  // escrito (o escaneado con la cámara) en el campo enfocado de la app.
  let phoneUrl = $state("");
  let phoneErr = $state("");
  async function conectarCelular() {
    phoneErr = "";
    try {
      let st = await invoke<WebStatus>("web_server_status");
      if (!st.running) st = await invoke<WebStatus>("web_server_start", { port: config.webPort });
      if (st.url) phoneUrl = `${st.url}/api`;
      else phoneErr = "No se pudo obtener la IP de la red local.";
    } catch (e) {
      phoneErr = String(e);
    }
  }

  // --- Debrid: device flow --------------------------------------------------
  let rdLink = $state<RdDeviceStart | null>(null);
  let rdStarting = $state(false);
  let rdWaiting = false;
  let rdErr = $state("");
  let rdManual = $state("");
  let showRdManual = $state(false);
  let secondsLeft = $state(0);
  let countdown: number | null = null;

  async function startRd() {
    cancelRd();
    rdErr = "";
    rdStarting = true;
    try {
      const start = await invoke<RdDeviceStart>("rd_device_start");
      rdLink = start;
      secondsLeft = start.expires_in;
      countdown = window.setInterval(() => {
        secondsLeft = Math.max(0, secondsLeft - 1);
        if (secondsLeft === 0) cancelRd();
      }, 1000);
      rdWaiting = true;
      void pollRd(start);
    } catch (e) {
      rdErr = String(e);
    } finally {
      rdStarting = false;
    }
  }

  async function pollRd(start: RdDeviceStart) {
    try {
      await invoke("rd_device_poll", {
        deviceCode: start.device_code,
        interval: start.interval,
        expiresIn: start.expires_in,
      });
      await refreshRdLinked();
      rdLink = null;
      stopCountdown();
    } catch (e) {
      if (!rdWaiting) return; // cancelado por el usuario
      rdErr = String(e);
    } finally {
      rdWaiting = false;
    }
  }

  function cancelRd() {
    rdWaiting = false;
    rdLink = null;
    stopCountdown();
  }
  function stopCountdown() {
    if (countdown !== null) {
      clearInterval(countdown);
      countdown = null;
    }
  }
  function fmtMmSs(s: number): string {
    return `${Math.floor(s / 60)}:${(s % 60).toString().padStart(2, "0")}`;
  }
  async function abrirVerificacion() {
    if (!rdLink) return;
    try { await openUrl(rdLink.verification_url); } catch { /* sin navegador */ }
  }

  // Token pegado a mano: no pasa por localStorage, va al store 0600 del backend.
  async function guardarTokenRd() {
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
      showRdManual = false;
    } catch (e) {
      rdErr = String(e);
    }
  }

  async function desvincularRd() {
    try { await invoke("rd_creds_clear"); } catch { /* no estaba */ }
    await refreshRdLinked();
  }

  // --- Descarga local -------------------------------------------------------
  // Se aplica al instante: es la vía de reproducción de quien no pone debrid,
  // y el paso siguiente ya la da por hecha.
  function toggleLocal(v: boolean) {
    config.torrentLocal = v;
    saveConfig();
  }
  function setCalidad(q: (typeof TORRENT_QUALITY_OPTIONS)[number]["id"]) {
    config.torrentMaxQuality = q;
    saveConfig();
  }
  function setBuffer(mb: number) {
    config.torrentBufferMb = mb;
    saveConfig();
  }

  // --- Resumen final --------------------------------------------------------
  const puedeVerCatalogo = $derived(!!tmdb.trim());
  const puedeReproducir = $derived(config.rdLinked || config.torrentLocal);

  onMount(async () => {
    loadConfig();
    lang = config.lang;
    mode = config.modeOverride;
    tmdb = config.tmdbKey;
    omdb = config.omdbKey;
    subMode = config.subMode;
    await refreshRdLinked();
    try {
      const w = await invoke<WifiState>("wifi_status");
      hayRed = w.online;
    } catch {
      // wifi_status no existe fuera de Linux/Kütral OS: se asume que hay red.
      hayRed = true;
    }
    void conectarCelular();
    setTimeout(() => enfocarPrimero(document.querySelector(".ob-body") ?? document), 80);
  });

  onDestroy(() => {
    cancelRd();
  });

  // Al cambiar de paso: guardar lo contestado y llevar el foco al primer
  // control del paso nuevo, o el mando se queda sin punto de partida.
  let ultimoPaso = "";
  $effect(() => {
    const p = paso;
    if (p === ultimoPaso) return;
    ultimoPaso = p;
    guardar();
    setTimeout(() => enfocarPrimero(document.querySelector(".ob-body") ?? document), 60);
  });

  const TEXTO = ["text", "password", "search", "url", "email", "tel", "number"];

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    const tag = t?.tagName;
    const tipo = (t as HTMLInputElement | null)?.type ?? "";
    const inText = tag === "INPUT" || tag === "TEXTAREA" || (t?.isContentEditable ?? false);

    if (e.key === "Escape") {
      e.preventDefault();
      if (rdLink) { cancelRd(); return; }
      atras();
      return;
    }
    if (e.key === "Backspace") {
      if (inText) return; // borra texto
      e.preventDefault();
      atras();
      return;
    }
    if (e.key.startsWith("Arrow")) {
      const horizontal = e.key === "ArrowLeft" || e.key === "ArrowRight";
      if (horizontal && tag === "INPUT" && TEXTO.includes(tipo)) return;
      if (tag === "SELECT") return;
      e.preventDefault();
      navegar(e.key.slice(5).toLowerCase() as "up" | "down" | "left" | "right");
      return;
    }
    // El botón A del mando manda Enter: sobre radio/checkbox tiene que marcar.
    if (e.key === "Enter" && tag === "INPUT" && (tipo === "radio" || tipo === "checkbox")) {
      e.preventDefault();
      (t as HTMLInputElement).click();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="ob-root">
  <div class="ob-card">
    <header class="ob-head" data-section="ob-head">
      <span class="ob-marca">Kütral</span>
      <div class="ob-pasos" aria-label="Progreso">
        {#each pasos as p, n}
          <span class="ob-punto" class:on={n <= i} title={p}></span>
        {/each}
      </div>
      <button data-nav class="ob-saltar" onclick={saltarTodo}>Saltar</button>
    </header>

    <div class="ob-body" data-section="ob-body">
      {#if paso === "idioma"}
        <h1>Idioma y pantalla</h1>
        <p class="ob-sub">Se cambia después en Configuración.</p>
        <div class="ob-group">
          {#each LANGS as l}
            <label class="ob-card-op" class:sel={lang === l.id}>
              <input data-nav type="radio" name="lang" value={l.id} bind:group={lang} />
              <strong>{l.label}</strong>
            </label>
          {/each}
        </div>
        <h2>Modo</h2>
        <div class="ob-group">
          <label class="ob-card-op" class:sel={mode === "auto"}>
            <input data-nav type="radio" name="mode" value="auto" bind:group={mode} />
            <div><strong>Automático</strong><span>Pantalla completa en Kütral OS, ventana en el resto.</span></div>
          </label>
          <label class="ob-card-op" class:sel={mode === "kiosk"}>
            <input data-nav type="radio" name="mode" value="kiosk" bind:group={mode} />
            <div><strong>TV</strong><span>Pantalla completa siempre. Para usar con mando.</span></div>
          </label>
          <label class="ob-card-op" class:sel={mode === "desktop"}>
            <input data-nav type="radio" name="mode" value="desktop" bind:group={mode} />
            <div><strong>Escritorio</strong><span>Ventana normal.</span></div>
          </label>
        </div>

      {:else if paso === "que-es"}
        <h1>Qué necesitas</h1>
        <p class="ob-sub">Kütral no trae ninguna cuenta incluida. Esto es lo que hay.</p>
        <table class="ob-tabla">
          <thead>
            <tr><th>Sección</th><th>Qué hace falta</th></tr>
          </thead>
          <tbody>
            <tr><td>Anime, IPTV, Juegos</td><td class="ok">Nada. Funcionan ya.</td></tr>
            <tr><td>Subtítulos</td><td class="ok">Nada.</td></tr>
            <tr><td>Películas y series</td><td>Key de TMDb — gratis, un formulario.</td></tr>
            <tr><td>Reproducir</td><td>Cuenta de debrid (de pago) o descarga local.</td></tr>
          </tbody>
        </table>
        <p class="ob-nota">Los dos pasos que vienen cubren esas dos filas.</p>

      {:else if paso === "red"}
        <h1>Sin conexión</h1>
        <p class="ob-sub">Kütral necesita internet para todo salvo lo ya descargado.</p>
        <button data-nav class="ob-btn" onclick={() => (wifiOpen = true)}>Conectar a una red wifi</button>
        <p class="ob-nota">Si estás por cable y aun así aparece esto, sigue de largo.</p>

      {:else if paso === "catalogo"}
        <h1>Catálogo de películas y series</h1>
        <p class="ob-sub">
          Kütral usa TMDb para el catálogo. La key es gratis y se pide en
          <code>themoviedb.org/settings/api</code>. Sin ella, Pelis y Series
          quedan vacías; Anime, IPTV y Juegos funcionan igual.
        </p>
        <div class="ob-row">
          <input data-nav
            type={showTmdb ? "text" : "password"}
            bind:value={tmdb}
            placeholder="tmdb_xxxxxxxxxxxxxxxxxxxxxxxx"
            autocomplete="off"
            spellcheck="false"
          />
          <button data-nav class="ob-mini" onclick={() => (showTmdb = !showTmdb)}>
            {showTmdb ? "🙈" : "👁"}
          </button>
        </div>
        {#if phoneUrl}
          <div class="ob-phone">
            <RemoteQr url={phoneUrl} label="Teclado" size={110} />
            <p>
              Escanea con el celular para escribir, pegar o leer la key con la
              cámara. Aparece sola en el campo de arriba.
            </p>
          </div>
        {:else if phoneErr}
          <p class="ob-err">Teclado por celular no disponible: {phoneErr}</p>
        {/if}

      {:else if paso === "reproducir"}
        <h1>Cómo reproducir</h1>
        <p class="ob-sub">
          Los buscadores de fuentes devuelven enlaces torrent. Hay dos formas de
          convertirlos en video.
        </p>

        <section class="ob-op">
          <h2>Con debrid <span class="ob-tag">de pago</span></h2>
          <p>
            Real-Debrid baja el torrent por ti y te lo sirve como un archivo
            normal: rápido, y tu IP no aparece en el torrent.
          </p>
          {#if config.rdLinked}
            <p class="ob-ok">✓ Cuenta vinculada.
              <button data-nav class="ob-link" onclick={desvincularRd}>Desvincular</button>
            </p>
          {:else if rdLink}
            <div class="ob-rd">
              <RemoteQr url={rdLink.verification_url} label="Vincular" size={110} />
              <div>
                <p>
                  Abre
                  <button data-nav class="ob-link" onclick={abrirVerificacion}>{rdLink.verification_url}</button>
                  y escribe este código:
                </p>
                <div class="ob-code">{rdLink.user_code}</div>
                <p class="ob-wait">Esperando autorización… {fmtMmSs(secondsLeft)}</p>
                <button data-nav class="ob-mini" onclick={cancelRd}>Cancelar</button>
              </div>
            </div>
          {:else}
            <button data-nav class="ob-btn" onclick={startRd} disabled={rdStarting}>
              {rdStarting ? "Conectando…" : "Vincular cuenta"}
            </button>
            <button data-nav class="ob-link" onclick={() => (showRdManual = !showRdManual)}>
              {showRdManual ? "Ocultar" : "Pegar un token en vez de vincular"}
            </button>
            {#if showRdManual}
              <div class="ob-row">
                <input data-nav type="password" bind:value={rdManual} placeholder="access_token" autocomplete="off" spellcheck="false" />
                <button data-nav class="ob-mini" onclick={guardarTokenRd} disabled={!rdManual.trim()}>Guardar</button>
              </div>
            {/if}
          {/if}
          {#if rdErr}<p class="ob-err">{rdErr}</p>{/if}
        </section>

        <section class="ob-op">
          <h2>Sin debrid: descarga local <span class="ob-tag gratis">gratis</span></h2>
          <p>
            Kütral baja el torrent y lo reproduce mientras se descarga.
            <strong>Tu IP queda visible</strong> para el resto de la gente que
            comparte ese archivo, y la velocidad depende de cuántos lo compartan.
          </p>
          <label class="ob-toggle">
            <input data-nav type="checkbox" checked={config.torrentLocal} onchange={(e) => toggleLocal(e.currentTarget.checked)} />
            <span>Activar descarga local</span>
          </label>
          {#if config.torrentLocal}
            <h3>Calidad máxima</h3>
            <div class="ob-group">
              {#each TORRENT_QUALITY_OPTIONS as q}
                <label class="ob-card-op" class:sel={config.torrentMaxQuality === q.id}>
                  <input data-nav type="radio" name="obq" value={q.id} checked={config.torrentMaxQuality === q.id} onchange={() => setCalidad(q.id)} />
                  <div><strong>{q.label}</strong><span>{q.hint}</span></div>
                </label>
              {/each}
            </div>
            <h3>Cuánto bajar antes de empezar a ver</h3>
            <div class="ob-group">
              {#each TORRENT_BUFFER_OPTIONS as b}
                <label class="ob-card-op" class:sel={config.torrentBufferMb === b.mb}>
                  <input data-nav type="radio" name="obb" value={b.mb} checked={config.torrentBufferMb === b.mb} onchange={() => setBuffer(b.mb)} />
                  <div><strong>{b.label}</strong><span>{b.hint}</span></div>
                </label>
              {/each}
            </div>
          {/if}
        </section>

        {#if !puedeReproducir}
          <p class="ob-warn">
            Sin ninguna de las dos no se puede reproducir nada. Puedes seguir y
            decidirlo después en Configuración.
          </p>
        {/if}

      {:else if paso === "opcionales"}
        <h1>Opcionales</h1>
        <p class="ob-sub">Nada de esto hace falta. Se puede dejar en blanco.</p>
        <h2>Audio</h2>
        <div class="ob-group">
          <label class="ob-card-op" class:sel={subMode === "dub"}>
            <input data-nav type="radio" name="submode" value="dub" bind:group={subMode} />
            <div><strong>Doblado</strong><span>Prioriza audio en español.</span></div>
          </label>
          <label class="ob-card-op" class:sel={subMode === "sub"}>
            <input data-nav type="radio" name="submode" value="sub" bind:group={subMode} />
            <div><strong>Subtitulado</strong><span>Audio original + subtítulos en español.</span></div>
          </label>
        </div>
        <h2>Key de OMDb</h2>
        <p class="ob-sub">
          Gratis en <code>omdbapi.com/apikey.aspx</code>. Agrega premios,
          recaudación y ratings de IMDb / Rotten Tomatoes al detalle.
        </p>
        <div class="ob-row">
          <input data-nav type="text" bind:value={omdb} placeholder="xxxxxxxx" autocomplete="off" spellcheck="false" />
        </div>

      {:else}
        <h1>Listo</h1>
        <ul class="ob-check">
          <li class:ok={puedeVerCatalogo}>
            {puedeVerCatalogo ? "✓" : "—"} Películas y series
            {#if !puedeVerCatalogo}<span> · falta la key de TMDb</span>{/if}
          </li>
          <li class="ok">✓ Anime, IPTV y Juegos</li>
          <li class:ok={puedeReproducir}>
            {puedeReproducir ? "✓" : "—"} Reproducción
            {#if config.rdLinked}<span> · debrid</span>
            {:else if config.torrentLocal}<span> · descarga local</span>
            {:else}<span> · sin debrid ni descarga local</span>{/if}
          </li>
        </ul>
        <p class="ob-nota">Todo esto se cambia en Configuración, cuando quieras.</p>
      {/if}
    </div>

    <footer class="ob-pie" data-section="ob-pie">
      <button data-nav class="ob-mini" onclick={atras} disabled={i === 0}>Atrás</button>
      <button data-nav class="ob-btn" onclick={siguiente}>
        {paso === "listo" ? "Empezar" : "Siguiente"}
      </button>
    </footer>
  </div>
</div>

<WifiManager bind:open={wifiOpen} />

<style>
  .ob-root {
    height: 100%;
    overflow: auto;
    background: #0d0d12;
    color: #e6e6ec;
    padding: 28px 24px 48px;
    display: flex;
    justify-content: center;
  }
  .ob-card {
    width: 100%;
    max-width: 760px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .ob-head {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .ob-marca {
    font-weight: 700;
    color: #f5c518;
    letter-spacing: 0.5px;
  }
  .ob-pasos {
    display: flex;
    gap: 6px;
    flex: 1;
  }
  .ob-punto {
    width: 22px;
    height: 3px;
    border-radius: 2px;
    background: #2a2a35;
  }
  .ob-punto.on { background: #f5c518; }
  .ob-saltar {
    background: transparent;
    border: 1px solid #2a2a36;
    color: #b9b9c6;
    border-radius: 8px;
    padding: 6px 12px;
    font-size: 13px;
    cursor: pointer;
  }
  .ob-body { display: flex; flex-direction: column; gap: 14px; }
  h1 { margin: 0; font-size: 26px; font-weight: 600; }
  h2 { margin: 6px 0 0; font-size: 16px; font-weight: 600; }
  h3 { margin: 8px 0 0; font-size: 13px; font-weight: 600; color: #b9b9c6; }
  .ob-sub, .ob-nota { margin: 0; color: #b9b9c6; font-size: 14px; line-height: 1.5; }
  .ob-nota { font-size: 13px; }
  code {
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 5px;
    padding: 1px 5px;
    font-size: 12px;
  }

  .ob-group { display: flex; flex-wrap: wrap; gap: 8px; }
  .ob-card-op {
    flex: 1 1 200px;
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 11px 13px;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 10px;
    cursor: pointer;
  }
  .ob-card-op.sel { border-color: #f5c518; background: #1a1a12; }
  .ob-card-op:focus-within { box-shadow: 0 0 0 2px rgba(245, 197, 24, 0.35); }
  .ob-card-op strong { display: block; font-size: 14px; }
  .ob-card-op span { display: block; color: #b9b9c6; font-size: 12px; margin-top: 2px; }

  .ob-row { display: flex; gap: 8px; align-items: center; }
  .ob-row input {
    flex: 1;
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 8px;
    color: #e6e6ec;
    padding: 10px 12px;
    font-size: 14px;
  }
  .ob-row input:focus { outline: none; border-color: #f5c518; }

  .ob-btn {
    background: #f5c518;
    color: #0d0d12;
    border: none;
    border-radius: 8px;
    padding: 10px 20px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    align-self: flex-start;
  }
  .ob-btn:disabled { opacity: 0.5; cursor: default; }
  .ob-mini {
    background: #22222c;
    color: #e6e6ec;
    border: 1px solid #33333f;
    border-radius: 8px;
    padding: 9px 14px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .ob-mini:disabled { opacity: 0.45; cursor: default; }
  .ob-link {
    background: transparent;
    border: none;
    color: #f5c518;
    font-size: 13px;
    cursor: pointer;
    padding: 4px 0;
    text-align: left;
    align-self: flex-start;
    text-decoration: underline;
  }

  .ob-tabla { border-collapse: collapse; width: 100%; font-size: 14px; }
  .ob-tabla th, .ob-tabla td {
    text-align: left;
    padding: 9px 10px;
    border-bottom: 1px solid #22222c;
  }
  .ob-tabla th { color: #b9b9c6; font-weight: 600; font-size: 12px; }
  .ob-tabla td.ok { color: #8fd18f; }

  .ob-op {
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 12px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .ob-op p { margin: 0; font-size: 14px; line-height: 1.5; color: #d0d0d9; }
  .ob-tag {
    font-size: 11px;
    font-weight: 600;
    color: #0d0d12;
    background: #b9b9c6;
    border-radius: 5px;
    padding: 2px 6px;
    vertical-align: middle;
  }
  .ob-tag.gratis { background: #8fd18f; }

  .ob-toggle { display: flex; align-items: center; gap: 9px; font-size: 14px; cursor: pointer; }

  .ob-rd { display: flex; gap: 16px; align-items: flex-start; flex-wrap: wrap; }
  .ob-code {
    font-size: 26px;
    font-weight: 700;
    letter-spacing: 3px;
    color: #f5c518;
    margin: 6px 0;
  }
  .ob-wait { color: #b9b9c6; font-size: 13px; }

  .ob-phone { display: flex; gap: 14px; align-items: center; }
  .ob-phone p { margin: 0; color: #b9b9c6; font-size: 13px; line-height: 1.5; }

  .ob-ok { color: #8fd18f; }
  .ob-err { color: #ff8a8a; font-size: 13px; margin: 0; }
  .ob-warn {
    margin: 0;
    background: #2a1f10;
    border: 1px solid #6b4d15;
    border-radius: 10px;
    padding: 11px 13px;
    font-size: 13px;
    line-height: 1.5;
  }

  .ob-check { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .ob-check li {
    background: #15151c;
    border: 1px solid #2a2a35;
    border-radius: 9px;
    padding: 11px 13px;
    font-size: 14px;
    color: #b9b9c6;
  }
  .ob-check li.ok { color: #e6e6ec; border-color: #35502f; }
  .ob-check li span { color: #b9b9c6; font-size: 13px; }

  .ob-pie {
    display: flex;
    gap: 10px;
    align-items: center;
    justify-content: flex-end;
    border-top: 1px solid #22222c;
    padding-top: 14px;
  }
</style>
