// descarga_http.rs — descargas por HTTP: lo que entrega Real-Debrid.
//
// Real-Debrid convierte un magnet (rd_resolver) o un link de hoster (Mega,
// Mediafire… vía rd_unrestrict) en una URL directa. Bajar esa URL es más rápido
// que el swarm y no expone la IP de nadie. Hasta ahora la cola solo sabía de
// torrents; esto suma descargas HTTP que se ven en la MISMA cola:
// `torrent_list` las incluye con ids desde ID_BASE y un info_hash "http-…",
// y pausar/reanudar/quitar se enrutan acá por id. Así la ficha, la lista de
// capítulos y "ver mientras baja" funcionan sin cambios.
//
//  - Se reanuda con Range sobre un `.part`; al terminar se renombra.
//  - Las URL de RD vencen: ante 403/404/410 se vuelve a pedir desde el
//    origen (magnet o hoster), que es lo que queda guardado.
//  - La cola persiste en app_data/descargas-http.json: al abrir la app,
//    `http_restaurar` retoma lo que quedó a medias.

use crate::torrent::{TorrentAdded, TorrentStatus};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::Manager;

/// Los ids de librqbit son chicos y consecutivos: desde acá son nuestros.
pub const ID_BASE: usize = 1_000_000;

/// De dónde sale la URL. Se guarda esto (no la URL, que vence) para poder
/// pedirla de nuevo a Real-Debrid.
#[derive(Serialize, Deserialize, Clone, Debug, Hash)]
#[serde(tag = "tipo", rename_all = "lowercase")]
pub enum Origen {
    /// Copias en hosters: se prueban en orden con rd_unrestrict.
    Links { links: Vec<String> },
    /// Torrent resuelto por RD; la pista elige el capítulo en un pack.
    Magnet {
        magnet: String,
        #[serde(default)]
        file_idx: Option<u32>,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        season: Option<u32>,
        #[serde(default)]
        episode: Option<u32>,
    },
    /// URL directa que no vence.
    Url { url: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Guardada {
    id: usize,
    info_hash: String,
    origen: Origen,
    titulo: String,
    nombre: String,
    ruta: String,
    size: u64,
    added_at: i64,
    terminada: bool,
    pausada: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    error: Option<String>,
}

struct Bajada {
    g: Mutex<Guardada>,
    progreso: AtomicU64,
    bps: AtomicU64,
    parar: AtomicBool,
    corriendo: AtomicBool,
    resolviendo: AtomicBool,
}

static BAJADAS: Mutex<Vec<Arc<Bajada>>> = Mutex::new(Vec::new());
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static RESTAURADA: AtomicBool = AtomicBool::new(false);

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn archivo_estado() -> Option<PathBuf> {
    APP.get()?.path().app_data_dir().ok().map(|d| d.join("descargas-http.json"))
}

fn guardar() {
    let Some(f) = archivo_estado() else { return };
    let lista: Vec<Guardada> = BAJADAS
        .lock()
        .map(|v| v.iter().filter_map(|b| b.g.lock().ok().map(|g| g.clone())).collect())
        .unwrap_or_default();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    match serde_json::to_vec_pretty(&lista) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&f, json) {
                eprintln!("[http] no se pudo guardar la cola: {e}");
            }
        }
        Err(e) => eprintln!("[http] cola ilegible: {e}"),
    }
}

fn cliente() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // Sin timeout total: una película de 20 GB tarda lo que tarda. Lo que
        // corta un cuelgue es el timeout de lectura por pedazo (ver bajar()).
        .build()
        .map_err(|e| format!("cliente http: {e}"))
}

/// Pide (otra vez) la URL directa a Real-Debrid.
async fn resolver(origen: &Origen) -> Result<String, String> {
    let app = APP.get().ok_or("app sin iniciar")?.clone();
    match origen {
        Origen::Url { url } => Ok(url.clone()),
        Origen::Links { links } => {
            let mut ultimo = String::from("sin links");
            for l in links {
                match crate::rd::rd_unrestrict(app.clone(), l.clone()).await {
                    Ok(u) => return Ok(u),
                    Err(e) => ultimo = e,
                }
            }
            Err(format!("ningún hoster abrió en Real-Debrid: {ultimo}"))
        }
        Origen::Magnet { magnet, file_idx, filename, season, episode } => {
            let pista = crate::pack::Pista {
                file_idx: *file_idx,
                filename: filename.clone(),
                season: *season,
                episode: *episode,
            };
            crate::rd::rd_resolver(app, magnet.clone(), Some(pista)).await.map(|r| r.url)
        }
    }
}

