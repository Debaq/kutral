<script lang="ts" module>
  type Tab = "cuenta" | "reproduccion" | "descargas" | "transmitir" | "sistema";
  // Al volver a /config en la misma sesión se abre la última pestaña usada.
  let ultima: Tab = "cuenta";
</script>

<script lang="ts">
  import "$lib/ajustes/ajustes.css";
  import { onMount, onDestroy, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { config, loadConfig } from "$lib/config.svelte";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { navegar } from "$lib/nav";
  import RemoteQr from "$lib/RemoteQr.svelte";
  import type { WebStatus } from "$lib/tipos";
  import { guardado, guardar } from "$lib/ajustes/guardar.svelte";
  import TabCuenta from "$lib/ajustes/TabCuenta.svelte";
  import TabReproduccion from "$lib/ajustes/TabReproduccion.svelte";
  import TabDescargas from "$lib/ajustes/TabDescargas.svelte";
  import TabTransmitir from "$lib/ajustes/TabTransmitir.svelte";
  import TabSistema from "$lib/ajustes/TabSistema.svelte";

  const TABS: { id: Tab; icono: string; label: string }[] = [
    { id: "cuenta", icono: "🔑", label: "Cuenta y servicios" },
    { id: "reproduccion", icono: "🎬", label: "Reproducción" },
    { id: "descargas", icono: "⬇", label: "Descargas" },
    { id: "transmitir", icono: "📺", label: "Transmitir y TV" },
    { id: "sistema", icono: "⚙", label: "Sistema" },
  ];

  let tab = $state<Tab>(ultima);
  let raiz: HTMLDivElement;
  let barra: HTMLElement;
  let tabCuenta: TabCuenta;

  function elegir(t: Tab) {
    if (t === tab) return;
    tab = t;
    ultima = t;
    raiz?.scrollTo({ top: 0 });
  }

  function botonActivo(): HTMLElement | null {
    return barra?.querySelector<HTMLElement>(`[data-tab="${tab}"]`) ?? null;
  }

  // Con el control, moverse por la barra va cambiando de pestaña. Pero al
  // entrar a la barra desde el contenido (←) el foco cae en el botón más
  // cercano en altura, que casi nunca es el de la pestaña abierta: se lo
  // devolvemos a esa, si no ← cambiaría de pestaña sin querer.
  function focoEnBarra(e: FocusEvent) {
    const boton = (e.target as HTMLElement).closest<HTMLElement>("[data-tab]");
    if (!boton) return;
    const desde = e.relatedTarget as Node | null;
    if (!desde || !barra.contains(desde)) {
      if (boton.dataset.tab !== tab) botonActivo()?.focus();
      return;
    }
    elegir(boton.dataset.tab as Tab);
  }

  // --- Teclado por celular: QR a /api para escribir/pegar/escanear claves ---
  let phoneAbierto = $state(false);
  let phoneUrl = $state("");
  let phoneBusy = $state(false);
  let phoneErr = $state("");
  async function conectarCelular() {
    phoneAbierto = !phoneAbierto;
    if (!phoneAbierto || phoneUrl) return;
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

  onMount(async () => {
    ayuda.set("config", [
      { tecla: "← → ↑ ↓", desc: "Moverse entre pestañas y ajustes" },
      { tecla: "Enter · Espacio", desc: "Activar / marcar" },
      { tecla: "Esc · Backspace", desc: "Volver a inicio" },
      { tecla: "I-I", desc: "Ayuda" },
    ]);
    loadConfig();
    // Sin foco inicial la pantalla arrancaba muerta para mando: las flechas no
    // tenían desde dónde salir y no había forma de llegar a ningún control.
    await tick();
    setTimeout(() => botonActivo()?.focus(), 60);
  });

  // Un campo de texto se guarda al soltarlo (change); si se sale con Esc
  // estando adentro, el blur lo dispara antes de desmontar.
  onDestroy(() => {
    (document.activeElement as HTMLElement | null)?.blur?.();
  });

  function back() {
    (document.activeElement as HTMLElement | null)?.blur?.();
    goto("/");
  }

  // Tipos de <input> donde las flechas horizontales son del control (mueven el
  // cursor o el valor) y no de la navegación.
  const TEXTO = ["text", "password", "search", "url", "email", "tel", "number"];

  // Esc/Backspace en /config: si hay una vinculación RD en curso, Esc la
  // cancela; si no, ambas vuelven al home. Backspace en inputs queda intocable
  // (borra texto).
  function onGlobalKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    const tag = t?.tagName;
    const tipo = (t as HTMLInputElement | null)?.type ?? "";
    const inText =
      tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" ||
      (t?.isContentEditable ?? false);

    if (e.key === "Escape" || e.key === "Backspace") {
      if (e.key === "Escape" && tabCuenta?.cancelarFlujo()) {
        e.preventDefault();
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
    // Espacio, y el OK del control web manda Enter.
    if (e.key === "Enter" && tag === "INPUT" && (tipo === "radio" || tipo === "checkbox")) {
      e.preventDefault();
      (t as HTMLInputElement).click();
    }
  }
</script>

<svelte:head>
  <title>Configuración · Kütral</title>
</svelte:head>

<svelte:window onkeydown={onGlobalKey} />

<div class="cfg-root" bind:this={raiz}>
  <div class="cfg-wrap">
    <header class="cfg-head">
      <button data-nav class="back" onclick={back} title="Volver">← Volver</button>
      <h1>Configuración</h1>
      <span class="espacio"></span>
      <span class="guardado" class:visible={guardado.visible} aria-live="polite">✓ Guardado</span>
      <button data-nav class="back" onclick={conectarCelular} title="Escribir claves y direcciones largas desde el celular">
        📱 Escribir desde el celular
      </button>
    </header>

    {#if phoneAbierto}
      <section class="block phone-block">
        <h2>Escribir desde el celular</h2>
        {#if phoneUrl}
          <div class="phone-qr">
            <RemoteQr url={phoneUrl} label="Teclado" size={132} />
            <div class="phone-steps">
              <p class="hint" style="margin:0 0 8px;">
                1. Escanea con la cámara del celular (misma WiFi).<br />
                2. Toca aquí el campo a llenar (TMDb, OMDb, una lista…).<br />
                3. Escribe, pega o <strong>escanea otro QR</strong> en el celular y pulsa <em>Enviar</em>.
              </p>
              <code class="phone-url">{phoneUrl}</code>
            </div>
          </div>
        {:else if phoneBusy}
          <p class="hint"><span class="spinner"></span> Conectando…</p>
        {/if}
        {#if phoneErr}<p class="err">{phoneErr}</p>{/if}
      </section>
    {/if}

    <div class="cfg-body">
      <nav class="cfg-tabs" data-section="tabs" bind:this={barra} onfocusin={focoEnBarra}>
        {#each TABS as t (t.id)}
          <button
            data-nav
            data-tab={t.id}
            class="cfg-tab"
            class:activa={tab === t.id}
            aria-current={tab === t.id ? "page" : undefined}
            onclick={() => elegir(t.id)}
          >
            <span class="cfg-tab-icono">{t.icono}</span>
            {t.label}
          </button>
        {/each}
      </nav>

      <!-- Todas montadas y ocultas con hidden: cambiar de pestaña no corta una
           vinculación RD, una búsqueda de TVs ni una actualización en curso.
           Cualquier change adentro guarda (ver guardar.svelte.ts). -->
      <div class="cfg-paneles" onchange={guardar}>
        <div class="cfg-panel" hidden={tab !== "cuenta"}><TabCuenta bind:this={tabCuenta} /></div>
        <div class="cfg-panel" hidden={tab !== "reproduccion"}><TabReproduccion /></div>
        <div class="cfg-panel" hidden={tab !== "descargas"}><TabDescargas /></div>
        <div class="cfg-panel" hidden={tab !== "transmitir"}><TabTransmitir /></div>
        <div class="cfg-panel" hidden={tab !== "sistema"}><TabSistema /></div>
      </div>
    </div>
  </div>
</div>
