// Kodios — agregador multi-fuente de torrents (magnets crudos) para debrid.
//
// Corre varias fuentes EN PARALELO (best-effort: si una cae, no rompe la
// búsqueda), deduplica por info_hash y mergea metadata (mejor seeders/size).
//
// Fuentes:
//   - Torrentio    (Stremio addon, IMDb)  → movie/series/anime
//   - MediaFusion  (Stremio addon, IMDb)  → movie/series/anime  [best-effort]
//   - YTS          (API, IMDb)            → movie
//   - EZTV         (API, IMDb)            → series
//   - Nyaa         (RSS, por título)      → anime
//
// POLÍTICA: solo magnets CRUDOS. La resolución magnet→URL directa la hace
// rd.rs al reproducir (los links pre-resueltos expiran).

use serde::Serialize;
use std::collections::HashMap;

const UA: &str = "kutral-kodios/0.1";
const TIMEOUT_S: u64 = 10; // timeout HTTP por fuente
const SOURCE_DEADLINE_S: u64 = 12; // tope duro por fuente (cuelgues)
const PER_SOURCE_CAP: usize = 60;

// Trackers públicos para construir magnets cuando la fuente solo da el hash.
const COMMON_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://open.stealth.si:80/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.torrent.eu.org:451/announce",
];

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

/// Fuente torrent lista para resolver vía debrid.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub source: String,            // "torrentio", "yts", "eztv", "nyaa"…
    pub title: String,
    pub magnet: Option<String>,
    /// URL ya resuelta por la fuente (Torrentio+RD). Si existe, se reproduce
    /// directo en mpv SIN addMagnet del cliente → evita el 451 de RD.
    pub url: Option<String>,
    pub info_hash: Option<String>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    pub quality: Quality,
    pub rd_cached: Option<bool>,
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .build()
        .map_err(|e| format!("client: {e}"))
}

// ========================================================================
// Comando Tauri + agregador
// ========================================================================

/// Busca fuentes para reproducir, agregando todas las fuentes relevantes.
/// `kind`: "movie" | "series" | "anime". `title` se usa para Nyaa (anime).
#[tauri::command]
pub async fn kodios_search(
    imdb_id: String,
    kind: String,
    season: Option<u32>,
    episode: Option<u32>,
    title: Option<String>,
) -> Result<Vec<Source>, String> {
    if imdb_id.is_empty() || !imdb_id.starts_with("tt") {
        return Err("imdb_id inválido (esperado ttXXXXXXX)".into());
    }
    Ok(aggregate(imdb_id, kind, season, episode, title.unwrap_or_default()).await)
}

async fn aggregate(
    imdb: String,
    kind: String,
    season: Option<u32>,
    episode: Option<u32>,
    title: String,
) -> Vec<Source> {
    eprintln!("[kodios] search imdb={imdb} kind={kind} s={season:?} e={episode:?}");
    let mut tasks: Vec<tokio::task::JoinHandle<Result<Vec<Source>, String>>> = Vec::new();

    // Torrentio SIN token (método Kodi/kodios): magnets crudos con info_hash
    // → instantAvailability marca cacheadas → resolvemos FRESCO con addMagnet+
    // unrestrict al reproducir (URLs no expiran como las pre-resueltas).
    {
        let (i, k) = (imdb.clone(), kind.clone());
        tasks.push(tokio::spawn(async move {
            torrentio_search(&i, &k, season, episode, None).await
        }));
    }
    {
        let (i, k) = (imdb.clone(), kind.clone());
        tasks.push(tokio::spawn(async move { mediafusion_search(&i, &k, season, episode).await }));
    }

    match kind.as_str() {
        "movie" => {
            let i = imdb.clone();
            tasks.push(tokio::spawn(async move { yts_search(&i).await }));
        }
        "series" => {
            let i = imdb.clone();
            tasks.push(tokio::spawn(async move { eztv_search(&i, season, episode).await }));
        }
        "anime" => {
            let t = title.clone();
            tasks.push(tokio::spawn(async move { nyaa_search(&t, episode).await }));
        }
        _ => {}
    }

    let mut all: Vec<Source> = Vec::new();
    let deadline = std::time::Duration::from_secs(SOURCE_DEADLINE_S);
    for h in tasks {
        match tokio::time::timeout(deadline, h).await {
            Ok(Ok(Ok(mut v))) => {
                if let Some(s) = v.first() {
                    eprintln!("[kodios]   {} → {} fuentes", s.source, v.len());
                }
                all.append(&mut v);
            }
            Ok(Ok(Err(e))) => eprintln!("[kodios]   fuente falló: {e}"),
            Ok(Err(e)) => eprintln!("[kodios]   task panic: {e}"),
            Err(_) => eprintln!("[kodios]   fuente timeout (>{SOURCE_DEADLINE_S}s)"),
        }
    }
    let merged = merge_and_sort(all);
    eprintln!("[kodios] total tras merge: {} fuentes", merged.len());
    merged
}

