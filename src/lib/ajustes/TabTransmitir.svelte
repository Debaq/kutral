<script lang="ts">
  import { onMount } from "svelte";
  import {
    cast,
    buscarTvs,
    agregarTvPorIp,
    elegirTv,
    olvidarTv,
    leccionesDe,
    olvidarAprendido,
    protocolo,
    redInfo,
    type RedInfo,
  } from "$lib/cast.svelte";
  import { config, IPTV_DEFAULT_LISTS } from "$lib/config.svelte";
  import { guardar, numero } from "./guardar.svelte";

  // Puerto y firewall, para avisar si la TV no va a poder entrar a este equipo.
  let redTv = $state<RedInfo | null>(null);
  // IP escrita a mano para una TV que la búsqueda no encuentra.
  let tvIp = $state("");

  onMount(() => {
    void redInfo().then((r) => (redTv = r));
  });

  let nuevaListaNombre = $state("");
  let nuevaListaUrl = $state("");

  function agregarLista() {
    const url = nuevaListaUrl.trim();
    if (!url) return;
    const name = nuevaListaNombre.trim() || `Lista ${config.iptvLists.length + 1}`;
    config.iptvLists = [...config.iptvLists, { name, url }];
    nuevaListaNombre = "";
    nuevaListaUrl = "";
    guardar();
  }
  function quitarLista(i: number) {
    config.iptvLists = config.iptvLists.filter((_, idx) => idx !== i);
    guardar();
  }
  function restaurarListaDefault() {
    config.iptvLists = IPTV_DEFAULT_LISTS.map((l) => ({ ...l }));
    guardar();
  }
</script>

<section class="block">
  <h2>Transmitir a la TV</h2>
  <p class="hint">
    Manda la película a una TV de tu casa: Google TV, Chromecast y
    casi todas las LG por Google Cast; Samsung y otras por DLNA.
    Kütral recuerda tu TV y aprende qué formatos reproduce.
  </p>
  {#if cast.tv}
    <div class="cast-tv">
      <span class="cast-nombre">
        📺 <strong>{cast.tv.nombre}</strong>
        <span class="cast-proto">{protocolo(cast.tv)} · {cast.tv.ip}</span>
      </span>
      <button data-nav class="btn-sec" onclick={olvidarTv}>Olvidar</button>
    </div>
    <p class="hint">
      Todo empieza en este equipo, donde eliges audio y subtítulos. Para
      verlo en la TV, «Pasar a la TV» en el reproductor la manda en el
      mismo minuto, con lo que tengas puesto. En la lista de fuentes,
      <kbd>T</kbd> la manda directo.
    </p>
  {/if}
  <button data-nav class="btn-sec" onclick={() => void buscarTvs()} disabled={cast.buscando}>
    {cast.buscando ? "Buscando…" : "🔍 Buscar TVs en la red"}
  </button>
  {#if cast.busquedaLarga}
    <p class="hint">
      La búsqueda rápida no encontró nada: probando los equipos de la red uno
      por uno (hay routers que no dejan pasar la búsqueda normal). Tarda unos
      segundos.
    </p>
  {/if}
  <div class="cast-ip">
    <input
      data-nav
      type="text"
      inputmode="decimal"
      placeholder="IP de la TV, ej. 192.168.1.5"
      bind:value={tvIp}
      onkeydown={(e) => {
        if (e.key === "Enter" && tvIp.trim()) void agregarTvPorIp(tvIp).catch(() => {});
      }}
    />
    <button
      data-nav
      class="btn-sec"
      disabled={cast.buscando || !tvIp.trim()}
      onclick={() => void agregarTvPorIp(tvIp).catch(() => {})}
    >
      ➕ Agregar por IP
    </button>
  </div>
  {#if cast.errorBusqueda}<p class="err">{cast.errorBusqueda}</p>{/if}
  {#if cast.encontradas.length}
    <div class="cast-lista">
      {#each cast.encontradas as tv (tv.id)}
        <button
          data-nav
          class="cast-opcion"
          class:elegida={cast.tv?.id === tv.id}
          onclick={() => elegirTv(tv)}
        >
          <span>{cast.tv?.id === tv.id ? "✓ " : ""}{tv.nombre}</span>
          <span class="cast-proto">
            {protocolo(tv)}{tv.modelo ? ` · ${tv.modelo}` : ""} · {tv.ip}
          </span>
        </button>
      {/each}
    </div>
  {/if}
  {#if cast.tv}
    {@const lecciones = leccionesDe(cast.tv.id)}
    <h3 class="subhead">Lo que aprendió de esta TV</h3>
    {#if lecciones.length}
      <ul class="cast-lecciones">
        {#each lecciones as l (l.clave)}
          <li class:no={!l.ok}>{l.ok ? "✅" : "❌"} {l.rasgo}</li>
        {/each}
      </ul>
      <button data-nav class="btn-sec" onclick={() => cast.tv && olvidarAprendido(cast.tv.id)}>
        Olvidar lo aprendido
      </button>
    {:else}
      <p class="hint">
        Todavía nada: cada transmisión le enseña algo. Si una película
        se ve pero no se oye, usa "🔇 No se oye" y Kütral buscará otra
        versión.
      </p>
    {/if}
  {/if}
  {#if redTv?.firewall}
    <p class="warn-box cast-fw">
      <strong>Firewall activo ({redTv.firewall}).</strong> La TV necesita
      entrar a este equipo por el puerto {redTv.puerto} para los
      subtítulos, las TVs DLNA (Samsung) y las copias bajadas. Si eso no
      funciona, ábrelo una vez con{redTv.firewall === "Windows Defender"
        ? " (en una terminal abierta como administrador)"
        : ""}:
      <code>{redTv.comando}</code>
    </p>
  {/if}
</section>

<section class="block">
  <h2>Mando remoto (servidor web)</h2>
  <p class="hint">
    Levanta un servidor HTTP en la red local para usar el celular como
    mando. Aplica al próximo inicio de Kütral.
  </p>
  <label class="toggle-row">
    <input data-nav type="checkbox" bind:checked={config.webAutoStart} />
    <span>Activar al iniciar la app</span>
  </label>
  <label class="field">
    <span class="field-label">Puerto</span>
    <input data-nav
      type="number"
      min="1024"
      max="65535"
      value={config.webPort}
      onchange={(e) => (config.webPort = numero(e, 1024, 65535, 8080))}
    />
  </label>
</section>

<section class="block">
  <h2>TV en vivo — Listas de canales</h2>
  <p class="hint">
    Playlists M3U que alimentan los canales en vivo. Puedes tener varias
    (iptv-org, tu proveedor, listas propias). Se cargan todas juntas en
    la sección TV.
  </p>

  {#if config.iptvLists.length}
    <ul class="iptv-listas">
      {#each config.iptvLists as lista, i}
        <li class="iptv-item">
          <input data-nav
            class="iptv-nombre"
            type="text"
            value={lista.name}
            onchange={(e) => (lista.name = e.currentTarget.value.trim() || "Lista")}
            placeholder="Nombre"
            spellcheck="false"
          />
          <input data-nav
            class="iptv-url"
            type="text"
            value={lista.url}
            onchange={(e) => (lista.url = e.currentTarget.value.trim())}
            placeholder="https://…/lista.m3u"
            spellcheck="false"
          />
          <button data-nav class="iptv-del" title="Quitar" onclick={() => quitarLista(i)}>✕</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="hint">No hay listas. Agrega una abajo.</p>
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

  <button data-nav class="link-tiny" onclick={restaurarListaDefault}>
    Restaurar listas por defecto (Español + global)
  </button>
</section>
