use serde::{Deserialize, Serialize};

const TMDB_BASE: &str = "https://api.themoviedb.org/3";
const LANG: &str = "es-ES";

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn extract_imdb_id(path: &str) -> Option<String> {
    path.split('/').find_map(|seg| {
        if seg.starts_with("tt") && seg.len() >= 8 && seg[2..].chars().all(|c| c.is_ascii_digit()) {
            Some(seg.to_string())
        } else {
            None
        }
    })
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(BROWSER_UA)
        .build()
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TmdbItem {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub vote_average: f32,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub first_air_date: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TmdbListResp {
    pub page: u64,
    #[serde(default)]
    pub total_pages: u64,
    pub results: Vec<TmdbItem>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PersonMini {
    pub id: u64,
    pub name: String,
    pub profile_path: Option<String>,
    pub character: Option<String>,
    pub job: Option<String>,
}

#[derive(Serialize)]
pub struct TmdbDetail {
    pub id: u64,
    pub media_type: String,
    pub title: String,
    pub overview: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: f32,
    pub year: String,
    pub imdb_id: Option<String>,
    pub runtime: Option<u32>,
    pub genres: Vec<String>,
    pub directors: Vec<PersonMini>,
    pub cast: Vec<PersonMini>,
}

#[derive(Deserialize)]
struct DetailRaw {
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    overview: String,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    vote_average: f32,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    external_ids: Option<ExternalIds>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default)]
    episode_run_time: Option<Vec<u32>>,
    #[serde(default)]
    genres: Vec<Genre>,
    #[serde(default)]
    credits: Option<CreditsRaw>,
    #[serde(default)]
    created_by: Option<Vec<CreatorRaw>>,
}

#[derive(Deserialize)]
struct CreditsRaw {
    #[serde(default)]
    cast: Vec<CastRaw>,
    #[serde(default)]
    crew: Vec<CrewRaw>,
}

#[derive(Deserialize)]
struct CastRaw {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    profile_path: Option<String>,
    #[serde(default)]
    character: Option<String>,
    #[serde(default)]
    order: Option<u32>,
}

#[derive(Deserialize)]
struct CrewRaw {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    profile_path: Option<String>,
    #[serde(default)]
    job: Option<String>,
}

#[derive(Deserialize)]
struct CreatorRaw {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    profile_path: Option<String>,
}

#[derive(Deserialize)]
struct ExternalIds {
    #[serde(default)]
    imdb_id: Option<String>,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let resp = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("red: {}", e))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("TMDb {}: {}", status, body));
    }
    serde_json::from_str(&body).map_err(|e| format!("parse: {} :: {}", e, body))
}

#[derive(Serialize, Deserialize)]
pub struct GenreItem {
    pub id: u64,
    pub name: String,
}

#[derive(Deserialize)]
struct GenresResp {
    genres: Vec<GenreItem>,
}

#[tauri::command]
async fn tmdb_genres(media_type: String, api_key: String) -> Result<Vec<GenreItem>, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let url = format!(
        "{}/genre/{}/list?api_key={}&language={}",
        TMDB_BASE, media_type, api_key, LANG
    );
    let r: GenresResp = fetch_json(&url).await?;
    Ok(r.genres)
}

#[tauri::command]
async fn tmdb_discover(
    media_type: String,
    page: u64,
    api_key: String,
    sort_by: Option<String>,
    with_genres: Option<String>,
    origin_country: Option<String>,
    force_genres: Option<String>,
) -> Result<TmdbListResp, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let sort = sort_by.unwrap_or_else(|| "popularity.desc".into());
    let mut url = format!(
        "{}/discover/{}?api_key={}&language={}&page={}&sort_by={}&include_adult=false",
        TMDB_BASE, media_type, api_key, LANG, page, sort
    );
    if sort.starts_with("vote_average") {
        url.push_str("&vote_count.gte=100");
    }
    // Combinar géneros user + forzados (anime fuerza 16)
    let mut all_genres: Vec<String> = Vec::new();
    if let Some(f) = force_genres.as_ref().filter(|s| !s.is_empty()) {
        for g in f.split(',') { all_genres.push(g.to_string()); }
    }
    if let Some(g) = with_genres.as_ref().filter(|s| !s.is_empty()) {
        for g in g.split(',') {
            let g = g.to_string();
            if !all_genres.contains(&g) { all_genres.push(g); }
        }
    }
    if !all_genres.is_empty() {
        url.push_str(&format!("&with_genres={}", all_genres.join(",")));
    }
    if let Some(c) = origin_country.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&with_origin_country={}", c));
    }
    fetch_json(&url).await
}

