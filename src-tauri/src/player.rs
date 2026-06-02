// Player mpv — reproduce la URL directa de RealDebrid.
//
// El webview (WebKitGTK/WebView2) no reproduce mkv/HEVC/AV1, así que
// spawneamos mpv fullscreen sobre kutral y lo controlamos por su socket IPC
// JSON.
//
// Cross-platform:
//   - Linux/Mac → socket Unix (/tmp/kutral-mpv.sock)
//   - Windows   → named pipe (\\.\pipe\kutral-mpv)
//
// Requisito: mpv en el PATH (o bundleado). En la ISO viene incluido; en la
// build Windows el instalador debe traerlo o pedirlo.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::Child;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

/// Estado del player: el proceso mpv vivo (si hay). Cross-platform.
#[derive(Default)]
pub struct PlayerState {
    pub child: Mutex<Option<Child>>,
}

/// Ruta IPC que se pasa a `--input-ipc-server` y a la que nos conectamos.
fn ipc_path() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\kutral-mpv".to_string()
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join("kutral-mpv.sock")
            .to_string_lossy()
            .into_owned()
    }
}

/// Abre la conexión al IPC de mpv (socket Unix o named pipe Windows).
fn connect_ipc() -> Result<Box<dyn Write>, String> {
    #[cfg(windows)]
    {
        // En Windows un named pipe se abre como archivo R/W.
        use std::fs::OpenOptions;
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(ipc_path())
            .map_err(|e| format!("pipe: {e}"))?;
        Ok(Box::new(f))
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::net::UnixStream;
        let s = UnixStream::connect(ipc_path()).map_err(|e| format!("socket: {e}"))?;
        Ok(Box::new(s))
    }
}

/// Lanza mpv fullscreen con la URL. Mata cualquier mpv previo.
#[tauri::command]
pub fn mpv_play(
    app: tauri::AppHandle,
    state: tauri::State<'_, PlayerState>,
    url: String,
    title: Option<String>,
    start_secs: Option<u64>,
) -> Result<(), String> {
    use std::process::Command;

    if url.is_empty() {
        return Err("url vacía".into());
    }

    // Cerrar mpv anterior si quedó vivo.
    kill_existing(&state);

    // En Unix limpiamos el socket viejo; en Windows el pipe se recrea solo.
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(ipc_path());

    // input.conf: Esc y Backspace cierran mpv (por defecto Esc solo sale de
    // fullscreen). Así el usuario vuelve a kutral con esas teclas.
    let conf = std::env::temp_dir().join("kutral-mpv-input.conf");
    let _ = std::fs::write(&conf, "ESC quit\nBS quit\nq quit\n");

    let mut cmd = Command::new("mpv");
    cmd.arg(format!("--input-ipc-server={}", ipc_path()))
        .arg(format!("--input-conf={}", conf.display()))
        .arg("--fullscreen")
        .arg("--force-window=immediate")
        .arg("--no-terminal")
        .arg("--really-quiet")
        .arg("--hwdec=auto-safe") // HDR/AV1 por hardware si se puede
        .arg("--keep-open=no");

    if let Some(t) = &title {
        cmd.arg(format!("--force-media-title={t}"));
    }
    if let Some(s) = start_secs {
        if s > 0 {
            cmd.arg(format!("--start=+{s}"));
        }
    }
    cmd.arg(&url);

    eprintln!("[mpv] spawn fullscreen url={url}");
    let child = cmd.spawn().map_err(|e| {
        let msg = if e.kind() == std::io::ErrorKind::NotFound {
            "mpv no está instalado (falta en el PATH)".to_string()
        } else {
            format!("spawn mpv: {e}")
        };
        eprintln!("[mpv] ERROR: {msg}");
        msg
    })?;

    eprintln!("[mpv] pid={:?} lanzado", child.id());
    *state.child.lock().unwrap() = Some(child);
    // Avisar al front: ya hay mpv → muestra OSD global y empieza a sondear.
    let _ = app.emit("mpv:state", true);
    Ok(())
}