/// Dedup por info_hash + merge (mejor seeders/size, acumula fuentes),
/// luego ordena por calidad y seeders.
fn merge_and_sort(all: Vec<Source>) -> Vec<Source> {
    let mut by_hash: HashMap<String, Source> = HashMap::new();
    let mut no_hash: Vec<Source> = Vec::new();

    for s in all {
        match s.info_hash.clone() {
            Some(h) => {
                let key = h.to_lowercase();
                if let Some(e) = by_hash.get_mut(&key) {
                    if s.seeders.unwrap_or(0) > e.seeders.unwrap_or(0) {
                        e.seeders = s.seeders;
                    }
                    if e.size_bytes.is_none() {
                        e.size_bytes = s.size_bytes;
                    }
                    if e.quality == Quality::Unknown && s.quality != Quality::Unknown {
                        e.quality = s.quality;
                    }
                    // URL pre-resuelta gana (reproducción directa sin addMagnet).
                    if e.url.is_none() && s.url.is_some() {
                        e.url = s.url.clone();
                        e.rd_cached = Some(true);
                    }
                    if !e.source.split('+').any(|x| x == s.source) {
                        e.source = format!("{}+{}", e.source, s.source);
                    }
                } else {
                    by_hash.insert(key, s);
                }
            }
            None => no_hash.push(s),
        }
    }

    let mut out: Vec<Source> = by_hash.into_values().collect();
    out.extend(no_hash);
    out.sort_by(|a, b| {
        qrank(b.quality)
            .cmp(&qrank(a.quality))
            .then(b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)))
    });
    out
}

fn qrank(q: Quality) -> u8 {
    match q {
        Quality::P2160 => 5,
        Quality::P1080 => 4,
        Quality::P720 => 3,
        Quality::Sd => 2,
        Quality::Cam => 1,
        Quality::Unknown => 0,
    }
}

// ========================================================================
// Fuentes tipo Stremio (Torrentio, MediaFusion)
// ========================================================================

#[derive(serde::Deserialize)]
struct StremioResp {
    #[serde(default)]
    streams: Vec<StremioStream>,
}

#[derive(serde::Deserialize)]
struct StremioStream {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "infoHash")]
    info_hash: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
    /// Presente cuando el addon está configurado con token de debrid:
    /// URL pre-resuelta lista para reproducir.
    #[serde(default)]
    url: Option<String>,
}

/// Parser genérico de addon Stremio que devuelve streams con infoHash.
async fn stremio_search(
    base: &str,
    config_prefix: &str,
    source_name: &str,
    imdb: &str,
    kind: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    let (typ, id) = stremio_id(imdb, kind, season, episode)?;
    let url = format!("{base}{config_prefix}/stream/{typ}/{id}.json");

    let resp = http()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("{source_name} red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{source_name} {}", resp.status()));
    }
    let r: StremioResp = resp.json().await.map_err(|e| format!("{source_name} parse: {e}"))?;

    let mut out = Vec::with_capacity(r.streams.len().min(PER_SOURCE_CAP));
    for s in r.streams.into_iter().take(PER_SOURCE_CAP) {
        let title = s.title.clone().or_else(|| s.name.clone()).unwrap_or_default();
        let (seeders, size_bytes) = parse_meta(&title);
        let magnet = s
            .info_hash
            .as_ref()
            .map(|h| build_magnet(h, &title, s.sources.as_deref()));
        // URL pre-resuelta (Torrentio+RD) → cacheado, listo para mpv directo.
        let pre = s.url.is_some();
        out.push(Source {
            source: source_name.into(),
            quality: detect_quality(&title),
            title,
            magnet,
            url: s.url,
            info_hash: s.info_hash,
            size_bytes,
            seeders,
            rd_cached: if pre { Some(true) } else { None },
        });
    }
    Ok(out)
}

