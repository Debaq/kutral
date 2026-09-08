// Motor de recomendación determinista.
// Devuelve el pool entero rankeado. La UI muestra la #1 destacada y el resto
// navegable. Sin Math.random — los desempates salen de un PRNG con semilla
// derivada del pool.
//
// Qué cambió respecto de la versión anterior, y por qué:
//
//  1. AFINIDAD PROMEDIADA, NO SUMADA. Antes se sumaba el peso de cada género
//     de la peli: una peli con 4 géneros arrancaba con 4x la afinidad de una
//     con 1, sin que al usuario le gustara más. Ahora es un promedio ponderado
//     por confianza de la señal.
//
//  2. CALIDAD BAYESIANA. Antes era vote_average/10 pelado, así que un 8.9 con
//     110 votos le ganaba a un 8.2 con 12.000. Ahora la nota se contrae hacia
//     la media global según cuántos votos la sostienen (fórmula IMDb).
//
//  3. EL SESGO NOCTURNO YA NO PISA AL INTENT. Antes se sumaba SIEMPRE y
//     después del score normalizado: pedías "algo denso" a las 23h y el motor
//     te penalizaba lo denso 0.15 y premiaba lo liviano 0.30. Ahora solo se
//     aplica cuando no pediste un tono (intent "sorpresa").
//
//  4. EL CONTEXTO HACE ALGO. Antes se preguntaba quién estaba en el sillón y
//     solo se usaba para el filtro familiar de "con niños"; el resto de las
//     respuestas se descartaba.
//
//  5. LAS VISTAS PESAN MENOS. Siguen apareciendo (rever es legítimo) pero ya
//     no pueden salir #1 por delante de algo que no viste.
//
//  6. DIVERSIDAD EN EL TOPE. Un re-ranking MMR evita que las primeras cinco
//     sean la misma peli con distinto título.

import type {
  Pelicula,
  EstadoVera,
  RankingPeli,
  MotivoRanking,
  Reaccion,
  PerfilHistorico,
  Contexto,
} from "./tipos";
import {
  HORA_NOCHE,
  UMBRAL_NO_MATCH,
  VOTOS_MINIMOS,
  MEDIA_GLOBAL_TMDB,
  LAMBDA_MMR,
  TOPE_MMR,
  PENAL_VISTA_BASE,
  PENAL_DURACION_MAX,
  PENAL_IDIOMA_INCOMODO,
  BONUS_ANIMACION,
} from "./config";
import { getPerfilHistorico } from "./historial";
import { tonoDeIds } from "./generos";
import { idsDeIntent } from "./intenciones";

// =============================================================================
// PESOS NUMÉRICOS — REVISAR A OJO, EL TYPE-CHECK NO LOS VALIDA.
// =============================================================================

// Pesos del score base. Suman 1.0 y el resultado vive en 0..1.
const W_PERFIL = 0.55;      // afinidad del usuario (histórico + sesión)
const W_CALIDAD = 0.30;     // rating bayesiano
const W_PROCEDENCIA = 0.15; // discover/reco/diversidad/local/fallback

// Sesgo nocturno. Solo aplica cuando el usuario NO pidió un tono explícito.
const SESGO_NOCHE_LIVIANO = 0.1;
const SESGO_NOCHE_DENSO = -0.05;

// Valor categórico de "procedencia" como señal numérica.
//   reco       1.0  vino de recommendations de algo que te gustó
//   discover   0.5  matchea el intent puro, neutro
//   diversidad 0.5  presencia LGBT+ garantizada, mismo trato que discover
//   local      0.5  catálogo importado, ya filtrado por tu perfil
//   fallback   0.0  último recurso, no es lo que pediste
const VALOR_PROCEDENCIA: Record<Pelicula["procedencia"], number> = {
  reco: 1.0,
  discover: 0.5,
  diversidad: 0.5,
  local: 0.5,
  fallback: 0.0,
};

// Confianza relativa de cada señal de afinidad. El género es la señal gruesa
// y confiable; el director acierta fuerte pero con pocas muestras; el actor de
// reparto es la más ruidosa (un actor bueno en una peli mala sigue siendo un
// actor bueno). Estos números ponderan el PROMEDIO, no lo suman.
const CONF_GENERO = 1.0;
const CONF_TONO = 0.7;
const CONF_DIRECTOR = 0.8;
const CONF_ACTOR = 0.35;
const CONF_DECADA = 0.3;

