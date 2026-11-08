// Buscadores de fuentes: cuáles se consultan y cómo les va.
//
// Cada búsqueda (kodios_search) devuelve, además de las fuentes, cómo respondió
// cada buscador: "ok", "vacio" (respondió pero no tenía ese título), "falla" o
// "timeout". Acá se lleva la cuenta:
//
//  - Apagado a mano: Configuración → Fuentes (config.buscadoresApagados).
//  - En pausa sola: FALLAS_PARA_PAUSA fallas SEGUIDAS → no se consulta durante
//    PAUSA_MS; pasado eso se vuelve a probar, y si falla de nuevo vuelve a la
//    pausa. Un "vacío" no cuenta como falla: no tener una peli no es estar roto.
//
// La salud vive en este equipo (localStorage): depende de la red de cada uno
// (un sitio bloqueado por el operador en Chile puede andar en otro país).

import { invoke } from "@tauri-apps/api/core";
import { config } from "$lib/config.svelte";
import type { Src } from "$lib/fuentes";

export type EstadoSalud = "ok" | "vacio" | "falla" | "timeout";
export type Salud = { id: string; estado: EstadoSalud; n: number; error: string; ms: number };

export type InfoBalandro = {
  activo?: boolean;
  nota?: string;
  host?: string;
  idiomas?: string[];
} | null;

export type BuscadorInfo = {
  id: string;
  nombre: string;
  descripcion: string;
  tipos: string[];
  idioma: string;
  balandro: string | null;
  balandro_info: InfoBalandro;
};

export type Registro = {
  /** Fallas seguidas (se pone en 0 al responder bien). */
  fallas: number;
  /** Última vez que respondió (ok o vacío), en ms epoch. */
  ultimoOk: number;
  /** Fuentes que trajo la última vez que trajo algo. */
  ultimoN: number;
  ultimaFalla: number;
  error: string;
  /** Hasta cuándo no se consulta (0 = no está en pausa). */
  pausaHasta: number;
  ms: number;
};

export const FALLAS_PARA_PAUSA = 3;
export const PAUSA_MS = 6 * 3600_000;
const K = "salud_buscadores";

function leer(): Record<string, Registro> {
  try {
    const v = JSON.parse(localStorage.getItem(K) || "{}");
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

export const salud = $state<Record<string, Registro>>(leer());

function persistir() {
  try {
    localStorage.setItem(K, JSON.stringify(salud));
  } catch {
    /* sin espacio o sin localStorage: la salud vale solo para esta sesión */
  }
}

function registro(id: string): Registro {
  return (salud[id] ??= { fallas: 0, ultimoOk: 0, ultimoN: 0, ultimaFalla: 0, error: "", pausaHasta: 0, ms: 0 });
}

/** Apagados a mano en Configuración. */
export function apagadosAMano(): string[] {
  return (config.buscadoresApagados || "").split(",").map((x) => x.trim()).filter(Boolean);
}

export function encendido(id: string): boolean {
  return !apagadosAMano().includes(id);
}

export function encender(id: string, on: boolean) {
  const resto = apagadosAMano().filter((x) => x !== id);
  config.buscadoresApagados = (on ? resto : [...resto, id]).join(",");
}

export function enPausa(id: string, ahora = Date.now()): boolean {
  return (salud[id]?.pausaHasta ?? 0) > ahora;
}

/** Lo que NO se consulta en esta búsqueda: apagados a mano + en pausa. */
export function apagadas(): string[] {
  const ahora = Date.now();
  const pausa = Object.keys(salud).filter((id) => enPausa(id, ahora));
  return [...new Set([...apagadosAMano(), ...pausa])];
}

/** Anota cómo respondió cada buscador. */
export function anotar(lista: Salud[], ahora = Date.now()) {
  for (const s of lista) {
    const r = registro(s.id);
    r.ms = s.ms;
    if (s.estado === "ok" || s.estado === "vacio") {
      r.fallas = 0;
      r.pausaHasta = 0;
      r.ultimoOk = ahora;
      if (s.estado === "ok") r.ultimoN = s.n;
      r.error = "";
    } else {
      r.fallas += 1;
      r.ultimaFalla = ahora;
      r.error = s.error;
      if (r.fallas >= FALLAS_PARA_PAUSA) r.pausaHasta = ahora + PAUSA_MS;
    }
  }
  persistir();
}

/** Sacar de la pausa a mano ("Probar ya"). */
export function reactivar(id: string) {
  const r = registro(id);
  r.fallas = 0;
  r.pausaHasta = 0;
  persistir();
}

export type PedidoKodios = {
  imdbId: string;
  kind: string;
  title?: string;
  originalTitle?: string;
  season?: number;
  episode?: number;
  kitsuId?: number;
  tmdbId?: number;
  anio?: number;
};

/** Busca fuentes con los buscadores encendidos y anota su salud. */
export async function buscarKodios(p: PedidoKodios): Promise<Src[]> {
  const r = await invoke<{ fuentes: Src[]; salud: Salud[] }>("kodios_search", {
    ...p,
    apagadas: apagadas(),
  });
  anotar(r.salud);
  return r.fuentes;
}

export async function catalogo(): Promise<BuscadorInfo[]> {
  return invoke<BuscadorInfo[]>("fuentes_catalogo");
}
