
mod anilist;
mod anime_web;
mod awards;
mod cache;
mod cast;
mod creds;
mod dlna;
mod kitsu;
mod kodios;
mod lan;
mod net;
mod omdb;
mod opensubtitles;
mod probe;
mod rd;
mod player;
#[cfg(target_os = "linux")]
mod mpv_embed;
#[cfg(target_os = "linux")]
mod reposo;
#[cfg(windows)]
mod reposo_win;
mod red_directa;
mod screening;
mod sistema;
mod tmdb;
mod vera;
mod torrent;
mod trailers;
mod webserver;
mod wyzie;
mod winproc;

// Lo de TMDb que usan los demás módulos, con la ruta de siempre.
pub(crate) use tmdb::{
    fetch_json, tmdb_buscar, tmdb_overview_es, tmdb_trending, EpisodeMini, ExternalIds,
    PersonMini, SeasonMini, TmdbItem, TmdbListResp, LANG, TMDB_BASE,
};

/// El front avisa cuando reproduce fuera de mpv (iframe web, IPTV con hls.js)
/// para que el escritorio no se duerma. mpv avisa por su cuenta.
#[tauri::command]
fn reposo_inhibir(fuente: String, on: bool) {
    #[cfg(target_os = "linux")]
    reposo::set(&fuente, on);
    #[cfg(windows)]
    reposo_win::set(&fuente, on);
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = (fuente, on);
}

/// Log de depuración desde el frontend a la consola de Tauri (stderr).
#[tauri::command]
fn ui_log(msg: String) {
    eprintln!("[ui] {msg}");
}

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Cliente HTTP compartido para TMDb/OMDb/imágenes.
///
/// `reqwest::Client` es un Arc por dentro: clonarlo reusa el pool de
/// conexiones (keep-alive + TLS ya negociado). Construir uno por request
/// tiraba el pool cada vez → un handshake TLS completo por cada card del grid.
///
/// El `timeout` es igual de importante: sin él, un request estancado (CDN que
/// acepta la conexión y no responde) no termina NUNCA. El frontend deja
/// `listLoading = true` para siempre y el grid queda congelado sin reintentar.
fn client() -> Result<reqwest::Client, String> {
    static HTTP: std::sync::OnceLock<Result<reqwest::Client, String>> = std::sync::OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(BROWSER_UA)
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(5))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| e.to_string())
    })
    .clone()
}

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tauri::Manager;

// Helper de tecla virtual usado por webserver.rs (HTTP /key remote control).
// No es comando Tauri por sí mismo: se invoca desde el handler HTTP.
pub fn press_key(key: &str) -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {}", e))?;
    let k = match key.to_lowercase().as_str() {
        "space" | " " => Key::Space,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "enter" | "return" => Key::Return,
        "backspace" | "back" => Key::Backspace,
        "arrowup" | "up" => Key::UpArrow,
        "arrowdown" | "down" => Key::DownArrow,
        "arrowleft" | "left" => Key::LeftArrow,
        "arrowright" | "right" => Key::RightArrow,
        s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
        _ => return Err(format!("tecla desconocida: {}", key)),
    };
    enigo.key(k, Direction::Click).map_err(|e| e.to_string())
}