// Coeficiente del peso de intuición previa (sesión) respecto al juicio.
// Si solo hay interes (sin juicio), el peso es interes * COEF. Si hay juicio,
// manda juicio directo. Mismo criterio que historial.ts.
const COEF_INTERES_SIN_JUICIO = 0.6;

// Normalización de la afinidad promedio a 0..1. Rango típico observado:
// -10..+10. Mapeo afín, fuera de rango satura. Sin señal → 0 → 0.5 (neutro).
const AFINIDAD_MIN = -10;
const AFINIDAD_MAX = 10;
function normalizarAfinidad(v: number): number {
  const t = (v - AFINIDAD_MIN) / (AFINIDAD_MAX - AFINIDAD_MIN);
  return Math.max(0, Math.min(1, t));
}

// Qué mueve cada contexto del sillón. Valores por género (nombre es-ES);
// se aplican una vez, con tope, no una vez por género coincidente.
//
// No es una teoría del entretenimiento: es qué se elige distinto según con
// quién estés. Solo, uno banca una densidad que con amigos se hace cuesta
// arriba; en pareja el terror es una apuesta arriesgada salvo que sea el plan.
// "ninos" ya pasó por el filtro duro de familyFriendly, así que acá solo
// refuerza lo que efectivamente funciona con público mixto.
const AJUSTE_CONTEXTO: Record<Contexto, Record<string, number>> = {
  solo: {
    Documental: 0.06,
    Drama: 0.05,
    Misterio: 0.04,
    Familia: -0.05,
  },
  pareja: {
    Romance: 0.06,
    Drama: 0.04,
    Comedia: 0.03,
    Terror: -0.04,
    Documental: -0.03,
  },
  amigos: {
    Comedia: 0.06,
    Acción: 0.05,
    Terror: 0.04,
    Documental: -0.05,
    Drama: -0.03,
  },
  ninos: {
    Animación: 0.06,
    Familia: 0.06,
    Aventura: 0.04,
  },
};

// Tope del ajuste de contexto, en cualquier dirección. Sin esto, una peli
// "Comedia + Acción + Terror" vista con amigos acumularía +0.15 y el contexto
// pasaría a mandar por encima del perfil, que es la señal fuerte.
const TOPE_AJUSTE_CONTEXTO = 0.1;

//
// =============================================================================

// Peso de una reacción de sesión actual.
// Juicio manda si existe; si no, interes pesa menos (intuición vs realidad).
function pesoReaccionSesion(r: Reaccion): number {
  if (r.juicio !== null) return r.juicio;
  return r.interes * COEF_INTERES_SIN_JUICIO;
}

// Nota ponderada por cantidad de votos (fórmula IMDb), devuelta en 0..1.
//   (v/(v+m))*R + (m/(v+m))*C
// Con v=0 el resultado es exactamente C/10: "no sabemos nada, asumí la media".
export function calidadBayesiana(rating: number, votos: number): number {
  const v = Math.max(0, votos);
  const ponderado =
    (v / (v + VOTOS_MINIMOS)) * rating +
    (VOTOS_MINIMOS / (v + VOTOS_MINIMOS)) * MEDIA_GLOBAL_TMDB;
  return Math.max(0, Math.min(1, ponderado / 10));
}

// Filtros duros del pool. NO excluye vistas (aparecen con badge y penalizadas).
// Solo familyFriendly cuando hay niños — regla de marca: "con niños" siempre
// implica adulto presente, pero el contenido igual tiene que servir para todos.
function filtrar(catalogo: Pelicula[], estado: EstadoVera): Pelicula[] {
  return catalogo.filter((p) => {
    if (estado.contexto === "ninos" && !p.familyFriendly) return false;
    return true;
  });
}

interface TablasPerfil {
  generos: Map<string, number>;
  tonos: Map<string, number>;
  directores: Map<string, number>;
  actores: Map<string, number>;
  decadas: Map<string, number>;
}