#[tauri::command]
async fn tmdb_search(
    media_type: String,
    query: String,
    page: u64,
    api_key: String,
) -> Result<TmdbListResp, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let q = urlencoding::encode(&query);
    let url = format!(
        "{}/search/{}?api_key={}&language={}&page={}&query={}",
        TMDB_BASE, media_type, api_key, LANG, page, q
    );
    fetch_json(&url).await
}

#[derive(Serialize, Deserialize)]
pub struct VideoItem {
    pub key: String,
    pub name: String,
    pub site: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub official: bool,
}

#[derive(Deserialize)]
struct VideosResp {
    results: Vec<VideoItem>,
}

#[tauri::command]
async fn tmdb_videos(
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<Vec<VideoItem>, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    // 1ra pasada: idioma local
    let url_local = format!(
        "{}/{}/{}/videos?api_key={}&language={}",
        TMDB_BASE, media_type, id, api_key, LANG
    );
    let mut vids: VideosResp = fetch_json(&url_local).await.unwrap_or(VideosResp { results: vec![] });
    // Fallback inglés si no hay nada (común para trailers)
    if vids.results.is_empty() {
        let url_en = format!(
            "{}/{}/{}/videos?api_key={}&language=en-US",
            TMDB_BASE, media_type, id, api_key
        );
        if let Ok(v) = fetch_json::<VideosResp>(&url_en).await {
            vids = v;
        }
    }
    // Filtro: solo YouTube + Trailer/Teaser, preferir oficiales
    let mut filtered: Vec<VideoItem> = vids
        .results
        .into_iter()
        .filter(|v| v.site == "YouTube" && (v.kind == "Trailer" || v.kind == "Teaser"))
        .collect();
    filtered.sort_by(|a, b| {
        let order = |k: &str| if k == "Trailer" { 0 } else { 1 };
        b.official.cmp(&a.official).then(order(&a.kind).cmp(&order(&b.kind)))
    });
    Ok(filtered)
}

#[derive(Serialize)]
pub struct UrlStatus {
    pub status: u16,
    pub available: bool,
    pub reason: String,
}

#[derive(Serialize)]
pub struct ItemStatus {
    pub id: u64,
    pub has_imdb: bool,
    pub imdb_id: Option<String>,
    pub has_trailer: bool,
}

#[derive(Deserialize)]
struct ExternalIdsResp {
    #[serde(default)]
    imdb_id: Option<String>,
}

#[tauri::command]
async fn item_status(media_type: String, id: u64, api_key: String) -> Result<ItemStatus, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let ext_url = format!(
        "{}/{}/{}/external_ids?api_key={}",
        TMDB_BASE, media_type, id, api_key
    );
    let vid_url_local = format!(
        "{}/{}/{}/videos?api_key={}&language={}",
        TMDB_BASE, media_type, id, api_key, LANG
    );
    let vid_url_en = format!(
        "{}/{}/{}/videos?api_key={}&language=en-US",
        TMDB_BASE, media_type, id, api_key
    );
    // En paralelo
    let (ext_res, vids_local, vids_en) = tokio::join!(
        fetch_json::<ExternalIdsResp>(&ext_url),
        fetch_json::<VideosResp>(&vid_url_local),
        fetch_json::<VideosResp>(&vid_url_en),
    );
    let imdb_id = ext_res.ok().and_then(|e| e.imdb_id).filter(|s| !s.is_empty());
    let has_imdb = imdb_id.is_some();
    let has_trailer = {
        let combined: Vec<VideoItem> = vids_local
            .map(|r| r.results)
            .unwrap_or_default()
            .into_iter()
            .chain(vids_en.map(|r| r.results).unwrap_or_default())
            .collect();
        combined
            .iter()
            .any(|v| v.site == "YouTube" && (v.kind == "Trailer" || v.kind == "Teaser"))
    };
    Ok(ItemStatus { id, has_imdb, imdb_id, has_trailer })
}

