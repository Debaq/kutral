// Qué trae cada versión en el idioma elegido, ya revisado (ffprobe sobre el
// archivo, ver probeEs en SourcePicker). Revisar cuesta resolver el torrent y
// leer la cabecera, 5-15 s por fuente: si ya se supo, no se vuelve a revisar
// al volver a ver el título o al probar otra versión.
//
// Por torrent (hash), capítulo (en un pack cada uno es otro archivo) e idioma.
// Lo que no se pudo revisar ("unknown") no se guarda.

export type Veredicto = "audio" | "subs" | "none";

const K = "kutral:idioma_fuentes";
const VENCE_MS = 180 * 86_400_000;
const MAX = 2000;

type Guardado = Record<string, { v: Veredicto; t: number }>;

let memoria: Guardado | null = null;

function leer(): Guardado {
  if (memoria) return memoria;
  try {
    const v = JSON.parse(localStorage.getItem(K) || "{}") as Guardado;
    memoria = v && typeof v === "object" ? v : {};
  } catch {
    memoria = {};
  }
  return memoria;
}

function clave(hash: string, cap: string, idioma: string): string {
  return `${hash.toLowerCase()}|${cap}|${idioma}`;
}

export function veredictoGuardado(hash: string | null, cap: string, idioma: string): Veredicto | null {
  if (!hash) return null;
  const e = leer()[clave(hash, cap, idioma)];
  return e && Date.now() - e.t <= VENCE_MS ? e.v : null;
}

export function guardarVeredicto(hash: string | null, cap: string, idioma: string, v: string): void {
  if (!hash || (v !== "audio" && v !== "subs" && v !== "none")) return;
  const m = leer();
  m[clave(hash, cap, idioma)] = { v, t: Date.now() };
  // Se va lo más viejo cuando pasa del tope.
  const claves = Object.keys(m);
  if (claves.length > MAX) {
    claves.sort((a, b) => m[a].t - m[b].t);
    for (const k of claves.slice(0, claves.length - MAX)) delete m[k];
  }
  try {
    localStorage.setItem(K, JSON.stringify(m));
  } catch {
    /* sin storage: dura lo que dure la sesión */
  }
}
