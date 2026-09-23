// Transmitir a la TV (Google Cast / Chromecast integrado en Google TV, LG, etc.).
// Las TVs sin Cast (Samsung) van por DLNA: ver dlna.rs. Los commands de acá
// atienden a las dos y derivan según `CastTv::tipo`.
//
// Kütral hace de EMISOR: le pasa a la TV la URL del video y la TV la baja y la
// reproduce por su cuenta con el Default Media Receiver (app CC1AD845). El PC
// queda como control remoto. Sirve tal cual con URLs directas de RealDebrid:
// no hace falta proxy ni cabeceras (probado 2026-09-22 con H.264, HEVC y EAC3).
//
// Qué NO sirve: lo que solo existe en este equipo (descarga local en
// 127.0.0.1, archivos del disco). La TV no llega a esas direcciones.
//
// Cliente CASTV2 propio en vez de rust_cast: la LG presenta un certificado
// X.509 v1 y rust_cast lo rechaza aun "sin verificación" (valida la firma con
// webpki, que no acepta v1). El protocolo es chico: TLS al puerto 8009 y
// mensajes protobuf de 6 campos que llevan JSON adentro.
//
// Diseño SIN conexión persistente: cada comando abre su conexión, hace lo suyo
// y la cierra. Una conexión viva exige contestar los PING de la TV desde un
// hilo aparte; abrir una por comando cuesta ~200 ms en la LAN y la
// reproducción en la TV no depende de que sigamos conectados.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const APP_DMR: &str = "CC1AD845";
const SERVICIO: &str = "_googlecast._tcp.local.";
const EMISOR: &str = "sender-kutral";
const RECEPTOR: &str = "receiver-0";
const NS_CONEXION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_LATIDO: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEPTOR: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
/// Espera máxima por respuesta. Abrir la app de video en una TV que estaba en
/// otra cosa tarda unos segundos; más que esto es que la TV no está.
const ESPERA_RESPUESTA: Duration = Duration::from_secs(12);

/// TV encontrada en la red, por Google Cast o por DLNA.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CastTv {
    /// Cast: UUID del equipo (TXT `id`). DLNA: UDN. Estable aunque cambie la
    /// IP por DHCP.
    pub id: String,
    pub nombre: String,
    pub modelo: String,
    pub ip: String,
    pub puerto: u16,
    /// "cast" | "dlna". Vacío = "cast" (TVs recordadas antes de DLNA).
    #[serde(default)]
    pub tipo: String,
    /// DLNA: URL de control y tipo de servicio de AVTransport y
    /// RenderingControl, sacados de la descripción del equipo.
    #[serde(default)]
    pub av_url: String,
    #[serde(default)]
    pub av_srv: String,
    #[serde(default)]
    pub rc_url: String,
    #[serde(default)]
    pub rc_srv: String,
}

impl CastTv {
    pub fn es_dlna(&self) -> bool {
        self.tipo == "dlna"
    }
}

/// Lo que se está transmitiendo. Vive acá para que `cast_status` y los
/// controles sepan a qué TV hablarle sin que el frontend la repita.
#[derive(Clone)]
struct Sesion {
    tv: CastTv,
    titulo: String,
    /// URL que se le pasó a la TV (en DLNA, la de la pasarela): si la TV
    /// reporta otra, alguien le mandó otra cosa.
    uri: String,
    inicio: Instant,
    /// DLNA: llegó a PLAYING alguna vez (ver `estado_dlna`).
    vio_play: bool,
    ultima_pos: f64,
    ultima_dur: f64,
    con_subs: bool,
}

fn sesion() -> &'static Mutex<Option<Sesion>> {
    static S: std::sync::OnceLock<Mutex<Option<Sesion>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Serialize, Default)]