#[tauri::command]
async fn check_url(url: String) -> Result<UrlStatus, String> {
    let initial = url.clone();
    let resp = match client()?.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            // Error de red: NO bloqueamos. Que el user pruebe en el iframe.
            eprintln!("[check_url] red error → asume disponible: {}", e);
            return Ok(UrlStatus { status: 0, available: true, reason: format!("red: {}", e) });
        }
    };
    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();
    eprintln!("[check_url] {} → {} (final: {})", initial, status, final_url);

    // Solo bloqueamos con evidencia clara de "no existe"
    // - 404 Not Found
    // - 410 Gone
    // Otros 4xx/5xx pueden ser bloqueos de Cloudflare/anti-bot: asumimos disponible
    if status == 404 || status == 410 {
        return Ok(UrlStatus { status, available: false, reason: format!("status {}", status) });
    }
    if status >= 500 {
        // 5xx → server caído / WAF, no concluyente
        return Ok(UrlStatus { status, available: true, reason: format!("status {}", status) });
    }
    if status == 403 || status == 401 {
        // Bloqueo bot probable → no concluye nada de la peli
        return Ok(UrlStatus { status, available: true, reason: format!("bot block {}", status) });
    }

    // Redirect: solo cuenta si pierde el imdb_id (típico redirect a home / página genérica)
    let initial_path = url::Url::parse(&initial).ok().map(|u| u.path().to_string()).unwrap_or_default();
    let final_path = url::Url::parse(&final_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
    let orig_imdb = extract_imdb_id(&initial_path);
    let final_imdb = extract_imdb_id(&final_path);
    if let (Some(o), Some(f)) = (&orig_imdb, &final_imdb) {
        if o == f {
            // Misma peli en URL final aunque cambie la ruta (ej: /title/tt → /embed/movie/tt)
            return Ok(UrlStatus { status, available: true, reason: format!("redirect preserva {}", o) });
        }
    }
    if orig_imdb.is_some() && final_imdb.is_none() {
        return Ok(UrlStatus { status, available: false, reason: format!("redirect pierde imdb → {}", final_path) });
    }

    // Body / title (limitado a primeros 8KB para evitar grandes downloads)
    let body = resp.text().await.unwrap_or_default();
    let sample: String = body.chars().take(8000).collect();
    let lower = sample.to_lowercase();
    let title = lower
        .split("<title")
        .nth(1)
        .and_then(|s| s.split('>').nth(1))
        .and_then(|s| s.split("</title>").next())
        .unwrap_or("")
        .trim()
        .to_string();
    // Solo marcamos no disponible si el title MISMO contiene 404 (o 'not found' / 'no encontrad')
    // Y NO contiene un IMDb ID (que sí estaría si fuera la peli buena)
    let has_tt = sample.contains("tt") && sample.matches("tt").count() > 2;
    let title_bad = (title.contains("404")
        || title.contains("not found")
        || title.contains("no encontrad")
        || title.contains("page not found"))
        && !has_tt;
    if title_bad {
        return Ok(UrlStatus { status, available: false, reason: format!("title: {}", title) });
    }
    Ok(UrlStatus { status, available: true, reason: "ok".into() })
}

#[tauri::command]
async fn tmdb_detail(
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<TmdbDetail, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    let extras = if media_type == "tv" {
        "&append_to_response=external_ids,credits"
    } else {
        "&append_to_response=credits"
    };
    let url = format!(
        "{}/{}/{}?api_key={}&language={}{}",
        TMDB_BASE, media_type, id, api_key, LANG, extras
    );
    let raw: DetailRaw = fetch_json(&url).await?;

    let title = raw.title.or(raw.name).unwrap_or_default();
    let date = raw.release_date.or(raw.first_air_date).unwrap_or_default();
    let year = date.split('-').next().unwrap_or("").to_string();
    let imdb_id = raw.imdb_id.or_else(|| raw.external_ids.and_then(|e| e.imdb_id));
    let runtime = raw.runtime.or_else(|| raw.episode_run_time.and_then(|v| v.first().copied()));
    let genres = raw.genres.into_iter().map(|g| g.name).collect();

    // Director (movies) o Creators (tv)
    let mut directors: Vec<PersonMini> = Vec::new();
    if media_type == "movie" {
        if let Some(c) = raw.credits.as_ref() {
            for cw in &c.crew {
                if cw.job.as_deref() == Some("Director") {
                    directors.push(PersonMini {
                        id: cw.id,
                        name: cw.name.clone(),
                        profile_path: cw.profile_path.clone(),
                        character: None,
                        job: Some("Director".into()),
                    });
                }
            }
        }
    } else if let Some(cb) = raw.created_by.as_ref() {
        for cr in cb {
            directors.push(PersonMini {
                id: cr.id,
                name: cr.name.clone(),
                profile_path: cr.profile_path.clone(),
                character: None,
                job: Some("Creador".into()),
            });
        }
    }
    // Cast top 12 ordenado por TMDb (campo "order")
    let mut cast: Vec<PersonMini> = Vec::new();
    if let Some(c) = raw.credits {
        let mut cast_raw = c.cast;
        cast_raw.sort_by_key(|c| c.order.unwrap_or(u32::MAX));
        for cr in cast_raw.into_iter().take(12) {
            cast.push(PersonMini {
                id: cr.id,
                name: cr.name,
                profile_path: cr.profile_path,
                character: cr.character,
                job: None,
            });
        }
    }

    Ok(TmdbDetail {
        id: raw.id,
        media_type,
        title,
        overview: raw.overview,
        poster_path: raw.poster_path,
        backdrop_path: raw.backdrop_path,
        vote_average: raw.vote_average,
        year,
        imdb_id,
        runtime,
        genres,
        directors,
        cast,
    })
}

