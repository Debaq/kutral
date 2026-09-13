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

use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

/// Estado del emulador: el proceso RetroArch vivo (si hay).
#[derive(Default)]
pub struct EmuState {
    pub child: Mutex<Option<Child>>,
}

/// Puerto UDP del network command interface de RetroArch.
const NET_CMD_PORT: u16 = 55355;
// Network Gamepad de RetroArch: recibe input de pad por UDP (player 1 =
// base_port + 0). Funciona aunque RetroArch tenga el foco, con hold real y
// baja latencia — a diferencia de despachar teclas en el webview (que no le
// llegan al proceso de RetroArch).
const NET_REMOTE_PORT: u16 = 55400;
const RETRO_DEVICE_JOYPAD: i32 = 1;

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

/// Nombre del set en libretro-database (con espacios).
fn dat_name(system: &str) -> Result<&'static str, String> {
    match system {
        "nes" => Ok("Nintendo - Nintendo Entertainment System"),
        "snes" => Ok("Nintendo - Super Nintendo Entertainment System"),
        "gba" => Ok("Nintendo - Game Boy Advance"),
        "gbc" => Ok("Nintendo - Game Boy Color"),
        "ds" => Ok("Nintendo - Nintendo DS"),
        other => Err(format!("sistema desconocido: {other}")),
    }
}

/// Extrae el valor de `key "valor"` dentro de un bloque del .dat.
fn dat_field(block: &str, key: &str) -> Option<String> {
    let pat = format!("{key} \"");
    let i = block.find(&pat)? + pat.len();
    let rest = &block[i..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Baja un .dat de libretro-database y lo parsea a (comment → valor del campo).
async fn fetch_dat(
    name: &str,
    category: &str,
    field: &str,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let url = format!(
        "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/{}/{}.dat",
        category,
        urlencoding::encode(name)
    );
    let client = match http() {
        Ok(c) => c,
        Err(_) => return map,
    };
    let body = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => return map,
    };
    for block in body.split("game (").skip(1) {
        if let (Some(name), Some(val)) = (dat_field(block, "comment"), dat_field(block, field)) {
            map.insert(name, val);
        }
    }
    map
}

/// Metadata para los pasillos tipo Blockbuster: género, año y franquicia
/// (señal de "conocido") por juego. Cachea a disco; sin red tras la 1ª vez.
#[tauri::command]
pub async fn emu_metadata(
    app: tauri::AppHandle,
    system: String,
) -> Result<serde_json::Value, String> {
    let name = dat_name(&system)?;

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cache dir: {e}"))?
        .join("emu");
    let _ = std::fs::create_dir_all(&cache_dir);
    // meta2: esquema ampliado (dev/editor/jugadores/mes/nota Edge).
    let cache_file = cache_dir.join(format!("meta2-{system}.json"));

    if let Ok(txt) = std::fs::read_to_string(&cache_file) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if v.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                return Ok(v);
            }
        }
    }

    let genres = fetch_dat(name, "genre", "genre").await;
    let years = fetch_dat(name, "releaseyear", "releaseyear").await;
    let franchises = fetch_dat(name, "franchise", "franchise").await;
    let esrb = fetch_dat(name, "esrb", "esrb_rating").await;
    let devs = fetch_dat(name, "developer", "developer").await;
    let pubs = fetch_dat(name, "publisher", "publisher").await;
    let users = fetch_dat(name, "maxusers", "users").await;
    let months = fetch_dat(name, "releasemonth", "releasemonth").await;
    let edge = fetch_dat(name, "magazine/edge", "edge_rating").await;

    // Une todas las claves de comment vistas.
    let mut keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in [&genres, &years, &franchises, &esrb, &devs, &pubs, &users, &months, &edge] {
        keys.extend(m.keys().cloned());
    }

    let mut obj = serde_json::Map::new();
    for k in keys {
        let mut e = serde_json::Map::new();
        let mut put = |field: &str, src: &std::collections::HashMap<String, String>| {
            if let Some(v) = src.get(&k) {
                e.insert(field.into(), serde_json::Value::String(v.clone()));
            }
        };
        put("genre", &genres);
        put("year", &years);
        put("esrb", &esrb);
        put("developer", &devs);
        put("publisher", &pubs);
        put("players", &users);
        put("month", &months);
        put("edge", &edge);
        if franchises.contains_key(&k) {
            e.insert("franchise".into(), serde_json::Value::Bool(true));
        }
        obj.insert(k, serde_json::Value::Object(e));
    }
    let value = serde_json::Value::Object(obj);

    if value.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
        let _ = std::fs::write(&cache_file, value.to_string());
    }
    Ok(value)
}