pub struct CastStatus {
    /// Hay una transmisión iniciada desde Kütral (aunque la TV ya haya
    /// terminado: eso lo dice `estado`).
    pub activa: bool,
    pub tv: String,
    pub titulo: String,
    /// "PLAYING" | "PAUSED" | "BUFFERING" | "IDLE" | "LOADING" | "SIN_APP"
    /// (alguien cambió de app en la TV) | "SIN_CONEXION".
    pub estado: String,
    /// Por qué quedó IDLE: "FINISHED" | "ERROR" | "CANCELLED" | "INTERRUPTED"
    /// | "SIN_ACCESO" (DLNA: la TV nunca llegó a pedirle el video a Kütral,
    /// casi siempre un firewall; NO es culpa del formato).
    pub motivo: String,
    /// "SUBS_SIN_ACCESO": se mandaron subtítulos y la TV nunca los pidió.
    pub aviso: String,
    pub pos: f64,
    pub duracion: f64,
    pub volumen: f64,
    pub silencio: bool,
}

// ---- Protobuf mínimo --------------------------------------------------------
//
// CastMessage (cast_channel.proto), todos proto2 `required` salvo el payload:
//   1 protocol_version (enum, 0 = CASTV2_1_0)   2 source_id    3 destination_id
//   4 namespace   5 payload_type (enum, 0 = STRING)   6 payload_utf8
// Solo se usan payloads de texto (JSON).

fn varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(b);
            return;
        }
        buf.push(b | 0x80);
    }
}