/// Nombre de archivo desde la URL de RD (…/d/ID/Nombre.Del.Release.mkv).
fn nombre_de_url(url: &str) -> String {
    let sin_query = url.split(['?', '#']).next().unwrap_or(url);
    let ultimo = sin_query.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let dec = urlencoding::decode(ultimo).map(|c| c.into_owned()).unwrap_or_else(|_| ultimo.to_string());
    let limpio: String = dec
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect();
    let limpio = limpio.trim().trim_matches('.').to_string();
    if limpio.is_empty() {
        "descarga".into()
    } else {
        limpio
    }
}

/// Ruta libre en `dir`: si el nombre ya existe (o está bajando), le suma (2), (3)…
fn ruta_libre(dir: &Path, nombre: &str) -> PathBuf {
    let ocupadas: Vec<String> = BAJADAS
        .lock()
        .map(|v| v.iter().filter_map(|b| b.g.lock().ok().map(|g| g.ruta.clone())).collect())
        .unwrap_or_default();
    let (base, ext) = match nombre.rsplit_once('.') {
        Some((b, e)) if !b.is_empty() && e.len() <= 5 => (b.to_string(), format!(".{e}")),
        _ => (nombre.to_string(), String::new()),
    };
    let mut n = 1;
    loop {
        let cand = if n == 1 { dir.join(nombre) } else { dir.join(format!("{base} ({n}){ext}")) };
        let s = cand.to_string_lossy().to_string();
        if !cand.exists() && !Path::new(&format!("{s}.part")).exists() && !ocupadas.contains(&s) {
            return cand;
        }
        n += 1;
    }
}

/// Tamaño total según la respuesta (Content-Range de un 206, o el largo).
fn total_de(resp: &reqwest::Response, ya: u64) -> Option<u64> {
    if let Some(cr) = resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()) {
        if let Some(t) = cr.rsplit('/').next().and_then(|t| t.trim().parse().ok()) {
            return Some(t);
        }
    }
    resp.content_length().map(|l| if resp.status().as_u16() == 206 { l + ya } else { l })
}

async fn bajar(b: &Arc<Bajada>) -> Result<(), String> {
    let (origen, ruta, mut url) = {
        let g = b.g.lock().map_err(|_| "estado envenenado")?;
        (g.origen.clone(), g.ruta.clone(), g.url.clone())
    };
    if url.is_empty() {
        b.resolviendo.store(true, Ordering::Relaxed);
        let r = resolver(&origen).await;
        b.resolviendo.store(false, Ordering::Relaxed);
        url = r?;
        if let Ok(mut g) = b.g.lock() {
            g.url = url.clone();
        }
    }
    let part = PathBuf::from(format!("{ruta}.part"));
    if let Some(d) = part.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("crear carpeta: {e}"))?;
    }
    let cli = cliente()?;
    let mut re_resuelta = false;
    loop {
        let ya = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut req = cli.get(&url);
        if ya > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={ya}-"));
        }
        let mut resp = req.send().await.map_err(|e| format!("conexión: {e}"))?;
        let st = resp.status().as_u16();
        if matches!(st, 401 | 403 | 404 | 410) && !re_resuelta {
            // La URL de RD venció: se pide otra desde el origen.
            re_resuelta = true;
            b.resolviendo.store(true, Ordering::Relaxed);
            let r = resolver(&origen).await;
            b.resolviendo.store(false, Ordering::Relaxed);
            url = r?;
            if let Ok(mut g) = b.g.lock() {
                g.url = url.clone();
            }
            continue;
        }
        if st == 416 {
            // Pedimos desde el final: ya estaba todo.
            break;
        }
        if !(st == 200 || st == 206) {
            return Err(format!("el servidor respondió {st}"));
        }
        // 200 a un pedido con Range: el servidor no reanuda, se empieza de cero.
        let reanuda = st == 206 && ya > 0;
        let base = if reanuda { ya } else { 0 };
        if let Some(t) = total_de(&resp, base) {
            if let Ok(mut g) = b.g.lock() {
                g.size = t;
            }
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(reanuda)
            .truncate(!reanuda)
            .open(&part)
            .map_err(|e| format!("abrir {}: {e}", part.display()))?;
        let mut hecho = base;
        b.progreso.store(hecho, Ordering::Relaxed);
        let mut muestra = (Instant::now(), hecho);
        loop {
            if b.parar.load(Ordering::Relaxed) {
                let _ = f.flush();
                return Err("pausada".into());
            }
            let pedazo = tokio::time::timeout(Duration::from_secs(60), resp.chunk())
                .await
                .map_err(|_| "la descarga se quedó sin datos 60 s".to_string())?
                .map_err(|e| format!("lectura: {e}"))?;
            let Some(pedazo) = pedazo else { break };
            f.write_all(&pedazo).map_err(|e| format!("escribir: {e}"))?;
            hecho += pedazo.len() as u64;
            b.progreso.store(hecho, Ordering::Relaxed);
            let dt = muestra.0.elapsed();
            if dt >= Duration::from_secs(1) {
                let bps = ((hecho - muestra.1) as f64 / dt.as_secs_f64()) as u64;
                b.bps.store(bps, Ordering::Relaxed);
                muestra = (Instant::now(), hecho);
            }
        }
        f.flush().map_err(|e| format!("escribir: {e}"))?;
        let size = b.g.lock().map(|g| g.size).unwrap_or(0);
        if size > 0 && hecho < size {
            // Cortó antes de tiempo: se sigue desde donde quedó.
            continue;
        }
        break;
    }
    std::fs::rename(&part, &ruta).map_err(|e| format!("renombrar al terminar: {e}"))?;
    Ok(())
}

