// Windows: que la pantalla no se apague mientras se reproduce algo fuera de
// mpv (el iframe web de Descubrir, el IPTV con hls.js). mpv, en su propia
// ventana, ya lo hace solo.
//
// SetThreadExecutionState vale mientras viva el hilo que lo llamó, así que
// va en un hilo propio que dura toda la sesión: no depende de en qué hilo
// corra el comando de Tauri. Varias fuentes pueden pedirlo a la vez; se
// mantiene mientras al menos una siga activa.

use std::collections::HashSet;
use std::sync::{mpsc, Mutex, OnceLock};

const ES_CONTINUOUS: u32 = 0x8000_0000;
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
}

struct Estado {
    fuentes: HashSet<String>,
    hilo: mpsc::Sender<bool>,
}

fn estado() -> &'static Mutex<Estado> {
    static E: OnceLock<Mutex<Estado>> = OnceLock::new();
    E.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<bool>();
        std::thread::spawn(move || {
            for activo in rx {
                let flags = if activo {
                    ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
                } else {
                    ES_CONTINUOUS
                };
                // SAFETY: función de kernel32 sin punteros; solo flags.
                unsafe { SetThreadExecutionState(flags) };
            }
        });
        Mutex::new(Estado { fuentes: HashSet::new(), hilo: tx })
    })
}

pub fn set(fuente: &str, on: bool) {
    let mut e = estado().lock().unwrap_or_else(|e| e.into_inner());
    let antes = !e.fuentes.is_empty();
    if on {
        e.fuentes.insert(fuente.to_string());
    } else {
        e.fuentes.remove(fuente);
    }
    let ahora = !e.fuentes.is_empty();
    if antes != ahora {
        let _ = e.hilo.send(ahora);
    }
}
