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
    // Nuevos filtros opcionales (Vera). Si son None, comportamiento previo.
    vote_average_gte: Option<f32>,
    vote_count_gte: Option<u32>,
    primary_release_date_gte: Option<String>,
    primary_release_date_lte: Option<String>,
    with_original_language: Option<String>,
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
    // Si el caller pasa vote_count_gte se usa; si no, mantenemos el
    // default histórico de 100 cuando el sort es por vote_average.
    let vcount = vote_count_gte.unwrap_or_else(|| {
        if sort.starts_with("vote_average") { 100 } else { 0 }
    });
    if vcount > 0 {
        url.push_str(&format!("&vote_count.gte={}", vcount));
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
    if let Some(v) = vote_average_gte {
        url.push_str(&format!("&vote_average.gte={}", v));
    }
    if let Some(d) = primary_release_date_gte.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&primary_release_date.gte={}", d));
    }
    if let Some(d) = primary_release_date_lte.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&primary_release_date.lte={}", d));
    }
    if let Some(l) = with_original_language.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&with_original_language={}", l));
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

// Recomendaciones / similares por película. Usado por el motor de Vera
// para apalancarse en el algoritmo de TMDb (collaborative-ish) en lugar
// de pesos hechos a mano.
//
// `kind` controla el endpoint: "recommendations" (default) o "similar".
// Ambos devuelven el mismo shape TmdbListResp, así que la respuesta es
// uniforme y el cliente decide cuál pedir.
#[tauri::command]
async fn tmdb_recommendations(
    media_type: String,
    id: u64,
    page: u64,
    api_key: String,
    kind: Option<String>,
) -> Result<TmdbListResp, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let endpoint = match kind.as_deref() {
        Some("similar") => "similar",
        _ => "recommendations",
    };
    let url = format!(
        "{}/{}/{}/{}?api_key={}&language={}&page={}",
        TMDB_BASE, media_type, id, endpoint, api_key, LANG, page
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

// ============================================================
// Vera v3 — motor de recomendaciones (placeholders por ahora)
// ============================================================

#[derive(Debug, Serialize)]
pub struct VeraOption {
    id: String,
    label: String,
    description: Option<String>,
}

fn opt(id: &str, label: &str, desc: Option<&str>) -> VeraOption {
    VeraOption {
        id: id.to_string(),
        label: label.to_string(),
        description: desc.map(String::from),
    }
}

#[tauri::command]
fn vera_intent_options() -> Vec<VeraOption> {
    vec![
        opt("mood_match", "Acorde a cómo me siento", Some("Lo que te encaje hoy.")),
        opt("mood_shift", "Sacarme del estado", Some("Algo que te lleve a otro lugar.")),
        opt("background", "Para dejar de fondo", Some("Mientras hacés otra cosa.")),
        opt("marks_you", "Algo que me marque", Some("Una obra que te toque.")),
        opt("light", "Algo liviano", Some("Sin pedirte nada.")),
        opt("surprise", "Sorprendeme", Some("Vera elige una sorpresa.")),
        opt("decide", "Decide por mí", Some("Una sola opción, sin elegir.")),
    ]
}

#[tauri::command]
fn vera_genre_list() -> Vec<VeraOption> {
    [
        ("drama", "Drama"), ("comedy", "Comedia"), ("romance", "Romance"),
        ("action", "Acción"), ("adventure", "Aventura"), ("scifi", "Ciencia ficción"),
        ("fantasy", "Fantasía"), ("horror", "Terror"), ("thriller", "Thriller"),
        ("mystery", "Misterio"), ("crime", "Crimen"), ("war", "Bélico"),
        ("historical", "Histórico"), ("biographical", "Biográfico"),
        ("musical", "Musical"), ("western", "Western"), ("documentary", "Documental"),
        ("animation", "Animación"), ("family", "Familiar"), ("sports", "Deportes"),
        ("reality", "Reality"), ("magic_realism", "Realismo mágico"),
        ("anthology", "Antología"),
    ].iter().map(|(id, label)| opt(id, label, None)).collect()
}

#[tauri::command]
fn vera_theme_list() -> Vec<VeraOption> {
    [
        ("graphic_violence", "Violencia gráfica"),
        ("torture", "Tortura"),
        ("explicit_sex", "Contenido sexual explícito"),
        ("nudity", "Desnudez"),
        ("sexual_abuse", "Abuso sexual"),
        ("child_abuse", "Abuso infantil"),
        ("domestic_violence", "Violencia doméstica"),
        ("animal_violence", "Violencia hacia animales"),
        ("pet_death", "Muerte de mascotas"),
        ("suicide", "Suicidio"),
        ("self_harm", "Autolesión"),
        ("eating_disorder", "Trastornos alimentarios"),
        ("terminal_illness", "Enfermedad terminal"),
        ("child_death", "Muerte de niños"),
        ("pregnancy_loss", "Embarazo o pérdida"),
        ("abortion", "Aborto"),
        ("drugs", "Drogas"),
        ("addiction", "Adicciones"),
        ("strong_language", "Lenguaje vulgar fuerte"),
        ("bullying", "Bullying"),
        ("racism", "Discriminación racial"),
        ("homophobia", "Discriminación homofóbica"),
        ("religion_central", "Religión como tema central"),
        ("partisan_politics", "Política partidaria"),
    ].iter().map(|(id, label)| opt(id, label, None)).collect()
}

// -------- Vera: importador de catálogo desde TMDb --------

// Mapping keyword TMDb (lowercase) → sensitive_themes IDs.
// Match por contains() para tolerar variaciones ("drug abuse", "drug addiction").
fn map_keyword_to_themes(kw_lower: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut push = |v: &'static str| { if !out.contains(&v) { out.push(v); } };

    if kw_lower.contains("suicide") { push("suicide"); }
    if kw_lower.contains("self-harm") || kw_lower.contains("self harm") || kw_lower.contains("cutting") {
        push("self_harm");
    }
    if kw_lower.contains("rape") || kw_lower.contains("sexual assault") || kw_lower.contains("sexual abuse") {
        push("sexual_abuse");
    }
    if kw_lower.contains("child abuse") || kw_lower.contains("pedophilia") { push("child_abuse"); }
    if kw_lower.contains("domestic violence") || kw_lower.contains("domestic abuse") { push("domestic_violence"); }
    if kw_lower.contains("torture") { push("torture"); }
    if kw_lower.contains("animal cruelty") || kw_lower.contains("animal abuse") { push("animal_violence"); }
    if kw_lower.contains("dog death") || kw_lower.contains("pet death") || kw_lower.contains("death of dog") {
        push("pet_death");
    }
    if kw_lower.contains("terminal illness") || kw_lower.contains("cancer")
        || kw_lower.contains("dying patient") || kw_lower.contains("dementia") || kw_lower.contains("alzheimer") {
        push("terminal_illness");
    }
    if kw_lower.contains("child death") || kw_lower.contains("death of child")
        || kw_lower.contains("death of son") || kw_lower.contains("death of daughter") {
        push("child_death");
    }
    if kw_lower.contains("miscarriage") || kw_lower.contains("stillbirth") || kw_lower.contains("pregnancy loss") {
        push("pregnancy_loss");
    }
    if kw_lower.contains("abortion") { push("abortion"); }
    if kw_lower.contains("drug abuse") || kw_lower.contains("drug addiction")
        || kw_lower.contains("heroin") || kw_lower.contains("cocaine") || kw_lower.contains("methamphetamine") {
        push("drugs"); push("addiction");
    }
    if kw_lower.contains("alcoholism") || kw_lower.contains("alcoholic") { push("addiction"); }
    if kw_lower.contains("eating disorder") || kw_lower.contains("anorexia") || kw_lower.contains("bulimia") {
        push("eating_disorder");
    }
    if kw_lower.contains("bullying") { push("bullying"); }
    if kw_lower.contains("racism") || kw_lower.contains("racial discrimination") { push("racism"); }
    if kw_lower.contains("homophobia") { push("homophobia"); }
    if kw_lower.contains("nudity") { push("nudity"); }
    if kw_lower.contains("explicit sex") || kw_lower.contains("erotica") { push("explicit_sex"); }
    if kw_lower.contains("graphic violence") || kw_lower.contains("gore") || kw_lower.contains("bloodbath") {
        push("graphic_violence");
    }

    out
}

// TMDb provider ID → vera platform IDs.
fn map_provider_id(tmdb_id: u64) -> Option<&'static str> {
    match tmdb_id {
        8 => Some("netflix"),
        9 | 10 | 119 => Some("prime"),
        337 => Some("disney"),
        384 | 1899 | 1825 => Some("hbo"),
        2 | 350 => Some("apple"),
        11 => Some("mubi"),
        531 => Some("paramount"),
        619 => Some("star"),
        283 | 1968 => Some("crunchyroll"),
        188 | 192 => Some("youtube"),
        _ => None,
    }
}