/// Título limpio para buscar en Wikipedia: sin (tags) ni [tags], conserva
/// mayúsculas y espacios. "Chrono Trigger (USA)" → "Chrono Trigger".
fn clean_title(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Resumen de Wikipedia por título directo (REST summary). None si no existe o
/// es una página de desambiguación.
async fn wiki_summary(lang: &str, title: &str) -> Option<(String, String)> {
    let url = format!(
        "https://{lang}.wikipedia.org/api/rest_v1/page/summary/{}",
        urlencoding::encode(title)
    );
    let resp = http().ok()?.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let j: serde_json::Value = resp.json().await.ok()?;
    if j.get("type").and_then(|t| t.as_str()) == Some("disambiguation") {
        return None;
    }
    let extract = j.get("extract").and_then(|e| e.as_str()).unwrap_or("");
    if extract.trim().is_empty() {
        return None;
    }
    let page = j
        .get("content_urls")
        .and_then(|c| c.get("desktop"))
        .and_then(|d| d.get("page"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    Some((extract.to_string(), page))
}

/// Busca en Wikipedia y devuelve el título del primer resultado.
async fn wiki_search(lang: &str, query: &str) -> Option<String> {
    let url = format!(
        "https://{lang}.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=1&srsearch={}",
        urlencoding::encode(query)
    );
    let j: serde_json::Value = http().ok()?.get(&url).send().await.ok()?.json().await.ok()?;
    j.get("query")?
        .get("search")?
        .get(0)?
        .get("title")?
        .as_str()
        .map(|s| s.to_string())
}

/// Sinopsis del juego desde Wikipedia (ES → EN; directo → búsqueda). On-demand
/// al abrir la ficha; cachea aciertos y fallos a disco. Devuelve {extract,url}
/// o {} si no se encontró.
#[tauri::command]
pub async fn emu_synopsis(
    app: tauri::AppHandle,
    name: String,
) -> Result<serde_json::Value, String> {
    let title = clean_title(&name);
    if title.is_empty() {
        return Ok(serde_json::json!({}));
    }

    // Caché: un archivo por título (hash) en app_cache/emu/synopsis/.
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cache dir: {e}"))?
        .join("emu")
        .join("synopsis");
    let _ = std::fs::create_dir_all(&dir);
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    let file = dir.join(format!("{}.json", &hex::encode(h.finalize())[..16]));
    if let Ok(txt) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            return Ok(v);
        }
    }

    let mut found: Option<(String, String, String)> = None; // extract, url, lang
    'outer: for lang in ["es", "en"] {
        if let Some((ex, url)) = wiki_summary(lang, &title).await {
            found = Some((ex, url, lang.to_string()));
            break 'outer;
        }
        let q = format!("{title} videojuego");
        if let Some(t) = wiki_search(lang, &q).await {
            if let Some((ex, url)) = wiki_summary(lang, &t).await {
                found = Some((ex, url, lang.to_string()));
                break 'outer;
            }
        }
    }

    let value = match found {
        Some((ex, url, lang)) => serde_json::json!({ "extract": ex, "url": url, "lang": lang }),
        None => serde_json::json!({}),
    };
    let _ = std::fs::write(&file, value.to_string());
    Ok(value)
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
        return Err(format!("mirror {} — no disponible", resp.status()));
    }
    // Mirror caído / catch-all: si responde HTML, NO es el ROM.
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if ctype.contains("text/html") {
        return Err("la descarga automática no está disponible (mirror caído) — sube el ROM tú vía el panel web".into());
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
    // Verificar que lo bajado sea un zip real, no una página de error.
    if let Err(e) = validate_rom(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| format!("rename: {e}"))?;

    let _ = app.emit("emu_download_done", serde_json::json!({ "system": system, "name": name }));
    Ok(dest.to_string_lossy().into_owned())
}

/// Clave de emparejamiento: quita grupos (...) y [...], minúsculas y solo
/// alfanumérico. "Mega Man X (USA) (Rev 1)" → "megamanx". Salva diferencias de
/// puntuación/caso entre el archivo y el catálogo libretro (& vs _, etc.).
fn base_title(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Busca el ROM en <app_data>/roms/<sys>/ por nombre, con CUALQUIER extensión
/// (.zip o rom crudo .nes/.sfc/.gba/.gbc/.nds). Empareja por título base, así
/// "Mega Man X.zip" calza con el catálogo "Mega Man X (USA)".
fn resolve_rom(app: &tauri::AppHandle, system: &str, rom: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(rom);
    if p.is_absolute() {
        return Ok(p);
    }
    let dir = roms_system_dir(app, system)?;
    let want = PathBuf::from(rom)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rom.to_string());

    // 1) Exacto tal cual (rápido).
    let exact = dir.join(rom);
    if exact.is_file() {
        return Ok(exact);
    }
    // 2) Por stem exacto, luego por título base.
    let want_base = base_title(&want);
    let mut por_base: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem == want {
                return Ok(path);
            }
            if por_base.is_none() && base_title(stem) == want_base {
                por_base = Some(path);
            }
        }
    }
    por_base.ok_or_else(|| format!("\"{want}\" no está en tu biblioteca — súbela primero"))
}

