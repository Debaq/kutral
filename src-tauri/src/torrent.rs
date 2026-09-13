// Cliente torrent local — plan B cuando RealDebrid bloquea un hash por DMCA (451).
//
// El 451 de RD es cumplimiento legal por-infohash, NO falta de disponibilidad:
// el swarm sigue vivo y el mismo torrent se baja sin problemas peer-a-peer. Este
// módulo baja ese torrent con librqbit y lo sirve por HTTP en 127.0.0.1 (puerto
// efímero) MIENTRAS se descarga. librqbit prioriza las piezas cercanas a la
// posición del stream abierto, así que la descarga es secuencial de facto y mpv
// puede reproducir tras unos segundos de buffer inicial: mismo flujo que RD.
//
// AVISO DE PRIVACIDAD: sin debrid de por medio tu IP queda expuesta en el swarm
// (RD hacía de proxy). Por eso el frontend lo trata como opt-in explícito.
//
// Flujo: torrent_add (magnet → metadata → elige el video más grande →
// only_files) → prebuffer en segundo plano → torrent_status hasta buffer_ready
// → mpv abre stream_url.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::TcpListener;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig,
};
use serde::Serialize;
use tauri::Manager;
use tiny_http::{Header, Method, Response, Server, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Tope de subida. No la desactivamos (el tit-for-tat de BitTorrent premia
/// subir: sin dar nada bajas más lento), pero la capamos para no ahogar la
/// conexión del usuario mientras ve la película.
const UPLOAD_BPS: u32 = 512 * 1024;

/// Máximo esperando la metadata del magnet (DHT + trackers pueden tardar). Si
/// se pasa, el torrent está muerto o sin peers y el frontend prueba otro: por
/// eso no conviene alargarlo, cada espera se paga entera.
const META_TIMEOUT_S: u64 = 60;

/// Buffer inicial por defecto antes de lanzar mpv, en MB.
pub const BUFFER_MB_DEFAULT: u64 = 24;

const VIDEO_EXT: [&str; 7] = [".mkv", ".mp4", ".avi", ".mov", ".m4v", ".ts", ".webm"];

// ---- Estado global -------------------------------------------------------

/// Datos nuestros por torrent. Todo lo que se puede derivar del handle de
/// librqbit (nombre, tamaño, progreso) NO se guarda acá: se lee en vivo. Así
/// los torrents restaurados de la sesión persistida funcionan sin rehidratar.
struct Entry {
    /// Título lindo para la UI (el de la ficha, no el del release).
    title: String,
    file_id: usize,
    added_at: i64,
    buffered: Arc<AtomicU64>,
    buffer_target: Arc<AtomicU64>,
}

pub struct Tor {
    session: Arc<Session>,
    /// Puerto del servidor HTTP local que le sirve el archivo a mpv.
    port: u16,
    entries: Mutex<HashMap<usize, Entry>>,
}

static TOR: tokio::sync::OnceCell<Arc<Tor>> = tokio::sync::OnceCell::const_new();

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Carpeta destino por defecto: <Descargas>/Kutral, o <app_data>/torrents.
fn default_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Ok(d) = app.path().download_dir() {
        return Ok(d.join("Kutral"));
    }
    app.path()
        .app_data_dir()
        .map(|p| p.join("torrents"))
        .map_err(|e| format!("app_data_dir: {e}"))
}

/// Carpeta efectiva: la que eligió el usuario en Configuración, o la de por
/// defecto si no eligió ninguna. La preferencia vive en el frontend (como el
/// resto de la config) y llega en cada llamada.
fn output_dir(app: &tauri::AppHandle, custom: Option<&str>) -> Result<PathBuf, String> {
    match custom.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => default_dir(app),
    }
}

