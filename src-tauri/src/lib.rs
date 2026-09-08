use serde::{Deserialize, Serialize};

mod anilist;
mod anime_web;
mod awards;
mod creds;
mod emu;
mod kitsu;
mod kodios;
mod net;
mod opensubtitles;
mod probe;
mod rd;
mod player;
#[cfg(target_os = "linux")]
mod mpv_embed;
mod screening;
mod torrent;
mod webserver;

const TMDB_BASE: &str = "https://api.themoviedb.org/3";
const LANG: &str = "es-ES";

/// Log de depuración desde el frontend a la consola de Tauri (stderr).
#[tauri::command]
fn ui_log(msg: String) {
    eprintln!("[ui] {msg}");
}

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Cliente HTTP compartido para TMDb/OMDb/imágenes.
///
/// `reqwest::Client` es un Arc por dentro: clonarlo reusa el pool de
/// conexiones (keep-alive + TLS ya negociado). Construir uno por request
/// tiraba el pool cada vez → un handshake TLS completo por cada card del grid.
///
/// El `timeout` es igual de importante: sin él, un request estancado (CDN que
/// acepta la conexión y no responde) no termina NUNCA. El frontend deja
/// `listLoading = true` para siempre y el grid queda congelado sin reintentar.
fn client() -> Result<reqwest::Client, String> {
    static HTTP: std::sync::OnceLock<Result<reqwest::Client, String>> = std::sync::OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(BROWSER_UA)
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(5))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| e.to_string())
    })
    .clone()
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
    // Los tres campos de abajo TMDb ya los manda en /discover y /recommendations.
    // Se agregan para que Vera pueda armar y rankear el pool con la respuesta
    // del listado, sin un /detail por título (eran ~60 requests por ronda).
    #[serde(default)]
    pub genre_ids: Vec<u64>,
    #[serde(default)]
    pub vote_count: Option<u32>,
    #[serde(default)]
    pub popularity: Option<f32>,
    #[serde(default)]
    pub original_language: Option<String>,
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
    pub images: Vec<String>,
    #[serde(default)]
    pub number_of_seasons: Option<u32>,
    #[serde(default)]
    pub seasons: Vec<SeasonMini>,
    #[serde(default)]
    pub tagline: Option<String>,
    #[serde(default)]
    pub original_title: Option<String>,
    #[serde(default)]
    pub original_language: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub budget: Option<u64>,
    #[serde(default)]
    pub revenue: Option<u64>,
    #[serde(default)]
    pub vote_count: Option<u32>,
    #[serde(default)]
    pub popularity: Option<f32>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub production_companies: Vec<String>,
    #[serde(default)]
    pub production_countries: Vec<String>,
    #[serde(default)]
    pub spoken_languages: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Temas sensibles derivados de las keywords TMDb con el MISMO mapeo que
    /// usa `vera_import_catalog` (`map_keyword_to_themes`). Viaja en el detail
    /// para que Vera pueda filtrar por trigger warnings también en títulos que
    /// no están en el catálogo local. Vacío = sin coincidencias (no "sin datos").
    #[serde(default)]
    pub sensitive_themes: Vec<String>,
}

#[derive(Serialize)]
pub struct SeasonMini {
    pub season_number: u32,
    pub episode_count: u32,
    pub name: String,
    pub air_date: Option<String>,
    pub poster_path: Option<String>,
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
    #[serde(default)]
    images: Option<ImagesRaw>,
    #[serde(default)]
    number_of_seasons: Option<u32>,
    #[serde(default)]
    seasons: Option<Vec<SeasonRaw>>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    original_name: Option<String>,
    #[serde(default)]
    original_language: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    budget: Option<u64>,
    #[serde(default)]
    revenue: Option<u64>,
    #[serde(default)]
    vote_count: Option<u32>,
    #[serde(default)]
    popularity: Option<f32>,
    #[serde(default)]
    production_companies: Vec<NamedRaw>,
    #[serde(default)]
    production_countries: Vec<NamedRaw>,
    #[serde(default)]
    spoken_languages: Vec<NamedRaw>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    keywords: Option<KeywordsField>,
}

