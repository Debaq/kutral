<script lang="ts">
  import { onMount } from "svelte";
  import {
    config,
    TORRENT_BUFFER_DEFAULT,
    TORRENT_BUFFER_MIN,
    TORRENT_BUFFER_MAX,
    TORRENT_BUFFER_OPTIONS,
    TORRENT_QUALITY_OPTIONS,
    TORRENT_MAX_GB_DEFAULT,
    TORRENT_MAX_GB_MIN,
    TORRENT_MAX_GB_MAX,
    TORRENT_PARALELAS_DEFAULT,
    TORRENT_PARALELAS_MIN,
    TORRENT_PARALELAS_MAX,
  } from "$lib/config.svelte";
  import { torrentDefaultDir, torrentCheckDir } from "$lib/torrents.svelte";
  import { guardar, numero } from "./guardar.svelte";

  let torrentDirPath = $state("");
  // La carpeta no se ata a config: solo queda puesta si se puede escribir en ella.
  let torrentDirInput = $state(config.torrentDir);
  let dirEstado = $state<"idle" | "probando" | "ok" | "error">("idle");
  let dirMsg = $state("");

  onMount(async () => {
    torrentDirInput = config.torrentDir;
    // Solo informativo; no levanta la sesión torrent si no está iniciada.
    try { torrentDirPath = await torrentDefaultDir(); } catch {}
  });

  // Comprueba que la carpeta exista y se pueda escribir antes de dejarla puesta.
  async function probarCarpeta() {
    dirEstado = "probando";
    dirMsg = "";
    try {
      const real = await torrentCheckDir(torrentDirInput);
      dirEstado = "ok";
      dirMsg = real;
      config.torrentDir = torrentDirInput.trim();
      guardar();
    } catch (e) {
      dirEstado = "error";
      dirMsg = String(e);
    }
  }

  function carpetaDeSiempre() {
    torrentDirInput = "";
    dirEstado = "idle";
    dirMsg = "";
    config.torrentDir = "";
    guardar();
  }
</script>

<section class="block">
  <h2>Descarga local</h2>
  <p class="hint">
    Kütral baja el torrent y lo reproduce mientras se descarga, sin
    debrid de por medio.
    {#if config.rdLinked}
      Con tu debrid vinculado se usa solo cuando él rechaza una fuente
      por DMCA (error 451).
    {:else}
      <strong>Sin debrid vinculado es la única forma de reproducir.</strong>
    {/if}
  </p>
  <p class="warn-box">
    <strong>Tu IP queda visible</strong> para el resto de la gente que
    comparte ese archivo. El debrid hacía de intermediario; aquí no hay
    ninguno. Decide sabiendo eso.
  </p>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.torrentLocal} />
    <span>Bajar y reproducir en local</span>
  </label>
</section>

{#if config.torrentLocal}
  <section class="block">
    <h2>Calidad y peso</h2>
    <p class="hint">
      Bajándolo tú hay que sostener el bitrate del archivo o el video se
      corta: un 4K de 60 GB pide unos 67 Mbps constantes; un 1080p de
      4 GB, unos 4,5 Mbps. Estos topes valen <strong>solo</strong> para
      la descarga local.
    </p>
    <div class="mode-group">
      {#each TORRENT_QUALITY_OPTIONS as q}
        <label class="mode-card" class:sel={config.torrentMaxQuality === q.id}>
          <input data-nav
            type="radio"
            name="torrentMaxQuality"
            value={q.id}
            bind:group={config.torrentMaxQuality}
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
        value={config.torrentMaxGb}
        onchange={(e) => (config.torrentMaxGb = numero(e, TORRENT_MAX_GB_MIN, TORRENT_MAX_GB_MAX, TORRENT_MAX_GB_DEFAULT, false))}
      />
    </label>
    <label class="field">
      <span class="field-label">Descargas a la vez</span>
      <input data-nav
        type="number"
        min={TORRENT_PARALELAS_MIN}
        max={TORRENT_PARALELAS_MAX}
        value={config.torrentMaxParalelas}
        onchange={(e) => (config.torrentMaxParalelas = numero(e, TORRENT_PARALELAS_MIN, TORRENT_PARALELAS_MAX, TORRENT_PARALELAS_DEFAULT))}
      />
    </label>
    <p class="hint">
      Cuántas descargas de la cola corren juntas. Más no es más rápido:
      tu conexión es la misma y se reparte entre todas, así que lo único
      que cambia es que ninguna queda lista hasta el final.
    </p>
  </section>

  <section class="block">
    <h2>Antes de empezar a ver</h2>
    <p class="hint">Cuánto bajar antes de arrancar el video.</p>
    <div class="mode-group">
      {#each TORRENT_BUFFER_OPTIONS as b}
        <label class="mode-card" class:sel={config.torrentBufferMb === b.mb}>
          <input data-nav
            type="radio"
            name="torrentBufferMb"
            value={b.mb}
            bind:group={config.torrentBufferMb}
          />
          <div>
            <strong>{b.label}</strong>
            <span>{b.hint} ({b.mb} MB)</span>
          </div>
        </label>
      {/each}
    </div>
    <label class="field">
      <span class="field-label">O el valor exacto (MB)</span>
      <input data-nav
        type="number"
        min={TORRENT_BUFFER_MIN}
        max={TORRENT_BUFFER_MAX}
        value={config.torrentBufferMb}
        onchange={(e) => (config.torrentBufferMb = numero(e, TORRENT_BUFFER_MIN, TORRENT_BUFFER_MAX, TORRENT_BUFFER_DEFAULT))}
      />
    </label>
    <p class="hint">
      Más buffer = arranca más lento y se corta menos. Con internet
      lento o un torrent con poca gente, sube esto antes que bajar la
      calidad.
    </p>
  </section>

  <section class="block">
    <h2>Carpeta de descarga</h2>
    <label class="field">
      <span class="field-label">Ruta</span>
      <input data-nav
        type="text"
        placeholder={torrentDirPath}
        bind:value={torrentDirInput}
        onchange={probarCarpeta}
      />
    </label>
    <div class="dir-row">
      <button data-nav class="link-tiny" onclick={probarCarpeta}>Comprobar carpeta</button>
      {#if torrentDirInput.trim()}
        <button data-nav class="link-tiny" onclick={carpetaDeSiempre}>Usar la de siempre</button>
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
  </section>
{/if}