fn map_genre_id(media_type: &str, tmdb_id: u64) -> &'static [&'static str] {
    match media_type {
        "movie" => match tmdb_id {
            28 => &["action"],
            12 => &["adventure"],
            16 => &["animation"],
            35 => &["comedy"],
            80 => &["crime"],
            99 => &["documentary"],
            18 => &["drama"],
            10751 => &["family"],
            14 => &["fantasy"],
            36 => &["historical"],
            27 => &["horror"],
            10402 => &["musical"],
            9648 => &["mystery"],
            10749 => &["romance"],
            878 => &["scifi"],
            53 => &["thriller"],
            10752 => &["war"],
            37 => &["western"],
            _ => &[],
        },
        "tv" => match tmdb_id {
            10759 => &["action", "adventure"],
            16 => &["animation"],
            35 => &["comedy"],
            80 => &["crime"],
            99 => &["documentary"],
            18 => &["drama"],
            10751 => &["family"],
            10762 => &["family"],
            9648 => &["mystery"],
            10763 => &["documentary"],
            10764 => &["reality"],
            10765 => &["scifi", "fantasy"],
            10766 => &["drama"],
            10767 => &["reality"],
            10768 => &["war"],
            37 => &["western"],
            _ => &[],
        },
        _ => &[],
    }
}