/// Envía un comando JSON al IPC de mpv.
/// `args` es el array de mpv, ej: ["set_property","pause",true] o ["seek",30,"relative"].
#[tauri::command]
pub fn mpv_cmd(args: Vec<serde_json::Value>) -> Result<(), String> {
    // mpv puede tardar un pelo en crear el socket/pipe tras spawnear; reintenta.
    let mut conn = None;
    for _ in 0..20 {
        match connect_ipc() {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let mut conn = conn.ok_or("mpv no responde (IPC)")?;

    let payload = serde_json::json!({ "command": args });
    let mut line = serde_json::to_string(&payload).map_err(|e| format!("json: {e}"))?;
    line.push('\n');
    conn.write_all(line.as_bytes())
        .map_err(|e| format!("ipc write: {e}"))?;
    Ok(())
}

/// Cierra mpv (quit limpio por IPC + kill por si acaso).
#[tauri::command]
pub fn mpv_stop(app: tauri::AppHandle, state: tauri::State<'_, PlayerState>) -> Result<(), String> {
    let _ = mpv_cmd(vec![serde_json::Value::String("quit".into())]);
    kill_existing(&state);
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(ipc_path());
    let _ = app.emit("mpv:state", false);
    Ok(())
}

/// ¿Hay un mpv vivo? (reapea el proceso si ya murió)
fn alive(state: &tauri::State<'_, PlayerState>) -> bool {
    let mut guard = state.child.lock().unwrap();
    match guard.as_mut() {
        Some(c) => match c.try_wait() {
            Ok(Some(_)) => {
                *guard = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

#[tauri::command]
pub fn mpv_running(state: tauri::State<'_, PlayerState>) -> bool {
    alive(&state)
}

/// Traduce una tecla canónica del mando (web/teclado) a un comando IPC de mpv.
/// Devuelve `true` si la manejó (mpv vivo + tecla mapeada): en ese caso el
/// webserver NO debe emitir `remote_key`, porque el control es de mpv, no de la
/// UI. Esto da control absoluto del reproductor desde el teléfono aunque mpv
/// tenga el foco (el camino es phone → Rust → IPC, sin pasar por el webview).
pub fn remote_to_mpv(app: &tauri::AppHandle, key: &str) -> bool {
    use tauri::Manager;
    let state = app.state::<PlayerState>();
    if !alive(&state) {
        return false;
    }
    // Volver/atrás cierra mpv (mismo efecto que Esc nativo).
    if key == "Backspace" || key == "Escape" {
        let _ = mpv_stop(app.clone(), state);
        return true;
    }
    let args = match key {
        " " | "Enter" => serde_json::json!(["cycle", "pause"]),
        "ArrowLeft" => serde_json::json!(["seek", -10, "relative"]),
        "ArrowRight" => serde_json::json!(["seek", 10, "relative"]),
        "ArrowUp" => serde_json::json!(["add", "volume", 5]),
        "ArrowDown" => serde_json::json!(["add", "volume", -5]),
        "[" => serde_json::json!(["seek", -60, "relative"]),
        "]" => serde_json::json!(["seek", 60, "relative"]),
        "m" | "M" => serde_json::json!(["cycle", "mute"]),
        "s" | "S" => serde_json::json!(["cycle", "sub-visibility"]),
        _ => return false, // tecla sin mapeo de mpv → que la maneje el front
    };
    if let Some(arr) = args.as_array() {
        let _ = mpv_cmd(arr.clone());
    }
    true
}

/// Estado en vivo de mpv leído por IPC (get_property).
#[derive(serde::Serialize, Default)]
pub struct MpvStatus {
    pub running: bool,
    pub pos: f64,
    pub duration: f64,
    pub pause: bool,
    pub volume: f64,
    pub mute: bool,
    pub sub: bool,
    pub title: String,
}

/// Escribe N get_property y junta las respuestas por request_id. Lectura con
/// timeout (Unix) para no colgarse si mpv no responde.
fn query_props<S: Read + Write>(
    s: &mut S,
    names: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    for (i, n) in names.iter().enumerate() {
        let cmd = serde_json::json!({ "command": ["get_property", n], "request_id": i + 1 });
        let mut line = cmd.to_string();
        line.push('\n');
        if s.write_all(line.as_bytes()).is_err() {
            break;
        }
    }
    let _ = s.flush();

    let mut out = serde_json::Map::new();
    let mut reader = BufReader::new(s);
    let mut line = String::new();
    let mut leidos = 0usize;
    // Tope de líneas por si mpv emite muchos eventos entre respuestas.
    for _ in 0..200 {
        if leidos >= names.len() {
            break;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let v: serde_json::Value = match serde_json::from_str(line.trim()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(id) = v.get("request_id").and_then(|x| x.as_u64()) {
                    if let Some(name) = names.get((id as usize).wrapping_sub(1)) {
                        out.insert(
                            (*name).to_string(),
                            v.get("data").cloned().unwrap_or(serde_json::Value::Null),
                        );
                        leidos += 1;
                    }
                }
            }
            Err(_) => break, // timeout / EOF
        }
    }
    out
}

#[cfg(not(windows))]
fn read_props(names: &[&str]) -> serde_json::Map<String, serde_json::Value> {
    use std::os::unix::net::UnixStream;
    let mut s = match UnixStream::connect(ipc_path()) {
        Ok(s) => s,
        Err(_) => return serde_json::Map::new(),
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(400)));
    query_props(&mut s, names)
}

#[cfg(windows)]
fn read_props(names: &[&str]) -> serde_json::Map<String, serde_json::Value> {
    use std::fs::OpenOptions;
    let mut f = match OpenOptions::new().read(true).write(true).open(ipc_path()) {
        Ok(f) => f,
        Err(_) => return serde_json::Map::new(),
    };
    query_props(&mut f, names)
}

/// Estado en vivo para el OSD: posición, duración, pausa, volumen, subs, título.
#[tauri::command]
pub fn mpv_status(state: tauri::State<'_, PlayerState>) -> MpvStatus {
    build_status(&state)
}

/// Igual que `mpv_status` pero usable desde Rust (ej. el webserver) con un
/// AppHandle en vez de una State inyectada por Tauri.
pub fn status_for(app: &tauri::AppHandle) -> MpvStatus {
    use tauri::Manager;
    build_status(&app.state::<PlayerState>())
}

fn build_status(state: &tauri::State<'_, PlayerState>) -> MpvStatus {
    if !alive(state) {
        return MpvStatus::default();
    }
    let p = read_props(&[
        "time-pos",
        "duration",
        "pause",
        "volume",
        "mute",
        "sub-visibility",
        "media-title",
    ]);
    let f = |k: &str| p.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let b = |k: &str| p.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    MpvStatus {
        running: true,
        pos: f("time-pos"),
        duration: f("duration"),
        pause: b("pause"),
        volume: f("volume"),
        mute: b("mute"),
        sub: b("sub-visibility"),
        title: p
            .get("media-title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn kill_existing(state: &tauri::State<'_, PlayerState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
