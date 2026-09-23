// Servidor HTTP en la red de la casa para que la TV alcance cosas que viven en
// este equipo. Puerto efímero, arranca la primera vez que se transmite.
//
//   /sub-N.vtt   subtítulo en WebVTT con CORS (lo que exige Google Cast)
//   /sub-N.srt   el mismo subtítulo en SRT (lo que entiende Samsung por DLNA)
//   /v/<id>.ext  PASARELA de video: la TV pide HTTP plano y acá se lo pedimos
//                a la fuente por HTTPS, reenviando el Range para que los
//                saltos funcionen. Muchas TVs DLNA no hablan HTTPS y
//                RealDebrid redirige todo HTTP a HTTPS. También sirve lo que
//                solo existe en este equipo: el stream del torrent local
//                (127.0.0.1) y los archivos ya bajados al disco.
//
// Independiente del servidor del mando remoto (webserver.rs), que es opcional
// y tiene puerto configurable.
//
// Puerto FIJO (PUERTO): con un firewall activo (firewalld en Fedora/Arch, ufw
// en Ubuntu) la TV no puede entrar a un puerto aleatorio, y uno fijo se abre
// una sola vez. Si está ocupado se usa uno cualquiera. Además se cuenta si la
// TV llegó a pedir el video / los subtítulos: una falla sin pedidos es de red
// (firewall), no del formato, y no debe enseñarle nada al aprendizaje.

use std::collections::HashMap;
use std::net::IpAddr;
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Clone)]
struct Video {
    /// URL http(s) o ruta de un archivo del disco.
    url: String,
    content_type: String,
    /// URL del .srt para la cabecera `CaptionInfo.sec` (subtítulos Samsung).
    caption: Option<String>,
}

#[derive(Default)]
struct Estado {
    videos: HashMap<String, Video>,
    vtt: String,
    srt: String,
    /// Pedidos de la TV desde la última publicación.
    pedidos_video: u32,
    pedidos_subs: u32,
}

pub const PUERTO: u16 = 8765;

pub struct Lan {
    puerto: u16,
    estado: Arc<Mutex<Estado>>,
}

/// Perfil DLNA genérico: acepta rangos por bytes (OP=01) y streaming.
pub const DLNA_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";

static LAN: OnceLock<Lan> = OnceLock::new();

pub fn lan() -> Result<&'static Lan, String> {
    if let Some(l) = LAN.get() {
        return Ok(l);
    }
    // Dos transmisiones que arrancan a la vez no pueden levantar dos
    // servidores: el segundo tomaría un puerto cualquiera y quedaría huérfano
    // (get_or_init se queda con el primero).
    static ARRANQUE: Mutex<()> = Mutex::new(());
    let _g = ARRANQUE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(l) = LAN.get() {
        return Ok(l);
    }
    let server = Server::http(("0.0.0.0", PUERTO))
        .or_else(|_| Server::http("0.0.0.0:0"))
        .map_err(|e| format!("servidor LAN: {e}"))?;
    let puerto = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or("servidor LAN sin puerto")?;
    let estado = Arc::new(Mutex::new(Estado::default()));
    let est = estado.clone();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let est = est.clone();
            // Un hilo por pedido: la pasarela de video bloquea mientras dure
            // el stream y la TV abre varias conexiones al saltar.
            std::thread::spawn(move || atender(req, &est));
        }
    });
    eprintln!("[lan] servidor para la TV en el puerto {puerto}");
    Ok(LAN.get_or_init(|| Lan { puerto, estado }))
}

impl Lan {
    fn base(&self, tv_ip: &str, tv_puerto: u16) -> Result<String, String> {
        Ok(format!("http://{}:{}", ip_hacia(tv_ip, tv_puerto)?, self.puerto))
    }

    /// Publica el subtítulo en los dos formatos. Devuelve (url_vtt, url_srt).
    pub fn publicar_subs(&self, bytes: &[u8], tv_ip: &str, tv_puerto: u16) -> Result<(String, String), String> {
        let texto = texto_de(bytes);
        let mut e = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        e.vtt = srt_a_vtt(&texto);
        e.srt = vtt_a_srt(&texto);
        e.pedidos_subs = 0;
        drop(e);
        // Nombre distinto cada vez: las TVs cachean por URL.
        let n = marca();
        let base = self.base(tv_ip, tv_puerto)?;
        Ok((format!("{base}/sub-{n}.vtt"), format!("{base}/sub-{n}.srt")))
    }