#[derive(Serialize)]
pub struct PersonFilmography {
    pub id: u64,
    pub title: String,
    pub poster_path: Option<String>,
    pub year: String,
    pub media_type: String,
    pub roles: Vec<String>,
    pub vote_average: f32,
    pub popularity: f32,
}

#[derive(Serialize)]
pub struct PersonInfo {
    pub id: u64,
    pub name: String,
    pub biography: String,
    pub profile_path: Option<String>,
    pub birthday: Option<String>,
    pub deathday: Option<String>,
    pub place_of_birth: Option<String>,
    pub known_for_department: Option<String>,
    pub filmography: Vec<PersonFilmography>,
}

#[derive(Deserialize)]
struct PersonRaw {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    biography: String,
    #[serde(default)]
    profile_path: Option<String>,
    #[serde(default)]
    birthday: Option<String>,
    #[serde(default)]
    deathday: Option<String>,
    #[serde(default)]
    place_of_birth: Option<String>,
    #[serde(default)]
    known_for_department: Option<String>,
    #[serde(default)]
    combined_credits: Option<CombinedCreditsRaw>,
}

#[derive(Deserialize)]
struct CombinedCreditsRaw {
    #[serde(default)]
    cast: Vec<CombinedCreditRaw>,
    #[serde(default)]
    crew: Vec<CombinedCreditRaw>,
}

#[derive(Deserialize)]
struct CombinedCreditRaw {
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    character: Option<String>,
    #[serde(default)]
    job: Option<String>,
    #[serde(default)]
    vote_average: f32,
    #[serde(default)]
    popularity: f32,
}

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tauri::Manager;