#[derive(Deserialize)]
struct NamedRaw {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct SeasonRaw {
    #[serde(default)]
    season_number: u32,
    #[serde(default)]
    episode_count: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct ImagesRaw {
    #[serde(default)]
    backdrops: Vec<ImageFile>,
}

#[derive(Deserialize)]
struct ImageFile {
    #[serde(default)]
    file_path: Option<String>,
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

// ========================================================================
// Sinopsis en español para anime
// ========================================================================

/// Orden de preferencia para la sinopsis. `es-MX` primero a propósito: la
/// marca escribe en español NEUTRO y las traducciones `es-ES` de TMDb están
/// hechas para España (aparece algún "vosotros"). Después va España, después
/// cualquier variante, y recién ahí quien llama se queda con el inglés.
const OVERVIEW_ES_PREF: &[&str] = &["MX", "ES", "419", "AR", "CO", "US"];

/// Sinopsis en español para un anime, vía su ficha equivalente en TMDb.
///
/// Ni AniList ni Kitsu tienen texto en español (sus `titles` traen locales,
/// pero la sinopsis es siempre inglés). El `themoviedb_id` sale de ani.zip, en
/// la MISMA llamada que ya se hace para episodios e IDs cruzados.
///
/// Una sola petición: `language=es-MX` resuelve el caso común, y
/// `append_to_response=translations` trae el resto de variantes por si esa no
/// existe — TMDb devuelve `overview` VACÍO cuando no hay traducción, no un
/// error, así que sin el append no habría forma de distinguirlo.
///
/// Devuelve `None` si no hay español: quien llama conserva la sinopsis original.
pub(crate) async fn tmdb_overview_es(
    tmdb_type: Option<&str>,
    tmdb_id: u64,
    api_key: &str,
) -> Option<String> {
    if api_key.is_empty() || tmdb_id == 0 {
        return None;
    }
    // ani.zip marca "MOVIE"/"TV"; ante la duda, serie (la mayoría del catálogo).
    let kind = if tmdb_type.is_some_and(|t| t.eq_ignore_ascii_case("movie")) {
        "movie"
    } else {
        "tv"
    };
    let url = format!(
        "{}/{}/{}?api_key={}&language=es-MX&append_to_response=translations",
        TMDB_BASE, kind, tmdb_id, api_key
    );
    let v: serde_json::Value = client().ok()?.get(&url).send().await.ok()?.json().await.ok()?;

    let clean = |x: &serde_json::Value| {
        x.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
    };
    // Camino rápido: la propia respuesta ya vino en es-MX.
    if let Some(o) = clean(&v["overview"]) {
        return Some(o);
    }
    // Si no, elegimos entre las traducciones al español que haya.
    let list = v["translations"]["translations"].as_array()?;
    let es: Vec<&serde_json::Value> =
        list.iter().filter(|t| t["iso_639_1"] == "es").collect();
    for region in OVERVIEW_ES_PREF {
        if let Some(t) = es.iter().find(|t| t["iso_3166_1"] == *region) {
            if let Some(o) = clean(&t["data"]["overview"]) {
                return Some(o);
            }
        }
    }
    // Cualquier variante de español con texto.
    es.iter().find_map(|t| clean(&t["data"]["overview"]))
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
    // B5: keywords TMDb (ej. 158718 para LGBT+). Acepta coma=AND o pipe=OR.
    with_keywords: Option<String>,
    // Solo movie: tipos de release TMDb (1 premiere, 2/3 cine, 4 digital,
    // 5 físico, 6 TV). Pipe=OR. Con esto seteado, las fechas filtran sobre
    // release_date (la fecha de ESOS tipos), no primary_release_date.
    with_release_type: Option<String>,
) -> Result<TmdbListResp, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let release_type = with_release_type
        .filter(|s| !s.is_empty() && media_type == "movie");

    let sort = sort_by.unwrap_or_else(|| "popularity.desc".into());
    // El sort tiene que mirar el MISMO campo que filtra la fecha (ver
    // `date_field` abajo). Con with_release_type el filtro es sobre
    // `release_date`, así que ordenar por `primary_release_date` mezclaba dos
    // campos: una peli con estreno principal en 2027 pero con cualquier fecha
    // vieja de tipo 6 (TV) pasaba el `.lte=hoy` y, ordenada por su fecha
    // primaria futura, se iba al TOPE de la primera página. Medido: el primer
    // resultado de "Más recientes" era un estreno de 2027-03-08.
    let sort = match release_type.is_some() && sort.starts_with("primary_release_date") {
        true => sort.replacen("primary_release_date", "release_date", 1),
        false => sort,
    };

    let mut url = format!(
        "{}/discover/{}?api_key={}&language={}&page={}&sort_by={}&include_adult=false",
        TMDB_BASE, media_type, api_key, LANG, page, sort
    );
    // Piso de votos. TMDb es editable por usuarios y su cola larga son fichas
    // placeholder: sin póster, sin votos, muchas sin estrenar de verdad. Los
    // órdenes por popularidad o votos se filtran solos, pero "Más recientes",
    // "Más antiguas" y A→Z/Z→A entregaban esa cola cruda (medido: 15 de 20
    // resultados con CERO votos en Z→A, y títulos que eran literalmente "!").
    //
    // 10 es el punto donde el ruido desaparece sin perder frescura: deja pasar
    // estrenos de los últimos días, que legítimamente tienen pocos votos. Con
    // 50 ya se perdía la última semana entera.
    let vcount = vote_count_gte.unwrap_or_else(|| {
        if sort.starts_with("vote_average") { 100 } else { 10 }
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
    // Campo de fecha según contexto: tv usa first_air_date; movie con
    // with_release_type debe usar release_date (TMDb filtra sobre las fechas
    // de esos tipos); movie a secas usa primary_release_date.
    let date_field = if media_type == "tv" {
        "first_air_date"
    } else if release_type.is_some() {
        "release_date"
    } else {
        "primary_release_date"
    };
    if let Some(d) = primary_release_date_gte.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&{}.gte={}", date_field, d));
    }
    if let Some(d) = primary_release_date_lte.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&{}.lte={}", date_field, d));
    }
    if let Some(t) = release_type {
        url.push_str(&format!("&with_release_type={}", t));
    }
    if let Some(l) = with_original_language.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&with_original_language={}", l));
    }
    if let Some(k) = with_keywords.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&with_keywords={}", k));
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

// ---------- Trailers ----------

/// Quita acentos de los caracteres latinos comunes. No cubre todo Unicode:
/// solo lo que aparece en títulos de TMDb en es-ES / en-US.
fn deaccent(c: char) -> Option<&'static str> {
    Some(match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => "a",
        'é' | 'è' | 'ë' | 'ê' => "e",
        'í' | 'ì' | 'ï' | 'î' => "i",
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => "o",
        'ú' | 'ù' | 'ü' | 'û' => "u",
        'ñ' => "n",
        'ç' => "c",
        'ø' => "o",
        'æ' => "ae",
        'ß' => "ss",
        _ => return None,
    })
}

/// Normaliza un título para comparar: minúsculas, sin acentos, sin puntuación,
/// espacios colapsados. "Amélie: Edición Especial" → "amelie edicion especial".
fn norm_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.to_lowercase().chars() {
        let mapped = deaccent(ch);
        let piece: &str = match mapped {
            Some(m) => m,
            None if ch.is_alphanumeric() => {
                out.push(ch);
                prev_space = false;
                continue;
            }
            None => {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
                continue;
            }
        };
        out.push_str(piece);
        prev_space = false;
    }
    out.trim().to_string()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 1.0 = idénticos, 0.0 = nada en común.
