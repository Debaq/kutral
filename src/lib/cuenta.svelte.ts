// Cuenta opcional en un servidor propio (server/ del repo, ver cuenta.rs).
//
// Sin cuenta Kutral funciona igual. Con cuenta, favoritos, historial (con el
// minuto donde quedaste) y listas se sincronizan entre equipos: al abrir la
// app, cada 5 minutos y unos segundos después de cada cambio local (en una
// reproducción, al detenerla: no en cada guardado de progreso). El token
// nunca pasa por aquí: lo guarda Rust.

import { invoke } from "@tauri-apps/api/core";
import { cargarHistorial } from "$lib/historial.svelte";
import { cargarListas } from "$lib/listas.svelte";
import { notify } from "$lib/notifStore.svelte";
import { refreshRdLinked } from "$lib/config.svelte";
import type { RdDeviceStart } from "$lib/tipos";

type EstadoCuenta = {
  servidor: string;
  usuario: string | null;
  perfil: string | null;
  ultima_sync: number;
};

type ResultadoSync = { subidos: number; bajados: number };

export type Amigo = { nombre: string; desde: number };
type Amigos = { amigos: Amigo[]; recibidas: Amigo[]; enviadas: Amigo[] };

/** Lo que viaja en una recomendación: lo justo para mostrar la tarjeta y abrir el título. */
export type TituloRecomendado = {
  tmdb_id: number;
  media_type: string;
  title: string;
  poster_path?: string | null;
  year?: number | null;
};

export type Recomendacion = {
  id: number;
  de: string;
  datos: TituloRecomendado;
  nota: string | null;
  creada: number;
  vista: boolean;
};

// Lo despachan quienes cambian algo sincronizable: historial.svelte.ts,
// listas.svelte.ts y HistorialTracker (al detener la reproducción).
const EVENTO_CAMBIO_LOCAL = "kutral:cambio-local";

const CADA = 5 * 60_000;
const ESPERA_TRAS_CAMBIO = 3_000;

export const cuenta = $state({
  servidor: "",
  usuario: null as string | null,
  perfil: null as string | null,
  ultimaSync: 0,
  sincronizando: false,
  /** Último error de sincronización (vacío si la última salió bien). */
  error: "",
});

/** Amigos y bandeja de recomendaciones. Se refrescan junto con cada sincronización. */
export const social = $state({
  amigos: [] as Amigo[],
  recibidas: [] as Amigo[],
  enviadas: [] as Amigo[],
  recomendaciones: [] as Recomendacion[],
  error: "",
});

export type Prestamo = { nombre: string; desde: number; ultimo_uso: number | null };

type EstadoLlavero = {
  mias: { rd?: { creada: number; prestada_a: Prestamo[] } };
  prestadas: { servicio: string; de: string; desde: number }[];
  rd_propio: boolean;
};

/** Llavero: el Real-Debrid que prestas a tus amigos y el que te prestan. */
export const llavero = $state({
  /** Hay RD vinculado en este equipo (propio). */
  rdPropio: false,
  /** Quién te presta RD, si alguien lo hace. Se usa solo sin RD propio. */
  rdDe: null as string | null,
  /** Tu vinculación para prestar; null = no prestas. */
  rdMia: null as { creada: number; prestada_a: Prestamo[] } | null,
  error: "",
});

function aplicarLlavero(e: EstadoLlavero): void {
  llavero.rdPropio = e.rd_propio;
  llavero.rdDe = e.prestadas.find((p) => p.servicio === "rd")?.de ?? null;
  llavero.rdMia = e.mias.rd ?? null;
}

function aplicar(e: EstadoCuenta): void {
  cuenta.servidor = e.servidor;
  cuenta.usuario = e.usuario;
  cuenta.perfil = e.perfil;
  cuenta.ultimaSync = e.ultima_sync;
}

export async function cargarCuenta(): Promise<void> {
  try {
    aplicar(await invoke<EstadoCuenta>("cuenta_estado"));
  } catch (e) {
    console.warn("[cuenta] estado:", e);
  }
}

export async function sincronizarCuenta(): Promise<void> {
  if (!cuenta.usuario || cuenta.sincronizando) return;
  cuenta.sincronizando = true;
  try {
    const r = await invoke<ResultadoSync>("cuenta_sincronizar");
    cuenta.error = "";
    if (r.bajados > 0) {
      await cargarHistorial();
      await cargarListas();
    }
  } catch (e) {
    cuenta.error = String(e);
  } finally {
    cuenta.sincronizando = false;
    // Refresca hora de la última sync, o refleja una sesión cerrada desde otro equipo.
    await cargarCuenta();
  }
  await cargarSocial();
}

// ─── Amigos y recomendaciones ───────────────────────────────────────────────

// Id de la última recomendación ya avisada con una notificación: sin esto,
// cada arranque volvería a avisar todo lo no visto.
const ULTIMA_AVISADA = "kutral_recomendacion_avisada";

function vaciarSocial(): void {
  social.amigos = [];
  social.recibidas = [];
  social.enviadas = [];
  social.recomendaciones = [];
  social.error = "";
}

function aplicarAmigos(a: Amigos): void {
  social.amigos = a.amigos;
  social.recibidas = a.recibidas;
  social.enviadas = a.enviadas;
}