fn arrancar(b: Arc<Bajada>) {
    if b.corriendo.swap(true, Ordering::SeqCst) {
        return;
    }
    b.parar.store(false, Ordering::SeqCst);
    if let Ok(mut g) = b.g.lock() {
        g.pausada = false;
        g.error = None;
    }
    tauri::async_runtime::spawn(async move {
        let r = bajar(&b).await;
        b.bps.store(0, Ordering::Relaxed);
        if let Ok(mut g) = b.g.lock() {
            match r {
                Ok(()) => {
                    g.terminada = true;
                    g.error = None;
                    b.progreso.store(g.size.max(b.progreso.load(Ordering::Relaxed)), Ordering::Relaxed);
                    eprintln!("[http] terminada: {}", g.nombre);
                }
                Err(e) if e == "pausada" => g.pausada = true,
                Err(e) => {
                    eprintln!("[http] {}: {e}", g.nombre);
                    g.error = Some(e);
                }
            }
        }
        b.corriendo.store(false, Ordering::SeqCst);
        guardar();
    });
}

fn buscar(id: usize) -> Option<Arc<Bajada>> {
    BAJADAS.lock().ok()?.iter().find(|b| b.g.lock().map(|g| g.id == id).unwrap_or(false)).cloned()
}

fn estado_de(b: &Bajada) -> Option<TorrentStatus> {
    let g = b.g.lock().ok()?;
    let progreso = if g.terminada { g.size } else { b.progreso.load(Ordering::Relaxed) };
    let bps = b.bps.load(Ordering::Relaxed);
    let corriendo = b.corriendo.load(Ordering::Relaxed);
    let pct = if g.size > 0 { (progreso as f64 / g.size as f64 * 100.0).min(100.0) } else { 0.0 };
    let state = if g.error.is_some() && !corriendo {
        "error"
    } else if g.pausada && !corriendo {
        "paused"
    } else if b.resolviendo.load(Ordering::Relaxed) || (corriendo && progreso == 0) {
        "initializing"
    } else {
        "live"
    };
    let eta = (bps > 0 && g.size > progreso).then(|| (g.size - progreso) / bps);
    Some(TorrentStatus {
        id: g.id,
        title: g.titulo.clone(),
        name: g.nombre.clone(),
        // Mientras baja se puede ver directo desde Real-Debrid.
        stream_url: g.url.clone(),
        size_bytes: g.size,
        progress_bytes: progreso,
        pct,
        download_bps: if corriendo { bps } else { 0 },
        upload_bps: 0,
        peers: 0,
        eta_secs: eta,
        state: state.into(),
        error: g.error.clone(),
        finished: g.terminada,
        paused: g.pausada && !corriendo,
        buffered_bytes: progreso,
        buffer_target: 0,
        buffer_ready: true,
        added_at: g.added_at,
        path: g.ruta.clone(),
        info_hash: g.info_hash.clone(),
    })
}