fn similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let d = levenshtein(&ac, &bc) as f32;
    1.0 - d / ac.len().max(bc.len()) as f32
}

/// ¿El video de YouTube se puede embeber?
///
/// oEmbed responde 200 solo si existe Y permite embed. 401/403 = embed
/// deshabilitado por el dueño (caso típico de trailers de distribuidoras, que
/// dentro del webview aparecen como "error 153"), 404 = borrado o privado.
/// Sin esta comprobación el iframe carga una pantalla negra y la app cree que
/// el trailer se está viendo.
/// (existe, permite embed). 200 = ambos; 401/403 = existe pero el dueño
/// bloqueó el embed (yt-dlp igual lo puede reproducir); el resto = borrado,
/// privado o key inválida.
async fn yt_probe(key: &str) -> (bool, bool) {
    let url = format!(
        "https://www.youtube.com/oembed?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{}&format=json",
        urlencoding::encode(key)
    );
    let c = match client() {
        Ok(c) => c,
        // Sin cliente HTTP no podemos descartar nada: lo damos por existente.
        Err(_) => return (true, false),
    };
    match c.get(&url).send().await {
        Ok(r) if r.status().is_success() => (true, true),
        Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => (true, false),
        Ok(_) => (false, false),
        // Fallo de red: no asumir que el video no existe.
        Err(_) => (true, false),
    }
}

#[derive(Serialize)]
pub struct TrailerKey {
    pub key: String,
    pub embeddable: bool,
}

/// Primer trailer de TMDb que además se puede embeber.
///
/// Si ninguno de los candidatos pasa la prueba devuelve el primero con
/// `embeddable: false`: el frontend lo abre en el navegador externo en vez de
/// mostrar un iframe roto.
#[tauri::command]
async fn tmdb_trailer_key(
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<Option<TrailerKey>, String> {
    let vids = tmdb_videos(media_type, id, api_key).await?;
    let mut first: Option<String> = None;
    // Tope de 5: cada comprobación es un request extra y la lista viene
    // ordenada por relevancia (oficial + Trailer antes que Teaser). Se salta
    // solo lo borrado: un video sin permiso de embed sigue sirviendo, porque
    // el camino normal de reproducción es yt-dlp, no el iframe.
    for v in vids.iter().take(5) {
        if first.is_none() {
            first = Some(v.key.clone());
        }
        let (exists, embeddable) = yt_probe(&v.key).await;
        if exists {
            return Ok(Some(TrailerKey { key: v.key.clone(), embeddable }));
        }
    }
    Ok(first.map(|key| TrailerKey { key, embeddable: false }))
}

/// Ruta del binario yt-dlp: vendor/ del bundle primero, PATH después.
fn ytdlp_bin(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    #[cfg(windows)]
    let exe = "yt-dlp.exe";
    #[cfg(not(windows))]
    let exe = "yt-dlp";

    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join(exe));
    }
    cands.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join(exe));
        }
    }
    for c in cands {
        if c.exists() {
            return c.to_string_lossy().into_owned();
        }
    }
    exe.to_string()
}

/// ¿yt-dlp puede resolver este video? (existe, no tiene bloqueo de edad/DRM)
///
/// No devolvemos la URL: los trailers de YouTube ya casi nunca traen formato
/// progresivo (video y audio van por streams DASH separados), así que un
/// `<video>` del webview no puede reproducirlos aunque le demos la URL. Quien
/// los junta es mpv vía ytdl_hook, y para eso le pasamos la URL de YouTube tal
/// cual. Esta comprobación solo sirve para saber si vale la pena abrir mpv o
/// hay que mostrar el QR.
#[tauri::command]
async fn yt_playable(app: tauri::AppHandle, key: String) -> Result<bool, String> {
    let k = key.trim().to_string();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Ok(false);
    }
    let bin = ytdlp_bin(&app);
    let url = format!("https://www.youtube.com/watch?v={}", k);

    let mut cmd = tokio::process::Command::new(&bin);
    let fut = cmd
        .args([
            "--no-playlist",
            "--no-warnings",
            "--socket-timeout", "10",
            "-f", "bv*+ba/b",
            "-g",
            &url,
        ])
        .kill_on_drop(true)
        .output();
    // Tope duro: si yt-dlp se cuelga, el menú no puede quedarse esperando.
    let out = match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(r) => r.map_err(|e| format!("yt-dlp: {}", e))?,
        Err(_) => return Err("yt-dlp: timeout".into()),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("yt-dlp: {}", err.lines().next().unwrap_or("").trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim().starts_with("http")))
}

#[derive(Serialize)]
pub struct AppleTrailer {
    pub url: String,
    pub title: String,
    pub year: Option<String>,
}

