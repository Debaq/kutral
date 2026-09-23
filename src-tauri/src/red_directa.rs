// Búsqueda de TVs sin multicast.
//
// La búsqueda normal (mDNS para Cast, SSDP para DLNA) depende de que el router
// reenvíe multicast por wifi, y hay routers que no lo hacen — sobre todo en la
// banda de 5 GHz. Ahí no llega nada, con o sin firewall. Por TCP en cambio sí
// se llega: esto prueba los equipos de la red uno por uno.
//
//   tv_en(ip)  una IP concreta: Cast si tiene el 8009, si no DLNA buscando en
//              sus puertos abiertos una descripción con AVTransport.
//   barrer()   toda la /24: primero los puertos que delatan una TV y después
//              tv_en en los que respondieron.

use crate::cast::CastTv;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

// En la red de la casa un puerto cerrado contesta al toque (RST); este tope
// solo pesa con equipos que descartan en silencio.
const ESPERA_CONEXION: Duration = Duration::from_millis(300);

/// Puertos que delatan una TV o algo que reproduce: Google Cast, webOS (LG),
/// Samsung, Sony, Panasonic, Sonos/Philips y los típicos de UPnP.
const SENALES: &[u16] = &[
    8008, 8009, 3000, 3001, 7676, 9197, 8001, 8002, 52323, 55000, 1400, 2869, 8200, 49152,
    49153, 49154,
];

/// Dónde suelen estar las descripciones UPnP (LG: "/", Samsung: "/dmr").
const RUTAS: &[&str] = &["/", "/dmr", "/description.xml", "/rootDesc.xml"];

async fn abierto(ip: Ipv4Addr, puerto: u16) -> bool {
    let destino = SocketAddr::from((ip, puerto));
    matches!(
        tokio::time::timeout(ESPERA_CONEXION, tokio::net::TcpStream::connect(destino)).await,
        Ok(Ok(_))
    )
}

async fn abiertos(ip: Ipv4Addr, puertos: impl IntoIterator<Item = u16>, paralelo: usize) -> Vec<u16> {
    let turnos = Arc::new(Semaphore::new(paralelo));
    let mut js = JoinSet::new();
    for p in puertos {
        let Ok(turno) = turnos.clone().acquire_owned().await else { break };
        js.spawn(async move {
            let ok = abierto(ip, p).await;
            drop(turno);
            ok.then_some(p)
        });
    }
    let mut out: Vec<u16> = js.join_all().await.into_iter().flatten().collect();
    out.sort_unstable();
    out
}

/// Google Cast por IP: el nombre y el UDN salen de eureka_info. El id queda
/// igual que el del TXT de mDNS (UDN sin guiones), así una TV recordada por la
/// búsqueda normal se reconoce.
async fn cast_en(ip: Ipv4Addr) -> CastTv {
    let info: serde_json::Value = async {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .build()
            .ok()?
            .get(format!("http://{ip}:8008/setup/eureka_info?params=name,device_info"))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()
    }
    .await
    .unwrap_or_default();
    let nombre = info["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string);
    let udn = info["ssdp_udn"].as_str().unwrap_or_default().replace('-', "");
    CastTv {
        id: if udn.is_empty() { format!("ip:{ip}") } else { udn },
        nombre: nombre.unwrap_or_else(|| format!("TV ({ip})")),
        modelo: info["device_info"]["model_name"].as_str().unwrap_or_default().to_string(),
        ip: ip.to_string(),
        puerto: 8009,
        tipo: "cast".into(),
        ..Default::default()
    }
}

async fn dlna_en(ip: Ipv4Addr, puertos: &[u16]) -> Option<CastTv> {
    let mut js = JoinSet::new();
    for &p in puertos {
        for r in RUTAS {
            let loc = format!("http://{ip}:{p}{r}");
            js.spawn(async move { crate::dlna::describir(&loc).await });
        }
    }
    while let Some(r) = js.join_next().await {
        if let Ok(Some(tv)) = r {
            js.abort_all();
            return Some(tv);
        }
    }
    None
}

/// La TV que haya en `ip`, si hay una.
pub async fn tv_en(ip: Ipv4Addr) -> Option<CastTv> {
    if abierto(ip, 8009).await {
        return Some(cast_en(ip).await);
    }
    let conocidos = abiertos(ip, SENALES.iter().copied(), 32).await;
    if let Some(tv) = dlna_en(ip, &conocidos).await {
        return Some(tv);
    }
    // El renderizador DLNA de LG cambia de puerto (1811 en una UK6200): hay
    // que recorrer. Los bajos primero, que es donde suelen estar.
    for rango in [1024..=10_000u16, 10_001..=65_535] {
        let puertos = abiertos(ip, rango, 512).await;
        let nuevos: Vec<u16> = puertos.into_iter().filter(|p| !conocidos.contains(p)).collect();
        if let Some(tv) = dlna_en(ip, &nuevos).await {
            return Some(tv);
        }
    }
    None
}

/// TVs en la /24 de este equipo, sin multicast.
pub async fn barrer() -> Vec<CastTv> {
    // La IP con la que este equipo sale a la red (no manda nada: solo elige ruta).
    let Ok(IpAddr::V4(propia)) = crate::lan::ip_hacia("8.8.8.8", 53) else {
        return Vec::new();
    };
    let [a, b, c, _] = propia.octets();
    let turnos = Arc::new(Semaphore::new(512));
    let mut js = JoinSet::new();
    for d in 1..=254u8 {
        let ip = Ipv4Addr::new(a, b, c, d);
        if ip == propia {
            continue;
        }
        for &p in SENALES {
            let Ok(turno) = turnos.clone().acquire_owned().await else { break };
            js.spawn(async move {
                let ok = abierto(ip, p).await;
                drop(turno);
                ok.then_some(ip)
            });
        }
    }
    let mut candidatos: Vec<Ipv4Addr> = js.join_all().await.into_iter().flatten().collect();
    candidatos.sort_unstable();
    candidatos.dedup();
    eprintln!("[red] búsqueda directa: candidatos {candidatos:?}");

    let mut js = JoinSet::new();
    for ip in candidatos {
        js.spawn(tv_en(ip));
    }
    let mut tvs: Vec<CastTv> = js.join_all().await.into_iter().flatten().collect();
    tvs.dedup_by(|x, y| x.id == y.id);
    for tv in &tvs {
        eprintln!("[red] encontrada {} ({}) en {} por {}", tv.nombre, tv.modelo, tv.ip, tv.tipo);
    }
    tvs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Red real, sin tocar la pantalla: `cargo test --lib red_directa --
    /// --ignored --nocapture` (con KUTRAL_TV_IP=… prueba solo esa IP).
    #[tokio::test]
    #[ignore]
    async fn red_real() {
        // Uno u otro, no los dos: dos recorridos seguidos de 65 mil puertos
        // y la TV deja de contestar un rato (medido con una LG UK6200).
        let t0 = std::time::Instant::now();
        if let Ok(ip) = std::env::var("KUTRAL_TV_IP") {
            let tv = tv_en(ip.parse().unwrap()).await;
            eprintln!("tv_en({ip}) en {:?}: {tv:?}", t0.elapsed());
            assert!(tv.is_some());
            return;
        }
        let tvs = barrer().await;
        eprintln!("barrer() en {:?}: {tvs:?}", t0.elapsed());
        assert!(!tvs.is_empty(), "no apareció ninguna TV");
    }
}