async fn scrape_level(
    cli: &reqwest::Client,
    url: &str,
    referer: Option<&str>,
    level: u32,
    report: &mut String,
    visited: &mut std::collections::HashSet<String>,
    max_level: u32,
) {
    if !visited.insert(url.to_string()) {
        report.push_str(&format!("\n[L{} skip ya visitado] {}\n", level, url));
        return;
    }
    let indent = "  ".repeat(level as usize);
    report.push_str(&format!("\n{}--- L{} GET {} ---\n", indent, level, url));
    let mut req = cli.get(url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            report.push_str(&format!("{}ERROR red: {}\n", indent, e));
            return;
        }
    };
    let status = resp.status();
    let final_url = resp.url().to_string();
    let body = resp.text().await.unwrap_or_default();
    report.push_str(&format!("{}status: {}\n", indent, status));
    report.push_str(&format!("{}final_url: {}\n", indent, final_url));
    report.push_str(&format!("{}body_size: {} bytes\n", indent, body.len()));

    // Extraer URLs/iframes/patrones
    let mut found_m3u8: Vec<String> = Vec::new();
    let mut found_vtt: Vec<String> = Vec::new();
    let mut found_iframes: Vec<String> = Vec::new();
    let mut hints: Vec<String> = Vec::new();
    let limit = body.chars().take(80000).collect::<String>();
    // Iframes con regex simple: src="..." dentro de <iframe>
    let lower_full = limit.to_lowercase();
    for (i, _) in lower_full.match_indices("<iframe") {
        let chunk_end = (i + 600).min(limit.len());
        let chunk = &limit[i..chunk_end];
        if let Some(src_start) = chunk.to_lowercase().find("src=") {
            let after = &chunk[src_start + 4..];
            let (quote, after2) = if after.starts_with('"') { ('"', &after[1..]) }
                else if after.starts_with('\'') { ('\'', &after[1..]) }
                else { (' ', after) };
            if let Some(end) = after2.find(quote) {
                let src = &after2[..end];
                if src.starts_with("http") && !found_iframes.iter().any(|x| x == src) {
                    found_iframes.push(src.to_string());
                }
            }
        }
    }
    for line in limit.lines() {
        let l = line.trim();
        if l.is_empty() { continue; }
        for token in l.split(|c: char| c == '"' || c == '\'' || c == ' ' || c == '<' || c == '>' || c == ',' || c == ')' || c == '(') {
            let t = token.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '-' && c != '_' && c != '?' && c != '=' && c != '&' && c != ':' && c != '~' && c != '%');
            if t.starts_with("http") {
                if t.contains(".m3u8") && !found_m3u8.iter().any(|x| x == t) {
                    found_m3u8.push(t.to_string());
                } else if (t.ends_with(".vtt") || t.contains(".vtt?")) && !found_vtt.iter().any(|x| x == t) {
                    found_vtt.push(t.to_string());
                }
            }
        }
        let lower = l.to_lowercase();
        for k in &[
            "subtitle", "caption", "track", "hls.", "hlsplayer", "playerinstance",
            "videojs", "shaka", "jwplayer", "dashjs", "file:", "sources:", "tracks:",
            "video.js", "playlist", ".m3u8", "subtitlelang", "audio_track", "subtitle_track",
            "qualityselectorinit", "kind=", "label=", "srclang=",
        ] {
            if lower.contains(k) && hints.len() < 30 {
                hints.push(format!("[{}] {}", k, l.chars().take(260).collect::<String>()));
            }
        }
    }

    if !found_m3u8.is_empty() {
        report.push_str(&format!("\n{}M3U8 encontrados:\n", indent));
        for u in &found_m3u8 { report.push_str(&format!("{}  {}\n", indent, u)); }
    }
    if !found_vtt.is_empty() {
        report.push_str(&format!("\n{}VTT (subtitulos) encontrados:\n", indent));
        for u in &found_vtt { report.push_str(&format!("{}  {}\n", indent, u)); }
    }
    if !found_iframes.is_empty() {
        report.push_str(&format!("\n{}Iframes anidados:\n", indent));
        for u in &found_iframes { report.push_str(&format!("{}  {}\n", indent, u)); }
    }
    if !hints.is_empty() {
        report.push_str(&format!("\n{}Hints ({}):\n", indent, hints.len()));
        for h in hints.iter().take(20) { report.push_str(&format!("{}  {}\n", indent, h)); }
    }

    // M3U8 master: bajar y parsear
    if let Some(m3u8) = found_m3u8.first() {
        report.push_str(&format!("\n{}--- M3U8 master ---\n", indent));
        match cli.get(m3u8).header("Referer", &final_url).send().await {
            Ok(r) => {
                let st = r.status();
                let playlist = r.text().await.unwrap_or_default();
                report.push_str(&format!("{}m3u8 status: {}\n", indent, st));
                report.push_str(&format!("{}m3u8 size: {} bytes\n", indent, playlist.len()));
                for line in playlist.lines().take(120) {
                    if line.starts_with("#EXT-X-STREAM-INF") || line.starts_with("#EXT-X-MEDIA") || line.starts_with("#EXTINF") {
                        report.push_str(&format!("{}  {}\n", indent, line));
                    } else if !line.starts_with('#') && !line.trim().is_empty() {
                        report.push_str(&format!("{}  url: {}\n", indent, line.trim()));
                    }
                }
            }
            Err(e) => report.push_str(&format!("{}m3u8 fetch error: {}\n", indent, e)),
        }
    }

    // Recurse en iframes hasta max_level
    if level < max_level {
        for ifr in &found_iframes {
            Box::pin(scrape_level(cli, ifr, Some(&final_url), level + 1, report, visited, max_level)).await;
        }
    }
}