/// Trailer desde iTunes (mp4 directo, sin restricciones de embed).
///
/// iTunes solo se puede consultar por texto — no tiene IDs de TMDb/IMDb — así
/// que el match tiene que ser estricto o devuelve cualquier cosa: su búsqueda
/// es difusa y para un título que no está en el catálogo US (cine coreano,
/// indio, europeo) responde 25 películas sin relación. Antes se aceptaba "el
/// primer candidato" y por eso salía el trailer de otra película.
///
/// Reglas: título normalizado con similitud >= 0.85 y año dentro de ±1. Sin
/// año confiable se exige similitud >= 0.95. Si nada cumple → `None`.
#[tauri::command]
async fn apple_trailer(
    title: String,
    original_title: String,
    year: String,
    media_type: String,
) -> Result<Option<AppleTrailer>, String> {
    #[derive(Deserialize)]
    struct Resp {
        results: Vec<Item>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Item {
        kind: Option<String>,
        track_name: Option<String>,
        preview_url: Option<String>,
        release_date: Option<String>,
    }

    // El título original va primero: iTunes US indexa en inglés / idioma
    // original, no con el título en español de TMDb.
    let mut queries: Vec<String> = Vec::new();
    for cand in [original_title.trim(), title.trim()] {
        if !cand.is_empty() && !queries.iter().any(|q| q.eq_ignore_ascii_case(cand)) {
            queries.push(cand.to_string());
        }
    }
    if queries.is_empty() {
        return Ok(None);
    }

    // Una serie nunca debe matchear una película homónima.
    let want_kinds: &[&str] = if media_type == "tv" {
        &["tv-episode", "tv-season"]
    } else {
        &["feature-movie"]
    };
    let want_year: Option<i32> = year.get(..4).and_then(|y| y.parse().ok());
    let targets: Vec<String> = queries
        .iter()
        .map(|q| norm_title(q))
        .filter(|s| !s.is_empty())
        .collect();
    let min_sim = if want_year.is_some() { 0.85 } else { 0.95 };

    for q in &queries {
        // Sin entity= : Apple bugea y devuelve vacío con entity=movie.
        let url = format!(
            "https://itunes.apple.com/search?term={}&country=us&limit=25",
            urlencoding::encode(q)
        );
        let r: Resp = match fetch_json::<Resp>(&url).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        let mut best: Option<(f32, &Item)> = None;
        for i in r.results.iter() {
            if i.preview_url.as_deref().map_or(true, |u| u.is_empty()) {
                continue;
            }
            if !i.kind.as_deref().map_or(false, |k| want_kinds.contains(&k)) {
                continue;
            }
            let name = norm_title(i.track_name.as_deref().unwrap_or(""));
            if name.is_empty() {
                continue;
            }
            let sim = targets
                .iter()
                .map(|t| similarity(t, &name))
                .fold(0.0f32, f32::max);
            if sim < min_sim {
                continue;
            }
            // iTunes fecha el estreno digital, no el de cines → tolerancia ±1.
            let iy: Option<i32> = i
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            let year_ok = match (want_year, iy) {
                (Some(w), Some(v)) => (w - v).abs() <= 1,
                (Some(_), None) => false,
                (None, _) => true,
            };
            if !year_ok {
                continue;
            }
            if best.map_or(true, |(s, _)| sim > s) {
                best = Some((sim, i));
            }
        }

        if let Some((_, found)) = best {
            return Ok(Some(AppleTrailer {
                url: found.preview_url.clone().unwrap_or_default(),
                title: found.track_name.clone().unwrap_or_default(),
                year: found
                    .release_date
                    .as_ref()
                    .and_then(|d| d.get(..4).map(|s| s.to_string())),
            }));
        }
    }

    Ok(None)
}

#[derive(Serialize)]
pub struct ItemStatus {
    pub id: u64,
    pub has_imdb: bool,
    pub imdb_id: Option<String>,
    pub has_trailer: bool,
    #[serde(default)]
    pub number_of_seasons: Option<u32>,
}

/// Respuesta combinada de `/{type}/{id}?append_to_response=external_ids,videos`.
#[derive(Deserialize)]
struct ItemStatusRaw {
    #[serde(default)]
    external_ids: Option<ExternalIds>,
    #[serde(default)]
    videos: Option<VideosResp>,
    // Solo presente en respuestas de series (/tv/{id}); gratis en el mismo fetch.
    #[serde(default)]
    number_of_seasons: Option<u32>,
}

#[tauri::command]
async fn item_status(media_type: String, id: u64, api_key: String) -> Result<ItemStatus, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    // UNA sola petición (append_to_response) en vez de 3 → 3× menos presión de
    // rate-limit en una grilla que sondea decenas de cards a la vez.
    //
    // CRÍTICO: si la petición falla (429/red), PROPAGAMOS el error con `?`. El
    // frontend entonces asume "ok" y NO oculta la card. Antes un fallo de red se
    // tragaba (`ext_res.ok()` → None) y se confundía con "sin imdb", ocultando
    // pelis y series enteras (todas las series desaparecían en el storm de 429).
    let url = format!(
        "{}/{}/{}?api_key={}&language={}&append_to_response=external_ids,videos",
        TMDB_BASE, media_type, id, api_key, LANG
    );
    let raw: ItemStatusRaw = fetch_json(&url).await?;
    let imdb_id = raw
        .external_ids
        .and_then(|e| e.imdb_id)
        .filter(|s| !s.is_empty());
    let has_imdb = imdb_id.is_some();
    let has_trailer = raw
        .videos
        .map(|v| v.results)
        .unwrap_or_default()
        .iter()
        .any(|v| v.site == "YouTube" && (v.kind == "Trailer" || v.kind == "Teaser"));
    Ok(ItemStatus { id, has_imdb, imdb_id, has_trailer, number_of_seasons: raw.number_of_seasons })
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
        "&append_to_response=external_ids,credits,images,keywords"
    } else {
        "&append_to_response=credits,images,keywords"
    };
    // include_image_language: backdrops sin texto (null) + es/en. Más variedad.
    let url = format!(
        "{}/{}/{}?api_key={}&language={}{}&include_image_language=es,en,null",
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

    // Backdrops extra (sin contar el principal), hasta 6 para que el front elija.
    let mut images: Vec<String> = Vec::new();
    if let Some(img) = raw.images {
        for b in img.backdrops.into_iter() {
            if let Some(fp) = b.file_path {
                if Some(&fp) != raw.backdrop_path.as_ref() {
                    images.push(fp);
                }
            }
            if images.len() >= 12 {
                break;
            }
        }
    }

    // Temporadas (solo TV): oculta especiales (S0) y temporadas vacías.
    let number_of_seasons = raw.number_of_seasons;
    let seasons: Vec<SeasonMini> = raw
        .seasons
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.season_number > 0 && s.episode_count > 0)
        .map(|s| SeasonMini {
            season_number: s.season_number,
            episode_count: s.episode_count,
            name: s.name,
            air_date: s.air_date,
            poster_path: s.poster_path,
        })
        .collect();

    let original_title = raw.original_title.or(raw.original_name);
    let production_companies = raw
        .production_companies
        .into_iter()
        .map(|n| n.name)
        .filter(|s| !s.is_empty())
        .collect();
    let production_countries = raw
        .production_countries
        .into_iter()
        .map(|n| n.name)
        .filter(|s| !s.is_empty())
        .collect();
    let spoken_languages = raw
        .spoken_languages
        .into_iter()
        .map(|n| n.name)
        .filter(|s| !s.is_empty())
        .collect();
    let release_date = if !date.is_empty() { Some(date) } else { None };

    // Keywords → temas sensibles, con el mismo mapeo que el importador de
    // catálogo (map_keyword_to_themes). /movie devuelve {keywords:[...]},
    // /tv devuelve {results:[...]}; KeywordsField cubre ambos.
    let mut sensitive_themes: Vec<String> = Vec::new();
    if let Some(kw) = raw.keywords.as_ref() {
        let items = if !kw.keywords.is_empty() { &kw.keywords } else { &kw.results };
        for k in items {
            for t in map_keyword_to_themes(&k.name.to_lowercase()) {
                if !sensitive_themes.iter().any(|x| x == t) {
                    sensitive_themes.push(t.to_string());
                }
            }
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
        images,
        number_of_seasons,
        seasons,
        tagline: raw.tagline.filter(|s| !s.is_empty()),
        original_title: original_title.filter(|s| !s.is_empty()),
        original_language: raw.original_language.filter(|s| !s.is_empty()),
        status: raw.status.filter(|s| !s.is_empty()),
        budget: raw.budget.filter(|&v| v > 0),
        revenue: raw.revenue.filter(|&v| v > 0),
        vote_count: raw.vote_count,
        popularity: raw.popularity,
        release_date,
        production_companies,
        production_countries,
        spoken_languages,
        homepage: raw.homepage.filter(|s| !s.is_empty()),
        sensitive_themes,
    })
}