async fn init(app: &tauri::AppHandle, custom: Option<&str>) -> Result<Arc<Tor>, String> {
    let dir = output_dir(app, custom)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("crear {}: {e}", dir.display()))?;

    // Sesión persistida: la cola de descargas sobrevive a reiniciar la app.
    let state_dir = app.path().app_data_dir().ok().map(|p| p.join("torrent-session"));
    let opts = SessionOptions {
        persistence: Some(SessionPersistenceConfig::Json { folder: state_dir }),
        fastresume: true,
        ratelimits: LimitsConfig {
            upload_bps: NonZeroU32::new(UPLOAD_BPS),
            download_bps: None,
        },
        ..Default::default()
    };
    let session = Session::new_with_opts(dir.clone(), opts)
        .await
        .map_err(|e| format!("sesión torrent: {e:#}"))?;

    let port = start_stream_server(session.clone())?;
    eprintln!("[torrent] sesión lista, stream en 127.0.0.1:{port}, destino {}", dir.display());

    Ok(Arc::new(Tor {
        session,
        port,
        entries: Mutex::new(HashMap::new()),
    }))
}

async fn ensure(app: &tauri::AppHandle, custom: Option<&str>) -> Result<Arc<Tor>, String> {
    TOR.get_or_try_init(|| init(app, custom)).await.cloned()
}

/// La sesión ya creada, sin crearla. Los comandos que operan sobre un torrent
/// existente usan esto: levantar DHT + listeners de BitTorrent es caro y no
/// debe pasar solo porque la UI consultó la cola.
fn existing() -> Result<Arc<Tor>, String> {
    TOR.get()
        .cloned()
        .ok_or_else(|| "la sesión torrent no está iniciada".to_string())
}

// ---- Servidor HTTP de streaming -----------------------------------------

/// Adaptador: expone el stream async de librqbit como `std::io::Read` para
/// tiny_http. Corre en su propio hilo (no dentro del runtime), así que
/// `Handle::block_on` es legal acá. El tipo concreto (`FileStream`) no está
/// re-exportado por librqbit, de ahí el `dyn`.
struct BlockingStream {
    rt: tokio::runtime::Handle,
    inner: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    remaining: u64,
}

impl Read for BlockingStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let cap = buf.len().min(self.remaining as usize);
        let n = self.rt.block_on(self.inner.read(&mut buf[..cap]))?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header válido")
}

fn mime_for(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.ends_with(".mp4") || l.ends_with(".m4v") {
        "video/mp4"
    } else if l.ends_with(".webm") {
        "video/webm"
    } else if l.ends_with(".avi") {
        "video/x-msvideo"
    } else if l.ends_with(".ts") {
        "video/mp2t"
    } else {
        "video/x-matroska"
    }
}

/// "bytes=100-" / "bytes=100-200" → (inicio, fin inclusive). None si no aplica.
fn parse_range(h: Option<&str>, len: u64) -> Option<(u64, u64)> {
    let raw = h?.trim().strip_prefix("bytes=")?;
    let (a, b) = raw.split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    if start >= len {
        return None;
    }
    let end = match b.trim() {
        "" => len - 1,
        s => s.parse::<u64>().ok()?.min(len - 1),
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

/// Levanta el servidor de streaming en 127.0.0.1 con puerto efímero (solo
/// local) y devuelve el puerto. Debe llamarse dentro del runtime tokio: captura
/// su Handle para que los hilos del servidor puedan esperar al stream async.
fn start_stream_server(session: Arc<Session>) -> Result<u16, String> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("bind stream server: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    let server = Server::from_listener(listener, None).map_err(|e| format!("http server: {e}"))?;
    let rt = tokio::runtime::Handle::current();
    std::thread::spawn(move || serve_loop(server, session, rt));
    Ok(port)
}

fn serve_loop(server: Server, session: Arc<Session>, rt: tokio::runtime::Handle) {
    loop {
        let req = match server.recv() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[torrent] http recv: {e}");
                return;
            }
        };
        let sess = session.clone();
        let rt2 = rt.clone();
        // Un hilo por request: mpv abre varias conexiones (y un stream lento no
        // debe bloquear los demás).
        std::thread::spawn(move || {
            if let Err(e) = handle_req(req, sess, rt2) {
                eprintln!("[torrent] http: {e}");
            }
        });
    }
}

