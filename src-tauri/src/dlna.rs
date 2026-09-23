// Transmitir por DLNA / UPnP (MediaRenderer). Es lo que traen las Samsung
// (Tizen no tiene Google Cast) y casi todas las LG, Sony, Philips, etc.
//
//   Descubrir: SSDP (M-SEARCH multicast a 239.255.255.250:1900) → cada TV
//              contesta con la URL de su descripción XML → de ahí salen el
//              nombre y las URLs de control de AVTransport y RenderingControl.
//   Controlar: SOAP por HTTP. SetAVTransportURI + Play, Pause, Seek, Stop,
//              GetTransportInfo / GetPositionInfo para el estado.
//
// A diferencia de Cast, la TV no baja de RD directo: muchas no hablan HTTPS,
// así que el video pasa por la pasarela de lan.rs. Los subtítulos van como lo
// entiende Samsung: `sec:CaptionInfoEx` en los metadatos DIDL y la cabecera
// `CaptionInfo.sec` en la respuesta del video.
//
// DLNA no avisa "formato no soportado": la TV simplemente vuelve a STOPPED.
// Por eso cast.rs mira si alguna vez llegó a PLAYING para distinguir una
// falla de un final.

use crate::cast::CastTv;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

const SSDP: &str = "239.255.255.250:1900";
const ST_RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

fn cliente() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .unwrap_or_default()
}

// ---- XML mínimo ----------------------------------------------------------------
//
// Las descripciones y respuestas SOAP son planas y predecibles: basta con
// buscar etiquetas por nombre local (ignorando el prefijo de namespace).

fn etiqueta<'a>(xml: &'a str, nombre: &str) -> Option<&'a str> {
    let mut desde = 0;
    while let Some(i) = xml[desde..].find('<') {
        let ini = desde + i + 1;
        let fin_tag = ini + xml[ini..].find('>')?;
        let tag = &xml[ini..fin_tag];
        desde = fin_tag;
        if tag.starts_with('/') || tag.ends_with('/') {
            continue;
        }
        let local = tag.split_whitespace().next()?.rsplit(':').next()?;
        if local == nombre {
            let resto = &xml[fin_tag + 1..];
            let cierre = resto.find("</")?;
            return Some(resto[..cierre].trim());
        }
    }
    None
}

fn des_escapar(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escapar(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// URL absoluta de un controlURL que puede venir relativo.
fn absoluta(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    url::Url::parse(base)
        .and_then(|b| b.join(rel))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| rel.to_string())
}

/// (controlURL, serviceType) del primer servicio cuyo tipo contiene `clave`.
fn servicio(desc: &str, base: &str, clave: &str) -> Option<(String, String)> {
    desc.split("<service>").skip(1).find_map(|bloque| {
        let tipo = etiqueta(bloque, "serviceType")?;
        if !tipo.contains(clave) {
            return None;
        }
        let ctrl = etiqueta(bloque, "controlURL")?;
        Some((absoluta(base, ctrl), tipo.to_string()))
    })
}

// ---- Descubrir -----------------------------------------------------------------

fn ssdp_ubicaciones(espera: Duration) -> Vec<String> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return vec![];
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
    let msg = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {ST_RENDERER}\r\n\r\n"
    );
    let mut ubicaciones: Vec<String> = Vec::new();
    let fin = Instant::now() + espera;
    let mut envios = 0;
    let mut buf = [0u8; 4096];
    // Con un firewall (firewalld) las respuestas al multicast se descartan:
    // vienen de la IP de la TV y el pedido salió a 239.255.255.250, así que
    // conntrack no las reconoce. Preguntar además a cada IP de la subred por
    // unicast sí pasa: la respuesta vuelve de la misma IP:puerto preguntada.
    // UPnP 1.1 obliga a contestar la búsqueda unicast. Son 254 paquetes chicos.
    let vecinos = subred_propia();
    while Instant::now() < fin {
        // UDP se pierde: el pedido va tres veces, espaciado.
        if envios < 3 {
            let _ = sock.send_to(msg.as_bytes(), SSDP);
            for ip in &vecinos {
                let _ = sock.send_to(msg.as_bytes(), (*ip, 1900));
            }
            envios += 1;
        }
        let Ok((n, _)) = sock.recv_from(&mut buf) else { continue };
        let resp = String::from_utf8_lossy(&buf[..n]);
        let loc = resp.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("location").then(|| v.trim().to_string())
        });
        if let Some(loc) = loc {
            if !ubicaciones.contains(&loc) {
                ubicaciones.push(loc);
            }
        }
    }
    ubicaciones
}

