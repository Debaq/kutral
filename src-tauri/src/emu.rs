// Emulador — lanza RetroArch fullscreen con el core libretro del sistema.
//
// Mismo molde que player.rs (mpv): spawneamos un proceso nativo fullscreen
// sobre kutral y lo controlamos por su interfaz de red.
//
// Control: RetroArch expone un "network command interface" por UDP. Le
// mandamos comandos de texto plano (PAUSE_TOGGLE, SAVE_STATE, QUIT, RESET…)
// al puerto 55355, igual que a mpv por su socket IPC. Lo habilitamos con un
// config temporal pasado por --appendconfig (no tocamos el retroarch.cfg del
// usuario).
//
// Cores por sistema (libretro):
//   nes  → fceumm        snes → snes9x
//   gbc  → gambatte      ds   → melonds
//
// Requisito: `retroarch` en el PATH + los cores .so disponibles. En la ISO
// vienen bundleados (como mpv).

use std::io::Write;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

use tauri::{Emitter, Manager};

/// Estado del emulador: el proceso RetroArch vivo (si hay).
#[derive(Default)]
pub struct EmuState {
    pub child: Mutex<Option<Child>>,
}

/// Puerto UDP del network command interface de RetroArch.
const NET_CMD_PORT: u16 = 55355;

/// Nombre del core libretro para cada sistema soportado.
fn core_name(system: &str) -> Result<&'static str, String> {
    match system {
        "nes" => Ok("fceumm"),
        "snes" => Ok("snes9x"),
        "gba" => Ok("mgba"),
        "gbc" => Ok("gambatte"),
        "ds" => Ok("melonds"),
        other => Err(format!("sistema desconocido: {other}")),
    }
}

/// Carpeta de binarios embebidos (retroarch + cores).
/// Producción: dentro del bundle (resource_dir/vendor, copiado vía
/// bundle.resources). Dev: src-tauri/vendor junto al manifiesto, porque
/// `tauri dev` NO copia los recursos al resource_dir.
fn vendor_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();

    // 1) Bundle (release).
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor"));
    }
    // 2) Dev: src-tauri/vendor (ruta del crate en tiempo de compilación).
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor"));
    // 3) Junto al ejecutable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("vendor"));
        }
    }

    cands.into_iter().find(|p| p.exists())
}

/// Ruta al binario RetroArch EMBEBIDO. Solo en dev (sin bundle) cae a PATH.
fn retroarch_bin(app: &tauri::AppHandle) -> String {
    #[cfg(windows)]
    let exe = "retroarch.exe";
    #[cfg(not(windows))]
    let exe = "retroarch";

    if let Some(v) = vendor_dir(app) {
        let p = v.join(exe);
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    // Dev fallback (solo cuando no hay bundle): PATH.
    exe.to_string()
}

/// Ruta al .so/.dll del core EMBEBIDO. Solo en dev cae a búsqueda en disco.
fn core_path(app: &tauri::AppHandle, system: &str) -> Result<String, String> {
    let core = core_name(system)?;
    #[cfg(windows)]
    let file = format!("{core}_libretro.dll");
    #[cfg(not(windows))]
    let file = format!("{core}_libretro.so");

    // 1) Core embebido en el bundle (lo normal en producción).
    if let Some(v) = vendor_dir(app) {
        let p = v.join("cores").join(&file);
        if p.exists() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }

    // 2) Dev fallback: dirs del sistema (solo para desarrollar sin bundle).
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("KUTRAL_CORE_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".config/retroarch/cores"));
    }
    dirs.push(PathBuf::from("/usr/lib/libretro"));
    dirs.push(PathBuf::from("/usr/lib64/libretro"));
    for d in &dirs {
        let p = d.join(&file);
        if p.exists() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    Err(format!("core embebido no encontrado: {file}"))
}

/// Repo de libretro-thumbnails (catálogo de carátulas) por sistema.
fn repo_name(system: &str) -> Result<&'static str, String> {
    match system {
        "nes" => Ok("Nintendo_-_Nintendo_Entertainment_System"),
        "snes" => Ok("Nintendo_-_Super_Nintendo_Entertainment_System"),
        "gba" => Ok("Nintendo_-_Game_Boy_Advance"),
        "gbc" => Ok("Nintendo_-_Game_Boy_Color"),
        "ds" => Ok("Nintendo_-_Nintendo_DS"),
        other => Err(format!("sistema desconocido: {other}")),
    }
}

/// Carpeta del set No-Intro en Myrient (mismos nombres que las carátulas).
fn myrient_folder(system: &str) -> Result<&'static str, String> {
    match system {
        "nes" => Ok("Nintendo - Nintendo Entertainment System (Headerless)"),
        "snes" => Ok("Nintendo - Super Nintendo Entertainment System"),
        "gba" => Ok("Nintendo - Game Boy Advance"),
        "gbc" => Ok("Nintendo - Game Boy Color"),
        "ds" => Ok("Nintendo - Nintendo DS (Decrypted)"),
        other => Err(format!("sistema desconocido: {other}")),
    }
}

