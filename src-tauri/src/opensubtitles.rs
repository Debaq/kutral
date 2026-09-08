// OpenSubtitles REST API (api.opensubtitles.com/api/v1).
// Subtítulos externos por imdb_id cuando el release no trae pista ES embebida.
//
// Cuotas (reales): anónimo 5 descargas/día por IP; con cuenta gratis 20/día.
// La API key de app NO es cuenta de usuario: siempre hace falta (registro 1 vez
// en https://www.opensubtitles.com/consumers). El login de usuario es opcional
// y solo sube la cuota 5→20. El token de login (JWT) expira ~24h → mismo patrón
// que RD (ver creds::expires_at).
//
// El token de usuario NUNCA llega al webview: vive en un store 0600 del backend.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

// API key de la app kutral. Pégala tras registrarte como consumer. Vacía =>
// subs de OpenSubtitles deshabilitados (el flujo cae a Wyzie si hay key).
const OS_API_KEY: &str = "Ai2jXR6L9YnTmJ4rY9NEzQcwjxGJU6rf";
const OS_BASE: &str = "https://api.opensubtitles.com/api/v1";
// User-Agent obligatorio y único por app, formato "Nombre vX.Y". Se arma desde
// la versión del crate a propósito: escrito a mano se quedó tres releases atrás
// (v26.5.3 cuando la app iba en la 26.5.6) porque nadie se acuerda de tocarlo.
//
// Cambiarlo es seguro: /subtitles ni siquiera valida UA o Api-Key, y el que sí
// autentica, POST /download, devuelve link con el UA nuevo (probado contra la
// API real). Lo que OpenSubtitles pide es que identifique a la app, no que
// coincida con una cadena congelada.
const OS_UA: &str = concat!("kutral v", env!("CARGO_PKG_VERSION"));

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct OsCreds {
    /// JWT devuelto por /login. Vacío = sólo cuota anónima (5/día).
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub username: String,
    /// Unix epoch (segundos) en que vence el token. 0 = desconocido.
    #[serde(default)]
    pub expires_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn creds_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    Ok(dir.join("os_creds.json"))
}

fn load(app: &tauri::AppHandle) -> Option<OsCreds> {
    let raw = std::fs::read_to_string(creds_path(app).ok()?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save(app: &tauri::AppHandle, creds: &OsCreds) -> Result<(), String> {
    let path = creds_path(app)?;
    let json = serde_json::to_string(creds).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Token de usuario vigente (no vencido). None si no hay o expiró.
fn valid_token(app: &tauri::AppHandle) -> Option<String> {
    let c = load(app)?;
    if c.token.is_empty() {
        return None;
    }
    // Margen de 5 min para no usar un token al borde del vencimiento.
    if c.expires_at != 0 && c.expires_at <= now_secs() + 300 {
        return None;
    }
    Some(c.token)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(OS_UA)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("client: {e}"))
}

// ---- Login --------------------------------------------------------------

#[derive(Deserialize)]
struct LoginResp {
    token: String,
}

/// Vincula una cuenta gratis de OpenSubtitles (sube la cuota a 20/día). El
/// frontend nunca recibe el token: se persiste en el store 0600.
#[tauri::command]
pub async fn os_login(
    app: tauri::AppHandle,
    username: String,
    password: String,
) -> Result<(), String> {
    if OS_API_KEY.is_empty() {
        return Err("Falta API key de OpenSubtitles (regístrala en /consumers).".into());
    }
    let cli = client()?;
    let body = serde_json::json!({ "username": username.trim(), "password": password });
    let r = cli
        .post(format!("{OS_BASE}/login"))
        .header("Api-Key", OS_API_KEY)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("login red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let b = r.text().await.unwrap_or_default();
        return Err(format!("OS login {st}: {b}"));
    }
    let resp: LoginResp = r.json().await.map_err(|e| format!("login parse: {e}"))?;
    save(
        &app,
        &OsCreds {
            token: resp.token,
            username: username.trim().to_string(),
            // El JWT de OpenSubtitles dura 24h. Guardamos vencimiento con margen.
            expires_at: now_secs() + 24 * 3600,
        },
    )
}

#[derive(Serialize, Default)]
pub struct OsStatus {
    pub linked: bool,
    pub username: String,
    pub has_api_key: bool,
}

/// Estado de la cuenta OS para el frontend (sin exponer el token).
#[tauri::command]
pub fn os_status(app: tauri::AppHandle) -> OsStatus {
    let c = load(&app).unwrap_or_default();
    OsStatus {
        linked: valid_token(&app).is_some(),
        username: c.username,
        has_api_key: !OS_API_KEY.is_empty(),
    }
}

#[tauri::command]
pub fn os_clear(app: tauri::AppHandle) -> Result<(), String> {
    let path = creds_path(&app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("remove: {e}"))?;
    }
    Ok(())
}

// ---- Búsqueda + descarga ------------------------------------------------

#[derive(Deserialize)]
struct SearchResp {
    #[serde(default)]
    data: Vec<SubItem>,
}

#[derive(Deserialize)]
struct SubItem {
    #[serde(default)]
    attributes: SubAttrs,
}

#[derive(Deserialize, Default)]
struct SubAttrs {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    release: Option<String>,
    #[serde(default)]
    download_count: i64,
    #[serde(default)]
    hearing_impaired: bool,
    #[serde(default)]
    files: Vec<SubFile>,
}

#[derive(Deserialize, Default, Clone)]
struct SubFile {
    #[serde(default)]
    file_id: i64,
    #[serde(default)]
    file_name: Option<String>,
}

#[derive(Deserialize)]
struct DownloadResp {
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    remaining: i64,
}

#[derive(Serialize)]
pub struct OsSubtitle {
    /// URL temporal directa al .srt (válida un rato). mpv la carga con sub-add.
    pub url: String,
    pub label: String,
    pub lang: String,
    /// Descargas restantes hoy según la API (cuota diaria).
    pub remaining: i64,
}

/// Busca el mejor subtítulo para `imdb_id` en `language` y devuelve su URL
/// directa de descarga + la cuota restante. imdb_id puede venir como "tt123"
/// o numérico (OS exige numérico).
/// Limpia un nombre para usarlo como archivo (sin / : etc.).
fn safe_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if "/\\:*?\"<>|\n\r\t".contains(c) { '_' } else { c })
        .collect();
    let t = cleaned.trim().trim_matches('.').trim();
    let t = if t.is_empty() { "subtitulo" } else { t };
    t.chars().take(120).collect()
}