fn stremio_id(
    imdb: &str,
    kind: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<(&'static str, String), String> {
    match kind {
        "movie" => Ok(("movie", imdb.to_string())),
        "series" | "show" | "anime" => {
            let s = season.ok_or("season requerido para series")?;
            let e = episode.ok_or("episode requerido para series")?;
            Ok(("series", format!("{imdb}:{s}:{e}")))
        }
        other => Err(format!("kind desconocido: {other}")),
    }
}

/// Torrentio: filtra basura y ordena por calidad. Con `token` RD, Torrentio
/// resuelve en su servidor y devuelve URLs directas (método de Kodi) → evita
/// el `addMagnet` del cliente que RD bloquea con 451.
pub async fn torrentio_search(
    imdb: &str,
    kind: &str,
    season: Option<u32>,
    episode: Option<u32>,
    token: Option<&str>,
) -> Result<Vec<Source>, String> {
    let mut opts = String::new();
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        opts.push_str(&format!("realdebrid={t}|"));
    }
    opts.push_str("qualityfilter=cam,scr,480p|sort=quality");
    let prefix = format!("/{opts}");
    stremio_search(
        "https://torrentio.strem.fun",
        &prefix,
        "torrentio",
        imdb,
        kind,
        season,
        episode,
    )
    .await
}

/// MediaFusion: instancia pública. Best-effort (puede requerir config → 401).
async fn mediafusion_search(
    imdb: &str,
    kind: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    stremio_search(
        "https://mediafusion.elfhosted.com",
        "",
        "mediafusion",
        imdb,
        kind,
        season,
        episode,
    )
    .await
}

// ========================================================================
// YTS (películas, IMDb)
// ========================================================================

#[derive(serde::Deserialize)]
struct YtsResp {
    data: Option<YtsData>,
}
#[derive(serde::Deserialize)]
struct YtsData {
    #[serde(default)]
    movies: Vec<YtsMovie>,
}
#[derive(serde::Deserialize)]
struct YtsMovie {
    #[serde(default)]
    title_long: String,
    #[serde(default)]
    torrents: Vec<YtsTorrent>,
}
#[derive(serde::Deserialize)]
struct YtsTorrent {
    hash: String,
    #[serde(default)]
    quality: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    seeds: u32,
    #[serde(default)]
    size_bytes: u64,
}

