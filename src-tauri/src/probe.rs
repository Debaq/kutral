// Inspección de pistas SIN reproducir (NIVEL 2 del filtro de idioma).
//
// ffprobe lee solo el header del contenedor vía HTTP range-requests (unos MB,
// no baja el archivo) y devuelve TODAS las pistas con su idioma/título. Sirve
// para confirmar si un release trae audio o subtítulos en español ANTES de
// abrir mpv, y saltar a otra fuente si no.
//
// Best-effort por diseño: si ffprobe no está instalado o el server no soporta
// ranges, devolvemos error y el caller decide (reproducir igual, no bloquear).

use serde::Serialize;
use std::time::Duration;

const PROBE_TIMEOUT_S: u64 = 20;

#[derive(Debug, Serialize)]
pub struct ProbeTrack {
    pub kind: String,  // "audio" | "subtitle"
    pub codec: String, // "aac", "subrip", …
    pub lang: String,  // tag ISO del contenedor ("spa", "es-419", "" si no hay)
    pub title: String, // título de la pista ("Latino", "Español (España)", …)
}

#[tauri::command]
pub async fn ffprobe_tracks(url: String) -> Result<Vec<ProbeTrack>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("probe: solo URLs http(s)".into());
    }
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
            &url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("ffprobe no disponible: {e}"))?;

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
        if kind != "audio" && kind != "subtitle" {
            continue;
        }
        tracks.push(ProbeTrack {
            kind: kind.to_string(),
            codec: s["codec_name"].as_str().unwrap_or_default().to_string(),
            lang: s["tags"]["language"].as_str().unwrap_or_default().to_string(),
            title: s["tags"]["title"].as_str().unwrap_or_default().to_string(),
        });
    }
    eprintln!(
        "[probe] {} pistas (audio/sub) en {}",
        tracks.len(),
        &url[..url.len().min(60)]
    );
    Ok(tracks)
}
