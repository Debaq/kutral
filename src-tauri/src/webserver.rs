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

/// Control remoto vertical (GET /): navegar el catálogo y manejar la
/// reproducción. Es el predeterminado.
const CONTROL_HTML: &str = include_str!("control.html");

/// Mando de juegos horizontal (GET /mando): el gamepad de RetroArch. Se llega
/// desde el conmutador de la barra superior del control.
const REMOTE_HTML: &str = include_str!("remote.html");

/// jsQR (UMD) servido en GET /jsqr.js: lector de QR en JS puro, fallback de
/// escaneo para navegadores sin BarcodeDetector (Safari iOS). Embebido para
/// funcionar offline en la red local (sin CDN).
const JSQR_JS: &str = include_str!("jsqr.min.js");

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

/// Teclado web (servido en GET /api): escribir, pegar o ESCANEAR con la cámara
/// una API key larga desde el celular. El texto se manda a POST /text y el
/// backend lo emite como `remote_text`; el frontend lo escribe en el campo que
/// tengas enfocado en /config. Resuelve el problema de "nadie escribe esa API
/// gigante con el control".
const KEYBOARD_HTML: &str = r#"<!doctype html>
<html lang="es"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>Teclado — Kütral</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin:0; font-family:system-ui,sans-serif; background:#0d0d12; color:#eee;
         padding:24px; max-width:560px; margin:0 auto; min-height:100dvh; }
  h1 { font-size:23px; margin:0 0 4px; }
  h1 b { background:linear-gradient(135deg,#f3a951,#ffb86b);
         -webkit-background-clip:text; background-clip:text; color:transparent; }
  p.sub { color:#888; margin:0 0 20px; font-size:14px; line-height:1.5; }
  textarea { width:100%; min-height:120px; padding:14px; border-radius:12px;
    background:#1a1a22; border:1px solid #2a2a36; color:#eee; font-size:17px;
    font-family:ui-monospace,monospace; resize:vertical; }
  textarea:focus { border-color:#f3a951; outline:none; }
  .btns { display:grid; grid-template-columns:1fr 1fr; gap:10px; margin-top:14px; }
  button { padding:15px; border:none; border-radius:12px; font-size:15px;
    font-weight:700; cursor:pointer; color:#eee; background:#1c1c26;
    border:1px solid #2a2a36; }
  button:active { transform:scale(.97); }
  button.primary { grid-column:1 / -1; color:#1a1208;
    background:linear-gradient(135deg,#f3a951,#ffb86b); border:0; font-size:17px; }
  button:disabled { opacity:.5; }
  #cam { margin-top:14px; display:none; }
  #cam.on { display:block; }
  video { width:100%; border-radius:12px; background:#000; aspect-ratio:4/3;
    object-fit:cover; }
  #msg { margin-top:16px; font-size:15px; min-height:22px; text-align:center; }
  .ok { color:#4ade80; } .err { color:#f87171; } .wait { color:#fbbf24; }
  .tip { margin-top:22px; color:#666; font-size:12.5px; line-height:1.5; }
</style></head>
<body>
  <h1><b>Teclado</b> · Kütral</h1>
  <p class="sub">Escribe, pega o escanea un QR con la clave. Al enviar, se escribe
    en el campo seleccionado en la pantalla de Kütral.</p>

  <textarea id="txt" placeholder="Pega o escribe la API key aquí…"
    autocapitalize="off" autocomplete="off" autocorrect="off" spellcheck="false"></textarea>

  <div class="btns">
    <button id="paste">📋 Pegar</button>
    <button id="scan">📷 Escanear QR</button>
    <button id="send" class="primary">Enviar a Kütral →</button>
  </div>

  <div id="cam"><video id="video" playsinline muted></video></div>
  <canvas id="cv" style="display:none"></canvas>
  <div id="msg"></div>
  <p class="tip">Tip: en la pantalla de Kütral toca primero el campo que quieres
    llenar (TMDb, OMDb…) y luego envía desde aquí.</p>

<script src="/jsqr.js"></script>
<script>
const $ = (s) => document.querySelector(s);
const txt = $('#txt'), msg = $('#msg');
function say(t, cls){ msg.className = cls || ''; msg.textContent = t; }

$('#paste').onclick = async () => {
  try {
    const t = await navigator.clipboard.readText();
    if (t) { txt.value = t.trim(); say('Pegado ✓', 'ok'); }
    else say('Portapapeles vacío', 'wait');
  } catch {
    say('Pega a mano (mantén pulsado el campo)', 'wait');
    txt.focus();
  }
};

$('#send').onclick = async () => {
  const text = txt.value.trim();
  if (!text) { say('Escribe o pega algo primero', 'wait'); return; }
  try {
    const r = await fetch('/text', {
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({ text })
    });
    say(r.ok ? 'Enviado ✓ — revisa la pantalla' : 'Error al enviar', r.ok ? 'ok' : 'err');
  } catch { say('Sin conexión con Kütral', 'err'); }
};

// --- Escaneo de QR con la cámara ---
// Usa BarcodeDetector nativo (Chrome Android) si existe; si no, jsQR sobre un
// canvas (fallback para Safari iOS y otros sin BarcodeDetector).
let stream = null, scanning = false;
function found(value) {
  txt.value = (value || '').trim();
  say('QR leído ✓ — pulsa Enviar', 'ok');
  stopCam();
}
$('#scan').onclick = async () => {
  if (scanning) { stopCam(); return; }
  const hasNative = ('BarcodeDetector' in window);
  if (!hasNative && typeof jsQR === 'undefined') {
    say('No se pudo cargar el lector de QR. Usa Pegar o escribe a mano.', 'wait');
    return;
  }
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: 'environment' } });
    const v = $('#video');
    v.srcObject = stream;
    await v.play();
    $('#cam').classList.add('on');
    $('#scan').textContent = '✕ Cerrar cámara';
    scanning = true;
    say('Apunta al QR…', 'wait');

    if (hasNative) {
      const det = new BarcodeDetector({ formats: ['qr_code'] });
      const tick = async () => {
        if (!scanning) return;
        try {
          const codes = await det.detect(v);
          if (codes.length && codes[0].rawValue) { found(codes[0].rawValue); return; }
        } catch {}
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    } else {
      const cv = $('#cv'), ctx = cv.getContext('2d', { willReadFrequently: true });
      const tick = () => {
        if (!scanning) return;
        if (v.readyState === v.HAVE_ENOUGH_DATA && v.videoWidth) {
          cv.width = v.videoWidth;
          cv.height = v.videoHeight;
          ctx.drawImage(v, 0, 0, cv.width, cv.height);
          const img = ctx.getImageData(0, 0, cv.width, cv.height);
          const code = jsQR(img.data, img.width, img.height,
            { inversionAttempts: 'dontInvert' });
          if (code && code.data) { found(code.data); return; }
        }
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    }
  } catch {
    say('No se pudo abrir la cámara', 'err');
  }
};
function stopCam(){
  scanning = false;
  if (stream) stream.getTracks().forEach(t => t.stop());
  stream = null;
  $('#cam').classList.remove('on');
  $('#scan').textContent = '📷 Escanear QR';
}
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
    parse_str_field(body, "key")
}

/// Extrae el valor string de un campo JSON: {"<field>": "..."}.
/// Parser mínimo y tolerante (mismo enfoque que el resto del módulo, sin serde).
fn parse_str_field(body: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let i = body.find(&needle)?;
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
    let g = state().lock().unwrap_or_else(|e| e.into_inner());
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
    let mut g = state().lock().unwrap_or_else(|e| e.into_inner());
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
                let mut r = Response::from_string(CONTROL_HTML);
                if let Some(h) = header(b"Content-Type", b"text/html; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/mando") => {
                let mut r = Response::from_string(REMOTE_HTML);
                if let Some(h) = header(b"Content-Type", b"text/html; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/api") => {
                let mut r = Response::from_string(KEYBOARD_HTML);
                if let Some(h) = header(b"Content-Type", b"text/html; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/jsqr.js") => {
                let mut r = Response::from_string(JSQR_JS);
                if let Some(h) = header(b"Content-Type", b"application/javascript; charset=utf-8") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"max-age=86400") {
                    r = r.with_header(h);
                }
                req.respond(r)
            }
            (Method::Get, "/health") => {
                req.respond(Response::from_string("ok"))
            }
            (Method::Get, "/mpv") => {
                // Estado en vivo del reproductor para que el mando muestre
                // título + barra de progreso mientras mpv reproduce.
                let st = crate::player::status_for(&app_th);
                let body = serde_json::to_string(&st).unwrap_or_else(|_| "{}".into());
                let mut r = Response::from_string(body);
                if let Some(h) = header(b"Content-Type", b"application/json") {
                    r = r.with_header(h);
                }
                if let Some(h) = header(b"Cache-Control", b"no-store") {
                    r = r.with_header(h);
                }
                req.respond(r)
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
                    // ¿Es soltar la tecla? {"down": false}. Por defecto es apretar.
                    let down = !body.contains("\"down\":false")
                        && !body.contains("\"down\": false");
                    match parse_key_body(&body) {
                        Some(k) if crate::emu::remote_to_emu(&app_th, &k, down) => {
                            // RetroArch vivo: el botón va al Network Gamepad (UDP),
                            // con hold real (down/up). phone → Rust → RetroArch,
                            // sin pasar por el webview (clave: el juego tiene el foco).
                            req.respond(Response::from_string("emu"))
                        }
                        Some(k) if down && crate::player::remote_to_mpv(&app_th, &k) => {
                            // mpv está vivo y la tecla es de control de reproducción:
                            // se mandó directo al IPC de mpv (phone → Rust → mpv),
                            // sin pasar por el webview. Funciona aunque mpv tenga
                            // el foco (clave en Wayland).
                            req.respond(Response::from_string("mpv"))
                        }
                        // Soltar tecla fuera de un juego: no hay nada que navegar.
                        Some(_) if !down => req.respond(Response::from_string("ok")),
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
            (Method::Post, "/text") => {
                // Texto largo (API key) tecleado/pegado/escaneado en el celular.
                // Se emite al frontend, que lo escribe en el input enfocado.
                let mut body = String::new();
                if req.as_reader().read_to_string(&mut body).is_err() {
                    let r = Response::from_string("bad body")
                        .with_status_code(StatusCode(400));
                    req.respond(r)
                } else {
                    match parse_str_field(&body, "text") {
                        Some(t) => match app_th.emit("remote_text", t) {
                            Ok(_) => req.respond(Response::from_string("ok")),
                            Err(e) => {
                                eprintln!("[web /text] emit fail: {}", e);
                                let r = Response::from_string(format!("err: {}", e))
                                    .with_status_code(StatusCode(500));
                                req.respond(r)
                            }
                        },
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
    let mut g = state().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut s) = g.take() {
        s.stop.store(true, Ordering::Relaxed);
        if let Some(h) = s.handle.take() {
            drop(g);
            let _ = h.join();
        }
    }
    Ok(())
}