    /// Publica un video (URL o archivo del disco) por la pasarela y devuelve
    /// la URL HTTP para la TV.
    pub fn publicar_video(
        &self,
        url: &str,
        content_type: &str,
        caption: Option<String>,
        tv_ip: &str,
        tv_puerto: u16,
    ) -> Result<String, String> {
        let ext = match content_type {
            "video/x-matroska" => "mkv",
            "video/webm" => "webm",
            "video/x-msvideo" => "avi",
            _ => "mp4",
        };
        // Aleatorio, no la hora: el id es lo único que separa a un equipo
        // cualquiera de la red de ver lo que se transmite (y de usar el enlace
        // de RealDebrid de la cuenta).
        let id = id_aleatorio();
        let mut e = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        // Solo el último video: nada de ir juntando URLs de RD en memoria.
        e.videos.clear();
        e.pedidos_video = 0;
        e.videos.insert(
            id.clone(),
            Video {
                url: url.strip_prefix("file://").unwrap_or(url).to_string(),
                content_type: content_type.to_string(),
                caption,
            },
        );
        drop(e);
        Ok(format!("{}/v/{id}.{ext}", self.base(tv_ip, tv_puerto)?))
    }
}

/// (pedidos de video, pedidos de subtítulos) desde la última publicación.
/// 0 si el servidor ni siquiera arrancó.
pub fn pedidos() -> (u32, u32) {
    lan_si_existe()
        .map(|l| {
            let e = l.estado.lock().unwrap_or_else(|e| e.into_inner());
            (e.pedidos_video, e.pedidos_subs)
        })
        .unwrap_or((0, 0))
}

fn lan_si_existe() -> Option<&'static Lan> {
    // Sin arrancarlo: preguntar no debe abrir un puerto.
    LAN.get()
}

#[derive(Serialize)]
pub struct RedInfo {
    pub puerto: u16,
    /// "firewalld" | "ufw" | "Windows Defender" | "" (ninguno que tape).
    pub firewall: String,
    /// Comando para abrir el puerto, listo para copiar.
    pub comando: String,
}

/// Nombre de la regla que abre el puerto en Windows: con ella ya creada el
/// aviso deja de salir.
#[cfg(windows)]
const REGLA_WINDOWS: &str = "Kutral TV";

// Windows Defender viene activo casi siempre: lo que importa es si ya hay
// regla. netsh habla el idioma del sistema; PowerShell devuelve valores fijos.
#[cfg(windows)]
fn firewall_activo() -> &'static str {
    let script = format!(
        "if (Get-NetFirewallRule -DisplayName '{REGLA_WINDOWS}' -ErrorAction SilentlyContinue) {{ 'regla' }} \
         elseif (@(Get-NetFirewallProfile | Where-Object {{ $_.Enabled -eq 'True' }}).Count -gt 0) {{ 'activo' }}"
    );
    let mut cmd = std::process::Command::new("powershell");
    crate::winproc::hide_console(&mut cmd);
    match cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]).output() {
        Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "activo" => "Windows Defender",
        _ => "",
    }
}

#[cfg(not(windows))]
fn firewall_activo() -> &'static str {
    let activo = |svc: &str| {
        std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", svc])
            .status()
            .map(|s| s.success())
            .ok()
    };
    match activo("firewalld") {
        Some(true) => return "firewalld",
        // Sin systemctl (flatpak): el directorio de estado delata al daemon.
        None if std::path::Path::new("/run/firewalld").exists() => return "firewalld",
        _ => {}
    }
    let ufw = std::fs::read_to_string("/etc/ufw/ufw.conf").unwrap_or_default();
    if ufw.lines().any(|l| l.trim() == "ENABLED=yes") {
        return "ufw";
    }
    ""
}