#[derive(Deserialize)]
struct GenreWithId {
    id: u64,
}

#[derive(Deserialize)]
struct ImportDetailRaw {
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
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
    genres: Vec<GenreWithId>,
    #[serde(default)]
    original_language: Option<String>,
    #[serde(default)]
    popularity: f32,
    // append_to_response=keywords
    #[serde(default)]
    keywords: Option<KeywordsField>,
    // append_to_response=watch/providers (TMDb usa nombre con slash)
    #[serde(default, rename = "watch/providers")]
    watch_providers: Option<WatchProvidersField>,
}

#[derive(Deserialize)]
struct KeywordItem {
    #[serde(default)]
    name: String,
}

// /movie/{id}/keywords devuelve {keywords: [...]}
// /tv/{id}/keywords devuelve {results: [...]}
#[derive(Deserialize)]
struct KeywordsField {
    #[serde(default)]
    keywords: Vec<KeywordItem>,
    #[serde(default)]
    results: Vec<KeywordItem>,
}

#[derive(Deserialize)]
struct WatchProvidersField {
    #[serde(default)]
    results: std::collections::HashMap<String, WatchProvidersRegion>,
}

#[derive(Deserialize)]
struct WatchProvidersRegion {
    #[serde(default)]
    flatrate: Vec<WatchProviderItem>,
    #[serde(default)]
    free: Vec<WatchProviderItem>,
    #[serde(default)]
    ads: Vec<WatchProviderItem>,
}

#[derive(Deserialize)]
struct WatchProviderItem {
    #[serde(default)]
    provider_id: u64,
}

#[derive(Clone, Serialize)]
struct ImportProgress {
    page: u32,
    total_pages: u32,
    item_in_page: u32,
    page_items: u32,
    inserted: u32,
    skipped: u32,
    current: String,
}

#[derive(Serialize)]
pub struct ImportSummary {
    pub inserted: u32,
    pub skipped: u32,
}

