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

use std::io::Write;
use std::process::Child;
use std::sync::Mutex;

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

    let mut cmd = Command::new("mpv");
    cmd.arg(format!("--input-ipc-server={}", ipc_path()))
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

    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "mpv no está instalado (falta en el PATH)".to_string()
        } else {
            format!("spawn mpv: {e}")
        }
    })?;

    *state.child.lock().unwrap() = Some(child);
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
pub fn mpv_stop(state: tauri::State<'_, PlayerState>) -> Result<(), String> {
    let _ = mpv_cmd(vec![serde_json::Value::String("quit".into())]);
    kill_existing(&state);
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(ipc_path());
    Ok(())
}

/// ¿Hay un mpv vivo?
#[tauri::command]
pub fn mpv_running(state: tauri::State<'_, PlayerState>) -> bool {
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

fn kill_existing(state: &tauri::State<'_, PlayerState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
