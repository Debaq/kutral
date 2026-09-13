// Player — reproducción de video.
//
// Linux: libmpv EMBEBIDO en la ventana de kutral (ver `mpv_embed`), una sola
//        ventana, render OpenGL en un GtkGLArea. Las commands delegan ahí.
// Otros: mpv como PROCESO externo fullscreen controlado por IPC JSON (socket
//        Unix / named pipe). Camino histórico, se mantiene hasta portar el
//        embed a Windows.

use std::sync::Mutex;

#[cfg(not(target_os = "linux"))]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(not(target_os = "linux"))]
use std::process::Child;

/// Estado del player de proceso (solo no-Linux). En Linux el handle vive en
/// `mpv_embed`; este campo queda sin usar pero el tipo es común al `manage`.
#[derive(Default)]
pub struct PlayerState {
    #[cfg(not(target_os = "linux"))]
    pub child: Mutex<Option<Child>>,
    #[cfg(target_os = "linux")]
    pub _unused: Mutex<()>,
}

/// Un canal IPTV para la playlist de mpv.
#[derive(serde::Deserialize)]
pub struct IptvItem {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// Ítem de un picker in-video provisto por el frontend (ej. lista de subtítulos
/// descargables). Al elegirlo, el backend emite "player:menu-pick" con `id`.
/// Solo Linux: el picker vive en el embed de libmpv. Fuera de ahí el comando
/// recibe el JSON crudo y no lo mira, así que el tipo ni existe.
#[cfg(target_os = "linux")]
#[derive(serde::Deserialize)]
pub struct PickerItem {
    pub label: String,
    pub id: String,
}

/// Estado en vivo de mpv para el OSD.
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

/// Sesión suspendida: lo que la UI necesita para dibujar la pill "volver a la
/// reproducción" (Esc sale a los menús pero no cierra el archivo).
#[derive(serde::Serialize, Default)]
pub struct MpvSession {
    /// Hay algo esperando volver.
    pub suspended: bool,
    pub title: String,
    pub pos: f64,
    pub duration: f64,
    /// IPTV: sigue sonando de fondo, no está pausado.
    pub live: bool,
}

/// Lo que hace falta para saber si la red está aguantando el video.
///
/// `cache_secs` son segundos de video ya bufferados por delante;
/// `cache_speed`, los bytes/s que entran. Las dos juntas distinguen los dos
/// problemas que se parecen en pantalla: el buffer que se vacía y vuelve
/// (sube la espera y listo) del caudal que NO alcanza para el bitrate del
/// archivo, donde ningún buffer sirve y solo queda bajarla antes de verla.
#[derive(serde::Serialize, Default)]
pub struct CacheStats {
    pub running: bool,
    /// mpv está detenido esperando que se llene el cache.
    pub paused_for_cache: bool,
    pub cache_secs: f64,
    pub cache_speed: f64,
    pub duration: f64,
    pub pos: f64,
    /// Tamaño del archivo si el stream lo declara (Content-Length). 0 si no.
    pub file_size: f64,
    /// Segundos que mpv junta antes de reanudar tras un corte.
    pub pause_wait: f64,
}

/// Una pista (audio/sub) del contenedor: verdad del archivo, no del nombre.
#[derive(serde::Serialize, Default)]
pub struct MpvTrack {
    pub id: i64,
    /// "audio" | "sub"
    pub kind: String,
    pub lang: String,
    pub title: String,
    pub selected: bool,
}

/// Construye el m3u temporal (común a ambos caminos): EXTINF con títulos para
/// que uosc los muestre al zapear.
fn write_iptv_playlist(items: &[IptvItem]) -> Result<std::path::PathBuf, String> {
    let mut m3u = String::from("#EXTM3U\n");
    for it in items {
        let name = it.title.clone().unwrap_or_default().replace(['\n', '\r'], " ");
        m3u.push_str(&format!("#EXTINF:-1,{name}\n{}\n", it.url));
    }
    let pl = std::env::temp_dir().join("kutral-iptv.m3u");
    std::fs::write(&pl, m3u).map_err(|e| format!("playlist: {e}"))?;
    Ok(pl)
}

// ══════════════════════════════════════════════════════════════════════════
//  LINUX — libmpv embebido (delega en mpv_embed)
// ══════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "linux")]
pub mod imp {
    use super::*;
    use crate::mpv_embed;

