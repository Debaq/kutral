<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    config,
    idiomaContenido,
    nombreIdioma,
    SUB_LANGS,
    SCREENING_MIN,
    SCREENING_MAX,
    CACHE_PRESETS,
    CACHE_WAIT_MIN,
    CACHE_WAIT_MAX,
    CACHE_WAIT_MAX_DEFAULT,
    cachePresetActual,
  } from "$lib/config.svelte";
  import { setConcurrenciaScreening } from "$lib/screening.svelte";
  import { texto, numero } from "./guardar.svelte";

  let syncBorradas = $state(false);

  async function borrarSincronias() {
    try {
      await invoke("sub_sync_clear");
      syncBorradas = true;
      setTimeout(() => (syncBorradas = false), 2500);
    } catch (e) {
      console.warn("[sync] borrar", e);
    }
  }
</script>

<section class="block">
  <h2>Audio y subtítulos</h2>
  <p class="hint">
    Cómo prefieres ver. Kütral ordena las fuentes y elige la pista
    correcta del archivo. Si el release no la trae, busca subtítulos.
  </p>
  <div class="mode-group">
    <label class="mode-card" class:sel={config.subMode === "dub"}>
      <input data-nav type="radio" name="subMode" value="dub" bind:group={config.subMode} />
      <div>
        <strong>Doblado</strong>
        <span>
          {idiomaContenido() === "es"
            ? "Audio en español primero (Latino / Castellano)."
            : `Audio en ${nombreIdioma(idiomaContenido())} primero.`}
        </span>
      </div>
    </label>
    <label class="mode-card" class:sel={config.subMode === "sub"}>
      <input data-nav type="radio" name="subMode" value="sub" bind:group={config.subMode} />
      <div>
        <strong>Subtitulado</strong>
        <span>
          Audio original con subtítulos en {nombreIdioma(config.subsLang !== "off" ? config.subsLang : idiomaContenido())}.
        </span>
      </div>
    </label>
  </div>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.verifyEsTracks} />
    <span>Verificar español real antes de reproducir</span>
  </label>
  <p class="hint">
    Inspecciona las pistas del archivo (sin descargarlo) y confirma si
    trae audio o subtítulos en español. Si no trae, prueba la siguiente
    fuente. Agrega unos segundos por fuente inspeccionada.
  </p>

  <label class="field">
    <span class="field-label">Idioma de los subtítulos</span>
    <select data-nav bind:value={config.subsLang}>
      {#each SUB_LANGS as sl}
        <option value={sl.id}>{sl.label}</option>
      {/each}
    </select>
  </label>
  <label class="field">
    <span class="field-label">Tamaño de letra ({config.subSize}%)</span>
    <input data-nav
      type="range"
      min="50"
      max="200"
      step="10"
      bind:value={config.subSize}
    />
  </label>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.subsPorFuente} />
    <span>Elegir subtítulos según la fuente</span>
  </label>
  <p class="hint">
    Prefiere el subtítulo hecho para el archivo que estás viendo (✓) o
    para el mismo release (≈), que suelen venir ya sincronizados. Apagado,
    usa el más descargado.
  </p>

  <h3 class="subhead">Sincronía</h3>
  <label class="field">
    <span class="field-label">Recordar sincronía</span>
    <select data-nav bind:value={config.syncModo}>
      <option value="fuente">Misma fuente y subtítulo (recomendado)</option>
      <option value="titulo">Por título, cualquier fuente</option>
      <option value="no">No recordar</option>
    </select>
  </label>
  <p class="hint">
    Lo que ajustes en el reproductor vuelve la próxima vez. El desfase es
    de cada archivo con cada subtítulo: "por título" lo aplica también
    con otra fuente, donde suele no calzar.
  </p>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.syncCapAnterior} disabled={config.syncModo === "no"} />
    <span>Usar el ajuste del capítulo anterior (misma fuente)</span>
  </label>
  <p class="hint">
    En una serie del mismo grupo, el desfase suele repetirse entre
    capítulos. Se avisa en pantalla al aplicarlo.
  </p>
  <label class="field">
    <span class="field-label">Sincronía automática por audio</span>
    <select data-nav bind:value={config.syncAuto}>
      <option value="boton">Con el botón del reproductor (recomendado)</option>
      <option value="siempre">Siempre al cargar subtítulos externos</option>
      <option value="no">Desactivada</option>
    </select>
  </label>
  <p class="hint">
    Compara dónde se habla con dónde aparecen los subtítulos y corrige el
    desfase. Analiza unos 4 minutos alrededor de donde vas (a veces 8),
    así que baja esa parte del video otra vez: en 1080p son unos
    200–400 MB. Si no está seguro, no cambia nada. Solo subtítulos
    externos: los incluidos en el video ya vienen sincronizados. En el
    reproductor: Subtítulos → Sincronizar y mover….
  </p>
  <button data-nav type="button" class="btn-sec" onclick={borrarSincronias}>
    {syncBorradas ? "✓ Sincronías borradas" : "Borrar sincronías guardadas"}
  </button>
</section>