#[derive(Serialize, Default)]
pub struct OmdbRating {
    pub source: String,
    pub value: String,
}

#[derive(Serialize, Default)]
pub struct OmdbDetail {
    pub plot: Option<String>,
    pub awards: Option<String>,
    pub rated: Option<String>,
    pub writer: Option<String>,
    pub country: Option<String>,
    pub language: Option<String>,
    pub released: Option<String>,
    pub metascore: Option<String>,
    pub imdb_rating: Option<String>,
    pub imdb_votes: Option<String>,
    pub box_office: Option<String>,
    pub production: Option<String>,
    pub ratings: Vec<OmdbRating>,
}

#[derive(Deserialize)]
struct OmdbRawRating {
    #[serde(rename = "Source", default)]
    source: String,
    #[serde(rename = "Value", default)]
    value: String,
}

#[derive(Deserialize)]
struct OmdbRaw {
    #[serde(rename = "Response", default)]
    response: String,
    #[serde(rename = "Error", default)]
    error: Option<String>,
    #[serde(rename = "Plot", default)]
    plot: Option<String>,
    #[serde(rename = "Awards", default)]
    awards: Option<String>,
    #[serde(rename = "Rated", default)]
    rated: Option<String>,
    #[serde(rename = "Writer", default)]
    writer: Option<String>,
    #[serde(rename = "Country", default)]
    country: Option<String>,
    #[serde(rename = "Language", default)]
    language: Option<String>,
    #[serde(rename = "Released", default)]
    released: Option<String>,
    #[serde(rename = "Metascore", default)]
    metascore: Option<String>,
    #[serde(rename = "imdbRating", default)]
    imdb_rating: Option<String>,
    #[serde(rename = "imdbVotes", default)]
    imdb_votes: Option<String>,
    #[serde(rename = "BoxOffice", default)]
    box_office: Option<String>,
    #[serde(rename = "Production", default)]
    production: Option<String>,
    #[serde(rename = "Ratings", default)]
    ratings: Vec<OmdbRawRating>,
}

fn clean_na(s: Option<String>) -> Option<String> {
    s.and_then(|v| {
        let t = v.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("N/A") {
            None
        } else {
            Some(t.to_string())
        }
    })
}

/// Datos de OMDb (premios, plot largo, ratings, box office) por imdb_id.
/// Devuelve Err si key vacía. Devuelve None-equivalente con campos vacíos
/// si OMDb responde "Movie not found".
#[tauri::command]
async fn omdb_detail(imdb_id: String, api_key: String) -> Result<OmdbDetail, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("falta omdb key".into());
    }
    let id = imdb_id.trim();
    if id.is_empty() {
        return Err("falta imdb id".into());
    }
    let url = format!(
        "https://www.omdbapi.com/?apikey={}&i={}&plot=full",
        key, id
    );
    let raw: OmdbRaw = fetch_json(&url).await.map_err(|e| format!("OMDb: {}", e))?;
    if raw.response.eq_ignore_ascii_case("False") {
        return Err(raw.error.unwrap_or_else(|| "OMDb: sin datos".into()));
    }
    Ok(OmdbDetail {
        plot: clean_na(raw.plot),
        awards: clean_na(raw.awards),
        rated: clean_na(raw.rated),
        writer: clean_na(raw.writer),
        country: clean_na(raw.country),
        language: clean_na(raw.language),
        released: clean_na(raw.released),
        metascore: clean_na(raw.metascore),
        imdb_rating: clean_na(raw.imdb_rating),
        imdb_votes: clean_na(raw.imdb_votes),
        box_office: clean_na(raw.box_office),
        production: clean_na(raw.production),
        ratings: raw
            .ratings
            .into_iter()
            .filter(|r| !r.value.is_empty() && !r.value.eq_ignore_ascii_case("N/A"))
            .map(|r| OmdbRating {
                source: r.source,
                value: r.value,
            })
            .collect(),
    })
}

