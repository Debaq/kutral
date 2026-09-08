<script lang="ts">
  // Vera and Chill — flujo completo.
  // Pantallas: entrada → [setup] → contexto → intencion → calificar → ranking.
  //
  // SETUP (opcional, una vez): plataformas, géneros y temas que no quieres ver,
  //   tope de duración y voz de Vera. Vive en la tabla vera_setup.
  // CALIFICAR: N cartas, una por vez. N se adapta a cuánto te conoce Vera
  //   (3 si ya tiene perfil, 8 si es tu primera vez). ←→ ajusta -5..+5.
  //   ↑ toggle vista (juicio dual). ↓ saltar (interes=-1). Enter pasa.
  // RANKING: lista navegable, #1 destacada. ↑↓ navega, Enter Descubre.
  //
  // Esc = volver atrás (a la pantalla previa, o a home si ya estás en entrada).
  // Backspace = salir directo a la home, sin importar la pantalla.
  //
  // ENRIQUECIMIENTO PEREZOSO: el pool se arma con los listados de TMDb (rápido,
  // 3 requests) y solo las pelis que vas a MIRAR piden /detail. Las cartas de
  // calibración se enriquecen antes de mostrarse; el resto del pool, en
  // segundo plano mientras calificas. Ver tmdb.ts.

  import { fade, fly } from "svelte/transition";
  import { tick, onMount } from "svelte";
  import { goto } from "$app/navigation";

  import type {
    Pelicula,
    Contexto,
    EstadoVera,
    Reaccion,
    RankingPeli,
    PerfilHistorico,
    PerfilSetup,
    Personalidad,
    PrefAnimacion,
  } from "$lib/vera/tipos";
  import { recomendar, hayMatch } from "$lib/vera/motor";
  import {
    cargarCatalogoPorIntent,
    enriquecer,
    motivoExclusion,
    posterUrl,
    backdropUrl,
    imageUrl,
    TmdbNoKeyError,
    type FiltrosDiscover,
  } from "$lib/vera/tmdb";
  import { INTENCIONES, filtrosParaIntent } from "$lib/vera/intenciones";
  import { setReaccion, getReaccionesMap } from "$lib/vera/ratings";
  import { getPerfilHistorico, pesoReaccionPersistida } from "$lib/vera/historial";
  import { cartasParaMuestras } from "$lib/vera/config";
  import {
    getSetup,
    guardarSetup,
    listarPlataformas,
    listarGeneros,
    listarTemas,
    type OpcionCatalogo,
  } from "$lib/vera/setup";
  import { registrarCambio, registrarEleccion } from "$lib/vera/aprendizaje";
  import {
    IDIOMAS,
    COMODIDAD_IDIOMAS,
    comodidadDesde,
    nombreIdioma,
  } from "$lib/vera/idiomas";
  import { voz, PERSONALIDADES, etiquetaPersonalidad } from "$lib/vera/voz";
  import { ayuda, type AtajoLinea } from "$lib/atajos/store.svelte";

  // --- Pantallas ---
  type Pantalla =
    | "entrada"
    | "setup"
    | "contexto"
    | "intencion"
    | "calificar"
    | "ranking";
  let pantalla = $state<Pantalla>("entrada");

  // --- Estado global Vera ---
  let estado = $state<EstadoVera>({
    contexto: null,
    intencion: null,
    reacciones: [],
    horaActual: new Date().getHours(),
    setup: null,
  });

  // Copy según la personalidad elegida en el setup. Solo texto: el motor
  // rankea igual para las tres voces.
  let v = $derived(voz(estado.setup?.personalidad));

  // --- Catálogo (pool) ---
  let catalogo = $state<Pelicula[]>([]);
  let cargandoPool = $state(false);
  let errorPool = $state<string | null>(null);
  let errorPoolEsKey = $state(false);
  // Cuántas pelis se descartaron por chocar con algo que el usuario pidió no
  // ver (tema sensible, idioma que no tolera, animación). Se muestra: filtrar
  // en silencio hace parecer que Vera "no encontró" nada.
  let descartadas = $state(0);

  // --- Entrada ---
  const OPCIONES_ENTRADA = ["empezar", "configurar"] as const;
  let idxEntrada = $state(0);

  // --- Contexto ---
  // Regla de marca: "ninos" SIEMPRE implica adulto presente.
  const contextos: {
    id: Contexto;
    label: string;
    icono: string;
    nota?: string;
  }[] = [
    { id: "solo", label: "Solo", icono: "🧍" },
    { id: "pareja", label: "En pareja", icono: "👥" },
    { id: "amigos", label: "Con amigos o familia", icono: "👨‍👩‍👦‍👦" },
    {
      id: "ninos",
      label: "Con niños",
      icono: "👶",
      nota: "con un adulto cerca",
    },
  ];
  let idxContexto = $state(0);

  // --- Intencion ---
  let idxIntencion = $state(0);
  // Filtros fijados al confirmar el intent. NO derivar: el sort_by + page
  // random deben quedar estables durante la ronda (variar en cada llamada
  // los reshufflearía). "Otra ronda" sí los re-calcula para traer pool fresco.
  let filtrosIntent = $state<FiltrosDiscover | null>(null);

  // --- Calificar ---
  // Cuántas cartas se piden sale de cartasParaMuestras(): 8 la primera vez,
  // 3 cuando Vera ya tiene perfil. Antes eran 8 siempre, incluso para alguien
  // con 200 pelis calificadas.
  let pelisACalificar = $state<Pelicula[]>([]);
  let idxCalificar = $state(0);
  // Flag de "saltada" para mostrar feedback chico ("Saltada · -1 automático").
  let ultimaSaltada = $state(false);

  // --- Ranking ---
  let ranking = $state<RankingPeli[]>([]);
  let idxActiva = $state(0);
  // Cache del perfil histórico: cargado UNA vez al entrar a calificar y
  // reusado en cada rerankeo para evitar I/O a SQLite por flecha.
  let historicoCache: PerfilHistorico | null = null;
  // State local de reacciones del catálogo actual. Mirror reactivo de
  // localStorage para que el template re-renderice cuando cambia.
  let reaccionesPorPeli = $state<
    Map<string, { interes: number; juicio: number | null }>
  >(new Map());

  // --- Setup ---
  type PasoSetup =
    | "plataformas"
    | "animacion"
    | "idiomas"
    | "idiomasEvitados"
    | "generos"
    | "temas"
    | "duracion"
    | "voz";
  const PASOS: PasoSetup[] = [
    "plataformas",
    "animacion",
    "idiomas",
    "idiomasEvitados",
    "generos",
    "temas",
    "duracion",
    "voz",
  ];
  let pasoSetup = $state(0);
  let idxSetup = $state(0);
  let guardandoSetup = $state(false);
  let opsPlataformas = $state<OpcionCatalogo[]>([]);
  let opsGeneros = $state<OpcionCatalogo[]>([]);
  let opsTemas = $state<OpcionCatalogo[]>([]);
  // Borrador editable. Se vuelca a vera_setup recién al terminar el asistente.
  let borrador = $state<PerfilSetup>({
    plataformas: [],
    generosExcluidos: [],
    temasExcluidos: [],
    personalidad: "calida",
    duracionMax: null,
    animacion: "indiferente",
    idiomasComodos: [],
    idiomasEvitados: [],
  });

  // Opciones de tope de duración. null = sin tope.
  const DURACIONES: { label: string; valor: number | null }[] = [
    { label: "Sin tope", valor: null },
    { label: "Hasta 1h 30", valor: 90 },
    { label: "Hasta 2h", valor: 120 },
    { label: "Hasta 2h 30", valor: 150 },
  ];

  // Opciones del paso actual del setup, como lista uniforme para navegar.
  let opcionesPaso = $derived.by(() => {
    switch (PASOS[pasoSetup]) {
      case "plataformas":
        return opsPlataformas;
      case "generos":
        return opsGeneros;
      case "temas":
        return opsTemas;
      case "animacion":
        return ANIMACIONES.map((a) => ({
          id: a.id,
          label: a.label,
          description: a.desc,
        }));
      case "idiomas":
        return COMODIDAD_IDIOMAS.map((c) => ({
          id: c.id,
          label: c.label,
          description: c.desc,
        }));
      case "idiomasEvitados":
        return IDIOMAS.map((i) => ({
          id: i.codigo,
          label: i.nombre,
          description: null,
        }));
      case "duracion":
        return DURACIONES.map((d, i) => ({
          id: String(i),
          label: d.label,
          description: null,
        }));
      case "voz":
        return PERSONALIDADES.map((p) => {
          const e = etiquetaPersonalidad(p);
          return { id: p, label: e.label, description: e.desc };
        });
    }
  });

  // Pasos de selección múltiple (Espacio marca). Los otros dos son de opción
  // única y Enter confirma directamente.
  let pasoEsMultiple = $derived(
    ["plataformas", "generos", "temas", "idiomasEvitados"].includes(
      PASOS[pasoSetup],
    ),
  );

  // Postura frente a la animación. Tres estados y no una casilla, porque
  // "me encantan" y "no me gustan" son cosas distintas y ninguna es el default.
  const ANIMACIONES: { id: PrefAnimacion; label: string; desc: string }[] = [
    { id: "gusta", label: "Me encantan", desc: "Súbelas en el ranking" },
    { id: "indiferente", label: "Me dan igual", desc: "Trátalas como el resto" },
    { id: "no", label: "No, gracias", desc: "No me las muestres" },
  ];

  function seleccionDelPaso(): string[] {
    switch (PASOS[pasoSetup]) {
      case "plataformas":
        return borrador.plataformas;
      case "generos":
        return borrador.generosExcluidos;
      case "temas":
        return borrador.temasExcluidos;
      case "idiomasEvitados":
        return borrador.idiomasEvitados;
      default:
        return [];
    }
  }

  function estaMarcada(id: string): boolean {
    const paso = PASOS[pasoSetup];
    if (paso === "duracion") {
      return DURACIONES[Number(id)]?.valor === borrador.duracionMax;
    }
    if (paso === "voz") return borrador.personalidad === id;
    if (paso === "animacion") return borrador.animacion === id;
    if (paso === "idiomas") {
      return comodidadDesde(borrador.idiomasComodos) === id;
    }
    return seleccionDelPaso().includes(id);
  }

  function alternarMarca(id: string): void {
    const paso = PASOS[pasoSetup];
    const quitar = (xs: string[]) =>
      xs.includes(id) ? xs.filter((x) => x !== id) : [...xs, id];
    if (paso === "plataformas") borrador.plataformas = quitar(borrador.plataformas);
    else if (paso === "generos")
      borrador.generosExcluidos = quitar(borrador.generosExcluidos);
    else if (paso === "temas")
      borrador.temasExcluidos = quitar(borrador.temasExcluidos);
    else if (paso === "idiomasEvitados")
      borrador.idiomasEvitados = quitar(borrador.idiomasEvitados);
    else if (paso === "duracion")
      borrador.duracionMax = DURACIONES[Number(id)]?.valor ?? null;
    else if (paso === "voz") borrador.personalidad = id as Personalidad;
    else if (paso === "animacion") borrador.animacion = id as PrefAnimacion;
    else if (paso === "idiomas") {
      borrador.idiomasComodos =
        COMODIDAD_IDIOMAS.find((c) => c.id === id)?.idiomas ?? [];
    }
  }

  async function avanzarSetup(): Promise<void> {
    // En los pasos de opción única, Enter también elige lo que está resaltado.
    if (!pasoEsMultiple) {
      const op = opcionesPaso[idxSetup];
      if (op) alternarMarca(op.id);
    }
    if (pasoSetup + 1 < PASOS.length) {
      pasoSetup++;
      idxSetup = 0;
      return;
    }
    guardandoSetup = true;
    const ok = await guardarSetup(borrador);
    guardandoSetup = false;
    if (ok) {
      estado.setup = { ...borrador };
    } else {
      // Sin DB el perfil no persiste, pero sí vale para esta sesión: mejor
      // aplicar lo que el usuario acaba de elegir que descartarlo en silencio.
      estado.setup = { ...borrador };
      errorPool = "El perfil no se pudo guardar; vale solo para esta sesión.";
    }
    pantalla = "entrada";
    idxEntrada = 0;
  }

  function retrocederSetup(): void {
    if (pasoSetup > 0) {
      pasoSetup--;
      idxSetup = 0;
    } else {
      pantalla = "entrada";
    }
  }

  async function abrirSetup(): Promise<void> {
    // Las listas viven en Rust (mismo vocabulario que el importador de
    // catálogo). Se piden una sola vez por sesión.
    if (opsPlataformas.length === 0) {
      const [pl, ge, te] = await Promise.all([
        listarPlataformas(),
        listarGeneros(),
        listarTemas(),
      ]);
      opsPlataformas = pl;
      opsGeneros = ge;
      opsTemas = te;
    }
    borrador = estado.setup
      ? { ...estado.setup }
      : {
          plataformas: [],
          generosExcluidos: [],
          temasExcluidos: [],
          personalidad: "calida",
          duracionMax: null,
          animacion: "indiferente",
          idiomasComodos: [],
          idiomasEvitados: [],
        };
    pasoSetup = 0;
    idxSetup = 0;
    pantalla = "setup";
  }

  // --- Refs para foco ---
  //
  // El foco de las listas se resuelve por CONTENEDOR + `data-idx`, no con un
  // `bind:this` por botón. Dos motivos:
  //
  //  1. El bug que había: cada `{#each}` hacía `bind:this` sobre una sola
  //     variable, así que la ref terminaba apuntando al ÚLTIMO botón del grupo
  //     y el foco nunca caía sobre la opción activa — se veía el resalte en una
  //     y el anillo de foco en otra.
  //  2. La corrección obvia (un array `$state` indexado, `bind:this={refs[i]}`)
  //     se muerde la cola: el binding LEE `refs[i]` al renderizar y ESCRIBE en
  //     él al montar, y escribir un índice nuevo en un array proxy también muta
  //     `length`, así que el `{#each}` se reinvalida a sí mismo y el render
  //     entra en bucle.
  //
  // Un contenedor y una consulta por índice no tienen ninguno de los dos
  // problemas: el DOM no es estado reactivo y no hace falta tratarlo como tal.
  let refLista = $state<HTMLElement | null>(null);
  let refCalificar = $state<HTMLElement | null>(null);
  let refRanking = $state<HTMLElement | null>(null);
  let refDescubrir = $state<HTMLButtonElement | null>(null);

  // Índice de la opción enfocada en la pantalla actual.
  let idxFoco = $derived.by(() => {
    switch (pantalla) {
      case "entrada":
        return idxEntrada;
      case "setup":
        return idxSetup;
      case "contexto":
        return idxContexto;
      case "intencion":
        return idxIntencion;
      default:
        return -1;
    }
  });

  $effect(() => {
    void pantalla;
    void idxFoco;
    void pasoSetup;
    // idxActiva importa aunque el ranking no use `idxFoco`: el botón Descubrir
    // vive dentro de un {#key} y se REMONTA cada vez que cambia la peli activa,
    // así que sin esta dependencia el foco se perdía al navegar con ↑↓.
    void idxActiva;
    tick().then(() => {
      if (pantalla === "calificar") refCalificar?.focus();
      else if (pantalla === "ranking") (refDescubrir ?? refRanking)?.focus();
      else if (idxFoco >= 0) {
        refLista
          ?.querySelector<HTMLElement>(`[data-idx="${idxFoco}"]`)
          ?.focus();
      }
    });
  });

  onMount(async () => {
    estado.setup = await getSetup();
  });

  // --- Helpers ---
  const clamp = (lo: number, hi: number, v: number) =>
    Math.max(lo, Math.min(hi, v));

  function obtenerReaccion(id: string): {
    interes: number;
    juicio: number | null;
  } {
    return reaccionesPorPeli.get(id) ?? { interes: 0, juicio: null };
  }

  function actualizarReaccionLocal(
    id: string,
    rec: { interes: number; juicio: number | null },
  ): void {
    const m = new Map(reaccionesPorPeli);
    m.set(id, rec);
    reaccionesPorPeli = m;
  }

  // Persiste la reacción en localStorage y en la memoria larga (vera_weights).
  // Single source of mutation.
  //
  // El delta para vera_weights se calcula ACÁ porque este es el único punto
  // que conoce el peso anterior y el nuevo a la vez. Mandar solo el nuevo
  // haría que recalificar la misma peli la contara dos veces.
  function guardarReaccion(
    p: Pelicula,
    rec: { interes: number; juicio: number | null },
  ): void {
    const antes = obtenerReaccion(p.id);
    const pesoAntes = pesoReaccionPersistida({ tmdb: p.rating, ...antes });
    const pesoAhora = pesoReaccionPersistida({ tmdb: p.rating, ...rec });
    setReaccion(p.id, { tmdb: p.rating, ...rec });
    actualizarReaccionLocal(p.id, rec);
    void registrarCambio(p, pesoAntes, pesoAhora);
  }

  // Convierte reaccionesPorPeli + catalogo en estado.reacciones para el motor.
  function reaccionesDesdeCatalogo(): Reaccion[] {
    const out: Reaccion[] = [];
    for (const p of catalogo) {
      const r = reaccionesPorPeli.get(p.id);
      if (!r) continue;
      if (r.interes === 0 && r.juicio === null) continue;
      out.push({ pelicula: p, interes: r.interes, juicio: r.juicio });
    }
    return out;
  }

  // Mapa id → juicio para que el motor pueda penalizar lo ya visto.
  function juiciosActuales(): Map<string, number | null> {
    const m = new Map<string, number | null>();
    for (const [id, r] of reaccionesPorPeli) m.set(id, r.juicio);
    return m;
  }

  // --- Enriquecimiento ---

  // Pide /detail para las pelis dadas y vuelca el resultado en el catálogo.
  // Devuelve las que hay que DESCARTAR: al enriquecer aparecen los temas
  // sensibles, y recién ahí se puede saber si la peli choca con lo que el
  // usuario pidió no ver.
  async function enriquecerYAplicar(pelis: Pelicula[]): Promise<Set<string>> {
    const completas = await enriquecer(pelis);
    if (completas.size === 0) return new Set();

    const aDescartar = new Set<string>();
    for (const [id, p] of completas) {
      if (motivoExclusion(p, estado.setup) !== null) aDescartar.add(id);
    }

    catalogo = catalogo
      .map((p) => completas.get(p.id) ?? p)
      .filter((p) => !aDescartar.has(p.id));

    if (aDescartar.size > 0) descartadas += aDescartar.size;
    return aDescartar;
  }

  // --- Navegación ---

  function elegirContextoActual(): void {
    estado.contexto = contextos[idxContexto].id;
    pantalla = "intencion";
  }

  // Confirma el intent y dispara la carga del pool + entrada a calificar.
  async function elegirIntencionActual(): Promise<void> {
    const id = INTENCIONES[idxIntencion].id;
    estado.intencion = id;
    filtrosIntent = filtrosParaIntent(id);
    await cargarRonda(filtrosIntent);
  }

  // Carga un pool y entra a calificar. Compartido por "confirmar intent" y
  // "otra ronda" — el único cambio entre los dos es de dónde salen los filtros.
  async function cargarRonda(filtros: FiltrosDiscover): Promise<void> {
    cargandoPool = true;
    errorPool = null;
    errorPoolEsKey = false;
    descartadas = 0;
    try {
      catalogo = await cargarCatalogoPorIntent(filtros, estado.setup);
      await entrarACalificar();
    } catch (e: unknown) {
      if (e instanceof TmdbNoKeyError) {
        errorPoolEsKey = true;
        errorPool = e.message;
      } else {
        errorPool = e instanceof Error ? e.message : String(e);
      }
      pantalla = "intencion";
    } finally {
      cargandoPool = false;
    }
  }

  // Prepara la ronda de calificación: carga histórico, hidrata reacciones,
  // elige las cartas y las enriquece antes de mostrarlas.
  async function entrarACalificar(): Promise<void> {
    historicoCache = await getPerfilHistorico();

    // Hidratar reacciones locales con lo persistido.
    const mapaPersistido = getReaccionesMap();
    const nuevoMap = new Map<
      string,
      { interes: number; juicio: number | null }
    >();
    for (const p of catalogo) {
      const rec = mapaPersistido[p.id];
      if (rec)
        nuevoMap.set(p.id, { interes: rec.interes, juicio: rec.juicio });
    }
    reaccionesPorPeli = nuevoMap;

    // Cuántas cartas pedir depende de cuánto te conoce Vera.
    const cuantas = cartasParaMuestras(historicoCache.muestras);

    // Preferir pelis SIN reacción previa: calificar de nuevo algo que ya
    // calificaste no le enseña nada al motor y es el peaje que más molesta.
    const frescas = catalogo.filter((p) => !mapaPersistido[p.id]);
    const repetidas = catalogo.filter((p) => mapaPersistido[p.id]);
    const elegidas = [...frescas, ...repetidas].slice(0, cuantas);

    // Las cartas se enriquecen ANTES de mostrarse: son las únicas fichas que
    // el usuario mira completas en esta pantalla. Un lote de 6 en paralelo.
    const descartadas = await enriquecerYAplicar(elegidas);
    pelisACalificar = elegidas
      .filter((p) => !descartadas.has(p.id))
      .map((p) => catalogo.find((c) => c.id === p.id) ?? p);

    idxCalificar = 0;
    ultimaSaltada = false;
    precargarImagenes(pelisACalificar);
    pantalla = "calificar";

    // El resto del pool se enriquece en segundo plano, mientras el usuario
    // califica. Para cuando llegue al ranking, las fichas ya están completas.
    void enriquecerFondo(elegidas.map((p) => p.id));
  }

  // Enriquecimiento en segundo plano del resto del pool. No bloquea la UI y no
  // toca `pantalla`: solo mejora los datos que el ranking va a usar.
  async function enriquecerFondo(yaHechos: string[]): Promise<void> {
    const hechos = new Set(yaHechos);
    const resto = catalogo.filter((p) => !hechos.has(p.id) && !p.enriquecida);
    if (resto.length === 0) return;
    await enriquecerYAplicar(resto);
    // Si el usuario ya está en el ranking, rerankeamos con los datos nuevos:
    // ahora hay director, reparto y duración, que el score usa.
    if (pantalla === "ranking") await rerankear();
  }

  function precargarImagenes(pelis: Pelicula[]): void {
    for (const p of pelis) {
      const url = posterUrl(p, "w500");
      if (url) {
        const img = new Image();
        img.src = url;
      }
      const back = backdropUrl(p, "w780");
      if (back) {
        const img = new Image();
        img.src = back;
      }
    }
  }

  // --- Acciones en pantalla "calificar" ---

  function ajustarCarta(delta: -1 | 1): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    ultimaSaltada = false;
    const actual = obtenerReaccion(p.id);
    let nueva: { interes: number; juicio: number | null };
    if (actual.juicio !== null) {
      nueva = {
        interes: actual.interes,
        juicio: clamp(-5, 5, actual.juicio + delta),
      };
    } else {
      nueva = {
        interes: clamp(-5, 5, actual.interes + delta),
        juicio: null,
      };
    }
    guardarReaccion(p, nueva);
  }

  function toggleVistaCarta(): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    ultimaSaltada = false;
    const actual = obtenerReaccion(p.id);
    guardarReaccion(p, {
      interes: actual.interes,
      juicio: actual.juicio === null ? 0 : null,
    });
  }

  // ↓ Saltar carta: asigna interes=-1 (puntaje bajo automático, no descarta).
  // El silencio cuenta — quien salta enseña al sistema que no le interesó
  // tanto como para opinar. La peli aparece más abajo en el ranking pero
  // no desaparece.
  function saltarCarta(): void {
    const p = pelisACalificar[idxCalificar];
    if (!p) return;
    const actual = obtenerReaccion(p.id);
    // Solo asignar -1 si todavía no calificó (no pisar si ya puso algo).
    if (actual.interes === 0 && actual.juicio === null) {
      guardarReaccion(p, { interes: -1, juicio: null });
    }
    ultimaSaltada = true;
    avanzarCarta();
  }

  function avanzarCarta(): void {
    if (idxCalificar + 1 < pelisACalificar.length) {
      idxCalificar++;
      ultimaSaltada = false;
    } else {
      void irAlRanking();
    }
  }

  // --- Ranking ---

  async function rerankear(): Promise<void> {
    if (!historicoCache) historicoCache = await getPerfilHistorico();
    estado.reacciones = reaccionesDesdeCatalogo();
    const previa = ranking[idxActiva]?.pelicula.id;
    ranking = await recomendar(
      catalogo,
      estado,
      historicoCache,
      juiciosActuales(),
    );
    // Mantener la peli que el usuario estaba mirando aunque cambie de posición
    // tras el rerankeo: que se le mueva la ficha bajo el cursor es peor que
    // que el orden no sea perfecto.
    if (previa) {
      const i = ranking.findIndex((r) => r.pelicula.id === previa);
      idxActiva = i >= 0 ? i : 0;
    }
  }

  async function irAlRanking(): Promise<void> {
    idxActiva = 0;
    await rerankear();
    pantalla = "ranking";
  }

  // Otra ronda: nuevos filtros (page+sort random), nuevo pool, vuelve a
  // calificar. Mantiene contexto + intent.
  async function otraRonda(): Promise<void> {
    if (estado.intencion === null) {
      pantalla = "intencion";
      return;
    }
    filtrosIntent = filtrosParaIntent(estado.intencion);
    await cargarRonda(filtrosIntent);
  }

  // Handoff al player de la home con la peli activa del ranking.
  // Palabra de marca: "Descubrir", no "Reproducir" ni "Play".
  function descubrir(): void {
    const activa = ranking[idxActiva];
    if (!activa) return;
    const r = obtenerReaccion(activa.pelicula.id);
    void registrarEleccion(activa.pelicula, r.juicio ?? r.interes);
    goto(`/?play=${activa.pelicula.id}&type=movie`);
  }

  function salirDeVera(): void {
    goto("/");
  }

  // Esc: paso atrás dentro del flujo. Si ya estás en la primera pantalla
  // (entrada), sale a la home.
  function escAtras(): void {
    if (pantalla === "entrada") salirDeVera();
    else if (pantalla === "setup") retrocederSetup();
    else if (pantalla === "contexto") pantalla = "entrada";
    else if (pantalla === "intencion") pantalla = "contexto";
    else if (pantalla === "calificar") pantalla = "intencion";
    else if (pantalla === "ranking") pantalla = "calificar";
  }

  // --- Atajos ---

  function atajosActuales(): AtajoLinea[] {
    // Reglas globales — todas las pantallas las heredan al final:
    //   Esc       = paso atrás (a la pantalla anterior; si ya estás en
    //               entrada, sale a home).
    //   Backspace = salir directo a la home raíz.
    //   I-I       = ayuda.
    const globales: AtajoLinea[] = [
      { tecla: "Esc", desc: "Volver atrás" },
      { tecla: "Backspace", desc: "Salir a home" },
      { tecla: "I-I", desc: "Ayuda" },
    ];
    if (pantalla === "entrada") {
      return [
        { tecla: "← →", desc: "Empezar / configurar" },
        { tecla: "Enter", desc: "Confirmar" },
        ...globales,
      ];
    }
    if (pantalla === "setup") {
      return [
        { tecla: "← → ↑ ↓", desc: "Mover entre opciones" },
        ...(pasoEsMultiple
          ? [{ tecla: "Espacio", desc: "Marcar / desmarcar" }]
          : []),
        { tecla: "Enter", desc: "Siguiente paso" },
        ...globales,
      ];
    }
    if (pantalla === "contexto") {
      return [
        { tecla: "← → ↑ ↓", desc: "Mover entre opciones" },
        { tecla: "Enter", desc: "Confirmar contexto" },
        ...globales,
      ];
    }
    if (pantalla === "intencion") {
      return [
        { tecla: "← →", desc: "Elegir intención" },
        { tecla: "Enter", desc: "Confirmar y buscar" },
        ...globales,
      ];
    }
    if (pantalla === "calificar") {
      return [
        { tecla: "← →", desc: "Ajustar puntaje (-5..+5)" },
        { tecla: "↑", desc: "Marcar / desmarcar como vista" },
        { tecla: "↓", desc: "Saltar (puntaje -1 automático)" },
        { tecla: "Enter", desc: "Siguiente carta" },
        ...globales,
      ];
    }
    if (pantalla === "ranking") {
      return [
        { tecla: "↑ ↓", desc: "Navegar entre pelis" },
        { tecla: "Enter", desc: "Descubrir esta peli" },
        { tecla: "Espacio", desc: "Otra ronda" },
        ...globales,
      ];
    }
    return [];
  }

  $effect(() => {
    void pantalla;
    void pasoSetup;
    void cargandoPool;
    void errorPool;
    ayuda.set(pantalla, atajosActuales());
  });

  // --- Handler único de teclado ---
  function onKey(e: KeyboardEvent): void {
    if (ayuda.visible) return;

    const k = e.key;

    // Esc = paso atrás (consistente en toda la app).
    if (k === "Escape") {
      e.preventDefault();
      escAtras();
      return;
    }

    // Backspace = salir directo a la home raíz (consistente en toda la app).
    if (k === "Backspace") {
      e.preventDefault();
      salirDeVera();
      return;
    }

    if (pantalla === "entrada") {
      if (k === "ArrowRight" || k === "ArrowDown") {
        e.preventDefault();
        idxEntrada = (idxEntrada + 1) % OPCIONES_ENTRADA.length;
      } else if (k === "ArrowLeft" || k === "ArrowUp") {
        e.preventDefault();
        idxEntrada =
          (idxEntrada - 1 + OPCIONES_ENTRADA.length) % OPCIONES_ENTRADA.length;
      } else if (k === "Enter") {
        e.preventDefault();
        if (OPCIONES_ENTRADA[idxEntrada] === "empezar") pantalla = "contexto";
        else void abrirSetup();
      }
      return;
    }

    if (pantalla === "setup") {
      if (guardandoSetup) return;
      const n = opcionesPaso.length;
      if (n === 0) {
        if (k === "Enter") {
          e.preventDefault();
          void avanzarSetup();
        }
        return;
      }
      if (k === "ArrowRight" || k === "ArrowDown") {
        e.preventDefault();
        idxSetup = (idxSetup + 1) % n;
      } else if (k === "ArrowLeft" || k === "ArrowUp") {
        e.preventDefault();
        idxSetup = (idxSetup - 1 + n) % n;
      } else if (k === " ") {
        e.preventDefault();
        const op = opcionesPaso[idxSetup];
        if (op) alternarMarca(op.id);
      } else if (k === "Enter") {
        e.preventDefault();
        void avanzarSetup();
      }
      return;
    }

    if (pantalla === "contexto") {
      if (k === "ArrowRight" || k === "ArrowDown") {
        e.preventDefault();
        idxContexto = (idxContexto + 1) % contextos.length;
      } else if (k === "ArrowLeft" || k === "ArrowUp") {
        e.preventDefault();
        idxContexto =
          (idxContexto - 1 + contextos.length) % contextos.length;
      } else if (k === "Enter") {
        e.preventDefault();
        elegirContextoActual();
      }
      return;
    }

    if (pantalla === "intencion") {
      if (cargandoPool) return;
      if (errorPool) {
        if (k === "Enter") {
          e.preventDefault();
          void elegirIntencionActual();
        }
        return;
      }
      if (k === "ArrowRight") {
        e.preventDefault();
        idxIntencion = (idxIntencion + 1) % INTENCIONES.length;
      } else if (k === "ArrowLeft") {
        e.preventDefault();
        idxIntencion =
          (idxIntencion - 1 + INTENCIONES.length) % INTENCIONES.length;
      } else if (k === "Enter") {
        e.preventDefault();
        void elegirIntencionActual();
      }
      return;
    }

    if (pantalla === "calificar") {
      if (pelisACalificar.length === 0) return;
      if (k === "ArrowLeft") {
        e.preventDefault();
        ajustarCarta(-1);
      } else if (k === "ArrowRight") {
        e.preventDefault();
        ajustarCarta(1);
      } else if (k === "ArrowUp") {
        e.preventDefault();
        toggleVistaCarta();
      } else if (k === "ArrowDown") {
        e.preventDefault();
        saltarCarta();
      } else if (k === "Enter") {
        e.preventDefault();
        avanzarCarta();
      }
      return;
    }

    if (pantalla === "ranking") {
      if (k === " ") {
        e.preventDefault();
        void otraRonda();
        return;
      }
      if (ranking.length === 0) return;
      if (k === "ArrowUp") {
        e.preventDefault();
        idxActiva = Math.max(0, idxActiva - 1);
      } else if (k === "ArrowDown") {
        e.preventDefault();
        idxActiva = Math.min(ranking.length - 1, idxActiva + 1);
      } else if (k === "Enter") {
        e.preventDefault();
        descubrir();
      }
      return;
    }
  }

  // Drift de fondo desde el pool (vacío hasta que se carga).
  let driftPosters = $derived(catalogo.slice(0, 12).filter((p) => p.posterPath));

  // ¿Vale la pena lo que Vera encontró? Si ni la #1 llega al umbral, lo dice.
  let sinMatch = $derived(ranking.length > 0 && !hayMatch(ranking));

  // Etiquetas legibles para los temas sensibles de la ficha.
  let etiquetaTema = $derived.by(() => {
    const m = new Map<string, string>();
    for (const t of opsTemas) m.set(t.id, t.label);
    return m;
  });

  function nombreTema(id: string): string {
    return etiquetaTema.get(id) ?? id;
  }

  // Frase de por qué Vera propone esta peli. Se arma con las etiquetas que más
  // aportaron al score, no con una plantilla fija.
  function porQueTexto(r: RankingPeli): string {
    const razones = r.porQue.razones;
    if (razones.length === 0) {
      return r.porQue.calidad > 0.75
        ? "Muy bien valorada, y todavía no sé qué te gusta."
        : "Todavía te conozco poco: esta es una apuesta.";
    }
    if (razones.length === 1) return `Porque te gusta ${razones[0]}.`;
    const ultima = razones[razones.length - 1];
    return `Porque te gusta ${razones.slice(0, -1).join(", ")} y ${ultima}.`;
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- Fondo cinematográfico (drift desde catálogo cuando existe) -->
<div class="fondo" aria-hidden="true">
  {#each driftPosters as p, i (p.id)}
    <div
      class="drift"
      style="background-color:{p.poster}; background-image:url('{posterUrl(p, 'w342')}'); left:{(i * 9) % 100}%; animation-delay:-{i * 2.3}s;"
    ></div>
  {/each}
  <div class="vignette"></div>
</div>

{#if pantalla === "entrada"}
  <section class="pantalla entrada" in:fade={{ duration: 380 }}>
    <h1 class="marca">
      <span class="marca-vera">Vera</span> <em>and Chill</em>
    </h1>
    <p class="slogan">{v.slogan}</p>
    <p class="sub">
      {#if estado.setup}
        Sé qué no quieres ver. Califica unas pocas y te muestro el ranking.
      {:else}
        Calificas unas cuantas, te muestro el ranking, eliges.
      {/if}
    </p>
    <div class="opciones entrada-opciones" bind:this={refLista}>
      <button
        data-idx="0"
        class="btn-grande naranja"
        class:activa={idxEntrada === 0}
        onclick={() => (pantalla = "contexto")}
      >
        Empezar
      </button>
      <button
        data-idx="1"
        class="btn-secundario"
        class:activa={idxEntrada === 1}
        onclick={() => void abrirSetup()}
      >
        {estado.setup ? "Ajustar preferencias" : "Configurar a Vera"}
      </button>
    </div>
    <p class="atajo">
      <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar
    </p>
  </section>
{:else if pantalla === "setup"}
  {@const paso = PASOS[pasoSetup]}
  <section class="pantalla setup" in:fly={{ y: 12, duration: 320 }}>
    <p class="contador">Paso {pasoSetup + 1} / {PASOS.length}</p>
    <h2>
      {#if paso === "plataformas"}¿Qué plataformas tienes?
      {:else if paso === "animacion"}¿Te gustan las películas animadas?
      {:else if paso === "idiomas"}¿En qué idiomas te sientes cómodo?
      {:else if paso === "idiomasEvitados"}¿Hay algún idioma que no toleres oír?
      {:else if paso === "generos"}¿Qué géneros no quieres ver nunca?
      {:else if paso === "temas"}¿Qué temas prefieres evitar?
      {:else if paso === "duracion"}¿Cuánto tiempo tienes, normalmente?
      {:else}¿Cómo quieres que te hable?{/if}
    </h2>
    <p class="setup-sub">
      {#if paso === "plataformas"}
        Sirve para saber dónde está lo que te propongo. Puedes no marcar ninguna.
      {:else if paso === "animacion"}
        La animación divide aguas y no es solo cosa de niños. Si la marcas como
        "no", no aparece nunca; si te encanta, sube en el ranking.
      {:else if paso === "idiomas"}
        Esto es sobre leer subtítulos, no sobre rechazar películas. Lo que quede
        fuera baja en el ranking, pero sigue estando.
      {:else if paso === "idiomasEvitados"}
        Esto sí es un veto. Si hay una lengua que no soportas escuchar, nada
        rodado en ella va a aparecer. Puedes no marcar ninguna.
      {:else if paso === "generos"}
        Lo que marques no aparece más. Puedes no marcar ninguno.
      {:else if paso === "temas"}
        Nada de lo que marques va a aparecer. No se muestra, no se sugiere, no
        se explica: simplemente no está.
      {:else if paso === "duracion"}
        Lo más largo baja en el ranking, no desaparece.
      {:else}
        Cambia cómo te escribo. El ranking es el mismo en las tres.
      {/if}
    </p>

    {#if opcionesPaso.length === 0}
      <p class="vacio">
        Esta lista no está disponible fuera de la app. <kbd>Enter</kbd> para
        seguir.
      </p>
    {:else}
      <div
        class="grilla-setup"
        role="listbox"
        aria-label={paso}
        bind:this={refLista}
      >
        {#each opcionesPaso as op, i (op.id)}
          <button
            class="chip"
            class:activa={i === idxSetup}
            class:marcada={estaMarcada(op.id)}
            role="option"
            aria-selected={estaMarcada(op.id)}
            tabindex={i === idxSetup ? 0 : -1}
            data-idx={i}
            onclick={() => {
              idxSetup = i;
              alternarMarca(op.id);
            }}
          >
            <span class="chip-marca">{estaMarcada(op.id) ? "✓" : ""}</span>
            <span class="chip-label">{op.label}</span>
            {#if op.description}<span class="chip-desc">{op.description}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}

    <p class="atajo">
      <kbd>← → ↑ ↓</kbd> mover ·
      {#if pasoEsMultiple}<kbd>Espacio</kbd> marcar ·{/if}
      <kbd>Enter</kbd>
      {pasoSetup + 1 === PASOS.length ? "guardar" : "siguiente"} ·
      <kbd>Esc</kbd> atrás
    </p>
  </section>
{:else if pantalla === "contexto"}
  <section class="pantalla contexto" in:fly={{ y: 12, duration: 320 }}>
    <h2>{v.preguntaContexto}</h2>
    <div
      class="opciones"
      role="radiogroup"
      aria-label="contexto"
      bind:this={refLista}
    >
      {#each contextos as c, i (c.id)}
        <button
          class="opcion"
          class:activa={i === idxContexto}
          aria-checked={i === idxContexto}
          role="radio"
          tabindex={i === idxContexto ? 0 : -1}
          data-idx={i}
          onclick={() => {
            idxContexto = i;
            elegirContextoActual();
          }}
        >
          <span class="icono">{c.icono}</span>
          <span class="label">{c.label}</span>
          {#if c.nota}<span class="nota">{c.nota}</span>{/if}
        </button>
      {/each}
    </div>
    <p class="atajo">
      <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar ·
      <kbd>Esc</kbd> atrás · <kbd>Backspace</kbd> a home
    </p>
  </section>
{:else if pantalla === "intencion"}
  <section class="pantalla intencion" in:fly={{ y: 12, duration: 320 }}>
    <h2>{v.preguntaIntencion}</h2>
    <div
      class="opciones"
      role="radiogroup"
      aria-label="intencion"
      bind:this={refLista}
    >
      {#each INTENCIONES as o, i (o.id)}
        <button
          class="opcion"
          class:activa={i === idxIntencion}
          aria-checked={i === idxIntencion}
          role="radio"
          tabindex={i === idxIntencion ? 0 : -1}
          data-idx={i}
          disabled={cargandoPool}
          onclick={() => {
            idxIntencion = i;
            void elegirIntencionActual();
          }}
        >
          <span class="icono">{o.icono}</span>
          <span class="label">{o.label}</span>
          <span class="nota">{o.desc}</span>
        </button>
      {/each}
    </div>

    {#if cargandoPool}
      <div class="overlay-cargando" in:fade={{ duration: 200 }}>
        <p>{v.cargando}</p>
      </div>
    {:else if errorPool}
      <div class="estado error">
        <p>⚠ {errorPool}</p>
        {#if errorPoolEsKey}
          <p class="error-sub">
            Configura la API key TMDb desde la pantalla principal de Kütral
            o desde <code>/vera/catalog</code>. Después <kbd>Enter</kbd> para
            reintentar.
          </p>
        {:else}
          <p class="error-sub"><kbd>Enter</kbd> para reintentar.</p>
        {/if}
      </div>
    {:else}
      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> elegir · <kbd>Enter</kbd> confirmar ·
        <kbd>Esc</kbd> atrás · <kbd>Backspace</kbd> a home
      </p>
    {/if}
  </section>
{:else if pantalla === "calificar"}
  <section
    class="pantalla calificar"
    bind:this={refCalificar}
    tabindex="-1"
  >
    {#if pelisACalificar.length === 0}
      <p class="vacio">No hay películas para calificar.</p>
    {:else}
      {@const carta = pelisACalificar[idxCalificar]}
      {@const reac = obtenerReaccion(carta.id)}
      {@const visto = reac.juicio !== null}
      {@const valor = visto ? (reac.juicio ?? 0) : reac.interes}

      <p class="contador">
        Carta {idxCalificar + 1} / {pelisACalificar.length}
        <span class="contador-sub">· {v.calibrando}</span>
      </p>

      {#key carta.id}
        <div class="ficha-carta" in:fade={{ duration: 240 }}>
          <div
            class="poster-carta"
            style="background-color:{carta.poster}; background-image:url('{posterUrl(carta, 'w500')}')"
          ></div>
          <div class="info-carta">
            <h2 class="titulo">
              {carta.titulo}
              {#if visto}<span class="badge-visto" title="Ya vista">✓</span>{/if}
            </h2>
            <p class="meta">
              {carta.anio} · {carta.director}
              {#if carta.runtime} · {carta.runtime} min{/if}
              {#if carta.rating}
                <span class="tmdb-rating">
                  ★ {carta.rating.toFixed(1)}
                  {#if carta.votos}<span class="votos">({carta.votos.toLocaleString("es")} votos)</span>{/if}
                </span>
              {/if}
            </p>
            <p class="generos">{carta.generos.join(" · ")}</p>
            <p class="descripcion">{carta.descripcion}</p>
            {#if carta.actores.length}
              <p class="actores">
                <span class="etiqueta">Con</span>
                {carta.actores.slice(0, 4).join(", ")}
              </p>
            {/if}
            {#if carta.imagenes.length}
              <div class="fotogramas">
                {#each carta.imagenes.slice(0, 3) as img (img)}
                  <div
                    class="fotograma"
                    style="background-image:url('{imageUrl(img, 'w300')}')"
                    aria-hidden="true"
                  ></div>
                {/each}
              </div>
            {/if}
            {#if carta.trivia}
              <p class="trivia">💡 {carta.trivia}</p>
            {/if}

            <div class="control-interes">
              <p class="control-etiqueta">
                {visto ? "Tu juicio" : "Tu interés"}:
                <strong>{valor > 0 ? `+${valor}` : valor}</strong>
              </p>
              <div class="escala">
                {#each [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5] as vv (vv)}
                  <span
                    class="celda"
                    class:activa={vv === valor}
                    class:visto
                    class:negativa={vv < 0}
                    class:positiva={vv > 0}
                  >{vv > 0 ? `+${vv}` : vv}</span>
                {/each}
              </div>
              {#if ultimaSaltada}
                <p class="saltada-msg">
                  Saltada · −1 automático. Si quieres que cuente, califica.
                </p>
              {/if}
            </div>
          </div>
        </div>
      {/key}

      <p class="atajo">
        <kbd>←</kbd> <kbd>→</kbd> puntaje · <kbd>↑</kbd> ya la vi ·
        <kbd>↓</kbd> saltar · <kbd>Enter</kbd> siguiente ·
        <kbd>Esc</kbd> atrás · <kbd>Backspace</kbd> a home
      </p>
    {/if}
  </section>
{:else if pantalla === "ranking"}
  <section class="pantalla ranking" bind:this={refRanking} tabindex="-1">
    {#if ranking.length === 0}
      <p class="vacio">No hay películas para ofrecer.</p>
    {:else}
      {@const activa = ranking[idxActiva]}
      {@const peli = activa.pelicula}
      {@const reac = obtenerReaccion(peli.id)}
      {@const visto = reac.juicio !== null}

      <!-- Backdrop -->
      {#if peli.backdropPath}
        {#key peli.id}
          <div
            class="backdrop-ranking"
            style="background-image:url('{backdropUrl(peli, 'w1280')}')"
            aria-hidden="true"
          ></div>
        {/key}
      {/if}

      {#if sinMatch}
        <div class="aviso-nomatch">
          <p class="nomatch-titulo">{v.sinMatch}</p>
          <p class="nomatch-sub">{v.sinMatchSub}</p>
          <p class="nomatch-atajo">
            <kbd>Espacio</kbd> para otra ronda · o baja y elige igual
          </p>
        </div>
      {/if}

      {#if descartadas > 0}
        <p class="aviso-filtrado">
          {descartadas}
          {descartadas === 1 ? "película quedó fuera" : "películas quedaron fuera"}
          por lo que pediste evitar.
        </p>
      {/if}

      <!-- Ficha destacada -->
      {#key peli.id}
        <div class="ficha-activa" in:fade={{ duration: 240 }}>
          <div
            class="poster-activa"
            style="background-color:{peli.poster}; background-image:url('{posterUrl(peli, 'w500')}')"
          ></div>
          <div class="info-activa">
            <h2 class="titulo">
              <span class="rango">#{idxActiva + 1}</span>
              {peli.titulo}
              {#if visto}<span class="badge-visto" title="Ya vista">✓</span>{/if}
            </h2>
            <p class="meta">
              {peli.anio} · {peli.director}
              {#if peli.runtime} · {peli.runtime} min{/if}
              {#if peli.rating}
                <span class="tmdb-rating">
                  ★ {peli.rating.toFixed(1)}
                  {#if peli.votos}<span class="votos">({peli.votos.toLocaleString("es")} votos)</span>{/if}
                </span>
              {/if}
            </p>
            <p class="generos">{peli.generos.join(" · ")}</p>

            <p class="porque">{porQueTexto(activa)}</p>

            <p class="descripcion">{peli.descripcion}</p>
            {#if peli.actores.length}
              <p class="actores">
                <span class="etiqueta">Con</span>
                {peli.actores.slice(0, 4).join(", ")}
              </p>
            {/if}

            {#if peli.plataformas.length}
              <p class="plataformas">
                <span class="etiqueta">En</span>
                {peli.plataformas.join(" · ")}
              </p>
            {/if}

            {#if peli.idiomaOriginal && peli.idiomaOriginal !== "es"}
              <p class="idioma">
                <span class="etiqueta">Idioma original</span>
                {nombreIdioma(peli.idiomaOriginal)}
              </p>
            {/if}

            {#if peli.temasSensibles.length}
              <p class="temas">
                <span class="etiqueta">Contiene</span>
                {peli.temasSensibles.map(nombreTema).join(" · ")}
              </p>
            {/if}

            {#if peli.imagenes.length}
              <div class="fotogramas">
                {#each peli.imagenes.slice(0, 3) as img (img)}
                  <div
                    class="fotograma"
                    style="background-image:url('{imageUrl(img, 'w300')}')"
                    aria-hidden="true"
                  ></div>
                {/each}
              </div>
            {/if}
            {#if peli.trivia}
              <p class="trivia">💡 {peli.trivia}</p>
            {/if}

            {#if peli.enriquecida && !peli.imdbId}
              <p class="aviso-fuente">
                Sin id IMDb: puede que no haya fuentes para reproducirla.
              </p>
            {/if}

            <button
              bind:this={refDescubrir}
              class="btn-grande naranja"
              onclick={descubrir}
            >
              ▶ Descubrir
            </button>
          </div>
        </div>
      {/key}

      <!-- Resto del ranking -->
      {#if ranking.length > 1}
        <div class="resto">
          <p class="resto-titulo">El resto del ranking</p>
          <ul class="resto-lista">
            {#each ranking as r, i (r.pelicula.id)}
              {#if i !== idxActiva}
                {@const recR = obtenerReaccion(r.pelicula.id)}
                <li class:visto={recR.juicio !== null}>
                  <span class="posicion">#{i + 1}</span>
                  <span class="resto-titulo-peli">{r.pelicula.titulo}</span>
                  <span class="resto-anio">{r.pelicula.anio}</span>
                  <span class="resto-rating">★ {r.pelicula.rating.toFixed(1)}</span>
                  {#if recR.juicio !== null}<span class="resto-badge">✓</span>{/if}
                </li>
              {/if}
            {/each}
          </ul>
        </div>
      {/if}

      <p class="atajo ranking-atajo">
        <kbd>↑</kbd> <kbd>↓</kbd> navegar · <kbd>Enter</kbd> descubrir ·
        <kbd>Espacio</kbd> otra ronda · <kbd>Esc</kbd> atrás ·
        <kbd>Backspace</kbd> a home
      </p>
    {/if}
  </section>
{/if}

<style>
  :global(body) {
    margin: 0;
    background: #0a0608;
    color: #fff8e7;
    font-family:
      "Inter",
      system-ui,
      -apple-system,
      sans-serif;
    overflow-x: hidden;
  }

  :global(button:focus-visible),
  :global([role="radio"]:focus-visible),
  :global([role="option"]:focus-visible) {
    outline: 3px solid #ffae5c;
    outline-offset: 4px;
  }
  :global(.ranking:focus-visible),
  :global(.calificar:focus-visible) {
    outline: none;
  }

  /* --- Fondo drift --- */
  .fondo {
    position: fixed;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
  }
  .drift {
    position: absolute;
    width: 160px;
    height: 240px;
    border-radius: 12px;
    top: -40px;
    opacity: 0.16;
    filter: blur(3px);
    background-size: cover;
    background-position: center;
    animation: drift 38s linear infinite;
  }
  .vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(
      ellipse at center,
      transparent 30%,
      rgba(10, 6, 8, 0.92) 90%
    );
  }
  @keyframes drift {
    0% { transform: translateY(-20%); }
    100% { transform: translateY(120vh); }
  }

  /* Respeta la preferencia del sistema: el drift es decorativo y para quien
     pidió menos movimiento es exactamente el tipo de animación que molesta. */
  @media (prefers-reduced-motion: reduce) {
    .drift { animation: none; }
  }

  .pantalla {
    position: relative;
    z-index: 1;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 36px 20px;
    text-align: center;
  }
  .pantalla:focus { outline: none; }

  /* --- Entrada (marca arcoíris) --- */
  .marca {
    font-size: 64px;
    margin: 0 0 8px;
    letter-spacing: -1px;
    line-height: 1.1;
  }
  .marca-vera {
    background: linear-gradient(
      90deg,
      #e40303,
      #ff8c00,
      #ffed00,
      #008026,
      #004dff,
      #750787
    );
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
    font-weight: 700;
  }
  .marca em {
    color: #ffae5c;
    -webkit-text-fill-color: #ffae5c;
    font-style: italic;
    font-weight: 300;
    text-shadow: 0 0 24px rgba(255, 140, 60, 0.4);
  }
  .slogan {
    color: #ffd6a8;
    margin: 0 0 4px;
    font-size: 22px;
    font-style: italic;
    text-shadow: 0 0 16px rgba(255, 140, 60, 0.3);
  }
  .sub {
    color: #d9c0a4;
    margin: 0 0 36px;
    font-size: 16px;
  }
  .entrada-opciones {
    align-items: center;
  }

  /* --- Botones grandes --- */
  .btn-grande {
    font-size: 30px;
    padding: 22px 52px;
    border-radius: 999px;
    border: none;
    cursor: pointer;
    font-weight: 600;
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .btn-grande.naranja {
    background: linear-gradient(135deg, #ff7a2e, #ff4a1c);
    color: #1a0a04;
    box-shadow:
      0 8px 24px rgba(255, 100, 40, 0.45),
      0 0 32px rgba(255, 140, 60, 0.35);
  }
  .btn-grande.naranja:hover,
  .btn-grande.naranja:focus-visible,
  .btn-grande.naranja.activa {
    transform: translateY(-2px);
    box-shadow:
      0 12px 32px rgba(255, 100, 40, 0.55),
      0 0 48px rgba(255, 140, 60, 0.5);
  }
  .btn-secundario {
    background: rgba(50, 22, 12, 0.65);
    border: 1.5px solid rgba(255, 140, 60, 0.35);
    color: #ffe6c8;
    font-size: 20px;
    padding: 16px 32px;
    border-radius: 999px;
    cursor: pointer;
    transition: all 0.18s;
  }
  .btn-secundario:hover,
  .btn-secundario.activa {
    border-color: #ff7a2e;
    background: rgba(80, 32, 14, 0.95);
    transform: translateY(-2px);
  }

  /* --- Contexto + Intención --- */
  .contexto h2,
  .intencion h2,
  .setup h2 {
    font-size: 32px;
    margin: 0 0 12px;
    color: #ffe6c8;
  }
  .contexto h2,
  .intencion h2 {
    margin-bottom: 32px;
  }
  .opciones {
    display: flex;
    gap: 18px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .opcion {
    background: rgba(50, 22, 12, 0.65);
    border: 1.5px solid rgba(255, 140, 60, 0.35);
    color: #ffe6c8;
    padding: 22px 22px;
    border-radius: 18px;
    font-size: 18px;
    min-width: 150px;
    max-width: 180px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    transition: all 0.18s;
    position: relative;
  }
  .opcion .label {
    font-size: 19px;
    text-align: center;
    line-height: 1.2;
  }
  .opcion .nota {
    font-size: 11px;
    color: #ffae5c;
    font-style: italic;
    opacity: 0.85;
    margin-top: 2px;
  }
  .opcion.activa {
    border-color: #ff7a2e;
    background: rgba(80, 32, 14, 0.95);
    transform: translateY(-3px);
    box-shadow: 0 0 32px rgba(255, 140, 60, 0.35);
  }
  .opcion:disabled {
    opacity: 0.5;
    cursor: wait;
    transform: none;
  }
  .icono { font-size: 38px; }

  /* --- Setup --- */
  .setup-sub {
    color: #d9c0a4;
    font-size: 17px;
    margin: 0 0 28px;
    max-width: 640px;
    line-height: 1.5;
  }
  .grilla-setup {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    justify-content: center;
    max-width: 1000px;
  }
  .chip {
    background: rgba(50, 22, 12, 0.55);
    border: 1.5px solid rgba(255, 140, 60, 0.28);
    color: #e6d4bd;
    border-radius: 999px;
    padding: 12px 22px;
    font-size: 17px;
    cursor: pointer;
    display: flex;
    align-items: baseline;
    gap: 8px;
    transition: all 0.15s;
  }
  .chip-marca {
    color: #8ab87a;
    font-weight: 700;
    min-width: 12px;
  }
  .chip-desc {
    font-size: 13px;
    color: #998878;
    font-style: italic;
  }
  .chip.marcada {
    background: rgba(138, 184, 122, 0.18);
    border-color: rgba(138, 184, 122, 0.6);
    color: #ffe6c8;
  }
  .chip.activa {
    border-color: #ff7a2e;
    transform: translateY(-2px);
    box-shadow: 0 0 22px rgba(255, 140, 60, 0.3);
  }

  .overlay-cargando {
    margin-top: 24px;
    color: #ffd6a8;
    font-style: italic;
  }
  .overlay-cargando::after {
    content: "";
    display: inline-block;
    width: 10px;
    height: 10px;
    margin-left: 10px;
    border: 2px solid #ffae5c;
    border-radius: 50%;
    border-right-color: transparent;
    animation: vera-spin 0.8s linear infinite;
    vertical-align: middle;
  }
  @keyframes vera-spin {
    to { transform: rotate(360deg); }
  }

  .estado {
    margin-top: 16px;
    color: #d9c0a4;
    font-size: 17px;
  }
  .estado.error {
    color: #ffae5c;
    max-width: 520px;
  }
  .error-sub {
    color: #d9c0a4;
    font-size: 14px;
    margin-top: 8px;
  }
  .error-sub code {
    background: rgba(255, 140, 60, 0.18);
    padding: 1px 6px;
    border-radius: 4px;
  }

  /* --- Calificar: 1 carta a la vez con ficha completa --- */
  .calificar,
  .ranking,
  .setup {
    justify-content: flex-start;
    padding: 24px 20px 60px;
  }
  .vacio {
    color: #ffd6a8;
    font-size: 18px;
    margin-top: 60px;
  }
  .contador {
    color: #d9c0a4;
    font-size: 20px;
    margin: 0 0 20px;
    letter-spacing: 1px;
  }
  .contador-sub {
    color: #998878;
    font-size: 16px;
    font-style: italic;
    letter-spacing: 0;
  }
  .ficha-carta,
  .ficha-activa {
    display: flex;
    gap: 48px;
    max-width: 1500px;
    width: 100%;
    margin: 0 0 28px;
    text-align: left;
    position: relative;
    z-index: 2;
  }
  .poster-carta,
  .poster-activa {
    width: 380px;
    min-width: 380px;
    height: 570px;
    border-radius: 22px;
    background-size: cover;
    background-position: center;
    box-shadow:
      0 24px 56px rgba(0, 0, 0, 0.7),
      0 0 64px rgba(255, 100, 40, 0.25);
  }
  .info-carta,
  .info-activa {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 14px;
    color: #ffe6c8;
  }
  .titulo {
    font-size: 48px;
    margin: 0;
    color: #ffe6c8;
    line-height: 1.1;
  }
  .rango {
    color: #ff7a2e;
    font-weight: 700;
    margin-right: 12px;
  }
  .badge-visto {
    color: #8ab87a;
    margin-left: 12px;
    font-size: 36px;
  }
  .meta {
    font-size: 24px;
    color: #d9c0a4;
    margin: 0;
    letter-spacing: 0.3px;
  }
  .tmdb-rating {
    background: rgba(255, 170, 80, 0.18);
    border: 1px solid rgba(255, 170, 80, 0.55);
    color: #ffd66a;
    padding: 4px 16px;
    border-radius: 999px;
    font-weight: 600;
    margin-left: 12px;
    font-size: 22px;
  }
  .votos {
    color: #b09a80;
    font-weight: 400;
    font-size: 17px;
  }
  .generos {
    font-size: 20px;
    color: #ffae5c;
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.8px;
  }
  .porque {
    font-size: 22px;
    color: #ffd6a8;
    margin: 0;
    font-style: italic;
  }
  .descripcion {
    font-size: 24px;
    line-height: 1.5;
    color: #e6d4bd;
    margin: 8px 0 0;
    max-height: 7em;
    overflow: hidden;
  }
  .actores,
  .plataformas,
  .temas,
  .idioma {
    font-size: 22px;
    color: #d9c0a4;
    margin: 0;
  }
  .temas {
    color: #d9a0a0;
    font-size: 19px;
  }
  .idioma {
    color: #d9c0a4;
    font-size: 19px;
    margin: 0;
  }
  .plataformas {
    color: #a8c8d9;
    font-size: 19px;
    text-transform: capitalize;
  }
  .etiqueta {
    color: #998878;
    margin-right: 6px;
  }
  .fotogramas {
    display: flex;
    gap: 14px;
    margin: 8px 0 0;
  }
  .fotograma {
    flex: 1 1 0;
    aspect-ratio: 16 / 9;
    border-radius: 12px;
    background-size: cover;
    background-position: center;
    background-color: #1f1410;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
  }
  .trivia {
    font-size: 22px;
    color: #ffd6a8;
    background: rgba(255, 140, 60, 0.12);
    padding: 14px 20px;
    border-radius: 14px;
    border-left: 5px solid #ff7a2e;
    margin: 10px 0 0;
    font-style: italic;
  }
  .aviso-fuente {
    font-size: 17px;
    color: #d9a0a0;
    margin: 0;
  }

  /* --- Control de interés (calificar) --- */
  .control-interes {
    margin-top: 22px;
    padding: 20px 26px;
    background: rgba(50, 22, 12, 0.6);
    border-radius: 16px;
    border: 1px solid rgba(255, 140, 60, 0.3);
  }
  .control-etiqueta {
    font-size: 22px;
    color: #d9c0a4;
    margin: 0 0 14px;
    letter-spacing: 0.3px;
  }
  .control-etiqueta strong {
    color: #ffe6c8;
    font-size: 30px;
    margin-left: 6px;
  }
  .escala {
    display: flex;
    gap: 8px;
    justify-content: space-between;
  }
  .celda {
    flex: 1;
    text-align: center;
    padding: 14px 0;
    border-radius: 10px;
    font-size: 22px;
    color: #998878;
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid transparent;
    font-variant-numeric: tabular-nums;
    transition: all 0.12s;
  }
  .celda.negativa { color: rgba(255, 120, 120, 0.55); }
  .celda.positiva { color: rgba(180, 220, 140, 0.65); }
  .celda.activa {
    background: rgba(255, 140, 60, 0.25);
    border-color: #ff7a2e;
    color: #ffe6c8;
    font-weight: 700;
    transform: translateY(-1px);
  }
  .celda.activa.visto {
    background: rgba(138, 184, 122, 0.25);
    border-color: #8ab87a;
  }
  .saltada-msg {
    margin: 8px 0 0;
    color: #ffae5c;
    font-size: 12px;
    font-style: italic;
  }

  /* --- Ranking --- */
  .backdrop-ranking {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center top;
    opacity: 0.28;
    filter: blur(2px);
    z-index: 0;
    pointer-events: none;
  }
  .backdrop-ranking::after {
    content: "";
    position: absolute;
    inset: 0;
    background: radial-gradient(
      ellipse at center,
      transparent 20%,
      rgba(10, 6, 8, 0.85) 75%
    );
    pointer-events: none;
  }

  .aviso-nomatch {
    position: relative;
    z-index: 2;
    max-width: 1500px;
    width: 100%;
    margin: 0 0 20px;
    padding: 18px 24px;
    text-align: left;
    border-radius: 16px;
    background: rgba(90, 40, 18, 0.55);
    border: 1px solid rgba(255, 140, 60, 0.45);
  }
  .nomatch-titulo {
    margin: 0;
    font-size: 26px;
    color: #ffe6c8;
  }
  .nomatch-sub {
    margin: 6px 0 0;
    font-size: 19px;
    color: #d9c0a4;
  }
  .nomatch-atajo {
    margin: 10px 0 0;
    font-size: 16px;
    color: #998878;
  }
  .aviso-filtrado {
    position: relative;
    z-index: 2;
    max-width: 1500px;
    width: 100%;
    margin: 0 0 16px;
    text-align: left;
    color: #998878;
    font-size: 17px;
  }

  .resto {
    max-width: 1500px;
    width: 100%;
    margin: 12px 0 24px;
    position: relative;
    z-index: 2;
  }
  .resto-titulo {
    color: #998878;
    font-size: 20px;
    margin: 0 0 12px;
    text-align: left;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }
  .resto-lista {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .resto-lista li {
    display: grid;
    grid-template-columns: 52px 1fr 110px 110px 36px;
    gap: 18px;
    align-items: center;
    padding: 14px 20px;
    border-radius: 12px;
    background: rgba(50, 22, 12, 0.4);
    border: 1px solid rgba(255, 140, 60, 0.1);
    color: #d9c0a4;
    font-size: 22px;
    text-align: left;
  }
  .resto-lista li.visto {
    opacity: 0.65;
  }
  .posicion {
    color: #ffae5c;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .resto-titulo-peli {
    color: #ffe6c8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .resto-anio,
  .resto-rating {
    color: #998878;
    font-size: 19px;
    text-align: right;
  }
  .resto-badge {
    color: #8ab87a;
    font-size: 24px;
    text-align: center;
  }
  .ranking-atajo {
    position: relative;
    z-index: 2;
    margin-top: 16px;
  }

  /* --- Atajos hint --- */
  .atajo {
    color: #998878;
    font-size: 19px;
    margin-top: 26px;
    letter-spacing: 0.5px;
  }
  :global(kbd) {
    background: rgba(255, 140, 60, 0.18);
    border: 1px solid rgba(255, 140, 60, 0.4);
    border-radius: 8px;
    padding: 4px 12px;
    font-family: inherit;
    font-size: 19px;
    color: #ffe6c8;
    margin: 0 3px;
  }
</style>