fn open_vera_db(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let dir = app.path().app_config_dir().map_err(|e| format!("config_dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
    let path = dir.join("kutral.db");
    rusqlite::Connection::open(&path).map_err(|e| format!("open db {}: {}", path.display(), e))
}

#[tauri::command]
async fn vera_import_catalog(
    window: tauri::Window,
    app: tauri::AppHandle,
    api_key: String,
    media_type: String,
    pages: u32,
    watch_region: Option<String>,
) -> Result<ImportSummary, String> {
    use tauri::Emitter;
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    if pages == 0 || pages > 50 {
        return Err("pages debe ser 1..=50".into());
    }
    let region = watch_region.unwrap_or_else(|| "CL".into()).to_uppercase();

    let cli = client()?;
    let mut inserted: u32 = 0;
    let mut skipped: u32 = 0;
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let extras = if media_type == "tv" {
        "&append_to_response=external_ids,keywords,watch/providers"
    } else {
        "&append_to_response=keywords,watch/providers"
    };

    for page in 1..=pages {
        let discover_url = format!(
            "{}/discover/{}?api_key={}&language={}&page={}&sort_by=popularity.desc&include_adult=false",
            TMDB_BASE, media_type, api_key, LANG, page
        );
        let list: TmdbListResp = match fetch_json(&discover_url).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[vera_import] discover page {} fail: {}", page, e);
                continue;
            }
        };
        let page_items = list.results.len() as u32;

        for (idx, item) in list.results.iter().enumerate() {
            let label = item
                .title
                .clone()
                .or_else(|| item.name.clone())
                .unwrap_or_else(|| format!("tmdb:{}", item.id));

            let _ = window.emit(
                "vera:import:progress",
                ImportProgress {
                    page,
                    total_pages: pages,
                    item_in_page: idx as u32 + 1,
                    page_items,
                    inserted,
                    skipped,
                    current: label.clone(),
                },
            );

            let url = format!(
                "{}/{}/{}?api_key={}&language={}{}",
                TMDB_BASE, media_type, item.id, api_key, LANG, extras
            );
            let raw: ImportDetailRaw = match cli.get(&url).send().await {
                Ok(r) => {
                    let body = r.text().await.unwrap_or_default();
                    match serde_json::from_str(&body) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[vera_import] parse {}: {}", item.id, e);
                            skipped += 1;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[vera_import] fetch {}: {}", item.id, e);
                    skipped += 1;
                    continue;
                }
            };

            let imdb_id = raw.imdb_id.clone().or_else(|| raw.external_ids.and_then(|e| e.imdb_id));
            let Some(imdb_id) = imdb_id.filter(|s| !s.is_empty()) else {
                skipped += 1;
                continue;
            };

            let title = raw.title.or(raw.name).unwrap_or_default();
            let date = raw.release_date.or(raw.first_air_date).unwrap_or_default();
            let year: Option<i32> = date.split('-').next().and_then(|s| s.parse().ok());
            let runtime = raw.runtime.or_else(|| raw.episode_run_time.and_then(|v| v.first().copied()));

            let mut genres: Vec<&str> = Vec::new();
            for g in &raw.genres {
                for v in map_genre_id(&media_type, g.id) {
                    if !genres.contains(v) { genres.push(v); }
                }
            }
            let genres_json = serde_json::to_string(&genres).unwrap_or_else(|_| "[]".into());
            let languages_json = match raw.original_language.as_deref() {
                Some(l) if !l.is_empty() => serde_json::to_string(&vec![l]).unwrap_or_else(|_| "[]".into()),
                _ => "[]".into(),
            };

            // Keywords → sensitive_themes
            let mut themes: Vec<&'static str> = Vec::new();
            if let Some(kw) = raw.keywords.as_ref() {
                let items = if !kw.keywords.is_empty() { &kw.keywords } else { &kw.results };
                for k in items {
                    let lower = k.name.to_lowercase();
                    for t in map_keyword_to_themes(&lower) {
                        if !themes.contains(&t) { themes.push(t); }
                    }
                }
            }
            let themes_json = serde_json::to_string(&themes).unwrap_or_else(|_| "[]".into());

            // watch/providers[region] → platforms
            let mut plats: Vec<&'static str> = Vec::new();
            if let Some(wp) = raw.watch_providers.as_ref() {
                if let Some(rg) = wp.results.get(&region) {
                    for src in [&rg.flatrate, &rg.free, &rg.ads] {
                        for p in src {
                            if let Some(id) = map_provider_id(p.provider_id) {
                                if !plats.contains(&id) { plats.push(id); }
                            }
                        }
                    }
                }
            }
            let plats_json = serde_json::to_string(&plats).unwrap_or_else(|_| "[]".into());

            let conn = match open_vera_db(&app) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[vera_import] open db: {}", e);
                    skipped += 1;
                    continue;
                }
            };
            let res = conn.execute(
                "INSERT INTO vera_titles
                    (imdb_id, tmdb_id, title, year, runtime_min, format, genres,
                     tone_tags, use_tags, sensitive_themes, age_min, country,
                     languages, platforms, popularity, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '[]', '[]', ?8, 0, NULL, ?9, ?10, ?11, ?12)
                 ON CONFLICT(imdb_id) DO UPDATE SET
                    tmdb_id = ?2, title = ?3, year = ?4, runtime_min = ?5,
                    format = ?6, genres = ?7, sensitive_themes = ?8,
                    languages = ?9, platforms = ?10, popularity = ?11, updated_at = ?12",
                rusqlite::params![
                    imdb_id,
                    raw.id as i64,
                    title,
                    year,
                    runtime,
                    media_type,
                    genres_json,
                    themes_json,
                    languages_json,
                    plats_json,
                    raw.popularity as f64,
                    now,
                ],
            );
            match res {
                Ok(_) => inserted += 1,
                Err(e) => {
                    eprintln!("[vera_import] insert {}: {}", imdb_id, e);
                    skipped += 1;
                }
            }
        }
    }

    let _ = window.emit(
        "vera:import:progress",
        ImportProgress {
            page: pages,
            total_pages: pages,
            item_in_page: 0,
            page_items: 0,
            inserted,
            skipped,
            current: "fin".into(),
        },
    );

    Ok(ImportSummary { inserted, skipped })
}