fn handle_req(
    req: tiny_http::Request,
    session: Arc<Session>,
    rt: tokio::runtime::Handle,
) -> Result<(), String> {
    // Ruta: /t/{torrent_id}/{file_id}[/nombre.mkv]
    let url = req.url().to_string();
    let parts: Vec<&str> = url.trim_start_matches('/').split('/').collect();
    let bad = |req: tiny_http::Request, code: u16, msg: &str| -> Result<(), String> {
        req.respond(Response::from_string(msg).with_status_code(StatusCode(code)))
            .map_err(|e| format!("respond: {e}"))
    };
    if parts.len() < 3 || parts[0] != "t" {
        return bad(req, 404, "no");
    }
    let (id, file_id) = match (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return bad(req, 400, "ruta inválida"),
    };
    let handle = match session.get(TorrentIdOrHash::Id(id)) {
        Some(h) => h,
        None => return bad(req, 404, "torrent no encontrado"),
    };
    let info = handle
        .with_metadata(|m| {
            m.file_infos.get(file_id).map(|f| {
                (
                    f.len,
                    f.relative_filename.to_string_lossy().to_string(),
                )
            })
        })
        .ok()
        .flatten();
    let (flen, fname) = match info {
        Some(x) => x,
        None => return bad(req, 404, "archivo no encontrado"),
    };

    let range_hdr = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string());
    let range = parse_range(range_hdr.as_deref(), flen);
    // Range presente pero imposible de satisfacer: 416, no el archivo entero.
    if range.is_none() && range_hdr.is_some() {
        let resp = Response::from_string("rango inválido")
            .with_status_code(StatusCode(416))
            .with_header(header("Content-Range", &format!("bytes */{flen}")));
        return req.respond(resp).map_err(|e| format!("respond: {e}"));
    }
    let (start, end) = range.unwrap_or((0, flen.saturating_sub(1)));
    let len = end - start + 1;

    let mut headers = vec![
        header("Accept-Ranges", "bytes"),
        header("Content-Type", mime_for(&fname)),
    ];
    let status = if range.is_some() {
        headers.push(header(
            "Content-Range",
            &format!("bytes {start}-{end}/{flen}"),
        ));
        206
    } else {
        200
    };

    // HEAD: mpv a veces sondea antes de pedir el cuerpo. Solo cabeceras.
    if *req.method() == Method::Head {
        let resp = Response::new(
            StatusCode(status),
            headers,
            std::io::empty(),
            Some(len as usize),
            None,
        );
        return req.respond(resp).map_err(|e| format!("respond: {e}"));
    }

    let stream = rt.block_on(async {
        let mut s = handle
            .clone()
            .stream(file_id)
            .await
            .map_err(|e| format!("abrir stream: {e:#}"))?;
        if start > 0 {
            s.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| format!("seek: {e}"))?;
        }
        Ok::<_, String>(s)
    });
    let stream = match stream {
        Ok(s) => s,
        Err(e) => return bad(req, 500, &e),
    };

    let body = BlockingStream {
        rt,
        inner: Box::new(stream),
        remaining: len,
    };
    let resp = Response::new(StatusCode(status), headers, body, Some(len as usize), None);
    // Un error acá suele ser mpv cerrando la conexión (seek del usuario): normal.
    req.respond(resp).map_err(|e| format!("respond: {e}"))
}

// ---- Selección de archivo y prebuffer ------------------------------------

fn is_video(p: &str) -> bool {
    let l = p.to_lowercase();
    VIDEO_EXT.iter().any(|e| l.ends_with(e))
}

/// Índice del archivo de video más grande (o del más grande a secas si el
/// torrent no trae extensiones reconocibles).
fn pick_file(handle: &Arc<ManagedTorrent>) -> Result<(usize, String, u64), String> {
    handle
        .with_metadata(|m| {
            let mut best: Option<(usize, String, u64)> = None;
            let mut best_any: Option<(usize, String, u64)> = None;
            for (i, f) in m.file_infos.iter().enumerate() {
                let name = f.relative_filename.to_string_lossy().to_string();
                if best_any.as_ref().is_none_or(|b| f.len > b.2) {
                    best_any = Some((i, name.clone(), f.len));
                }
                if is_video(&name) && best.as_ref().is_none_or(|b| f.len > b.2) {
                    best = Some((i, name, f.len));
                }
            }
            best.or(best_any)
        })
        .map_err(|e| format!("metadata: {e:#}"))?
        .ok_or_else(|| "el torrent no trae archivos".to_string())
}