/// Carpeta local de ROMs de un sistema: <app_data>/roms/<sys>/.
fn roms_system_dir(app: &tauri::AppHandle, system: &str) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("roms").join(system))
}

/// Cliente HTTP con User-Agent (GitHub lo exige).
fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("kutral")
        .build()
        .map_err(|e| format!("http build: {e}"))
}

/// Catálogo completo de un sistema: todos los nombres de juego (de las
/// carátulas de libretro-thumbnails). Cachea a disco; sin red tras la 1ª vez.
#[tauri::command]
pub async fn emu_catalog(
    app: tauri::AppHandle,
    system: String,
) -> Result<Vec<String>, String> {
    let repo = repo_name(&system)?;

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cache dir: {e}"))?
        .join("emu");
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let cache_file = cache_dir.join(format!("{system}.json"));

    if let Ok(txt) = std::fs::read_to_string(&cache_file) {
        if let Ok(v) = serde_json::from_str::<Vec<String>>(&txt) {
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }

    let url = format!(
        "https://api.github.com/repos/libretro-thumbnails/{repo}/git/trees/master?recursive=1"
    );
    let resp = http()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("catálogo red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("catálogo {}", resp.status()));
    }
    let json: serde_json::Value =
        resp.json().await.map_err(|e| format!("catálogo json: {e}"))?;

    let mut names: Vec<String> = Vec::new();
    if let Some(tree) = json.get("tree").and_then(|t| t.as_array()) {
        for item in tree {
            if let Some(path) = item.get("path").and_then(|p| p.as_str()) {
                if let Some(rest) = path.strip_prefix("Named_Boxarts/") {
                    if let Some(name) = rest.strip_suffix(".png") {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    names.sort();

    if !names.is_empty() {
        let _ = std::fs::write(
            &cache_file,
            serde_json::to_string(&names).unwrap_or_default(),
        );
    }
    Ok(names)
}

/// Qué juegos del sistema ya tengo (nombres sin extensión, de la carpeta de ROMs).
#[tauri::command]
pub fn emu_owned(app: tauri::AppHandle, system: String) -> Result<Vec<String>, String> {
    let dir = roms_system_dir(&app, &system)?;
    let mut out: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    Ok(out)
}

/// Descarga el ROM al hacer clic. Mirror HTTP (Myrient) → guarda en
/// <app_data>/roms/<sys>/<name>.zip con progreso por evento. (RD = respaldo, TODO.)
#[tauri::command]
pub async fn emu_download(
    app: tauri::AppHandle,
    system: String,
    name: String,
) -> Result<String, String> {
    if name.is_empty() {
        return Err("nombre vacío".into());
    }
    let folder = myrient_folder(&system)?;
    let dir = roms_system_dir(&app, &system)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let dest = dir.join(format!("{name}.zip"));

    // Si ya está, listo.
    if dest.exists() {
        return Ok(dest.to_string_lossy().into_owned());
    }

    let url = format!(
        "https://myrient.erista.me/files/No-Intro/{}/{}.zip",
        urlencoding::encode(folder),
        urlencoding::encode(&name)
    );

    let mut resp = http()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("descarga red: {e}"))?;
    if !resp.status().is_success() {
        // TODO: aquí cae el respaldo RealDebrid (resolver magnet del ROM).
        return Err(format!("mirror {} — no está en Myrient", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    // Descarga a archivo temporal; al terminar se renombra (evita ROM a medias).
    let tmp = dir.join(format!("{name}.zip.part"));
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("crear: {e}"))?;
    let mut received: u64 = 0;

    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).map_err(|e| format!("escribir: {e}"))?;
                received += chunk.len() as u64;
                let _ = app.emit(
                    "emu_download_progress",
                    serde_json::json!({
                        "system": system,
                        "name": name,
                        "received": received,
                        "total": total,
                    }),
                );
            }
            Ok(None) => break,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("descarga: {e}"));
            }
        }
    }
    drop(file);
    std::fs::rename(&tmp, &dest).map_err(|e| format!("rename: {e}"))?;

    let _ = app.emit("emu_download_done", serde_json::json!({ "system": system, "name": name }));
    Ok(dest.to_string_lossy().into_owned())
}

/// Resuelve la ruta de la ROM. Si es relativa, la cuelga de <app_data>/roms/<sys>/.
fn resolve_rom(app: &tauri::AppHandle, system: &str, rom: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(rom);
    if p.is_absolute() {
        return Ok(p);
    }
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("roms").join(system).join(rom))
}

