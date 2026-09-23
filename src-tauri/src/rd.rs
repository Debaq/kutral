// RealDebrid — resolución magnet → URL directa reproducible + cache check.
//
// Portado de kodi-os/scraper/src/realdebrid.rs. El token vive en el store del
// backend (creds.rs) y nunca pasa por el webview. Toda llamada va por
// `con_token`, que lo renueva solo: antes de que venza (dura 24 h) y cuando RD
// lo rechaza antes de tiempo, que pasa al vincular la misma cuenta en otro
// equipo.
//
// Flujo resolve: addMagnet → selectFiles(video más grande) → poll hasta
// "downloaded" (instantáneo si está cacheado) → unrestrict → URL directa.

use serde::{Deserialize, Serialize};

const BASE: &str = "https://api.real-debrid.com/rest/1.0";
const OAUTH: &str = "https://api.real-debrid.com/oauth/v2";
const RD_GRANT_DEVICE: &str = "http://oauth.net/grant_type/device/1.0";
const UA: &str = "kutral-kodios/0.1";
const REQ_TIMEOUT_S: u64 = 20;
const POLL_MAX_S: u64 = 30;

#[derive(Deserialize)]
struct AddMagnetResp {
    id: String,
}

#[derive(Deserialize)]
struct TorrentInfo {
    status: String,
    files: Vec<TorrentFile>,
    links: Vec<String>,
}

#[derive(Deserialize)]
struct TorrentFile {
    id: u32,
    path: String,
    bytes: u64,
}