/// Baja los primeros `target` bytes del archivo abriendo un stream propio: eso
/// activa la priorización secuencial de librqbit y deja listo el arranque de
/// mpv. Corre en segundo plano y publica el avance en `buffered`.
fn start_prebuffer(
    handle: Arc<ManagedTorrent>,
    file_id: usize,
    target: u64,
    buffered: Arc<AtomicU64>,
) {
    tokio::spawn(async move {
        let mut s = match handle.stream(file_id).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[torrent] prebuffer, abrir stream: {e:#}");
                return;
            }
        };
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        while done < target {
            match s.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    done += n as u64;
                    buffered.store(done, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("[torrent] prebuffer: {e}");
                    break;
                }
            }
        }
        eprintln!("[torrent] prebuffer listo: {} MB", done / 1_048_576);
    });
}

// ---- Tipos de salida -----------------------------------------------------

#[derive(Serialize)]
pub struct TorrentAdded {
    pub id: usize,
    pub file_id: usize,
    pub name: String,
    pub size_bytes: u64,
    pub stream_url: String,
    /// Ruta absoluta del archivo elegido en disco. Es lo que permite volver a
    /// verlo sin red una vez terminada la descarga (tabla `descargas`).
    pub path: String,
    /// Infohash en hex: identifica la descarga entre reinicios, cuando los ids
    /// de sesión ya no valen.
    pub info_hash: String,
}

#[derive(Serialize)]
pub struct TorrentStatus {
    pub id: usize,
    pub title: String,
    pub name: String,
    pub stream_url: String,
    pub size_bytes: u64,
    pub progress_bytes: u64,
    /// 0..100 sobre los archivos seleccionados.
    pub pct: f64,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub peers: u32,
    /// Segundos estimados para terminar. null si no hay velocidad todavía.
    pub eta_secs: Option<u64>,
    /// "initializing" | "live" | "paused" | "error"
    pub state: String,
    pub error: Option<String>,
    pub finished: bool,
    pub paused: bool,
    pub buffered_bytes: u64,
    pub buffer_target: u64,
    pub buffer_ready: bool,
    pub added_at: i64,
    /// Ruta absoluta del archivo en disco (sirve igual para los torrents
    /// restaurados de la sesión persistida, que no tienen Entry nuestro).
    pub path: String,
    pub info_hash: String,
}

fn status_of(tor: &Tor, id: usize, handle: &Arc<ManagedTorrent>) -> TorrentStatus {
    let stats = handle.stats();
    let (file_id, title, added_at, buffered, target) = {
        let map = tor.entries.lock().expect("entries lock");
        match map.get(&id) {
            Some(e) => (
                e.file_id,
                e.title.clone(),
                e.added_at,
                e.buffered.load(Ordering::Relaxed),
                e.buffer_target.load(Ordering::Relaxed),
            ),
            // Torrent restaurado de la sesión persistida: sin datos nuestros.
            None => (
                handle.only_files().and_then(|v| v.first().copied()).unwrap_or(0),
                String::new(),
                0,
                0,
                0,
            ),
        }
    };
    let name = handle
        .with_metadata(|m| {
            m.file_infos
                .get(file_id)
                .map(|f| f.relative_filename.to_string_lossy().to_string())
        })
        .ok()
        .flatten()
        .or_else(|| handle.name())
        .unwrap_or_default();

    let (down, up, peers) = match &stats.live {
        Some(l) => (
            l.download_speed.as_bytes(),
            l.upload_speed.as_bytes(),
            l.snapshot.peer_stats.live,
        ),
        None => (0, 0, 0),
    };
    let pending = stats.total_bytes.saturating_sub(stats.progress_bytes);
    let eta_secs = if down > 0 && pending > 0 {
        Some(pending / down)
    } else {
        None
    };
    let state = match stats.state {
        librqbit::TorrentStatsState::Initializing { .. } => "initializing",
        librqbit::TorrentStatsState::Live => "live",
        librqbit::TorrentStatsState::Paused => "paused",
        librqbit::TorrentStatsState::Error => "error",
    };
    let pct = if stats.total_bytes > 0 {
        stats.progress_bytes as f64 * 100.0 / stats.total_bytes as f64
    } else {
        0.0
    };

    let path = handle
        .output_folder()
        .join(&name)
        .to_string_lossy()
        .to_string();

    TorrentStatus {
        id,
        title,
        stream_url: stream_url(tor.port, id, file_id, &name),
        name,
        size_bytes: stats.total_bytes,
        progress_bytes: stats.progress_bytes,
        pct,
        download_bps: down,
        upload_bps: up,
        peers,
        eta_secs,
        state: state.to_string(),
        error: stats.error.clone(),
        finished: stats.finished,
        paused: handle.is_paused(),
        buffered_bytes: buffered,
        buffer_target: target,
        // Con el archivo ya terminado el buffer sobra.
        buffer_ready: stats.finished || (target > 0 && buffered >= target),
        added_at,
        path,
        info_hash: handle.info_hash().as_string(),
    }
}

