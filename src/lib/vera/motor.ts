// Motor de recomendación determinista (B5).
// Devuelve el pool entero rankeado por score desc (no más segura/distinta/
// sorpresa). La UI muestra #1 destacada y el resto navegable.
// Combina perfil del usuario (histórico + sesión) + vote_average TMDb +
// procedencia del candidato. Sin Math.random — tie-break con PRNG seeded.

import type {
  Pelicula,
  EstadoVera,
  RankingPeli,
  Reaccion,
  PerfilHistorico,
} from "./tipos";
import { HORA_NOCHE } from "./config";
import { getPerfilHistorico } from "./historial";

// =============================================================================
// PESOS NUMÉRICOS — REVISAR A OJO, EL TYPE-CHECK NO LOS VALIDA.
// =============================================================================
//
// Sesgo nocturno reducido (no domina al perfil del usuario).
const SESGO_NOCHE_LIVIANO = 0.3;
const SESGO_NOCHE_DENSO = -0.15;

// Pesos del score final. Suman 1.0.
const W_PERFIL = 0.55;        // afinidad del usuario (histórico + sesión)
const W_VOTE = 0.30;          // vote_average de TMDb normalizado a 0..1
const W_PROCEDENCIA = 0.15;   // discover/reco/diversidad/fallback

// Valor categórico para "procedencia" como señal numérica.
//   reco       1.0  vino de recommendations de algo que te gustó
//   discover   0.5  matchea el intent puro, neutro
//   diversidad 0.5  presencia LGBT+ garantizada, mismo trato que discover
//   fallback   0.0  último recurso, no es lo que pediste
const VALOR_PROCEDENCIA: Record<Pelicula["procedencia"], number> = {
  reco: 1.0,
  discover: 0.5,
  diversidad: 0.5,
  fallback: 0.0,
};

// Coeficiente del peso de intuición previa (sesión) respecto al juicio.
// Si solo hay interes (sin juicio), el peso es interes * COEF. Si hay juicio,
// manda juicio directo. Mismo criterio que historial.ts.pesoReaccionPersistida.
const COEF_INTERES_SIN_JUICIO = 0.6;

// Normalización del perfil agregado (histórico + sesión) a 0..1.
// Rango típico observado en pruebas: -10..+10. Mapeo afín, fuera de rango satura.
const AFINIDAD_MIN = -10;
const AFINIDAD_MAX = 10;
function normalizarAfinidad(v: number): number {
  const t = (v - AFINIDAD_MIN) / (AFINIDAD_MAX - AFINIDAD_MIN);
  return Math.max(0, Math.min(1, t));
}
//
// =============================================================================

// Peso de una reacción de sesión actual (modelo B5).
// Juicio manda si existe; si no, interes pesa menos (intuición vs realidad).
function pesoReaccionSesion(r: Reaccion): number {
  if (r.juicio !== null) return r.juicio;
  return r.interes * COEF_INTERES_SIN_JUICIO;
}

// Filtros duros del pool. B5: NO excluye vistas (aparecen con badge).
// Solo familyFriendly cuando hay niños.
function filtrar(
  catalogo: Pelicula[],
  estado: EstadoVera,
): Pelicula[] {
  return catalogo.filter((p) => {
    if (estado.contexto === "ninos" && !p.familyFriendly) return false;
    return true;
  });
}

// Combina perfil histórico + reacciones de sesión actual en una sola tabla
// de pesos por género (y tono). El histórico ya viene calculado de historial.ts;
// las reacciones de sesión se suman acá con pesoReaccionSesion.
//
// Decisión: las reacciones de sesión suman EL MISMO peso a género y a tono.
// El modelo viejo tenía tono pesando menos (0.8 vs 1.2 para género) bajo la
// idea de que "interés a priori" dice más del género (visible en el póster)
// que del tono (que solo se descubre viendo). En B5 el peso viene del juicio
// post-vista, donde +5 a una peli denso ES señal directa de "me gustó denso";
// diluirla es perder información real. Si en pruebas el ranking se obsesiona
// con liviano/denso por sobre el género, acá es donde se ajusta (separar el
// peso de tono con un coef como 0.7).
function combinarPerfil(
  historico: PerfilHistorico,
  reaccionesSesion: Reaccion[],
): { generos: Map<string, number>; tonos: Map<string, number> } {
  const generos = new Map(historico.generosPesos);
  const tonos = new Map<string, number>();
  for (const r of reaccionesSesion) {
    const peso = pesoReaccionSesion(r);
    if (peso === 0) continue;
    for (const g of r.pelicula.generos) {
      generos.set(g, (generos.get(g) ?? 0) + peso);
    }
    tonos.set(r.pelicula.tono, (tonos.get(r.pelicula.tono) ?? 0) + peso);
  }
  return { generos, tonos };
}

// Score por peli. Combina perfil normalizado + vote + procedencia + sesgo noche.
function scorePeli(
  p: Pelicula,
  generos: Map<string, number>,
  tonos: Map<string, number>,
  esNoche: boolean,
): number {
  let afinidad = 0;
  for (const g of p.generos) {
    afinidad += generos.get(g) ?? 0;
  }
  afinidad += tonos.get(p.tono) ?? 0;
  const sPerfil = normalizarAfinidad(afinidad);

  const sVote = Math.max(0, Math.min(1, p.rating / 10));
  const sProc = VALOR_PROCEDENCIA[p.procedencia] ?? 0.5;

  let s = W_PERFIL * sPerfil + W_VOTE * sVote + W_PROCEDENCIA * sProc;

  if (esNoche) {
    s += p.tono === "liviano" ? SESGO_NOCHE_LIVIANO : SESGO_NOCHE_DENSO;
  }
  return s;
}

// PRNG seeded determinista. Sin Math.random en ningún lado del motor.
// Usado para tie-breaks reproducibles dentro del mismo pool.
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

// Recomienda B5: devuelve TODO el pool rankeado por score desc.
// La UI consume este array: la #1 va destacada, el resto navegable.
//
// Async porque internamente lee perfil histórico (SQLite + ratings persistidos).
// El caller puede pasar `historicoPrecargado` para evitar releer el histórico
// cuando re-rankea por cambios en sesión (mientras el histórico no cambia,
// solo cambian las reacciones de sesión).
export async function recomendar(
  catalogo: Pelicula[],
  estado: EstadoVera,
  historicoPrecargado?: PerfilHistorico,
): Promise<RankingPeli[]> {
  const candidatos = filtrar(catalogo, estado);
  if (candidatos.length === 0) return [];

  const historico = historicoPrecargado ?? (await getPerfilHistorico());
  const { generos, tonos } = combinarPerfil(historico, estado.reacciones);
  const esNoche = estado.horaActual >= HORA_NOCHE || estado.horaActual < 5;

  const puntuados: RankingPeli[] = candidatos.map((p) => ({
    pelicula: p,
    score: scorePeli(p, generos, tonos, esNoche),
  }));

  // Tie-break determinista: cuando dos pelis empatan en score (diferencia <
  // 1e-4), un PRNG seeded por el pool desempata. Sin Math.random.
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

  return puntuados;
}