async fn yts_search(imdb: &str) -> Result<Vec<Source>, String> {
    let url = format!("https://yts.mx/api/v2/list_movies.json?query_term={imdb}");
    let resp = http()?.get(&url).send().await.map_err(|e| format!("yts red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("yts {}", resp.status()));
    }
    let r: YtsResp = resp.json().await.map_err(|e| format!("yts parse: {e}"))?;
    let mut out = Vec::new();
    for m in r.data.map(|d| d.movies).unwrap_or_default() {
        for t in m.torrents {
            let name = format!("{} {} {}", m.title_long, t.quality, t.kind).trim().to_string();
            out.push(Source {
                source: "yts".into(),
                quality: detect_quality(&t.quality),
                magnet: Some(build_magnet(&t.hash, &name, None)),
                url: None,
                info_hash: Some(t.hash),
                size_bytes: if t.size_bytes > 0 { Some(t.size_bytes) } else { None },
                seeders: Some(t.seeds),
                rd_cached: None,
                title: name,
            });
        }
    }
    Ok(out)
}

// ========================================================================
// EZTV (series, IMDb)
// ========================================================================

#[derive(serde::Deserialize)]
struct EztvResp {
    #[serde(default)]
    torrents: Vec<EztvTorrent>,
}
#[derive(serde::Deserialize)]
struct EztvTorrent {
    #[serde(default)]
    title: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    magnet_url: Option<String>,
    #[serde(default)]
    seeds: u32,
    #[serde(default)]
    size_bytes: String, // EZTV manda size como string
    #[serde(default)]
    season: String,
    #[serde(default)]
    episode: String,
}

async fn eztv_search(
    imdb: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    // EZTV espera el imdb COMPLETO con "tt" (ej. tt0903747).
    let url = format!("https://eztvx.to/api/get-torrents?imdb_id={imdb}&limit=100");
    let resp = http()?.get(&url).send().await.map_err(|e| format!("eztv red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("eztv {}", resp.status()));
    }
    let r: EztvResp = resp.json().await.map_err(|e| format!("eztv parse: {e}"))?;
    let mut out = Vec::new();
    for t in r.torrents.into_iter().take(PER_SOURCE_CAP) {
        // Filtra por temporada/episodio si se pidió.
        if let Some(s) = season {
            if t.season.parse::<u32>().ok() != Some(s) {
                continue;
            }
        }
        if let Some(e) = episode {
            if t.episode.parse::<u32>().ok() != Some(e) {
                continue;
            }
        }
        let magnet = t
            .magnet_url
            .clone()
            .or_else(|| if t.hash.is_empty() { None } else { Some(build_magnet(&t.hash, &t.title, None)) });
        out.push(Source {
            source: "eztv".into(),
            quality: detect_quality(&t.title),
            magnet,
            url: None,
            info_hash: if t.hash.is_empty() { None } else { Some(t.hash) },
            size_bytes: t.size_bytes.parse::<u64>().ok().filter(|n| *n > 0),
            seeders: Some(t.seeds),
            rd_cached: None,
            title: t.title,
        });
    }
    Ok(out)
}

// ========================================================================
// Nyaa (anime, por título — RSS)
// ========================================================================

async fn nyaa_search(title: &str, episode: Option<u32>) -> Result<Vec<Source>, String> {
    if title.trim().is_empty() {
        return Err("nyaa: sin título".into());
    }
    // Query: título + nº de episodio con padding (formato típico de fansubs).
    let mut q = title.trim().to_string();
    if let Some(e) = episode {
        q.push_str(&format!(" {e:02}"));
    }
    // c=1_2 = Anime · English-translated; f=0 = sin filtro; s=seeders desc.
    let url = format!(
        "https://nyaa.si/?page=rss&c=1_2&f=0&s=seeders&o=desc&q={}",
        urlencoding::encode(&q)
    );
    let resp = http()?.get(&url).send().await.map_err(|e| format!("nyaa red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("nyaa {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| format!("nyaa body: {e}"))?;

    let mut out = Vec::new();
    for item in body.split("<item>").skip(1).take(PER_SOURCE_CAP) {
        let item = item.split("</item>").next().unwrap_or("");
        let title = rss_field(item, "title").unwrap_or_default();
        let hash = match rss_field(item, "nyaa:infoHash") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let seeders = rss_field(item, "nyaa:seeders").and_then(|s| s.parse::<u32>().ok());
        let size = rss_field(item, "nyaa:size").and_then(|s| parse_size(&s.replace("iB", "B")));
        out.push(Source {
            source: "nyaa".into(),
            quality: detect_quality(&title),
            magnet: Some(build_magnet(&hash, &title, None)),
            url: None,
            info_hash: Some(hash),
            size_bytes: size,
            seeders,
            rd_cached: None,
            title,
        });
    }
    Ok(out)
}

/// Extrae el contenido de <tag>…</tag> de un item RSS (maneja CDATA).
fn rss_field(item: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = item.find(&open)? + open.len();
    let rel_e = item[s..].find(&close)?;
    let raw = item[s..s + rel_e].trim();
    let raw = raw
        .strip_prefix("<![CDATA[")
        .map(|x| x.strip_suffix("]]>").unwrap_or(x))
        .unwrap_or(raw);
    Some(raw.trim().to_string())
}

// ========================================================================
// Helpers comunes
// ========================================================================

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

/// Parseo manual de "👤 12 💾 1.2 GB" en títulos Stremio.
fn parse_meta(t: &str) -> (Option<u32>, Option<u64>) {
    let seeders = capture_after(t, '👤').and_then(|s| {
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().ok()
    });
    let size = capture_after(t, '💾').and_then(parse_size);
    (seeders, size)
}

fn capture_after(t: &str, marker: char) -> Option<&str> {
    let idx = t.find(marker)?;
    Some(t[idx + marker.len_utf8()..].trim_start())
}

/// Parsea "1.2 GB" / "850 MB" / "1.4 TB" → bytes.
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
    } else if rest.starts_with("KB") {
        1024.0
    } else {
        return None;
    };
    Some((n * mult) as u64)
}

fn build_magnet(hash: &str, name: &str, trackers: Option<&[String]>) -> String {
    let mut m = format!("magnet:?xt=urn:btih:{hash}&dn={}", urlencoding::encode(name));
    // Trackers de la fuente (Stremio los manda como "tracker:URL").
    let mut added = false;
    if let Some(trs) = trackers {
        for t in trs.iter().filter_map(|s| s.strip_prefix("tracker:")) {
            m.push_str("&tr=");
            m.push_str(&urlencoding::encode(t));
            added = true;
        }
    }
    // Si la fuente no trae trackers, usa los públicos comunes.
    if !added {
        for t in COMMON_TRACKERS {
            m.push_str("&tr=");
            m.push_str(&urlencoding::encode(t));
        }
    }
    m
}
