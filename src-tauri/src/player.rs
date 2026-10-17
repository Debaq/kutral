// Player — reproducción de video.
//
// libmpv EMBEBIDO en la ventana de kutral (ver `mpv_embed`), una sola ventana,
// en Linux y en Windows. Las commands delegan ahí. (Hasta octubre de 2026
// Windows usaba mpv.exe como proceso aparte, controlado por IPC.)

use std::sync::Mutex;

/// Las commands lo reciben por `manage`; el handle de mpv vive en `mpv_embed`.
#[derive(Default)]
pub struct PlayerState {
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
    /// Pista "forzada": solo carteles y diálogos en otro idioma, no la
    /// película entera.
    pub forced: bool,
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
//  libmpv embebido (delega en mpv_embed)
// ══════════════════════════════════════════════════════════════════════════
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
        sync: Option<crate::sub_sync::SyncCtx>,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("url vacía".into());
        }
        mpv_embed::play(&url, title.as_deref(), start_secs, sync)
    }

    /// Lo que suena es un capítulo de un pack: "Capítulos" en el bar.
    #[tauri::command]
    pub fn mpv_capitulos(on: bool) {
        mpv_embed::set_capitulos(on);
    }

    /// Hay una TV elegida: "Pasar a la TV" en el bar.
    #[tauri::command]
    pub fn mpv_tv(on: bool) {
        mpv_embed::set_tv(on);
    }

    /// Minuto, audio y subtítulos de lo que suena, para seguir en la TV.
    #[tauri::command]
    pub fn mpv_para_tv() -> mpv_embed::ParaTv {
        mpv_embed::para_tv()
    }

    /// Identidad del subtítulo externo recién cargado (ver mpv_embed::SUB_KEYS).
    #[tauri::command]
    pub fn mpv_sub_key(path: String, key: String) {
        mpv_embed::set_sub_key(&path, &key);
    }

    /// Trailer: no pisa la película que estuviera esperando y al salir no
    /// deja nada en la pill (ver mpv_embed::play_trailer).
    #[tauri::command]
    pub fn mpv_play_trailer(
        _app: tauri::AppHandle,
        _state: tauri::State<'_, PlayerState>,
        url: String,
        title: Option<String>,
        audio_url: Option<String>,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("url vacía".into());
        }
        let audio = audio_url.filter(|a| !a.is_empty());
        mpv_embed::play_trailer(&url, title.as_deref(), audio.as_deref())
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
    pub fn mpv_open_picker(
        title: String,
        items: Vec<PickerItem>,
        sel: Option<usize>,
    ) -> Result<(), String> {
        mpv_embed::open_picker(
            title,
            items.into_iter().map(|i| (i.label, i.id)).collect(),
            sel.unwrap_or(0),
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
            .map(|(id, kind, lang, title, selected, forced)| MpvTrack {
                id,
                kind,
                lang,
                title,
                selected,
                forced,
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
            "a" | "A" => serde_json::json!(["cycle", "audio"]),
            "j" | "J" => serde_json::json!(["cycle", "sub"]),
            _ => return false,
        };
        if let Some(arr) = args.as_array() {
            let _ = mpv_embed::run_command(arr);
        }
        true
    }
}

// El webserver llama estas dos directamente; las commands se referencian como
// `player::imp::*` desde generate_handler (necesita los helpers del módulo).
pub use imp::{remote_to_mpv, status_for};