/// Para `torrent_list`: las descargas HTTP con la misma forma que un torrent.
pub fn lista() -> Vec<TorrentStatus> {
    BAJADAS
        .lock()
        .map(|v| v.iter().filter_map(|b| estado_de(b.as_ref())).collect())
        .unwrap_or_default()
}

pub fn estado(id: usize) -> Result<TorrentStatus, String> {
    buscar(id).and_then(|b| estado_de(&b)).ok_or_else(|| "descarga no encontrada".into())
}

pub fn pausar(id: usize) -> Result<(), String> {
    let b = buscar(id).ok_or("descarga no encontrada")?;
    b.parar.store(true, Ordering::SeqCst);
    if let Ok(mut g) = b.g.lock() {
        g.pausada = true;
    }
    guardar();
    Ok(())
}

pub fn reanudar(id: usize) -> Result<(), String> {
    let b = buscar(id).ok_or("descarga no encontrada")?;
    if b.g.lock().map(|g| g.terminada).unwrap_or(false) {
        return Ok(());
    }
    arrancar(b);
    guardar();
    Ok(())
}

pub fn quitar(id: usize, borrar: bool) -> Result<(), String> {
    let b = buscar(id).ok_or("descarga no encontrada")?;
    b.parar.store(true, Ordering::SeqCst);
    let ruta = b.g.lock().map(|g| g.ruta.clone()).unwrap_or_default();
    if let Ok(mut v) = BAJADAS.lock() {
        v.retain(|x| !Arc::ptr_eq(x, &b));
    }
    if borrar && !ruta.is_empty() {
        let _ = std::fs::remove_file(&ruta);
        let _ = std::fs::remove_file(format!("{ruta}.part"));
    }
    guardar();
    Ok(())
}

