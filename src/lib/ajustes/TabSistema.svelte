<script lang="ts">
  import Spinner from "$lib/ui/Spinner.svelte";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { getVersion } from "@tauri-apps/api/app";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { config, LANGS, cambiarIdioma } from "$lib/config.svelte";
  import { notify } from "$lib/notifStore.svelte";
  import { resetOnboarding } from "$lib/onboarding";

  // Updater
  let currentVer = $state("");

  // Pasar a kiosko quita los botones de la ventana al instante. Si no era lo
  // que se quería, vuelve solo al modo anterior en unos segundos (como al
  // cambiar la resolución de pantalla).
  const KIOSKO_SEG = 15;
  let kioskoPrevio = $state<typeof config.modeOverride | null>(null);
  let kioskoSeg = $state(0);
  let kioskoTimer: ReturnType<typeof setInterval> | null = null;
  let modoAntes = config.modeOverride;
  function alCambiarModo() {
    if (config.modeOverride === "kiosk" && modoAntes !== "kiosk") {
      kioskoPrevio = modoAntes;
      kioskoSeg = KIOSKO_SEG;
      if (kioskoTimer) clearInterval(kioskoTimer);
      kioskoTimer = setInterval(() => {
        kioskoSeg--;
        if (kioskoSeg <= 0) revertirKiosko();
      }, 1000);
    }
    modoAntes = config.modeOverride;
  }
  function mantenerKiosko() {
    if (kioskoTimer) clearInterval(kioskoTimer);
    kioskoTimer = null;
    kioskoPrevio = null;
  }
  function revertirKiosko() {
    if (kioskoTimer) clearInterval(kioskoTimer);
    kioskoTimer = null;
    if (kioskoPrevio) config.modeOverride = kioskoPrevio;
    modoAntes = config.modeOverride;
    kioskoPrevio = null;
  }
  $effect(() => () => {
    if (kioskoTimer) clearInterval(kioskoTimer);
  });
  type UpdStage = "idle" | "checking" | "uptodate" | "available" | "installing" | "ready" | "error";
  let updStage = $state<UpdStage>("idle");
  let updPending: Update | null = null;
  let updVersion = $state("");
  let updNotes = $state("");
  let updErr = $state("");
  let updDownloaded = $state(0);
  let updTotal = $state(0);

  onMount(async () => {
    try { currentVer = await getVersion(); } catch {}
  });

  // Vuelve a abrir el asistente inicial. Se limpia la marca para que, si se
  // cierra a mitad, la próxima vez tampoco se dé por hecho.
  function repetirAsistente() {
    resetOnboarding();
    goto("/bienvenida");
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

<section class="block">
  <h2>Idioma</h2>
  <p class="hint">
    De títulos, sinopsis y pósters, del audio que busca "Doblado" y del orden de
    las fuentes. Los menús siguen en español por ahora.
  </p>
  <div class="radio-group">
    {#each LANGS as l}
      <label class="radio">
        <input
          data-nav
          type="radio"
          name="lang"
          value={l.id}
          checked={config.lang === l.id}
          onchange={() => cambiarIdioma(l.id)}
        />
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
    <label class="mode-card" class:sel={config.modeOverride === "auto"}>
      <input data-nav type="radio" name="mode" value="auto" bind:group={config.modeOverride} onchange={alCambiarModo} />
      <div><strong>Auto</strong><span>Detecta y aplica el modo correcto.</span></div>
    </label>
    <label class="mode-card" class:sel={config.modeOverride === "desktop"}>
      <input data-nav type="radio" name="mode" value="desktop" bind:group={config.modeOverride} onchange={alCambiarModo} />
      <div><strong>Escritorio</strong><span>Ventana con minimizar / maximizar / cerrar.</span></div>
    </label>
    <label class="mode-card" class:sel={config.modeOverride === "kiosk"}>
      <input data-nav type="radio" name="mode" value="kiosk" bind:group={config.modeOverride} onchange={alCambiarModo} />
      <div><strong>Kiosko / embebido</strong><span>Pantalla completa. Solo "Salir" + gestor WiFi.</span></div>
    </label>
  </div>
  {#if kioskoPrevio}
    <div class="warn-box kiosko-confirma" role="alert">
      <strong>Modo kiosko activado.</strong> Si no lo confirmas, vuelve al modo anterior en {kioskoSeg} s.
      <div class="kiosko-btns">
        <button data-nav class="btn-sec" onclick={mantenerKiosko}>Mantener kiosko</button>
        <button data-nav class="btn-sec" onclick={revertirKiosko}>Volver al modo anterior</button>
      </div>
    </div>
  {/if}
  {#if config.modeOverride !== "kiosk"}
    <label class="toggle-row">
      <input data-nav type="checkbox" bind:checked={config.pantallaCompleta} />
      <span>Abrir en pantalla completa</span>
    </label>
    <p class="hint">
      Se recuerda entre sesiones. También cambia al usar el botón ⛶
      de la barra superior.
    </p>
  {/if}
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.sonidoInicio} />
    <span>Sonido del fuego al abrir Kütral</span>
  </label>
</section>

<section class="block">
  <h2>Actualizaciones</h2>
  <p class="hint">
    Versión actual: <em>{currentVer || "?"}</em>
  </p>

  {#if updStage === "idle"}
    <button data-nav class="btn-link" onclick={checkUpdate}>Buscar actualizaciones</button>
  {:else if updStage === "checking"}
    <p class="upd-line"><Spinner tam={12} grosor={2} /> Buscando…</p>
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
    <p class="upd-line"><Spinner tam={12} grosor={2} /> Descargando…</p>
    {#if updTotal > 0}
      <div class="bar"><div class="bar-fill" style="width: {updPct()}%"></div></div>
      <p class="upd-mb">{updMb(updDownloaded)} / {updMb(updTotal)} MB · {updPct()}%</p>
    {:else}
      <p class="upd-mb">{updMb(updDownloaded)} MB</p>
    {/if}
  {:else if updStage === "ready"}
    <p class="upd-line"><Spinner tam={12} grosor={2} /> Reiniciando…</p>
  {:else if updStage === "error"}
    <p class="err">{updErr}</p>
    <button data-nav class="link-tiny" onclick={checkUpdate}>Reintentar</button>
  {/if}
</section>

<section class="block">
  <h2>Asistente inicial</h2>
  <p class="hint">
    Vuelve a pasar por los pasos del primer inicio.
  </p>
  <button data-nav class="btn-sec" onclick={repetirAsistente}>Repetir asistente</button>
</section>