<section class="block">
  <h2>Selección de fuente</h2>
  <p class="hint">
    Al "Ver con debrid", cómo se elige qué fuente reproducir.
  </p>
  <div class="mode-group">
    <label class="mode-card" class:sel={config.sourceSelect === "auto"}>
      <input data-nav type="radio" name="sourceSelect" value="auto" bind:group={config.sourceSelect} />
      <div>
        <strong>Automática</strong>
        <span>Reproduce la mejor al instante, sin preguntar.</span>
      </div>
    </label>
    <label class="mode-card" class:sel={config.sourceSelect === "manual"}>
      <input data-nav type="radio" name="sourceSelect" value="manual" bind:group={config.sourceSelect} />
      <div>
        <strong>Manual</strong>
        <span>Siempre muestra el selector para elegir tú la fuente.</span>
      </div>
    </label>
  </div>

  <h3 class="subhead">Preferidas</h3>
  <p class="hint">
    Texto de interés en el nombre del torrent (release/grupo). Si el nombre
    o el proveedor lo contiene, esa fuente sube al tope (en modo automático,
    es la que se reproduce). Útil para grupos que suelen traer audio en
    español. Varias separadas por coma.
  </p>
  <label class="field">
    <span class="field-label">Películas</span>
    <input data-nav
      type="text"
      value={config.preferredSourceMovie}
      onchange={(e) => (config.preferredSourceMovie = texto(e))}
      placeholder="ej. FullScrab, YIFY"
      autocomplete="off"
      spellcheck="false"
    />
  </label>
  <label class="field">
    <span class="field-label">Series</span>
    <input data-nav
      type="text"
      value={config.preferredSourceSeries}
      onchange={(e) => (config.preferredSourceSeries = texto(e))}
      placeholder="ej. MeGusta, FLUX"
      autocomplete="off"
      spellcheck="false"
    />
  </label>
  <label class="field">
    <span class="field-label">Anime</span>
    <input data-nav
      type="text"
      value={config.preferredSourceAnime}
      onchange={(e) => (config.preferredSourceAnime = texto(e))}
      placeholder="ej. Erai-raws, SubsPlease"
      autocomplete="off"
      spellcheck="false"
    />
  </label>

  <h3 class="subhead">Lista negra</h3>
  <p class="hint">
    Texto que NO quieres. Si el nombre del torrent o el proveedor lo contiene,
    esa fuente se hunde al fondo del orden (nunca se auto-reproduce salvo que
    no haya otra). Varias separadas por coma.
  </p>
  <label class="field">
    <span class="field-label">Películas</span>
    <input data-nav
      type="text"
      value={config.blockedSourceMovie}
      onchange={(e) => (config.blockedSourceMovie = texto(e))}
      placeholder="ej. CAM, HDTS, TELESYNC"
      autocomplete="off"
      spellcheck="false"
    />
  </label>
  <label class="field">
    <span class="field-label">Series</span>
    <input data-nav
      type="text"
      value={config.blockedSourceSeries}
      onchange={(e) => (config.blockedSourceSeries = texto(e))}
      placeholder="ej. HDTV, x265"
      autocomplete="off"
      spellcheck="false"
    />
  </label>
  <label class="field">
    <span class="field-label">Anime</span>
    <input data-nav
      type="text"
      value={config.blockedSourceAnime}
      onchange={(e) => (config.blockedSourceAnime = texto(e))}
      placeholder="ej. HEVC, 480p"
      autocomplete="off"
      spellcheck="false"
    />
  </label>
</section>

<section class="block">
  <h2>Cacheo de red</h2>
  <p class="hint">
    Cuánto video junta el reproductor antes de seguir cuando la conexión
    se atraganta. El problema típico no es que el cache sea chico, sino
    que arranca de nuevo con muy poco y se vuelve a cortar enseguida.
  </p>
  <div class="mode-group">
    {#each CACHE_PRESETS as p}
      <label class="mode-card" class:sel={config.cachePreset === p.id}>
        <input data-nav
          type="radio"
          name="cachePreset"
          value={p.id}
          bind:group={config.cachePreset}
          onchange={() => (config.cacheWait = p.wait)}
        />
        <div>
          <strong>{p.label}</strong>
          <span>{p.hint} ({p.wait} s)</span>
        </div>
      </label>
    {/each}
  </div>
  <label class="field">
    <span class="field-label">O el valor exacto (segundos)</span>
    <input data-nav
      type="number"
      min={CACHE_WAIT_MIN}
      max={CACHE_WAIT_MAX}
      value={config.cacheWait}
      onchange={(e) => (config.cacheWait = numero(e, CACHE_WAIT_MIN, CACHE_WAIT_MAX, cachePresetActual().wait, false))}
    />
  </label>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.cacheAuto} />
    <span>Subirlo solo cuando la red se porta mal</span>
  </label>
  <p class="hint">
    Con esto activo, tras varios cortes seguidos el reproductor duplica
    la espera y avisa en pantalla. Si llega al tope y sigue cortándose
    —o si mide que la conexión no alcanza para el peso del archivo—
    ofrece bajarla y verla después, que es la única salida real ahí.
  </p>
  <label class="field">
    <span class="field-label">Tope de la subida automática (segundos)</span>
    <input data-nav
      type="number"
      min={CACHE_WAIT_MIN}
      max={CACHE_WAIT_MAX}
      value={config.cacheWaitMax}
      onchange={(e) => (config.cacheWaitMax = numero(e, config.cacheWait, CACHE_WAIT_MAX, CACHE_WAIT_MAX_DEFAULT, false))}
      disabled={!config.cacheAuto}
    />
  </label>
</section>

<section class="block">
  <h2>Verificación de disponibilidad</h2>
  <p class="hint">
    Verificaciones simultáneas al detectar si una película se puede
    reproducir. Más alto = los badges "no disponible" aparecen
    más rápido, pero puede saturar la red y cortar el video.
  </p>
  <div class="mode-group">
    {#each Array.from({ length: SCREENING_MAX - SCREENING_MIN + 1 }, (_, i) => i + SCREENING_MIN) as n}
      <label class="mode-card" class:sel={config.screeningConcurrency === n}>
        <input data-nav
          type="radio"
          name="screeningConc"
          value={n}
          bind:group={config.screeningConcurrency}
          onchange={() => void setConcurrenciaScreening(n)}
        />
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
