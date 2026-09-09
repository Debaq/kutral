<script lang="ts">
  // Juegos — catálogo completo por sistema (carátulas de libretro-thumbnails),
  // marca lo que no tienes y al hacer clic baja el ROM (Myrient).
  //
  // Nav: ← → ↑ ↓ / d-pad mueve foco · Enter / A abre (Descubre si la tienes,
  //   descarga si no) · LB/RB cambia sistema · Esc/Backspace/B salen a home.
  //
  // Estilo Blockbuster: pasillos por género (filas horizontales). Estrenos y
  // conocidos (franquicia) van de frente; el resto, de lomo (spine).

  import { onMount, onDestroy, tick, untrack } from "svelte";
  import { navegar } from "$lib/nav";
  import { goto } from "$app/navigation";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { config, nameInRegions } from "$lib/config.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";

  type Sinopsis = { extract?: string; url?: string; lang?: string };

  type System = "nes" | "snes" | "gba" | "gbc" | "ds";

  type Juego = {
    id: string;
    name: string; // variante elegida: nombre No-Intro (carátula + .zip)
    display: string; // título base sin tags, para mostrar
    system: System;
  };

  const SISTEMAS: { key: System; label: string; color: string }[] = [
    { key: "nes", label: "NES", color: "#e60012" },
    { key: "snes", label: "SNES", color: "#7d4fff" },
    { key: "gba", label: "GBA", color: "#d4308a" },
    { key: "gbc", label: "GB Color", color: "#00b06b" },
    { key: "ds", label: "DS", color: "#2b6cff" },
  ];

  // Repos de libretro-thumbnails por sistema (carátulas oficiales).
  const REPO: Record<System, string> = {
    nes: "Nintendo_-_Nintendo_Entertainment_System",
    snes: "Nintendo_-_Super_Nintendo_Entertainment_System",
    gba: "Nintendo_-_Game_Boy_Advance",
    gbc: "Nintendo_-_Game_Boy_Color",
    ds: "Nintendo_-_Nintendo_DS",
  };

  // libretro reemplaza estos chars por "_" en el nombre del archivo.
  function cleanName(title: string): string {
    return title.replace(/[&*/:`<>?\\|]/g, "_");
  }
  function thumbUrl(sys: System, name: string): string {
    const repo = REPO[sys];
    const file = encodeURIComponent(cleanName(name));
    return `https://raw.githubusercontent.com/libretro-thumbnails/${repo}/master/Named_Boxarts/${file}.png`;
  }

  function colorDe(sys: System): string {
    return SISTEMAS.find((s) => s.key === sys)?.color ?? "#888";
  }

  // --- Estado ---
  type Meta = {
    genre?: string; year?: string; franchise?: boolean; esrb?: string;
    developer?: string; publisher?: string; players?: string; month?: string; edge?: string;
  };
  let filtro = $state<System>("nes");
  let catalogo = $state<Record<string, string[]>>({});
  let meta = $state<Record<string, Meta>>({}); // metadata del sistema actual
  let owned = $state<Set<string>>(new Set());
  // Clave de emparejamiento robusta: sin tags, minúsculas, solo alfanumérico.
  // Salva diferencias de puntuación/caso entre el ROM ("Batman & Robin") y el
  // catálogo libretro ("Batman _ Robin", "Acro-Bat" vs "Acro-bat", etc.).
  function matchKey(name: string): string {
    return baseTitle(name).toLowerCase().replace(/[^a-z0-9]+/g, "");
  }
  let ownedKeys = $derived(new Set([...owned].map(matchKey)));
  function tieneJuego(name: string): boolean {
    return ownedKeys.has(matchKey(name));
  }
  let cargando = $state(false);
  let errorMsg = $state("");

  // Navegación 2D por pasillos (fila/columna).
  let fRow = $state(0);
  let fCol = $state(0);
  let cardRefs = $state<Record<string, HTMLButtonElement>>({});

  // Carátulas resueltas: name → src local (asset://).
  let boxarts = $state<Record<string, string>>({});
  // Descargas en curso: name → {received,total}.
  let bajando = $state<Record<string, { received: number; total: number }>>({});

  // Filtros extra (estilo pelis). Por defecto: solo las que tienes (menos
  // fatiga). El toggle revela el catálogo completo para explorar/coleccionar.
  let busca = $state("");
  let soloTengo = $state(true);
  // Filtros nuevos: género (pasillo) y jugado/no jugado. "Todos" = sin filtro.
  let generoSel = $state<string>("Todos");
  let jugadoSel = $state<"todos" | "si" | "no">("todos");

  // Vista: biblioteca (grid) o colección (stats completista).
  let vista = $state<"biblioteca" | "coleccion">("biblioteca");

  // Jugados: por sistema, set de matchKeys. Persiste en localStorage.
  let jugados = $state<Record<string, Set<string>>>({});
  // Owned de TODOS los sistemas (para la vista Colección).
  let ownedAll = $state<Record<string, Set<string>>>({});

  // Prefetch lento de carátulas: matchKeys ya cacheadas en disco (resumible).
  let artDone = $state<Set<string>>(new Set());
  let prefTotal = $state(0);
  let prefCorriendo = $state(false);
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  function cargarJugados() {
    try {
      const raw = JSON.parse(localStorage.getItem("emu_played") || "{}");
      const out: Record<string, Set<string>> = {};
      for (const k of Object.keys(raw)) out[k] = new Set(raw[k]);
      jugados = out;
    } catch {
      jugados = {};
    }
  }
  function marcarJugado(sys: System, name: string) {
    const key = matchKey(name);
    const set = new Set(jugados[sys] ?? []);
    set.add(key);
    jugados = { ...jugados, [sys]: set };
    const plain: Record<string, string[]> = {};
    for (const k of Object.keys(jugados)) plain[k] = [...jugados[k]];
    try {
      localStorage.setItem("emu_played", JSON.stringify(plain));
    } catch {}
  }

  // Modal de detalle del juego seleccionado.
  let detalle = $state<Juego | null>(null);
  let modalSel = $state(0); // 0 = Descubrir, 1 = Descargar
  // Sinopsis (Wikipedia) por nombre, on-demand al abrir la ficha.
  let sinopsis = $state<Record<string, Sinopsis>>({});
  let cargandoSyn = $state(false);
  async function loadSinopsis(name: string) {
    if (sinopsis[name]) return;
    cargandoSyn = true;
    try {
      const s = await invoke<Sinopsis>("emu_synopsis", { name });
      sinopsis = { ...sinopsis, [name]: s ?? {} };
    } catch {
      sinopsis = { ...sinopsis, [name]: {} };
    } finally {
      cargandoSyn = false;
    }
  }

  // Título base: nombre sin los (tags) ni [tags] → "1942 (USA) (Rev 1)" → "1942".
  function baseTitle(name: string): string {
    return name
      .replace(/\s*\([^)]*\)/g, "")
      .replace(/\s*\[[^\]]*\]/g, "")
      .trim();
  }

  // Tags que indican variante de baja prioridad (no la versión "buena").
  const MALOS = [
    "(Beta", "(Proto", "(Demo", "(Sample", "(Promo",
    "(Pirate", "(Unl", "(Aftermarket", "(Hack", "(Test",
  ];

  // Etiquetas de idioma por subsLang (config), para preferir esa pista al elegir
  // variante. (Futuro: usar el idioma de subtítulos seleccionado.)
  const LANG_TAG: Record<string, string[]> = {
    es: ["Spain", "(Es", ",Es"],
    en: ["(En", ",En", "USA", "World"],
    pt: ["Brazil", "Portugal", "(Pt"],
    fr: ["France", "(Fr"],
    it: ["Italy", "(It"],
    de: ["Germany", "(De"],
    ja: ["Japan", "(Ja"],
  };

  // Preferencia de región/idioma al deduplicar (menor = mejor):
  // español → europea → idioma de subtítulos → inglés (USA/World) → resto → Japón.
  function prefRank(name: string): number {
    if (name.includes("(Spain") || name.includes("Spain)") || /[(,]Es[,)]/.test(name)) return 0;
    if (name.includes("Europe")) return 1;
    const lt = LANG_TAG[config.subsLang];
    if (lt && lt.some((t) => name.includes(t))) return 2;
    if (name.includes("USA") || name.includes("World")) return 3;
    if (name.includes("Japan")) return 5;
    return 4;
  }

  // Menor puntaje = mejor variante.
  function variantScore(name: string): number {
    let s = 0;
    if (MALOS.some((m) => name.includes(m))) s += 1000;
    s += prefRank(name) * 10;
    const m = name.match(/Rev (\d+)/); // revisión más alta = mejor
    s -= m ? parseInt(m[1], 10) : 0;
    s += name.length * 0.001; // desempate: nombre más corto
    return s;
  }

  // Catálogo filtrado por región + deduplicado por título (1 card por juego).
  // Orden: las que tengo primero, luego alfabético.
  let baseLista = $derived.by(() => {
    const names = (catalogo[filtro] ?? []).filter((n) =>
      nameInRegions(n, config.gameRegions),
    );
    const grupos = new Map<string, string>(); // base → mejor variante
    for (const n of names) {
      const base = baseTitle(n);
      const actual = grupos.get(base);
      if (!actual || variantScore(n) < variantScore(actual)) {
        grupos.set(base, n);
      }
    }
    const q = busca.trim().toLowerCase();
    return [...grupos.entries()]
      .map(([display, name]) => ({
        id: `${filtro}:${name}`,
        name,
        display,
        system: filtro,
      }))
      .filter((g) => !q || g.display.toLowerCase().includes(q))
      .filter((g) => !soloTengo || tieneJuego(g.name))
      .sort((a, b) => {
        const ta = tieneJuego(a.name) ? 0 : 1;
        const tb = tieneJuego(b.name) ? 0 : 1;
        if (ta !== tb) return ta - tb; // tenidas primero
        return a.display.localeCompare(b.display);
      }) as Juego[];
  });

  // Pasillo (género) de un juego, considerando "Familia" por ESRB.
  function generoDe(name: string): string {
    return esFamilia(name) ? "Familia" : pasilloDe(name);
  }
  // ¿Lo marqué como jugado? (set de matchKeys por sistema)
  function fueJugado(name: string): boolean {
    return (jugados[filtro] ?? new Set()).has(matchKey(name));
  }

  // Géneros presentes en el catálogo actual (para los chips), en orden fijo.
  let generosDisponibles = $derived.by(() => {
    const set = new Set(baseLista.map((g) => generoDe(g.name)));
    return ORDEN_PASILLOS.filter((p) => set.has(p));
  });

  // Lista final = base + filtros género/jugado.
  let lista = $derived.by(() => {
    return baseLista
      .filter((g) => generoSel === "Todos" || generoDe(g.name) === generoSel)
      .filter((g) => {
        if (jugadoSel === "si") return fueJugado(g.name);
        if (jugadoSel === "no") return !fueJugado(g.name);
        return true;
      });
  });

  // --- Metadata por matchKey (el catálogo usa "_" donde la DB usa "&", etc.) ---
  let metaByKey = $derived.by(() => {
    const out: Record<string, Meta> = {};
    for (const [name, m] of Object.entries(meta)) out[matchKey(name)] = m;
    return out;
  });
  function metaDe(name: string): Meta {
    return metaByKey[matchKey(name)] ?? {};
  }
  function edgeDe(name: string): number {
    return parseInt(metaDe(name).edge ?? "0", 10) || 0;
  }
  // Veredicto geek: ¿clásico o del montón? Combina nota Edge, franquicia y fama.
  function veredicto(name: string): { txt: string; tier: number } {
    const e = edgeDe(name);
    const fr = !!metaDe(name).franchise;
    const fm = fama(name) !== Infinity;
    if (e >= 9) return { txt: "Obra maestra", tier: 4 };
    if (e >= 8 || (fr && fm)) return { txt: "Imprescindible", tier: 3 };
    if (fr || fm) return { txt: "Clásico conocido", tier: 2 };
    if (e >= 6) return { txt: "Recomendado", tier: 1 };
    if (e > 0 && e <= 4) return { txt: "Del montón", tier: 0 };
    return { txt: "A descubrir", tier: 1 };
  }
  const ESRB_FAMILIA = ["E", "EC", "E10+", "K-A", "KA"];
  function esFamilia(name: string): boolean {
    const r = (metaDe(name).esrb ?? "").toUpperCase();
    return ESRB_FAMILIA.includes(r);
  }

  // Género libretro → pasillo en español.
  function pasilloDe(name: string): string {
    const g = (metaDe(name).genre ?? "").toLowerCase();
    if (!g) return "Otros";
    if (g.includes("platform")) return "Plataformas";
    if (g.includes("sport")) return "Deportes";
    if (g.includes("rac") || g.includes("driv")) return "Carreras";
    if (g.includes("fight") || g.includes("beat")) return "Lucha";
    if (g.includes("shoot")) return "Disparos";
    if (g.includes("role") || g.includes("rpg")) return "RPG";
    if (g.includes("puzzle") || g.includes("board") || g.includes("card")) return "Puzzle";
    if (g.includes("adventure")) return "Aventura";
    if (g.includes("strateg") || g.includes("simul")) return "Estrategia";
    if (g.includes("action")) return "Acción";
    return "Otros";
  }

  const ORDEN_PASILLOS = [
    "Acción", "Plataformas", "RPG", "Aventura", "Deportes", "Carreras",
    "Lucha", "Disparos", "Puzzle", "Estrategia", "Familia", "Otros",
  ];

  // Top de la historia: títulos más icónicos/vendidos, ordenados por fama.
  // Match por substring del título base (en minúsculas).
  const FAMOSOS = [
    "super mario", "mario kart", "zelda", "donkey kong", "yoshi", "pok",
    "super metroid", "metroid", "kirby", "star fox", "f-zero", "final fantasy",
    "chrono trigger", "secret of mana", "earthbound", "super punch", "punch-out",
    "street fighter", "mortal kombat", "killer instinct", "mega man", "megaman",
    "castlevania", "contra", "double dragon", "tetris", "excitebike",
    "kid icarus", "pilotwings", "super smash", "goldeneye", "wave race",
    "banjo", "conker", "harvest moon", "actraiser", "super castlevania",
  ];
  function fama(name: string): number {
    const t = name.toLowerCase();
    const i = FAMOSOS.findIndex((k) => t.includes(k));
    return i === -1 ? Infinity : i;
  }

  // Columnas que caben a lo ancho (carta ~150px + gap).
  let cols = $state(8);

  type Item = { juego: Juego; face: boolean };
  type Pasillo = { label: string; kind: "top" | "genero"; items: Item[] };

  // Blockbuster: arriba 2 filas con el TOP de la historia (de frente, solo lo
  // que cabe a lo ancho, sin scroll). Abajo, por categoría, todo de lomo —
  // sin repetir lo que ya está arriba.
  let pasillos = $derived.by(() => {
    const items = lista;
    const byDisp = (a: Juego, b: Juego) => a.display.localeCompare(b.display);
    const out: Pasillo[] = [];

    // TOP: famosos de verdad, ordenados por fama, máximo 2 filas a lo ancho.
    const top = [...items]
      .filter((g) => fama(g.name) !== Infinity)
      .sort((a, b) => fama(a.name) - fama(b.name) || byDisp(a, b))
      .slice(0, cols * 2);
    const enTop = new Set(top.map((g) => g.id));
    if (top.length) {
      const r1 = top.slice(0, cols);
      const r2 = top.slice(cols, cols * 2);
      out.push({ label: "★ Top de la historia", kind: "top", items: r1.map((j) => ({ juego: j, face: true })) });
      if (r2.length)
        out.push({ label: "", kind: "top", items: r2.map((j) => ({ juego: j, face: true })) });
    }

    // Resto, agrupado por categoría, todo de lomo.
    const grupos = new Map<string, Juego[]>();
    for (const g of items) {
      if (enTop.has(g.id)) continue;
      const p = esFamilia(g.name) ? "Familia" : pasilloDe(g.name);
      if (!grupos.has(p)) grupos.set(p, []);
      grupos.get(p)!.push(g);
    }
    // Categorías con muy pocos juegos se juntan en una fila "Varios".
    const MIN = 5;
    const varios: Juego[] = [];
    for (const label of ORDEN_PASILLOS) {
      const gs = grupos.get(label);
      if (!gs || !gs.length) continue;
      // Familia siempre va sola; el resto chico se junta en Varios.
      if (label !== "Familia" && (label === "Otros" || gs.length < MIN)) {
        varios.push(...gs);
        continue;
      }
      out.push({
        label,
        kind: "genero",
        items: gs.sort(byDisp).map((j) => ({ juego: j, face: false })),
      });
    }
    if (varios.length) {
      out.push({
        label: "Varios",
        kind: "genero",
        items: varios.sort(byDisp).map((j) => ({ juego: j, face: false })),
      });
    }
    return out;
  });

  // --- Carga de catálogo + metadata + biblioteca al cambiar de sistema ---
  async function cargarSistema(sys: System) {
    cargando = !catalogo[sys];
    errorMsg = "";
    fRow = 0;
    fCol = 0;
    generoSel = "Todos";
    jugadoSel = "todos";
    try {
      const [cat, own, md] = await Promise.all([
        invoke<string[]>("emu_catalog", { system: sys }),
        invoke<string[]>("emu_owned", { system: sys }),
        invoke<Record<string, Meta>>("emu_metadata", { system: sys }).catch(() => ({})),
      ]);
      catalogo = { ...catalogo, [sys]: cat };
      owned = new Set(own);
      meta = md;
    } catch (e) {
      errorMsg = String(e);
    } finally {
      cargando = false;
      await tick();
      if (document.activeElement?.tagName !== "INPUT") cardRefs["0:0"]?.focus();
    }
  }

  // Solo al cambiar de sistema. untrack: cargarSistema lee/escribe `catalogo`,
  // sin esto el effect se auto-dispararía en bucle (y robaría el foco).
  $effect(() => {
    const sys = filtro;
    untrack(() => void cargarSistema(sys));
  });

  // Al entrar a Colección, cargar catálogo+biblioteca de todos los sistemas.
  $effect(() => {
    if (vista === "coleccion") untrack(() => void cargarColeccion());
  });

  // Títulos PAL renombrados respecto al boxart No-Intro (libretro). El nombre
  // europeo no existe como carátula; mapeamos al título USA equivalente.
  const PAL_ALIAS: Record<string, string> = {
    "Star Wing": "Star Fox",
    Starwing: "Star Fox",
    "Lylat Wars": "Star Fox 64",
    "The 7th Saga": "The 7th Saga",
    "Terranigma": "Terranigma",
    "Mega Man": "Mega Man",
    Rockman: "Mega Man",
  };
  const BOX_REGIONS = ["(USA)", "(World)", "(Europe)", "(Japan)"];

  // Candidatos de URL de carátula: variante elegida → sin tags de revisión →
  // mismo título base con otras regiones → alias PAL→USA. Cubre los casos en
  // que la variante por región no tiene boxart pero otra sí.
  function boxartCandidates(sys: System, name: string): string[] {
    const urls: string[] = [];
    const push = (n: string) => {
      const u = thumbUrl(sys, n);
      if (n && !urls.includes(u)) urls.push(u);
    };
    push(name);
    const limpio = name.replace(/\s*\((Rev|Beta|Proto|Demo|Sample)[^)]*\)/g, "").trim();
    if (limpio !== name) push(limpio);
    const bases = [baseTitle(name)];
    const alias = PAL_ALIAS[baseTitle(name)];
    if (alias && alias !== baseTitle(name)) bases.push(alias);
    for (const b of bases) {
      for (const r of BOX_REGIONS) push(`${b} ${r}`);
      push(b);
    }
    return urls;
  }

  // --- Carátulas (lazy) ---
  async function loadBoxart(g: Juego) {
    if (boxarts[g.name]) return;
    for (const url of boxartCandidates(g.system, g.name)) {
      try {
        const local = await invoke<string>("cache_image", { url, maxW: 512 });
        boxarts = { ...boxarts, [g.name]: convertFileSrc(local) };
        return;
      } catch {
        // Candidato 404 / sin red → probar el siguiente.
      }
    }
    // Ninguno resolvió → placeholder de color.
  }

  function attachThumb(node: HTMLElement, g: Juego) {
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          void loadBoxart(g);
          io.disconnect();
        }
      },
      { rootMargin: "300px" },
    );
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  // --- Prefetch lento de carátulas (acumula offline, resumible) ---
  function cargarArtDone() {
    try {
      artDone = new Set(JSON.parse(localStorage.getItem("emu_art_done") || "[]"));
    } catch {
      artDone = new Set();
    }
  }
  function guardarArtDone() {
    try {
      localStorage.setItem("emu_art_done", JSON.stringify([...artDone]));
    } catch {}
  }

  // Recorre TUS juegos (todos los sistemas) y baja+cachea la carátula, una por
  // una y despacio, saltando las ya hechas. Se reanuda solo entre sesiones.
  async function prefetchArte() {
    if (prefCorriendo) return;
    prefCorriendo = true;
    try {
      const tareas: { sys: System; name: string; k: string }[] = [];
      const vistos = new Set<string>();
      for (const s of SISTEMAS) {
        let cat = catalogo[s.key];
        if (!cat) {
          cat = await invoke<string[]>("emu_catalog", { system: s.key });
          catalogo = { ...catalogo, [s.key]: cat };
        }
        let own = ownedAll[s.key];
        if (!own) {
          const o = await invoke<string[]>("emu_owned", { system: s.key });
          own = new Set(o.map(matchKey));
          ownedAll = { ...ownedAll, [s.key]: own };
        }
        for (const name of cat) {
          const k = matchKey(name);
          if (!own.has(k) || vistos.has(k)) continue;
          vistos.add(k);
          tareas.push({ sys: s.key, name, k });
        }
      }
      prefTotal = tareas.length;
      for (const t of tareas) {
        if (artDone.has(t.k)) continue;
        try {
          await invoke<string>("cache_image", { url: thumbUrl(t.sys, t.name), maxW: 512 });
        } catch {
          // sin carátula / sin red → no la marco, se reintenta otro día
          await sleep(120);
          continue;
        }
        artDone = new Set(artDone).add(t.k);
        guardarArtDone();
        await sleep(180); // despacito, sin apurar al servidor
      }
    } finally {
      prefCorriendo = false;
    }
  }

  // --- Acciones ---
  // Clic en card → abre el modal de detalle (info + botones).
  function abrirJuego(g: Juego) {
    detalle = g;
    modalSel = tieneJuego(g.name) ? 0 : 1;
    void loadBoxart(g);
    void loadSinopsis(g.name);
  }
  function cerrarModal() {
    detalle = null;
  }

  async function descubrir(g: Juego) {
    try {
      await invoke("emu_play", { system: g.system, rom: `${g.name}.zip` });
      marcarJugado(g.system, g.name); // cuenta para la colección
    } catch (e) {
      errorMsg = String(e);
    }
  }

  // --- Colección: cargar catálogo + biblioteca de TODOS los sistemas ---
  let cargandoColeccion = $state(false);
  async function cargarColeccion() {
    cargandoColeccion = true;
    try {
      const cats: Record<string, string[]> = { ...catalogo };
      const owns: Record<string, Set<string>> = {};
      await Promise.all(
        SISTEMAS.map(async (s) => {
          const [cat, own] = await Promise.all([
            invoke<string[]>("emu_catalog", { system: s.key }),
            invoke<string[]>("emu_owned", { system: s.key }),
          ]);
          cats[s.key] = cat;
          owns[s.key] = new Set(own.map(matchKey));
        }),
      );
      catalogo = cats;
      ownedAll = owns;
    } catch (e) {
      errorMsg = String(e);
    } finally {
      cargandoColeccion = false;
    }
  }

  // Stats por sistema (universo deduplicado por matchKey).
  let stats = $derived.by(() => {
    return SISTEMAS.map((s) => {
      const total = new Set((catalogo[s.key] ?? []).map(matchKey));
      const own = ownedAll[s.key] ?? new Set<string>();
      const tienes = [...own].filter((k) => total.has(k)).length;
      const jug = jugados[s.key] ?? new Set<string>();
      const jugadosN = [...jug].filter((k) => total.has(k)).length;
      return {
        ...s,
        total: total.size,
        tienes,
        faltan: Math.max(0, total.size - tienes),
        jugados: jugadosN,
        pct: total.size ? Math.round((tienes / total.size) * 100) : 0,
      };
    });
  });
  let totalGlobal = $derived(stats.reduce((a, s) => a + s.total, 0));
  let tienesGlobal = $derived(stats.reduce((a, s) => a + s.tienes, 0));
  let jugadosGlobal = $derived(stats.reduce((a, s) => a + s.jugados, 0));

  async function descargar(g: Juego) {
    if (bajando[g.name]) return;
    bajando = { ...bajando, [g.name]: { received: 0, total: 0 } };
    try {
      await invoke("emu_download", { system: g.system, name: g.name });
      // El evento emu_download_done confirma; por si acaso marcamos tenida.
      owned = new Set(owned).add(g.name);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      const { [g.name]: _drop, ...rest } = bajando;
      bajando = rest;
    }
  }

  // Activa el botón seleccionado del modal.
  function accionModal() {
    if (!detalle) return;
    const tengo = tieneJuego(detalle.name);
    if (modalSel === 0 && tengo) void descubrir(detalle);
    else if (modalSel === 1 && !tengo && !bajando[detalle.name]) void descargar(detalle);
  }

  // --- Navegación 2D por pasillos ---
  function clamp(v: number, max: number): number {
    return Math.max(0, Math.min(v, max));
  }
  async function focar() {
    await tick();
    const el = cardRefs[`${fRow}:${fCol}`];
    el?.scrollIntoView({ block: "nearest", inline: "center" });
    el?.focus();
    // El lomo enfocado se da vuelta y muestra la carátula → bajarla.
    const g = juegoFocado();
    if (g) void loadBoxart(g);
  }
  function juegoFocado(): Juego | null {
    return pasillos[fRow]?.items[fCol]?.juego ?? null;
  }
  function moverH(d: number) {
    const row = pasillos[fRow];
    if (!row) return;
    fCol = clamp(fCol + d, row.items.length - 1);
    void focar();
  }
  function moverV(d: number) {
    if (!pasillos.length) return;
    fRow = clamp(fRow + d, pasillos.length - 1);
    fCol = clamp(fCol, (pasillos[fRow]?.items.length ?? 1) - 1);
    void focar();
  }

  /** ¿El foco está en una carátula/lomo del grid? Ahí manda fRow/fCol. */
  function enGrid(): boolean {
    const el = document.activeElement as HTMLElement | null;
    return !!el?.matches?.(".spine, .card:not(.nav-card)");
  }

  /** Sale del grid hacia arriba: primero "Otras secciones", si no, la cabecera. */
  function salirArriba() {
    const el =
      document.querySelector<HTMLElement>('[data-section="secciones"] [data-nav]') ??
      document.querySelector<HTMLElement>('[data-section="top"] [data-nav]');
    el?.focus();
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  /** Entra al grid desde arriba, en la última carátula que estuvo enfocada. */
  function entrarAlGrid() {
    if (!pasillos.length) return false;
    void focar();
    return true;
  }

  const ORDEN: System[] = SISTEMAS.map((s) => s.key);
  function ciclarSistema(dir: number) {
    const i = ORDEN.indexOf(filtro);
    filtro = ORDEN[(i + dir + ORDEN.length) % ORDEN.length];
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;

    // En el buscador: dejar escribir; Escape lo cierra/desenfoca.
    const tgt = e.target as HTMLElement | null;
    if (tgt?.tagName === "INPUT") {
      // Sliders de la barra superior: las flechas son del control.
      if ((tgt as HTMLInputElement).type === "range") return;
      if (k === "Escape") (tgt as HTMLInputElement).blur();
      else if (k === "ArrowUp") { e.preventDefault(); navegar("up"); }
      else if (k === "ArrowDown") { e.preventDefault(); if (!navegar("down")) entrarAlGrid(); }
      return;
    }

    // Modal abierto: ← → cambia botón, Enter/A activa, Esc/Backspace cierra.
    if (detalle) {
      if (k === "Escape" || k === "Backspace") {
        e.preventDefault();
        cerrarModal();
      } else if (k === "ArrowLeft" || k === "ArrowRight") {
        e.preventDefault();
        modalSel = modalSel === 0 ? 1 : 0;
      } else if (k === "Enter") {
        e.preventDefault();
        accionModal();
      }
      return;
    }

    // Vista Colección: navegación espacial sobre cabecera + tarjetas de sistema.
    if (vista === "coleccion") {
      if (k === "Escape" || k === "Backspace") {
        e.preventDefault();
        vista = "biblioteca";
      } else if (k.startsWith("Arrow")) {
        e.preventDefault();
        navegar(k.slice(5).toLowerCase() as "up" | "down" | "left" | "right");
      } else if (k === "Enter") {
        const el = document.activeElement as HTMLElement | null;
        if (el?.matches?.("[data-nav]")) { e.preventDefault(); el.click(); }
      }
      return;
    }

    if (k === "Escape" || k === "Backspace") {
      e.preventDefault();
      goto("/");
    } else if (k === "ArrowRight") {
      e.preventDefault();
      if (enGrid()) moverH(1); else navegar("right");
    } else if (k === "ArrowLeft") {
      e.preventDefault();
      if (enGrid()) moverH(-1); else navegar("left");
    } else if (k === "ArrowDown") {
      e.preventDefault();
      // Fuera del grid bajamos por geometría; cuando ya no queda nada marcado
      // abajo, el siguiente escalón es el grid (que no lleva [data-nav]).
      if (enGrid()) moverV(1);
      else if (!navegar("down")) entrarAlGrid();
    } else if (k === "ArrowUp") {
      e.preventDefault();
      // Desde la primera fila del grid se sale a la cabecera; antes el foco
      // quedaba encerrado ahí abajo y filtros, buscador y pestañas eran
      // inalcanzables sin mouse.
      if (enGrid()) { if (fRow === 0) salirArriba(); else moverV(-1); }
      else navegar("up");
    } else if (k === "Enter") {
      e.preventDefault();
      if (enGrid()) {
        const g = juegoFocado();
        if (g) abrirJuego(g);
      } else {
        const el = document.activeElement as HTMLElement | null;
        if (el?.matches?.("[data-nav]")) el.click();
      }
    } else if (k === "[" || k === "PageUp") {
      e.preventDefault();
      ciclarSistema(-1);
    } else if (k === "]" || k === "PageDown") {
      e.preventDefault();
      ciclarSistema(1);
    }
  }

  // Nota: el mando físico lo maneja el bridge GLOBAL del layout (dispatcha las
  // mismas teclas que el web), así que acá no hay loop de gamepad propio — todo
  // pasa por onKey, igual que teclado y mando web.

  // --- Eventos de descarga desde el backend ---
  let unlisten: UnlistenFn[] = [];

  function calcCols() {
    cols = Math.max(2, Math.floor((window.innerWidth - 72) / 160));
  }

  onMount(async () => {
    cargarJugados();
    cargarArtDone();
    calcCols();
    window.addEventListener("resize", calcCols);
    // Prefetch lento de carátulas, tras un respiro para no competir con la carga.
    void sleep(3000).then(() => prefetchArte());
    unlisten.push(
      await listen<{ name: string; received: number; total: number }>(
        "emu_download_progress",
        (e) => {
          bajando = {
            ...bajando,
            [e.payload.name]: {
              received: e.payload.received,
              total: e.payload.total,
            },
          };
        },
      ),
    );
    unlisten.push(
      await listen<{ name: string }>("emu_download_done", (e) => {
        owned = new Set(owned).add(e.payload.name);
        const { [e.payload.name]: _d, ...rest } = bajando;
        bajando = rest;
      }),
    );
  });

  onDestroy(() => {
    window.removeEventListener("resize", calcCols);
    unlisten.forEach((u) => u());
  });

  function pct(b: { received: number; total: number }): number {
    return b.total > 0 ? Math.round((b.received / b.total) * 100) : 0;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="juegos">
  <header class="top" data-section="top">
    <h1><span class="marca">Juegos</span> <em>retro</em></h1>
    <div class="vistas">
      <button data-nav class:active={vista === "biblioteca"} onclick={() => (vista = "biblioteca")}>
        🎮 Biblioteca
      </button>
      <button data-nav class:active={vista === "coleccion"} onclick={() => (vista = "coleccion")}>
        🏆 Colección
      </button>
    </div>
    {#if prefCorriendo && prefTotal}
      <span class="pref" title="Bajando carátulas en segundo plano">
        ⬇ {artDone.size}/{prefTotal} carátulas
      </span>
    {/if}
    {#if vista === "biblioteca"}
      <nav class="filtros">
        {#each SISTEMAS as s}
          <button
            data-nav
            class:active={filtro === s.key}
            style="--c:{s.color}"
            onclick={() => (filtro = s.key)}
          >
            {s.label}
          </button>
        {/each}
      </nav>
      <div class="filtros2">
        <input
          data-nav
          class="buscar"
          type="search"
          placeholder="Buscar juego…"
          bind:value={busca}
        />
        <button data-nav class="toggle" class:active={soloTengo} onclick={() => (soloTengo = !soloTengo)}>
          {soloTengo ? "★ Mi biblioteca" : "Catálogo completo"}
        </button>
      </div>
      <div class="filtros3">
        <div class="generos">
          <button data-nav class="gchip" class:active={generoSel === "Todos"} onclick={() => (generoSel = "Todos")}>
            Todos
          </button>
          {#each generosDisponibles as g}
            <button data-nav class="gchip" class:active={generoSel === g} onclick={() => (generoSel = g)}>
              {g}
            </button>
          {/each}
        </div>
        <div class="jugado-seg">
          <button data-nav class:active={jugadoSel === "todos"} onclick={() => (jugadoSel = "todos")}>Todos</button>
          <button data-nav class:active={jugadoSel === "si"} onclick={() => (jugadoSel = "si")}>✓ Jugados</button>
          <button data-nav class:active={jugadoSel === "no"} onclick={() => (jugadoSel = "no")}>○ Sin jugar</button>
        </div>
      </div>
    {/if}
  </header>

  {#if vista === "coleccion"}
    <div class="coleccion">
      <div class="resumen">
        <div class="big">
          <strong>{tienesGlobal}</strong><span>/ {totalGlobal} juegos</span>
        </div>
        <div class="big">
          <strong>{jugadosGlobal}</strong><span>jugados</span>
        </div>
        <div class="big">
          <strong>{Math.max(0, totalGlobal - tienesGlobal)}</strong><span>te faltan</span>
        </div>
        <div class="big">
          <strong>{totalGlobal ? Math.round((tienesGlobal / totalGlobal) * 100) : 0}%</strong>
          <span>colección</span>
        </div>
      </div>
      {#if cargandoColeccion}
        <div class="empty">Calculando colección…</div>
      {/if}
      <div class="sistemas">
        {#each stats as s}
          <button data-nav class="sis-card" onclick={() => { vista = "biblioteca"; filtro = s.key; soloTengo = true; }}>
            <div class="sis-top">
              <span class="sis-label" style="color:{s.color}">{s.label}</span>
              <span class="sis-pct">{s.pct}%</span>
            </div>
            <div class="barra"><div class="barra-fill" style="width:{s.pct}%; background:{s.color}"></div></div>
            <div class="sis-nums">
              <span><b>{s.tienes}</b>/{s.total} tienes</span>
              <span><b>{s.jugados}</b> jugados</span>
              <span>{s.faltan} faltan</span>
            </div>
          </button>
        {/each}
      </div>
    </div>
  {:else if cargando}
    <div class="empty">Cargando catálogo…</div>
  {:else if errorMsg}
    <div class="empty err">{errorMsg}</div>
  {:else if lista.length === 0}
    <div class="empty">
      {soloTengo
        ? "No tienes juegos de este sistema. Súbelos por el panel web, o toca «Catálogo completo» para explorar."
        : "Catálogo vacío."}
    </div>
  {:else}
    <section class="pasillo" data-section="secciones">
      <h2 class="pasillo-titulo">Otras secciones</h2>
      <div class="fila">
        <a class="card nav-card nav-movie" data-nav href="/?tab=movie" title="Películas">
          <div class="nav-face"><span class="nav-marca">Pelis</span><em>cine</em></div>
          <div class="card-meta"><span class="card-title">🎬 Películas</span></div>
        </a>
        <a class="card nav-card nav-tv" data-nav href="/?tab=tv" title="Series">
          <div class="nav-face"><span class="nav-marca">Series</span><em>tv</em></div>
          <div class="card-meta"><span class="card-title">📺 Series</span></div>
        </a>
        <a class="card nav-card nav-anime" data-nav href="/?tab=anime" title="Anime">
          <div class="nav-face"><span class="nav-marca">Anime</span><em>日本</em></div>
          <div class="card-meta"><span class="card-title">🌸 Anime</span></div>
        </a>
        <a class="card nav-card nav-iptv" data-nav href="/iptv" title="TV en vivo">
          <div class="nav-face"><span class="nav-marca">IPTV</span><em>en vivo</em></div>
          <div class="card-meta"><span class="card-title">📡 En vivo</span></div>
        </a>
      </div>
    </section>
    {#each pasillos as p, r (r)}
      <section class="pasillo" class:pasillo-top={p.kind === "top"}>
        {#if p.label}
          <h2 class="pasillo-titulo">{p.label}</h2>
        {/if}
        <div class="fila" class:fila-top={p.kind === "top"}>
          {#each p.items as it, c (it.juego.id)}
            {@const g = it.juego}
            {@const tengo = tieneJuego(g.name)}
            {@const dl = bajando[g.name]}
            {#if it.face}
              <!-- De frente: carátula (estrenos / conocidos) -->
              <button
                class="card"
                class:focused={fRow === r && fCol === c}
                class:no-tengo={!tengo}
                bind:this={cardRefs[`${r}:${c}`]}
                use:attachThumb={g}
                onclick={() => { fRow = r; fCol = c; abrirJuego(g); }}
                onfocus={() => { fRow = r; fCol = c; }}
              >
                {#if boxarts[g.name]}
                  <img src={boxarts[g.name]} alt={g.display} loading="lazy" />
                {:else}
                  <div class="no-box" style="--c:{colorDe(g.system)}">
                    <span>{g.display}</span>
                  </div>
                {/if}
                {#if tengo}
                  <span class="card-badge tengo" style="background:{colorDe(g.system)}">▶</span>
                {:else if !dl}
                  <span class="card-badge falta">No la tienes</span>
                {/if}
                {#if dl}
                  <div class="dl"><div class="dl-bar" style="width:{pct(dl)}%"></div>
                    <span class="dl-txt">{pct(dl)}%</span></div>
                {/if}
                <div class="card-meta"><span class="card-title">{g.display}</span></div>
              </button>
            {:else}
              {@const enf = fRow === r && fCol === c}
              <!-- De lomo: spine fino; al enfocarlo se da vuelta y muestra la cara -->
              <button
                class="spine"
                class:focused={enf}
                class:girado={enf && !!boxarts[g.name]}
                class:no-tengo={!tengo}
                style="--c:{colorDe(g.system)}"
                bind:this={cardRefs[`${r}:${c}`]}
                title={g.display}
                onclick={() => { fRow = r; fCol = c; abrirJuego(g); }}
                onfocus={() => { fRow = r; fCol = c; void loadBoxart(g); }}
                onmouseenter={() => void loadBoxart(g)}
              >
                {#if enf && boxarts[g.name]}
                  <img class="spine-art" src={boxarts[g.name]} alt={g.display} />
                  {#if tengo}
                    <span class="card-badge tengo" style="background:{colorDe(g.system)}">▶</span>
                  {:else}
                    <span class="card-badge falta">No la tienes</span>
                  {/if}
                {:else}
                  <span class="spine-txt">{g.display}</span>
                {/if}
              </button>
            {/if}
          {/each}
        </div>
      </section>
    {/each}
  {/if}

  <footer class="ayuda">
    <kbd>←→↑↓</kbd>/<kbd>d-pad</kbd> navega · <kbd>Enter</kbd>/<kbd>A</kbd> abre ·
    <kbd>LB</kbd>/<kbd>RB</kbd> sistema · <kbd>Esc</kbd>/<kbd>B</kbd> a home
  </footer>

  {#if detalle}
    {@const d = detalle}
    {@const tengo = tieneJuego(d.name)}
    {@const dl = bajando[d.name]}
    {@const md = metaDe(d.name)}
    {@const ver = veredicto(d.name)}
    {@const syn = sinopsis[d.name]}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="modal-bg"
      role="presentation"
      onclick={cerrarModal}
    >
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        aria-label={d.display}
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
      >
        <div class="modal-art">
          {#if boxarts[d.name]}
            <img src={boxarts[d.name]} alt={d.display} />
          {:else}
            <div class="no-box" style="--c:{colorDe(d.system)}">
              <span>{d.display}</span>
            </div>
          {/if}
        </div>
        <div class="modal-info">
          <h2>{d.display}</h2>
          <div class="chips">
            <span class="chip" style="background:{colorDe(d.system)}">
              {SISTEMAS.find((s) => s.key === d.system)?.label}
            </span>
            <span class="chip veredicto" data-tier={ver.tier}>{ver.txt}</span>
            <span class="chip estado" class:ok={tengo}>
              {tengo ? "En tu biblioteca" : "No la tienes"}
            </span>
          </div>

          <!-- Ficha geek: datos reales de libretro-database -->
          <dl class="ficha">
            {#if md.developer}<dt>Desarrolla</dt><dd>{md.developer}</dd>{/if}
            {#if md.publisher}<dt>Edita</dt><dd>{md.publisher}</dd>{/if}
            {#if md.year}<dt>Año</dt><dd>{md.year}{md.month ? `/${md.month.padStart(2, "0")}` : ""}</dd>{/if}
            {#if md.genre}<dt>Género</dt><dd>{md.genre}</dd>{/if}
            {#if md.players}<dt>Jugadores</dt><dd>{md.players}</dd>{/if}
            {#if md.esrb}<dt>Clasificación</dt><dd>{md.esrb}</dd>{/if}
            {#if edgeDe(d.name) > 0}
              <dt>Nota Edge</dt>
              <dd class="edge"><b>{md.edge}</b>/10 <span class="edge-bar"><i style="width:{edgeDe(d.name) * 10}%"></i></span></dd>
            {/if}
          </dl>
          {#if !md.developer && !md.year && !md.genre}
            <p class="sin-ficha">Sin ficha en la base de datos.</p>
          {/if}

          <!-- Sinopsis (Wikipedia) -->
          {#if syn?.extract}
            <p class="sinopsis">{syn.extract}</p>
            {#if syn.url}
              <button class="wiki-link" onclick={() => syn.url && openUrl(syn.url)}>
                Leer en Wikipedia ({syn.lang}) ↗
              </button>
            {/if}
          {:else if cargandoSyn && !syn}
            <p class="sinopsis cargando">Buscando sinopsis…</p>
          {:else if syn}
            <p class="sinopsis cargando">Sin sinopsis en Wikipedia.</p>
          {/if}

          <p class="variante">{d.name}</p>

          {#if dl}
            <div class="modal-dl">
              <div class="modal-dl-bar" style="width:{pct(dl)}%"></div>
              <span>Bajando {pct(dl)}%</span>
            </div>
          {/if}

          <div class="modal-btns">
            <button
              class="btn primary"
              class:sel={modalSel === 0}
              disabled={!tengo}
              onclick={() => { modalSel = 0; accionModal(); }}
            >
              ▶ Descubrir
            </button>
            <button
              class="btn"
              class:sel={modalSel === 1}
              disabled={tengo || !!dl}
              onclick={() => { modalSel = 1; accionModal(); }}
            >
              {dl ? "Bajando…" : "⬇ Descargar"}
            </button>
          </div>

          <p class="cerrar-hint"><kbd>Esc</kbd>/<kbd>B</kbd> cerrar</p>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .juegos {
    height: 100%;
    overflow-y: auto;
    padding: 28px 36px 80px;
    color: #eee;
    background: #0d0d12;
  }

  .filtros2 {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-left: auto;
  }
  .buscar {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #eee;
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 13px;
    min-width: 200px;
  }
  .toggle {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #bbb;
    padding: 7px 14px;
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
  }
  .toggle.active {
    color: #fff;
    border-color: #f5c518;
    box-shadow: 0 0 0 1px #f5c518 inset;
  }

  /* --- Fila 2 de filtros: género + jugado/no jugado --- */
  .filtros3 {
    flex-basis: 100%;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin-top: 4px;
  }
  .generos {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .gchip {
    background: #15151c;
    border: 1px solid #2a2a36;
    color: #aaa;
    padding: 5px 12px;
    border-radius: 999px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .gchip:hover {
    color: #fff;
    background: #1f1f28;
  }
  .gchip.active {
    background: #7d4fff;
    border-color: #7d4fff;
    color: #fff;
  }
  .jugado-seg {
    display: flex;
    gap: 2px;
    margin-left: auto;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 999px;
    padding: 3px;
  }
  .jugado-seg button {
    background: transparent;
    border: 0;
    color: #999;
    padding: 5px 12px;
    border-radius: 999px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .jugado-seg button.active {
    background: #2b6cff;
    color: #fff;
  }

  /* --- Cards de otras secciones (tamaño carátula de juego) --- */
  .nav-card {
    text-decoration: none;
    display: block;
  }
  .nav-face {
    width: 150px;
    height: 225px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    text-align: center;
  }
  .nav-marca {
    font-size: 30px;
    font-weight: 800;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    color: transparent;
  }
  .nav-face em {
    font-style: italic;
    font-weight: 300;
    font-size: 15px;
  }
  .nav-movie .nav-face {
    background: radial-gradient(ellipse at 50% 35%, #2a2008 0%, #14100a 65%, #070503 100%);
  }
  .nav-movie .nav-marca {
    background-image: linear-gradient(90deg, #f5c518, #ffd76a, #fff0b3, #ffd76a, #f5c518);
  }
  .nav-movie .nav-face em { color: #ffd76a; }
  .nav-tv .nav-face {
    background: radial-gradient(ellipse at 50% 35%, #07261c 0%, #051613 65%, #03090a 100%);
  }
  .nav-tv .nav-marca {
    background-image: linear-gradient(90deg, #10b981, #34d399, #a7f3d0, #34d399, #10b981);
  }
  .nav-tv .nav-face em { color: #6ee7b7; }
  .nav-anime .nav-face {
    background: radial-gradient(ellipse at 50% 35%, #2e0f24 0%, #1a0814 65%, #0d0309 100%);
  }
  .nav-anime .nav-marca {
    background-image: linear-gradient(90deg, #f472b6, #ff9ad4, #ffd1ec, #ff9ad4, #f472b6);
  }
  .nav-anime .nav-face em { color: #ff9ad4; }
  .nav-iptv .nav-face {
    background: radial-gradient(ellipse at 50% 35%, #2a0d08 0%, #160806 65%, #0a0303 100%);
  }
  .nav-iptv .nav-marca {
    background-image: linear-gradient(90deg, #ef4444, #f97316, #fbbf24, #f97316, #ef4444);
  }
  .nav-iptv .nav-face em { color: #fca56b; }
  .nav-card:focus,
  .nav-card:focus-visible {
    outline: 3px solid #f5c518;
    outline-offset: 2px;
  }

  /* --- Switch de vista --- */
  .vistas {
    display: flex;
    gap: 6px;
  }
  .vistas button {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #bbb;
    padding: 7px 16px;
    border-radius: 999px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .vistas button.active {
    color: #fff;
    background: linear-gradient(135deg, #7d4fff, #2b6cff);
    border-color: transparent;
  }
  .pref {
    font-size: 12px;
    color: #9c7bff;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 999px;
    padding: 5px 12px;
    white-space: nowrap;
  }

  /* --- Colección --- */
  .coleccion {
    max-width: 1100px;
  }
  .resumen {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    margin-bottom: 26px;
  }
  .resumen .big {
    flex: 1;
    min-width: 150px;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 14px;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .resumen .big strong {
    font-size: 34px;
    font-weight: 800;
    background: linear-gradient(135deg, #9c7bff, #6ec1ff);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .resumen .big span {
    color: #888;
    font-size: 13px;
  }
  .sistemas {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 16px;
  }
  .sis-card {
    text-align: left;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 14px;
    padding: 18px;
    cursor: pointer;
    color: #eee;
  }
  .sis-card:hover {
    border-color: #444;
  }
  .sis-top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 10px;
  }
  .sis-label {
    font-size: 18px;
    font-weight: 800;
  }
  .sis-pct {
    font-size: 20px;
    font-weight: 800;
    color: #fff;
  }
  .barra {
    height: 10px;
    border-radius: 999px;
    background: #0e0e14;
    overflow: hidden;
    margin-bottom: 12px;
  }
  .barra-fill {
    height: 100%;
    border-radius: 999px;
    transition: width 0.3s;
  }
  .sis-nums {
    display: flex;
    gap: 16px;
    font-size: 12px;
    color: #999;
  }
  .sis-nums b {
    color: #eee;
  }

  /* --- Modal --- */
  .modal-bg {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    padding: 24px;
  }
  .modal {
    display: flex;
    gap: 28px;
    background: #15151c;
    border: 1px solid #2a2a36;
    border-radius: 16px;
    padding: 24px;
    max-width: 720px;
    width: 100%;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.7);
  }
  .modal-art {
    flex: 0 0 220px;
    border-radius: 10px;
    overflow: hidden;
  }
  .modal-art img,
  .modal-art .no-box {
    width: 220px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
  }
  .modal-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .modal-info h2 {
    margin: 0 0 12px;
    font-size: 24px;
  }
  .chips {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }
  .chip {
    font-size: 12px;
    font-weight: 700;
    color: #fff;
    padding: 3px 10px;
    border-radius: 6px;
  }
  .chip.estado {
    background: #444;
    color: #ddd;
  }
  .chip.estado.ok {
    background: #1f7a3d;
    color: #fff;
  }
  .variante {
    color: #777;
    font-size: 12px;
    margin: 0 0 18px;
    word-break: break-word;
  }
  .chip.veredicto {
    background: #333;
    color: #ddd;
  }
  .chip.veredicto[data-tier="4"] { background: #f5c518; color: #1a1a1a; }
  .chip.veredicto[data-tier="3"] { background: #9c7bff; color: #fff; }
  .chip.veredicto[data-tier="2"] { background: #2b6cff; color: #fff; }
  .chip.veredicto[data-tier="0"] { background: #555; color: #bbb; }
  .ficha {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 14px;
    margin: 4px 0 14px;
    font-size: 13px;
  }
  .ficha dt {
    color: #888;
  }
  .ficha dd {
    margin: 0;
    color: #eee;
  }
  .ficha dd.edge {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .edge-bar {
    display: inline-block;
    width: 90px;
    height: 6px;
    background: #2a2a36;
    border-radius: 3px;
    overflow: hidden;
  }
  .edge-bar i {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, #7d4fff, #6ec1ff);
  }
  .sin-ficha {
    color: #666;
    font-size: 12px;
    margin: 0 0 12px;
  }
  .sinopsis {
    font-size: 13px;
    line-height: 1.5;
    color: #ccc;
    margin: 0 0 8px;
    max-height: 7.5em;
    overflow-y: auto;
  }
  .sinopsis.cargando {
    color: #777;
    font-style: italic;
  }
  .wiki-link {
    background: none;
    border: none;
    color: #6ec1ff;
    font-size: 12px;
    cursor: pointer;
    padding: 0;
    margin: 0 0 14px;
  }
  .wiki-link:hover {
    text-decoration: underline;
  }
  .modal-dl {
    position: relative;
    height: 26px;
    border-radius: 6px;
    overflow: hidden;
    background: #0e0e14;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 16px;
    font-size: 12px;
    font-weight: 700;
  }
  .modal-dl-bar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    background: linear-gradient(90deg, #7d4fff, #2b6cff);
  }
  .modal-dl span {
    position: relative;
  }
  .modal-btns {
    display: flex;
    gap: 12px;
    margin-top: auto;
  }
  .btn {
    flex: 1;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid #2a2a36;
    background: #1a1a22;
    color: #eee;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
  }
  .btn.primary {
    background: linear-gradient(135deg, #7d4fff, #2b6cff);
    border: none;
  }
  .btn.sel {
    outline: none;
    box-shadow: 0 0 0 2px #f5c518;
  }
  .btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .cerrar-hint {
    color: #666;
    font-size: 11px;
    margin: 14px 0 0;
    text-align: right;
  }

  .top {
    display: flex;
    align-items: baseline;
    gap: 24px;
    margin-bottom: 22px;
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: 30px;
    font-weight: 800;
  }
  .marca {
    background: linear-gradient(135deg, #7d4fff, #2b6cff);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  h1 em {
    font-style: italic;
    color: #888;
    font-weight: 500;
  }

  .filtros {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .filtros button {
    background: #1a1a22;
    border: 1px solid #2a2a36;
    color: #bbb;
    padding: 6px 14px;
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
  }
  .filtros button.active {
    color: #fff;
    border-color: var(--c, #7d4fff);
    box-shadow: 0 0 0 1px var(--c, #7d4fff) inset;
  }

  /* --- Pasillos (Blockbuster) --- */
  .pasillo {
    margin-bottom: 26px;
  }
  .pasillo-titulo {
    font-size: 17px;
    font-weight: 800;
    margin: 0 0 10px;
    color: #ddd;
    letter-spacing: 0.3px;
  }
  .pasillo-top {
    margin-bottom: 14px;
  }
  /* Fila horizontal: lomos alineados abajo como repisa (scroll lateral). */
  .fila {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    overflow-x: auto;
    overflow-y: hidden;
    padding: 4px 2px 12px;
    scrollbar-width: thin;
  }
  .fila::-webkit-scrollbar {
    height: 8px;
  }
  .fila::-webkit-scrollbar-thumb {
    background: #2a2a36;
    border-radius: 4px;
  }
  /* Top: solo lo que cabe a lo ancho, sin scroll. */
  .fila-top {
    overflow: hidden;
    padding-bottom: 6px;
  }

  .card {
    position: relative;
    flex: 0 0 150px;
    width: 150px;
    padding: 0;
    border: none;
    border-radius: 10px;
    overflow: hidden;
    background: #15151c;
    cursor: pointer;
    text-align: left;
    transition: transform 0.12s, box-shadow 0.12s;
  }
  /* --- Lomo (spine): el resto, apretado en la repisa --- */
  .spine {
    flex: 0 0 34px;
    width: 34px;
    height: 225px;
    border: none;
    border-left: 1px solid rgba(255, 255, 255, 0.08);
    border-right: 1px solid rgba(0, 0, 0, 0.5);
    border-radius: 3px;
    cursor: pointer;
    padding: 8px 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(
      90deg,
      color-mix(in srgb, var(--c) 70%, #000) 0%,
      color-mix(in srgb, var(--c) 40%, #000) 50%,
      #000 100%
    );
    transition: flex-basis 0.12s, width 0.12s;
  }
  .spine-txt {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-height: 205px;
    font-size: 12px;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.7);
  }
  .spine.no-tengo {
    filter: grayscale(0.6) brightness(0.55);
  }
  .spine.focused,
  .spine:focus,
  .spine:focus-visible {
    outline: none;
    flex-basis: 44px;
    width: 44px;
    box-shadow: 0 0 0 2px #f5c518;
  }
  /* Enfocado con carátula → se da vuelta: se ensancha y muestra la cara. */
  .spine.girado {
    position: relative;
    flex-basis: 150px;
    width: 150px;
    padding: 0;
    overflow: hidden;
    border-radius: 8px;
    background: #15151c;
  }
  .spine-art {
    width: 150px;
    height: 225px;
    object-fit: cover;
    display: block;
  }
  .card:hover {
    transform: translateY(-3px);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.6);
  }
  .card.focused,
  .card:focus,
  .card:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px #f5c518;
  }
  /* No la tienes → carátula apagada. */
  .card.no-tengo img,
  .card.no-tengo .no-box {
    filter: grayscale(0.7) brightness(0.6);
  }

  .card img,
  .no-box {
    width: 100%;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    display: block;
    background: #222;
  }
  .no-box {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 14px;
    text-align: center;
    font-weight: 700;
    font-size: 14px;
    line-height: 1.3;
    background: radial-gradient(
      circle at 50% 35%,
      color-mix(in srgb, var(--c) 55%, #000),
      #111
    );
  }

  .card-badge {
    position: absolute;
    top: 8px;
    left: 8px;
    font-size: 11px;
    font-weight: 700;
    color: #fff;
    padding: 2px 8px;
    border-radius: 6px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
  }
  .card-badge.falta {
    background: #444;
    color: #ddd;
  }
  .card-badge.tengo {
    font-size: 12px;
    padding: 2px 7px;
  }

  .dl {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 42px;
    background: rgba(0, 0, 0, 0.55);
    height: 22px;
    display: flex;
    align-items: center;
  }
  .dl-bar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    background: linear-gradient(90deg, #7d4fff, #2b6cff);
  }
  .dl-txt {
    position: relative;
    width: 100%;
    text-align: center;
    font-size: 11px;
    font-weight: 700;
  }

  .card-meta {
    padding: 10px 12px;
  }
  .card-title {
    font-size: 13px;
    font-weight: 600;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .empty {
    color: #777;
    padding: 60px 0;
    text-align: center;
  }
  .empty.err {
    color: #f87171;
  }

  .ayuda {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 10px;
    text-align: center;
    font-size: 12px;
    color: #888;
    background: linear-gradient(transparent, #0d0d12 40%);
  }
  kbd {
    background: #222;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 11px;
  }
</style>