fn campo_texto(buf: &mut Vec<u8>, n: u64, s: &str) {
    varint(buf, n << 3 | 2);
    varint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

fn codificar(origen: &str, destino: &str, ns: &str, json: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(json.len() + 128);
    varint(&mut b, 1 << 3); // protocol_version = 0
    varint(&mut b, 0);
    campo_texto(&mut b, 2, origen);
    campo_texto(&mut b, 3, destino);
    campo_texto(&mut b, 4, ns);
    varint(&mut b, 5 << 3); // payload_type = STRING
    varint(&mut b, 0);
    campo_texto(&mut b, 6, json);
    b
}

struct Mensaje {
    origen: String,
    ns: String,
    json: Value,
}

fn leer_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for desplazamiento in (0..64).step_by(7) {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << desplazamiento;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn decodificar(b: &[u8]) -> Option<Mensaje> {
    let (mut i, mut origen, mut ns, mut texto) = (0, String::new(), String::new(), String::new());
    while i < b.len() {
        let tag = leer_varint(b, &mut i)?;
        match tag & 7 {
            0 => {
                leer_varint(b, &mut i)?;
            }
            2 => {
                let n = leer_varint(b, &mut i)? as usize;
                let v = b.get(i..i.checked_add(n)?)?;
                i += n;
                let s = || String::from_utf8_lossy(v).into_owned();
                match tag >> 3 {
                    2 => origen = s(),
                    4 => ns = s(),
                    6 => texto = s(),
                    _ => {} // destino y payload binario: no nos interesan
                }
            }
            _ => return None, // tipos de cable que CastMessage no usa
        }
    }
    let json = serde_json::from_str(&texto).unwrap_or(Value::Null);
    Some(Mensaje { origen, ns, json })
}

// ---- Conexión ---------------------------------------------------------------

/// Los equipos Cast presentan un certificado de Google que ninguna CA del
/// sistema valida (y la LG, además, uno X.509 v1 que webpki no sabe leer). La
/// autenticidad real de Cast va por un desafío aparte que ningún emisor de la
/// LAN usa: catt y pychromecast tampoco verifican nada. Es la red de la casa.
#[derive(Debug)]
struct SinVerificar(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for SinVerificar {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn config_tls() -> Result<Arc<rustls::ClientConfig>, String> {
    static CFG: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    if let Some(c) = CFG.get() {
        return Ok(c.clone());
    }
    let prov = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(prov.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SinVerificar(prov)))
        .with_no_client_auth();
    Ok(CFG.get_or_init(|| Arc::new(cfg)).clone())
}

struct Conexion {
    tls: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    pedido: u64,
    /// Transportes (receptor, app) a los que ya mandamos CONNECT.
    conectados: Vec<String>,
}

impl Conexion {
    fn abrir(tv: &CastTv) -> Result<Conexion, String> {
        let ip: std::net::IpAddr = tv.ip.parse().map_err(|_| format!("IP inválida: {}", tv.ip))?;
        let tcp = TcpStream::connect_timeout(&SocketAddr::new(ip, tv.puerto), Duration::from_secs(4))
            .map_err(|e| format!("No se pudo conectar con {}: {e}", tv.nombre))?;
        // Timeout corto por lectura: el bucle de `esperar` reintenta hasta su
        // propio plazo, así un socket muerto nunca cuelga el hilo.
        tcp.set_read_timeout(Some(Duration::from_millis(1500))).ok();
        tcp.set_write_timeout(Some(Duration::from_secs(4))).ok();
        tcp.set_nodelay(true).ok();
        let nombre = rustls::pki_types::ServerName::IpAddress(ip.into());
        let cli = rustls::ClientConnection::new(config_tls()?, nombre)
            .map_err(|e| format!("tls: {e}"))?;
        let mut c = Conexion {
            tls: rustls::StreamOwned::new(cli, tcp),
            pedido: 0,
            conectados: Vec::new(),
        };
        c.conectar(RECEPTOR)?;
        Ok(c)
    }

    fn enviar(&mut self, destino: &str, ns: &str, cuerpo: &Value) -> Result<(), String> {
        let msg = codificar(EMISOR, destino, ns, &cuerpo.to_string());
        let mut marco = (msg.len() as u32).to_be_bytes().to_vec();
        marco.extend_from_slice(&msg);
        self.tls
            .write_all(&marco)
            .and_then(|_| self.tls.flush())
            .map_err(|e| format!("envío a la TV: {e}"))
    }

    fn conectar(&mut self, destino: &str) -> Result<(), String> {
        if self.conectados.iter().any(|d| d == destino) {
            return Ok(());
        }
        self.enviar(destino, NS_CONEXION, &json!({ "type": "CONNECT" }))?;
        self.conectados.push(destino.to_string());
        Ok(())
    }

    /// Un mensaje, o None si venció el timeout de lectura sin datos.
    fn leer(&mut self) -> Result<Option<Mensaje>, String> {
        let mut largo = [0u8; 4];
        match self.tls.read_exact(&mut largo) {
            Ok(()) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Ok(None)
            }
            Err(e) => return Err(format!("lectura de la TV: {e}")),
        }
        let n = u32::from_be_bytes(largo) as usize;
        if n > 1 << 20 {
            return Err("mensaje de la TV demasiado grande".into());
        }
        let mut b = vec![0u8; n];
        self.tls
            .read_exact(&mut b)
            .map_err(|e| format!("lectura de la TV: {e}"))?;
        Ok(decodificar(&b))
    }

    /// Manda un pedido y espera la respuesta con el mismo requestId.
    /// Contesta los PING mientras tanto.
    fn pedir(&mut self, destino: &str, ns: &str, mut cuerpo: Value) -> Result<Value, String> {
        self.pedido += 1;
        let id = self.pedido;
        cuerpo["requestId"] = json!(id);
        self.enviar(destino, ns, &cuerpo)?;
        let fin = Instant::now() + ESPERA_RESPUESTA;
        while Instant::now() < fin {
            let Some(m) = self.leer()? else { continue };
            if m.ns == NS_LATIDO && m.json["type"] == "PING" {
                let origen = m.origen.clone();
                self.enviar(&origen, NS_LATIDO, &json!({ "type": "PONG" }))?;
                continue;
            }
            if m.ns == ns && m.json["requestId"].as_u64() == Some(id) {
                return Ok(m.json);
            }
        }
        Err("La TV no respondió a tiempo".into())
    }

    fn estado_receptor(&mut self) -> Result<Value, String> {
        let r = self.pedir(RECEPTOR, NS_RECEPTOR, json!({ "type": "GET_STATUS" }))?;
        Ok(r["status"].clone())
    }
}

/// La app de video de la TV si ya está abierta: (transportId, sessionId).
fn app_video(receptor: &Value) -> Option<(String, String)> {
    receptor["applications"].as_array()?.iter().find_map(|a| {
        (a["appId"] == APP_DMR).then(|| {
            (
                a["transportId"].as_str().unwrap_or_default().to_string(),
                a["sessionId"].as_str().unwrap_or_default().to_string(),
            )
        })
    })
}

/// Estado del video en la app: la primera entrada de MEDIA_STATUS.
fn medio(c: &mut Conexion, transporte: &str) -> Result<Option<Value>, String> {
    c.conectar(transporte)?;
    let r = c.pedir(transporte, NS_MEDIA, json!({ "type": "GET_STATUS" }))?;
    Ok(r["status"].as_array().and_then(|a| a.first().cloned()))
}

/// Content-type por extensión. El receptor lo usa para elegir demuxer: con
/// uno equivocado un MKV no arranca aunque el códec sea compatible.
fn content_type(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".mkv") {
        "video/x-matroska"
    } else if path.ends_with(".webm") {
        "video/webm"
    } else if path.ends_with(".m3u8") {
        "application/x-mpegURL"
    } else if path.ends_with(".avi") {
        "video/x-msvideo"
    } else {
        "video/mp4"
    }
}

/// ¿La TV puede bajar esta URL directo? Lo local de este equipo (torrent en
/// 127.0.0.1, archivos del disco) no: eso va por la pasarela de lan.rs.
fn url_transmitible(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    !matches!(
        u.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("[::1]") | None
    )
}

async fn bloqueante<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("cast: {e}"))?
}

