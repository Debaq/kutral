// yt-dlp: dónde está y cómo se mantiene al día.
//
// El binario que viaja en vendor/ es el del día del build y nunca cambia.
// YouTube rompe los extractores cada pocas semanas, así que la app envejecía
// sola: los trailers funcionaban al instalarla y un mes después quedaba el QR.
// `yt-dlp -U` no sirve: se reemplaza en su propia ruta, y `/app` del flatpak es
// de solo lectura (Program Files igual, sin admin).
//
// Por eso hay una copia escribible en el directorio de datos del usuario, que
// tiene prioridad sobre vendor/. Se revisa la última release de GitHub como
// máximo una vez al día, en segundo plano, y lo bajado se valida con
// `--version` antes de reemplazar nada: una descarga a medias nunca deja a la
// app sin yt-dlp.

use std::path::PathBuf;
use std::time::Duration;

use tauri::Manager;

use crate::winproc;

#[cfg(windows)]
const EXE: &str = "yt-dlp.exe";
#[cfg(not(windows))]
const EXE: &str = "yt-dlp";

/// Nombre del binario en las releases de GitHub para esta plataforma.
/// `None` = plataforma sin binario autocontenido: se queda con el de vendor/.
fn asset() -> Option<&'static str> {
    if cfg!(windows) {
        Some("yt-dlp.exe")
    } else if cfg!(target_os = "macos") {
        Some("yt-dlp_macos")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("yt-dlp_linux")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("yt-dlp_linux_aarch64")
    } else {
        None
    }
}

const UN_DIA: Duration = Duration::from_secs(24 * 60 * 60);

fn dir_usuario(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("yt-dlp"))
}

/// Ruta del yt-dlp a usar: la copia actualizada del usuario primero, después
/// la de vendor/ (bundle, árbol de desarrollo, junto al ejecutable).
pub fn path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = dir_usuario(app) {
        cands.push(d.join(EXE));
    }
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join(EXE));
    }
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(EXE));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join(EXE));
        }
    }
    cands.into_iter().find(|c| c.exists())
}

/// Como `path`, pero si no hay ninguno cae al del PATH del sistema.
pub fn bin(app: &tauri::AppHandle) -> String {
    path(app)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| EXE.to_string())
}

/// `yt-dlp --version` → "2026.09.20". `None` si no corre.
async fn version(bin: &std::path::Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    winproc::hide_console_tokio(&mut cmd);
    let fut = cmd.arg("--version").kill_on_drop(true).output();
    let out = tokio::time::timeout(Duration::from_secs(20), fut).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// Las versiones de yt-dlp son fechas con puntos ("2026.09.20", a veces con un
/// ".1" extra). Se comparan número a número, no como texto.
fn mas_nueva(a: &str, b: &str) -> bool {
    let partes = |s: &str| -> Vec<u64> {
        s.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    partes(a) > partes(b)
}

/// Revisa una vez al día si hay yt-dlp nuevo y, si hay, lo deja en el
/// directorio del usuario. Corre en segundo plano: no demora el arranque y
/// cualquier fallo solo se anota en el log.
pub fn actualizar_en_segundo_plano(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = actualizar(&app).await {
            eprintln!("[yt-dlp] actualización: {e}");
        }
    });
}

async fn actualizar(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(asset) = asset() else { return Ok(()) };
    let dir = dir_usuario(app).ok_or("sin directorio de datos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Una vez al día como máximo. La marca se escribe ANTES de consultar: si
    // GitHub falla o limita, no se reintenta en cada arranque.
    let marca = dir.join("ultimo_chequeo");
    let reciente = std::fs::metadata(&marca)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|e| e < UN_DIA);
    if reciente {
        return Ok(());
    }
    let _ = std::fs::write(&marca, b"");

    // Cliente propio: el compartido corta a los 15 s y el binario pesa ~40 MB.
    let cli = reqwest::Client::builder()
        .user_agent(concat!("Kutral/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let rel: serde_json::Value = cli
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let ultima = rel["tag_name"].as_str().ok_or("release sin tag")?.to_string();

    let actual = match path(app) {
        Some(p) => version(&p).await,
        None => None,
    };
    if actual.as_deref().is_some_and(|a| !mas_nueva(&ultima, a)) {
        return Ok(());
    }

    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{ultima}/{asset}");
    let bytes = cli
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    // Se escribe al lado y se valida antes de reemplazar: el rename es atómico
    // y el binario anterior sigue en su lugar si algo sale mal.
    let tmp = dir.join(format!("{EXE}.nuevo"));
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    match version(&tmp).await {
        Some(v) if v == ultima => {}
        otra => {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("el binario bajado no corre ({otra:?})"));
        }
    }
    std::fs::rename(&tmp, dir.join(EXE)).map_err(|e| e.to_string())?;
    eprintln!("[yt-dlp] actualizado: {} → {ultima}", actual.unwrap_or_else(|| "ninguno".into()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mas_nueva;

    #[test]
    fn compara_versiones_por_numero() {
        assert!(mas_nueva("2026.10.01", "2026.09.20"));
        assert!(mas_nueva("2026.09.20.1", "2026.09.20"));
        assert!(mas_nueva("2027.01.02", "2026.12.31"));
        assert!(!mas_nueva("2026.09.20", "2026.09.20"));
        assert!(!mas_nueva("2026.09.01", "2026.10.01"));
    }
}
