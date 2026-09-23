use serde::{Deserialize, Serialize};

mod anilist;
mod anime_web;
mod awards;
mod cache;
mod cast;
mod creds;
mod dlna;
mod kitsu;
mod kodios;
mod lan;
mod net;
mod opensubtitles;
mod probe;
mod rd;
mod player;
#[cfg(target_os = "linux")]
mod mpv_embed;
#[cfg(target_os = "linux")]
mod reposo;
mod screening;
mod sistema;
mod vera;
mod torrent;
mod trailers;
mod webserver;
mod wyzie;
mod winproc;

use trailers::VideosResp;
use vera::{map_keyword_to_themes, KeywordsField};

/// El front avisa cuando reproduce fuera de mpv (iframe web, IPTV con hls.js)
/// para que el escritorio no se duerma. mpv avisa por su cuenta.
#[tauri::command]
fn reposo_inhibir(fuente: String, on: bool) {
    #[cfg(target_os = "linux")]
    reposo::set(&fuente, on);
    #[cfg(not(target_os = "linux"))]
    let _ = (fuente, on);
}

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

pub(crate) async fn fetch_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
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

// Cada filtro es un argumento con nombre del invoke del front: agruparlos en
// un struct cambiaría la forma de la llamada en todos los que la usan.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn tmdb_discover(
    app: tauri::AppHandle,
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

    // Caché en disco de la página armada. La clave sale de la URL final SIN la
    // api key: dos keys distintas ven exactamente el mismo catálogo, no tiene
    // sentido bajarlo dos veces.
    let clave = cache::clave(&["discover", &url.replace(&format!("api_key={}&", api_key), "")]);
    if let Some(json) = cache::lista_get(&app, &clave) {
        if let Ok(r) = serde_json::from_str::<TmdbListResp>(&json) {
            return Ok(r);
        }
    }
    let resp: TmdbListResp = fetch_json(&url).await?;
    // Página vacía = casi siempre un hipo de TMDb. Cachearla dejaría el
    // catálogo en blanco durante 12 horas.
    if !resp.results.is_empty() {
        if let Ok(json) = serde_json::to_string(&resp) {
            cache::lista_put(&app, &clave, &json);
        }
    }
    Ok(resp)
}

#[tauri::command]
async fn tmdb_search(
    media_type: String,
    query: String,
    page: u64,
    api_key: String,
) -> Result<TmdbListResp, String> {
    tmdb_buscar(media_type, query, page, api_key).await
}