fn registrar(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// Carga la cola guardada y retoma lo que quedó a medias. Idempotente.
/// Devuelve cuántas descargas hay.
#[tauri::command]
pub async fn http_restaurar(app: tauri::AppHandle) -> usize {
    registrar(&app);
    if RESTAURADA.swap(true, Ordering::SeqCst) {
        return BAJADAS.lock().map(|v| v.len()).unwrap_or(0);
    }
    let guardadas: Vec<Guardada> = archivo_estado()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut retomar = Vec::new();
    if let Ok(mut v) = BAJADAS.lock() {
        for mut g in guardadas {
            let ya = std::fs::metadata(format!("{}.part", g.ruta)).map(|m| m.len()).unwrap_or(0);
            if g.terminada && !Path::new(&g.ruta).exists() {
                // El usuario borró el archivo a mano: fuera de la cola.
                continue;
            }
            // La URL de la vez pasada probablemente venció: se pide de nuevo.
            g.url.clear();
            let pendiente = !g.terminada && !g.pausada && g.error.is_none();
            let b = Arc::new(Bajada {
                g: Mutex::new(g),
                progreso: AtomicU64::new(ya),
                bps: AtomicU64::new(0),
                parar: AtomicBool::new(false),
                corriendo: AtomicBool::new(false),
                resolviendo: AtomicBool::new(false),
            });
            if pendiente {
                retomar.push(b.clone());
            }
            v.push(b);
        }
    }
    for b in retomar {
        arrancar(b);
    }
    BAJADAS.lock().map(|v| v.len()).unwrap_or(0)
}

/// Empieza una descarga HTTP. Resuelve la URL ANTES de volver, así un DMCA o
/// un hoster caído se avisa al tiro y la cola puede probar por torrent.
#[tauri::command]
pub async fn http_descargar(
    app: tauri::AppHandle,
    origen: Origen,
    titulo: String,
    dir: Option<String>,
) -> Result<TorrentAdded, String> {
    registrar(&app);
    if !RESTAURADA.load(Ordering::SeqCst) {
        http_restaurar(app.clone()).await;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    origen.hash(&mut h);
    let info_hash = format!("http-{:016x}", h.finish());
    // Ya está en la cola (mismo magnet y capítulo, o mismos links): se reusa.
    let existente = BAJADAS
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|b| b.g.lock().map(|g| g.info_hash == info_hash).unwrap_or(false)).cloned());
    if let Some(b) = existente {
        let g = b.g.lock().map_err(|_| "estado envenenado")?.clone();
        if !g.terminada && !b.corriendo.load(Ordering::SeqCst) {
            arrancar(b.clone());
        }
        return Ok(TorrentAdded {
            id: g.id,
            file_id: 0,
            name: g.nombre,
            size_bytes: g.size,
            stream_url: g.url,
            path: g.ruta,
            info_hash,
            pack: Vec::new(),
            reutilizado: true,
        });
    }

    let url = resolver(&origen).await?;
    let nombre = nombre_de_url(&url);
    let carpeta = crate::torrent::output_dir(&app, dir.as_deref())?;
    std::fs::create_dir_all(&carpeta).map_err(|e| format!("crear {}: {e}", carpeta.display()))?;
    let ruta = ruta_libre(&carpeta, &nombre).to_string_lossy().to_string();

    // Tamaño antes de empezar (un byte con Range): la cola lo usa para el
    // espacio en disco y la ficha para mostrarlo.
    let mut size = 0;
    if let Ok(cli) = cliente() {
        if let Ok(Ok(r)) = tokio::time::timeout(
            Duration::from_secs(15),
            cli.get(&url).header(reqwest::header::RANGE, "bytes=0-0").send(),
        )
        .await
        {
            size = total_de(&r, 0).filter(|t| *t > 1).unwrap_or(0);
        }
    }

    let id = {
        let v = BAJADAS.lock().map_err(|_| "estado envenenado")?;
        ID_BASE + v.iter().filter_map(|b| b.g.lock().ok().map(|g| g.id - ID_BASE + 1)).max().unwrap_or(0)
    };
    let g = Guardada {
        id,
        info_hash: info_hash.clone(),
        origen,
        titulo,
        nombre: nombre.clone(),
        ruta: ruta.clone(),
        size,
        added_at: ahora(),
        terminada: false,
        pausada: false,
        url: url.clone(),
        error: None,
    };
    let b = Arc::new(Bajada {
        g: Mutex::new(g),
        progreso: AtomicU64::new(0),
        bps: AtomicU64::new(0),
        parar: AtomicBool::new(false),
        corriendo: AtomicBool::new(false),
        resolviendo: AtomicBool::new(false),
    });
    if let Ok(mut v) = BAJADAS.lock() {
        v.push(b.clone());
    }
    guardar();
    arrancar(b);
    eprintln!("[http] bajando {nombre} ({size} B) → {ruta}");
    Ok(TorrentAdded {
        id,
        file_id: 0,
        name: nombre,
        size_bytes: size,
        stream_url: url,
        path: ruta,
        info_hash,
        pack: Vec::new(),
        reutilizado: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombre_sale_de_la_url_de_rd() {
        assert_eq!(
            nombre_de_url("https://abc.download.real-debrid.com/d/XYZ/Superman.2025.1080p.Dual.mkv"),
            "Superman.2025.1080p.Dual.mkv"
        );
        assert_eq!(nombre_de_url("https://x.com/d/1/Pel%C3%ADcula%20(2025).mp4?t=1"), "Película (2025).mp4");
        assert_eq!(nombre_de_url("https://x.com/d/1/a%2Fb%3Ac.mkv"), "a_b_c.mkv");
        assert_eq!(nombre_de_url("https://x.com/"), "x.com");
        assert_eq!(nombre_de_url(""), "descarga");
    }

    #[test]
    fn origen_serializa_con_tipo() {
        let o: Origen = serde_json::from_str(r#"{"tipo":"links","links":["https://mega.nz/file/a#b"]}"#).unwrap();
        assert!(matches!(o, Origen::Links { .. }));
        let m: Origen = serde_json::from_str(r#"{"tipo":"magnet","magnet":"magnet:?xt=1","season":1,"episode":2}"#).unwrap();
        assert!(matches!(m, Origen::Magnet { season: Some(1), episode: Some(2), .. }));
    }
}