#[tauri::command]
async fn sim_key(key: String) -> Result<(), String> {
    use enigo::{Enigo, Key, Keyboard, Settings, Direction};
    eprintln!("[sim_key] intentando '{}'", key);
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[sim_key] enigo init falló: {}", e);
            return Err(format!("enigo init: {}", e));
        }
    };
    let k = match key.to_lowercase().as_str() {
        "space" => Key::Space,
        "k" => Key::Unicode('k'),
        "m" => Key::Unicode('m'),
        "f" => Key::Unicode('f'),
        "escape" => Key::Escape,
        "tab" => Key::Tab,
        "enter" => Key::Return,
        s if s.len() == 1 => Key::Unicode(s.chars().next().unwrap()),
        _ => return Err(format!("tecla desconocida: {}", key)),
    };
    match enigo.key(k, Direction::Click) {
        Ok(_) => { eprintln!("[sim_key] '{}' OK", key); Ok(()) }
        Err(e) => { eprintln!("[sim_key] '{}' falló: {}", key, e); Err(e.to_string()) }
    }
}

#[tauri::command]
async fn sim_mouse_move_click(x: i32, y: i32) -> Result<(), String> {
    use enigo::{Button, Coordinate, Enigo, Mouse, Settings, Direction};
    eprintln!("[sim_mouse] move+click a ({}, {})", x, y);
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| { eprintln!("enigo: {}", e); e.to_string() })?;
    enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    enigo.button(Button::Left, Direction::Click).map_err(|e| e.to_string())?;
    eprintln!("[sim_mouse] click OK");
    Ok(())
}

#[tauri::command]
async fn sim_mouse_wake() -> Result<(), String> {
    use enigo::{Coordinate, Enigo, Mouse, Settings};
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[sim_mouse_wake] enigo init falló: {} — probable Wayland sin uinput/AT-SPI", e);
            return Err(e.to_string());
        }
    };
    // Mover 8px y volver, suficiente para que controles del player aparezcan
    enigo.move_mouse(8, 0, Coordinate::Rel).map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(15));
    enigo.move_mouse(-8, 0, Coordinate::Rel).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn sim_diagnostic() -> Result<String, String> {
    use enigo::{Enigo, Mouse, Settings};
    let mut report = String::new();
    report.push_str(&format!("session_type: {}\n", std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into())));
    report.push_str(&format!("wayland_display: {}\n", std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "(none)".into())));
    report.push_str(&format!("display: {}\n", std::env::var("DISPLAY").unwrap_or_else(|_| "(none)".into())));
    match Enigo::new(&Settings::default()) {
        Ok(e) => {
            report.push_str("enigo init: OK\n");
            match e.location() {
                Ok((x, y)) => report.push_str(&format!("mouse pos: ({}, {})\n", x, y)),
                Err(err) => report.push_str(&format!("mouse pos err: {}\n", err)),
            }
        }
        Err(e) => {
            report.push_str(&format!("enigo init FAIL: {}\n", e));
        }
    }
    eprintln!("[sim_diagnostic]\n{}", report);
    Ok(report)
}

#[tauri::command]
async fn inspect_player(imdb_id: String) -> Result<String, String> {
    if imdb_id.is_empty() {
        return Err("imdb_id vacío".into());
    }
    let cli = client()?;
    let mut report = String::new();
    let mut visited = std::collections::HashSet::new();
    report.push_str(&format!("\n========== INSPECT {} ==========\n", imdb_id));

    let roots = vec![
        format!("https://playimdb.com/es/title/{}/", imdb_id),
        format!("https://playimdb.com/embed/series/{}", imdb_id),
    ];
    for url in &roots {
        scrape_level(&cli, url, None, 0, &mut report, &mut visited, 3).await;
    }

    report.push_str("\n========== FIN INSPECT ==========\n");
    eprintln!("{}", report);
    Ok(report)
}

