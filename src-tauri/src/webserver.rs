use serde::Serialize;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const REMOTE_HTML: &str = include_str!("remote.html");

/// Sistemas de emulador soportados (deben calzar con emu.rs).
const SYSTEMS: [&str; 5] = ["nes", "snes", "gba", "gbc", "ds"];

/// Carpeta donde viven las ROMs subidas: <app_data>/roms/.
fn roms_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("roms"))
}

/// Valida que un nombre de archivo sea seguro (sin path traversal).
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && name.len() <= 255
}

/// Lista las ROMs subidas como JSON: [{"system":"nes","name":"x.nes"},…].
fn roms_list_json(app: &tauri::AppHandle) -> String {
    #[derive(Serialize)]
    struct Rom {
        system: String,
        name: String,
    }
    let mut out: Vec<Rom> = Vec::new();
    if let Ok(base) = roms_dir(app) {
        for sys in SYSTEMS {
            let dir = base.join(sys);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    if e.path().is_file() {
                        if let Some(n) = e.file_name().to_str() {
                            out.push(Rom {
                                system: sys.to_string(),
                                name: n.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
}

/// Página web de subida de ROMs (servida en GET /roms).
const ROMS_HTML: &str = r#"<!doctype html>
<html lang="es"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>Subir ROMs — Kütral</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin:0; font-family:system-ui,sans-serif; background:#0d0d12; color:#eee;
         padding:24px; max-width:640px; margin:0 auto; }
  h1 { font-size:24px; margin:0 0 4px; }
  h1 b { background:linear-gradient(135deg,#7d4fff,#2b6cff);
         -webkit-background-clip:text; background-clip:text; color:transparent; }
  p.sub { color:#888; margin:0 0 20px; }
  label { display:block; font-size:13px; color:#aaa; margin:14px 0 6px; }
  select, input[type=file] { width:100%; padding:12px; border-radius:10px;
    background:#1a1a22; border:1px solid #2a2a36; color:#eee; font-size:15px; }
  button { width:100%; margin-top:18px; padding:14px; border:none; border-radius:10px;
    background:linear-gradient(135deg,#7d4fff,#2b6cff); color:#fff; font-size:16px;
    font-weight:700; cursor:pointer; }
  button:disabled { opacity:.5; }
  #log { margin-top:20px; font-size:13px; }
  .row { display:flex; justify-content:space-between; padding:8px 10px; border-radius:8px;
    background:#15151c; margin-bottom:6px; }
  .ok { color:#4ade80; } .err { color:#f87171; } .wait { color:#fbbf24; }
  ul#list { list-style:none; padding:0; margin:16px 0 0; }
  ul#list li { padding:6px 10px; background:#141019; border-radius:6px; margin-bottom:5px;
    font-size:13px; display:flex; justify-content:space-between; }
  ul#list .sys { color:#9c7bff; font-weight:700; text-transform:uppercase; font-size:11px; }
</style></head>
<body>
  <h1><b>Juegos</b> · subir ROMs</h1>
  <p class="sub">Selecciona todos los que quieras. El sistema se detecta por la
    extensión; los .zip y desconocidos van al sistema de respaldo.</p>

  <label for="sys">Sistema de respaldo (para .zip y desconocidos)</label>
  <select id="sys">
    <option value="nes">NES</option>
    <option value="snes">SNES</option>
    <option value="gba">Game Boy Advance</option>
    <option value="gbc">GB Color</option>
    <option value="ds">Nintendo DS</option>
  </select>

  <label for="files">ROMs (puedes seleccionar varios)</label>
  <input id="files" type="file" multiple>

  <button id="go">Subir</button>
  <div id="log"></div>

  <label style="margin-top:24px">Ya subidas</label>
  <ul id="list"></ul>

<script>
const $ = (s) => document.querySelector(s);

// Autodetección de sistema por extensión. .zip/desconocido → respaldo.
const EXT2SYS = {
  nes:'nes',
  sfc:'snes', smc:'snes', fig:'snes', swc:'snes', bs:'snes',
  gba:'gba',
  gbc:'gbc', gb:'gbc',
  nds:'ds',
};
function sysFor(filename, fallback) {
  const ext = filename.split('.').pop().toLowerCase();
  return EXT2SYS[ext] || fallback;
}

async function refresh() {
  try {
    const r = await fetch('/roms/list');
    const items = await r.json();
    $('#list').innerHTML = items.map(i =>
      `<li><span>${i.name}</span><span class="sys">${i.system}</span></li>`).join('')
      || '<li style="color:#666">— vacío —</li>';
  } catch {}
}

$('#go').onclick = async () => {
  const fallback = $('#sys').value;
  const files = $('#files').files;
  if (!files.length) return;
  $('#go').disabled = true;
  $('#log').innerHTML = '';
  let ok = 0, fail = 0;
  for (const f of files) {
    const sys = sysFor(f.name, fallback);
    const row = document.createElement('div');
    row.className = 'row';
    row.innerHTML = `<span>${f.name}</span><span class="wait">${sys} · subiendo…</span>`;
    $('#log').appendChild(row);
    try {
      const res = await fetch(`/roms/${sys}/${encodeURIComponent(f.name)}`,
        { method:'PUT', body: f });
      row.lastChild.className = res.ok ? 'ok' : 'err';
      row.lastChild.textContent = res.ok ? `${sys} ✓` : 'error';
      res.ok ? ok++ : fail++;
    } catch {
      row.lastChild.className = 'err';
      row.lastChild.textContent = 'falló';
      fail++;
    }
  }
  $('#go').disabled = false;
  $('#go').textContent = `Subir (${ok} listos${fail?`, ${fail} fallaron`:''})`;
  refresh();
};
refresh();
</script>
</body></html>"#;

struct ServerState {
    handle: Option<JoinHandle<()>>,
    stop: std::sync::Arc<AtomicBool>,
    port: u16,
    ip: String,
}

fn state() -> &'static Mutex<Option<ServerState>> {
    static S: OnceLock<Mutex<Option<ServerState>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

#[derive(Serialize)]
pub struct WebStatus {
    pub running: bool,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub url: Option<String>,
}

fn local_ip() -> String {
    local_ip_address::local_ip()
        .map(|i| i.to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

fn build_url(ip: &str, port: u16) -> String {
    format!("http://{}:{}", ip, port)
}

fn header(name: &[u8], value: &[u8]) -> Option<Header> {
    Header::from_bytes(name, value).ok()
}

fn parse_key_body(body: &str) -> Option<String> {
    // JSON mínimo: {"key": "..."}
    let needle = "\"key\"";
    let i = body.find(needle)?;
    let after = &body[i + needle.len()..];
    let colon = after.find(':')?;
    let after = &after[colon + 1..].trim_start();
    let mut chars = after.chars();
    let q = chars.next()?;
    if q != '"' {
        return None;
    }
    let rest: String = chars.collect();
    let mut out = String::new();
    let mut it = rest.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = (&mut it).take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) {
                            out.push(c);
                        }
                    }
                }
                other => out.push(other),
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

#[tauri::command]
pub fn web_server_status() -> WebStatus {
    let g = state().lock().unwrap();
    match g.as_ref() {
        Some(s) => WebStatus {
            running: true,
            ip: Some(s.ip.clone()),
            port: Some(s.port),
            url: Some(build_url(&s.ip, s.port)),
        },
        None => WebStatus {
            running: false,
            ip: None,
            port: None,
            url: None,
        },
    }
}

#[tauri::command]
pub fn web_server_start(
    app: tauri::AppHandle,
    port: Option<u16>,
) -> Result<WebStatus, String> {
    let mut g = state().lock().unwrap();
    if g.is_some() {
        let s = g.as_ref().unwrap();
        return Ok(WebStatus {
            running: true,
            ip: Some(s.ip.clone()),
            port: Some(s.port),
            url: Some(build_url(&s.ip, s.port)),
        });
    }
    let port = port.unwrap_or(8080);
    let addr = format!("0.0.0.0:{}", port);
    let server = Server::http(&addr).map_err(|e| format!("bind {}: {}", addr, e))?;
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stop_th = stop.clone();
    let app_th = app.clone();
    let handle = std::thread::spawn(move || loop {
        if stop_th.load(Ordering::Relaxed) {
            break;
        }
        let mut req = match server.recv_timeout(Duration::from_millis(300)) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => break,
        };
        let url = req.url().to_string();
        let method = req.method().clone();
        let path = url.split('?').next().unwrap_or("/").to_string();

        let resp_result = match (method, path.as_str()) {
            (Method::Get, "/") | (Method::Get, "/index.html") => {
                let mut r = Response::from_string(REMOTE_HTML);
                if let Some(h) = header(b"Content-Type", b"text/html; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/health") => {
                req.respond(Response::from_string("ok"))
            }
            (Method::Get, "/roms") => {
                let mut r = Response::from_string(ROMS_HTML);
                if let Some(h) = header(b"Content-Type", b"text/html; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/roms/list") => {
                let body = roms_list_json(&app_th);
                let mut r = Response::from_string(body);
                if let Some(h) = header(b"Content-Type", b"application/json") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Put, p) if p.starts_with("/roms/") => {
                // Formato esperado: /roms/<sys>/<nombre>
                let rest = &p["/roms/".len()..];
                let mut it = rest.splitn(2, '/');
                let sys = it.next().unwrap_or("");
                let name_enc = it.next().unwrap_or("");
                let name = urlencoding::decode(name_enc)
                    .map(|c| c.into_owned())
                    .unwrap_or_default();

                if !SYSTEMS.contains(&sys) || !safe_name(&name) {
                    let r = Response::from_string("ruta inválida")
                        .with_status_code(StatusCode(400));
                    req.respond(r)
                } else {
                    let mut bytes = Vec::new();
                    match req.as_reader().read_to_end(&mut bytes) {
                        Err(_) => {
                            let r = Response::from_string("bad body")
                                .with_status_code(StatusCode(400));
                            req.respond(r)
                        }
                        Ok(_) => {
                            let res = roms_dir(&app_th).and_then(|base| {
                                let dir = base.join(sys);
                                std::fs::create_dir_all(&dir)
                                    .map_err(|e| format!("mkdir: {e}"))?;
                                std::fs::write(dir.join(&name), &bytes)
                                    .map_err(|e| format!("write: {e}"))
                            });
                            match res {
                                Ok(_) => {
                                    let _ = app_th.emit("rom_uploaded", name.clone());
                                    req.respond(Response::from_string("ok"))
                                }
                                Err(e) => {
                                    eprintln!("[web /roms] {}", e);
                                    let r = Response::from_string(format!("err: {}", e))
                                        .with_status_code(StatusCode(500));
                                    req.respond(r)
                                }
                            }
                        }
                    }
                }
            }
            (Method::Post, "/key") => {
                let mut body = String::new();
                if req.as_reader().read_to_string(&mut body).is_err() {
                    let r = Response::from_string("bad body")
                        .with_status_code(StatusCode(400));
                    req.respond(r)
                } else {
                    match parse_key_body(&body) {
                        Some(k) => {
                            // Emite evento al frontend; el frontend dispatcha
                            // un KeyboardEvent nativo. Portable a Wayland/X11/macOS/Windows
                            // sin depender de enigo (que falla en Wayland).
                            match app_th.emit("remote_key", k.clone()) {
                                Ok(_) => req.respond(Response::from_string("ok")),
                                Err(e) => {
                                    eprintln!("[web /key] emit fail: {}", e);
                                    let r = Response::from_string(format!("err: {}", e))
                                        .with_status_code(StatusCode(500));
                                    req.respond(r)
                                }
                            }
                        }
                        None => {
                            let r = Response::from_string("bad json")
                                .with_status_code(StatusCode(400));
                            req.respond(r)
                        }
                    }
                }
            }
            _ => {
                let r = Response::from_string("not found").with_status_code(StatusCode(404));
                req.respond(r)
            }
        };
        if let Err(e) = resp_result {
            eprintln!("[web] respond err: {}", e);
        }
    });
    let ip = local_ip();
    *g = Some(ServerState {
        handle: Some(handle),
        stop,
        port,
        ip: ip.clone(),
    });
    Ok(WebStatus {
        running: true,
        ip: Some(ip.clone()),
        port: Some(port),
        url: Some(build_url(&ip, port)),
    })
}

#[tauri::command]
pub fn web_server_stop() -> Result<(), String> {
    let mut g = state().lock().unwrap();
    if let Some(mut s) = g.take() {
        s.stop.store(true, Ordering::Relaxed);
        if let Some(h) = s.handle.take() {
            drop(g);
            let _ = h.join();
        }
    }
    Ok(())
}