    #[tauri::command]
    pub fn mpv_play(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
        url: String,
        title: Option<String>,
        start_secs: Option<u64>,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("url vacía".into());
        }
        mpv_embed::play(&url, title.as_deref(), start_secs)
    }

    /// Trailer: no pisa la película que estuviera esperando y al salir no
    /// deja nada en la pill (ver mpv_embed::play_trailer).
    #[tauri::command]
    pub fn mpv_play_trailer(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
        url: String,
        title: Option<String>,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("url vacía".into());
        }
        mpv_embed::play_trailer(&url, title.as_deref())
    }

    #[tauri::command]
    pub fn mpv_play_iptv(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
        items: Vec<IptvItem>,
        start: Option<usize>,
    ) -> Result<(), String> {
        if items.is_empty() {
            return Err("sin canales".into());
        }
        let start = start.unwrap_or(0).min(items.len() - 1);
        let pl = write_iptv_playlist(&items)?;
        mpv_embed::play_iptv(&pl.to_string_lossy(), start)
    }

    #[tauri::command]
    pub fn mpv_cmd(args: Vec<serde_json::Value>) -> Result<(), String> {
        mpv_embed::run_command(&args)
    }

    #[tauri::command]
    pub fn mpv_open_picker(title: String, items: Vec<PickerItem>) -> Result<(), String> {
        mpv_embed::open_picker(
            title,
            items.into_iter().map(|i| (i.label, i.id)).collect(),
        );
        Ok(())
    }

    #[tauri::command]
    pub fn mpv_stop(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        mpv_embed::stop()
    }

    /// Esc: vuelve a los menús dejando la sesión viva (pausada; IPTV sigue).
    #[tauri::command]
    pub fn mpv_suspend(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        mpv_embed::suspend()
    }

    /// Retoma la sesión suspendida (botón "volver a la reproducción").
    #[tauri::command]
    pub fn mpv_resume(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        mpv_embed::resume()
    }

    #[tauri::command]
    pub fn mpv_session(_state: tauri::State<'_, PlayerState>) -> MpvSession {
        if !mpv_embed::is_suspended() {
            return MpvSession::default();
        }
        MpvSession {
            suspended: true,
            title: mpv_embed::get_string("media-title"),
            pos: mpv_embed::get_f64("time-pos"),
            duration: mpv_embed::get_f64("duration"),
            live: mpv_embed::is_live(),
        }
    }

    #[tauri::command]
    pub fn mpv_running(_state: tauri::State<'_, PlayerState>) -> bool {
        mpv_embed::is_running()
    }

    #[tauri::command]
    pub fn mpv_tracks(_state: tauri::State<'_, PlayerState>) -> Vec<MpvTrack> {
        mpv_embed::tracks()
            .into_iter()
            .map(|(id, kind, lang, title, selected)| MpvTrack {
                id,
                kind,
                lang,
                title,
                selected,
            })
            .collect()
    }

    #[tauri::command]
    pub fn mpv_status(_state: tauri::State<'_, PlayerState>) -> MpvStatus {
        build_status()
    }

    pub fn status_for(_app: &tauri::AppHandle) -> MpvStatus {
        build_status()
    }

    fn build_status() -> MpvStatus {
        if !mpv_embed::is_running() {
            return MpvStatus::default();
        }
        MpvStatus {
            running: true,
            pos: mpv_embed::get_f64("time-pos"),
            duration: mpv_embed::get_f64("duration"),
            pause: mpv_embed::get_bool("pause"),
            volume: mpv_embed::get_f64("volume"),
            mute: mpv_embed::get_bool("mute"),
            sub: mpv_embed::get_bool("sub-visibility"),
            title: mpv_embed::get_string("media-title"),
        }
    }

    #[tauri::command]
    pub fn mpv_cache_stats(_state: tauri::State<'_, PlayerState>) -> CacheStats {
        if !mpv_embed::is_running() {
            return CacheStats::default();
        }
        CacheStats {
            running: true,
            paused_for_cache: mpv_embed::get_bool("paused-for-cache"),
            cache_secs: mpv_embed::get_f64("demuxer-cache-duration"),
            cache_speed: mpv_embed::get_f64("cache-speed"),
            duration: mpv_embed::get_f64("duration"),
            pos: mpv_embed::get_f64("time-pos"),
            file_size: mpv_embed::get_f64("file-size"),
            pause_wait: mpv_embed::get_f64("cache-pause-wait"),
        }
    }

    /// Ajusta el cacheo de red en caliente. Se llama al empezar (preset del
    /// usuario) y cada vez que la escalada automática sube la espera.
    #[tauri::command]
    pub fn mpv_set_cache(
        wait_secs: f64,
        readahead_secs: f64,
        max_mb: u64,
    ) -> Result<(), String> {
        // Los tres se intentan aunque uno falle: `cache-pause-wait` es el que
        // arregla el síntoma, y perderlo porque una versión de mpv no acepta
        // otro en caliente sería el peor cambio posible.
        let mut err = String::new();
        for (k, v) in [
            ("cache-pause-wait", format!("{wait_secs}")),
            ("demuxer-readahead-secs", format!("{readahead_secs}")),
            ("demuxer-max-bytes", format!("{max_mb}MiB")),
        ] {
            if let Err(e) = mpv_embed::set_prop(k, &v) {
                if err.is_empty() {
                    err = e;
                }
            }
        }
        if err.is_empty() { Ok(()) } else { Err(err) }
    }

    /// Traduce tecla canónica del mando a comando mpv. `true` si la manejó
    /// (mpv vivo + tecla mapeada) → el webserver NO emite remote_key.
    pub fn remote_to_mpv(_app: &tauri::AppHandle, key: &str) -> bool {
        if !mpv_embed::is_running() {
            return false;
        }
        if key == "Backspace" || key == "Escape" {
            let _ = mpv_embed::suspend();
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
            _ => return false,
        };
        if let Some(arr) = args.as_array() {
            let _ = mpv_embed::run_command(arr);
        }
        true
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  NO-LINUX — mpv proceso externo + IPC (camino histórico)
// ══════════════════════════════════════════════════════════════════════════
#[cfg(not(target_os = "linux"))]
pub mod imp {
    use super::*;
    use tauri::Emitter;

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

    fn connect_ipc() -> Result<Box<dyn Write>, String> {
        #[cfg(windows)]
        {
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

    fn mpv_bin(app: &tauri::AppHandle) -> String {
        use tauri::Manager;
        #[cfg(windows)]
        let exe = "mpv.exe";
        #[cfg(not(windows))]
        let exe = "mpv";

        let mut cands: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(res) = app.path().resource_dir() {
            cands.push(res.join("vendor").join(exe));
        }
        cands.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
        if let Ok(p) = std::env::current_exe() {
            if let Some(dir) = p.parent() {
                cands.push(dir.join("vendor").join(exe));
            }
        }
        for c in cands {
            if c.exists() {
                return c.to_string_lossy().into_owned();
            }
        }
        exe.to_string()
    }

    /// yt-dlp vendorizado: ytdl_hook lo necesita para los trailers de YouTube
    /// (streams DASH separados) sin depender del PATH del sistema.
    fn ytdlp_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
        use tauri::Manager;
        #[cfg(windows)]
        let exe = "yt-dlp.exe";
        #[cfg(not(windows))]
        let exe = "yt-dlp";
        let mut cands: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(res) = app.path().resource_dir() {
            cands.push(res.join("vendor").join(exe));
        }
        cands.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
        if let Ok(p) = std::env::current_exe() {
            if let Some(dir) = p.parent() {
                cands.push(dir.join("vendor").join(exe));
            }
        }
        cands.into_iter().find(|c| c.exists())
    }

    fn mpv_config_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
        use tauri::Manager;
        let mut cands: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(res) = app.path().resource_dir() {
            cands.push(res.join("vendor").join("mpv-config"));
        }
        cands.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("vendor")
                .join("mpv-config"),
        );
        if let Ok(p) = std::env::current_exe() {
            if let Some(dir) = p.parent() {
                cands.push(dir.join("vendor").join("mpv-config"));
            }
        }
        cands.into_iter().find(|c| c.exists())
    }

    fn spawn_mpv(
        app: &tauri::AppHandle,
        state: &tauri::State<'_, PlayerState>,
        extra: Vec<String>,
    ) -> Result<(), String> {
        use std::process::Command;

        kill_existing(state);

        #[cfg(not(windows))]
        let _ = std::fs::remove_file(ipc_path());

        let mut cmd = Command::new(mpv_bin(app));
        #[cfg(not(windows))]
        cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        cmd.arg(format!("--input-ipc-server={}", ipc_path()))
            .arg("--fullscreen")
            .arg("--force-window=immediate")
            .arg("--no-terminal")
            .arg("--really-quiet")
            .arg("--keep-open=no");

        if let Some(cfg) = mpv_config_dir(app) {
            cmd.arg(format!("--config-dir={}", cfg.display()));
        } else {
            let conf = std::env::temp_dir().join("kutral-mpv-input.conf");
            let _ = std::fs::write(&conf, "ESC quit\nBS quit\nq quit\n");
            cmd.arg(format!("--input-conf={}", conf.display()))
                .arg("--hwdec=auto-safe");
        }

        if let Some(y) = ytdlp_path(app) {
            cmd.arg(format!("--script-opts=ytdl_hook-ytdl_path={}", y.display()));
        }

        for a in extra {
            cmd.arg(a);
        }

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
        *state.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        let _ = app.emit("mpv:state", true);
        Ok(())
    }

    #[tauri::command]
    pub fn mpv_play(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
        url: String,
        title: Option<String>,
        start_secs: Option<u64>,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("url vacía".into());
        }
        let mut extra: Vec<String> = Vec::new();
        if let Some(t) = &title {
            extra.push(format!("--force-media-title={t}"));
        }
        if let Some(s) = start_secs {
            if s > 0 {
                extra.push(format!("--start=+{s}"));
            }
        }
        extra.push(url.clone());
        eprintln!("[mpv] spawn fullscreen url={url}");
        spawn_mpv(&app, &state, extra)
    }

    /// Trailer. Con mpv de proceso externo no hay sesión que preservar (cada
    /// reproducción es un proceso nuevo que mata al anterior), así que se
    /// comporta como un play normal.
    #[tauri::command]
    pub fn mpv_play_trailer(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
        url: String,
        title: Option<String>,
    ) -> Result<(), String> {
        mpv_play(app, state, url, title, None)
    }

    #[tauri::command]
    pub fn mpv_play_iptv(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
        items: Vec<IptvItem>,
        start: Option<usize>,
    ) -> Result<(), String> {
        if items.is_empty() {
            return Err("sin canales".into());
        }
        let start = start.unwrap_or(0).min(items.len() - 1);
        let pl = write_iptv_playlist(&items)?;

        let mut extra: Vec<String> = Vec::new();
        if let Some(cfg) = mpv_config_dir(&app) {
            extra.push(format!("--input-conf={}", cfg.join("iptv-input.conf").display()));
        }
        extra.push("--load-unsafe-playlists".into());
        extra.push(format!("--playlist-start={start}"));
        extra.push(format!("--playlist={}", pl.display()));
        eprintln!("[mpv] spawn IPTV playlist start={start} n={}", items.len());
        spawn_mpv(&app, &state, extra)
    }

    #[tauri::command]
    pub fn mpv_cmd(args: Vec<serde_json::Value>) -> Result<(), String> {
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

    // El picker in-video solo existe con libmpv embebido (Linux). Stub no-op:
    // recibe los ítems como JSON crudo, porque acá nadie los lee.
    #[tauri::command]
    pub fn mpv_open_picker(
        _title: String,
        _items: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        Ok(())
    }

    #[tauri::command]
    pub fn mpv_stop(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        let _ = mpv_cmd(vec![serde_json::Value::String("quit".into())]);
        kill_existing(&state);
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(ipc_path());
        let _ = app.emit("mpv:state", false);
        Ok(())
    }

    fn alive(state: &tauri::State<'_, PlayerState>) -> bool {
        let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Sin embed no hay superficie que ocultar: suspender = pausar y avisar a
    /// la UI, que dibuja la pill para volver.
    #[tauri::command]
    pub fn mpv_suspend(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        if !alive(&state) {
            return Ok(());
        }
        let _ = mpv_cmd(vec![
            serde_json::Value::String("set_property".into()),
            serde_json::Value::String("pause".into()),
            serde_json::Value::Bool(true),
        ]);
        let _ = app.emit("mpv:suspended", true);
        Ok(())
    }

    #[tauri::command]
    pub fn mpv_resume(
        app: tauri::AppHandle,
        state: tauri::State<'_, PlayerState>,
    ) -> Result<(), String> {
        if !alive(&state) {
            return Err("no hay reproducción en pausa".into());
        }
        let _ = mpv_cmd(vec![
            serde_json::Value::String("set_property".into()),
            serde_json::Value::String("pause".into()),
            serde_json::Value::Bool(false),
        ]);
        let _ = app.emit("mpv:suspended", false);
        Ok(())
    }

    #[tauri::command]
    pub fn mpv_session(state: tauri::State<'_, PlayerState>) -> MpvSession {
        let st = build_status(&state);
        if !st.running || !st.pause {
            return MpvSession::default();
        }
        MpvSession {
            suspended: true,
            title: st.title,
            pos: st.pos,
            duration: st.duration,
            live: st.duration <= 0.0,
        }
    }

    #[tauri::command]
    pub fn mpv_running(state: tauri::State<'_, PlayerState>) -> bool {
        alive(&state)
    }

    #[tauri::command]
    pub fn mpv_cache_stats(state: tauri::State<'_, PlayerState>) -> CacheStats {
        if !alive(&state) {
            return CacheStats::default();
        }
        let p = read_props(&[
            "paused-for-cache",
            "demuxer-cache-duration",
            "cache-speed",
            "duration",
            "time-pos",
            "file-size",
            "cache-pause-wait",
        ]);
        let num = |k: &str| p.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        CacheStats {
            running: true,
            paused_for_cache: p
                .get("paused-for-cache")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            cache_secs: num("demuxer-cache-duration"),
            cache_speed: num("cache-speed"),
            duration: num("duration"),
            pos: num("time-pos"),
            file_size: num("file-size"),
            pause_wait: num("cache-pause-wait"),
        }
    }

    /// Igual que en Linux, pero por el pipe: mpv acepta set_property por IPC,
    /// así que la escalada de cache funciona idéntica sin embed.
    #[tauri::command]
    pub fn mpv_set_cache(
        wait_secs: f64,
        readahead_secs: f64,
        max_mb: u64,
    ) -> Result<(), String> {
        let set = |name: &str, val: String| {
            mpv_cmd(vec![
                serde_json::Value::String("set_property".into()),
                serde_json::Value::String(name.into()),
                serde_json::Value::String(val),
            ])
        };
        // Igual que en Linux: uno que falle no se lleva a los otros dos.
        let mut err = String::new();
        for (k, v) in [
            ("cache-pause-wait", format!("{wait_secs}")),
            ("demuxer-readahead-secs", format!("{readahead_secs}")),
            ("demuxer-max-bytes", format!("{max_mb}MiB")),
        ] {
            if let Err(e) = set(k, v) {
                if err.is_empty() {
                    err = e;
                }
            }
        }
        if err.is_empty() { Ok(()) } else { Err(err) }
    }

    pub fn remote_to_mpv(app: &tauri::AppHandle, key: &str) -> bool {
        use tauri::Manager;
        let state = app.state::<PlayerState>();
        if !alive(&state) {
            return false;
        }
        if key == "Backspace" || key == "Escape" {
            let _ = mpv_suspend(app.clone(), state);
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
            _ => return false,
        };
        if let Some(arr) = args.as_array() {
            let _ = mpv_cmd(arr.clone());
        }
        true
    }

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
                Err(_) => break,
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
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(400)));
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

    #[tauri::command]
    pub fn mpv_tracks(state: tauri::State<'_, PlayerState>) -> Vec<MpvTrack> {
        if !alive(&state) {
            return Vec::new();
        }
        let p = read_props(&["track-list"]);
        let Some(arr) = p.get("track-list").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|t| {
                let kind = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if kind != "audio" && kind != "sub" {
                    return None;
                }
                Some(MpvTrack {
                    id: t.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                    kind: kind.to_string(),
                    lang: t.get("lang").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    title: t.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    selected: t.get("selected").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect()
    }

    #[tauri::command]
    pub fn mpv_status(state: tauri::State<'_, PlayerState>) -> MpvStatus {
        build_status(&state)
    }

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
        if let Some(mut child) = state.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// El webserver llama estas dos directamente; las commands se referencian como
// `player::imp::*` desde generate_handler (necesita los helpers del módulo).
pub use imp::{remote_to_mpv, status_for};
