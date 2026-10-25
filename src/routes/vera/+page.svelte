<script lang="ts">
  // Vera elige qué ver. Una pantalla, dos toques: con quién y qué te tinca.
  // Devuelve tres cartas (la segura, una distinta y la sorpresa, ver tres.ts)
  // sacadas del motor (motor.ts) sobre un pool armado en vivo (tmdb.ts).
  //
  // Sin cuestionario: Vera aprende de lo que el perfil ya hace en Kütral
  // (historial.ts: vistas, a medias, favoritos) y de un 👍/👎 opcional, al
  // marcar "Ya la vi" o la próxima vez que se abre después de ver una que
  // eligió (elegidas.ts). Lo único configurable es lo que nunca quieres ver.
  //
  // Teclado: flechas mueven (navegación espacial), Enter elige, Esc vuelve un
  // paso, Backspace sale a la home. Todo se puede con el mando.
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { goto } from "$app/navigation";
  import { navegar, enfocarPrimero } from "$lib/nav";
  import { ayuda } from "$lib/atajos/store.svelte";
  import { perfilInfantil } from "$lib/perfiles.svelte";
  import type {
    Contexto,
    EstadoVera,
    Intencion,
    Pelicula,
    PerfilSetup,
    RankingPeli,
  } from "$lib/vera/tipos";
  import { INTENCIONES, filtrosParaIntent } from "$lib/vera/intenciones";
  import { recomendar, hayMatch } from "$lib/vera/motor";
  import {
    cargarCatalogoPorIntent,
    enriquecer,
    motivoExclusion,
    asegurarGeneros,
    posterUrl,
    backdropUrl,
    TmdbNoKeyError,
  } from "$lib/vera/tmdb";
  import { getPerfilHistorico, getVistas, idsConSenal, titulosDe } from "$lib/vera/historial";
  import { getReaccionesMap } from "$lib/vera/ratings";
  import {
    getSetup,
    guardarSetup,
    listarGeneros,
    listarTemas,
    type OpcionCatalogo,
  } from "$lib/vera/setup";
  import { RANURAS, candidataPara, type Ranura } from "$lib/vera/tres";
  import { anotarElegida, porOpinar, opinar, noPreguntar } from "$lib/vera/elegidas";

  type Pantalla = "preguntas" | "pensando" | "cartas" | "nunca";
  type Carta = { r: RankingPeli; ranura: Ranura; preguntando: boolean; cargando: boolean };

  const CONTEXTOS: { id: Contexto; label: string; icono: string }[] = [
    { id: "solo", label: "Solo", icono: "🙂" },
    { id: "pareja", label: "En pareja", icono: "💑" },
    { id: "amigos", label: "Con amigos", icono: "🍿" },
    // Regla de marca: "con niños" siempre implica un adulto presente.
    { id: "ninos", label: "Con niños", icono: "🧸" },
  ];

  const RANURA_LABEL: Record<Ranura, string> = {
    segura: "La segura",
    distinta: "Una distinta",
    sorpresa: "La sorpresa",
  };

  // "Algo corto": hasta esto, en minutos. Lo más largo no se propone.
  const MAX_CORTA = 110;
  // Intentos por carta hasta dar con una que sirva (reproducible, sin lo que
  // nunca quieres ver, corta si se pidió).
  const INTENTOS_CARTA = 8;
  // Las primeras del ranking se completan juntas antes de mostrar nada: las
  // cartas salen de ahí y "Otra" casi siempre encuentra la siguiente lista.
  const PRECARGA = 9;

  const SETUP_VACIO: PerfilSetup = {
    plataformas: [],
    generosExcluidos: [],
    temasExcluidos: [],
    personalidad: "calida",
    duracionMax: null,
    animacion: "indiferente",
    idiomasComodos: [],
    idiomasEvitados: [],
  };

  let pantalla = $state<Pantalla>("preguntas");
  const ninosFijo = $derived(perfilInfantil());
  let contexto = $state<Contexto | null>(null);
  let intencion = $state<Intencion | null>(null);
  let corta = $state(false);
  let setup = $state<PerfilSetup | null>(null);
  let error = $state("");
  let aviso = $state("");
  let cartas = $state<(Carta | null)[]>([]);
  let opinable = $state<Pelicula | null>(null);
  let titulosSemilla = $state(new Map<string, string>());
  let raiz = $state<HTMLElement | null>(null);

  // Lo que dura una búsqueda: el ranking y lo que ya no se ofrece (vistas,
  // mostradas, descartadas). No es $state: no se pinta.
  let ranking: RankingPeli[] = [];
  let usadas = new Set<string>();
  // Mostradas en esta visita a Vera: "Otras tres" no las repite.
  const mostradas = new Set<string>();
  // La búsqueda vigente: Esc a mitad de "Pensando…" la descarta.
  let gen = 0;

  const ctxActual = $derived<Contexto>(ninosFijo ? "ninos" : (contexto ?? "solo"));

  // --- Lo que nunca quieres ver ---------------------------------------------
  let opsGeneros = $state<OpcionCatalogo[]>([]);
  let opsTemas = $state<OpcionCatalogo[]>([]);
  let nuncaGeneros = $state(new Set<string>());
  let nuncaTemas = $state(new Set<string>());
  let guardandoNunca = $state(false);

  async function abrirNunca() {
    if (!opsGeneros.length) [opsGeneros, opsTemas] = await Promise.all([listarGeneros(), listarTemas()]);
    nuncaGeneros = new Set(setup?.generosExcluidos ?? []);
    nuncaTemas = new Set(setup?.temasExcluidos ?? []);
    pantalla = "nunca";
    void enfocar(".nunca");
  }

  function alternar(conjunto: Set<string>, id: string): Set<string> {
    const s = new Set(conjunto);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    return s;
  }

  async function guardarNunca() {
    guardandoNunca = true;
    const nuevo: PerfilSetup = {
      ...(setup ?? SETUP_VACIO),
      generosExcluidos: [...nuncaGeneros],
      temasExcluidos: [...nuncaTemas],
    };
    if (await guardarSetup(nuevo)) setup = nuevo;
    guardandoNunca = false;
    pantalla = "preguntas";
    void enfocar(".intenciones");
  }

  // --- Preguntas -------------------------------------------------------------

  async function enfocar(sel: string) {
    await tick();
    setTimeout(() => raiz && enfocarPrimero(raiz.querySelector(sel) ?? raiz), 30);
  }

  function elegirContexto(c: Contexto) {
    contexto = c;
    void enfocar(".intenciones");
  }

  function elegirIntencion(i: Intencion) {
    intencion = i;
    void buscar();
  }

  // El tope de "algo corto" va como tope de duración de la sesión.
  function setupDeSesion(): PerfilSetup | null {
    if (!corta) return setup;
    return { ...(setup ?? SETUP_VACIO), duracionMax: MAX_CORTA };
  }

  // --- Búsqueda --------------------------------------------------------------

  async function buscar() {
    if (!intencion) return;
    const g = ++gen;
    pantalla = "pensando";
    error = "";
    aviso = "";
    try {
      const filtros = { ...filtrosParaIntent(intencion), ninos: ctxActual === "ninos" };
      // Lo visto desde la home no pasó por Vera: sus géneros se piden una vez.
      await asegurarGeneros(await idsConSenal()).catch(() => {});
      const [pool, historico, vistas] = await Promise.all([
        cargarCatalogoPorIntent(filtros, setup),
        getPerfilHistorico(),
        getVistas(),
      ]);
      if (g !== gen) return;
      const reacciones = getReaccionesMap();
      const juicios = new Map<string, number | null>();
      for (const id of vistas) juicios.set(id, reacciones[id]?.juicio ?? 0);
      const estado: EstadoVera = {
        contexto: ctxActual,
        intencion,
        reacciones: [],
        horaActual: new Date().getHours(),
        setup: setupDeSesion(),
      };
      ranking = await recomendar(pool, estado, historico, juicios);
      if (g !== gen) return;
      // Lo que ya viste no se propone: rever es un plan que uno busca solo.
      usadas = new Set([...vistas, ...mostradas]);
      if (ranking.length && !hayMatch(ranking)) {
        aviso = "No encontré nada que te calce del todo. Estas son lo más cercano.";
      }
      // Las primeras se completan juntas (director, duración, si se puede ver).
      const libres = ranking.filter((r) => !usadas.has(r.pelicula.id)).slice(0, PRECARGA);
      const ricas = await enriquecer(libres.map((r) => r.pelicula));
      if (g !== gen) return;
      ranking = ranking.map((r) => {
        const p = ricas.get(r.pelicula.id);
        return p ? { ...r, pelicula: p } : r;
      });
      cartas = [null, null, null];
      for (let i = 0; i < RANURAS.length; i++) {
        cartas[i] = await llenar(i, RANURAS[i]);
        if (g !== gen) return;
      }
      if (!cartas.some(Boolean)) {
        error = "No encontré nada que se pueda ver con eso. Prueba otra cosa.";
        pantalla = "preguntas";
        return;
      }
      void cargarSemillas();
      pantalla = "cartas";
      void enfocar(".cartas");
    } catch (e) {
      if (g !== gen) return;
      error =
        e instanceof TmdbNoKeyError
          ? "Vera necesita la key de TMDb (Configuración → Cuenta)."
          : `No pude buscar: ${e instanceof Error ? e.message : String(e)}`;
      pantalla = "preguntas";
    }
  }

  // ¿Sirve para proponerla? Que se pueda ver (sin imdb no la encuentra ningún
  // buscador de fuentes), que no choque con lo que nunca quieres ver (los
  // temas se saben recién con el detalle) y, si pediste algo corto, que lo sea.
  function sirve(p: Pelicula): boolean {
    if (!p.imdbId) return false;
    if (motivoExclusion(p, setup) !== null) return false;
    if (corta && p.runtime != null && p.runtime > MAX_CORTA) return false;
    return true;
  }

  async function llenar(i: number, ranura: Ranura): Promise<Carta | null> {
    for (let intento = 0; intento < INTENTOS_CARTA; intento++) {
      const otras = cartas
        .filter((c, j): c is Carta => j !== i && c !== null)
        .map((c) => c.r.pelicula);
      const c = candidataPara(ranura, ranking, usadas, otras);
      if (!c) return null;
      usadas.add(c.pelicula.id);
      mostradas.add(c.pelicula.id);
      const rica = c.pelicula.enriquecida
        ? c.pelicula
        : ((await enriquecer([c.pelicula])).get(c.pelicula.id) ?? c.pelicula);
      if (!sirve(rica)) continue;
      return { r: { ...c, pelicula: rica }, ranura, preguntando: false, cargando: false };
    }
    return null;
  }

  async function cargarSemillas() {
    const ids = cartas.map((c) => c?.r.pelicula.semilla).filter((x): x is string => !!x);
    if (ids.length) titulosSemilla = await titulosDe(ids);
  }

  // --- Cartas ----------------------------------------------------------------

  function descubrir(c: Carta) {
    anotarElegida(c.r.pelicula);
    goto(`/?play=${c.r.pelicula.id}&type=movie`);
  }

  async function otra(i: number) {
    const c = cartas[i];
    if (!c || c.cargando) return;
    cartas[i] = { ...c, cargando: true, preguntando: false };
    const nueva = await llenar(i, c.ranura);
    if (nueva) {
      cartas[i] = nueva;
      void cargarSemillas();
    } else {
      cartas[i] = { ...c, cargando: false };
      aviso = "No quedan más de esas con lo que pediste. Prueba «Otras tres» o cambia lo que pediste.";
    }
    void enfocar(`.carta[data-i="${i}"]`);
  }

  function yaLaVi(i: number) {
    const c = cartas[i];
    if (!c) return;
    cartas[i] = { ...c, preguntando: true };
    void enfocar(`.carta[data-i="${i}"] .opinion`);
  }

  async function responder(i: number, gusto: boolean | null) {
    const c = cartas[i];
    if (!c) return;
    await opinar(c.r.pelicula, gusto);
    await otra(i);
  }

  async function responderPendiente(gusto: boolean | null | "omitir") {
    const p = opinable;
    if (!p) return;
    opinable = null;
    if (gusto === "omitir") noPreguntar(p);
    else await opinar(p, gusto);
    void enfocar(ninosFijo ? ".intenciones" : ".contextos");
  }

  function frase(c: Carta): string {
    const p = c.r.pelicula;
    const semilla = p.semilla ? titulosSemilla.get(p.semilla) : undefined;
    const razones = c.r.porQue.razones.filter((r) => r !== "liviano" && r !== "denso");
    const ninos = ctxActual === "ninos" ? " Para ver en familia." : "";
    if (c.ranura === "sorpresa") {
      return (p.votos < 3000 ? "Menos conocida, y muy bien valorada." : "Una que no habrías buscado.") + ninos;
    }
    if (semilla) return `Porque viste ${semilla}.${ninos}`;
    if (razones.length) return `Porque te gusta ${lista(razones)}.${ninos}`;
    if (c.ranura === "distinta" && p.generos[0]) return `Para cambiar de aire: ${p.generos[0].toLowerCase()}.${ninos}`;
    return (c.r.porQue.calidad > 0.75 ? "De lo mejor valorado de lo que pediste." : "Calza con lo que pediste.") + ninos;
  }

  function lista(xs: string[]): string {
    const v = xs.map((x) => (/^[A-ZÁÉÍÓÚÑ][a-záéíóúñ]+$/.test(x) ? x.toLowerCase() : x));
    return v.length === 1 ? v[0] : `${v.slice(0, -1).join(", ")} y ${v[v.length - 1]}`;
  }

  function datos(p: Pelicula): string {
    const partes = [p.anio, p.runtime ? `${Math.floor(p.runtime / 60)} h ${p.runtime % 60} min` : "", p.generos.slice(0, 2).join(" · ")];
    return partes.filter(Boolean).join(" · ");
  }

  // --- Teclado ---------------------------------------------------------------

  function atras() {
    if (pantalla === "nunca") {
      pantalla = "preguntas";
      void enfocar(".intenciones");
    } else if (pantalla === "cartas" || pantalla === "pensando") {
      gen++;
      pantalla = "preguntas";
      void enfocar(".intenciones");
    } else {
      goto("/");
    }
  }

  function onKey(e: KeyboardEvent) {
    if (ayuda.visible) return;
    if (e.key === "Escape") {
      e.preventDefault();
      atras();
      return;
    }
    if (e.key === "Backspace") {
      e.preventDefault();
      goto("/");
      return;
    }
    if (e.key.startsWith("Arrow") && raiz) {
      e.preventDefault();
      navegar(e.key.slice(5).toLowerCase() as "up" | "down" | "left" | "right", raiz);
    }
  }

  $effect(() => {
    ayuda.set("vera", [
      { tecla: "← → ↑ ↓", desc: "Moverse" },
      { tecla: "Enter", desc: "Elegir" },
      { tecla: "Esc", desc: "Volver un paso" },
      { tecla: "Backspace", desc: "Salir a la home" },
      { tecla: "I-I", desc: "Ayuda" },
    ]);
  });

  onMount(async () => {
    setup = await getSetup();
    opinable = await porOpinar();
    void enfocar(opinable ? ".opinable" : ninosFijo ? ".intenciones" : ".contextos");
  });

  const fondo = $derived.by(() => {
    const c = cartas.find((x) => x && x.ranura === "segura") ?? cartas.find(Boolean);
    return c ? backdropUrl(c.r.pelicula) : "";
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="vera" bind:this={raiz}>
  {#if pantalla === "cartas" && fondo}
    <div class="fondo" style:background-image="url('{fondo}')" transition:fade={{ duration: 400 }}></div>
  {/if}

  <header class="cabeza">
    <h1 class="marca"><span class="marca-vera">Vera</span> <em>and Chill</em></h1>
  </header>

  {#if pantalla === "preguntas"}
    <section class="pantalla" in:fade={{ duration: 250 }}>
      {#if opinable}
        <div class="opinable zona" data-section="opinable">
          <span>¿Qué tal <strong>{opinable.titulo}</strong>?</span>
          <button data-nav class="chip" onclick={() => void responderPendiente(true)}>👍 Me gustó</button>
          <button data-nav class="chip" onclick={() => void responderPendiente(false)}>👎 No me gustó</button>
          <button data-nav class="chip tenue" onclick={() => void responderPendiente("omitir")}>Ahora no</button>
        </div>
      {/if}

      <h2>¿Con quién vas a ver?</h2>
      {#if ninosFijo}
        <p class="nota">🧸 Perfil de niños: todo lo que propone Vera es apto para ellos.</p>
      {:else}
        <div class="fila contextos zona" data-section="contextos">
          {#each CONTEXTOS as c (c.id)}
            <button
              data-nav
              class="opcion"
              class:sel={contexto === c.id}
              onclick={() => elegirContexto(c.id)}
            >
              <span class="icono">{c.icono}</span>
              <span>{c.label}</span>
            </button>
          {/each}
        </div>
      {/if}

      <h2>¿Qué te tinca?</h2>
      <div class="fila intenciones zona" data-section="intenciones">
        {#each INTENCIONES as i (i.id)}
          <button data-nav class="opcion grande" class:sel={intencion === i.id} onclick={() => elegirIntencion(i.id)}>
            <span class="icono">{i.icono}</span>
            <span>{i.label}</span>
            <small>{i.desc}</small>
          </button>
        {/each}
      </div>

      <div class="fila extras zona" data-section="extras">
        <button data-nav class="chip" class:sel={corta} onclick={() => (corta = !corta)}>
          ⏱ {corta ? "Algo corto (hasta 1 h 50)" : "¿Tienes poco tiempo?"}
        </button>
        <button data-nav class="chip tenue" onclick={() => void abrirNunca()}>
          🚫 Lo que nunca quiero ver{(setup?.generosExcluidos.length ?? 0) + (setup?.temasExcluidos.length ?? 0)
            ? ` (${(setup?.generosExcluidos.length ?? 0) + (setup?.temasExcluidos.length ?? 0)})`
            : ""}
        </button>
      </div>

      {#if error}<p class="error">{error}</p>{/if}
    </section>
  {:else if pantalla === "pensando"}
    <section class="pantalla" in:fade={{ duration: 200 }}>
      <div class="pensando" aria-live="polite">
        <span class="punto"></span><span class="punto"></span><span class="punto"></span>
      </div>
      <p class="nota">Buscando tres que te calcen…</p>
      <p class="atajo"><kbd>Esc</kbd> para cambiar lo que pediste</p>
    </section>
  {:else if pantalla === "cartas"}
    <section class="pantalla" in:fade={{ duration: 250 }}>
      {#if aviso}<p class="nota">{aviso}</p>{/if}
      <div class="cartas zona" data-section="cartas">
        {#each cartas as c, i (i)}
          {#if c}
            <article class="carta" data-i={i} class:cargando={c.cargando} in:fly={{ y: 16, duration: 300, delay: i * 80 }}>
              <span class="ranura">{RANURA_LABEL[c.ranura]}</span>
              <div class="poster">
                {#if c.r.pelicula.posterPath}
                  <img src={posterUrl(c.r.pelicula, "w500")} alt="" />
                {/if}
              </div>
              <h3>{c.r.pelicula.titulo}</h3>
              <p class="datos">{datos(c.r.pelicula)}</p>
              <p class="frase">{frase(c)}</p>
              {#if c.preguntando}
                <div class="opinion">
                  <span>¿Te gustó?</span>
                  <button data-nav class="chip" onclick={() => void responder(i, true)}>👍</button>
                  <button data-nav class="chip" onclick={() => void responder(i, false)}>👎</button>
                  <button data-nav class="chip tenue" onclick={() => void responder(i, null)}>Ni fu ni fa</button>
                </div>
              {:else}
                <div class="acciones">
                  <button data-nav class="btn principal" onclick={() => descubrir(c)}>▶ Descubrir</button>
                  <button data-nav class="btn" onclick={() => void otra(i)}>{c.cargando ? "…" : "↻ Otra"}</button>
                  <button data-nav class="btn" onclick={() => yaLaVi(i)}>✓ Ya la vi</button>
                </div>
              {/if}
            </article>
          {/if}
        {/each}
      </div>
      <div class="fila extras zona" data-section="cartas-pie">
        <button data-nav class="chip" onclick={() => void buscar()}>🎲 Otras tres</button>
        <button data-nav class="chip tenue" onclick={atras}>← Cambiar lo que pedí</button>
      </div>
    </section>
  {:else}
    <section class="pantalla nunca" in:fade={{ duration: 200 }}>
      <h2>Lo que nunca quieres ver</h2>
      <p class="nota">Vera no te lo va a proponer. Se cambia cuando quieras.</p>
      {#if opsGeneros.length}
        <h3>Géneros</h3>
        <div class="fila zona" data-section="nunca-generos">
          {#each opsGeneros as o (o.id)}
            <button data-nav class="chip" class:sel={nuncaGeneros.has(o.id)} onclick={() => (nuncaGeneros = alternar(nuncaGeneros, o.id))}>
              {nuncaGeneros.has(o.id) ? "🚫 " : ""}{o.label}
            </button>
          {/each}
        </div>
      {/if}
      {#if opsTemas.length}
        <h3>Temas</h3>
        <div class="fila zona" data-section="nunca-temas">
          {#each opsTemas as o (o.id)}
            <button data-nav class="chip" class:sel={nuncaTemas.has(o.id)} onclick={() => (nuncaTemas = alternar(nuncaTemas, o.id))}>
              {nuncaTemas.has(o.id) ? "🚫 " : ""}{o.label}
            </button>
          {/each}
        </div>
      {/if}
      <div class="fila extras zona" data-section="nunca-pie">
        <button data-nav class="btn principal" disabled={guardandoNunca} onclick={() => void guardarNunca()}>Guardar</button>
        <button data-nav class="btn" onclick={atras}>Cancelar</button>
      </div>
    </section>
  {/if}
</div>

<style>
  .vera {
    position: relative;
    min-height: 100vh;
    background: #0a0608;
    color: #fff8e7;
    font-family: "Inter", system-ui, -apple-system, sans-serif;
    overflow-x: hidden;
  }
  .fondo {
    position: fixed;
    inset: 0;
    background-size: cover;
    background-position: center;
    filter: blur(18px) brightness(0.35);
    transform: scale(1.08);
    pointer-events: none;
  }
  .cabeza {
    position: relative;
    text-align: center;
    padding-top: 28px;
  }
  .marca {
    margin: 0;
    font-size: 42px;
    letter-spacing: -1px;
  }
  .marca-vera {
    background: linear-gradient(90deg, #e40303, #ff8c00, #ffed00, #008026, #24408e, #732982);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .marca em {
    font-weight: 400;
    color: #d8cbb8;
  }
  .pantalla {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 18px 20px 40px;
    text-align: center;
  }
  h2 {
    margin: 14px 0 0;
    font-size: 22px;
    font-weight: 700;
  }
  h3 {
    margin: 8px 0 0;
    font-size: 15px;
    color: #d8cbb8;
  }
  .fila {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 12px;
    max-width: 1100px;
  }
  .opcion {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 132px;
    padding: 14px 18px;
    border-radius: 16px;
    border: 2px solid #3a2c26;
    background: rgba(255, 248, 231, 0.04);
    color: inherit;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
    transition: transform 0.12s, border-color 0.12s;
  }
  .opcion.grande {
    min-width: 168px;
    padding: 18px 20px;
  }
  .opcion small {
    font-weight: 400;
    font-size: 12px;
    color: #b8a994;
  }
  .opcion .icono {
    font-size: 30px;
  }
  .opcion.sel {
    border-color: #ffae5c;
    background: rgba(255, 174, 92, 0.12);
  }
  .opcion:focus,
  .opcion:hover {
    outline: none;
    border-color: #ffae5c;
    transform: scale(1.04);
  }
  .chip {
    padding: 9px 16px;
    border-radius: 999px;
    border: 1px solid #4a3a32;
    background: rgba(255, 248, 231, 0.05);
    color: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .chip.sel {
    border-color: #ffae5c;
    background: rgba(255, 174, 92, 0.16);
  }
  .chip.tenue {
    color: #b8a994;
  }
  .chip:focus,
  .chip:hover,
  .btn:focus,
  .btn:hover {
    outline: none;
    border-color: #ffae5c;
    box-shadow: 0 0 0 3px rgba(255, 174, 92, 0.3);
  }
  .opinable {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 12px 16px;
    border-radius: 14px;
    background: rgba(255, 174, 92, 0.08);
    border: 1px solid #5a4030;
  }
  .nota {
    margin: 0;
    color: #c9b9a2;
    font-size: 14px;
    max-width: 640px;
  }
  .error {
    margin: 0;
    color: #ff8a7a;
    font-size: 14px;
  }
  .atajo {
    margin: 0;
    font-size: 12px;
    color: #8a7c6a;
  }
  kbd {
    border: 1px solid #4a3a32;
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 11px;
  }
  .pensando {
    display: flex;
    gap: 10px;
    margin-top: 60px;
  }
  .punto {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #ffae5c;
    animation: latido 1.1s ease-in-out infinite;
  }
  .punto:nth-child(2) {
    animation-delay: 0.18s;
  }
  .punto:nth-child(3) {
    animation-delay: 0.36s;
  }
  @keyframes latido {
    0%,
    100% {
      opacity: 0.25;
      transform: scale(0.8);
    }
    50% {
      opacity: 1;
      transform: scale(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .punto {
      animation: none;
    }
  }
  .cartas {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 300px));
    gap: 22px;
    justify-content: center;
    width: 100%;
  }
  @media (max-width: 900px) {
    .cartas {
      grid-template-columns: minmax(0, 340px);
    }
  }
  .carta {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border-radius: 18px;
    background: rgba(20, 14, 12, 0.82);
    border: 1px solid #3a2c26;
    text-align: left;
    transition: opacity 0.15s;
  }
  .carta.cargando {
    opacity: 0.55;
  }
  .carta:focus-within {
    border-color: #ffae5c;
  }
  .ranura {
    align-self: flex-start;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.8px;
    text-transform: uppercase;
    color: #ffae5c;
  }
  .poster {
    aspect-ratio: 2 / 3;
    max-height: 42vh;
    border-radius: 12px;
    overflow: hidden;
    background: #1f1814;
  }
  .poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .carta h3 {
    margin: 4px 0 0;
    font-size: 18px;
    color: #fff8e7;
  }
  .datos {
    margin: 0;
    font-size: 12px;
    color: #b8a994;
  }
  .frase {
    margin: 0;
    font-size: 14px;
    line-height: 1.4;
    color: #f0e2cc;
    min-height: 2.8em;
  }
  .acciones,
  .opinion {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .opinion span {
    font-size: 14px;
    font-weight: 700;
  }
  .btn {
    padding: 9px 14px;
    border-radius: 10px;
    border: 1px solid #4a3a32;
    background: transparent;
    color: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn.principal {
    background: #ffae5c;
    border-color: #ffae5c;
    color: #1a0f08;
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>
