// Llavero compartido: prestar la cuenta de Real-Debrid a amigos del mismo
// servidor de cuentas (server/, rutas /llavero).
//
// Quien presta vincula un código NUEVO solo para el llavero (no la de este
// equipo): las credenciales van directo al servidor, que las guarda y renueva.
// Probado contra RD real: dos vinculaciones de la misma cuenta tienen
// refresh_token propio y renovar una no invalida los tokens de la otra; al
// dejar de prestar, el servidor anula el token prestado con
// disable_access_token, que corta solo ese.
//
// Quien recibe el préstamo nunca tiene el refresh_token: pide el access_token
// al servidor y lo guarda solo en memoria. Al arrancar, o si RD lo rechaza,
// lo vuelve a pedir; si ya no se lo prestan, el servidor responde 403.

use serde_json::{json, Value};
use std::sync::Mutex;

struct Prestado {
    token: String,
    /// Unix (s) en que vence.
    expira: u64,
}

static PRESTADO: Mutex<Option<Prestado>> = Mutex::new(None);
/// Quién presta RD a esta cuenta, según el último /llavero. Para que
/// `rd_creds_status` responda sin ir a la red.
static PRESTA_RD: Mutex<Option<String>> = Mutex::new(None);
/// Un pedido de token a la vez: varias búsquedas de fuentes lo piden juntas.
static PIDIENDO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const MARGEN_S: u64 = 300;

fn ahora_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn olvidar_prestamo() {
    *PRESTADO.lock().unwrap() = None;
    *PRESTA_RD.lock().unwrap() = None;
}

/// ¿Alguien le presta RD a esta cuenta? (sin red; lo actualiza `llavero_estado`).
pub fn presta_rd() -> Option<String> {
    PRESTA_RD.lock().unwrap().clone()
}

/// Token de RD prestado. `rechazado` = el que RD acaba de rechazar: se pide
/// uno renovado.
pub async fn token_prestado(app: &tauri::AppHandle, rechazado: Option<&str>) -> Result<String, String> {
    let _turno = PIDIENDO.lock().await;
    if let Some(p) = PRESTADO.lock().unwrap().as_ref() {
        let sirve = rechazado.is_none_or(|r| r != p.token) && p.expira > ahora_s() + MARGEN_S;
        if sirve {
            return Ok(p.token.clone());
        }
    }
    if !crate::cuenta::con_cuenta(app) {
        return Err("RD no vinculado".into());
    }
    let ruta = if rechazado.is_some() { "/llavero/rd/token?renovar=1" } else { "/llavero/rd/token" };
    match crate::cuenta::con_sesion(app, reqwest::Method::GET, ruta, None).await {
        Ok(v) => {
            let token = v["access_token"].as_str().ok_or("respuesta sin token")?.to_string();
            let expira = v["expira"].as_u64().unwrap_or(0);
            *PRESTADO.lock().unwrap() = Some(Prestado { token: token.clone(), expira });
            *PRESTA_RD.lock().unwrap() = v["de"].as_str().map(String::from);
            Ok(token)
        }
        Err(e) => {
            // Ya no se lo prestan (o la sesión se cerró): RD vuelve a "no vinculado".
            olvidar_prestamo();
            Err(e)
        }
    }
}

// ─── Comandos Tauri ─────────────────────────────────────────────────────────

/// `{mias: {rd?: {creada, prestada_a}}, prestadas: [{servicio, de}], rd_propio}`.
#[tauri::command]
pub async fn llavero_estado(app: tauri::AppHandle) -> Result<Value, String> {
    if !crate::cuenta::con_cuenta(&app) {
        olvidar_prestamo();
        return Ok(json!({ "mias": {}, "prestadas": [], "rd_propio": crate::creds::rd_creds_status_local(&app) }));
    }
    let mut v = crate::cuenta::con_sesion(&app, reqwest::Method::GET, "/llavero", None).await?;
    let de = v["prestadas"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["servicio"] == "rd")
        .and_then(|p| p["de"].as_str())
        .map(String::from);
    if de.is_none() {
        *PRESTADO.lock().unwrap() = None;
    }
    *PRESTA_RD.lock().unwrap() = de;
    v["rd_propio"] = json!(crate::creds::rd_creds_status_local(&app));
    Ok(v)
}

/// Espera la autorización del código (pedido con `rd_device_start`) y sube
/// esa vinculación al llavero. No toca la vinculación de este equipo.
#[tauri::command]
pub async fn llavero_vincular(
    app: tauri::AppHandle,
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<Value, String> {
    let c = crate::rd::esperar_vinculacion(&device_code, interval, expires_in).await?;
    let cuerpo = json!({
        "client_id": c.client_id,
        "client_secret": c.client_secret,
        "refresh_token": c.refresh_token,
        "access_token": c.access_token,
        "expira": c.expires_at,
    });
    crate::cuenta::con_sesion(&app, reqwest::Method::POST, "/llavero/rd", Some(&cuerpo)).await
}

/// Borra la vinculación del llavero: deja de prestarse a todos.
#[tauri::command]
pub async fn llavero_borrar(app: tauri::AppHandle) -> Result<Value, String> {
    crate::cuenta::con_sesion(&app, reqwest::Method::DELETE, "/llavero/rd", None).await
}

#[tauri::command]
pub async fn llavero_prestar(app: tauri::AppHandle, nombre: String) -> Result<Value, String> {
    let cuerpo = json!({ "nombre": nombre.trim() });
    crate::cuenta::con_sesion(&app, reqwest::Method::POST, "/llavero/rd/prestamos", Some(&cuerpo)).await
}

#[tauri::command]
pub async fn llavero_dejar_de_prestar(app: tauri::AppHandle, nombre: String) -> Result<Value, String> {
    let nombre = nombre.trim().to_lowercase();
    if nombre.is_empty() || !nombre.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) {
        return Err("nombre inválido".into());
    }
    crate::cuenta::con_sesion(&app, reqwest::Method::DELETE, &format!("/llavero/rd/prestamos/{nombre}"), None)
        .await
}