// Combina perfil histórico + reacciones de la sesión actual.
// El histórico ya viene calculado de historial.ts (incluye la memoria larga de
// vera_weights); las reacciones de sesión se suman acá.
//
// Decisión: las reacciones de sesión suman EL MISMO peso a género y a tono.
// El modelo viejo hacía pesar menos al tono bajo la idea de que "interés a
// priori" dice más del género (visible en el póster) que del tono (que solo se
// descubre viendo). Pero el peso viene del juicio post-vista, donde +5 a una
// peli densa ES señal directa de "me gustó densa"; diluirla pierde información
// real. La confianza relativa se aplica después, en el promedio (CONF_*).
function combinarPerfil(
  historico: PerfilHistorico,
  reaccionesSesion: Reaccion[],
): TablasPerfil {
  const generos = new Map(historico.generosPesos);
  const directores = new Map(historico.directoresPesos);
  const actores = new Map(historico.actoresPesos);
  const decadas = new Map(historico.decadasPesos);
  const tonos = new Map<string, number>();

  const sumar = (m: Map<string, number>, k: string, v: number) => {
    if (!k) return;
    m.set(k, (m.get(k) ?? 0) + v);
  };

  for (const r of reaccionesSesion) {
    const peso = pesoReaccionSesion(r);
    if (peso === 0) continue;
    const p = r.pelicula;
    for (const g of p.generos) sumar(generos, g, peso);
    sumar(tonos, p.tono, peso);
    sumar(directores, p.director, peso);
    for (const a of p.actores) sumar(actores, a, peso);
    const anio = parseInt(p.anio, 10);
    if (Number.isFinite(anio)) {
      sumar(decadas, String(Math.floor(anio / 10) * 10), peso);
    }
  }
  return { generos, tonos, directores, actores, decadas };
}

// Afinidad de una peli con el perfil: promedio ponderado por confianza de las
// señales DISPONIBLES. Una peli sin enriquecer (sin director ni reparto) se
// juzga solo por género, tono y década, sin que eso la castigue: el divisor
// solo cuenta las señales que existen.
//
// Devuelve también las etiquetas que más aportaron, para poder explicar el
// ranking en la ficha ("te la propongo por Drama y Denis Villeneuve").
function afinidadDe(
  p: Pelicula,
  t: TablasPerfil,
): { valor: number; razones: string[] } {
  let suma = 0;
  let pesos = 0;
  const aportes: { etiqueta: string; valor: number }[] = [];

  const agregar = (valor: number | undefined, conf: number, etiqueta: string) => {
    if (valor === undefined) return;
    suma += valor * conf;
    pesos += conf;
    if (valor > 0) aportes.push({ etiqueta, valor: valor * conf });
  };

  // Los géneros se promedian ENTRE ELLOS primero y entran como una sola señal.
  // Así "Drama" cuenta lo mismo tenga la peli 1 género o 5.
  if (p.generos.length > 0) {
    let sg = 0;
    let n = 0;
    for (const g of p.generos) {
      const v = t.generos.get(g);
      if (v === undefined) continue;
      sg += v;
      n++;
      if (v > 0) aportes.push({ etiqueta: g, valor: v });
    }
    if (n > 0) {
      suma += (sg / n) * CONF_GENERO;
      pesos += CONF_GENERO;
    }
  }

  agregar(t.tonos.get(p.tono), CONF_TONO, p.tono === "denso" ? "denso" : "liviano");
  if (p.director) agregar(t.directores.get(p.director), CONF_DIRECTOR, p.director);

  // Igual que con los géneros: el reparto entra promediado, no sumado.
  if (p.actores.length > 0) {
    let sa = 0;
    let n = 0;
    for (const a of p.actores) {
      const v = t.actores.get(a);
      if (v === undefined) continue;
      sa += v;
      n++;
      if (v > 0) aportes.push({ etiqueta: a, valor: v });
    }
    if (n > 0) {
      suma += (sa / n) * CONF_ACTOR;
      pesos += CONF_ACTOR;
    }
  }

  const anio = parseInt(p.anio, 10);
  if (Number.isFinite(anio)) {
    const dec = String(Math.floor(anio / 10) * 10);
    agregar(t.decadas.get(dec), CONF_DECADA, `los ${dec}`);
  }

  const razones = aportes
    .sort((a, b) => b.valor - a.valor)
    .slice(0, 3)
    .map((a) => a.etiqueta);

  return { valor: pesos === 0 ? 0 : suma / pesos, razones };
}

// Ajuste por quién está en el sillón. Con tope, ver TOPE_AJUSTE_CONTEXTO.
function ajusteContexto(p: Pelicula, contexto: Contexto | null): number {
  if (contexto === null) return 0;
  const tabla = AJUSTE_CONTEXTO[contexto];
  let s = 0;
  for (const g of p.generos) s += tabla[g] ?? 0;
  return Math.max(-TOPE_AJUSTE_CONTEXTO, Math.min(TOPE_AJUSTE_CONTEXTO, s));
}