#[derive(Serialize)]
pub struct EpisodeMini {
    pub episode_number: u32,
    pub name: String,
    pub overview: String,
    pub still_path: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<u32>,
}

#[derive(Deserialize)]
struct SeasonDetailRaw {
    #[serde(default)]
    episodes: Vec<EpisodeRaw>,
}

#[derive(Deserialize)]
struct EpisodeRaw {
    #[serde(default)]
    episode_number: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    overview: String,
    #[serde(default)]
    still_path: Option<String>,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
}

/// Episodios de una temporada (TMDb /tv/{id}/season/{n}).
#[tauri::command]
async fn tmdb_season(
    id: u64,
    season_number: u32,
    api_key: String,
) -> Result<Vec<EpisodeMini>, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    let url = format!(
        "{}/tv/{}/season/{}?api_key={}&language={}",
        TMDB_BASE, id, season_number, api_key, LANG
    );
    let raw: SeasonDetailRaw = fetch_json(&url).await?;
    Ok(raw
        .episodes
        .into_iter()
        .map(|e| EpisodeMini {
            episode_number: e.episode_number,
            name: e.name,
            overview: e.overview,
            still_path: e.still_path,
            air_date: e.air_date,
            runtime: e.runtime,
        })
        .collect())
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

// Helper de tecla virtual usado por webserver.rs (HTTP /key remote control).
// No es comando Tauri por sí mismo: se invoca desde el handler HTTP.
pub fn press_key(key: &str) -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {}", e))?;
    let k = match key.to_lowercase().as_str() {
        "space" | " " => Key::Space,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "enter" | "return" => Key::Return,
        "backspace" | "back" => Key::Backspace,
        "arrowup" | "up" => Key::UpArrow,
        "arrowdown" | "down" => Key::DownArrow,
        "arrowleft" | "left" => Key::LeftArrow,
        "arrowright" | "right" => Key::RightArrow,
        s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
        _ => return Err(format!("tecla desconocida: {}", key)),
    };
    enigo.key(k, Direction::Click).map_err(|e| e.to_string())
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
    // .jpg, no .webp: ver el encoder más abajo. El cambio de extensión invalida
    // solo el cache viejo (se regenera al vuelo); los .webp huérfanos los barre
    // la limpieza de cache del sistema o un borrado manual de <cache>/imgs.
    let filename = format!("{}.jpg", &hash[..16]);

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

    // Decode + resize + JPEG encode (en blocking thread, image es CPU-bound).
    //
    // Antes esto era WebPEncoder::new_lossless. El `image` crate SOLO sabe
    // escribir WebP sin pérdida, y lossless sobre una foto (póster, backdrop,
    // retrato) es lo peor de los dos mundos: decenas de veces más lento que
    // JPEG y el archivo sale MÁS GRANDE que el original de TMDb. Con el grid
    // pidiendo ~100 imágenes al scrollear, eso clavaba la CPU y la app se
    // quedaba pegada. JPEG q=82 es visualmente indistinguible a este tamaño.
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
        // JPEG no lleva alfa: rgb8, no rgba8.
        let rgb = resized.to_rgb8();
        let mut file = std::fs::File::create(&path_clone).map_err(|e| e.to_string())?;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 82);
        use image::ImageEncoder;
        encoder
            .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("encode jpeg: {}", e))?;
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
    app: tauri::AppHandle,
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<(), String> {
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

        // Hipo de red transitorio: NO abortar el flujo, reintentar al próximo
        // poll. Un solo blip no debe tirar abajo todo el login.
        let resp = match cli.get(&creds_url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[rd] creds red transitorio, reintento: {}", e);
                continue;
            }
        };
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
        let tok = match cli
            .post("https://api.real-debrid.com/oauth/v2/token")
            .form(&form)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[rd] token red transitorio, reintento: {}", e);
                continue;
            }
        };
        if !tok.status().is_success() {
            let st = tok.status();
            let body = tok.text().await.unwrap_or_default();
            return Err(format!("RD token {}: {}", st, body));
        }
        let t: RdTokenResp = tok.json().await.map_err(|e| format!("parse token: {}", e))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // El token NO vuelve al webview: se persiste en el store 0600 del backend.
        crate::creds::save(
            &app,
            &crate::creds::RdCreds {
                access_token: t.access_token,
                refresh_token: t.refresh_token,
                client_id: creds.client_id,
                client_secret: creds.client_secret,
                expires_at: now + t.expires_in,
            },
        )?;
        return Ok(());
    }
}

// ============================================================
// Audio (wpctl) + Brillo (brightnessctl)
// ============================================================

#[derive(Serialize)]
pub struct AudioState {
    volume: u8,
    muted: bool,
    available: bool,
}

#[tauri::command]
async fn audio_get() -> Result<AudioState, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(AudioState { volume: 50, muted: false, available: false }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let out = match Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return Ok(AudioState { volume: 50, muted: false, available: false }),
        };
        let s = String::from_utf8_lossy(&out.stdout);
        let mut volume = 50u8;
        if let Some(idx) = s.find("Volume:") {
            let after = &s[idx + 7..];
            if let Some(tok) = after.split_whitespace().next() {
                if let Ok(v) = tok.parse::<f32>() {
                    let pct = (v * 100.0).round();
                    volume = pct.clamp(0.0, 150.0) as u8;
                }
            }
        }
        let muted = s.contains("MUTED");
        Ok(AudioState { volume, muted, available: true })
    }
}

