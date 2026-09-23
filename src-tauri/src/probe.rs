// Inspección de pistas SIN reproducir (NIVEL 2 del filtro de idioma).
//
// ffprobe lee solo el header del contenedor vía HTTP range-requests (unos MB,
// no baja el archivo) y devuelve TODAS las pistas con su idioma/título. Sirve
// para confirmar si un release trae audio o subtítulos en español ANTES de
// abrir mpv, y saltar a otra fuente si no.
//
// Best-effort por diseño: si ffprobe no está instalado o el server no soporta
// ranges, devolvemos error y el caller decide (reproducir igual, no bloquear).
//
// Sin ffprobe (Windows: el bundle no lo trae, pesa ~100 MB) se le pregunta a
// mpv, que sí viene en vendor/: un script Lua vuelca `track-list` apenas abre
// el archivo y sale sin reproducir. Sin esto, en Windows la TV recibía
// versiones con audio DTS sin saberlo y sonaba muda.

use serde::Serialize;
use std::time::Duration;

const PROBE_TIMEOUT_S: u64 = 20;

#[derive(Debug, Serialize)]
pub struct ProbeTrack {
    pub kind: String,  // "video" | "audio" | "subtitle"
    pub codec: String, // "hevc", "aac", "subrip", …
    pub lang: String,  // tag ISO del contenedor ("spa", "es-419", "" si no hay)
    pub title: String, // título de la pista ("Latino", "Español (España)", …)
    /// Marcada como predeterminada en el contenedor. Es la que suena al
    /// transmitir a la TV: el receptor Cast no deja elegir pista embebida.
    pub default: bool,
    /// Perfil del códec ("Main 10", "High", "DTS-HD MA"…) y " DV" si trae
    /// Dolby Vision. Dos HEVC no son lo mismo para una TV: 8 bits vs 10 bits
    /// vs Dolby Vision fallan por separado.
    pub profile: String,
}

fn perfil(s: &serde_json::Value) -> String {
    let mut p = s["profile"].as_str().unwrap_or_default().to_string();
    if p == "unknown" {
        p.clear();
    }
    let dovi = s["side_data_list"]
        .as_array()
        .is_some_and(|l| l.iter().any(|d| d["side_data_type"].as_str().is_some_and(|t| t.contains("DOVI"))));
    if dovi {
        p.push_str(" DV");
    }
    p.trim().to_string()
}

#[tauri::command]
pub async fn ffprobe_tracks(app: tauri::AppHandle, url: String) -> Result<Vec<ProbeTrack>, String> {
    // URLs http(s) o archivos del disco (la copia ya bajada, que también se
    // puede mandar a la TV y hay que saber qué códecs trae).
    let es_url = url.starts_with("http://") || url.starts_with("https://");
    if !es_url && !std::path::Path::new(&url).is_file() {
        return Err("probe: ni URL http(s) ni archivo".into());
    }
    match con_ffprobe(&url).await {
        Err(e) if e.starts_with(SIN_FFPROBE) => {
            eprintln!("[probe] {e} → mpv");
            con_mpv(&app, &url).await
        }
        r => r,
    }
}

const SIN_FFPROBE: &str = "ffprobe no disponible";

async fn con_ffprobe(url: &str) -> Result<Vec<ProbeTrack>, String> {
    let mut cmd = tokio::process::Command::new("ffprobe");
    crate::winproc::hide_console_tokio(&mut cmd);
    let child = cmd
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-probesize",
            "20M",
            url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{SIN_FFPROBE}: {e}"))?;

    let out = tokio::time::timeout(
        Duration::from_secs(PROBE_TIMEOUT_S),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| format!("ffprobe timeout (>{PROBE_TIMEOUT_S}s)"))?
    .map_err(|e| format!("ffprobe: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffprobe falló: {}", err.trim()));
    }

    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe parse: {e}"))?;
    let mut tracks = Vec::new();
    for s in v["streams"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let kind = s["codec_type"].as_str().unwrap_or_default();
        // La carátula de un MKV también es un stream "video" (mjpeg/png con
        // attached_pic): no es el video de verdad.
        let caratula = s["disposition"]["attached_pic"].as_i64() == Some(1);
        if !matches!(kind, "video" | "audio" | "subtitle") || caratula {
            continue;
        }
        tracks.push(ProbeTrack {
            kind: kind.to_string(),
            codec: s["codec_name"].as_str().unwrap_or_default().to_string(),
            lang: s["tags"]["language"].as_str().unwrap_or_default().to_string(),
            title: s["tags"]["title"].as_str().unwrap_or_default().to_string(),
            default: s["disposition"]["default"].as_i64() == Some(1),
            profile: perfil(s),
        });
    }
    eprintln!(
        "[probe] {} pistas en {}",
        tracks.len(),
        &url[..url.len().min(60)]
    );
    Ok(tracks)
}

// Vuelca la lista de pistas a un archivo y sale. A un archivo y no a stdout:
// mpv.exe en Windows no siempre escribe en un stdout redirigido.
const SCRIPT_PISTAS: &str = r#"
local salida = mp.get_opt("kutral-pistas")
mp.register_event("file-loaded", function()
  local f = io.open(salida, "w")
  if f then
    f:write(mp.get_property("track-list") or "[]")
    f:close()
  end
  mp.command("quit")
end)
"#;

