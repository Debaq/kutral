// Mando físico para el emulador — captura y mapeo hacia RetroArch.
//
// Kütral ya tiene un mapeo de mando para la INTERFAZ (src/lib/controls.ts,
// índices del Gamepad API del webview). Ese mapeo NO sirve para RetroArch: el
// emulador es otro proceso y lee el mando por su cuenta (driver udev), con una
// numeración propia. Los índices del Gamepad API del navegador no coinciden
// (el navegador mete los gatillos como botones 6/7 y la cruceta como 12..15).
//
// Por eso la captura se hace acá, leyendo /dev/input/eventN directo:
//   - Botón: índice = cuántos códigos de tecla soportados hay por debajo del
//     pulsado, contando desde BTN_MISC. Es exactamente el orden que arma
//     RetroArch en su driver udev (y joydev), así que los números calzan.
//   - Eje: mismo conteo sobre el mapa de ejes, saltando la cruceta (los hats
//     RetroArch los numera aparte).
//   - Cruceta (ABS_HAT0X/Y): va como "h0up" / "h0left"… en la config.
//
// El mapa se guarda en app_config_dir/mando_juegos.json y emu.rs lo vuelca al
// config temporal que le pasa a RetroArch por --appendconfig.
//
// Solo Linux: en otras plataformas los comandos responden un error claro y
// `cfg_lines` devuelve vacío (RetroArch usa su autoconfig de siempre).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Botones del retropad que se pueden reasignar, en el nombre que usa la
/// config de RetroArch (`input_player1_<accion>_btn`).
pub const ACCIONES: &[&str] = &[
    "a", "b", "x", "y", "l", "r", "l2", "r2", "select", "start", "up", "down", "left", "right",
];

/// A qué entrada física quedó atado un botón del retropad.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bind {
    /// Botón del mando. `n` es el índice como lo cuenta RetroArch.
    Btn { n: i32 },
    /// Eje analógico. `dir` es -1 o +1 (mitad del recorrido).
    Axis { n: i32, dir: i32 },
    /// Cruceta digital. `dir` ∈ up|down|left|right.
    Hat { n: i32, dir: String },
}

impl Bind {
    /// (sufijo de la clave, valor) tal como los escribe retroarch.cfg.
    fn cfg(&self) -> (&'static str, String) {
        match self {
            Bind::Btn { n } => ("btn", n.to_string()),
            Bind::Axis { n, dir } => ("axis", format!("{}{}", if *dir < 0 { "-" } else { "+" }, n)),
            Bind::Hat { n, dir } => ("btn", format!("h{n}{dir}")),
        }
    }

}

/// Mapa completo guardado en disco.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PadMap {
    /// Ruta del /dev/input/eventN que se usó para capturar (informativo).
    #[serde(default)]
    pub device: String,
    /// Nombre del mando, para mostrarlo en Configuración.
    #[serde(default)]
    pub name: String,
    /// Puerto del mando en RetroArch (0 = el primero).
    #[serde(default)]
    pub pad_index: i32,
    /// accion del retropad → entrada física.
    #[serde(default)]
    pub binds: BTreeMap<String, Bind>,
}

/// Un mando detectado.
#[derive(Serialize, Clone, Debug)]
pub struct PadDevice {
    pub path: String,
    pub name: String,
    /// Orden entre los mandos detectados: el mismo criterio que usa RetroArch
    /// para numerar puertos si no hay nada más enchufado.
    pub index: i32,
}

fn map_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir config: {e}"))?;
    Ok(dir.join("mando_juegos.json"))
}

fn load(app: &tauri::AppHandle) -> PadMap {
    let Ok(p) = map_path(app) else {
        return PadMap::default();
    };
    let Ok(txt) = std::fs::read_to_string(p) else {
        return PadMap::default();
    };
    serde_json::from_str(&txt).unwrap_or_default()
}

/// Líneas de retroarch.cfg para el mapa guardado. Vacío si no hay mapa: ahí
/// RetroArch usa su autoconfig, que es lo que hacía Kütral siempre.
pub fn cfg_lines(app: &tauri::AppHandle) -> String {
    let map = load(app);
    if map.binds.is_empty() {
        return String::new();
    }
    // Sin esto el autoconfig del mando pisa los binds del config al conectar.
    let mut out = String::from("input_autodetect_enable = \"false\"\n");
    out.push_str(&format!(
        "input_player1_joypad_index = \"{}\"\n",
        map.pad_index
    ));
    for accion in ACCIONES {
        if let Some(b) = map.binds.get(*accion) {
            let (suf, val) = b.cfg();
            out.push_str(&format!("input_player1_{accion}_{suf} = \"{val}\"\n"));
        }
    }
    // Salida de emergencia: Select + Start cierran RetroArch. Sin esto, con un
    // mapa propio y sin teclado no hay forma de salir del juego.
    if let (Some(Bind::Btn { n: sel }), Some(Bind::Btn { n: sta })) =
        (map.binds.get("select"), map.binds.get("start"))
    {
        out.push_str(&format!("input_enable_hotkey_btn = \"{sel}\"\n"));
        out.push_str(&format!("input_exit_emulator_btn = \"{sta}\"\n"));
    }
    out
}

