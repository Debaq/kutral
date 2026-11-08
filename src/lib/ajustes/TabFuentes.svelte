<script lang="ts">
  // Configuración → Fuentes: los buscadores que consulta "Descubrir".
  //
  // Cada uno con su interruptor y cómo le fue la última vez (salud que anota
  // buscadores.svelte.ts en cada búsqueda). Los sitios web en español además
  // muestran lo que dicen los humanos de Balandro: si está activo, su dominio
  // de hoy y su nota. Kütral ya usa ese dominio solo (dominios.rs).
  import Spinner from "$lib/ui/Spinner.svelte";
  import { onMount } from "svelte";
  import {
    catalogo,
    salud,
    encendido,
    encender,
    enPausa,
    reactivar,
    FALLAS_PARA_PAUSA,
    PAUSA_MS,
    type BuscadorInfo,
  } from "$lib/buscadores.svelte";
  import { guardar } from "./guardar.svelte";

  let lista = $state<BuscadorInfo[]>([]);
  let cargando = $state(true);
  let error = $state("");
  // Para que "hace 5 min" avance con la pestaña abierta.
  let ahora = $state(Date.now());

  onMount(() => {
    catalogo()
      .then((l) => (lista = l))
      .catch((e) => (error = String(e)))
      .finally(() => (cargando = false));
    const t = setInterval(() => (ahora = Date.now()), 30_000);
    return () => clearInterval(t);
  });

  const GRUPOS = [
    { titulo: "Películas y series", filtro: (b: BuscadorInfo) => !b.tipos.includes("anime") },
    { titulo: "Anime", filtro: (b: BuscadorInfo) => b.tipos.includes("anime") },
  ];

  const IDIOMA: Record<string, string> = {
    es: "Español",
    en: "Inglés",
    multi: "Varios idiomas",
  };
  const TIPO: Record<string, string> = { movie: "películas", series: "series", anime: "anime" };

  function hace(ms: number): string {
    const s = Math.max(0, Math.round((ahora - ms) / 1000));
    if (s < 60) return "recién";
    const m = Math.round(s / 60);
    if (m < 60) return `hace ${m} min`;
    const h = Math.round(m / 60);
    if (h < 48) return `hace ${h} h`;
    return `hace ${Math.round(h / 24)} días`;
  }

  function hora(ms: number): string {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  function dominio(host?: string): string {
    return (host || "").replace(/^https?:\/\//, "").replace(/\/$/, "");
  }

  /** Una línea con cómo le fue: lo más importante primero. */
  function estado(id: string): { txt: string; cls: string } {
    const r = salud[id];
    if (!encendido(id)) return { txt: "Apagado: no se consulta.", cls: "off" };
    if (!r || (!r.ultimoOk && !r.fallas)) return { txt: "Todavía sin usar.", cls: "" };
    if (enPausa(id, ahora)) {
      return {
        txt: `⏸ En pausa: falló ${r.fallas} veces seguidas${r.error ? ` (${r.error})` : ""}. Se vuelve a probar a las ${hora(r.pausaHasta)}.`,
        cls: "mal",
      };
    }
    if (r.fallas > 0) {
      return {
        txt: `⚠ Falló ${r.fallas === 1 ? "la última vez" : `${r.fallas} veces seguidas`}${r.error ? `: ${r.error}` : ""}. A las ${FALLAS_PARA_PAUSA} queda en pausa.`,
        cls: "warn",
      };
    }
    const seg = r.ms ? ` · ${(r.ms / 1000).toFixed(1)} s` : "";
    const n = r.ultimoN ? ` · trajo ${r.ultimoN} fuente${r.ultimoN === 1 ? "" : "s"}` : "";
    return { txt: `✓ Respondió ${hace(r.ultimoOk)}${n}${seg}`, cls: "bien" };
  }

  function cambiar(id: string, on: boolean) {
    encender(id, on);
    guardar();
  }

  function probarYa(id: string) {
    reactivar(id);
    ahora = Date.now();
  }
</script>

<section class="block">
  <h2>Fuentes</h2>
  <p class="hint">
    Los buscadores que consulta <em>Descubrir</em> para encontrar versiones de
    lo que quieres ver. Apaga los que no te sirvan. Si uno falla
    {FALLAS_PARA_PAUSA} veces seguidas queda en pausa solo y se vuelve a probar
    a las {PAUSA_MS / 3600_000} horas. Que no encuentre un título no cuenta
    como falla.
  </p>
  <p class="hint">
    Los sitios en español los vigilan a diario los humanos de
    <strong>Balandro</strong>: cuando uno se muda de dominio, Kütral usa el
    nuevo sin actualizarse.
  </p>
</section>

{#if cargando}
  <section class="block"><Spinner /></section>
{:else if error}
  <section class="block"><p class="warn-box">No se pudo leer la lista: {error}</p></section>
{:else}
  {#each GRUPOS as g (g.titulo)}
    {@const items = lista.filter(g.filtro)}
    {#if items.length}
      <section class="block">
        <h2>{g.titulo}</h2>
        <div class="fuentes">
          {#each items as b (b.id)}
            {@const est = estado(b.id)}
            {@const bal = b.balandro_info}
            <div class="fuente" class:apagada={!encendido(b.id)}>
              <label class="toggle-row">
                <input
                  data-nav
                  type="checkbox"
                  checked={encendido(b.id)}
                  onchange={(e) => cambiar(b.id, (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="fuente-nombre">{b.nombre}</span>
                <span class="fuente-chip">{IDIOMA[b.idioma] ?? b.idioma}</span>
                <span class="fuente-chip">{b.tipos.map((t) => TIPO[t] ?? t).join(" y ")}</span>
              </label>
              <p class="hint fuente-desc">{b.descripcion}</p>
              <p class="fuente-estado {est.cls}">
                {est.txt}
                {#if enPausa(b.id, ahora) && encendido(b.id)}
                  <button data-nav class="btn-sec fuente-btn" onclick={() => probarYa(b.id)}>Probar ya</button>
                {/if}
              </p>
              {#if bal}
                <p class="fuente-balandro">
                  Según Balandro:
                  {#if bal.activo}<span class="bien">activo</span>{:else}<span class="mal">inactivo</span>{/if}
                  {#if bal.host} · dominio <code>{dominio(bal.host)}</code>{/if}
                  {#if bal.nota}<br /><span class="fuente-nota">{bal.nota}</span>{/if}
                </p>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {/each}
{/if}

<style>
  .fuentes {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .fuente {
    border: 1px solid var(--borde, #2c2c35);
    border-radius: 10px;
    padding: 10px 12px;
    background: var(--superficie-1, #17171c);
  }
  .fuente.apagada {
    opacity: 0.6;
  }
  .fuente-nombre {
    font-weight: 600;
  }
  .fuente-chip {
    font-size: 11.5px;
    color: #b8b8c2;
    border: 1px solid #34343f;
    border-radius: 999px;
    padding: 1px 8px;
  }
  .fuente-desc {
    margin: 4px 0 6px 26px;
  }
  .fuente-estado,
  .fuente-balandro {
    margin: 0 0 0 26px;
    font-size: 12.5px;
    color: #a9a9b3;
    line-height: 1.5;
  }
  .fuente-balandro {
    margin-top: 4px;
  }
  .fuente-nota {
    color: #8c8c96;
  }
  .fuente-btn {
    margin-left: 8px;
    padding: 3px 10px;
    font-size: 12px;
  }
  .bien {
    color: var(--ok, #7fd88a);
  }
  .warn {
    color: #f0c674;
  }
  .mal {
    color: #ff9a9a;
  }
  .off {
    color: #8c8c96;
  }
  code {
    font-size: 12px;
    color: #d8d8e0;
  }
</style>