/// Baja un .srt desde `url` a la carpeta de Descargas (subcarpeta "Kütral
/// Subtítulos") para coleccionarlos, y devuelve la RUTA LOCAL para cargarla en
/// mpv. Funciona con cualquier fuente (OpenSubtitles, Wyzie): el frontend pasa
/// la URL ya resuelta.
#[tauri::command]
pub async fn subtitle_save(
    app: tauri::AppHandle,
    url: String,
    filename: String,
) -> Result<String, String> {
    if url.is_empty() {
        return Err("url vacía".into());
    }
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("download_dir: {e}"))?
        .join("Kütral Subtítulos");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let path = dir.join(format!("{}.srt", safe_filename(&filename)));

    let cli = client()?;
    let r = cli
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("descarga sub red: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("descarga sub {}", r.status()));
    }
    let bytes = r.bytes().await.map_err(|e| format!("descarga sub body: {e}"))?;
    std::fs::write(&path, &bytes).map_err(|e| format!("write sub: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn os_search(
    app: tauri::AppHandle,
    imdb_id: String,
    language: String,
) -> Result<OsSubtitle, String> {
    if OS_API_KEY.is_empty() {
        return Err("Falta API key de OpenSubtitles.".into());
    }
    let imdb_num = imdb_id.trim_start_matches("tt").trim_start_matches('0');
    if imdb_num.is_empty() {
        return Err("imdb_id vacío".into());
    }
    let lang = if language.is_empty() { "es" } else { &language };
    let cli = client()?;
    let token = valid_token(&app);

    // 1) Búsqueda. Api-Key obligatoria; Bearer opcional (sube la cuota).
    let url = format!("{OS_BASE}/subtitles?imdb_id={imdb_num}&languages={lang}");
    let mut req = cli.get(&url).header("Api-Key", OS_API_KEY).header("Accept", "application/json");
    if let Some(t) = &token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let r = req.send().await.map_err(|e| format!("search red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let b = r.text().await.unwrap_or_default();
        return Err(format!("OS search {st}: {b}"));
    }
    let sr: SearchResp = r.json().await.map_err(|e| format!("search parse: {e}"))?;

    // Mejor candidato: idioma correcto, sin hearing-impaired, con file_id,
    // más descargado (proxy de calidad/sync).
    let best = sr
        .data
        .into_iter()
        .filter(|it| !it.attributes.hearing_impaired)
        .filter(|it| !it.attributes.files.is_empty())
        .max_by_key(|it| it.attributes.download_count)
        .ok_or("Sin subtítulos para esta película.")?;
    let label = best
        .attributes
        .release
        .clone()
        .or_else(|| best.attributes.files.first().and_then(|f| f.file_name.clone()))
        .unwrap_or_else(|| "Subtítulos".into());
    let sub_lang = best.attributes.language.clone().unwrap_or_else(|| lang.to_string());
    let file_id = best.attributes.files[0].file_id;

    // 2) Descarga: convierte file_id en URL temporal directa. Esto SÍ consume
    //    cuota; la API devuelve cuánto queda.
    let dl_body = serde_json::json!({ "file_id": file_id });
    let mut dreq = cli
        .post(format!("{OS_BASE}/download"))
        .header("Api-Key", OS_API_KEY)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&dl_body);
    if let Some(t) = &token {
        dreq = dreq.header("Authorization", format!("Bearer {t}"));
    }
    let dr = dreq.send().await.map_err(|e| format!("download red: {e}"))?;
    if !dr.status().is_success() {
        let st = dr.status();
        let b = dr.text().await.unwrap_or_default();
        // 406 típico = cuota diaria agotada.
        return Err(format!("OS download {st}: {b}"));
    }
    let d: DownloadResp = dr.json().await.map_err(|e| format!("download parse: {e}"))?;
    let link = d.link.filter(|s| !s.is_empty()).ok_or("OS sin link de descarga")?;
    Ok(OsSubtitle {
        url: link,
        label: d.file_name.unwrap_or(label),
        lang: sub_lang,
        remaining: d.remaining,
    })
}

/// Un candidato de la lista de OpenSubtitles (sin resolver el link aún).
#[derive(Serialize)]
pub struct OsListItem {
    pub file_id: i64,
    pub label: String,
    pub lang: String,
    pub downloads: i64,
    pub hi: bool,
}

/// Lista VARIOS subtítulos para `imdb_id` (no resuelve links → NO gasta cuota).
/// El frontend muestra esto en el picker; al elegir llama `os_download`.
#[tauri::command]
pub async fn os_list(
    app: tauri::AppHandle,
    imdb_id: String,
    language: String,
) -> Result<Vec<OsListItem>, String> {
    if OS_API_KEY.is_empty() {
        return Err("Falta API key de OpenSubtitles.".into());
    }
    let imdb_num = imdb_id.trim_start_matches("tt").trim_start_matches('0');
    if imdb_num.is_empty() {
        return Err("imdb_id vacío".into());
    }
    let lang = if language.is_empty() { "es" } else { &language };
    let cli = client()?;
    let token = valid_token(&app);

    let url =
        format!("{OS_BASE}/subtitles?imdb_id={imdb_num}&languages={lang}&order_by=download_count");
    let mut req = cli
        .get(&url)
        .header("Api-Key", OS_API_KEY)
        .header("Accept", "application/json");
    if let Some(t) = &token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let r = req.send().await.map_err(|e| format!("search red: {e}"))?;
    if !r.status().is_success() {
        let st = r.status();
        let b = r.text().await.unwrap_or_default();
        return Err(format!("OS search {st}: {b}"));
    }
    let sr: SearchResp = r.json().await.map_err(|e| format!("search parse: {e}"))?;
    let mut items: Vec<OsListItem> = sr
        .data
        .into_iter()
        .filter(|it| !it.attributes.files.is_empty())
        .map(|it| {
            let file = it.attributes.files[0].clone();
            let label = it
                .attributes
                .release
                .clone()
                .or_else(|| file.file_name.clone())
                .unwrap_or_else(|| "Subtítulo".into());
            OsListItem {
                file_id: file.file_id,
                label,
                lang: it
                    .attributes
                    .language
                    .clone()
                    .unwrap_or_else(|| lang.to_string()),
                downloads: it.attributes.download_count,
                hi: it.attributes.hearing_impaired,
            }
        })
        .collect();
    items.sort_by(|a, b| b.downloads.cmp(&a.downloads));
    items.truncate(25);
    Ok(items)
}

/// Resuelve el link directo de un `file_id` (POST /download). Gasta 1 de cuota.
#[tauri::command]
pub async fn os_download(app: tauri::AppHandle, file_id: i64) -> Result<OsSubtitle, String> {
    if OS_API_KEY.is_empty() {
        return Err("Falta API key de OpenSubtitles.".into());
    }
    let cli = client()?;
    let token = valid_token(&app);
    let dl_body = serde_json::json!({ "file_id": file_id });
    let mut dreq = cli
        .post(format!("{OS_BASE}/download"))
        .header("Api-Key", OS_API_KEY)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&dl_body);
    if let Some(t) = &token {
        dreq = dreq.header("Authorization", format!("Bearer {t}"));
    }
    let dr = dreq.send().await.map_err(|e| format!("download red: {e}"))?;
    if !dr.status().is_success() {
        let st = dr.status();
        let b = dr.text().await.unwrap_or_default();
        return Err(format!("OS download {st}: {b}"));
    }
    let d: DownloadResp = dr.json().await.map_err(|e| format!("download parse: {e}"))?;
    let link = d.link.filter(|s| !s.is_empty()).ok_or("OS sin link de descarga")?;
    Ok(OsSubtitle {
        url: link,
        label: d.file_name.unwrap_or_default(),
        lang: String::new(),
        remaining: d.remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_user_agent_sigue_la_version_del_crate() {
        assert_eq!(OS_UA, format!("kutral v{}", env!("CARGO_PKG_VERSION")));
        // El formato que exige OpenSubtitles es "Nombre vX.Y".
        assert!(OS_UA.starts_with("kutral v"));
        assert!(OS_UA.len() > "kutral v".len(), "sin versión: {OS_UA}");
    }
}