// ---- Subtítulos -------------------------------------------------------------

/// Baja (o lee del disco) el subtítulo y lo publica en la LAN en VTT (Cast)
/// y SRT (Samsung). Sin subtítulos se transmite igual: mejor la película sin
/// subs que nada, así que el error solo se loguea.
async fn subs_para(origen: Option<&str>, tv: &CastTv) -> Option<(String, String)> {
    const TOPE: usize = 5 << 20;
    let origen = origen.filter(|o| !o.is_empty())?;
    let bytes = if origen.starts_with("http://") || origen.starts_with("https://") {
        match reqwest::get(origen).await {
            Ok(r) => r.bytes().await.ok()?.to_vec(),
            Err(e) => {
                eprintln!("[cast] sin subtítulos: {e}");
                return None;
            }
        }
    } else {
        std::fs::read(origen).ok()?
    };
    if bytes.len() > TOPE {
        return None;
    }
    let lan = crate::lan::lan().ok()?;
    match lan.publicar_subs(&bytes, &tv.ip, tv.puerto) {
        Ok(u) => Some(u),
        Err(e) => {
            eprintln!("[cast] sin subtítulos: {e}");
            None
        }
    }
}

// ---- Commands ---------------------------------------------------------------

/// Busca TVs en la red: Google Cast (mDNS) y DLNA (SSDP) a la vez. Una TV que
/// ofrece las dos (casi todas las LG) aparece una vez, como Cast: reporta
/// errores de formato, deja elegir subtítulos y baja de RD sin pasarela.
#[tauri::command]
pub async fn cast_scan(ms: Option<u64>) -> Result<Vec<CastTv>, String> {
    let espera = Duration::from_millis(ms.unwrap_or(3000).clamp(1000, 10000));
    let (cast, dlna) = tokio::join!(buscar_cast(espera), crate::dlna::buscar(espera));
    let mut tvs = cast?;
    for d in dlna {
        if !tvs.iter().any(|t| t.ip == d.ip) {
            tvs.push(d);
        }
    }
    tvs.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Ok(tvs)
}

async fn buscar_cast(espera: Duration) -> Result<Vec<CastTv>, String> {
    bloqueante(move || {
        use mdns_sd::{ServiceDaemon, ServiceEvent};
        let mdns = ServiceDaemon::new().map_err(|e| format!("mDNS: {e}"))?;
        let rx = mdns.browse(SERVICIO).map_err(|e| format!("mDNS: {e}"))?;
        let fin = Instant::now() + espera;
        let mut tvs: Vec<CastTv> = Vec::new();
        while let Some(resta) = fin.checked_duration_since(Instant::now()) {
            let Ok(ev) = rx.recv_timeout(resta) else { break };
            let ServiceEvent::ServiceResolved(info) = ev else { continue };
            let Some(ip) = info.get_addresses_v4().into_iter().next() else {
                continue;
            };
            let prop = |k: &str| info.get_property_val_str(k).unwrap_or_default().to_string();
            let id = prop("id");
            let nombre = match prop("fn") {
                n if n.is_empty() => info.host.trim_end_matches('.').to_string(),
                n => n,
            };
            let tv = CastTv {
                id: if id.is_empty() { info.fullname.clone() } else { id },
                nombre,
                modelo: prop("md"),
                ip: ip.to_string(),
                puerto: info.port,
                tipo: "cast".into(),
                ..Default::default()
            };
            if !tvs.iter().any(|t| t.id == tv.id) {
                eprintln!("[cast] encontrada {} ({}) en {}:{}", tv.nombre, tv.modelo, tv.ip, tv.puerto);
                tvs.push(tv);
            }
        }
        let _ = mdns.shutdown();
        Ok(tvs)
    })
    .await
}

