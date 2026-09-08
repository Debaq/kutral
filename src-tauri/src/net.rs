// net.rs — cliente HTTP resistente a bloqueos de operador.
//
// POR QUÉ EXISTE: España (orden judicial LaLiga/Movistar), Italia (Piracy
// Shield), Portugal y Argentina bloquean sitios, y el operador casi siempre lo
// aplica en el DNS. Un resolver DoH (DNS sobre HTTPS) saltea eso: la consulta
// viaja cifrada dentro de HTTPS y el ISP no la puede reescribir.
//
// El bootstrap va por IP LITERAL (1.1.1.1 / 8.8.8.8) a propósito: sus
// certificados traen la IP como SAN, así que el TLS valida sin resolver ningún
// nombre. Si hubiera que resolver "cloudflare-dns.com" primero, el bloqueo DNS
// nos ganaría antes de empezar.
//
// Segunda capa: cadena de dominios espejo por sitio (`get_first`). Si un
// dominio cae, se prueba el siguiente.
//
// LO QUE NO CUBRE: bloqueo por rango IP (España tumba rangos enteros de
// Cloudflare los fines de semana). Ahí la única salida es degradar a la ruta
// torrent, que va por debrid y es inmune. Por eso todo acá falla EN SILENCIO
// y nunca es la única vía de reproducción.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Resolvers DoH por IP literal (sin dependencia de DNS para arrancar).
/// Cloudflare usa `?name=`, Google el alias `/resolve` con el mismo formato.
const DOH_ENDPOINTS: &[&str] = &["https://1.1.1.1/dns-query", "https://8.8.8.8/resolve"];
const DOH_TIMEOUT_S: u64 = 6;
/// Piso y techo del TTL que devuelve el DoH. Los CDN de streaming rotan IP con
/// TTL de 5s: cachear 5 minutos nos dejaría pegados a un nodo muerto.
const TTL_MIN_S: u64 = 30;
const TTL_MAX_S: u64 = 900;
const SCRAPE_TIMEOUT_S: u64 = 12;

/// UA de navegador real: varios sitios de anime devuelven 403 a UA de bot.
pub const SCRAPE_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

struct Cached {
    ips: Vec<IpAddr>,
    until: Instant,
}

/// Resolver DoH con caché. Cae al resolver del sistema si el DoH no responde
/// (red sin salida a 1.1.1.1, portal cautivo, etc.).
pub struct DohResolver {
    boot: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, Cached>>>,
}

impl DohResolver {
    fn new() -> Result<Self, String> {
        // Cliente de arranque SIN resolver custom: solo habla con IPs literales.
        let boot = reqwest::Client::builder()
            .timeout(Duration::from_secs(DOH_TIMEOUT_S))
            .build()
            .map_err(|e| format!("doh boot client: {e}"))?;
        Ok(Self { boot, cache: Arc::new(Mutex::new(HashMap::new())) })
    }
}

/// Consulta A a un endpoint DoH-JSON. Devuelve (ips, ttl_segundos).
async fn doh_query(cli: &reqwest::Client, ep: &str, host: &str) -> Option<(Vec<IpAddr>, u64)> {
    let url = format!("{ep}?name={}&type=A", urlencoding::encode(host));
    let r = cli.get(&url).header("accept", "application/dns-json").send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: serde_json::Value = r.json().await.ok()?;
    let answers = v.get("Answer")?.as_array()?;
    let mut ips = Vec::new();
    let mut ttl = TTL_MAX_S;
    for a in answers {
        // type 1 = A. Los CNAME intermedios (type 5) se ignoran: el DoH ya
        // resolvió la cadena y los A finales vienen en la misma respuesta.
        if a.get("type").and_then(|t| t.as_u64()) != Some(1) {
            continue;
        }
        if let Some(ip) = a.get("data").and_then(|d| d.as_str()).and_then(|s| s.parse().ok()) {
            ips.push(ip);
            if let Some(t) = a.get("TTL").and_then(|t| t.as_u64()) {
                ttl = ttl.min(t);
            }
        }
    }
    if ips.is_empty() { None } else { Some((ips, ttl.clamp(TTL_MIN_S, TTL_MAX_S))) }
}

impl Resolve for DohResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_ascii_lowercase();
        let cli = self.boot.clone();
        let cache = self.cache.clone();
        Box::pin(async move {
            if let Some(ips) = cache
                .lock()
                .ok()
                .and_then(|c| c.get(&host).filter(|e| e.until > Instant::now()).map(|e| e.ips.clone()))
            {
                return Ok(to_addrs(ips));
            }
            for ep in DOH_ENDPOINTS {
                if let Some((ips, ttl)) = doh_query(&cli, ep, &host).await {
                    if let Ok(mut c) = cache.lock() {
                        c.insert(
                            host.clone(),
                            Cached { ips: ips.clone(), until: Instant::now() + Duration::from_secs(ttl) },
                        );
                    }
                    return Ok(to_addrs(ips));
                }
            }
            // Respaldo: resolver del sistema. Si el bloqueo era por DNS esto
            // también falla, pero cubre el caso "DoH inalcanzable, DNS sano".
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .collect();
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// El puerto real lo fija reqwest después (ver dns::resolve::http_resolve):
/// un 0 acá significa "usa el del esquema".
fn to_addrs(ips: Vec<IpAddr>) -> Addrs {
    Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)))
}

/// Cliente compartido de scraping. Único para toda la app: así la caché DNS y
/// el pool de conexiones se reaprovechan entre sitios.
pub fn scrape_client() -> Result<reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let resolver = Arc::new(DohResolver::new()?);
            reqwest::Client::builder()
                .user_agent(SCRAPE_UA)
                .timeout(Duration::from_secs(SCRAPE_TIMEOUT_S))
                .dns_resolver(resolver)
                .build()
                .map_err(|e| format!("scrape client: {e}"))
        })
        .clone()
}

/// GET de texto con UA de navegador y Referer opcional (varios reproductores
/// embebidos exigen el Referer del sitio que los incrusta).
pub async fn get_text(url: &str, referer: Option<&str>) -> Result<String, String> {
    let cli = scrape_client()?;
    let mut req = cli.get(url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let resp = req.send().await.map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GET {url}: {}", resp.status()));
    }
    resp.text().await.map_err(|e| format!("body {url}: {e}"))
}

/// Prueba `path` contra cada dominio espejo hasta que uno responda. Devuelve
/// (base_que_funcionó, cuerpo). Un dominio bloqueado por IP no se puede
/// saltear con DoH: para eso está la cadena.
pub async fn get_first(mirrors: &[&str], path: &str) -> Result<(String, String), String> {
    let mut last = String::from("sin espejos");
    for base in mirrors {
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        match get_text(&url, Some(base)).await {
            Ok(body) => return Ok(((*base).to_string(), body)),
            Err(e) => {
                eprintln!("[net] espejo caído {base}: {e}");
                last = e;
            }
        }
    }
    Err(last)
}
