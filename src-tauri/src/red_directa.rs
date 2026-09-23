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
//              tv_en en los que respondieron. Los equipos que el sistema ya
//              conoce (tabla ARP) van antes y con más paciencia: el barrido
//              de 254 direcciones, casi todas vacías, satura la resolución
//              ARP; una TV viva quedaba sin respuesta a tiempo y a veces la
//              búsqueda no traía nada (medido: la misma LG, una vez sí y otra no).

use crate::cast::CastTv;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

// En la red de la casa un puerto cerrado contesta al toque (RST); este tope
// solo pesa con equipos que descartan en silencio.
const ESPERA_CONEXION: Duration = Duration::from_millis(300);
/// Equipos que ya se sabe que están conectados: vale esperar más.
const ESPERA_VECINO: Duration = Duration::from_millis(1500);
/// El resto del barrido: la mayoría no existe y hay que resolver ARP de cada una.
const ESPERA_BARRIDO: Duration = Duration::from_millis(600);

/// Puertos que delatan una TV o algo que reproduce: Google Cast, webOS (LG),
/// Samsung, Sony, Panasonic, Sonos/Philips y los típicos de UPnP.
const SENALES: &[u16] = &[
    8008, 8009, 3000, 3001, 7676, 9197, 8001, 8002, 52323, 55000, 1400, 2869, 8200, 49152,
    49153, 49154,
];

/// Dónde suelen estar las descripciones UPnP (LG: "/", Samsung: "/dmr").
const RUTAS: &[&str] = &["/", "/dmr", "/description.xml", "/rootDesc.xml"];

async fn abierto(ip: Ipv4Addr, puerto: u16) -> bool {
    abierto_en(ip, puerto, ESPERA_CONEXION).await
}

async fn abierto_en(ip: Ipv4Addr, puerto: u16, espera: Duration) -> bool {
    let destino = SocketAddr::from((ip, puerto));
    matches!(
        tokio::time::timeout(espera, tokio::net::TcpStream::connect(destino)).await,
        Ok(Ok(_))
    )
}

/// ¿Rechaza al toque las conexiones a puertos cerrados? Las TVs lo hacen (RST)
/// y entonces recorrer sus 65 mil puertos lleva segundos; un celular suele
/// descartar en silencio y recorrerlo es esperar el tope en cada puerto.
async fn rechaza_rapido(ip: Ipv4Addr) -> bool {
    let mut rapidos = 0;
    // Puertos que nadie usa: lo esperable es que estén cerrados.
    for p in [1u16, 7, 9] {
        let destino = SocketAddr::from((ip, p));
        if let Ok(Err(e)) =
            tokio::time::timeout(ESPERA_CONEXION, tokio::net::TcpStream::connect(destino)).await
        {
            if e.kind() == std::io::ErrorKind::ConnectionRefused {
                rapidos += 1;
            }
        }
    }
    rapidos >= 2
}

