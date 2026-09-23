use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::Emitter;
use tiny_http::{Header, Method, Response, Server, StatusCode};

/// Control remoto (GET /): navegar el catálogo y manejar la reproducción.
const CONTROL_HTML: &str = include_str!("control.html");

/// jsQR (UMD) servido en GET /jsqr.js: lector de QR en JS puro, fallback de
/// escaneo para navegadores sin BarcodeDetector (Safari iOS). Embebido para
/// funcionar offline en la red local (sin CDN).
const JSQR_JS: &str = include_str!("jsqr.min.js");

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
    const r = await fetch('text', {
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
    token: String,
}

/// Secreto que va en la URL del QR (`http://ip:puerto/<token>/...`). Sin él
/// cualquier equipo de la red, o cualquier página web abierta en este mismo
/// equipo (un anuncio dentro del iframe del reproductor, por ejemplo), podía
/// mandar teclas y texto con un `fetch` que el navegador ni siquiera frena por
/// CORS. Se guarda en disco para que el control que el celular tiene en
/// favoritos siga sirviendo después de reiniciar.
fn token(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    let ruta = app.path().app_config_dir().ok().map(|d| d.join("web-token"));
    if let Some(t) = ruta.as_ref().and_then(|r| std::fs::read_to_string(r).ok()) {
        let t = t.trim();
        if t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit()) {
            return t.to_string();
        }
    }
    let mut b = [0u8; 12];
    if getrandom::getrandom(&mut b).is_err() {
        // Sin fuente de azar del sistema (no debería pasar): RandomState
        // también sale sembrado por el sistema operativo.
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        b[..8].copy_from_slice(&h.finish().to_le_bytes());
    }
    let t = hex::encode(b);
    if let Some(r) = ruta {
        if let Some(dir) = r.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&r, &t).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&r, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    t
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

/// Key de TMDb que usa el control web para buscar y listar. El backend no la
/// guarda en ningún lado (vive en localStorage del front), así que el front la
/// empuja con `web_set_tmdb_key` al arrancar y cada vez que cambia.
fn tmdb_key() -> &'static Mutex<String> {
    static K: OnceLock<Mutex<String>> = OnceLock::new();
    K.get_or_init(|| Mutex::new(String::new()))
}

#[tauri::command]
pub fn web_set_tmdb_key(key: String) {
    let mut g = tmdb_key().lock().unwrap_or_else(|e| e.into_inner());
    *g = key;
}

fn key_actual() -> String {
    tmdb_key()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Valor de un parámetro de la query string, ya decodificado.
fn query_param(url: &str, name: &str) -> Option<String> {
    let qs = url.split_once('?')?.1;
    for par in qs.split('&') {
        let (k, v) = par.split_once('=').unwrap_or((par, ""));
        if k == name {
            return Some(urlencoding::decode(&v.replace('+', " ")).ok()?.into_owned());
        }
    }
    None
}

/// Listado de TMDb → lo mínimo que el control necesita para pintar una card.
/// El control corre en un celular por la red local: mandarle el objeto entero
/// de TMDb sería mandar diez veces más de lo que muestra.
fn items_json(resp: &crate::TmdbListResp, tipo: &str) -> String {
    #[derive(Serialize)]
    struct Card<'a> {
        id: u64,
        tipo: &'a str,
        titulo: String,
        anio: String,
        poster: Option<&'a str>,
        voto: f32,
    }
    let cards: Vec<Card> = resp
        .results
        .iter()
        .map(|it| {
            let fecha = it
                .release_date
                .as_deref()
                .or(it.first_air_date.as_deref())
                .unwrap_or("");
            Card {
                id: it.id,
                tipo,
                titulo: it
                    .title
                    .clone()
                    .or_else(|| it.name.clone())
                    .unwrap_or_default(),
                anio: fecha.chars().take(4).collect(),
                poster: it.poster_path.as_deref(),
                voto: it.vote_average,
            }
        })
        .collect();
    serde_json::json!({ "items": cards }).to_string()
}

fn json_resp(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut r = Response::from_string(body);
    if let Some(h) = header(b"Content-Type", b"application/json; charset=utf-8") {
        r = r.with_header(h);
    }
    if let Some(h) = header(b"Cache-Control", b"no-store") {
        r = r.with_header(h);
    }
    r
}

enum Ruta {
    /// Ruta ya sin el prefijo del token, lista para el `match`.
    Interna(String),
    Redirigir(String),
}

