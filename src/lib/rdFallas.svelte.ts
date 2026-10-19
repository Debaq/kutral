// Fuentes que Real-Debrid no pudo abrir. Al fallar, el selector sigue con la
// próxima; acá queda anotado POR QUÉ, para marcarla en la lista y no volver a
// gastar un intento en ella (cada uno son segundos de espera). Si el usuario
// la elige igual, se le explica y se le ofrece verla por torrent.
//
// Se anota solo lo que no se arregla reintentando al rato: un corte de red o
// un 503 de RD no dicen nada de la fuente. Cada motivo vence a su tiempo: un
// DMCA no se levanta, un torrent muerto puede revivir, y lo que RD no tenía en
// caché quizás ya lo bajó.

export type FallaRd = {
  tipo: "dmca" | "muerta" | "sin_cache" | "sin_capitulo";
  /** Para mostrar: "RD la bloquea por derechos de autor (DMCA)". */
  motivo: string;
  /** Corto, para el chip de la lista. */
  corto: string;
  cuando: number;
};

const K = "rd_fallas";
const DIA = 86_400_000;
const VENCE: Record<FallaRd["tipo"], number> = {
  dmca: 30 * DIA,
  muerta: 7 * DIA,
  sin_cache: DIA,
  sin_capitulo: 30 * DIA,
};

/** clave (hash, o hash:TxE si es del capítulo) → falla. */
export const rdFallas = $state<Record<string, FallaRd>>(leer());

// Y lo contrario: las que RD sí abrió. Quedan en la cuenta de RD, así que la
// próxima vez abren al instante y van primero. Vencen antes que un DMCA: RD
// limpia los torrents viejos de la cuenta.
const K_OK = "rd_ok";
const VENCE_OK = 7 * DIA;

/** hash → cuándo abrió en RD. */
export const rdOk = $state<Record<string, number>>(leerOk());

function leerOk(): Record<string, number> {
  try {
    const v = JSON.parse(localStorage.getItem(K_OK) || "{}") as Record<string, number>;
    const ahora = Date.now();
    for (const [k, t] of Object.entries(v)) {
      if (typeof t !== "number" || ahora - t > VENCE_OK) delete v[k];
    }
    return v;
  } catch {
    return {};
  }
}

function guardarOk() {
  try {
    localStorage.setItem(K_OK, JSON.stringify(rdOk));
  } catch {
    /* sin storage: dura lo que dure la sesión */
  }
}

/** ¿RD ya abrió esta fuente hace poco? */
export function okRd(hash: string | null | undefined): boolean {
  if (!hash) return false;
  const t = rdOk[hash.toLowerCase()];
  return !!t && Date.now() - t <= VENCE_OK;
}

export function anotarOkRd(hash: string | null | undefined) {
  if (!hash) return;
  rdOk[hash.toLowerCase()] = Date.now();
  guardarOk();
}

function leer(): Record<string, FallaRd> {
  try {
    const v = JSON.parse(localStorage.getItem(K) || "{}") as Record<string, FallaRd>;
    const ahora = Date.now();
    for (const [k, f] of Object.entries(v)) {
      if (!f?.tipo || ahora - f.cuando > (VENCE[f.tipo] ?? 0)) delete v[k];
    }
    return v;
  } catch {
    return {};
  }
}

function guardar() {
  try {
    localStorage.setItem(K, JSON.stringify(rdFallas));
  } catch {
    /* sin storage: dura lo que dure la sesión */
  }
}

/**
 * Qué dice el error de rd_resolver (rd.rs) de la fuente. null = no es culpa
 * de la fuente (red, token, RD caído): no se anota.
 */
export function clasificarFallaRd(err: string): Omit<FallaRd, "cuando"> | null {
  if (err.includes("BLOQUEADO_DMCA")) {
    return { tipo: "dmca", motivo: "RD la bloquea por derechos de autor (DMCA)", corto: "bloqueada (DMCA)" };
  }
  if (/RD torrent falló: (dead|virus|error|magnet_error)/.test(err)) {
    return { tipo: "muerta", motivo: "RD no pudo bajarla: el torrent está muerto o con error", corto: "torrent muerto" };
  }
  if (err.includes("RD no cacheado")) {
    return { tipo: "sin_cache", motivo: "RD no la tiene lista (no está en su caché) y no alcanzó a bajarla", corto: "no está en RD" };
  }
  if (/el pack no trae|sin link del capítulo/.test(err)) {
    return { tipo: "sin_capitulo", motivo: "el torrent no trae este capítulo", corto: "sin este capítulo" };
  }
  return null;
}

function claves(hash: string, cap: string): string[] {
  const h = hash.toLowerCase();
  return cap ? [h, `${h}:${cap}`] : [h];
}

/** `cap` = "T1E5" en series ("" en películas): lo del capítulo va aparte. */
export function fallaRd(hash: string | null | undefined, cap = ""): FallaRd | null {
  if (!hash) return null;
  for (const k of claves(hash, cap)) {
    const f = rdFallas[k];
    if (f && Date.now() - f.cuando <= VENCE[f.tipo]) return f;
  }
  return null;
}

/** Anota la falla si dice algo de la fuente. Devuelve lo anotado. */
export function anotarFallaRd(hash: string | null | undefined, cap: string, err: string): FallaRd | null {
  const c = clasificarFallaRd(err);
  if (!hash || !c) return null;
  const h = hash.toLowerCase();
  // "Sin este capítulo" es del capítulo; lo demás, del torrent entero.
  const k = c.tipo === "sin_capitulo" && cap ? `${h}:${cap}` : h;
  const f = { ...c, cuando: Date.now() };
  rdFallas[k] = f;
  guardar();
  if (h in rdOk) {
    delete rdOk[h];
    guardarOk();
  }
  return f;
}

/** RD sí pudo (o el usuario quiere reintentar): se borra la marca. */
export function olvidarFallaRd(hash: string | null | undefined, cap = "") {
  if (!hash) return;
  let hubo = false;
  for (const k of claves(hash, cap)) {
    if (k in rdFallas) {
      delete rdFallas[k];
      hubo = true;
    }
  }
  if (hubo) guardar();
}