/// Las otras 253 IPs de nuestra /24 (la red de una casa).
fn subred_propia() -> Vec<std::net::Ipv4Addr> {
    let Ok(std::net::IpAddr::V4(yo)) = crate::lan::ip_hacia("239.255.255.250", 1900) else {
        return vec![];
    };
    let [a, b, c, propio] = yo.octets();
    (1..=254u8)
        .filter(|&d| d != propio)
        .map(|d| std::net::Ipv4Addr::new(a, b, c, d))
        .collect()
}

async fn describir(loc: &str) -> Option<CastTv> {
    let desc = cliente().get(loc).send().await.ok()?.text().await.ok()?;
    let base = etiqueta(&desc, "URLBase").map(str::to_string).unwrap_or_else(|| loc.to_string());
    let (av_url, av_srv) = servicio(&desc, &base, "AVTransport")?;
    let (rc_url, rc_srv) = servicio(&desc, &base, "RenderingControl").unwrap_or_default();
    let ip = url::Url::parse(loc).ok()?.host_str()?.to_string();
    let puerto = url::Url::parse(loc).ok()?.port_or_known_default().unwrap_or(80);
    let nombre = des_escapar(etiqueta(&desc, "friendlyName").unwrap_or("TV"));
    let fabricante = etiqueta(&desc, "manufacturer").unwrap_or_default();
    let modelo = etiqueta(&desc, "modelName").unwrap_or_default();
    Some(CastTv {
        id: etiqueta(&desc, "UDN").unwrap_or(loc).to_string(),
        nombre,
        modelo: format!("{fabricante} {modelo}").trim().to_string(),
        ip,
        puerto,
        tipo: "dlna".into(),
        av_url,
        av_srv,
        rc_url,
        rc_srv,
    })
}

pub async fn buscar(espera: Duration) -> Vec<CastTv> {
    let ubicaciones = tokio::task::spawn_blocking(move || ssdp_ubicaciones(espera))
        .await
        .unwrap_or_default();
    let mut tvs: Vec<CastTv> = Vec::new();
    for loc in ubicaciones {
        if let Some(tv) = describir(&loc).await {
            if !tvs.iter().any(|t| t.id == tv.id) {
                eprintln!("[dlna] encontrada {} ({}) en {}", tv.nombre, tv.modelo, tv.ip);
                tvs.push(tv);
            }
        }
    }
    tvs
}

// ---- Controlar -----------------------------------------------------------------

async fn soap(url: &str, srv: &str, accion: &str, args: &str) -> Result<String, String> {
    if url.is_empty() {
        return Err(format!("La TV no ofrece {accion}"));
    }
    let cuerpo = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{accion} xmlns:u="{srv}">{args}</u:{accion}></s:Body></s:Envelope>"#
    );
    let r = cliente()
        .post(url)
        .header("Content-Type", r#"text/xml; charset="utf-8""#)
        .header("SOAPACTION", format!("\"{srv}#{accion}\""))
        .body(cuerpo)
        .send()
        .await
        .map_err(|e| format!("La TV no respondió: {e}"))?;
    let ok = r.status().is_success();
    let texto = r.text().await.unwrap_or_default();
    if !ok {
        let det = etiqueta(&texto, "errorDescription").unwrap_or("error UPnP");
        return Err(format!("{accion}: {det}"));
    }
    Ok(texto)
}

