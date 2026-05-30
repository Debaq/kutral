// RealDebrid — resolución magnet → URL directa reproducible + cache check.
//
// Portado de kodi-os/scraper/src/realdebrid.rs, adaptado a kutral SIN estado
// propio: el access_token se pasa en cada llamada (el frontend ya lo guarda
// en localStorage tras el device-flow de lib.rs). Para renovar token expirado
// está `rd_refresh`, que el frontend persiste igual que rd_device_poll.
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
        return Err(format!("addMagnet {}", r.status()));
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
    let cli = client()?;
    let id = add_magnet(&cli, token, magnet).await?;

    // Elegir archivo de video más grande
    let info = torrent_info(&cli, token, &id).await?;
    let target = info
        .files
        .iter()
        .filter(|f| is_video(&f.path))
        .max_by_key(|f| f.bytes)
        .ok_or("sin archivo de video")?;
    select_files(&cli, token, &id, &target.id.to_string()).await?;

    // Poll hasta "downloaded" (instantáneo si cacheado)
    let mut info = torrent_info(&cli, token, &id).await?;
    let mut waited = 0u64;
    while info.status != "downloaded" {
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
    unrestrict(&cli, token, &link).await
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
    Ok(found.into_iter().collect())
}

// ---- Comandos Tauri -----------------------------------------------------

/// Resuelve un magnet → URL directa lista para mpv.
#[tauri::command]
pub async fn rd_resolve(magnet: String, token: String) -> Result<String, String> {
    if token.is_empty() {
        return Err("token RD vacío".into());
    }
    resolve_magnet(&token, &magnet).await
}

/// Devuelve qué info_hashes ya están cacheados en RD (para badge "instantáneo").
#[tauri::command]
pub async fn rd_instant_available(
    hashes: Vec<String>,
    token: String,
) -> Result<Vec<String>, String> {
    if token.is_empty() {
        return Err("token RD vacío".into());
    }
    instant_available(&token, &hashes).await
}

#[derive(Serialize)]
pub struct RdRefreshed {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct RdTokenResp {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

/// Renueva el access_token usando las credenciales del device-flow.
/// El frontend lo persiste igual que tras rd_device_poll.
#[tauri::command]
pub async fn rd_refresh(
    client_id: String,
    client_secret: String,
    refresh_token: String,
) -> Result<RdRefreshed, String> {
    let cli = client()?;
    let form = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", refresh_token.as_str()),
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
        return Err(format!("RD refresh {st}: {body}"));
    }
    let t: RdTokenResp = r.json().await.map_err(|e| format!("refresh parse: {e}"))?;
    Ok(RdRefreshed {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_in: t.expires_in,
    })
}