function avisarNuevas(recs: Recomendacion[]): void {
  const ultima = Number(localStorage.getItem(ULTIMA_AVISADA) ?? 0);
  const nuevas = recs.filter((r) => !r.vista && r.id > ultima);
  if (!nuevas.length) return;
  localStorage.setItem(ULTIMA_AVISADA, String(Math.max(...nuevas.map((r) => r.id))));
  if (nuevas.length === 1) {
    const r = nuevas[0];
    notify("info", `${r.de} te recomendó ${r.datos.title}`, r.nota ?? "Está en Lo que viste → De amigos.");
  } else {
    notify("info", `Tienes ${nuevas.length} recomendaciones nuevas`, "Están en Lo que viste → De amigos.");
  }
}

export async function cargarSocial(): Promise<void> {
  if (!cuenta.usuario) {
    vaciarSocial();
    return;
  }
  try {
    aplicarAmigos(await invoke<Amigos>("cuenta_amigos"));
    const r = await invoke<{ recomendaciones: Recomendacion[] }>("cuenta_recomendaciones");
    social.recomendaciones = r.recomendaciones;
    social.error = "";
    avisarNuevas(r.recomendaciones);
  } catch (e) {
    social.error = String(e);
  }
  await cargarLlavero();
}

// ─── Llavero ────────────────────────────────────────────────────────────────

export async function cargarLlavero(): Promise<void> {
  try {
    aplicarLlavero(await invoke<EstadoLlavero>("llavero_estado"));
    llavero.error = "";
  } catch (e) {
    llavero.error = String(e);
  }
  // Un préstamo nuevo (o uno que se cortó) cambia si hay RD para usar.
  await refreshRdLinked();
}

/** Pide un código de Real-Debrid solo para el llavero (no toca el de este equipo). */
export async function llaveroCodigo(): Promise<RdDeviceStart> {
  return invoke<RdDeviceStart>("rd_device_start");
}

/** Espera a que autorices el código y sube esa vinculación al servidor. */
export async function llaveroVincular(start: RdDeviceStart): Promise<void> {
  await invoke("llavero_vincular", {
    deviceCode: start.device_code,
    interval: start.interval,
    expiresIn: start.expires_in,
  });
  await cargarLlavero();
}

export async function llaveroBorrar(): Promise<void> {
  await invoke("llavero_borrar");
  await cargarLlavero();
}

export async function llaveroPrestar(nombre: string): Promise<void> {
  await invoke("llavero_prestar", { nombre });
  await cargarLlavero();
}

export async function llaveroDejarDePrestar(nombre: string): Promise<void> {
  await invoke("llavero_dejar_de_prestar", { nombre });
  await cargarLlavero();
}

/** Pide la amistad, o la acepta si esa persona ya te la pidió. */
export async function agregarAmigo(nombre: string): Promise<void> {
  aplicarAmigos(await invoke<Amigos>("cuenta_amigo_agregar", { nombre }));
}

/** Rechaza una solicitud, cancela una enviada o quita a un amigo. */
export async function quitarAmigo(nombre: string): Promise<void> {
  aplicarAmigos(await invoke<Amigos>("cuenta_amigo_quitar", { nombre }));
}

export async function recomendar(para: string[], datos: TituloRecomendado, nota: string): Promise<void> {
  await invoke("cuenta_recomendar", { para, datos, nota: nota.trim() || null });
}

export function recomendacionesSinVer(): number {
  return social.recomendaciones.filter((r) => !r.vista).length;
}

export async function marcarRecomendacionesVistas(): Promise<void> {
  if (!recomendacionesSinVer()) return;
  try {
    await invoke("cuenta_recomendaciones_vistas");
    social.recomendaciones = social.recomendaciones.map((r) => ({ ...r, vista: true }));
  } catch (e) {
    console.warn("[cuenta] marcar vistas:", e);
  }
}

export async function quitarRecomendacion(id: number): Promise<void> {
  await invoke("cuenta_recomendacion_quitar", { id });
  social.recomendaciones = social.recomendaciones.filter((r) => r.id !== id);
}

let espera: ReturnType<typeof setTimeout> | null = null;

function programarSync(): void {
  if (!cuenta.usuario) return;
  if (espera) clearTimeout(espera);
  espera = setTimeout(() => {
    espera = null;
    void sincronizarCuenta();
  }, ESPERA_TRAS_CAMBIO);
}

let iniciada = false;

/** Una vez al arrancar, con la DB ya migrada (después de cargarHistorial). */
export async function iniciarCuenta(): Promise<void> {
  if (iniciada) return;
  iniciada = true;
  await cargarCuenta();
  void sincronizarCuenta();
  setInterval(() => void sincronizarCuenta(), CADA);
  window.addEventListener(EVENTO_CAMBIO_LOCAL, programarSync);
  // Volvió la red: lo anotado sin conexión sube ya, sin esperar el turno.
  window.addEventListener("online", programarSync);
}

export async function probarServidor(servidor: string): Promise<number> {
  return invoke<number>("cuenta_probar", { servidor });
}

/** Entra, o crea la cuenta si viene invitación. Sincroniza de inmediato. */
export async function entrarCuenta(
  servidor: string,
  nombre: string,
  clave: string,
  invitacion?: string,
): Promise<void> {
  aplicar(
    await invoke<EstadoCuenta>("cuenta_entrar", {
      servidor,
      nombre,
      clave,
      invitacion: invitacion || null,
    }),
  );
  await sincronizarCuenta();
}

export async function salirCuenta(): Promise<void> {
  await invoke("cuenta_salir");
  cuenta.error = "";
  await cargarCuenta();
  vaciarSocial();
  await cargarLlavero();
}