async fn av(tv: &CastTv, accion: &str, args: &str) -> Result<String, String> {
    soap(&tv.av_url, &tv.av_srv, accion, &format!("<InstanceID>0</InstanceID>{args}")).await
}

fn hms(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn segundos(hms: &str) -> f64 {
    // "1:02:03", "01:02:03.500"; "NOT_IMPLEMENTED" → 0.
    hms.split(':')
        .map(|p| p.parse::<f64>().unwrap_or(0.0))
        .fold(0.0, |acc, p| acc * 60.0 + p)
}

fn didl(titulo: &str, url: &str, content_type: &str, srt: Option<&str>) -> String {
    let sub = srt
        .map(|s| {
            format!(
                r#"<sec:CaptionInfoEx sec:type="srt">{u}</sec:CaptionInfoEx><sec:CaptionInfo sec:type="srt">{u}</sec:CaptionInfo>"#,
                u = escapar(s)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:sec="http://www.sec.co.kr/"><item id="0" parentID="-1" restricted="1"><dc:title>{t}</dc:title><upnp:class>object.item.videoItem.movie</upnp:class>{sub}<res protocolInfo="http-get:*:{ct}:{f}">{u}</res></item></DIDL-Lite>"#,
        t = escapar(titulo),
        ct = content_type,
        f = crate::lan::DLNA_FEATURES,
        u = escapar(url),
    )
}

/// Carga y arranca. `url` ya es la de la pasarela (HTTP en la LAN).
pub async fn reproducir(
    tv: &CastTv,
    url: &str,
    content_type: &str,
    titulo: &str,
    srt: Option<&str>,
    desde: f64,
) -> Result<(), String> {
    // Varias TVs se niegan a cambiar de URI si hay algo cargado.
    let _ = av(tv, "Stop", "").await;
    let meta = escapar(&didl(titulo, url, content_type, srt));
    av(
        tv,
        "SetAVTransportURI",
        &format!("<CurrentURI>{}</CurrentURI><CurrentURIMetaData>{meta}</CurrentURIMetaData>", escapar(url)),
    )
    .await?;
    av(tv, "Play", "<Speed>1</Speed>").await?;
    if desde > 1.0 {
        // Seek recién cuando la TV ya está reproduciendo: antes lo ignoran.
        let tv = tv.clone();
        tokio::spawn(async move {
            for _ in 0..40 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if matches!(estado(&tv).await, Ok((ref e, _, _, _)) if e == "PLAYING") {
                    let _ = saltar(&tv, desde).await;
                    return;
                }
            }
        });
    }
    Ok(())
}

/// (estado de transporte, posición, duración, URI cargada).
pub async fn estado(tv: &CastTv) -> Result<(String, f64, f64, String), String> {
    let t = av(tv, "GetTransportInfo", "").await?;
    let e = etiqueta(&t, "CurrentTransportState").unwrap_or("STOPPED").to_string();
    let p = av(tv, "GetPositionInfo", "").await.unwrap_or_default();
    let pos = segundos(etiqueta(&p, "RelTime").unwrap_or("0"));
    let dur = segundos(etiqueta(&p, "TrackDuration").unwrap_or("0"));
    let uri = des_escapar(etiqueta(&p, "TrackURI").unwrap_or_default());
    Ok((e, pos, dur, uri))
}

pub async fn pausa(tv: &CastTv) -> Result<(), String> {
    av(tv, "Pause", "").await.map(|_| ())
}
pub async fn seguir(tv: &CastTv) -> Result<(), String> {
    av(tv, "Play", "<Speed>1</Speed>").await.map(|_| ())
}
pub async fn detener(tv: &CastTv) -> Result<(), String> {
    av(tv, "Stop", "").await.map(|_| ())
}
pub async fn saltar(tv: &CastTv, s: f64) -> Result<(), String> {
    av(tv, "Seek", &format!("<Unit>REL_TIME</Unit><Target>{}</Target>", hms(s)))
        .await
        .map(|_| ())
}

pub async fn volumen(tv: &CastTv, v: f64) -> Result<(), String> {
    let n = (v.clamp(0.0, 1.0) * 100.0).round() as u32;
    soap(
        &tv.rc_url,
        &tv.rc_srv,
        "SetVolume",
        &format!("<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>{n}</DesiredVolume>"),
    )
    .await
    .map(|_| ())
}

pub async fn silencio(tv: &CastTv, m: bool) -> Result<(), String> {
    soap(
        &tv.rc_url,
        &tv.rc_srv,
        "SetMute",
        &format!("<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredMute>{}</DesiredMute>", m as u8),
    )
    .await
    .map(|_| ())
}

/// ¿Contesta la TV? (para validar la TV recordada).
pub async fn vive(tv: &CastTv) -> bool {
    av(tv, "GetTransportInfo", "").await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
<friendlyName>[TV] Samsung 7 Series (55)</friendlyName>
<manufacturer>Samsung Electronics</manufacturer><modelName>UE55RU7100</modelName>
<UDN>uuid:0ab1-samsung</UDN>
<serviceList>
<service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
<controlURL>/upnp/control/RenderingControl1</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
<controlURL>upnp/control/AVTransport1</controlURL></service>
</serviceList></device></root>"#;

    #[test]
    fn lee_la_descripcion() {
        let base = "http://192.168.1.40:9197/dmr";
        assert_eq!(etiqueta(DESC, "friendlyName"), Some("[TV] Samsung 7 Series (55)"));
        let (url, srv) = servicio(DESC, base, "AVTransport").unwrap();
        assert_eq!(url, "http://192.168.1.40:9197/upnp/control/AVTransport1");
        assert_eq!(srv, "urn:schemas-upnp-org:service:AVTransport:1");
        let (url, _) = servicio(DESC, base, "RenderingControl").unwrap();
        assert_eq!(url, "http://192.168.1.40:9197/upnp/control/RenderingControl1");
    }

    #[test]
    fn lee_respuestas_soap_con_prefijos() {
        let r = r#"<s:Envelope><s:Body><u:GetPositionInfoResponse xmlns:u="x"><Track>1</Track><TrackDuration>1:30:05</TrackDuration><TrackURI>http://a/v/1.mkv?x=1&amp;y=2</TrackURI><RelTime>0:01:02</RelTime></u:GetPositionInfoResponse></s:Body></s:Envelope>"#;
        assert_eq!(segundos(etiqueta(r, "RelTime").unwrap()), 62.0);
        assert_eq!(segundos(etiqueta(r, "TrackDuration").unwrap()), 5405.0);
        assert_eq!(des_escapar(etiqueta(r, "TrackURI").unwrap()), "http://a/v/1.mkv?x=1&y=2");
        assert_eq!(segundos("NOT_IMPLEMENTED"), 0.0);
        assert_eq!(hms(3725.9), "1:02:05");
    }

    #[test]
    fn didl_lleva_subtitulos_samsung() {
        let d = didl("Peli & Co", "http://a/v/1.mkv", "video/x-matroska", Some("http://a/s.srt"));
        assert!(d.contains("<dc:title>Peli &amp; Co</dc:title>"));
        assert!(d.contains(r#"<sec:CaptionInfoEx sec:type="srt">http://a/s.srt</sec:CaptionInfoEx>"#));
        assert!(d.contains("http-get:*:video/x-matroska:DLNA.ORG_OP=01"));
    }

    /// Red real, sin tocar la pantalla: busca renderers DLNA y lee su estado.
    /// `cargo test --lib dlna::tests::red_real -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn red_real() {
        let tvs = buscar(Duration::from_secs(3)).await;
        for tv in &tvs {
            eprintln!("{tv:?}");
            eprintln!("  estado: {:?}", estado(tv).await);
        }
        assert!(!tvs.is_empty(), "no hay renderers DLNA en la red");
    }
}