#[tauri::command]
pub fn pad_map_get(app: tauri::AppHandle) -> PadMap {
    load(&app)
}

#[tauri::command]
pub fn pad_map_set(app: tauri::AppHandle, map: PadMap) -> Result<(), String> {
    let p = map_path(&app)?;
    let txt = serde_json::to_string_pretty(&map).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(p, txt).map_err(|e| format!("write mando_juegos.json: {e}"))
}

/// Borra el mapa: RetroArch vuelve a su autoconfig.
#[tauri::command]
pub fn pad_map_clear(app: tauri::AppHandle) -> Result<(), String> {
    let p = map_path(&app)?;
    match std::fs::remove_file(p) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("borrar mando_juegos.json: {e}")),
    }
}

// ========================================================================
// Linux: lectura directa de /dev/input
// ========================================================================
#[cfg(target_os = "linux")]
mod linux {
    use super::{Bind, PadDevice};

    const EV_KEY: u16 = 1;
    const EV_ABS: u16 = 3;
    const BTN_MISC: usize = 0x100;
    const BTN_JOYSTICK: usize = 0x120;
    const BTN_GAMEPAD: usize = 0x130;
    const ABS_HAT0X: u16 = 0x10;
    const ABS_HAT3Y: u16 = 0x17;
    /// input_event en 64 bits: timeval(16) + type(2) + code(2) + value(4).
    const EVENT_SIZE: usize = 24;

    /// "0 0 ... ffff000000000000" → words[0] = bits 0..63.
    pub(super) fn parse_bits(s: &str) -> Vec<u64> {
        let mut w: Vec<u64> = s
            .split_whitespace()
            .map(|x| u64::from_str_radix(x, 16).unwrap_or(0))
            .collect();
        w.reverse();
        w
    }

    fn bit(w: &[u64], i: usize) -> bool {
        w.get(i / 64).is_some_and(|x| (x >> (i % 64)) & 1 == 1)
    }

    /// Cuántos bits prendidos hay en [desde, hasta). Es el índice que le toca a
    /// `hasta` en el orden ascendente que arma RetroArch.
    pub(super) fn indice(w: &[u64], desde: usize, hasta: usize) -> i32 {
        (desde..hasta).filter(|i| bit(w, *i)).count() as i32
    }

    fn sysfs(dev: &str, archivo: &str) -> Option<String> {
        let ev = std::path::Path::new(dev).file_name()?.to_str()?;
        let p = format!("/sys/class/input/{ev}/device/{archivo}");
        std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }

    fn caps(dev: &str, cual: &str) -> Vec<u64> {
        sysfs(dev, &format!("capabilities/{cual}"))
            .map(|s| parse_bits(&s))
            .unwrap_or_default()
    }

    /// Mandos conectados, en orden de eventN (el mismo que ve RetroArch).
    pub fn devices() -> Vec<PadDevice> {
        let mut evs: Vec<(u32, String)> = Vec::new();
        let Ok(rd) = std::fs::read_dir("/dev/input") else {
            return Vec::new();
        };
        for e in rd.flatten() {
            let path = e.path();
            let Some(nombre) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(num) = nombre.strip_prefix("event").and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let p = path.to_string_lossy().to_string();
            let key = caps(&p, "key");
            // Un mando declara BTN_GAMEPAD o BTN_JOYSTICK. Sin esto entraban
            // teclados, mouses y el botón de encendido.
            if bit(&key, BTN_GAMEPAD) || bit(&key, BTN_JOYSTICK) {
                evs.push((num, p));
            }
        }
        evs.sort_by_key(|(n, _)| *n);
        evs.into_iter()
            .enumerate()
            .map(|(i, (_, path))| PadDevice {
                name: sysfs(&path, "name").unwrap_or_else(|| "Mando".into()),
                path,
                index: i as i32,
            })
            .collect()
    }

    /// Rango del eje vía ioctl EVIOCGABS: sin él no se sabe cuánto es "medio
    /// recorrido" (un stick va -32768..32767 y un gatillo 0..255).
    fn abs_rango(fd: i32, code: u16) -> Option<(i32, i32)> {
        // struct input_absinfo = 6 x i32 (value, min, max, fuzz, flat, res).
        let mut info = [0i32; 6];
        // _IOR('E', 0x40 + code, struct input_absinfo) con size = 24.
        let req: libc::c_ulong = 0x8018_4540 + code as libc::c_ulong;
        let r = unsafe { libc::ioctl(fd, req, info.as_mut_ptr()) };
        if r < 0 {
            return None;
        }
        Some((info[1], info[2]))
    }

    /// Espera a que se pulse algo en `dev` y devuelve a qué entrada equivale.
    /// `None` si se acabó el tiempo.
    pub fn capture(dev: &str, timeout_ms: u64) -> Result<Option<Bind>, String> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;

