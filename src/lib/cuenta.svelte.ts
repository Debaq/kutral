// Cuenta opcional en un servidor propio (server/ del repo, ver cuenta.rs).
//
// Sin cuenta Kutral funciona igual. Con cuenta, favoritos, historial (con el
// minuto donde quedaste), listas, y en la bóveda cifrada las claves de API,
// ajustes y vinculaciones (RD, OpenSubtitles) se sincronizan entre equipos: al abrir la
// app, cada 5 minutos y unos segundos después de cada cambio local (en una
// reproducción, al detenerla: no en cada guardado de progreso). El token
// nunca pasa por aquí: lo guarda Rust.

import { invoke } from "@tauri-apps/api/core";
import { cargarHistorial } from "$lib/historial.svelte";
import { cargarListas, crearLista, agregarALista, itemsDe, nombreDe, listas } from "$lib/listas.svelte";
import { cargarPerfiles } from "$lib/perfiles.svelte";
import { notify } from "$lib/notifStore.svelte";
import {
  refreshRdLinked,
  refreshOsStatus,
  ajustesDeCuenta,
  aplicarAjustesDeCuenta,
} from "$lib/config.svelte";
import type { RdDeviceStart } from "$lib/tipos";

type EstadoCuenta = {
  servidor: string;
  usuario: string | null;
  ultima_sync: number;
  boveda: boolean;
};

type ResultadoSync = { subidos: number; bajados: number };

type ResultadoBoveda = {
  ajustes: Record<string, string | null> | null;
  credenciales: boolean;
  bloqueada: boolean;
};

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

/** Una lista compartida que llega a la bandeja: viaja como su código. */
export type ListaRecomendada = {
  tipo: "lista";
  codigo: string;
  nombre: string;
  cantidad: number;
  poster_path?: string | null;
};

export type ListaCompartida = { codigo: string; lista_id: string; nombre: string; actualizada: number };

export type ListaPublica = ListaCompartida & {
  de: string;
  items: { clave: string; tmdb_id: number; media_type: string; title: string; poster_path: string | null }[];
};

export function esLista(d: TituloRecomendado | ListaRecomendada): d is ListaRecomendada {
  return (d as ListaRecomendada).tipo === "lista";
}

export type Recomendacion = {
  id: number;
  de: string;
  datos: TituloRecomendado | ListaRecomendada;
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
  ultimaSync: 0,
  /** Hay llave de la bóveda aquí. Sin ella (sesión vieja) se pide la contraseña. */
  boveda: true,
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
  /** Las listas propias que se comparten por código. */
  publicas: [] as ListaCompartida[],
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
  cuenta.ultimaSync = e.ultima_sync;
  cuenta.boveda = e.boveda;
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
  let ok = false;
  try {
    const r = await invoke<ResultadoSync>("cuenta_sincronizar");
    ok = true;
    cuenta.error = "";
    if (r.bajados > 0) {
      // Primero los perfiles: si borraron en otro equipo el que está activo,
      // se vuelve al principal antes de recargar lo demás.
      await cargarPerfiles();
      await cargarHistorial();
      await cargarListas();
    }
    await sincronizarBoveda();
  } catch (e) {
    cuenta.error = String(e);
  } finally {
    cuenta.sincronizando = false;
    // Refresca hora de la última sync, o refleja una sesión cerrada desde otro equipo.
    await cargarCuenta();
  }
  await cargarSocial();
  // Solo recién sincronizado: así las listas de aquí están al día y no se
  // publica una versión vieja encima de la de otro equipo.
  if (ok) await actualizarPublicadas();
}

/** Claves de API, ajustes y vinculaciones, cifrados con la contraseña (boveda.rs). */
async function sincronizarBoveda(): Promise<void> {
  const r = await invoke<ResultadoBoveda>("cuenta_boveda", { ajustes: ajustesDeCuenta() });
  if (r.ajustes) aplicarAjustesDeCuenta(r.ajustes);
  if (r.credenciales) {
    await refreshRdLinked();
    await refreshOsStatus();
  }
}

/** Sesión de antes de la bóveda: con la contraseña ya se puede sincronizar. */
export async function desbloquearCuenta(clave: string): Promise<void> {
  aplicar(await invoke<EstadoCuenta>("cuenta_desbloquear", { clave }));
  await sincronizarCuenta();
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
  social.publicas = [];
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
    const que = esLista(r.datos) ? `la lista «${r.datos.nombre}»` : r.datos.title;
    notify("info", `${r.de} te recomendó ${que}`, r.nota ?? "Está en Lo que viste → De amigos.");
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
    social.publicas = (await invoke<{ publicas: ListaCompartida[] }>("cuenta_mis_publicas")).publicas;
    social.error = "";
    avisarNuevas(r.recomendaciones);
  } catch (e) {
    social.error = String(e);
  }
  await cargarLlavero();
}