/// Equipos de la red que el sistema ya resolvió (tabla ARP).
fn vecinos() -> Vec<Ipv4Addr> {
    #[cfg(target_os = "linux")]
    {
        // Flags 0x2 = entrada completa (hay MAC); las incompletas no sirven.
        std::fs::read_to_string("/proc/net/arp")
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter_map(|l| {
                let c: Vec<&str> = l.split_whitespace().collect();
                (c.len() >= 3 && c[2] != "0x0").then(|| c[0].parse().ok()).flatten()
            })
            .collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // `arp -a` sale en el idioma del sistema, pero las IPs son IPs.
        let mut cmd = std::process::Command::new("arp");
        crate::winproc::hide_console(&mut cmd);
        cmd.arg("-a")
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// De `hosts`, qué puertos SENALES tiene abiertos cada uno (solo los que
/// tienen alguno).
async fn con_senales(hosts: &[Ipv4Addr], espera: Duration, paralelo: usize) -> Vec<(Ipv4Addr, u16)> {
    let turnos = Arc::new(Semaphore::new(paralelo));
    let mut js = JoinSet::new();
    for &ip in hosts {
        for &p in SENALES {
            let Ok(turno) = turnos.clone().acquire_owned().await else { break };
            js.spawn(async move {
                let ok = abierto_en(ip, p, espera).await;
                drop(turno);
                ok.then_some((ip, p))
            });
        }
    }
    let mut out: Vec<(Ipv4Addr, u16)> = js.join_all().await.into_iter().flatten().collect();
    out.sort_unstable();
    out
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

/// La TV que haya en `ip`, si hay una (IP escrita a mano: recorre todo).
pub async fn tv_en(ip: Ipv4Addr) -> Option<CastTv> {
    let senales = abiertos(ip, SENALES.iter().copied(), 32).await;
    tv_con(ip, &senales, true).await
}

/// `senales`: los puertos SENALES que ya se vio abiertos. Si ahí no hay Cast
/// ni DLNA, se recorren los 65 mil puertos: hay renderizadores en puertos que
/// cambian (LG: 1811 en una UK6200) o que no se conocen de antemano (otras
/// marcas, cajas Android sin Google). Sin fijarse en la marca: se recorre lo
/// que se pidió a mano y lo que rechaza rápido los puertos cerrados. Un
/// equipo que descarta en silencio (un celular) eran ~40 s de espera para nada.
async fn tv_con(ip: Ipv4Addr, senales: &[u16], recorrer: bool) -> Option<CastTv> {
    if senales.contains(&8009) {
        return Some(cast_en(ip).await);
    }
    if let Some(tv) = dlna_en(ip, senales).await {
        return Some(tv);
    }
    let webos = senales.contains(&3000) || senales.contains(&3001);
    if !(recorrer || webos || rechaza_rapido(ip).await) {
        return None;
    }
    // Por tramos, del más probable al menos (las LG tienen sus servicios UPnP
    // entre 1024 y 3000). Con 128 conexiones a la vez y no más: con 512 la
    // LG empezó a descartar y el tramo bueno salía vacío (medido). En un
    // webOS el primer tramo se repite una vez tras una pausa por lo mismo.
    // Los demás equipos pasan por los mismos tramos, sin la repetición.
    let tramos = [1024..=3_000u16, 1024..=3_000, 3_001..=10_000, 10_001..=65_535];
    for (i, tramo) in tramos.into_iter().enumerate() {
        if i == 1 {
            if !webos {
                continue;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let puertos = abiertos(ip, tramo, 128).await;
        let nuevos: Vec<u16> = puertos.into_iter().filter(|p| !senales.contains(p)).collect();
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
    let en_red = |ip: &Ipv4Addr| {
        let o = ip.octets();
        o[..3] == [a, b, c] && o[3] != 0 && o[3] != 255 && *ip != propia
    };
    let conocidos: Vec<Ipv4Addr> = vecinos().into_iter().filter(|ip| en_red(ip)).collect();
    // Primero los conocidos, solos: el barrido de después llena la tabla ARP
    // de entradas a medio resolver y puede desalojar a la TV.
    let mut vistos = con_senales(&conocidos, ESPERA_VECINO, 64).await;
    let resto: Vec<Ipv4Addr> = (1..=254u8)
        .map(|d| Ipv4Addr::new(a, b, c, d))
        .filter(|ip| en_red(ip) && !conocidos.contains(ip))
        .collect();
    vistos.extend(con_senales(&resto, ESPERA_BARRIDO, 256).await);
    let mut candidatos: std::collections::BTreeMap<Ipv4Addr, Vec<u16>> = Default::default();
    for (ip, p) in vistos {
        candidatos.entry(ip).or_default().push(p);
    }
    eprintln!("[red] búsqueda directa: conocidos {conocidos:?} → candidatos {candidatos:?}");

    let mut js = JoinSet::new();
    for (ip, senales) in candidatos {
        js.spawn(async move { tv_con(ip, &senales, false).await });
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