/// Qué puerto usa Kütral para la TV y si hay un firewall que lo tape.
/// Async: en Windows la consulta pasa por PowerShell (~1 s) y un comando
/// síncrono correría en el hilo de la UI.
#[tauri::command]
pub async fn cast_red_info() -> RedInfo {
    let puerto = LAN.get().map(|l| l.puerto).unwrap_or(PUERTO);
    let firewall = firewall_activo();
    let comando = match firewall {
        // Puerto y servicio en llamadas separadas: firewall-cmd rechaza
        // --add-port junto con --add-service ("not allowed with argument").
        "firewalld" => format!(
            "sudo firewall-cmd --permanent --add-port={puerto}/tcp && \
             sudo firewall-cmd --permanent --add-service=ssdp && sudo firewall-cmd --reload"
        ),
        "ufw" => format!("sudo ufw allow {puerto}/tcp && sudo ufw allow proto udp from any port 1900"),
        // Sin perfil: muchas redes de casa quedan marcadas como "pública" y una
        // regla solo para "privada" no las cubriría.
        #[cfg(windows)]
        "Windows Defender" => format!(
            "netsh advfirewall firewall add rule name=\"{REGLA_WINDOWS}\" dir=in action=allow protocol=TCP localport={puerto}"
        ),
        _ => String::new(),
    };
    RedInfo {
        puerto,
        firewall: firewall.into(),
        comando,
    }
}

fn id_aleatorio() -> String {
    let mut b = [0u8; 12];
    if getrandom::getrandom(&mut b).is_err() {
        // No debería pasar; RandomState también sale sembrado por el sistema.
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(marca());
        b[..8].copy_from_slice(&h.finish().to_le_bytes());
    }
    hex::encode(b)
}

fn marca() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// IP de este equipo en la interfaz que llega a la TV (con varias redes, la
/// "IP local" por defecto puede ser una que la TV no ve).
pub fn ip_hacia(tv_ip: &str, tv_puerto: u16) -> Result<IpAddr, String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.connect((tv_ip, tv_puerto)).map_err(|e| e.to_string())?;
    Ok(sock.local_addr().map_err(|e| e.to_string())?.ip())
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn atender(req: Request, est: &Mutex<Estado>) {
    let ruta = req.url().split('?').next().unwrap_or("").to_string();
    if *req.method() == Method::Options {
        let _ = req.respond(cors(Response::empty(204)));
        return;
    }
    if ruta.starts_with("/sub-") {
        let mut e = est.lock().unwrap_or_else(|e| e.into_inner());
        e.pedidos_subs += 1;
        let (cuerpo, tipo) = if ruta.ends_with(".srt") {
            (e.srt.clone(), "application/x-subrip; charset=utf-8")
        } else {
            (e.vtt.clone(), "text/vtt; charset=utf-8")
        };
        drop(e);
        let _ = req.respond(cors(Response::from_string(cuerpo).with_header(h("Content-Type", tipo))));
        return;
    }
    if let Some(resto) = ruta.strip_prefix("/v/") {
        let id = resto.split('.').next().unwrap_or("");
        let video = {
            let mut e = est.lock().unwrap_or_else(|e| e.into_inner());
            let v = e.videos.get(id).cloned();
            if v.is_some() {
                e.pedidos_video += 1;
            }
            v
        };
        match video {
            Some(v) if v.url.starts_with("http://") || v.url.starts_with("https://") => pasarela(req, &v),
            Some(v) => archivo(req, &v),
            None => {
                let _ = req.respond(Response::empty(404));
            }
        }
        return;
    }
    let _ = req.respond(Response::empty(404));
}

fn cors<R: std::io::Read>(r: Response<R>) -> Response<R> {
    r.with_header(h("Access-Control-Allow-Origin", "*"))
        .with_header(h("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"))
        .with_header(h("Access-Control-Allow-Headers", "*"))
        .with_header(h("Cache-Control", "no-store"))
}

fn cliente() -> &'static reqwest::blocking::Client {
    static C: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // Sin timeout total: una película dura horas.
            .timeout(None)
            .build()
            .expect("cliente HTTP")
    })
}

