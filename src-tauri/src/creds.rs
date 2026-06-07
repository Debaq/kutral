// Store de credenciales Real-Debrid. Los secrets (access_token, refresh_token,
// client_id, client_secret) viven en un archivo 0600 del dir de config, NUNCA
// en localStorage del webview → fuera del alcance de cualquier JS/XSS. Las
// commands rd_* leen el token de aquí; el frontend jamás lo recibe.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct RdCreds {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    /// Unix epoch (segundos) en que vence el access_token. 0 = desconocido.
    #[serde(default)]
    pub expires_at: u64,
}

fn creds_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    Ok(dir.join("rd_creds.json"))
}

pub fn load(app: &tauri::AppHandle) -> Option<RdCreds> {
    let path = creds_path(app).ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Access token actual. Error si RD no está vinculado.
pub fn token(app: &tauri::AppHandle) -> Result<String, String> {
    let c = load(app).ok_or("RD no vinculado")?;
    if c.access_token.is_empty() {
        return Err("RD no vinculado".into());
    }
    Ok(c.access_token)
}

/// Persiste las credenciales con permisos 0600 (solo el usuario).
pub fn save(app: &tauri::AppHandle, creds: &RdCreds) -> Result<(), String> {
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

// ---- Comandos Tauri -----------------------------------------------------

#[tauri::command]
pub fn rd_creds_save(
    app: tauri::AppHandle,
    access_token: String,
    refresh_token: String,
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    save(
        &app,
        &RdCreds {
            access_token: access_token.trim().to_string(),
            refresh_token,
            client_id,
            client_secret,
            expires_at: 0,
        },
    )
}

#[tauri::command]
pub fn rd_creds_clear(app: tauri::AppHandle) -> Result<(), String> {
    let path = creds_path(&app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("remove: {e}"))?;
    }
    Ok(())
}

/// ¿Hay cuenta RD vinculada? El frontend usa esto para el estado, sin tocar
/// nunca el token en sí.
#[tauri::command]
pub fn rd_creds_status(app: tauri::AppHandle) -> bool {
    load(&app).map(|c| !c.access_token.is_empty()).unwrap_or(false)
}