#[derive(Serialize)]
pub struct CatalogCount {
    pub total: i64,
    pub movies: i64,
    pub tv: i64,
}

#[tauri::command]
fn vera_catalog_count(app: tauri::AppHandle) -> Result<CatalogCount, String> {
    let conn = open_vera_db(&app)?;
    let mut stmt = conn
        .prepare("SELECT format, COUNT(*) FROM vera_titles GROUP BY format")
        .map_err(|e| e.to_string())?;
    let mut movies: i64 = 0;
    let mut tv: i64 = 0;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    for r in rows {
        let (fmt, n) = r.map_err(|e| e.to_string())?;
        match fmt.as_str() {
            "movie" => movies = n,
            "tv" => tv = n,
            _ => {}
        }
    }
    Ok(CatalogCount { total: movies + tv, movies, tv })
}

#[tauri::command]
fn vera_platform_list() -> Vec<VeraOption> {
    [
        ("netflix", "Netflix"), ("prime", "Prime Video"), ("disney", "Disney+"),
        ("hbo", "Max"), ("apple", "Apple TV+"), ("mubi", "Mubi"),
        ("paramount", "Paramount+"), ("star", "Star+"), ("crunchyroll", "Crunchyroll"),
        ("youtube", "YouTube"), ("free_tv", "TV abierta"),
    ].iter().map(|(id, label)| opt(id, label, None)).collect()
}

// ============================================================
// RealDebrid — OAuth Device Code flow
// ============================================================

const RD_CLIENT_ID: &str = "X245A4XAIBGVM"; // public open-source client_id
const RD_GRANT_DEVICE: &str = "http://oauth.net/grant_type/device/1.0";

#[derive(Serialize, Deserialize, Clone)]
pub struct RdDeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct RdCredentialsResp {
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct RdTokenResp {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Serialize)]
pub struct RdLinked {
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
    pub expires_in: u64,
}

#[tauri::command]
async fn rd_device_start() -> Result<RdDeviceStart, String> {
    let url = format!(
        "https://api.real-debrid.com/oauth/v2/device/code?client_id={}&new_credentials=yes",
        RD_CLIENT_ID
    );
    let cli = client()?;
    let resp = cli.get(&url).send().await.map_err(|e| format!("red: {}", e))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("RD {}: {}", st, body));
    }
    let v: RdDeviceStart = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    Ok(v)
}