async fn con_mpv(app: &tauri::AppHandle, url: &str) -> Result<Vec<ProbeTrack>, String> {
    let dir = std::env::temp_dir();
    let script = dir.join("kutral-pistas.lua");
    std::fs::write(&script, SCRIPT_PISTAS).map_err(|e| format!("probe mpv script: {e}"))?;
    // Un archivo por consulta: el selector inspecciona varias fuentes a la vez.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let salida = dir.join(format!("kutral-pistas-{}-{n}.json", std::process::id()));
    let _ = std::fs::remove_file(&salida);

    let mut cmd = tokio::process::Command::new(mpv_bin(app));
    crate::winproc::hide_console_tokio(&mut cmd);
    let child = cmd
        .args([
            "--no-config",
            "--vo=null",
            "--ao=null",
            "--ytdl=no",
            "--sub-auto=no",
            "--audio-file-auto=no",
            "--msg-level=all=no",
            "--network-timeout=15",
        ])
        .arg(format!("--script={}", script.display()))
        .arg(format!("--script-opts=kutral-pistas={}", salida.display()))
        .arg("--")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("probe: ni ffprobe ni mpv disponibles ({e})"))?;
    tokio::time::timeout(Duration::from_secs(PROBE_TIMEOUT_S), child.wait_with_output())
        .await
        .map_err(|_| format!("probe mpv timeout (>{PROBE_TIMEOUT_S}s)"))?
        .map_err(|e| format!("probe mpv: {e}"))?;

    let raw = std::fs::read_to_string(&salida);
    let _ = std::fs::remove_file(&salida);
    let raw = raw.map_err(|_| "probe mpv: no pudo abrir el archivo".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("probe mpv parse: {e}"))?;
    let tracks = pistas_de_mpv(&v);
    eprintln!("[probe] mpv: {} pistas en {}", tracks.len(), &url[..url.len().min(60)]);
    Ok(tracks)
}

/// `track-list` de mpv → mismas pistas que devuelve ffprobe. Los nombres de
/// códec y perfil son los de libavcodec en los dos casos.
fn pistas_de_mpv(v: &serde_json::Value) -> Vec<ProbeTrack> {
    let mut tracks = Vec::new();
    for t in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let kind = match t["type"].as_str().unwrap_or_default() {
            "video" => "video",
            "audio" => "audio",
            "sub" => "subtitle",
            _ => continue,
        };
        // Carátula del MKV: no es el video de verdad.
        if t["albumart"].as_bool() == Some(true) || t["image"].as_bool() == Some(true) {
            continue;
        }
        if t["external"].as_bool() == Some(true) {
            continue;
        }
        let mut profile = t["codec-profile"].as_str().unwrap_or_default().to_string();
        if profile == "unknown" {
            profile.clear();
        }
        if !t["dolby-vision-profile"].is_null() {
            profile.push_str(" DV");
        }
        tracks.push(ProbeTrack {
            kind: kind.to_string(),
            codec: t["codec"].as_str().unwrap_or_default().to_string(),
            lang: t["lang"].as_str().unwrap_or_default().to_string(),
            title: t["title"].as_str().unwrap_or_default().to_string(),
            default: t["default"].as_bool() == Some(true),
            profile: profile.trim().to_string(),
        });
    }
    tracks
}

/// mpv de vendor/ (el que va en el bundle de Windows); si no, el del PATH.
fn mpv_bin(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    let exe = if cfg!(windows) { "mpv.exe" } else { "mpv" };
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join(exe));
    }
    cands.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join(exe));
        }
    }
    cands.into_iter().find(|c| c.exists()).unwrap_or_else(|| exe.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pistas_de_mpv_mapea_como_ffprobe() {
        let v: serde_json::Value = serde_json::from_str(
            r#"[
              {"id":1,"type":"video","codec":"hevc","codec-profile":"Main 10","dolby-vision-profile":8,"default":false},
              {"id":2,"type":"video","codec":"mjpeg","albumart":true},
              {"id":1,"type":"audio","codec":"dts","codec-profile":"DTS-HD MA","lang":"spa","title":"Latino","default":true},
              {"id":2,"type":"audio","codec":"aac","lang":"eng","default":false},
              {"id":1,"type":"sub","codec":"subrip","lang":"spa","title":"Forzados","default":false},
              {"id":2,"type":"sub","codec":"subrip","external":true}
            ]"#,
        )
        .unwrap();
        let p = pistas_de_mpv(&v);
        assert_eq!(p.len(), 4);
        assert_eq!((p[0].kind.as_str(), p[0].codec.as_str(), p[0].profile.as_str()), ("video", "hevc", "Main 10 DV"));
        assert_eq!((p[1].codec.as_str(), p[1].profile.as_str(), p[1].default), ("dts", "DTS-HD MA", true));
        assert_eq!((p[1].lang.as_str(), p[1].title.as_str()), ("spa", "Latino"));
        assert!(!p[2].default);
        assert_eq!((p[3].kind.as_str(), p[3].title.as_str()), ("subtitle", "Forzados"));
    }
}