#[tauri::command]
async fn audio_set(volume: u8) -> Result<(), String> {
    let v = volume.min(150);
    #[cfg(not(target_os = "linux"))]
    { let _ = v; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = format!("{:.2}", v as f32 / 100.0);
        let out = Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &arg])
            .output()
            .map_err(|e| format!("wpctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

#[tauri::command]
async fn audio_set_mute(muted: bool) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    { let _ = muted; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = if muted { "1" } else { "0" };
        let out = Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", arg])
            .output()
            .map_err(|e| format!("wpctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct BrightnessState {
    percent: u8,
    available: bool,
}

#[tauri::command]
async fn brightness_get() -> Result<BrightnessState, String> {
    #[cfg(not(target_os = "linux"))]
    { return Ok(BrightnessState { percent: 100, available: false }); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let cur = Command::new("brightnessctl").args(["get"]).output();
        let max = Command::new("brightnessctl").args(["max"]).output();
        if let (Ok(c), Ok(m)) = (cur, max) {
            if c.status.success() && m.status.success() {
                let cs = String::from_utf8_lossy(&c.stdout).trim().parse::<u64>().unwrap_or(0);
                let ms = String::from_utf8_lossy(&m.stdout).trim().parse::<u64>().unwrap_or(0);
                if ms > 0 {
                    let p = ((cs as f64 / ms as f64) * 100.0).round() as u8;
                    return Ok(BrightnessState { percent: p.min(100), available: true });
                }
            }
        }
        Ok(BrightnessState { percent: 100, available: false })
    }
}

#[tauri::command]
async fn brightness_set(percent: u8) -> Result<(), String> {
    let p = percent.clamp(5, 100);
    #[cfg(not(target_os = "linux"))]
    { let _ = p; return Ok(()); }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let arg = format!("{}%", p);
        let out = Command::new("brightnessctl")
            .args(["set", &arg])
            .output()
            .map_err(|e| format!("brightnessctl: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
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

// ============================================================
// Subtítulos: Wyzie (sub.wyzie.io)
// ============================================================
// Endpoint observado: GET https://sub.wyzie.io/search?id={imdb}&language={lang}&key={key}
// Sin key devuelve 401 con instrucciones para registrar en store.wyzie.io/redeem.

#[derive(Serialize)]
pub struct WyzieSubtitle {
    pub url: String,
    pub label: String,
    pub lang: String,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct WyzieItem {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    display: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    isHearingImpaired: Option<bool>,
}

#[tauri::command]
async fn wyzie_search(
    imdb_id: String,
    language: String,
    api_key: String,
) -> Result<Vec<WyzieSubtitle>, String> {
    if api_key.is_empty() {
        return Err("falta wyzie key".into());
    }
    if imdb_id.is_empty() {
        return Err("imdb_id vacío".into());
    }
    let url = format!(
        "https://sub.wyzie.io/search?id={}&language={}&key={}",
        urlencoding::encode(&imdb_id),
        urlencoding::encode(&language),
        urlencoding::encode(&api_key)
    );
    let cli = client()?;
    let resp = cli
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("red: {}", e))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("wyzie {}: {}", status, body));
    }
    let items: Vec<WyzieItem> =
        serde_json::from_str(&body).map_err(|e| format!("parse: {} :: {}", e, body))?;
    let mut out: Vec<WyzieSubtitle> = Vec::new();
    for it in items {
        let Some(u) = it.url.filter(|s| !s.is_empty()) else {
            continue;
        };
        // Preferir VTT, también aceptamos SRT (el player los traduce).
        if let Some(fmt) = it.format.as_deref() {
            let fmt = fmt.to_lowercase();
            if fmt != "vtt" && fmt != "srt" {
                continue;
            }
        }
        if it.isHearingImpaired == Some(true) {
            continue;
        }
        let label = it
            .display
            .clone()
            .or_else(|| it.language.clone())
            .unwrap_or_else(|| "Subtítulos".into());
        let lang = it.language.clone().unwrap_or_else(|| language.clone());
        out.push(WyzieSubtitle {
            url: u,
            label,
            lang,
        });
    }
    Ok(out)
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
        Migration {
            version: 4,
            description: "unavailable_kind",
            // Drop+recreate: las filas previas se generaron con type=movie
            // hardcodeado y marcaron series como no disponibles. Resetear.
            sql: "
                DROP TABLE IF EXISTS unavailable_items;
                CREATE TABLE unavailable_items (
                    imdb_id TEXT NOT NULL,
                    kind TEXT NOT NULL DEFAULT 'movie',
                    detected_at INTEGER NOT NULL,
                    PRIMARY KEY (imdb_id, kind)
                );
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 5,
            description: "unavailable_reset_screening_deprecated",
            // El screening automático contra streamdata.vaplayer.ru dejó de ser
            // confiable (404 intermitentes en pelis sanas) y ya no se dispara
            // desde el frontend. Vaciamos lo acumulado por ese detector para no
            // seguir ocultando el botón Descubrir con datos viejos y falsos.
            // Los reportes manuales del user se vuelven a generar al vuelo.
            sql: "DELETE FROM unavailable_items;",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 6,
            description: "vera_setup_preferencias",
            // Tres preferencias que el esquema v3 no contemplaba:
            //   animation_pref  gusta | indiferente | no — la animación divide
            //                   aguas y "no me gustan" no se podía expresar
            //                   como simple exclusión de género sin perder el
            //                   caso contrario ("me encantan").
            //   languages_avoid idiomas que la persona no tolera escuchar.
            //   runtime_max     tope de duración. Antes iba embutido en
            //                   depth_profile como "auto:<min>"; ahora tiene
            //                   columna propia y se lee de las dos formas para
            //                   no perder lo ya guardado.
            // Aditiva: ALTER TABLE ADD COLUMN no toca las filas existentes.
            sql: "
                ALTER TABLE vera_setup ADD COLUMN animation_pref TEXT NOT NULL DEFAULT 'indiferente';
                ALTER TABLE vera_setup ADD COLUMN languages_avoid TEXT NOT NULL DEFAULT '[]';
                ALTER TABLE vera_setup ADD COLUMN runtime_max INTEGER;
            ",
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .setup(|app| {
            // Activa MSE en el WebKitGTK del webview para que hls.js pueda
            // reproducir HLS (IPTV) DENTRO de la app. Sin esto el <video>
            // queda en negro porque el webview no expone MediaSource.
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.with_webview(|webview| {
                        use webkit2gtk::{SettingsExt, WebViewExt};
                        let wv = webview.inner();
                        if let Some(settings) = WebViewExt::settings(&wv) {
                            settings.set_enable_mediasource(true);
                            settings.set_media_playback_requires_user_gesture(false);
                            settings.set_enable_webaudio(true);
                        }
                    });
                    // Reproductor libmpv embebido: crea el handle y mete el
                    // GtkGLArea en el toplevel del webview (una sola ventana).
                    if let Err(e) = mpv_embed::init(&app.handle(), &win) {
                        eprintln!("[mpv-embed] init falló: {e}");
                    }
                }
            }
            Ok(())
        })
        .manage(screening::ScreeningState::default())
        .manage(emu::EmuState::default())
        .manage(player::PlayerState::default())
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
            omdb_detail,
            tmdb_season,
            tmdb_recommendations,
            tmdb_genres,
            tmdb_videos,
            tmdb_trailer_key,
            yt_playable,
            apple_trailer,
            item_status,
            tmdb_person,
            cache_image,
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
            rd_device_poll,
            creds::rd_creds_save,
            creds::rd_creds_clear,
            creds::rd_creds_status,
            audio_get,
            audio_set,
            audio_set_mute,
            brightness_get,
            brightness_set,
            webserver::web_server_start,
            webserver::web_server_stop,
            webserver::web_server_status,
            ui_log,
            kodios::kodios_search,
            anilist::anilist_discover,
            anilist::anilist_detail,
            anilist::anizip_episodes,
            probe::ffprobe_tracks,
            rd::rd_resolve,
            rd::rd_unrestrict,
            rd::rd_instant_available,
            rd::rd_refresh,
            rd::rd_account,
            rd::rd_cleanup_torrents,
            torrent::torrent_add,
            torrent::torrent_status,
            torrent::torrent_list,
            torrent::torrent_pause,
            torrent::torrent_resume,
            torrent::torrent_remove,
            torrent::torrent_init,
            torrent::torrent_default_dir,
            torrent::torrent_check_dir,
            player::imp::mpv_play,
            player::imp::mpv_play_trailer,
            player::imp::mpv_play_iptv,
            player::imp::mpv_cmd,
            player::imp::mpv_open_picker,
            player::imp::mpv_stop,
            player::imp::mpv_suspend,
            player::imp::mpv_resume,
            player::imp::mpv_session,
            player::imp::mpv_running,
            player::imp::mpv_status,
            player::imp::mpv_tracks,
            opensubtitles::os_login,
            opensubtitles::os_status,
            opensubtitles::os_clear,
            opensubtitles::os_search,
            opensubtitles::os_list,
            opensubtitles::os_download,
            opensubtitles::subtitle_save,
            screening::screening_enqueue,
            screening::screening_get_unavailable,
            screening::screening_set_paused,
            screening::screening_set_concurrency,
            awards::wikidata_awards,
            emu::emu_play,
            emu::emu_cmd,
            emu::emu_stop,
            emu::emu_running,
            emu::emu_input,
            emu::emu_catalog,
            emu::emu_metadata,
            emu::emu_synopsis,
            emu::emu_owned,
            emu::emu_download,
            wyzie_search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ========================================================================
// Smoke test de la sinopsis en español. Ver nota en `kitsu.rs::tests`.
// Necesita una key de TMDb en el entorno:
//   TMDB_KEY=xxxx cargo test --lib overview -- --ignored --nocapture
// ========================================================================
#[cfg(test)]
mod overview_tests {
    #[tokio::test]
    #[ignore]
    async fn sinopsis_en_espanol() {
        let key = std::env::var("TMDB_KEY").unwrap_or_default();
        assert!(!key.is_empty(), "falta TMDB_KEY en el entorno");

        // (tipo, tmdb_id, título) — ids reales que da ani.zip.
        let casos = [
            (Some("TV"), 209867u64, "Frieren"),
            (Some("MOVIE"), 372058, "Kimi no Na wa"),
            (Some("MOVIE"), 129, "Spirited Away"),
        ];
        for (kind, id, name) in casos {
            let o = super::tmdb_overview_es(kind, id, &key).await;
            let txt = o.expect("sin sinopsis en español");
            assert!(txt.len() > 40, "{name}: sinopsis sospechosamente corta");
            eprintln!("\n{name} →\n  {}", &txt.chars().take(160).collect::<String>());
        }

        // Sin key no explota: devuelve None y quien llama deja el inglés.
        assert!(super::tmdb_overview_es(Some("TV"), 209867, "").await.is_none());
        // Id inexistente tampoco explota.
        assert!(super::tmdb_overview_es(Some("TV"), 99999999, &key).await.is_none());
    }
}