/// ¿Sigue la TV en esa IP? Sirve para validar la TV recordada antes de
/// escanear toda la red.
#[tauri::command]
pub async fn cast_ping(tv: CastTv) -> Result<bool, String> {
    if tv.es_dlna() {
        return Ok(crate::dlna::vive(&tv).await);
    }
    let nombre = tv.nombre.clone();
    let r = bloqueante(move || Conexion::abrir(&tv)?.estado_receptor()).await;
    if let Err(e) = &r {
        eprintln!("[cast] {nombre} no responde: {e}");
    }
    Ok(r.is_ok())
}

/// Manda un video a la TV. Devuelve cuando la TV aceptó el video; si el
/// formato no le sirve lo dice después, en `cast_status` (IDLE + ERROR).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn cast_play(
    tv: CastTv,
    url: String,
    titulo: String,
    subtitulo: Option<String>,
    imagen: Option<String>,
    desde: Option<f64>,
    subs: Option<String>,
    subs_idioma: Option<String>,
    archivo: Option<String>,
) -> Result<(), String> {
    let pista = subs_para(subs.as_deref(), &tv).await;
    let con_subs = pista.is_some();
    let desde = desde.unwrap_or(0.0).max(0.0);
    // El stream del torrent local (127.0.0.1:…/stream/0) no dice la extensión:
    // el frontend pasa el nombre del archivo para deducir el formato.
    let ct = content_type(archivo.as_deref().filter(|a| !a.is_empty()).unwrap_or(&url));
    let srt = pista.as_ref().map(|(_, s)| s.clone());
    // DLNA siempre por la pasarela (HTTPS, cabeceras DLNA, subs Samsung). Cast
    // baja directo de RD; solo lo local de este equipo pasa por la pasarela.
    let uri = if tv.es_dlna() || !url_transmitible(&url) {
        crate::lan::lan()?.publicar_video(&url, ct, srt.clone(), &tv.ip, tv.puerto)?
    } else {
        url.clone()
    };

    if tv.es_dlna() {
        crate::dlna::reproducir(&tv, &uri, ct, &titulo, srt.as_deref(), desde).await?;
    } else {
        let idioma = subs_idioma.unwrap_or_else(|| "es".into());
        let vtt = pista.map(|(v, _)| v);
        let (tv2, titulo2, uri2) = (tv.clone(), titulo.clone(), uri.clone());
        bloqueante(move || cargar_cast(&tv2, &uri2, &titulo2, subtitulo, imagen, desde, vtt, &idioma))
            .await?;
    }
    eprintln!("[cast] transmitiendo \"{}\" en {} ({})", titulo, tv.nombre, tv.tipo);
    *sesion().lock().unwrap() = Some(Sesion {
        tv,
        titulo,
        uri,
        inicio: Instant::now(),
        vio_play: false,
        ultima_pos: 0.0,
        ultima_dur: 0.0,
        con_subs,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cargar_cast(
    tv: &CastTv,
    url: &str,
    titulo: &str,
    subtitulo: Option<String>,
    imagen: Option<String>,
    desde: f64,
    vtt: Option<String>,
    idioma: &str,
) -> Result<(), String> {
    let mut c = Conexion::abrir(tv)?;
    let receptor = c.estado_receptor()?;
    let (transporte, sesion_app) = match app_video(&receptor) {
        Some(a) => a,
        None => {
            let r = c.pedir(RECEPTOR, NS_RECEPTOR, json!({ "type": "LAUNCH", "appId": APP_DMR }))?;
            if r["type"] == "LAUNCH_ERROR" {
                return Err(format!(
                    "La TV no abrió su reproductor: {}",
                    r["reason"].as_str().unwrap_or("sin motivo")
                ));
            }
            app_video(&r["status"]).ok_or("La TV no abrió su reproductor")?
        }
    };
    c.conectar(&transporte)?;
    let mut meta = json!({ "metadataType": 1, "title": titulo });
    if let Some(s) = subtitulo {
        meta["subtitle"] = json!(s);
    }
    if let Some(i) = imagen {
        meta["images"] = json!([{ "url": i }]);
    }
    let mut load = json!({
        "type": "LOAD",
        "sessionId": sesion_app,
        "autoplay": true,
        "currentTime": desde,
        "media": {
            "contentId": url,
            "streamType": "BUFFERED",
            "contentType": content_type(url),
            "metadata": meta,
        },
    });
    if let Some(sub) = vtt {
        load["media"]["tracks"] = json!([{
            "trackId": 1,
            "type": "TEXT",
            "subtype": "SUBTITLES",
            "trackContentId": sub,
            "trackContentType": "text/vtt",
            "name": "Subtítulos",
            "language": idioma,
        }]);
        // Letra blanca con borde, sin la caja negra por defecto del receptor.
        load["media"]["textTrackStyle"] = json!({
            "backgroundColor": "#00000000",
            "foregroundColor": "#FFFFFFFF",
            "edgeType": "OUTLINE",
            "edgeColor": "#000000FF",
            "fontScale": 1.1,
        });
        load["activeTrackIds"] = json!([1]);
    }
    let r = c.pedir(&transporte, NS_MEDIA, load)?;
    match r["type"].as_str() {
        Some("MEDIA_STATUS") => Ok(()),
        otro => Err(format!(
            "LOAD_FAILED: la TV rechazó el video ({})",
            otro.unwrap_or("sin respuesta")
        )),
    }
}

#[tauri::command]
pub async fn cast_status() -> Result<CastStatus, String> {
    let Some(s) = sesion().lock().unwrap().clone() else {
        return Ok(CastStatus::default());
    };
    let base = CastStatus {
        activa: true,
        tv: s.tv.nombre.clone(),
        titulo: s.titulo.clone(),
        ..Default::default()
    };
    let (con_subs, inicio) = (s.con_subs, s.inicio);
    let mut st = if s.tv.es_dlna() {
        estado_dlna(&s, base).await
    } else {
        estado_cast(s, base).await
    };
    // Cast pide el VTT al empezar a reproducir: si pasó un rato sonando y no
    // llegó ningún pedido, la TV no alcanza a Kütral.
    if con_subs
        && st.estado == "PLAYING"
        && inicio.elapsed() > Duration::from_secs(20)
        && crate::lan::pedidos().1 == 0
    {
        st.aviso = "SUBS_SIN_ACCESO".into();
    }
    Ok(st)
}

async fn estado_cast(s: Sesion, base: CastStatus) -> CastStatus {
    let r = bloqueante(move || {
        let mut c = Conexion::abrir(&s.tv)?;
        let receptor = c.estado_receptor()?;
        let vol = &receptor["volume"];
        let vol = (vol["level"].as_f64().unwrap_or(0.0), vol["muted"].as_bool().unwrap_or(false));
        let Some((transporte, _)) = app_video(&receptor) else {
            return Ok((None, vol, false));
        };
        Ok((medio(&mut c, &transporte)?, vol, true))
    })
    .await;
    let st = match r {
        Err(_) => CastStatus {
            estado: "SIN_CONEXION".into(),
            ..base
        },
        Ok((_, (volumen, silencio), false)) => CastStatus {
            estado: "SIN_APP".into(),
            volumen,
            silencio,
            ..base
        },
        Ok((None, (volumen, silencio), true)) => CastStatus {
            estado: "IDLE".into(),
            volumen,
            silencio,
            ..base
        },
        Ok((Some(m), (volumen, silencio), true)) => {
            // Mientras carga, el receptor informa IDLE + extendedStatus LOADING.
            let cargando = m["extendedStatus"]["playerState"] == "LOADING";
            CastStatus {
                estado: if cargando {
                    "LOADING".into()
                } else {
                    m["playerState"].as_str().unwrap_or("IDLE").to_string()
                },
                motivo: m["idleReason"].as_str().unwrap_or_default().to_string(),
                pos: m["currentTime"].as_f64().unwrap_or(0.0),
                duracion: m["media"]["duration"].as_f64().unwrap_or(0.0),
                volumen,
                silencio,
                ..base
            }
        }
    };
    st
}

/// DLNA no tiene "motivo" de parada: se deduce. Si nunca llegó a PLAYING y
/// quedó en STOPPED, la TV no pudo con el archivo (ERROR); si ya había
/// reproducido, terminó (cerca del final) o la pararon.
async fn estado_dlna(s: &Sesion, base: CastStatus) -> CastStatus {
    let Ok((e, pos, dur, uri)) = crate::dlna::estado(&s.tv).await else {
        return CastStatus {
            estado: "SIN_CONEXION".into(),
            ..base
        };
    };
    // Alguien mandó otra cosa a la TV (otra app, otro celular).
    if !uri.is_empty() && uri != s.uri {
        return CastStatus {
            estado: "SIN_APP".into(),
            ..base
        };
    }
    let vio_play = s.vio_play || e == "PLAYING";
    {
        let mut g = sesion().lock().unwrap();
        if let Some(act) = g.as_mut().filter(|a| a.uri == s.uri) {
            act.vio_play = vio_play;
            if dur > 0.0 {
                act.ultima_dur = dur;
            }
            if pos > 0.0 {
                act.ultima_pos = pos;
            }
        }
    }
    // La TV nunca vino a buscar el video: no es el formato, es la red.
    let sin_pedidos = crate::lan::pedidos().0 == 0;
    let falla = if sin_pedidos { "SIN_ACCESO" } else { "ERROR" };
    let (estado, motivo) = match e.as_str() {
        "PLAYING" => ("PLAYING", ""),
        "PAUSED_PLAYBACK" | "PAUSED_RECORDING" => ("PAUSED", ""),
        _ if !vio_play && sin_pedidos && s.inicio.elapsed() > Duration::from_secs(15) => {
            ("IDLE", "SIN_ACCESO")
        }
        "TRANSITIONING" => (if vio_play { "BUFFERING" } else { "LOADING" }, ""),
        _ if !vio_play && s.inicio.elapsed() < Duration::from_secs(8) => ("LOADING", ""),
        _ if !vio_play => ("IDLE", falla),
        _ if s.ultima_dur > 0.0 && s.ultima_pos >= s.ultima_dur - 90.0 => ("IDLE", "FINISHED"),
        _ => ("IDLE", "CANCELLED"),
    };
    CastStatus {
        estado: estado.into(),
        motivo: motivo.into(),
        pos: if pos > 0.0 { pos } else { s.ultima_pos },
        duracion: if dur > 0.0 { dur } else { s.ultima_dur },
        ..base
    }
}

/// Controles: "pausa" | "seguir" | "saltar" (valor = segundo absoluto) |
/// "volumen" (valor 0..1) | "silencio" (valor 1/0) | "detener".
#[tauri::command]
pub async fn cast_control(accion: String, valor: Option<f64>) -> Result<(), String> {
    let Some(s) = sesion().lock().unwrap().clone() else {
        return Err("No hay nada transmitiéndose".into());
    };
    let detener = accion == "detener";
    let r = if s.tv.es_dlna() {
        let tv = &s.tv;
        match accion.as_str() {
            "pausa" => crate::dlna::pausa(tv).await,
            "seguir" => crate::dlna::seguir(tv).await,
            "saltar" => crate::dlna::saltar(tv, valor.unwrap_or(0.0)).await,
            "volumen" => crate::dlna::volumen(tv, valor.unwrap_or(0.5)).await,
            "silencio" => crate::dlna::silencio(tv, valor.unwrap_or(1.0) > 0.5).await,
            "detener" => crate::dlna::detener(tv).await,
            otra => Err(format!("acción desconocida: {otra}")),
        }
    } else {
        bloqueante(move || control_cast(&s.tv, &accion, valor)).await
    };
    if detener {
        // Aunque la TV no conteste, del lado de Kütral la transmisión terminó.
        *sesion().lock().unwrap() = None;
    }
    r
}

fn control_cast(tv: &CastTv, accion: &str, valor: Option<f64>) -> Result<(), String> {
    let mut c = Conexion::abrir(tv)?;
    match accion {
        "volumen" => {
            let v = valor.unwrap_or(0.5).clamp(0.0, 1.0);
            c.pedir(RECEPTOR, NS_RECEPTOR, json!({ "type": "SET_VOLUME", "volume": { "level": v } }))?;
            return Ok(());
        }
        "silencio" => {
            let m = valor.unwrap_or(1.0) > 0.5;
            c.pedir(RECEPTOR, NS_RECEPTOR, json!({ "type": "SET_VOLUME", "volume": { "muted": m } }))?;
            return Ok(());
        }
        _ => {}
    }
    let receptor = c.estado_receptor()?;
    let Some((transporte, sesion_app)) = app_video(&receptor) else {
        return Ok(()); // ya no hay video en la TV: nada que controlar
    };
    if accion == "detener" {
        // Cerrar la app devuelve la TV a lo que estaba (pantalla de inicio
        // o la entrada anterior), no deja un reproductor negro abierto.
        c.pedir(RECEPTOR, NS_RECEPTOR, json!({ "type": "STOP", "sessionId": sesion_app }))?;
        return Ok(());
    }
    let Some(m) = medio(&mut c, &transporte)? else {
        return Ok(());
    };
    let id = m["mediaSessionId"].clone();
    let cuerpo = match accion {
        "pausa" => json!({ "type": "PAUSE", "mediaSessionId": id }),
        "seguir" => json!({ "type": "PLAY", "mediaSessionId": id }),
        "saltar" => json!({
            "type": "SEEK",
            "mediaSessionId": id,
            "currentTime": valor.unwrap_or(0.0).max(0.0),
        }),
        otra => return Err(format!("acción desconocida: {otra}")),
    };
    c.pedir(&transporte, NS_MEDIA, cuerpo)?;
    Ok(())
}

/// Olvida la sesión sin tocar la TV (terminó sola o alguien cambió de app).
#[tauri::command]
pub fn cast_soltar() {
    *sesion().lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_por_extension() {
        assert_eq!(content_type("https://x/d/AB/Peli.2026.mkv"), "video/x-matroska");
        assert_eq!(content_type("https://x/d/AB/Peli%20(2026).MP4?t=1"), "video/mp4");
        assert_eq!(content_type("https://x/live/index.m3u8"), "application/x-mpegURL");
        assert_eq!(content_type("https://x/d/AB/sin-extension"), "video/mp4");
    }

    #[test]
    fn solo_urls_que_la_tv_alcanza() {
        assert!(url_transmitible("https://scl1-4.download.real-debrid.com/d/X/a.mkv"));
        assert!(url_transmitible("http://192.168.1.5:8080/a.mp4"));
        assert!(!url_transmitible("http://127.0.0.1:41234/stream/0"));
        assert!(!url_transmitible("http://localhost:9000/a.mkv"));
        assert!(!url_transmitible("/home/nick/Descargas/a.mkv"));
        assert!(!url_transmitible("file:///home/nick/a.mkv"));
    }

    #[test]
    fn protobuf_ida_y_vuelta() {
        let json = r#"{"type":"PING","requestId":300}"#;
        let b = codificar("sender-0", "receiver-0", NS_LATIDO, json);
        let m = decodificar(&b).unwrap();
        assert_eq!(m.origen, "sender-0");
        assert_eq!(m.ns, NS_LATIDO);
        assert_eq!(m.json["requestId"], 300);
        // Payload > 127 bytes: el largo ocupa dos bytes de varint.
        let largo = format!(r#"{{"x":"{}"}}"#, "a".repeat(300));
        let m = decodificar(&codificar("a", "b", NS_MEDIA, &largo)).unwrap();
        assert_eq!(m.json["x"].as_str().unwrap().len(), 300);
    }

    /// Red real, sin tocar la pantalla: busca TVs y lee su estado.
    /// `cargo test --lib cast::tests::red_real -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn red_real() {
        let tvs = cast_scan(Some(3000)).await.unwrap();
        assert!(!tvs.is_empty(), "no hay TVs Cast en la red");
        for tv in tvs {
            let vive = cast_ping(tv.clone()).await.unwrap();
            eprintln!("{tv:?} ping={vive}");
            assert!(vive);
        }
    }
}
