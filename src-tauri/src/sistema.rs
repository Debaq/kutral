// Sistema del equipo: volumen (wpctl), brillo (brightnessctl), red (nmcli) y
// detección del SO. Casi todo es solo Linux y fuera del sandbox de flatpak.

use serde::Serialize;

// ============================================================
// Audio (wpctl) + Brillo (brightnessctl)
// ============================================================

#[derive(Serialize)]
pub struct AudioState {
    volume: u8,
    muted: bool,
    available: bool,
}

#[tauri::command]
pub async fn audio_get() -> Result<AudioState, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(AudioState { volume: 50, muted: false, available: false }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let out = match Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return Ok(AudioState { volume: 50, muted: false, available: false }),
        };
        let s = String::from_utf8_lossy(&out.stdout);
        let mut volume = 50u8;
        if let Some(idx) = s.find("Volume:") {
            let after = &s[idx + 7..];
            if let Some(tok) = after.split_whitespace().next() {
                if let Ok(v) = tok.parse::<f32>() {
                    let pct = (v * 100.0).round();
                    volume = pct.clamp(0.0, 150.0) as u8;
                }
            }
        }
        let muted = s.contains("MUTED");
        Ok(AudioState { volume, muted, available: true })
    }
}

#[tauri::command]
pub async fn audio_set(volume: u8) -> Result<(), String> {
    let v = volume.min(150);
    #[cfg(not(target_os = "linux"))]
    { let _ = v; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = format!("{:.2}", v as f32 / 100.0);
        let out = Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &arg])
            .output()
            .map_err(|e| format!("wpctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn audio_set_mute(muted: bool) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    { let _ = muted; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = if muted { "1" } else { "0" };
        let out = Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", arg])
            .output()
            .map_err(|e| format!("wpctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct BrightnessState {
    percent: u8,
    available: bool,
}

#[tauri::command]
pub async fn brightness_get() -> Result<BrightnessState, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(BrightnessState { percent: 100, available: false }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let cur = Command::new("brightnessctl").args(["get"]).output();
        let max = Command::new("brightnessctl").args(["max"]).output();
        if let (Ok(c), Ok(m)) = (cur, max) {
            if c.status.success() && m.status.success() {
                let cs = String::from_utf8_lossy(&c.stdout).trim().parse::<u64>().unwrap_or(0);
                let ms = String::from_utf8_lossy(&m.stdout).trim().parse::<u64>().unwrap_or(0);
                if ms > 0 {
                    let p = ((cs as f64 / ms as f64) * 100.0).round() as u8;
                    return Ok(BrightnessState { percent: p.min(100), available: true });
                }
            }
        }
        Ok(BrightnessState { percent: 100, available: false })
    }
}

#[tauri::command]
pub async fn brightness_set(percent: u8) -> Result<(), String> {
    let p = percent.clamp(5, 100);
    #[cfg(not(target_os = "linux"))]
    { let _ = p; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = format!("{}%", p);
        let out = Command::new("brightnessctl")
            .args(["set", &arg])
            .output()
            .map_err(|e| format!("brightnessctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

// ============================================================
// Kütral OS — gestión de red (nmcli)
// ============================================================

#[derive(Serialize)]
pub struct WifiNetwork {
    ssid: String,
    signal: u8,
    secured: bool,
    in_use: bool,
}

#[derive(Serialize)]
pub struct WifiStatus {
    online: bool,
    connected_ssid: Option<String>,
}

// nmcli es de Kütral OS: fuera de Linux los comandos de wifi devuelven un
// valor fijo y nadie llama a esto.
#[cfg(target_os = "linux")]
fn parse_nmcli_line(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                cur.push(next);
                chars.next();
                continue;
            }
        }
        if c == ':' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

#[tauri::command]
pub async fn wifi_status() -> Result<WifiStatus, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(WifiStatus { online: true, connected_ssid: None }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let online = Command::new("nmcli")
            .args(["-t", "-f", "STATE", "general", "status"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "connected")
            .unwrap_or(false);
        let mut connected_ssid: Option<String> = None;
        if let Ok(o) = Command::new("nmcli")
            .args(["-t", "-f", "ACTIVE,SSID", "device", "wifi", "list"])
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let p = parse_nmcli_line(line);
                if p.len() >= 2 && p[0] == "yes" {
                    let s = p[1].trim();
                    if !s.is_empty() && s != "--" {
                        connected_ssid = Some(s.to_string());
                        break;
                    }
                }
            }
        }
        Ok(WifiStatus { online, connected_ssid })
    }
}

#[tauri::command]
pub async fn wifi_scan() -> Result<Vec<WifiNetwork>, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(Vec::new()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let _ = Command::new("nmcli").args(["device", "wifi", "rescan"]).output();
        let out = Command::new("nmcli")
            .args(["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list"])
            .output()
            .map_err(|e| format!("nmcli: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let mut nets: Vec<WifiNetwork> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in body.lines() {
            let p = parse_nmcli_line(line);
            if p.len() < 4 { continue; }
            let in_use = p[0].trim() == "*";
            let ssid = p[1].trim().to_string();
            if ssid.is_empty() || ssid == "--" { continue; }
            if !seen.insert(ssid.clone()) { continue; }
            let signal: u8 = p[2].trim().parse().unwrap_or(0);
            let sec = p[3].trim();
            let secured = !sec.is_empty() && sec != "--";
            nets.push(WifiNetwork { ssid, signal, secured, in_use });
        }
        nets.sort_by_key(|n| std::cmp::Reverse(n.signal));
        Ok(nets)
    }
}

#[tauri::command]
pub async fn wifi_connect(ssid: String, password: Option<String>) -> Result<(), String> {
    if ssid.is_empty() { return Err("ssid vacío".into()); }
    #[cfg(not(target_os = "linux"))]
    { let _ = password; return Err("solo soportado en Linux".into()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let mut args: Vec<String> = vec![
            "device".into(), "wifi".into(), "connect".into(), ssid,
        ];
        if let Some(p) = password.filter(|s| !s.is_empty()) {
            args.push("password".into());
            args.push(p);
        }
        let out = Command::new("nmcli")
            .args(&args)
            .output()
            .map_err(|e| format!("nmcli: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

// ============================================================
// Kütral OS — detección de OS host
// ============================================================

#[derive(Serialize)]
pub struct OsInfo {
    is_kutral_os: bool,
    platform: &'static str,
    version: Option<String>,
    /// Corriendo dentro de un sandbox (flatpak). Cambia lo que la app puede
    /// hacer: /app es de solo lectura (el updater no puede instalar nada) y
    /// nmcli/brightnessctl/wpctl no existen dentro.
    sandboxed: bool,
}

fn detect_kutral_os() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("KUTRAL_OS").ok().as_deref() == Some("1") {
            return true;
        }
        if std::path::Path::new("/etc/kutral-os-release").exists() {
            return true;
        }
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            for line in contents.lines() {
                if line.trim() == "ID=kutral-os" {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// ¿Estamos dentro de un flatpak? El runtime siempre monta /.flatpak-info en
/// el sandbox; es la forma canónica de detectarlo desde adentro.
fn detect_sandbox() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/.flatpak-info").exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[tauri::command]
pub fn os_info() -> OsInfo {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    };
    OsInfo {
        is_kutral_os: detect_kutral_os(),
        platform,
        version: std::env::var("KUTRAL_OS_VERSION").ok(),
        sandboxed: detect_sandbox(),
    }
}