fn pasarela(req: Request, v: &Video) {
    let rango = req
        .headers()
        .iter()
        .find(|x| x.field.equiv("Range"))
        .map(|x| x.value.as_str().to_string());
    let es_head = *req.method() == Method::Head;
    let mut pedido = if es_head {
        cliente().head(&v.url)
    } else {
        cliente().get(&v.url)
    };
    if let Some(r) = &rango {
        pedido = pedido.header("Range", r);
    }
    let resp = match pedido.send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lan] pasarela: {e}");
            let _ = req.respond(Response::empty(502));
            return;
        }
    };
    let status = resp.status().as_u16();
    let largo = resp.content_length().map(|n| n as usize);
    let mut cabeceras = cabeceras_video(v);
    if let Some(cr) = resp.headers().get("content-range").and_then(|x| x.to_str().ok()) {
        cabeceras.push(h("Content-Range", cr));
    }
    let cuerpo: Box<dyn std::io::Read + Send> = if es_head {
        Box::new(std::io::empty())
    } else {
        Box::new(resp)
    };
    // Si la TV corta (salto, stop) falla la escritura y se suelta la descarga.
    let _ = req.respond(Response::new(StatusCode(status), cabeceras, cuerpo, largo, None));
}

fn cabeceras_video(v: &Video) -> Vec<Header> {
    let mut c = vec![
        h("Content-Type", &v.content_type),
        h("Accept-Ranges", "bytes"),
        h("transferMode.dlna.org", "Streaming"),
        h("contentFeatures.dlna.org", DLNA_FEATURES),
    ];
    if let Some(cap) = &v.caption {
        c.push(h("CaptionInfo.sec", cap));
    }
    c
}

/// "bytes=INI-FIN" | "bytes=INI-" | "bytes=-ULTIMOS" → (ini, fin) inclusivo.
fn rango(r: &str, total: u64) -> Option<(u64, u64)> {
    let (a, b) = r.trim().strip_prefix("bytes=")?.split(',').next()?.split_once('-')?;
    if total == 0 {
        return None;
    }
    let (ini, fin) = match (a.trim(), b.trim()) {
        ("", n) => (total.saturating_sub(n.parse().ok()?), total - 1),
        (i, "") => (i.parse().ok()?, total - 1),
        (i, f) => (i.parse().ok()?, f.parse::<u64>().ok()?.min(total - 1)),
    };
    (ini <= fin && ini < total).then_some((ini, fin))
}

/// Archivo del disco con soporte de Range (la TV salta pidiendo rangos).
fn archivo(req: Request, v: &Video) {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(&v.url) else {
        let _ = req.respond(Response::empty(404));
        return;
    };
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let pedido = req
        .headers()
        .iter()
        .find(|x| x.field.equiv("Range"))
        .and_then(|x| rango(x.value.as_str(), total));
    let mut cab = cabeceras_video(v);
    let (status, ini, largo) = match pedido {
        Some((ini, fin)) => {
            cab.push(h("Content-Range", &format!("bytes {ini}-{fin}/{total}")));
            (206, ini, fin - ini + 1)
        }
        None => (200, 0, total),
    };
    if f.seek(SeekFrom::Start(ini)).is_err() {
        let _ = req.respond(Response::empty(416));
        return;
    }
    let cuerpo: Box<dyn Read + Send> = if *req.method() == Method::Head {
        Box::new(std::io::empty())
    } else {
        Box::new(f.take(largo))
    };
    let _ = req.respond(Response::new(StatusCode(status), cab, cuerpo, Some(largo as usize), None));
}

// ---- Subtítulos --------------------------------------------------------------

pub fn texto_de(b: &[u8]) -> String {
    // Muchos .srt en español vienen en Latin-1/Windows-1252, no en UTF-8.
    let t = match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    };
    t.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

pub fn srt_a_vtt(srt: &str) -> String {
    if srt.trim_start().starts_with("WEBVTT") {
        return srt.to_string();
    }
    let mut out = String::from("WEBVTT\n\n");
    for linea in srt.lines() {
        // Solo la línea de tiempos: "00:01:02,500 --> 00:01:04,000". Una coma
        // en el diálogo se queda como está.
        if linea.contains("-->") {
            out.push_str(&linea.replace(',', "."));
        } else {
            out.push_str(linea);
        }
        out.push('\n');
    }
    out
}