/// Todo cuelga de `/<token>/` salvo lo que no expone nada. Sin el token la
/// ruta queda en una que no existe y se responde 404, igual que a cualquier
/// otra: no se delata que el servidor pide un secreto.
fn resolver(path: &str, prefijo: &str) -> Ruta {
    if path == "/jsqr.js" || path == "/health" {
        return Ruta::Interna(path.to_string());
    }
    match path.strip_prefix(prefijo) {
        // Sin la barra final las rutas relativas de la página resolverían
        // fuera del prefijo.
        Some("") => Ruta::Redirigir(format!("{}/", prefijo)),
        Some(resto) if resto.starts_with('/') => Ruta::Interna(resto.to_string()),
        _ => Ruta::Interna("/no-existe".to_string()),
    }
}

fn build_url(ip: &str, port: u16, token: &str) -> String {
    format!("http://{}:{}/{}", ip, port, token)
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

/// Atiende un pedido del control. /buscar y /catalogo corren en su propio
/// hilo: esperan a TMDb (hasta 15 s) y no pueden frenar las flechas.
fn atender(mut req: tiny_http::Request, app_th: &tauri::AppHandle, prefijo: &str) {
    let url = req.url().to_string();
    let method = req.method().clone();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let path = match resolver(&path, prefijo) {
        Ruta::Interna(p) => p,
        Ruta::Redirigir(destino) => {
            let r = Response::empty(302);
            let r = match header(b"Location", destino.as_bytes()) {
                Some(h) => r.with_header(h),
                None => r,
            };
            if let Err(e) = req.respond(r) {
                eprintln!("[web] respond err: {}", e);
            }
            return;
        }
    };

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
            // Estado en vivo del reproductor para que el control muestre
            // título + barra de progreso mientras mpv reproduce.
            let st = crate::player::status_for(app_th);
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
        (Method::Get, "/buscar") => {
            // Buscar desde el celular, con el teclado del celular. Es la
            // razón principal por la que alguien toma el teléfono en vez
            // de deletrear con la cruceta.
            let q = query_param(&url, "q").unwrap_or_default();
            let tipo = query_param(&url, "tipo").unwrap_or_else(|| "movie".into());
            let key = key_actual();
            if key.is_empty() {
                req.respond(json_resp(
                    serde_json::json!({ "error": "sin key de TMDb" }).to_string(),
                ))
            } else if q.trim().is_empty() {
                req.respond(json_resp(serde_json::json!({ "items": [] }).to_string()))
            } else {
                let r = tauri::async_runtime::block_on(crate::tmdb_buscar(
                    tipo.clone(),
                    q,
                    1,
                    key,
                ));
                match r {
                    Ok(lista) => req.respond(json_resp(items_json(&lista, &tipo))),
                    Err(e) => req.respond(json_resp(
                        serde_json::json!({ "error": e }).to_string(),
                    )),
                }
            }
        }
        (Method::Get, "/catalogo") => {
            let tipo = query_param(&url, "tipo").unwrap_or_else(|| "movie".into());
            let key = key_actual();
            if key.is_empty() {
                req.respond(json_resp(
                    serde_json::json!({ "error": "sin key de TMDb" }).to_string(),
                ))
            } else {
                let r = tauri::async_runtime::block_on(crate::tmdb_trending(
                    tipo.clone(),
                    1,
                    key,
                ));
                match r {
                    Ok(lista) => req.respond(json_resp(items_json(&lista, &tipo))),
                    Err(e) => req.respond(json_resp(
                        serde_json::json!({ "error": e }).to_string(),
                    )),
                }
            }
        }
        (Method::Post, "/abrir") => {
            // Mandar un título a la tele. Reusa el handoff que ya existe
            // para Vera (`/?play=<id>&type=<tipo>`): el front resuelve el
            // detalle y entra a Descubrir como si se hubiera clickeado la
            // card en la tele.
            let mut body = String::new();
            if req.as_reader().read_to_string(&mut body).is_err() {
                req.respond(Response::from_string("bad body").with_status_code(StatusCode(400)))
            } else {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let id = v.get("id").and_then(|x| x.as_u64());
                let tipo = v.get("tipo").and_then(|x| x.as_str()).unwrap_or("");
                match (id, tipo) {
                    (Some(id), t) if t == "movie" || t == "tv" || t == "anime" => {
                        match app_th.emit("remote_open", serde_json::json!({ "id": id, "tipo": t }))
                        {
                            Ok(_) => req.respond(Response::from_string("ok")),
                            Err(e) => req.respond(
                                Response::from_string(format!("err: {}", e))
                                    .with_status_code(StatusCode(500)),
                            ),
                        }
                    }
                    _ => req.respond(
                        Response::from_string("bad json").with_status_code(StatusCode(400)),
                    ),
                }
            }
        }
        (Method::Post, "/accion") => {
            // Acciones que no son una tecla. Hoy: cambiar de fuente, que
            // en la tele es un botón de la barra del reproductor y acá no
            // tenía equivalente.
            let mut body = String::new();
            if req.as_reader().read_to_string(&mut body).is_err() {
                req.respond(Response::from_string("bad body").with_status_code(StatusCode(400)))
            } else {
                match parse_str_field(&body, "accion").as_deref() {
                    Some("fuente") => match app_th.emit("player:cambiar-fuente", ()) {
                        Ok(_) => req.respond(Response::from_string("ok")),
                        Err(e) => req.respond(
                            Response::from_string(format!("err: {}", e))
                                .with_status_code(StatusCode(500)),
                        ),
                    },
                    _ => req.respond(
                        Response::from_string("bad json").with_status_code(StatusCode(400)),
                    ),
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
                    Some(k) if down && crate::player::remote_to_mpv(app_th, &k) => {
                        // mpv está vivo y la tecla es de control de reproducción:
                        // se mandó directo al IPC de mpv (phone → Rust → mpv),
                        // sin pasar por el webview. Funciona aunque mpv tenga
                        // el foco (clave en Wayland).
                        req.respond(Response::from_string("mpv"))
                    }
                    // Soltar tecla: la interfaz solo reacciona al apretar.
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
}

#[tauri::command]
pub fn web_server_status() -> WebStatus {
    let g = state().lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(s) => WebStatus {
            running: true,
            ip: Some(s.ip.clone()),
            port: Some(s.port),
            url: Some(build_url(&s.ip, s.port, &s.token)),
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
            url: Some(build_url(&s.ip, s.port, &s.token)),
        });
    }
    let port = port.unwrap_or(8080);
    let token = token(&app);
    let prefijo = format!("/{}", token);
    let addr = format!("0.0.0.0:{}", port);
    let server = Server::http(&addr).map_err(|e| format!("bind {}: {}", addr, e))?;
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stop_th = stop.clone();
    let app_th = app.clone();
    let handle = std::thread::spawn(move || loop {
        if stop_th.load(Ordering::Relaxed) {
            break;
        }
        let req = match server.recv_timeout(Duration::from_millis(300)) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => break,
        };
        // Solo lo que espera a TMDb va a otro hilo. Las teclas se atienden
        // acá, en orden: dos flechas seguidas no pueden llegar al revés.
        let ruta = req.url().split('?').next().unwrap_or("");
        if ruta.ends_with("/buscar") || ruta.ends_with("/catalogo") {
            let app = app_th.clone();
            let prefijo = prefijo.clone();
            std::thread::spawn(move || atender(req, &app, &prefijo));
        } else {
            atender(req, &app_th, &prefijo);
        }
    });
    let ip = local_ip();
    *g = Some(ServerState {
        handle: Some(handle),
        stop,
        port,
        ip: ip.clone(),
        token: token.clone(),
    });
    Ok(WebStatus {
        running: true,
        ip: Some(ip.clone()),
        port: Some(port),
        url: Some(build_url(&ip, port, &token)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn interna(path: &str) -> Option<String> {
        match resolver(path, "/abc123") {
            Ruta::Interna(p) => Some(p),
            Ruta::Redirigir(_) => None,
        }
    }

    #[test]
    fn sin_token_no_se_llega_a_nada() {
        assert_eq!(interna("/key").as_deref(), Some("/no-existe"));
        assert_eq!(interna("/").as_deref(), Some("/no-existe"));
        assert_eq!(interna("/abc12/key").as_deref(), Some("/no-existe"));
        // El token tiene que ser el segmento completo, no un prefijo.
        assert_eq!(interna("/abc1234/key").as_deref(), Some("/no-existe"));
    }

    #[test]
    fn con_token_se_quita_el_prefijo() {
        assert_eq!(interna("/abc123/").as_deref(), Some("/"));
        assert_eq!(interna("/abc123/key").as_deref(), Some("/key"));
        assert_eq!(interna("/abc123/api").as_deref(), Some("/api"));
    }

    #[test]
    fn sin_barra_final_redirige() {
        assert!(matches!(resolver("/abc123", "/abc123"), Ruta::Redirigir(d) if d == "/abc123/"));
    }

    #[test]
    fn lo_publico_no_pide_token() {
        assert_eq!(interna("/jsqr.js").as_deref(), Some("/jsqr.js"));
        assert_eq!(interna("/health").as_deref(), Some("/health"));
    }
}