#[tauri::command]
async fn rd_device_poll(
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<RdLinked, String> {
    if device_code.is_empty() {
        return Err("device_code vacío".into());
    }
    let cli = client()?;
    let poll_every = std::cmp::max(interval, 3);
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(expires_in.max(60));

    let creds_url = format!(
        "https://api.real-debrid.com/oauth/v2/device/credentials?client_id={}&code={}",
        RD_CLIENT_ID, device_code
    );

    loop {
        if std::time::Instant::now() >= deadline {
            return Err("código expirado".into());
        }
        tokio::time::sleep(std::time::Duration::from_secs(poll_every)).await;

        let resp = cli.get(&creds_url).send().await.map_err(|e| format!("red: {}", e))?;
        if !resp.status().is_success() {
            continue;
        }
        let body = resp.text().await.unwrap_or_default();
        let Ok(creds) = serde_json::from_str::<RdCredentialsResp>(&body) else {
            continue;
        };

        let form = [
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("code", device_code.as_str()),
            ("grant_type", RD_GRANT_DEVICE),
        ];
        let tok = cli
            .post("https://api.real-debrid.com/oauth/v2/token")
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("red token: {}", e))?;
        if !tok.status().is_success() {
            let st = tok.status();
            let body = tok.text().await.unwrap_or_default();
            return Err(format!("RD token {}: {}", st, body));
        }
        let t: RdTokenResp = tok.json().await.map_err(|e| format!("parse token: {}", e))?;
        return Ok(RdLinked {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            client_id: creds.client_id,
            client_secret: creds.client_secret,
            expires_in: t.expires_in,
        });
    }
}

// ============================================================
// Kütral OS — gestión de red (nmcli)
// ============================================================

#[derive(Serialize)]
pub struct WifiNetwork {
    ssid: String,
    signal: u8,
    secured: bool,
    in_use: bool,
}

#[derive(Serialize)]
pub struct WifiStatus {
    online: bool,
    connected_ssid: Option<String>,
}

fn parse_nmcli_line(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                cur.push(next);
                chars.next();
                continue;
            }
        }
        if c == ':' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

#[tauri::command]
async fn wifi_status() -> Result<WifiStatus, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(WifiStatus { online: true, connected_ssid: None }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let online = Command::new("nmcli")
            .args(["-t", "-f", "STATE", "general", "status"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "connected")
            .unwrap_or(false);
        let mut connected_ssid: Option<String> = None;
        if let Ok(o) = Command::new("nmcli")
            .args(["-t", "-f", "ACTIVE,SSID", "device", "wifi", "list"])
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let p = parse_nmcli_line(line);
                if p.len() >= 2 && p[0] == "yes" {
                    let s = p[1].trim();
                    if !s.is_empty() && s != "--" {
                        connected_ssid = Some(s.to_string());
                        break;
                    }
                }
            }
        }
        Ok(WifiStatus { online, connected_ssid })
    }
}

#[tauri::command]
async fn wifi_scan() -> Result<Vec<WifiNetwork>, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(Vec::new()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let _ = Command::new("nmcli").args(["device", "wifi", "rescan"]).output();
        let out = Command::new("nmcli")
            .args(["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list"])
            .output()
            .map_err(|e| format!("nmcli: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let mut nets: Vec<WifiNetwork> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in body.lines() {
            let p = parse_nmcli_line(line);
            if p.len() < 4 { continue; }
            let in_use = p[0].trim() == "*";
            let ssid = p[1].trim().to_string();
            if ssid.is_empty() || ssid == "--" { continue; }
            if !seen.insert(ssid.clone()) { continue; }
            let signal: u8 = p[2].trim().parse().unwrap_or(0);
            let sec = p[3].trim();
            let secured = !sec.is_empty() && sec != "--";
            nets.push(WifiNetwork { ssid, signal, secured, in_use });
        }
        nets.sort_by(|a, b| b.signal.cmp(&a.signal));
        Ok(nets)
    }
}