/// Escribe un config temporal que habilita el network command interface.
fn write_netcmd_config() -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join("kutral-retroarch.cfg");
    let body = format!(
        "network_cmd_enable = \"true\"\nnetwork_cmd_port = \"{NET_CMD_PORT}\"\n"
    );
    std::fs::write(&path, body).map_err(|e| format!("write cfg: {e}"))?;
    Ok(path)
}

/// Lanza RetroArch fullscreen con el core del sistema y la ROM. Mata cualquier
/// emulador previo.
#[tauri::command]
pub fn emu_play(
    app: tauri::AppHandle,
    state: tauri::State<'_, EmuState>,
    system: String,
    rom: String,
) -> Result<(), String> {
    use std::process::Command;

    if rom.is_empty() {
        return Err("rom vacía".into());
    }

    let bin = retroarch_bin(&app);
    let core = core_path(&app, &system)?;
    let rom_path = resolve_rom(&app, &system, &rom)?;
    if !rom_path.exists() {
        return Err(format!("ROM no existe: {}", rom_path.display()));
    }
    let cfg = write_netcmd_config()?;

    // Cerrar emulador anterior si quedó vivo.
    kill_existing(&state);

    let mut cmd = Command::new(&bin);
    // RetroArch va como AppImage: correrlo sin FUSE (se auto-extrae a /tmp).
    cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
    cmd.arg("-L")
        .arg(&core)
        .arg(rom_path.as_os_str())
        .arg("--fullscreen")
        .arg("--appendconfig")
        .arg(cfg.as_os_str());

    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "retroarch embebido no encontrado en el bundle".to_string()
        } else {
            format!("spawn retroarch: {e}")
        }
    })?;

    *state.child.lock().unwrap() = Some(child);
    Ok(())
}

/// Envía un comando de texto al network command interface de RetroArch por UDP.
/// Ej: "PAUSE_TOGGLE", "SAVE_STATE", "LOAD_STATE", "RESET", "MENU_TOGGLE", "QUIT".
#[tauri::command]
pub fn emu_cmd(cmd: String) -> Result<(), String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("udp bind: {e}"))?;
    let addr = format!("127.0.0.1:{NET_CMD_PORT}");
    sock.send_to(cmd.as_bytes(), &addr)
        .map_err(|e| format!("udp send: {e}"))?;
    Ok(())
}

/// Cierra RetroArch (QUIT por red + kill por si acaso).
#[tauri::command]
pub fn emu_stop(state: tauri::State<'_, EmuState>) -> Result<(), String> {
    let _ = emu_cmd("QUIT".into());
    kill_existing(&state);
    Ok(())
}

/// ¿Hay un emulador vivo?
#[tauri::command]
pub fn emu_running(state: tauri::State<'_, EmuState>) -> bool {
    let mut guard = state.child.lock().unwrap();
    match guard.as_mut() {
        Some(c) => match c.try_wait() {
            Ok(Some(_)) => {
                *guard = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

fn kill_existing(state: &tauri::State<'_, EmuState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