fn stream_url(port: u16, id: usize, file_id: usize, name: &str) -> String {
    // El nombre va solo para que mpv adivine el contenedor por extensión; el
    // servidor ignora ese segmento.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("video.mkv");
    format!(
        "http://127.0.0.1:{port}/t/{id}/{file_id}/{}",
        urlencoding::encode(base)
    )
}

// ---- Comandos Tauri ------------------------------------------------------

/// Agrega un magnet, elige el archivo de video más grande y arranca el buffer
/// inicial. Devuelve la URL local que mpv puede abrir (aún descargando).
#[tauri::command]
pub async fn torrent_add(
    app: tauri::AppHandle,
    magnet: String,
    title: Option<String>,
    buffer_mb: Option<u64>,
    dir: Option<String>,
) -> Result<TorrentAdded, String> {
    let tor = ensure(&app, dir.as_deref()).await?;
    eprintln!("[torrent] add {}", &magnet.chars().take(60).collect::<String>());

    // La carpeta se manda por torrent, no solo al crear la sesión: así cambiarla
    // en Configuración vale desde la próxima descarga, sin reiniciar la app.
    let destino = output_dir(&app, dir.as_deref())?;
    std::fs::create_dir_all(&destino)
        .map_err(|e| format!("crear {}: {e}", destino.display()))?;

    let resp = tor
        .session
        .add_torrent(
            AddTorrent::from_url(magnet.clone()),
            Some(AddTorrentOptions {
                // Necesario para reanudar/sembrar sobre archivos ya escritos.
                overwrite: true,
                output_folder: Some(destino.to_string_lossy().to_string()),
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| format!("agregar torrent: {e:#}"))?;
    let handle = resp
        .into_handle()
        .ok_or_else(|| "el torrent no quedó gestionado".to_string())?;
    let id = handle.id();

    // Esperar la metadata del magnet (DHT/trackers).
    tokio::time::timeout(
        std::time::Duration::from_secs(META_TIMEOUT_S),
        handle.wait_until_initialized(),
    )
    .await
    .map_err(|_| "sin metadata del magnet (timeout): torrent muerto o sin peers".to_string())?
    .map_err(|e| format!("inicializar torrent: {e:#}"))?;

    let (file_id, name, size_bytes) = pick_file(&handle)?;
    eprintln!("[torrent]   elegido #{file_id} {name} ({} MB)", size_bytes / 1_048_576);

    // Bajar SOLO ese archivo (los torrents de pack traen extras y otros idiomas).
    let only: HashSet<usize> = HashSet::from([file_id]);
    tor.session
        .update_only_files(&handle, &only)
        .await
        .map_err(|e| format!("seleccionar archivo: {e:#}"))?;

    let target = (buffer_mb.unwrap_or(BUFFER_MB_DEFAULT) * 1_048_576).min(size_bytes);
    let buffered = Arc::new(AtomicU64::new(0));
    tor.entries.lock().expect("entries lock").insert(
        id,
        Entry {
            title: title.unwrap_or_else(|| name.clone()),
            file_id,
            added_at: now_secs(),
            buffered: buffered.clone(),
            buffer_target: Arc::new(AtomicU64::new(target)),
        },
    );
    start_prebuffer(handle.clone(), file_id, target, buffered);

    let path = destino.join(&name).to_string_lossy().to_string();

    Ok(TorrentAdded {
        id,
        file_id,
        stream_url: stream_url(tor.port, id, file_id, &name),
        name,
        size_bytes,
        path,
        info_hash: handle.info_hash().as_string(),
    })
}

#[tauri::command]
pub async fn torrent_status(id: usize) -> Result<TorrentStatus, String> {
    let tor = existing()?;
    let handle = tor
        .session
        .get(TorrentIdOrHash::Id(id))
        .ok_or_else(|| "torrent no encontrado".to_string())?;
    Ok(status_of(&tor, id, &handle))
}

/// Todas las descargas de la sesión (la cola), más recientes primero. Con la
/// sesión sin iniciar devuelve vacío: consultar la cola no debe levantarla.
#[tauri::command]
pub async fn torrent_list() -> Result<Vec<TorrentStatus>, String> {
    let tor = match TOR.get() {
        Some(t) => t.clone(),
        None => return Ok(Vec::new()),
    };
    let handles: Vec<(usize, Arc<ManagedTorrent>)> = tor
        .session
        .with_torrents(|it| it.map(|(id, h)| (id, h.clone())).collect());
    let mut out: Vec<TorrentStatus> = handles
        .iter()
        .map(|(id, h)| status_of(&tor, *id, h))
        .collect();
    out.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(b.id.cmp(&a.id)));
    Ok(out)
}

#[tauri::command]
pub async fn torrent_pause(id: usize) -> Result<(), String> {
    let tor = existing()?;
    let handle = tor
        .session
        .get(TorrentIdOrHash::Id(id))
        .ok_or_else(|| "torrent no encontrado".to_string())?;
    tor.session
        .pause(&handle)
        .await
        .map_err(|e| format!("pausar: {e:#}"))
}

#[tauri::command]
pub async fn torrent_resume(id: usize) -> Result<(), String> {
    let tor = existing()?;
    let handle = tor
        .session
        .get(TorrentIdOrHash::Id(id))
        .ok_or_else(|| "torrent no encontrado".to_string())?;
    tor.session
        .unpause(&handle)
        .await
        .map_err(|e| format!("reanudar: {e:#}"))
}

/// Saca el torrent de la cola. Con `delete_files` borra también lo descargado.
#[tauri::command]
pub async fn torrent_remove(id: usize, delete_files: bool) -> Result<(), String> {
    let tor = existing()?;
    tor.session
        .delete(TorrentIdOrHash::Id(id), delete_files)
        .await
        .map_err(|e| format!("eliminar: {e:#}"))?;
    tor.entries.lock().expect("entries lock").remove(&id);
    Ok(())
}

/// Levanta la sesión torrent (DHT, listeners) y restaura la cola persistida.
/// Solo lo llama el frontend cuando el usuario tiene la descarga local activa:
/// sin eso, la app nunca abre un socket de BitTorrent.
#[tauri::command]
pub async fn torrent_init(app: tauri::AppHandle, dir: Option<String>) -> Result<usize, String> {
    let tor = ensure(&app, dir.as_deref()).await?;
    Ok(tor.session.with_torrents(|it| it.count()))
}

/// Carpeta por defecto de las descargas, para mostrarla como sugerencia en
/// Configuración cuando el usuario no eligió ninguna.
#[tauri::command]
pub async fn torrent_default_dir(app: tauri::AppHandle) -> Result<String, String> {
    Ok(default_dir(&app)?.to_string_lossy().to_string())
}

/// ¿Se puede escribir en esta carpeta? La crea si no existe y prueba un archivo
/// temporal: así Configuración avisa al elegirla y no al primer bloqueo DMCA.
#[tauri::command]
pub async fn torrent_check_dir(app: tauri::AppHandle, dir: Option<String>) -> Result<String, String> {
    let d = output_dir(&app, dir.as_deref())?;
    std::fs::create_dir_all(&d).map_err(|e| format!("no se pudo crear: {e}"))?;
    let probe = d.join(".kutral-escritura");
    std::fs::write(&probe, b"ok").map_err(|e| format!("no se puede escribir ahí: {e}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(d.to_string_lossy().to_string())
}

/// ¿Sigue ahí el archivo de una descarga vieja? Devuelve su tamaño, o null si
/// no existe (el usuario lo borró a mano, cambió la carpeta, montó otro disco).
/// Sin esto la app ofrecería "ya lo tienes" apuntando a un archivo fantasma.
#[tauri::command]
pub fn local_file_size(path: String) -> Option<u64> {
    std::fs::metadata(&path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// Bytes libres en la partición donde caen las descargas. Bajar una temporada
/// entera son decenas de GB: llenar el disco del equipo rompe bastante más que
/// la descarga, así que la cola pregunta antes de cada una.
#[tauri::command]
pub fn disk_free(app: tauri::AppHandle, dir: Option<String>) -> Result<u64, String> {
    let d = output_dir(&app, dir.as_deref())?;
    // Si la carpeta todavía no existe, la partición de su padre sirve igual.
    let objetivo = if d.exists() {
        d
    } else {
        d.parent().map(PathBuf::from).unwrap_or(d)
    };
    espacio_libre(&objetivo)
}

#[cfg(unix)]
fn espacio_libre(p: &std::path::Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes())
        .map_err(|e| format!("ruta inválida: {e}"))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(format!("statvfs: {}", std::io::Error::last_os_error()));
    }
    // f_bavail (no f_bfree): los bloques reservados para root no son nuestros.
    Ok(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(unix))]
fn espacio_libre(_p: &std::path::Path) -> Result<u64, String> {
    Err("espacio libre no disponible en esta plataforma".to_string())
}

#[cfg(test)]
mod tests {
    use super::{is_video, local_file_size, mime_for, parse_range, stream_url};

    #[test]
    fn range_abierto_llega_al_final() {
        assert_eq!(parse_range(Some("bytes=0-"), 1000), Some((0, 999)));
        assert_eq!(parse_range(Some("bytes=500-"), 1000), Some((500, 999)));
    }

    #[test]
    fn range_cerrado_se_recorta_al_tamano() {
        assert_eq!(parse_range(Some("bytes=10-20"), 1000), Some((10, 20)));
        assert_eq!(parse_range(Some("bytes=10-99999"), 1000), Some((10, 999)));
    }

    #[test]
    fn range_invalido_es_none() {
        assert_eq!(parse_range(None, 1000), None);
        assert_eq!(parse_range(Some("items=0-1"), 1000), None);
        // Inicio fuera del archivo, o fin antes del inicio.
        assert_eq!(parse_range(Some("bytes=1000-"), 1000), None);
        assert_eq!(parse_range(Some("bytes=50-10"), 1000), None);
    }

    #[test]
    fn detecta_videos_por_extension() {
        assert!(is_video("Pelicula.1080p.MKV"));
        assert!(is_video("dir/algo.mp4"));
        assert!(!is_video("subs.srt"));
        assert!(!is_video("RARBG.txt"));
    }

    #[test]
    fn mime_segun_contenedor() {
        assert_eq!(mime_for("a.mp4"), "video/mp4");
        assert_eq!(mime_for("a.webm"), "video/webm");
        // Lo desconocido cae en matroska: es el contenedor típico de escena.
        assert_eq!(mime_for("a.mkv"), "video/x-matroska");
        assert_eq!(mime_for("a.raro"), "video/x-matroska");
    }

    /// Camino completo REAL contra la red: magnet → metadata → selección de
    /// archivo → servidor HTTP → Range como los que manda mpv. Usa Sintel
    /// (Blender, CC-BY), que está bien sembrado. Ignorado por defecto: depende
    /// de internet y de que haya peers.
    ///
    /// Correr con: cargo test --lib torrent::tests::e2e -- --ignored --nocapture
    #[test]
    #[ignore = "necesita red y peers"]
    fn e2e_stream_por_http() {
        const SINTEL: &str = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10\
&dn=Sintel&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce\
&tr=udp%3A%2F%2Fexplodie.org%3A6969";

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = std::env::temp_dir().join("kutral-torrent-e2e");
            std::fs::create_dir_all(&dir).unwrap();
            let session = super::Session::new(dir).await.expect("crear sesión");
            let port = super::start_stream_server(session.clone()).expect("servidor");

            let handle = session
                .add_torrent(
                    super::AddTorrent::from_url(SINTEL),
                    Some(super::AddTorrentOptions {
                        overwrite: true,
                        ..Default::default()
                    }),
                )
                .await
                .expect("add_torrent")
                .into_handle()
                .expect("handle");

            tokio::time::timeout(
                std::time::Duration::from_secs(120),
                handle.wait_until_initialized(),
            )
            .await
            .expect("timeout esperando metadata")
            .expect("inicializar");

            let (file_id, name, len) = super::pick_file(&handle).expect("elegir archivo");
            println!("archivo elegido: #{file_id} {name} ({len} bytes)");
            assert!(super::is_video(&name), "debió elegir el video, eligió {name}");

            let url = super::stream_url(port, handle.id(), file_id, &name);
            println!("url: {url}");

            // Range parcial, como el primer pedido de mpv.
            let body = tokio::task::spawn_blocking(move || {
                let cli = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(180))
                    .build()
                    .unwrap();
                let r = cli
                    .get(&url)
                    .header("Range", "bytes=0-65535")
                    .send()
                    .expect("GET");
                assert_eq!(r.status().as_u16(), 206, "debe responder 206 Partial");
                let cr = r
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                assert!(cr.starts_with("bytes 0-65535/"), "content-range: {cr}");
                r.bytes().expect("cuerpo")
            })
            .await
            .expect("spawn_blocking");

            assert_eq!(body.len(), 65536, "debe devolver exactamente el rango pedido");
            // Un .mp4 real empieza con un box ftyp en los bytes 4..8.
            assert_eq!(&body[4..8], b"ftyp", "no parece un mp4: {:?}", &body[..12]);
            println!("primeros bytes: {:?}", &body[..12]);

            // Salto a mitad del archivo: es el camino que rompe si el seek del
            // stream está mal (mpv lo hace apenas el usuario mueve la barra).
            let medio = len / 2;
            let url2 = super::stream_url(port, handle.id(), file_id, &name);
            let body2 = tokio::task::spawn_blocking(move || {
                let cli = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .build()
                    .unwrap();
                let r = cli
                    .get(&url2)
                    .header("Range", format!("bytes={medio}-{}", medio + 32767))
                    .send()
                    .expect("GET seek");
                assert_eq!(r.status().as_u16(), 206);
                r.bytes().expect("cuerpo seek")
            })
            .await
            .expect("spawn_blocking seek");
            assert_eq!(body2.len(), 32768, "el seek debe devolver el rango pedido");
            println!("seek a {medio}: {} bytes ok", body2.len());
        });
    }

    #[test]
    fn tamano_solo_de_archivos_que_existen() {
        let dir = std::env::temp_dir().join("kutral-test-local-file");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("peli.mkv");
        std::fs::write(&f, b"1234567890").unwrap();

        assert_eq!(local_file_size(f.to_string_lossy().to_string()), Some(10));
        // Una carpeta NO es una copia local, aunque exista.
        assert_eq!(local_file_size(dir.to_string_lossy().to_string()), None);
        assert_eq!(
            local_file_size(dir.join("no-existe.mkv").to_string_lossy().to_string()),
            None
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_usa_solo_el_nombre_del_archivo_escapado() {
        let u = stream_url(9000, 3, 1, "Carpeta/Mi Peli [1080p].mkv");
        assert_eq!(
            u,
            "http://127.0.0.1:9000/t/3/1/Mi%20Peli%20%5B1080p%5D.mkv"
        );
    }
}