// Penalización por ya haberla visto.
// Rever algo que te encantó es un plan; rever algo que te dejó frío no.
// Por eso la penalización escala con el juicio: +5 apenas baja, -5 hunde.
function penalVista(juicio: number | null): number {
  if (juicio === null) return 0;
  // juicio -5..+5 → factor 1.4..0.2
  const factor = 1.4 - ((juicio + 5) / 10) * 1.2;
  return PENAL_VISTA_BASE * factor;
}

// Penalización por pasarse del tope de duración del perfil.
// Proporcional al exceso y con techo: 20 minutos de más no es lo mismo que
// una hora de más, pero tampoco queremos que una peli de 4h caiga al fondo
// absoluto si es lo único bueno del pool.
function penalDuracion(p: Pelicula, max: number | null): number {
  if (max === null || p.runtime === null || p.runtime <= max) return 0;
  const exceso = (p.runtime - max) / max;
  return Math.min(PENAL_DURACION_MAX, exceso * PENAL_DURACION_MAX * 2);
}

// Ajuste por las preferencias del perfil que son de GRADO, no de veto.
// Las de veto (géneros excluidos, temas sensibles, idiomas que no tolera,
// animación cuando no le gusta) ya sacaron la peli del pool en tmdb.ts: acá
// solo quedan las que mueven el orden.
//
// Idioma: penaliza estar fuera de la zona cómoda, pero solo si SABEMOS el
// idioma. Una peli sin idiomaOriginal no se castiga por una duda nuestra.
function ajustePreferencias(p: Pelicula, setup: EstadoVera["setup"]): number {
  if (!setup) return 0;
  let s = 0;
  const idioma = p.idiomaOriginal;
  if (
    setup.idiomasComodos.length > 0 &&
    idioma.length > 0 &&
    !setup.idiomasComodos.includes(idioma)
  ) {
    s -= PENAL_IDIOMA_INCOMODO;
  }
  if (setup.animacion === "gusta" && p.generos.includes("Animación")) {
    s += BONUS_ANIMACION;
  }
  return s;
}

// PRNG seeded determinista. Sin Math.random en ningún lado del motor.
// Usado para desempates reproducibles dentro del mismo pool.
function seedDelPool(pool: Pelicula[]): number {
  const s = pool
    .map((p) => p.id)
    .sort()
    .join("|");
  // FNV-1a 32 bits.
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = (h * 16777619) >>> 0;
  }
  return h;
}