/// Valida que el archivo sea un ROM de verdad y no basura (HTML, página de
/// error, etc.). Los .zip deben empezar con "PK"; los demás se aceptan por
/// extensión conocida.
fn validate_rom(path: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut f = std::fs::File::open(path).map_err(|e| format!("abrir rom: {e}"))?;
    let mut magic = [0u8; 4];
    let n = f.read(&mut magic).unwrap_or(0);
    if n < 2 {
        return Err("ROM vacío o corrupto".into());
    }
    // HTML disfrazado (mirror caído guardó una página).
    if magic.starts_with(b"<!DO") || magic.starts_with(b"<htm") || magic.starts_with(b"<HTM") {
        return Err("el archivo no es un ROM (parece HTML) — vuelve a subirlo".into());
    }
    if ext == "zip" && &magic[..2] != b"PK" {
        return Err("zip inválido — vuelve a subir el ROM".into());
    }
    Ok(())
}

/// Escribe un config temporal que habilita el network command interface y,
/// si el usuario configuró el mando en Kütral, sus binds (ver padmap.rs).
fn write_netcmd_config(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join("kutral-retroarch.cfg");
    let mut body = format!(
        "network_cmd_enable = \"true\"\n\
         network_cmd_port = \"{NET_CMD_PORT}\"\n\
         network_remote_enable = \"true\"\n\
         network_remote_base_port = \"{NET_REMOTE_PORT}\"\n\
         network_remote_enable_user_p1 = \"true\"\n"
    );
    body.push_str(&crate::padmap::cfg_lines(app));
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
    validate_rom(&rom_path)?;
    let cfg = write_netcmd_config(&app)?;

    // Cerrar emulador anterior si quedó vivo.
    kill_existing(&state);

    let mut cmd = Command::new(&bin);
    crate::winproc::hide_console(&mut cmd);
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

    *state.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
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

fn alive(state: &tauri::State<'_, EmuState>) -> bool {
    let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
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

/// ¿Hay un emulador vivo?
#[tauri::command]
pub fn emu_running(state: tauri::State<'_, EmuState>) -> bool {
    alive(&state)
}

/// Envía un estado de botón al Network Gamepad de RetroArch (player 1).
/// `id` es un RETRO_DEVICE_ID_JOYPAD_* (B=0,Y=1,SELECT=2,START=3,UP=4,DOWN=5,
/// LEFT=6,RIGHT=7,A=8,X=9,L=10,R=11). `pressed` = true al apretar, false al
/// soltar (hold real). El paquete replica el struct remote_message de
/// RetroArch: { i32 port; i32 device; i32 index; i32 id; u16 state; } nativo.
fn send_pad(id: i32, pressed: bool) -> Result<(), String> {
    let mut buf = [0u8; 20];
    buf[0..4].copy_from_slice(&0i32.to_le_bytes()); // port (user 0)
    buf[4..8].copy_from_slice(&RETRO_DEVICE_JOYPAD.to_le_bytes());
    buf[8..12].copy_from_slice(&0i32.to_le_bytes()); // index
    buf[12..16].copy_from_slice(&id.to_le_bytes());
    buf[16..18].copy_from_slice(&(if pressed { 1u16 } else { 0u16 }).to_le_bytes());
    // 18..20 = padding (0)
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("udp bind: {e}"))?;
    sock.send_to(&buf, format!("127.0.0.1:{NET_REMOTE_PORT}"))
        .map_err(|e| format!("udp send: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn emu_input(id: i32, pressed: bool) -> Result<(), String> {
    send_pad(id, pressed)
}

/// Mapea una tecla canónica del mando (web/teclado) a un botón del retropad.
fn key_to_retropad(key: &str) -> Option<i32> {
    Some(match key {
        "ArrowUp" => 4,
        "ArrowDown" => 5,
        "ArrowLeft" => 6,
        "ArrowRight" => 7,
        "Enter" => 8,     // A (botón A del mando)
        "Backspace" => 0, // B
        " " => 9,         // X (botón X del mando web)
        "i" | "I" => 1,   // Y (botón Y del mando web)
        "m" | "M" => 2,   // Select
        "Escape" => 3,    // Start
        "[" => 10,        // L (LB)
        "]" => 11,        // R (RB)
        _ => return None,
    })
}

/// Si hay un emulador vivo y la tecla mapea a un botón, manda el estado al
/// Network Gamepad y devuelve true (el webserver NO debe emitir remote_key).
/// Camino phone → Rust → RetroArch, sin pasar por el webview.
pub fn remote_to_emu(app: &tauri::AppHandle, key: &str, pressed: bool) -> bool {
    use tauri::Manager;
    let state = app.state::<EmuState>();
    if !alive(&state) {
        return false;
    }
    match key_to_retropad(key) {
        Some(id) => {
            let _ = send_pad(id, pressed);
            true
        }
        None => false,
    }
}

fn kill_existing(state: &tauri::State<'_, EmuState>) {
    if let Some(mut child) = state.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