        let key = caps(dev, "key");
        let abs = caps(dev, "abs");
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(dev)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    "sin permiso para leer el mando: agrega tu usuario al grupo `input`".to_string()
                }
                _ => format!("abrir {dev}: {e}"),
            })?;
        let fd = f.as_raw_fd();

        let hasta = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        let mut buf = [0u8; EVENT_SIZE];
        while std::time::Instant::now() < hasta {
            match f.read(&mut buf) {
                Ok(n) if n == EVENT_SIZE => {
                    let tipo = u16::from_ne_bytes([buf[16], buf[17]]);
                    let code = u16::from_ne_bytes([buf[18], buf[19]]);
                    let val = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);

                    if tipo == EV_KEY && val == 1 && (code as usize) >= BTN_MISC {
                        return Ok(Some(Bind::Btn {
                            n: indice(&key, BTN_MISC, code as usize),
                        }));
                    }
                    if tipo == EV_ABS {
                        if (ABS_HAT0X..=ABS_HAT3Y).contains(&code) {
                            if val == 0 {
                                continue; // soltar la cruceta no asigna nada
                            }
                            let eje_x = (code - ABS_HAT0X) % 2 == 0;
                            let dir = match (eje_x, val < 0) {
                                (true, true) => "left",
                                (true, false) => "right",
                                (false, true) => "up",
                                (false, false) => "down",
                            };
                            return Ok(Some(Bind::Hat {
                                n: ((code - ABS_HAT0X) / 2) as i32,
                                dir: dir.to_string(),
                            }));
                        }
                        // Eje analógico: pedimos medio recorrido para no tomar
                        // el reposo ni el ruido de un stick sucio.
                        let (min, max) = abs_rango(fd, code).unwrap_or((-32768, 32767));
                        let centro = (min + max) / 2;
                        let umbral = ((max - min) as f64 * 0.35) as i32;
                        if umbral > 0 && (val - centro).abs() >= umbral {
                            // Los hats no cuentan como ejes en RetroArch.
                            let n = (0..code as usize)
                                .filter(|i| {
                                    !(ABS_HAT0X as usize..=ABS_HAT3Y as usize).contains(i)
                                        && bit(&abs, *i)
                                })
                                .count() as i32;
                            return Ok(Some(Bind::Axis {
                                n,
                                dir: if val < centro { -1 } else { 1 },
                            }));
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
                Err(e) => return Err(format!("leer {dev}: {e}")),
            }
        }
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn pad_devices() -> Vec<PadDevice> {
    linux::devices()
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn pad_capture(device: String, timeout_ms: u64) -> Result<Option<Bind>, String> {
    // Bloqueante (espera a que el usuario apriete): fuera del hilo principal o
    // se congela la UI mientras dura la captura.
    tauri::async_runtime::spawn_blocking(move || linux::capture(&device, timeout_ms))
        .await
        .map_err(|e| format!("join: {e}"))?
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn pad_devices() -> Vec<PadDevice> {
    Vec::new()
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn pad_capture(_device: String, _timeout_ms: u64) -> Result<Option<Bind>, String> {
    Err("la configuración del mando solo está disponible en Linux".into())
}

// ========================================================================
// Tests de la numeración: es lo único delicado del módulo. Si los índices no
// calzan con los que arma RetroArch, el remapeo queda mudo.
// ========================================================================
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Mando tipo xbox: BTN_A..BTN_Y, BTN_TL/TR, BTN_SELECT/START/MODE.
    fn key_caps() -> String {
        let codes: [usize; 9] = [
            0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x13a, 0x13b, 0x13c,
        ];
        let mut words = [0u64; 6];
        for c in codes {
            words[c / 64] |= 1 << (c % 64);
        }
        let mut w: Vec<String> = words.iter().map(|x| format!("{x:x}")).collect();
        w.reverse(); // sysfs imprime la palabra más alta primero
        w.join(" ")
    }

    #[test]
    fn indices_de_botones_en_orden_de_keycode() {
        let bits = linux::parse_bits(&key_caps());
        // BTN_A es el primero → 0; BTN_B → 1; BTN_X (0x133) → 2 aunque haya un
        // hueco en 0x132 (BTN_C, que este mando no tiene).
        assert_eq!(linux::indice(&bits, 0x100, 0x130), 0);
        assert_eq!(linux::indice(&bits, 0x100, 0x131), 1);
        assert_eq!(linux::indice(&bits, 0x100, 0x133), 2);
        assert_eq!(linux::indice(&bits, 0x100, 0x13c), 8);
    }

    #[test]
    fn cfg_de_cada_tipo_de_bind() {
        assert_eq!(Bind::Btn { n: 3 }.cfg(), ("btn", "3".to_string()));
        assert_eq!(
            Bind::Axis { n: 2, dir: -1 }.cfg(),
            ("axis", "-2".to_string())
        );
        assert_eq!(
            Bind::Hat {
                n: 0,
                dir: "up".into()
            }
            .cfg(),
            ("btn", "h0up".to_string())
        );
    }
}