/// El cuerpo de `tmdb_search`, sin el `#[tauri::command]` encima: el macro de
/// Tauri no admite que el comando sea `pub(crate)`, y el servidor web (que no
/// pasa por `invoke`) necesita llamar esto igual.
pub(crate) async fn tmdb_buscar(
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

/// Tendencias de la semana. Es el listado que ve el control web al entrar al
/// catálogo: sin filtros, sin paginado fino, lo que la gente busca de verdad
/// cuando toma el celular para poner algo.
pub(crate) async fn tmdb_trending(
    media_type: String,
    page: u64,
    api_key: String,
) -> Result<TmdbListResp, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    let url = format!(
        "{}/trending/{}/week?api_key={}&language={}&page={}",
        TMDB_BASE, media_type, api_key, LANG, page
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
async fn item_status(
    app: tauri::AppHandle,
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<ItemStatus, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    // Caché: es UNA petición por card, o sea decenas por pantalla al paginar.
    // Lo que devuelve (tiene imdb, tiene trailer, cuántas temporadas) no cambia
    // de un día para otro, así que la card ya vista no vuelve a pegarle a la red.
    if let Some(c) = cache::status_get(&app, &media_type, id) {
        return Ok(ItemStatus {
            id,
            has_imdb: c.has_imdb,
            imdb_id: c.imdb_id,
            has_trailer: c.has_trailer,
            number_of_seasons: c.seasons,
        });
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
    cache::status_put(
        &app,
        &media_type,
        id,
        &cache::StatusRow {
            has_imdb,
            imdb_id: imdb_id.clone(),
            has_trailer,
            seasons: raw.number_of_seasons,
        },
    );
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
        for nombre in kw.nombres() {
            for t in map_keyword_to_themes(&nombre.to_lowercase()) {
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
        .error_for_status()
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
        // A un temporal y después rename (atómico): si el encode falla o la
        // app se cierra a mitad, en la ruta final nunca queda un .jpg cortado
        // que el `path.exists()` de arriba devolvería para siempre. El nombre
        // lleva un contador para que dos pedidos simultáneos de la misma URL
        // no escriban el mismo archivo a la vez.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path_clone.with_extension(format!("jpg.{}.tmp", n));
        let escribir = || -> Result<(), String> {
            let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 82);
            use image::ImageEncoder;
            encoder
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .map_err(|e| format!("encode jpeg: {}", e))?;
            std::fs::rename(&tmp, &path_clone).map_err(|e| e.to_string())
        };
        let r = escribir();
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
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
        for cr in c.cast.into_iter().chain(c.crew) {
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
        Migration {
            version: 7,
            description: "historial_por_episodio_y_favoritos",
            // watch_history nació con PK = imdb_id: una serie entera compartía
            // una fila y cada capítulo pisaba al anterior. Ahora la PK es
            // (imdb_id, season, episode) con -1/-1 para películas, que es el
            // valor con el que migran las filas viejas.
            //
            // SQLite no sabe cambiar una PK con ALTER: hay que reconstruir la
            // tabla y copiar. La copia preserva TODO el historial previo.
            //
            // `imdb_id` también acepta claves sintéticas `anilist:<id>` para el
            // anime que no tiene imdb (ver historial.svelte.ts). Por eso sigue
            // siendo TEXT y no se valida el formato tt*.
            //
            // still_path / episode_title: sin ellos la lista por fecha no
            // podría dibujar un capítulo (el poster de la serie no dice cuál
            // viste).
            sql: "
                CREATE TABLE watch_history_nuevo (
                    imdb_id TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    tmdb_id INTEGER NOT NULL,
                    media_type TEXT NOT NULL,
                    title TEXT NOT NULL,
                    poster_path TEXT,
                    still_path TEXT,
                    episode_title TEXT,
                    watched_seconds INTEGER NOT NULL DEFAULT 0,
                    runtime_seconds INTEGER,
                    progress_real REAL,
                    completed INTEGER NOT NULL DEFAULT 0,
                    last_watched INTEGER NOT NULL,
                    PRIMARY KEY (imdb_id, season, episode)
                );

                INSERT INTO watch_history_nuevo
                    (imdb_id, season, episode, tmdb_id, media_type, title,
                     poster_path, watched_seconds, runtime_seconds,
                     progress_real, completed, last_watched)
                SELECT imdb_id, -1, -1, tmdb_id, media_type, title,
                       poster_path, watched_seconds, runtime_seconds,
                       progress_real, completed, last_watched
                  FROM watch_history;

                DROP TABLE watch_history;
                ALTER TABLE watch_history_nuevo RENAME TO watch_history;

                CREATE INDEX IF NOT EXISTS idx_wh_last ON watch_history(last_watched);
                CREATE INDEX IF NOT EXISTS idx_wh_imdb ON watch_history(imdb_id);

                CREATE TABLE IF NOT EXISTS favorites (
                    imdb_id TEXT PRIMARY KEY,
                    tmdb_id INTEGER NOT NULL,
                    media_type TEXT NOT NULL,
                    title TEXT NOT NULL,
                    poster_path TEXT,
                    added_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_fav_added ON favorites(added_at);
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 8,
            description: "descargas locales",
            // Puente entre un título del catálogo y el archivo que quedó en
            // disco. Sin esta tabla la cola de torrents solo conoce nombres de
            // release: al volver a la ficha nadie sabe que ya la tienes bajada.
            //
            // Misma clave que watch_history (imdb, o `anilist:<id>` en anime) y
            // el mismo -1/-1 para películas: así una fila se busca con lo que
            // la ficha ya tiene a mano.
            sql: "CREATE TABLE IF NOT EXISTS descargas (
                    clave TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    info_hash TEXT NOT NULL,
                    ruta TEXT NOT NULL,
                    release_title TEXT NOT NULL,
                    quality TEXT NOT NULL DEFAULT 'unknown',
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    -- 0 mientras baja: un archivo a medias no se puede ofrecer
                    -- como copia local (mpv abriría un video cortado).
                    completa INTEGER NOT NULL DEFAULT 0,
                    added_at INTEGER NOT NULL,
                    PRIMARY KEY (clave, season, episode)
                );

                CREATE INDEX IF NOT EXISTS idx_desc_hash ON descargas(info_hash);
            ",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 9,
            description: "cola de descargas pendientes",
            // Lo que el usuario pidió bajar pero todavía no empezó. Existe
            // porque el cliente torrent baja TODO en paralelo: 24 capítulos a
            // la vez se reparten los seeds y no termina ninguno. Acá esperan
            // su turno, de a pocos por vez (config.torrentMaxParalelas).
            //
            // No se guarda el magnet: se busca al llegar el turno. Un magnet
            // de hace tres días puede estar sin seeds, y la búsqueda fresca
            // además respeta la config de idioma/calidad del momento.
            sql: "CREATE TABLE IF NOT EXISTS descargas_pendientes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    clave TEXT NOT NULL,
                    season INTEGER NOT NULL DEFAULT -1,
                    episode INTEGER NOT NULL DEFAULT -1,
                    title TEXT NOT NULL,
                    -- 'T1 E4' en series, vacío en películas.
                    etiqueta TEXT NOT NULL DEFAULT '',
                    imdb_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    original_title TEXT,
                    kitsu_id INTEGER,
                    -- 'espera' | 'buscando' | 'error'
                    estado TEXT NOT NULL DEFAULT 'espera',
                    error TEXT,
                    added_at INTEGER NOT NULL,
                    UNIQUE (clave, season, episode)
                );

                CREATE INDEX IF NOT EXISTS idx_pend_orden
                    ON descargas_pendientes(added_at, id);
            ",
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .setup(|app| {
            // Limpieza de páginas de catálogo vencidas. En un hilo aparte:
            // es I/O de disco y no tiene por qué demorar el arranque.
            {
                let h = app.handle().clone();
                std::thread::spawn(move || cache::purgar(&h));
            }
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
                    if let Err(e) = mpv_embed::init(app.handle(), &win) {
                        eprintln!("[mpv-embed] init falló: {e}");
                    }
                }
            }
            Ok(())
        })
        .manage(screening::ScreeningState::default())
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
            reposo_inhibir,
            tmdb_discover,
            tmdb_search,
            tmdb_detail,
            omdb_detail,
            tmdb_season,
            tmdb_recommendations,
            tmdb_genres,
            trailers::tmdb_videos,
            trailers::tmdb_trailer_key,
            trailers::yt_trailer_src,
            trailers::apple_trailer,
            item_status,
            tmdb_person,
            cache_image,
            vera::vera_intent_options,
            vera::vera_genre_list,
            vera::vera_theme_list,
            vera::vera_platform_list,
            vera::vera_import_catalog,
            vera::vera_catalog_count,
            sistema::os_info,
            sistema::wifi_status,
            sistema::wifi_scan,
            sistema::wifi_connect,
            rd::rd_device_start,
            rd::rd_device_poll,
            creds::rd_creds_save,
            creds::rd_creds_clear,
            creds::rd_creds_status,
            sistema::audio_get,
            sistema::audio_set,
            sistema::audio_set_mute,
            sistema::brightness_get,
            sistema::brightness_set,
            webserver::web_server_start,
            webserver::web_set_tmdb_key,
            webserver::web_server_stop,
            webserver::web_server_status,
            ui_log,
            kodios::kodios_search,
            anilist::anilist_discover,
            anilist::anilist_detail,
            anilist::anilist_relacionados,
            anilist::anizip_episodes,
            probe::ffprobe_tracks,
            cast::cast_scan,
            cast::cast_ping,
            cast::cast_play,
            cast::cast_status,
            cast::cast_control,
            cast::cast_soltar,
            lan::cast_red_info,
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
            torrent::local_file_size,
            torrent::disk_free,
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
            player::imp::mpv_cache_stats,
            player::imp::mpv_set_cache,
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
            wyzie::wyzie_search
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
            eprintln!("\n{name} →\n  {}", txt.chars().take(160).collect::<String>());
        }

        // Sin key no explota: devuelve None y quien llama deja el inglés.
        assert!(super::tmdb_overview_es(Some("TV"), 209867, "").await.is_none());
        // Id inexistente tampoco explota.
        assert!(super::tmdb_overview_es(Some("TV"), 99999999, &key).await.is_none());
    }
}