#[tauri::command]
async fn cache_image(
    app: tauri::AppHandle,
    url: String,
    max_w: Option<u32>,
) -> Result<String, String> {
    if url.is_empty() {
        return Err("url vacía".into());
    }
    // Hash URL para filename estable
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    if let Some(w) = max_w { hasher.update(w.to_le_bytes()); }
    let hash = hex::encode(hasher.finalize());
    // .jpg, no .webp: ver el encoder más abajo. El cambio de extensión invalida
    // solo el cache viejo (se regenera al vuelo); los .webp huérfanos los barre
    // la limpieza de cache del sistema o un borrado manual de <cache>/imgs.
    let filename = format!("{}.jpg", &hash[..16]);

    let cache_dir: PathBuf = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cache dir: {}", e))?
        .join("imgs");
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let path = cache_dir.join(&filename);

    if path.exists() {
        return Ok(path.to_string_lossy().to_string());
    }

    // Download
    let bytes = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("fetch: {}", e))?
        .error_for_status()
        .map_err(|e| format!("fetch: {}", e))?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    // Decode + resize + JPEG encode (en blocking thread, image es CPU-bound).
    //
    // Antes esto era WebPEncoder::new_lossless. El `image` crate SOLO sabe
    // escribir WebP sin pérdida, y lossless sobre una foto (póster, backdrop,
    // retrato) es lo peor de los dos mundos: decenas de veces más lento que
    // JPEG y el archivo sale MÁS GRANDE que el original de TMDb. Con el grid
    // pidiendo ~100 imágenes al scrollear, eso clavaba la CPU y la app se
    // quedaba pegada. JPEG q=82 es visualmente indistinguible a este tamaño.
    let path_clone = path.clone();
    let max_w_val = max_w.unwrap_or(0);
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let img = image::load_from_memory(&bytes).map_err(|e| format!("decode: {}", e))?;
        let resized = if max_w_val > 0 && img.width() > max_w_val {
            let ratio = max_w_val as f32 / img.width() as f32;
            let new_h = (img.height() as f32 * ratio).round() as u32;
            img.resize_exact(max_w_val, new_h, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        // JPEG no lleva alfa: rgb8, no rgba8.
        let rgb = resized.to_rgb8();
        // A un temporal y después rename (atómico): si el encode falla o la
        // app se cierra a mitad, en la ruta final nunca queda un .jpg cortado
        // que el `path.exists()` de arriba devolvería para siempre. El nombre
        // lleva un contador para que dos pedidos simultáneos de la misma URL
        // no escriban el mismo archivo a la vez.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path_clone.with_extension(format!("jpg.{}.tmp", n));
        let escribir = || -> Result<(), String> {
            let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 82);
            use image::ImageEncoder;
            encoder
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .map_err(|e| format!("encode jpeg: {}", e))?;
            std::fs::rename(&tmp, &path_clone).map_err(|e| e.to_string())
        };
        let r = escribir();
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(path.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri_plugin_sql::{Migration, MigrationKind};
    let migrations = vec![
        Migration {
            version: 1,
            description: "watch_history",
            sql: "CREATE TABLE IF NOT EXISTS watch_history (
                imdb_id TEXT PRIMARY KEY,
                tmdb_id INTEGER NOT NULL,
                media_type TEXT NOT NULL,
                title TEXT NOT NULL,
                poster_path TEXT,
                watched_seconds INTEGER NOT NULL DEFAULT 0,
                runtime_seconds INTEGER,
                progress_real REAL,
                completed INTEGER NOT NULL DEFAULT 0,
                last_watched INTEGER NOT NULL
            );",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "unavailable",
            sql: "CREATE TABLE IF NOT EXISTS unavailable_items (
                imdb_id TEXT PRIMARY KEY,
                detected_at INTEGER NOT NULL
            );",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 3,
            description: "vera_v3_schema",
            sql: "
                CREATE TABLE IF NOT EXISTS vera_setup (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    mode_io TEXT NOT NULL,
                    depth_profile TEXT NOT NULL,
                    languages_known TEXT NOT NULL DEFAULT '[]',
                    dub_pref TEXT NOT NULL,
                    platforms TEXT NOT NULL DEFAULT '[]',
                    excluded_genres TEXT NOT NULL DEFAULT '[]',
                    excluded_themes TEXT NOT NULL DEFAULT '[]',
                    personality TEXT NOT NULL DEFAULT 'warm',
                    completed_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_titles (
                    imdb_id TEXT PRIMARY KEY,
                    tmdb_id INTEGER,
                    title TEXT NOT NULL,
                    year INTEGER,
                    runtime_min INTEGER,
                    format TEXT NOT NULL,
                    genres TEXT NOT NULL DEFAULT '[]',
                    tone_tags TEXT NOT NULL DEFAULT '[]',
                    use_tags TEXT NOT NULL DEFAULT '[]',
                    sensitive_themes TEXT NOT NULL DEFAULT '[]',
                    age_min INTEGER NOT NULL DEFAULT 0,
                    country TEXT,
                    languages TEXT NOT NULL DEFAULT '[]',
                    platforms TEXT NOT NULL DEFAULT '[]',
                    popularity REAL DEFAULT 0,
                    updated_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_vera_titles_format ON vera_titles(format);
                CREATE INDEX IF NOT EXISTS idx_vera_titles_age ON vera_titles(age_min);

                CREATE TABLE IF NOT EXISTS vera_responses (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    intent TEXT NOT NULL,
                    format_pref TEXT,
                    time_available TEXT,
                    company TEXT,
                    ages TEXT,
                    ages_attentive INTEGER,
                    tones TEXT NOT NULL DEFAULT '[]',
                    session_excluded_genres TEXT NOT NULL DEFAULT '[]',
                    session_excluded_themes TEXT NOT NULL DEFAULT '[]',
                    created_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_weights (
                    tag TEXT PRIMARY KEY,
                    weight REAL NOT NULL DEFAULT 1.0,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_templates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    imdb_id TEXT NOT NULL,
                    context TEXT NOT NULL,
                    personality TEXT NOT NULL,
                    text TEXT NOT NULL,
                    FOREIGN KEY (imdb_id) REFERENCES vera_titles(imdb_id)
                );

                CREATE INDEX IF NOT EXISTS idx_vera_templates_lookup
                    ON vera_templates(imdb_id, context, personality);

                CREATE TABLE IF NOT EXISTS vera_feedback (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    imdb_id TEXT NOT NULL,
                    response_id INTEGER,
                    rating INTEGER NOT NULL,
                    finished INTEGER,
                    why_not TEXT,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY (imdb_id) REFERENCES vera_titles(imdb_id)
                );
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 4,
            description: "unavailable_kind",
            // Drop+recreate: las filas previas se generaron con type=movie
            // hardcodeado y marcaron series como no disponibles. Resetear.
            sql: "
                DROP TABLE IF EXISTS unavailable_items;
                CREATE TABLE unavailable_items (
                    imdb_id TEXT NOT NULL,
                    kind TEXT NOT NULL DEFAULT 'movie',
                    detected_at INTEGER NOT NULL,
                    PRIMARY KEY (imdb_id, kind)
                );
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 5,
            description: "unavailable_reset_screening_deprecated",
            // El screening automático contra streamdata.vaplayer.ru dejó de ser
            // confiable (404 intermitentes en pelis sanas) y ya no se dispara
            // desde el frontend. Vaciamos lo acumulado por ese detector para no
            // seguir ocultando el botón Descubrir con datos viejos y falsos.
            // Los reportes manuales del user se vuelven a generar al vuelo.
            sql: "DELETE FROM unavailable_items;",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 6,
            description: "vera_setup_preferencias",
            // Tres preferencias que el esquema v3 no contemplaba:
            //   animation_pref  gusta | indiferente | no — la animación divide
            //                   aguas y "no me gustan" no se podía expresar
            //                   como simple exclusión de género sin perder el
            //                   caso contrario ("me encantan").
            //   languages_avoid idiomas que la persona no tolera escuchar.
            //   runtime_max     tope de duración. Antes iba embutido en
            //                   depth_profile como "auto:<min>"; ahora tiene
            //                   columna propia y se lee de las dos formas para
            //                   no perder lo ya guardado.
            // Aditiva: ALTER TABLE ADD COLUMN no toca las filas existentes.
            sql: "
                ALTER TABLE vera_setup ADD COLUMN animation_pref TEXT NOT NULL DEFAULT 'indiferente';
                ALTER TABLE vera_setup ADD COLUMN languages_avoid TEXT NOT NULL DEFAULT '[]';
                ALTER TABLE vera_setup ADD COLUMN runtime_max INTEGER;
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 7,
            description: "historial_por_episodio_y_favoritos",
            // watch_history nació con PK = imdb_id: una serie entera compartía
            // una fila y cada capítulo pisaba al anterior. Ahora la PK es
            // (imdb_id, season, episode) con -1/-1 para películas, que es el
            // valor con el que migran las filas viejas.
            //
            // SQLite no sabe cambiar una PK con ALTER: hay que reconstruir la
            // tabla y copiar. La copia preserva TODO el historial previo.
            //
            // `imdb_id` también acepta claves sintéticas `anilist:<id>` para el
            // anime que no tiene imdb (ver historial.svelte.ts). Por eso sigue
            // siendo TEXT y no se valida el formato tt*.
            //
            // still_path / episode_title: sin ellos la lista por fecha no
            // podría dibujar un capítulo (el poster de la serie no dice cuál
            // viste).
            sql: "
                CREATE TABLE watch_history_nuevo (
                    imdb_id TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    tmdb_id INTEGER NOT NULL,
                    media_type TEXT NOT NULL,
                    title TEXT NOT NULL,
                    poster_path TEXT,
                    still_path TEXT,
                    episode_title TEXT,
                    watched_seconds INTEGER NOT NULL DEFAULT 0,
                    runtime_seconds INTEGER,
                    progress_real REAL,
                    completed INTEGER NOT NULL DEFAULT 0,
                    last_watched INTEGER NOT NULL,
                    PRIMARY KEY (imdb_id, season, episode)
                );

                INSERT INTO watch_history_nuevo
                    (imdb_id, season, episode, tmdb_id, media_type, title,
                     poster_path, watched_seconds, runtime_seconds,
                     progress_real, completed, last_watched)
                SELECT imdb_id, -1, -1, tmdb_id, media_type, title,
                       poster_path, watched_seconds, runtime_seconds,
                       progress_real, completed, last_watched
                  FROM watch_history;

                DROP TABLE watch_history;
                ALTER TABLE watch_history_nuevo RENAME TO watch_history;

                CREATE INDEX IF NOT EXISTS idx_wh_last ON watch_history(last_watched);
                CREATE INDEX IF NOT EXISTS idx_wh_imdb ON watch_history(imdb_id);

                CREATE TABLE IF NOT EXISTS favorites (
                    imdb_id TEXT PRIMARY KEY,
                    tmdb_id INTEGER NOT NULL,
                    media_type TEXT NOT NULL,
                    title TEXT NOT NULL,
                    poster_path TEXT,
                    added_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_fav_added ON favorites(added_at);
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 8,
            description: "descargas locales",
            // Puente entre un título del catálogo y el archivo que quedó en
            // disco. Sin esta tabla la cola de torrents solo conoce nombres de
            // release: al volver a la ficha nadie sabe que ya la tienes bajada.
            //
            // Misma clave que watch_history (imdb, o `anilist:<id>` en anime) y
            // el mismo -1/-1 para películas: así una fila se busca con lo que
            // la ficha ya tiene a mano.
            sql: "CREATE TABLE IF NOT EXISTS descargas (
                    clave TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    info_hash TEXT NOT NULL,
                    ruta TEXT NOT NULL,
                    release_title TEXT NOT NULL,
                    quality TEXT NOT NULL DEFAULT 'unknown',
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    -- 0 mientras baja: un archivo a medias no se puede ofrecer
                    -- como copia local (mpv abriría un video cortado).
                    completa INTEGER NOT NULL DEFAULT 0,
                    added_at INTEGER NOT NULL,
                    PRIMARY KEY (clave, season, episode)
                );

                CREATE INDEX IF NOT EXISTS idx_desc_hash ON descargas(info_hash);
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 9,
            description: "cola de descargas pendientes",
            // Lo que el usuario pidió bajar pero todavía no empezó. Existe
            // porque el cliente torrent baja TODO en paralelo: 24 capítulos a
            // la vez se reparten los seeds y no termina ninguno. Acá esperan
            // su turno, de a pocos por vez (config.torrentMaxParalelas).
            //
            // No se guarda el magnet: se busca al llegar el turno. Un magnet
            // de hace tres días puede estar sin seeds, y la búsqueda fresca
            // además respeta la config de idioma/calidad del momento.
            sql: "CREATE TABLE IF NOT EXISTS descargas_pendientes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    clave TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    title TEXT NOT NULL,
                    -- 'T1 E4' en series, vacío en películas.
                    etiqueta TEXT NOT NULL DEFAULT '',
                    imdb_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    original_title TEXT,
                    kitsu_id INTEGER,
                    -- 'espera' | 'buscando' | 'error'
                    estado TEXT NOT NULL DEFAULT 'espera',
                    error TEXT,
                    added_at INTEGER NOT NULL,
                    UNIQUE (clave, season, episode)
                );

                CREATE INDEX IF NOT EXISTS idx_pend_orden
                    ON descargas_pendientes(added_at, id);
            ",
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .setup(|app| {
            // Limpieza de páginas de catálogo vencidas. En un hilo aparte:
            // es I/O de disco y no tiene por qué demorar el arranque.
            {
                let h = app.handle().clone();
                std::thread::spawn(move || cache::purgar(&h));
            }
            // Activa MSE en el WebKitGTK del webview para que hls.js pueda
            // reproducir HLS (IPTV) DENTRO de la app. Sin esto el <video>
            // queda en negro porque el webview no expone MediaSource.
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.with_webview(|webview| {
                        use webkit2gtk::{SettingsExt, WebViewExt};
                        let wv = webview.inner();
                        if let Some(settings) = WebViewExt::settings(&wv) {
                            settings.set_enable_mediasource(true);
                            settings.set_media_playback_requires_user_gesture(false);
                            settings.set_enable_webaudio(true);
                        }
                    });
                    // Reproductor libmpv embebido: crea el handle y mete el
                    // GtkGLArea en el toplevel del webview (una sola ventana).
                    if let Err(e) = mpv_embed::init(app.handle(), &win) {
                        eprintln!("[mpv-embed] init falló: {e}");
                    }
                }
            }
            Ok(())
        })
        .manage(screening::ScreeningState::default())
        .manage(player::PlayerState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:kutral.db", migrations)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            reposo_inhibir,
            tmdb::tmdb_discover,
            tmdb::tmdb_search,
            tmdb::tmdb_detail,
            omdb::omdb_detail,
            tmdb::tmdb_season,
            tmdb::tmdb_recommendations,
            tmdb::tmdb_genres,
            trailers::tmdb_videos,
            trailers::tmdb_trailer_key,
            trailers::yt_trailer_src,
            trailers::apple_trailer,
            tmdb::item_status,
            tmdb::tmdb_person,
            cache_image,
            vera::vera_intent_options,
            vera::vera_genre_list,
            vera::vera_theme_list,
            vera::vera_platform_list,
            vera::vera_import_catalog,
            vera::vera_catalog_count,
            sistema::os_info,
            sistema::wifi_status,
            sistema::wifi_scan,
            sistema::wifi_connect,
            rd::rd_device_start,
            rd::rd_device_poll,
            creds::rd_creds_save,
            creds::rd_creds_clear,
            creds::rd_creds_status,
            sistema::audio_get,
            sistema::audio_set,
            sistema::audio_set_mute,
            sistema::brightness_get,
            sistema::brightness_set,
            webserver::web_server_start,
            webserver::web_set_tmdb_key,
            webserver::web_server_stop,
            webserver::web_server_status,
            ui_log,
            kodios::kodios_search,
            anilist::anilist_discover,
            anilist::anilist_detail,
            anilist::anilist_relacionados,
            anilist::anizip_episodes,
            probe::ffprobe_tracks,
            cast::cast_scan,
            cast::cast_tv_por_ip,
            cast::cast_ping,
            cast::cast_play,
            cast::cast_status,
            cast::cast_control,
            cast::cast_soltar,
            lan::cast_red_info,
            rd::rd_resolve,
            rd::rd_unrestrict,
            rd::rd_instant_available,
            rd::rd_account,
            rd::rd_cleanup_torrents,
            torrent::torrent_add,
            torrent::torrent_status,
            torrent::torrent_list,
            torrent::torrent_pause,
            torrent::torrent_resume,
            torrent::torrent_remove,
            torrent::torrent_init,
            torrent::torrent_default_dir,
            torrent::torrent_check_dir,
            torrent::local_file_size,
            torrent::disk_free,
            player::imp::mpv_play,
            player::imp::mpv_play_trailer,
            player::imp::mpv_play_iptv,
            player::imp::mpv_cmd,
            player::imp::mpv_open_picker,
            player::imp::mpv_stop,
            player::imp::mpv_suspend,
            player::imp::mpv_resume,
            player::imp::mpv_session,
            player::imp::mpv_running,
            player::imp::mpv_status,
            player::imp::mpv_tracks,
            player::imp::mpv_cache_stats,
            player::imp::mpv_set_cache,
            opensubtitles::os_login,
            opensubtitles::os_status,
            opensubtitles::os_clear,
            opensubtitles::os_search,
            opensubtitles::os_list,
            opensubtitles::os_download,
            opensubtitles::subtitle_save,
            screening::screening_enqueue,
            screening::screening_get_unavailable,
            screening::screening_set_paused,
            screening::screening_set_concurrency,
            awards::wikidata_awards,
            wyzie::wyzie_search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