#[tauri::command]
async fn wifi_connect(ssid: String, password: Option<String>) -> Result<(), String> {
    if ssid.is_empty() { return Err("ssid vacío".into()); }
    #[cfg(not(target_os = "linux"))]
    { let _ = password; return Err("solo soportado en Linux".into()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let mut args: Vec<String> = vec![
            "device".into(), "wifi".into(), "connect".into(), ssid,
        ];
        if let Some(p) = password.filter(|s| !s.is_empty()) {
            args.push("password".into());
            args.push(p);
        }
        let out = Command::new("nmcli")
            .args(&args)
            .output()
            .map_err(|e| format!("nmcli: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

// ============================================================
// Kütral OS — detección de OS host
// ============================================================

#[derive(Serialize)]
pub struct OsInfo {
    is_kutral_os: bool,
    platform: &'static str,
    version: Option<String>,
}

fn detect_kutral_os() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("KUTRAL_OS").ok().as_deref() == Some("1") {
            return true;
        }
        if std::path::Path::new("/etc/kutral-os-release").exists() {
            return true;
        }
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            for line in contents.lines() {
                if line.trim() == "ID=kutral-os" {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[tauri::command]
fn os_info() -> OsInfo {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    };
    OsInfo {
        is_kutral_os: detect_kutral_os(),
        platform,
        version: std::env::var("KUTRAL_OS_VERSION").ok(),
    }
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
        Migration {
            version: 3,
            description: "vera_v3_schema",
            sql: "
                CREATE TABLE IF NOT EXISTS vera_setup (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    mode_io TEXT NOT NULL,
                    depth_profile TEXT NOT NULL,
                    languages_known TEXT NOT NULL DEFAULT '[]',
                    dub_pref TEXT NOT NULL,
                    platforms TEXT NOT NULL DEFAULT '[]',
                    excluded_genres TEXT NOT NULL DEFAULT '[]',
                    excluded_themes TEXT NOT NULL DEFAULT '[]',
                    personality TEXT NOT NULL DEFAULT 'warm',
                    completed_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_titles (
                    imdb_id TEXT PRIMARY KEY,
                    tmdb_id INTEGER,
                    title TEXT NOT NULL,
                    year INTEGER,
                    runtime_min INTEGER,
                    format TEXT NOT NULL,
                    genres TEXT NOT NULL DEFAULT '[]',
                    tone_tags TEXT NOT NULL DEFAULT '[]',
                    use_tags TEXT NOT NULL DEFAULT '[]',
                    sensitive_themes TEXT NOT NULL DEFAULT '[]',
                    age_min INTEGER NOT NULL DEFAULT 0,
                    country TEXT,
                    languages TEXT NOT NULL DEFAULT '[]',
                    platforms TEXT NOT NULL DEFAULT '[]',
                    popularity REAL DEFAULT 0,
                    updated_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_vera_titles_format ON vera_titles(format);
                CREATE INDEX IF NOT EXISTS idx_vera_titles_age ON vera_titles(age_min);

                CREATE TABLE IF NOT EXISTS vera_responses (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    intent TEXT NOT NULL,
                    format_pref TEXT,
                    time_available TEXT,
                    company TEXT,
                    ages TEXT,
                    ages_attentive INTEGER,
                    tones TEXT NOT NULL DEFAULT '[]',
                    session_excluded_genres TEXT NOT NULL DEFAULT '[]',
                    session_excluded_themes TEXT NOT NULL DEFAULT '[]',
                    created_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_weights (
                    tag TEXT PRIMARY KEY,
                    weight REAL NOT NULL DEFAULT 1.0,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vera_templates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    imdb_id TEXT NOT NULL,
                    context TEXT NOT NULL,
                    personality TEXT NOT NULL,
                    text TEXT NOT NULL,
                    FOREIGN KEY (imdb_id) REFERENCES vera_titles(imdb_id)
                );

                CREATE INDEX IF NOT EXISTS idx_vera_templates_lookup
                    ON vera_templates(imdb_id, context, personality);

                CREATE TABLE IF NOT EXISTS vera_feedback (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    imdb_id TEXT NOT NULL,
                    response_id INTEGER,
                    rating INTEGER NOT NULL,
                    finished INTEGER,
                    why_not TEXT,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY (imdb_id) REFERENCES vera_titles(imdb_id)
                );
            ",
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            tmdb_recommendations,
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
            sim_diagnostic,
            vera_intent_options,
            vera_genre_list,
            vera_theme_list,
            vera_platform_list,
            vera_import_catalog,
            vera_catalog_count,
            os_info,
            wifi_status,
            wifi_scan,
            wifi_connect,
            rd_device_start,
            rd_device_poll
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
