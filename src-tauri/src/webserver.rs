use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

const REMOTE_HTML: &str = include_str!("remote.html");

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
pub fn web_server_start(port: Option<u16>) -> Result<WebStatus, String> {
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
            (Method::Post, "/key") => {
                let mut body = String::new();
                if req.as_reader().read_to_string(&mut body).is_err() {
                    let r = Response::from_string("bad body")
                        .with_status_code(StatusCode(400));
                    req.respond(r)
                } else {
                    match parse_key_body(&body) {
                        Some(k) => match crate::press_key(&k) {
                            Ok(_) => req.respond(Response::from_string("ok")),
                            Err(e) => {
                                eprintln!("[web /key] press_key fail: {}", e);
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
