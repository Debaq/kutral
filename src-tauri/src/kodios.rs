// Kodios — búsqueda de fuentes torrent vía Torrentio.
//
// Portado de kodi-os/scraper (kodios-scraperd), adaptado a kutral sin deps
// nuevas (sin regex, sin async_trait): un solo source, parseo manual.
//
// POLÍTICA DE FUENTES (heredada del daemon):
//   Solo magnets CRUDOS. El token RD NO se pasa a Torrentio — los links
//   pre-resueltos por Torrentio expiran rápido y rompen la reproducción.
//   La resolución magnet→URL directa se hace en rd.rs al momento de reproducir.
//
// Endpoint: https://torrentio.strem.fun/<config>/stream/<type>/<id>.json
//   type: "movie" | "series"
//   id:   imdb (tt1234567)  o  imdb:season:episode  para series

use serde::Serialize;

const BASE: &str = "https://torrentio.strem.fun";
// Filtra basura (cam/screener/480p) y ordena por calidad.
const CONFIG_PREFIX: &str = "/qualityfilter=cam,scr,480p|sort=quality";
const UA: &str = "kutral-kodios/0.1";
const TIMEOUT_S: u64 = 15;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Cam,
    Sd,
    P720,
    P1080,
    P2160,
    Unknown,
}

/// Fuente torrent lista para resolver vía RealDebrid.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub source: String,            // "torrentio"
    pub title: String,             // nombre del release
    pub magnet: Option<String>,    // magnet crudo (se resuelve con RD al reproducir)
    pub info_hash: Option<String>, // btih, sirve para instant-availability
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    pub quality: Quality,
    /// true si ya está cacheado en RD (lo rellena rd.rs); None = sin verificar.
    pub rd_cached: Option<bool>,
}

// ---- Parseo de la respuesta de Torrentio --------------------------------

#[derive(serde::Deserialize)]
struct Resp {
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(serde::Deserialize)]
struct Stream {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "infoHash")]
    info_hash: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
}

/// Busca fuentes torrent para un título.
///
/// `kind`: "movie" | "series"; para series `season`/`episode` son obligatorios.
pub async fn torrentio_search(
    imdb_id: &str,
    kind: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    if imdb_id.is_empty() || !imdb_id.starts_with("tt") {
        return Err("imdb_id inválido (esperado ttXXXXXXX)".into());
    }

    let (typ, id) = match kind {
        "movie" => ("movie", imdb_id.to_string()),
        "series" | "show" | "anime" => {
            let s = season.ok_or("season requerido para series")?;
            let e = episode.ok_or("episode requerido para series")?;
            ("series", format!("{imdb_id}:{s}:{e}"))
        }
        other => return Err(format!("kind desconocido: {other}")),
    };

    let url = format!("{BASE}{CONFIG_PREFIX}/stream/{typ}/{id}.json");

    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .build()
        .map_err(|e| format!("client: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("torrentio {}", resp.status()));
    }
    let r: Resp = resp.json().await.map_err(|e| format!("parse: {e}"))?;

    let mut out = Vec::with_capacity(r.streams.len());
    for s in r.streams {
        let title = s.title.clone().or_else(|| s.name.clone()).unwrap_or_default();
        let quality = detect_quality(&title);
        let (seeders, size_bytes) = parse_meta(&title);
        let magnet = s
            .info_hash
            .as_ref()
            .map(|h| build_magnet(h, &title, s.sources.as_deref()));
        out.push(Source {
            source: "torrentio".into(),
            title,
            magnet,
            info_hash: s.info_hash,
            size_bytes,
            seeders,
            quality,
            rd_cached: None,
        });
    }
    Ok(out)
}

// ---- Comando Tauri ------------------------------------------------------

/// Busca fuentes torrent para reproducir. El frontend recibe la lista ya
/// ordenada por calidad (Torrentio sort=quality). Aún sin resolver: los
/// magnets se pasan a RD al momento de "Descubrir".
#[tauri::command]
pub async fn kodios_search(
    imdb_id: String,
    kind: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    torrentio_search(&imdb_id, &kind, season, episode).await
}

// ---- Helpers ------------------------------------------------------------

fn detect_quality(t: &str) -> Quality {
    let s = t.to_lowercase();
    if s.contains("2160") || s.contains("4k") {
        Quality::P2160
    } else if s.contains("1080") {
        Quality::P1080
    } else if s.contains("720") {
        Quality::P720
    } else if s.contains("cam ") || s.contains(".cam.") {
        Quality::Cam
    } else if s.contains("480") || s.contains(" sd") {
        Quality::Sd
    } else {
        Quality::Unknown
    }
}

/// Torrentio mete "👤 12 💾 1.2 GB" en el título. Parseo manual sin regex.
fn parse_meta(t: &str) -> (Option<u32>, Option<u64>) {
    let seeders = capture_after(t, '👤').and_then(|s| {
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().ok()
    });
    let size = capture_after(t, '💾').and_then(parse_size);
    (seeders, size)
}

/// Devuelve el substring que sigue a `marker` (saltando espacios).
fn capture_after(t: &str, marker: char) -> Option<&str> {
    let idx = t.find(marker)?;
    Some(t[idx + marker.len_utf8()..].trim_start())
}

/// Parsea "1.2 GB" / "850 MB" → bytes.
fn parse_size(s: &str) -> Option<u64> {
    let num: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let n: f64 = num.parse().ok()?;
    let rest = s[num.len()..].trim_start().to_uppercase();
    let mult: f64 = if rest.starts_with("GB") {
        1_073_741_824.0
    } else if rest.starts_with("MB") {
        1_048_576.0
    } else if rest.starts_with("TB") {
        1_099_511_627_776.0
    } else {
        return None;
    };
    Some((n * mult) as u64)
}

fn build_magnet(hash: &str, name: &str, trackers: Option<&[String]>) -> String {
    let mut m = format!(
        "magnet:?xt=urn:btih:{hash}&dn={}",
        urlencoding::encode(name)
    );
    if let Some(trs) = trackers {
        for t in trs.iter().filter_map(|s| s.strip_prefix("tracker:")) {
            m.push_str("&tr=");
            m.push_str(&urlencoding::encode(t));
        }
    }
    m
}