function mulberry32(seedInicial: number): () => number {
  let seed = seedInicial >>> 0;
  return () => {
    seed = (seed + 0x6d2b79f5) >>> 0;
    let t = seed;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Similitud 0..1 entre dos pelis, para el re-ranking por diversidad.
// Jaccard de géneros como base, más un empujón si comparten director o década:
// dos dramas de los 70 del mismo director son "la misma recomendación".
function similitud(a: Pelicula, b: Pelicula): number {
  const ga = new Set(a.generos);
  const gb = new Set(b.generos);
  let inter = 0;
  for (const g of ga) if (gb.has(g)) inter++;
  const union = ga.size + gb.size - inter;
  let s = union === 0 ? 0 : inter / union;
  if (a.director && a.director === b.director) s = Math.min(1, s + 0.3);
  if (a.anio && b.anio && a.anio.slice(0, 3) === b.anio.slice(0, 3)) {
    s = Math.min(1, s + 0.1);
  }
  return s;
}

// Re-ranking MMR sobre las primeras TOPE_MMR posiciones.
// En cada paso elige el candidato que maximiza
//   λ * score − (1 − λ) * (máxima similitud con lo ya elegido)
// Resultado: el orden sigue mandado por el score, pero se rompe la racha de
// títulos gemelos que el ranking puro produce.
function diversificar(lista: RankingPeli[]): RankingPeli[] {
  if (lista.length <= 2) return lista;
  const tope = Math.min(TOPE_MMR, lista.length);
  const candidatos = lista.slice(0, tope);
  const resto = lista.slice(tope);
  const elegidos: RankingPeli[] = [];

  // La #1 no se toca: es la mejor del pool y es la que Vera propone.
  elegidos.push(candidatos.shift()!);

  while (candidatos.length > 0) {
    let mejorIdx = 0;
    let mejorValor = -Infinity;
    for (let i = 0; i < candidatos.length; i++) {
      let simMax = 0;
      for (const e of elegidos) {
        const s = similitud(candidatos[i].pelicula, e.pelicula);
        if (s > simMax) simMax = s;
      }
      const v = LAMBDA_MMR * candidatos[i].score - (1 - LAMBDA_MMR) * simMax;
      if (v > mejorValor) {
        mejorValor = v;
        mejorIdx = i;
      }
    }
    elegidos.push(candidatos.splice(mejorIdx, 1)[0]);
  }
  return [...elegidos, ...resto];
}

// ¿Vale la pena proponer algo, o Vera debería ser honesta?
// Se mide contra la #1: si ni la mejor del pool llega al umbral, ninguna llega.
export function hayMatch(ranking: RankingPeli[]): boolean {
  return ranking.length > 0 && ranking[0].score >= UMBRAL_NO_MATCH;
}

// Rankea el pool entero.
//
// Async porque internamente lee perfil histórico (SQLite + localStorage).
// El caller puede pasar `historicoPrecargado` para evitar releer el histórico
// cuando re-rankea por cambios de sesión (mientras el histórico no cambia,
// solo cambian las reacciones).
//
// `juicioPorId` trae las vistas: id → juicio. Se pasa desde afuera porque la
// UI ya tiene ese mapa hidratado y volver a leerlo acá sería I/O duplicada.
export async function recomendar(
  catalogo: Pelicula[],
  estado: EstadoVera,
  historicoPrecargado?: PerfilHistorico,
  juicioPorId?: Map<string, number | null>,
): Promise<RankingPeli[]> {
  const candidatos = filtrar(catalogo, estado);
  if (candidatos.length === 0) return [];

  const historico = historicoPrecargado ?? (await getPerfilHistorico());
  const tablas = combinarPerfil(historico, estado.reacciones);

  // El sesgo nocturno solo entra si el usuario no pidió un tono. Si pidió
  // "denso", el intent manda sobre la hora — es el bug que se arregló.
  const tonoPedido =
    estado.intencion === null
      ? null
      : tonoDeIds(idsDeIntent(estado.intencion));
  const esNoche =
    estado.horaActual >= HORA_NOCHE || estado.horaActual < 5;
  const aplicaSesgoNoche = esNoche && tonoPedido === null;

  const duracionMax = estado.setup?.duracionMax ?? null;

  const puntuados: RankingPeli[] = candidatos.map((p) => {
    const { valor, razones } = afinidadDe(p, tablas);
    const sPerfil = normalizarAfinidad(valor);
    const sCalidad = calidadBayesiana(p.rating, p.votos);
    const sProc = VALOR_PROCEDENCIA[p.procedencia] ?? 0.5;

    let score =
      W_PERFIL * sPerfil + W_CALIDAD * sCalidad + W_PROCEDENCIA * sProc;

    const ajuste =
      ajusteContexto(p, estado.contexto) + ajustePreferencias(p, estado.setup);
    score += ajuste;

    if (aplicaSesgoNoche) {
      score += p.tono === "liviano" ? SESGO_NOCHE_LIVIANO : SESGO_NOCHE_DENSO;
    }

    const pv = penalVista(juicioPorId?.get(p.id) ?? null);
    const pd = penalDuracion(p, duracionMax);
    score -= pv + pd;

    const porQue: MotivoRanking = {
      afinidad: sPerfil,
      calidad: sCalidad,
      procedencia: sProc,
      penalVista: -pv,
      penalDuracion: -pd,
      ajusteContexto: ajuste,
      razones,
    };
    return { pelicula: p, score, porQue };
  });

  // Desempate determinista: cuando dos pelis empatan (diferencia < 1e-4), un
  // PRNG con semilla derivada del pool desempata. Sin Math.random.
  const rng = mulberry32(seedDelPool(candidatos));
  const rndPorId = new Map<string, number>();
  for (const c of candidatos) rndPorId.set(c.id, rng());
  puntuados.sort((a, b) => {
    const d = b.score - a.score;
    if (Math.abs(d) > 1e-4) return d;
    const ra = rndPorId.get(a.pelicula.id) ?? 0;
    const rb = rndPorId.get(b.pelicula.id) ?? 0;
    return rb - ra;
  });

  return diversificar(puntuados);
}