pub fn vtt_a_srt(t: &str) -> String {
    if !t.trim_start().starts_with("WEBVTT") {
        return t.to_string();
    }
    // Bloques de VTT → SRT numerado. La cabecera y los bloques sin tiempos
    // (NOTE, STYLE) se descartan.
    let mut out = String::new();
    let mut n = 0;
    for bloque in t.split("\n\n") {
        let mut lineas = bloque.lines().skip_while(|l| !l.contains("-->"));
        let Some(tiempos) = lineas.next() else { continue };
        n += 1;
        // "00:01.000 --> 00:02.000 align:start" → sin ajustes, con coma y horas.
        let partes: Vec<String> = tiempos
            .split("-->")
            .map(|p| {
                let p = p.split_whitespace().next().unwrap_or("").replace('.', ",");
                if p.matches(':').count() == 1 { format!("00:{p}") } else { p }
            })
            .collect();
        out.push_str(&format!("{n}\n{} --> {}\n", partes[0], partes.get(1).map_or("", |s| s)));
        for l in lineas {
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_a_vtt_solo_toca_los_tiempos() {
        let srt = texto_de("\u{feff}1\r\n00:00:01,500 --> 00:00:03,000\r\nHola, ¿cómo estás?\r\n\r\n".as_bytes());
        let vtt = srt_a_vtt(&srt);
        assert!(vtt.starts_with("WEBVTT\n\n1\n00:00:01.500 --> 00:00:03.000\n"));
        assert!(vtt.contains("Hola, ¿cómo estás?"));
        assert_eq!(srt_a_vtt("WEBVTT\n\nx"), "WEBVTT\n\nx");
        assert_eq!(texto_de(&[0x61, 0xf1, 0x6f]), "año"); // Latin-1
    }

    #[test]
    fn rangos_http() {
        assert_eq!(rango("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(rango("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(rango("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(rango("bytes=500-5000", 1000), Some((500, 999)));
        assert_eq!(rango("bytes=1000-", 1000), None);
        assert_eq!(rango("items=0-1", 1000), None);
    }

    #[test]
    fn archivo_con_rango_por_http() {
        let dir = std::env::temp_dir().join(format!("kutral-lan-{}", marca()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("peli.mkv");
        std::fs::write(&ruta, (0..=255u8).cycle().take(10_000).collect::<Vec<_>>()).unwrap();
        let l = lan().unwrap();
        let v = l
            .publicar_video(ruta.to_str().unwrap(), "video/x-matroska", None, "127.0.0.1", 80)
            .unwrap();
        let r = reqwest::blocking::Client::new()
            .get(&v)
            .header("Range", "bytes=256-511")
            .send()
            .unwrap();
        assert_eq!(r.status().as_u16(), 206);
        assert_eq!(r.headers()["content-range"], "bytes 256-511/10000");
        let b = r.bytes().unwrap();
        assert_eq!(b.len(), 256);
        assert_eq!(b[0], 0);
        assert_eq!(crate::lan::pedidos().0, 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn vtt_a_srt_numera_y_completa_horas() {
        let vtt = "WEBVTT\n\nNOTE algo\n\n00:01.000 --> 00:02.500 align:start\nHola\n\n01:00:00.000 --> 01:00:01.000\nChao\n";
        let srt = vtt_a_srt(vtt);
        assert!(srt.starts_with("1\n00:00:01,000 --> 00:00:02,500\nHola\n\n2\n01:00:00,000 --> 01:00:01,000\nChao\n"));
        assert_eq!(vtt_a_srt("1\n00:00:01,000 --> x\n"), "1\n00:00:01,000 --> x\n");
    }

    /// Pasarela contra una URL real: `KUTRAL_URL=https://… cargo test --lib
    /// lan::tests::pasarela_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn pasarela_real() {
        let url = std::env::var("KUTRAL_URL").expect("KUTRAL_URL");
        let l = lan().unwrap();
        let v = l.publicar_video(&url, "video/mp4", None, "192.168.100.1", 80).unwrap();
        eprintln!("pasarela: {v}");
        let r = reqwest::blocking::Client::new()
            .get(&v)
            .header("Range", "bytes=1000-1999")
            .send()
            .unwrap();
        eprintln!("{} {:?}", r.status(), r.headers());
        assert_eq!(r.status().as_u16(), 206);
        assert!(r.headers()["content-range"].to_str().unwrap().starts_with("bytes 1000-1999/"));
        assert_eq!(r.bytes().unwrap().len(), 1000);
    }
}