#[derive(Deserialize)]
struct Unrestrict {
    download: String, // URL directa reproducible
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(REQ_TIMEOUT_S))
        .build()
        .map_err(|e| format!("client: {e}"))
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn add_magnet(cli: &reqwest::Client, token: &str, magnet: &str) -> Result<String, String> {
    let r = cli
        .post(format!("{BASE}/torrents/addMagnet"))
        .header("Authorization", bearer(token))
        .form(&[("magnet", magnet)])
        .send()
        .await
        .map_err(|e| format!("addMagnet red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let body = r.text().await.unwrap_or_default();
        // 451 = RD bloquea ese hash por DMCA (takedown). Es por-torrent:
        // el front debe saltar a otra fuente.
        if st.as_u16() == 451 {
            return Err(format!("BLOQUEADO_DMCA: RD 451 :: {body}"));
        }
        return Err(format!("addMagnet {st}: {body}"));
    }
    let v: AddMagnetResp = r.json().await.map_err(|e| format!("addMagnet parse: {e}"))?;
    Ok(v.id)
}

async fn torrent_info(cli: &reqwest::Client, token: &str, id: &str) -> Result<TorrentInfo, String> {
    let r = cli
        .get(format!("{BASE}/torrents/info/{id}"))
        .header("Authorization", bearer(token))
        .send()
        .await
        .map_err(|e| format!("info red: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("info {}", r.status()));
    }
    r.json().await.map_err(|e| format!("info parse: {e}"))
}

async fn select_files(
    cli: &reqwest::Client,
    token: &str,
    id: &str,
    files: &str,
) -> Result<(), String> {
    let r = cli
        .post(format!("{BASE}/torrents/selectFiles/{id}"))
        .header("Authorization", bearer(token))
        .form(&[("files", files)])
        .send()
        .await
        .map_err(|e| format!("select red: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("selectFiles {}", r.status()));
    }
    Ok(())
}

async fn unrestrict(cli: &reqwest::Client, token: &str, link: &str) -> Result<String, String> {
    let r = cli
        .post(format!("{BASE}/unrestrict/link"))
        .header("Authorization", bearer(token))
        .form(&[("link", link)])
        .send()
        .await
        .map_err(|e| format!("unrestrict red: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("unrestrict {}", r.status()));
    }
    let v: Unrestrict = r.json().await.map_err(|e| format!("unrestrict parse: {e}"))?;
    Ok(v.download)
}

fn is_video(p: &str) -> bool {
    let l = p.to_lowercase();
    [".mkv", ".mp4", ".avi", ".mov", ".m4v", ".ts", ".webm"]
        .iter()
        .any(|e| l.ends_with(e))
}

/// Resuelve un magnet a URL directa reproducible. Elige el archivo de video
/// más grande. Falla si no está cacheado en RD (timeout ~30s).
async fn resolve_magnet(token: &str, magnet: &str) -> Result<String, String> {
    let hash = magnet
        .split("btih:")
        .nth(1)
        .map(|s| s.chars().take(40).collect::<String>())
        .unwrap_or_default();
    eprintln!("[rd] resolve magnet hash={hash}");
    // Log del magnet completo para verificar a mano que existe / es válido
    // (pegándolo en un cliente torrent o buscando el hash en su tracker).
    eprintln!("[rd]   magnet={magnet}");
    let cli = client()?;
    let id = match add_magnet(&cli, token, magnet).await {
        Ok(id) => {
            eprintln!("[rd]   addMagnet OK id={id}");
            id
        }
        Err(e) => {
            eprintln!("[rd]   addMagnet ERR: {e}");
            return Err(e);
        }
    };

    // Elegir archivo de video más grande
    let info = torrent_info(&cli, token, &id).await?;
    eprintln!("[rd]   files={} status={}", info.files.len(), info.status);
    let target = info
        .files
        .iter()
        .filter(|f| is_video(&f.path))
        .max_by_key(|f| f.bytes)
        .ok_or_else(|| {
            eprintln!("[rd]   sin archivo de video en torrent");
            "sin archivo de video".to_string()
        })?;
    eprintln!("[rd]   elegido: {} ({} MB)", target.path, target.bytes / 1_048_576);
    select_files(&cli, token, &id, &target.id.to_string()).await?;

    // Poll hasta "downloaded" (instantáneo si cacheado)
    let mut info = torrent_info(&cli, token, &id).await?;
    let mut waited = 0u64;
    while info.status != "downloaded" {
        eprintln!("[rd]   poll status={} ({}s)", info.status, waited);
        if matches!(
            info.status.as_str(),
            "error" | "virus" | "dead" | "magnet_error"
        ) {
            return Err(format!("RD torrent falló: {}", info.status));
        }
        if waited >= POLL_MAX_S {
            return Err("RD no cacheado / timeout".into());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        waited += 2;
        info = torrent_info(&cli, token, &id).await?;
    }

    let link = info.links.first().ok_or("sin link")?.clone();
    let url = unrestrict(&cli, token, &link).await?;
    eprintln!("[rd]   unrestrict OK → {url}");
    Ok(url)
}

/// Check batch de instant-availability. Devuelve los hashes (lowercase) que
/// tienen al menos una variante cacheada en RD.
async fn instant_available(token: &str, hashes: &[String]) -> Result<Vec<String>, String> {
    use std::collections::HashSet;
    let mut found: HashSet<String> = HashSet::new();
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let cli = client()?;
    for chunk in hashes.chunks(40) {
        let path = chunk
            .iter()
            .map(|h| h.to_lowercase())
            .collect::<Vec<_>>()
            .join("/");
        let resp = match cli
            .get(format!("{BASE}/torrents/instantAvailability/{path}"))
            .header("Authorization", bearer(token))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            // Token rechazado: que suba, así con_token renueva y reintenta.
            Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => {
                return Err(format!("instantAvailability {}", r.status()));
            }
            _ => continue,
        };
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if let Some(map) = v.as_object() {
            for (hash, val) in map {
                let has = match val {
                    serde_json::Value::Object(o) => o.values().any(|x| {
                        !x.is_null()
                            && x.as_object().map(|o2| !o2.is_empty()).unwrap_or(false)
                    }),
                    _ => false,
                };
                if has {
                    found.insert(hash.to_lowercase());
                }
            }
        }
    }
    eprintln!("[rd] instant_available: {}/{} cacheados", found.len(), hashes.len());
    Ok(found.into_iter().collect())
}

// ---- Token: renovación automática ---------------------------------------

const REVINCULAR: &str =
    "Real-Debrid rechazó la sesión y no se pudo renovar: vuelve a vincularlo en Configuración.";

// Margen antes del vencimiento: renovar un poco antes evita que una
// reproducción arranque con un token que muere a mitad de camino.
const MARGEN_VENCE_S: u64 = 300;

// Una sola renovación a la vez: al abrir la lista de fuentes salen varias
// llamadas juntas y cada una renovaría por su cuenta.
static RENOVANDO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn ahora_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// expires_at 0 = desconocido (token pegado a mano): se usa hasta que RD lo rechace.
fn por_vencer(c: &crate::creds::RdCreds) -> bool {
    c.expires_at != 0 && c.expires_at <= ahora_s() + MARGEN_VENCE_S
}

fn token_rechazado(e: &str) -> bool {
    e.contains("401 Unauthorized") || e.contains("bad_token")
}

/// Pide un access_token nuevo con el refresh_token del store y lo guarda.
/// `rechazado` es el token que RD acaba de rechazar (None = renovar por
/// vencimiento).
async fn renovar(app: &tauri::AppHandle, rechazado: Option<&str>) -> Result<String, String> {
    let _turno = RENOVANDO.lock().await;
    let mut c = crate::creds::load(app).ok_or("RD no vinculado")?;
    // Otra llamada lo renovó mientras esperábamos el turno.
    let ya_renovado = match rechazado {
        Some(t) => c.access_token != t,
        None => !por_vencer(&c),
    };
    if ya_renovado && !c.access_token.is_empty() {
        return Ok(c.access_token);
    }
    if c.refresh_token.is_empty() || c.client_id.is_empty() {
        return Err(REVINCULAR.into());
    }
    let cli = client()?;
    let form = [
        ("client_id", c.client_id.as_str()),
        ("client_secret", c.client_secret.as_str()),
        ("code", c.refresh_token.as_str()),
        ("grant_type", RD_GRANT_DEVICE),
    ];
    let r = cli
        .post(format!("{OAUTH}/token"))
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("refresh red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let body = r.text().await.unwrap_or_default();
        eprintln!("[rd] refresh falló {st}: {body}");
        return Err(REVINCULAR.into());
    }
    let t: RdTokenResp = r.json().await.map_err(|e| format!("refresh parse: {e}"))?;
    c.access_token = t.access_token;
    c.refresh_token = t.refresh_token;
    c.expires_at = ahora_s() + t.expires_in;
    crate::creds::save(app, &c)?;
    eprintln!("[rd] token renovado");
    Ok(c.access_token)
}

/// Corre `op` con un token vigente; si RD lo rechaza, renueva y reintenta una vez.
async fn con_token<T, F, Fut>(app: &tauri::AppHandle, op: F) -> Result<T, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let c = crate::creds::load(app)
        .filter(|c| !c.access_token.is_empty())
        .ok_or("RD no vinculado")?;
    let token = if por_vencer(&c) { renovar(app, None).await? } else { c.access_token };
    match op(token.clone()).await {
        Err(e) if token_rechazado(&e) => {
            eprintln!("[rd] token rechazado ({e}), renovando");
            let nuevo = renovar(app, Some(&token)).await?;
            op(nuevo).await
        }
        r => r,
    }
}

// ---- Comandos Tauri -----------------------------------------------------

/// Resuelve un magnet → URL directa lista para mpv.
#[tauri::command]
pub async fn rd_resolve(app: tauri::AppHandle, magnet: String) -> Result<String, String> {
    let magnet = magnet.as_str();
    con_token(&app, |token| async move { resolve_magnet(&token, magnet).await }).await
}

/// Desbloquea un link de hoster (Mega, Streamtape, Voe, Mixdrop, MP4Upload…)
/// a URL directa reproducible. Es el MISMO endpoint que usa la ruta magnet,
/// expuesto aparte para las fuentes web cuyos servidores son hosters.
///
/// Bonus de geobloqueo: el archivo se sirve desde los servidores de RD, así
/// que el video no toca el dominio bloqueado por el operador — solo el scrape
/// del HTML lo hace.
///
/// Si RD no soporta el hoster devuelve error; el llamador debe probar el
/// siguiente servidor, no rendirse.
#[tauri::command]
pub async fn rd_unrestrict(app: tauri::AppHandle, link: String) -> Result<String, String> {
    if link.trim().is_empty() {
        return Err("link vacío".into());
    }
    let link = link.trim();
    con_token(&app, |token| async move {
        let cli = client()?;
        unrestrict(&cli, &token, link).await
    })
    .await
}

/// Devuelve qué info_hashes ya están cacheados en RD (para badge "instantáneo").
#[tauri::command]
pub async fn rd_instant_available(
    app: tauri::AppHandle,
    hashes: Vec<String>,
) -> Result<Vec<String>, String> {
    let hashes = hashes.as_slice();
    con_token(&app, |token| async move { instant_available(&token, hashes).await }).await
}

#[derive(Deserialize, Serialize)]
pub struct RdAccount {
    #[serde(default)]
    pub username: String,
    #[serde(default, rename(deserialize = "type"))]
    pub account_type: String, // "premium" | "free"
    #[serde(default)]
    pub premium: i64, // segundos de premium restantes
    #[serde(default)]
    pub expiration: String,
    #[serde(default)]
    pub points: i64,
}

/// Estado de la cuenta RD (premium/free, expiración). Diagnóstico de 451.
#[tauri::command]
pub async fn rd_account(app: tauri::AppHandle) -> Result<RdAccount, String> {
    con_token(&app, |token| async move { cuenta(&token).await }).await
}

async fn cuenta(token: &str) -> Result<RdAccount, String> {
    let cli = client()?;
    let r = cli
        .get(format!("{BASE}/user"))
        .header("Authorization", bearer(token))
        .send()
        .await
        .map_err(|e| format!("user red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let body = r.text().await.unwrap_or_default();
        return Err(format!("RD /user {st}: {body}"));
    }
    r.json().await.map_err(|e| format!("user parse: {e}"))
}

#[derive(Deserialize)]
struct RdTorrentListItem {
    id: String,
    #[serde(default)]
    added: String, // ISO 8601, ej "2026-05-30T18:41:56.000Z"
}

/// Días desde epoch (algoritmo de Howard Hinnant, sin deps).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parsea "2026-05-30T18:41:56[.xxx]Z" (UTC) → epoch segundos.
fn parse_iso_utc(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let se: i64 = s.get(17..19)?.parse().ok()?;
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Borra de la lista RD los torrents agregados hace más de `older_than_hours`.
/// No afecta reproducción ni la caché global de RD. Devuelve cuántos borró.
#[tauri::command]
pub async fn rd_cleanup_torrents(
    app: tauri::AppHandle,
    older_than_hours: u64,
) -> Result<usize, String> {
    if older_than_hours == 0 {
        return Ok(0);
    }
    con_token(&app, |token| async move { limpiar_viejos(&token, older_than_hours).await }).await
}

async fn limpiar_viejos(token: &str, older_than_hours: u64) -> Result<usize, String> {
    let cli = client()?;
    let r = cli
        .get(format!("{BASE}/torrents?limit=200"))
        .header("Authorization", bearer(token))
        .send()
        .await
        .map_err(|e| format!("list red: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("torrents list {}", r.status()));
    }
    let list: Vec<RdTorrentListItem> = r.json().await.map_err(|e| format!("list parse: {e}"))?;

    let cutoff = now_secs() - (older_than_hours as i64) * 3600;
    let mut deleted = 0usize;
    for t in list {
        match parse_iso_utc(&t.added) {
            Some(added) if added < cutoff => {
                let resp = cli
                    .delete(format!("{BASE}/torrents/delete/{}", t.id))
                    .header("Authorization", bearer(token))
                    .send()
                    .await;
                if matches!(resp, Ok(ref x) if x.status().is_success()) {
                    deleted += 1;
                }
            }
            _ => {}
        }
    }
    Ok(deleted)
}

#[derive(Deserialize)]
struct RdTokenResp {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

// ============================================================
// RealDebrid — OAuth Device Code flow
// ============================================================

const RD_CLIENT_ID: &str = "X245A4XAIBGVM"; // public open-source client_id

#[derive(Serialize, Deserialize, Clone)]
pub struct RdDeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct RdCredentialsResp {
    client_id: String,
    client_secret: String,
}

#[tauri::command]
pub async fn rd_device_start() -> Result<RdDeviceStart, String> {
    let url = format!(
        "https://api.real-debrid.com/oauth/v2/device/code?client_id={}&new_credentials=yes",
        RD_CLIENT_ID
    );
    let cli = crate::client()?;
    let resp = cli.get(&url).send().await.map_err(|e| format!("red: {}", e))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("RD {}: {}", st, body));
    }
    let v: RdDeviceStart = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    Ok(v)
}

#[tauri::command]
pub async fn rd_device_poll(
    app: tauri::AppHandle,
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<(), String> {
    if device_code.is_empty() {
        return Err("device_code vacío".into());
    }
    let cli = crate::client()?;
    let poll_every = std::cmp::max(interval, 3);
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(expires_in.max(60));

    let creds_url = format!(
        "https://api.real-debrid.com/oauth/v2/device/credentials?client_id={}&code={}",
        RD_CLIENT_ID, device_code
    );

    loop {
        if std::time::Instant::now() >= deadline {
            return Err("código expirado".into());
        }
        tokio::time::sleep(std::time::Duration::from_secs(poll_every)).await;

        // Hipo de red transitorio: NO abortar el flujo, reintentar al próximo
        // poll. Un solo blip no debe tirar abajo todo el login.
        let resp = match cli.get(&creds_url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[rd] creds red transitorio, reintento: {}", e);
                continue;
            }
        };
        if !resp.status().is_success() {
            continue;
        }
        let body = resp.text().await.unwrap_or_default();
        let Ok(creds) = serde_json::from_str::<RdCredentialsResp>(&body) else {
            continue;
        };

        let form = [
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("code", device_code.as_str()),
            ("grant_type", RD_GRANT_DEVICE),
        ];
        let tok = match cli
            .post("https://api.real-debrid.com/oauth/v2/token")
            .form(&form)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[rd] token red transitorio, reintento: {}", e);
                continue;
            }
        };
        if !tok.status().is_success() {
            let st = tok.status();
            let body = tok.text().await.unwrap_or_default();
            return Err(format!("RD token {}: {}", st, body));
        }
        let t: RdTokenResp = tok.json().await.map_err(|e| format!("parse token: {}", e))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // El token NO vuelve al webview: se persiste en el store 0600 del backend.
        crate::creds::save(
            &app,
            &crate::creds::RdCreds {
                access_token: t.access_token,
                refresh_token: t.refresh_token,
                client_id: creds.client_id,
                client_secret: creds.client_secret,
                expires_at: now + t.expires_in,
            },
        )?;
        return Ok(());
    }
}
