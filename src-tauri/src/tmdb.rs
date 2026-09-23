// TMDb: listados, búsqueda, detalle, temporadas y personas.

use serde::{Deserialize, Serialize};

use crate::{cache, client};
use crate::trailers::VideosResp;
use crate::vera::{map_keyword_to_themes, KeywordsField};

pub(crate) const TMDB_BASE: &str = "https://api.themoviedb.org/3";
pub(crate) const LANG: &str = "es-ES";

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
pub(crate) struct ExternalIds {
    #[serde(default)]
    pub(crate) imdb_id: Option<String>,
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
pub async fn tmdb_genres(media_type: String, api_key: String) -> Result<Vec<GenreItem>, String> {
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
pub async fn tmdb_discover(
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
pub async fn tmdb_search(
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
pub async fn tmdb_recommendations(
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
pub async fn item_status(
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
pub async fn tmdb_detail(
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
pub async fn tmdb_season(
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

#[tauri::command]
pub async fn tmdb_person(id: u64, api_key: String) -> Result<PersonInfo, String> {
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