// ─── Listas compartidas ─────────────────────────────────────────────────────

// lista_id → huella (nombre + títulos en orden) de lo último que se publicó
// desde este equipo. Si la lista cambió, se vuelve a publicar al sincronizar.
const HUELLAS = "kutral_listas_publicadas";

function huella(listaId: string): string {
  return JSON.stringify([nombreDe(listaId), ...itemsDe(listaId).map((it) => it.clave)]);
}

function huellas(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(HUELLAS) ?? "{}");
  } catch {
    return {};
  }
}

function anotarHuella(listaId: string, h: string | null): void {
  const hs = huellas();
  if (h === null) delete hs[listaId];
  else hs[listaId] = h;
  localStorage.setItem(HUELLAS, JSON.stringify(hs));
}

export function codigoDe(listaId: string): string | null {
  return social.publicas.find((p) => p.lista_id === listaId)?.codigo ?? null;
}

async function publicar(listaId: string): Promise<ListaCompartida> {
  const items = itemsDe(listaId).map((it) => ({
    clave: it.clave,
    tmdb_id: it.tmdb_id,
    media_type: it.media_type,
    title: it.title,
    poster_path: it.poster_path,
  }));
  const p = await invoke<ListaCompartida>("cuenta_publicar_lista", {
    listaId,
    nombre: nombreDe(listaId),
    items,
  });
  anotarHuella(listaId, huella(listaId));
  social.publicas = [...social.publicas.filter((x) => x.lista_id !== listaId), p];
  return p;
}

/** Comparte la lista: devuelve su código (el mismo si ya estaba compartida). */
export async function compartirLista(listaId: string): Promise<string> {
  return (await publicar(listaId)).codigo;
}

export async function dejarDeCompartir(listaId: string): Promise<void> {
  const codigo = codigoDe(listaId);
  if (!codigo) return;
  const r = await invoke<{ publicas: ListaCompartida[] }>("cuenta_despublicar_lista", { codigo });
  social.publicas = r.publicas;
  anotarHuella(listaId, null);
}

// Las listas compartidas que cambiaron aquí se vuelven a subir. Una que aquí
// no existe no se toca: puede ser de otro equipo y todavía no haber llegado
// (borrar una lista la deja de compartir explícitamente, ver historial).
async function actualizarPublicadas(): Promise<void> {
  if (!listas.cargadas) return;
  const hs = huellas();
  for (const p of social.publicas) {
    try {
      if (listas.todas.some((l) => l.id === p.lista_id) && hs[p.lista_id] !== huella(p.lista_id)) {
        await publicar(p.lista_id);
      }
    } catch (e) {
      console.warn("[cuenta] actualizar lista compartida:", e);
    }
  }
}

export async function verListaPublica(codigo: string): Promise<ListaPublica> {
  return invoke<ListaPublica>("cuenta_lista_publica", { codigo });
}

/** Copia una lista compartida a las propias. Devuelve el id de la copia. */
export async function importarLista(codigo: string): Promise<string> {
  const l = await verListaPublica(codigo);
  const id = await crearLista(l.nombre);
  if (!id) throw new Error("no se pudo crear la lista");
  for (const it of l.items) {
    await agregarALista(id, {
      clave: it.clave,
      tmdbId: it.tmdb_id,
      mediaType: it.media_type,
      title: it.title,
      posterPath: it.poster_path,
    });
  }
  return id;
}

/** Comparte la lista (si hacía falta) y se la manda a la bandeja de cada amigo. */
export async function enviarLista(listaId: string, para: string[], nota: string): Promise<void> {
  const codigo = await compartirLista(listaId);
  const its = itemsDe(listaId);
  await recomendar(
    para,
    {
      tipo: "lista",
      codigo,
      nombre: nombreDe(listaId),
      cantidad: its.length,
      poster_path: its.find((it) => it.poster_path)?.poster_path ?? null,
    },
    nota,
  );
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

export async function recomendar(
  para: string[],
  datos: TituloRecomendado | ListaRecomendada,
  nota: string,
): Promise<void> {
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