#[tauri::command]
async fn cache_image(
    app: tauri::AppHandle,
    url: String,
    max_w: Option<u32>,
) -> Result<String, String> {
    if url.is_empty() {
        return Err("url vacía".into());
    }
    // Hash URL para filename estable
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    if let Some(w) = max_w { hasher.update(w.to_le_bytes()); }
    let hash = hex::encode(hasher.finalize());
    let filename = format!("{}.webp", &hash[..16]);

    let cache_dir: PathBuf = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cache dir: {}", e))?
        .join("imgs");
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let path = cache_dir.join(&filename);

    if path.exists() {
        return Ok(path.to_string_lossy().to_string());
    }

    // Download
    let bytes = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("fetch: {}", e))?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    // Decode + resize + WebP encode (en blocking thread, image es CPU-bound)
    let path_clone = path.clone();
    let max_w_val = max_w.unwrap_or(0);
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let img = image::load_from_memory(&bytes).map_err(|e| format!("decode: {}", e))?;
        let resized = if max_w_val > 0 && img.width() > max_w_val {
            let ratio = max_w_val as f32 / img.width() as f32;
            let new_h = (img.height() as f32 * ratio).round() as u32;
            img.resize_exact(max_w_val, new_h, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let rgba = resized.to_rgba8();
        let encoder = image::codecs::webp::WebPEncoder::new_lossless(std::fs::File::create(&path_clone).map_err(|e| e.to_string())?);
        use image::ImageEncoder;
        encoder
            .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("encode webp: {}", e))?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn tmdb_person(id: u64, api_key: String) -> Result<PersonInfo, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    let url = format!(
        "{}/person/{}?api_key={}&language={}&append_to_response=combined_credits",
        TMDB_BASE, id, api_key, LANG
    );
    let raw: PersonRaw = fetch_json(&url).await?;

    let mut filmography: Vec<PersonFilmography> = Vec::new();
    if let Some(c) = raw.combined_credits {
        use std::collections::HashMap;
        let mut by_id: HashMap<(u64, String), PersonFilmography> = HashMap::new();
        for cr in c.cast.into_iter().chain(c.crew.into_iter()) {
            let title = cr.title.or(cr.name).unwrap_or_default();
            if title.is_empty() { continue; }
            let date = cr.release_date.or(cr.first_air_date).unwrap_or_default();
            let year = date.split('-').next().unwrap_or("").to_string();
            let media_type = cr.media_type.unwrap_or_else(|| "movie".into());
            let role = if let Some(ch) = cr.character.as_ref().filter(|s| !s.is_empty()) {
                ch.clone()
            } else if let Some(j) = cr.job.as_ref().filter(|s| !s.is_empty()) {
                j.clone()
            } else {
                continue;
            };
            let key = (cr.id, media_type.clone());
            let entry = by_id.entry(key).or_insert_with(|| PersonFilmography {
                id: cr.id,
                title,
                poster_path: cr.poster_path.clone(),
                year: year.clone(),
                media_type: media_type.clone(),
                roles: Vec::new(),
                vote_average: cr.vote_average,
                popularity: cr.popularity,
            });
            if !entry.roles.contains(&role) {
                entry.roles.push(role);
            }
            if entry.popularity < cr.popularity {
                entry.popularity = cr.popularity;
            }
        }
        filmography = by_id.into_values().collect();
        filmography.sort_by(|a, b| {
            b.popularity.partial_cmp(&a.popularity).unwrap_or(std::cmp::Ordering::Equal)
        });
        filmography.truncate(30);
    }

    Ok(PersonInfo {
        id: raw.id,
        name: raw.name,
        biography: raw.biography,
        profile_path: raw.profile_path,
        birthday: raw.birthday,
        deathday: raw.deathday,
        place_of_birth: raw.place_of_birth,
        known_for_department: raw.known_for_department,
        filmography,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri_plugin_sql::{Migration, MigrationKind};
    let migrations = vec![
        Migration {
            version: 1,
            description: "watch_history",
            sql: "CREATE TABLE IF NOT EXISTS watch_history (
                imdb_id TEXT PRIMARY KEY,
                tmdb_id INTEGER NOT NULL,
                media_type TEXT NOT NULL,
                title TEXT NOT NULL,
                poster_path TEXT,
                watched_seconds INTEGER NOT NULL DEFAULT 0,
                runtime_seconds INTEGER,
                progress_real REAL,
                completed INTEGER NOT NULL DEFAULT 0,
                last_watched INTEGER NOT NULL
            );",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "unavailable",
            sql: "CREATE TABLE IF NOT EXISTS unavailable_items (
                imdb_id TEXT PRIMARY KEY,
                detected_at INTEGER NOT NULL
            );",
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:kutral.db", migrations)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            tmdb_discover,
            tmdb_search,
            tmdb_detail,
            tmdb_genres,
            tmdb_videos,
            check_url,
            item_status,
            tmdb_person,
            cache_image,
            inspect_player,
            sim_key,
            sim_mouse_move_click,
            sim_mouse_wake,
            sim_diagnostic
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
