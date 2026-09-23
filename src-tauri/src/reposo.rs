// Inhibe el reposo del escritorio (bloqueo de pantalla, apagado de monitor,
// suspensión) mientras se reproduce algo.
//
// libmpv embebido con la render API no tiene ventana propia, así que su
// `stop-screensaver` no hace nada: sin esto KDE se duerme a mitad de la
// película porque no sabe que hay algo en pantalla.
//
// Van las dos interfaces freedesktop: ScreenSaver cubre bloqueo y apagado
// del monitor, PowerManagement la suspensión. NO el portal Inhibit: con
// xdg-desktop-portal 1.22 + KDE 6.7 el Request que devuelve ya no existe al
// cerrarlo y la inhibición queda colgada aunque Kütral muera (medido).
//
// Varias fuentes pueden pedirla a la vez (mpv, el iframe web, el IPTV): se
// mantiene mientras al menos una siga activa.

use gio::prelude::*;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

const APP: &str = "Kütral";
const MOTIVO: &str = "Reproduciendo video";
const TIMEOUT_MS: i32 = 5000;

/// (bus, ruta, interfaz) de cada inhibidor. Los dos usan el mismo par
/// Inhibit(app, motivo) → cookie / UnInhibit(cookie).
const INHIBIDORES: [(&str, &str, &str); 2] = [
    ("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver", "org.freedesktop.ScreenSaver"),
    (
        "org.freedesktop.PowerManagement",
        "/org/freedesktop/PowerManagement/Inhibit",
        "org.freedesktop.PowerManagement.Inhibit",
    ),
];

/// Cookie de cada inhibidor que aceptó, en el orden de `INHIBIDORES`.
type Activa = [Option<u32>; 2];

// Dos locks: el de fuentes se toma y suelta al toque desde quien avisa (a
// menudo el hilo de GTK); el de la inhibición se sostiene durante las
// llamadas D-Bus, que pueden tardar, y solo lo toman hilos aparte.
static FUENTES: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static ACTIVA: Mutex<Option<Activa>> = Mutex::new(None);

/// Marca `fuente` como reproduciendo (`on`) o no.
pub fn set(fuente: &str, on: bool) {
    {
        let mut f = FUENTES.lock().unwrap_or_else(|e| e.into_inner());
        let f = f.get_or_insert_with(HashSet::new);
        if on {
            f.insert(fuente.to_string());
        } else {
            f.remove(fuente);
        }
    }
    // El hilo reconcilia contra el conjunto vigente al tomar el lock, no
    // contra este aviso: un off+on seguidos terminan inhibidos sin importar
    // en qué orden corran sus hilos.
    std::thread::spawn(|| {
        let mut activa = ACTIVA.lock().unwrap_or_else(|e| e.into_inner());
        let querida = FUENTES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|f| !f.is_empty());
        match (querida, activa.is_some()) {
            (true, false) => *activa = inhibir(),
            (false, true) => {
                if let Some(a) = activa.take() {
                    liberar(a);
                }
            }
            _ => {}
        }
    });
}

// La conexión se guarda viva: GIO no retiene la de sesión por su cuenta, y
// si se cierra el escritorio suelta la inhibición al ver desconectarse a
// quien la pidió (medido: devolvía cookie y KDE nunca la listaba).
static BUS: OnceLock<gio::DBusConnection> = OnceLock::new();

fn bus() -> Option<gio::DBusConnection> {
    if let Some(c) = BUS.get() {
        return Some(c.clone());
    }
    let c = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(|e| eprintln!("[reposo] sin bus de sesión: {e}"))
        .ok()?;
    Some(BUS.get_or_init(|| c).clone())
}

fn inhibir() -> Option<Activa> {
    let conn = bus()?;
    let mut cookies: Activa = [None; 2];
    for (i, (nombre, ruta, iface)) in INHIBIDORES.iter().enumerate() {
        let r = conn.call_sync(
            Some(nombre),
            ruta,
            iface,
            "Inhibit",
            Some(&(APP, MOTIVO).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            None::<&gio::Cancellable>,
        );
        match r {
            Ok(v) => cookies[i] = v.child_value(0).get::<u32>(),
            Err(e) => eprintln!("[reposo] {iface} no disponible: {e}"),
        }
    }
    eprintln!("[reposo] inhibido {cookies:?}");
    cookies.iter().any(Option::is_some).then_some(cookies)
}

fn liberar(a: Activa) {
    let Some(conn) = bus() else { return };
    for ((nombre, ruta, iface), cookie) in INHIBIDORES.iter().zip(a) {
        let Some(cookie) = cookie else { continue };
        let r = conn.call_sync(
            Some(nombre),
            ruta,
            iface,
            "UnInhibit",
            Some(&(cookie,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            None::<&gio::Cancellable>,
        );
        if let Err(e) = r {
            eprintln!("[reposo] {iface} no se pudo liberar: {e}");
        }
    }
    eprintln!("[reposo] liberado");
}

#[cfg(test)]
mod tests {
    // Manual: pide y suelta la inhibición contra el escritorio real.
    // `cargo test --lib reposo -- --ignored --nocapture` y, mientras duerme,
    // mirar la lista de inhibiciones del escritorio.
    #[test]
    #[ignore]
    fn inhibe_y_libera() {
        let a = super::inhibir().expect("no se pudo inhibir");
        std::thread::sleep(std::time::Duration::from_secs(12));
        super::liberar(a);
    }
}
