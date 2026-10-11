// Cuenta opcional en un servidor propio (server/ del repo, ver cuenta.rs).
//
// Sin cuenta Kutral funciona igual. Con cuenta, los favoritos se sincronizan
// entre equipos: al abrir la app, cada 5 minutos y unos segundos después de
// cada cambio local. El token nunca pasa por aquí: lo guarda Rust.

import { invoke } from "@tauri-apps/api/core";
import { cargarHistorial } from "$lib/historial.svelte";

type EstadoCuenta = {
  servidor: string;
  usuario: string | null;
  perfil: string | null;
  ultima_sync: number;
};

type ResultadoSync = { subidos: number; bajados: number };

// Lo despacha quien cambia algo sincronizable (hoy: historial.svelte.ts).
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
    if (r.bajados > 0) await cargarHistorial();
  } catch (e) {
    cuenta.error = String(e);
  } finally {
    cuenta.sincronizando = false;
    // Refresca hora de la última sync, o refleja una sesión cerrada desde otro equipo.
    await cargarCuenta();
  }
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
}
