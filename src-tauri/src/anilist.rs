// AniList — fuente de conocimiento para el tab Anime (reemplaza a TMDb ahí).
//
// - Catálogo/búsqueda: GraphQL público de AniList (sin key, 90 req/min).
// - Metadata de episodios + mapeo de IDs (MAL/Kitsu/AniDB/IMDb): ani.zip,
//   una sola llamada por anime, cacheada en memoria.
//
// Patrón tomado de Otaku (plugin.video.otaku): un ID maestro (acá AniList),
// tabla de mappings para traducir a los IDs que entienden los scrapers
// (kitsu → Torrentio, imdb → resto), y un indexer aparte (ani.zip) para
// títulos/thumbnails de episodios porque AniList no los tiene.
//
// Las respuestas se adaptan al MISMO shape que los comandos TMDb
// (TmdbListResp/TmdbItem/EpisodeMini) para que el frontend cambie lo mínimo.
// Diferencia: poster_path/still_path llevan URL COMPLETA (https://…), no
// fragmento TMDb — el frontend detecta el prefijo http.

use crate::{EpisodeMini, PersonMini, SeasonMini, TmdbItem, TmdbListResp};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const GRAPHQL_URL: &str = "https://graphql.anilist.co";
const ANIZIP_BASE: &str = "https://api.ani.zip/mappings?";
const ANIZIP_TTL_H: u64 = 12;
const PER_PAGE: u32 = 20;

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("kutral/0.1")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| format!("client: {e}"))
}

// ========================================================================
// Salud de AniList  →  respaldo Kitsu
// ========================================================================
//
// AniList apaga su API entera cada tanto (403 "temporarily disabled"). Cuando
// pasa, TODA la pestaña anime se cae. En vez de reintentar contra un servicio
// muerto en cada scroll, marcamos la fuente como caída y servimos desde Kitsu
// (ver `kitsu.rs`), reintentando AniList cada ANILIST_RETRY_MIN minutos para
// volver solos cuando reviva.

const ANILIST_RETRY_MIN: u64 = 10;

/// `Some(t)` = no reintentar AniList hasta `t`. `None` = fuente sana.
fn anilist_gate() -> &'static Mutex<Option<Instant>> {
    static GATE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(None))
}

fn anilist_is_down() -> bool {
    let mut g = anilist_gate().lock().unwrap_or_else(|e| e.into_inner());
    match *g {
        Some(until) if Instant::now() < until => true,
        Some(_) => {
            // Venció la espera: dejamos pasar un intento de sondeo.
            *g = None;
            false
        }
        None => false,
    }
}

fn mark_anilist_down(why: &str) {
    let mut g = anilist_gate().lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        eprintln!("[anilist] caída ({why}) → respaldo Kitsu por {ANILIST_RETRY_MIN} min");
    }
    *g = Some(Instant::now() + Duration::from_secs(ANILIST_RETRY_MIN * 60));
}

fn mark_anilist_up() {
    let mut g = anilist_gate().lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        eprintln!("[anilist] revivió → vuelve a ser la fuente principal");
    }
    *g = None;
}

// ========================================================================
// GraphQL plumbing
// ========================================================================

/// Toda salida por error de acá se considera "AniList no está sirviendo":
/// red caída, status != 2xx o `errors` en el cuerpo (que es como viaja el 403
/// de "API temporarily disabled" — llega con HTTP 200 y el error adentro).
/// Un "media no encontrado" NO pasa por acá: eso es un 200 con `data: null`,
/// y lo distingue quien llama.
async fn gql(query: &str, variables: serde_json::Value) -> Result<serde_json::Value, String> {
    let out = gql_inner(query, variables).await;
    match &out {
        Ok(_) => mark_anilist_up(),
        Err(e) => mark_anilist_down(e),
    }
    out
}

async fn gql_inner(query: &str, variables: serde_json::Value) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({ "query": query, "variables": variables });
    let resp = http()?
        .post(GRAPHQL_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("anilist red: {e}"))?;
    let status = resp.status();
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("anilist parse: {e}"))?;
    if let Some(errs) = v.get("errors").filter(|e| !e.is_null()) {
        return Err(format!("anilist gql: {errs}"));
    }
    if !status.is_success() {
        return Err(format!("anilist {status}"));
    }
    Ok(v["data"].clone())
}

pub(crate) fn s(v: &serde_json::Value) -> Option<String> {
    v.as_str().filter(|x| !x.is_empty()).map(|x| x.to_string())
}

/// AniList manda descripciones con HTML liviano (<br>, <i>…). Lo limpiamos.
pub(crate) fn strip_html(t: &str) -> String {
    let t = t.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::with_capacity(t.len());
    let mut in_tag = false;
    for c in t.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn media_title(m: &serde_json::Value) -> String {
    s(&m["title"]["english"])
        .or_else(|| s(&m["title"]["romaji"]))
        .unwrap_or_default()
}

fn media_to_item(m: &serde_json::Value) -> TmdbItem {
    let date = m["startDate"]["year"].as_u64().map(|y| {
        let mo = m["startDate"]["month"].as_u64().unwrap_or(1);
        let d = m["startDate"]["day"].as_u64().unwrap_or(1);
        format!("{y:04}-{mo:02}-{d:02}")
    });
    TmdbItem {
        id: m["id"].as_u64().unwrap_or(0),
        title: None,
        name: Some(media_title(m)),
        poster_path: s(&m["coverImage"]["extraLarge"]).or_else(|| s(&m["coverImage"]["large"])),
        overview: s(&m["description"]).map(|d| strip_html(&d)).unwrap_or_default(),
        vote_average: m["averageScore"].as_f64().map(|x| x / 10.0).unwrap_or(0.0) as f32,
        release_date: None,
        first_air_date: date,
        // Vera no consume estas fuentes (solo TMDb); campos neutros.
        genre_ids: Vec::new(),
        vote_count: None,
        popularity: None,
        original_language: None,
    }
}

// ========================================================================
// Comando: catálogo / búsqueda
// ========================================================================

/// Catálogo anime vía AniList. `sort` es un MediaSort de AniList
/// (POPULARITY_DESC, SCORE_DESC, TRENDING_DESC, START_DATE_DESC, …).
/// `genres` es CSV de géneros AniList en inglés ("Action,Romance").
/// Con `search`, AniList ordena por relevancia y se ignora `sort`.
/// `season` (WINTER/SPRING/SUMMER/FALL) + `season_year` filtran por cour;
/// solo aplican si vienen ambos.
#[tauri::command]
pub async fn anilist_discover(
    app: tauri::AppHandle,
    page: u32,
    sort: Option<String>,
    genres: Option<String>,
    search: Option<String>,
    season: Option<String>,
    season_year: Option<u32>,
) -> Result<TmdbListResp, String> {
    // Caché en disco de la página, igual que el discover de TMDb. La búsqueda
    // queda afuera: cada tecla es una consulta distinta y llenaría el disco de
    // páginas que nadie vuelve a pedir.
    let buscando = search.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let clave = crate::cache::clave(&[
        "anilist",
        &page.to_string(),
        sort.as_deref().unwrap_or(""),
        genres.as_deref().unwrap_or(""),
        season.as_deref().unwrap_or(""),
        &season_year.map(|y| y.to_string()).unwrap_or_default(),
    ]);
    if !buscando {
        if let Some(json) = crate::cache::lista_get(&app, &clave) {
            if let Ok(r) = serde_json::from_str::<TmdbListResp>(&json) {
                return Ok(r);
            }
        }
    }

    // Ruta normal. Si AniList está caída (o se cae en este intento), el
    // catálogo lo sirve Kitsu con el MISMO vocabulario de entrada.
    let mut resp: Option<TmdbListResp> = None;
    if !anilist_is_down() {
        match discover_anilist(page, sort.clone(), genres.clone(), search.clone(), season.clone(), season_year).await {
            Ok(r) => resp = Some(r),
            Err(e) => eprintln!("[anilist] discover falló: {e}"),
        }
    }
    let resp = match resp {
        Some(r) => r,
        None => crate::kitsu::discover(page, sort, genres, search, season, season_year)
            .await
            .map_err(|e| format!("El catálogo de anime no está disponible ahora mismo. ({e})"))?,
    };
    // Vacío no se cachea: suele ser AniList/Kitsu fallando, no un catálogo
    // realmente vacío, y quedaría clavado 12 horas.
    if !buscando && !resp.results.is_empty() {
        if let Ok(json) = serde_json::to_string(&resp) {
            crate::cache::lista_put(&app, &clave, &json);
        }
    }
    Ok(resp)
}

async fn discover_anilist(
    page: u32,
    sort: Option<String>,
    genres: Option<String>,
    search: Option<String>,
    season: Option<String>,
    season_year: Option<u32>,
) -> Result<TmdbListResp, String> {
    let search = search.filter(|q| !q.trim().is_empty());
    let genre_list: Vec<String> = genres
        .unwrap_or_default()
        .split(',')
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();

    // Args dinámicos: AniList trata `search: null` como filtro real, así que
    // los campos opcionales solo entran a la query cuando hay valor.
    let mut defs = String::from("$page: Int, $perPage: Int");
    let mut args = String::from("type: ANIME, isAdult: false");
    let mut vars = serde_json::json!({ "page": page, "perPage": PER_PAGE });
    if let Some(q) = &search {
        defs.push_str(", $search: String");
        args.push_str(", search: $search, sort: SEARCH_MATCH");
        vars["search"] = serde_json::json!(q.trim());
    } else {
        defs.push_str(", $sort: [MediaSort]");
        args.push_str(", sort: $sort");
        vars["sort"] = serde_json::json!([sort.unwrap_or_else(|| "POPULARITY_DESC".into())]);
    }
    if !genre_list.is_empty() {
        defs.push_str(", $genres: [String]");
        args.push_str(", genre_in: $genres");
        vars["genres"] = serde_json::json!(genre_list);
    }
    if let (Some(se), Some(sy)) = (season.filter(|s| !s.trim().is_empty()), season_year) {
        defs.push_str(", $season: MediaSeason, $seasonYear: Int");
        args.push_str(", season: $season, seasonYear: $seasonYear");
        vars["season"] = serde_json::json!(se.trim());
        vars["seasonYear"] = serde_json::json!(sy);
    }

    let query = format!(
        "query ({defs}) {{
          Page(page: $page, perPage: $perPage) {{
            pageInfo {{ currentPage lastPage hasNextPage }}
            media({args}) {{
              id
              title {{ romaji english }}
              coverImage {{ extraLarge large }}
              description
              averageScore
              startDate {{ year month day }}
            }}
          }}
        }}"
    );

    let data = gql(&query, vars).await?;
    let pg = &data["Page"];
    let results: Vec<TmdbItem> = pg["media"]
        .as_array()
        .map(|a| a.iter().map(media_to_item).filter(|i| i.id > 0).collect())
        .unwrap_or_default();
    Ok(TmdbListResp {
        page: pg["pageInfo"]["currentPage"].as_u64().unwrap_or(page as u64),
        total_pages: pg["pageInfo"]["lastPage"].as_u64().unwrap_or(page as u64),
        results,
    })
}

// ========================================================================
// ani.zip — episodios + mapeo de IDs (cacheado)
// ========================================================================

struct AnizipEntry {
    at: Instant,
    data: serde_json::Value,
}

/// Caché indexado por `(parámetro, id)`. La clave lleva el nombre del parámetro
/// porque ani.zip se consulta por `anilist_id` (ruta normal) y por `kitsu_id`
/// (ruta de respaldo, ver `kitsu.rs`): son espacios de ids distintos y con una
/// clave solo numérica el anime 21 de AniList pisaría al 21 de Kitsu.
fn anizip_cache() -> &'static Mutex<HashMap<(&'static str, u64), AnizipEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<(&'static str, u64), AnizipEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Trae el mapping de ani.zip por el parámetro indicado ("anilist_id"/"kitsu_id").
pub(crate) async fn anizip_fetch_by(
    param: &'static str,
    id: u64,
) -> Result<serde_json::Value, String> {
    if let Some(e) = anizip_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&(param, id)) {
        if e.at.elapsed() < Duration::from_secs(ANIZIP_TTL_H * 3600) {
            return Ok(e.data.clone());
        }
    }
    let url = format!("{ANIZIP_BASE}{param}={id}");
    let resp = http()?.get(&url).send().await.map_err(|e| format!("anizip red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("anizip {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("anizip parse: {e}"))?;
    anizip_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((param, id), AnizipEntry { at: Instant::now(), data: v.clone() });
    Ok(v)
}

async fn anizip_fetch(anilist_id: u64) -> Result<serde_json::Value, String> {
    anizip_fetch_by("anilist_id", anilist_id).await
}

/// Episodios a partir de un mapping de ani.zip ya bajado. Compartido con la
/// ruta Kitsu, que trae el mismo JSON por otra clave.
pub(crate) fn anizip_to_episodes(data: &serde_json::Value) -> Vec<EpisodeMini> {
    let eps = data["episodes"].as_object().cloned().unwrap_or_default();
    let mut out: Vec<EpisodeMini> = Vec::with_capacity(eps.len());
    for (key, e) in eps {
        // Las keys son "1","2"… para regulares y "S1"… para specials.
        let Ok(n) = key.parse::<u32>() else { continue };
        let name = s(&e["title"]["en"])
            .or_else(|| s(&e["title"]["x-jat"]))
            .or_else(|| s(&e["title"]["ja"]))
            .unwrap_or_else(|| format!("Episodio {n}"));
        out.push(EpisodeMini {
            episode_number: n,
            name,
            overview: s(&e["overview"]).map(|o| strip_html(&o)).unwrap_or_default(),
            still_path: s(&e["image"]),
            air_date: s(&e["airDate"]).or_else(|| s(&e["airdate"])),
            runtime: e["runtime"].as_u64().or_else(|| e["length"].as_u64()).map(|x| x as u32),
        });
    }
    out.sort_by_key(|e| e.episode_number);
    out
}

/// IDs cruzados que entienden los scrapers (kitsu → Torrentio, imdb → resto).
#[derive(Debug, Default, Clone, Serialize)]
pub struct AnimeIds {
    pub mal_id: Option<u64>,
    pub kitsu_id: Option<u64>,
    pub anidb_id: Option<u64>,
    pub imdb_id: Option<String>,
    /// Ficha equivalente en TMDb. Ni AniList ni Kitsu tienen sinopsis en
    /// español, así que de acá sale la traducción (ver `crate::tmdb_overview_es`).
    pub tmdb_id: Option<u64>,
    /// "TV" o "MOVIE" según ani.zip: decide el endpoint TMDb a consultar.
    pub tmdb_type: Option<String>,
}

pub(crate) fn id_num(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|x| x.parse().ok()))
}

pub(crate) fn extract_ids(anizip: &serde_json::Value) -> AnimeIds {
    let m = &anizip["mappings"];
    AnimeIds {
        mal_id: id_num(&m["mal_id"]),
        kitsu_id: id_num(&m["kitsu_id"]),
        anidb_id: id_num(&m["anidb_id"]),
        imdb_id: s(&m["imdb_id"]).filter(|i| i.starts_with("tt")),
        tmdb_id: id_num(&m["themoviedb_id"]),
        tmdb_type: s(&m["type"]),
    }
}

// ========================================================================
// Comando: qué ver después de terminar un anime
// ========================================================================

/// Secuela (o continuación directa) primero, después las recomendaciones de la
/// comunidad. Es lo que alimenta la pantalla de fin: al terminar la temporada 1
/// lo que se quiere ofrecer es la temporada 2, no "otro anime parecido".
///
/// Devuelve el mismo shape que el catálogo (TmdbListResp) para que la pantalla
/// de fin trate igual a anime, series y películas.
#[tauri::command]
pub async fn anilist_relacionados(id: u64) -> Result<TmdbListResp, String> {
    // Id del catálogo de respaldo: lo atiende Kitsu de punta a punta.
    if let Some(kid) = crate::kitsu::split_id(id) {
        return crate::kitsu::relacionados(kid).await;
    }
    // AniList caída: el anime es suyo, pero ani.zip sabe su equivalente en
    // Kitsu y desde ahí se arma la misma lista (secuela + parecidos).
    if anilist_is_down() {
        return por_kitsu(id).await;
    }
    const CAMPOS: &str = "id type isAdult
      title { romaji english }
      coverImage { extraLarge large }
      description
      averageScore
      startDate { year month day }";
    let query = format!(
        "query ($id: Int) {{
          Media(id: $id, type: ANIME) {{
            relations {{ edges {{ relationType(version: 2) node {{ {CAMPOS} }} }} }}
            recommendations(sort: RATING_DESC, perPage: 12) {{
              nodes {{ mediaRecommendation {{ {CAMPOS} }} }}
            }}
          }}
        }}"
    );
    // Si AniList se cae JUSTO acá (es lo normal: la marca como caída este
    // mismo intento), la lista igual sale por Kitsu.
    let data = match gql(&query, serde_json::json!({ "id": id })).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[anilist] relacionados falló: {e}");
            return por_kitsu(id).await;
        }
    };
    let m = &data["Media"];

    let mut results: Vec<TmdbItem> = Vec::new();
    let mut vistos: Vec<u64> = vec![id];
    let push = |nodo: &serde_json::Value, results: &mut Vec<TmdbItem>, vistos: &mut Vec<u64>| {
        // Solo anime reproducible por la ruta normal: nada de manga ni +18.
        if s(&nodo["type"]).as_deref() != Some("ANIME") || nodo["isAdult"].as_bool() == Some(true) {
            return;
        }
        let it = media_to_item(nodo);
        if it.id == 0 || vistos.contains(&it.id) {
            return;
        }
        vistos.push(it.id);
        results.push(it);
    };

    // SEQUEL = la continuación. El resto de relaciones (precuelas, adaptaciones,
    // spin-offs) no son "lo que sigue" y quedan fuera a propósito.
    if let Some(edges) = m["relations"]["edges"].as_array() {
        for e in edges.iter().filter(|e| s(&e["relationType"]).as_deref() == Some("SEQUEL")) {
            push(&e["node"], &mut results, &mut vistos);
        }
    }
    if let Some(nodes) = m["recommendations"]["nodes"].as_array() {
        for n in nodes {
            push(&n["mediaRecommendation"], &mut results, &mut vistos);
        }
    }
    Ok(TmdbListResp { page: 1, total_pages: 1, results })
}

/// Respaldo por Kitsu para un id de AniList: ani.zip traduce el id y Kitsu pone
/// la secuela y los parecidos. Sin traducción no hay nada que ofrecer, y una
/// lista vacía es mejor que un error al terminar un capítulo.
async fn por_kitsu(anilist_id: u64) -> Result<TmdbListResp, String> {
    let vacia = TmdbListResp { page: 1, total_pages: 1, results: Vec::new() };
    let Ok(z) = anizip_fetch(anilist_id).await else { return Ok(vacia) };
    let Some(kid) = extract_ids(&z).kitsu_id else { return Ok(vacia) };
    Ok(crate::kitsu::relacionados(kid).await.unwrap_or(vacia))
}

/// Lista de episodios para el EpisodePicker (mismo shape que tmdb_season).
/// still_path lleva URL completa. Solo episodios regulares (sin specials).
///
/// El `id` puede venir del catálogo AniList o del de respaldo Kitsu; el rango
/// lo dice (ver `kitsu::is_kitsu_id`). El nombre del parámetro sigue siendo
/// `anilistId` para no tocar las llamadas del frontend.
#[tauri::command]
pub async fn anizip_episodes(anilist_id: u64) -> Result<Vec<EpisodeMini>, String> {
    if let Some(kid) = crate::kitsu::split_id(anilist_id) {
        return crate::kitsu::episodes(kid).await;
    }
    let data = anizip_fetch(anilist_id).await?;
    Ok(anizip_to_episodes(&data))
}

// ========================================================================
// Comando: detalle (shape compatible con tmdb_detail + IDs anime)
// ========================================================================

#[derive(Serialize)]
pub struct AnimeDetail {
    pub id: u64,
    pub media_type: String, // "tv" (o "movie" si format MOVIE → fuentes directo)
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
    pub number_of_seasons: Option<u32>,
    pub seasons: Vec<SeasonMini>,
    pub status: Option<String>,
    pub original_title: Option<String>,
    // --- extras anime (no existen en TMDb) ---
    pub is_anime: bool,
    pub mal_id: Option<u64>,
    pub kitsu_id: Option<u64>,
    pub anidb_id: Option<u64>,
    pub trailer_youtube: Option<String>,
    pub format: Option<String>,
}

/// El `id` puede ser de AniList o de Kitsu: el rango lo dice. Así un listado
/// servido por el respaldo sigue funcionando aunque AniList reviva entremedio
/// (si no, el id de Kitsu se consultaría contra AniList y traería otro anime).
#[tauri::command]
///
/// `api_key` (TMDb) es opcional y solo se usa para traer la sinopsis en
/// español: sin ella el detalle funciona igual, con el texto en inglés.
pub async fn anilist_detail(id: u64, api_key: Option<String>) -> Result<AnimeDetail, String> {
    let key = api_key.unwrap_or_default();
    if let Some(kid) = crate::kitsu::split_id(id) {
        return crate::kitsu::detail(kid, &key).await;
    }
    if anilist_is_down() {
        return Err("El detalle de este anime no está disponible: AniList no responde.".into());
    }
    detail_anilist(id, &key).await
}

/// Sinopsis en español si TMDb la tiene; si no, la original que ya venía.
/// Silencioso a propósito: una traducción que falla no rompe la ficha.
pub(crate) async fn overview_es_or(
    original: String,
    ids: &AnimeIds,
    api_key: &str,
) -> String {
    let Some(tmdb_id) = ids.tmdb_id else { return original };
    crate::tmdb_overview_es(ids.tmdb_type.as_deref(), tmdb_id, api_key)
        .await
        .unwrap_or(original)
}

/// Formatos que cuentan como temporada real de la historia principal: un
/// PREQUEL en OVA/ONA/special/movie es una historia lateral, no "temporada
/// anterior", así que no debe sumar al número.
fn is_season_format(fmt: &str) -> bool {
    matches!(fmt, "TV" | "TV_SHORT")
}

/// El PREQUEL (TV) más cercano en `relations.edges`, si hay uno.
fn tv_prequel_id(m: &serde_json::Value) -> Option<u64> {
    m["relations"]["edges"].as_array()?.iter().find_map(|e| {
        if e["relationType"].as_str() != Some("PREQUEL") {
            return None;
        }
        let node = &e["node"];
        if !is_season_format(node["format"].as_str().unwrap_or("")) {
            return None;
        }
        node["id"].as_u64()
    })
}

/// AniList no tiene "número de temporada": cada entrega (ej. "... Season 2")
/// es un Media aparte, sin relación con el 1 salvo por `relations`. Subimos
/// la cadena de PREQUELs de formato TV contando saltos para saber si esto es
/// la temporada 1, 2, 3… Si un salto falla (AniList cae a medio camino) nos
/// quedamos con lo ya contado — el conteo es un detalle cosmético, nunca
/// motivo para romper la ficha completa.
async fn climb_season_number(first: &serde_json::Value) -> u32 {
    let mut n: u32 = 1;
    let mut next = tv_prequel_id(first);
    for _ in 0..12 {
        let Some(id) = next else { break };
        let query = "query ($id: Int) {
          Media(id: $id, type: ANIME) {
            relations { edges { relationType(version: 2) node { id format } } }
          }
        }";
        let Ok(data) = gql(query, serde_json::json!({ "id": id })).await else { break };
        let m = &data["Media"];
        if m.is_null() {
            break;
        }
        n += 1;
        next = tv_prequel_id(m);
    }
    n
}

async fn detail_anilist(id: u64, api_key: &str) -> Result<AnimeDetail, String> {
    let query = "query ($id: Int) {
      Media(id: $id, type: ANIME) {
        id idMal
        title { romaji english native }
        description(asHtml: false)
        coverImage { extraLarge large }
        bannerImage
        averageScore
        episodes
        duration
        genres
        format
        status
        startDate { year }
        trailer { id site }
        studios(isMain: true) { nodes { name } }
        relations { edges { relationType(version: 2) node { id format } } }
        characters(sort: ROLE, perPage: 12) {
          edges {
            node { name { full } image { large } }
            voiceActors(language: JAPANESE) { name { full } }
          }
        }
      }
    }";
    // AniList y ani.zip en paralelo: los IDs cruzados (kitsu/imdb) salen de
    // ani.zip; si ani.zip cae, el detalle igual sirve (sin ruta debrid kitsu).
    let (data, anizip) = tokio::join!(
        gql(query, serde_json::json!({ "id": id })),
        anizip_fetch(id)
    );
    let data = data?;
    let m = &data["Media"];
    if m.is_null() {
        return Err(format!("anilist: media {id} no encontrado"));
    }
    let ids = anizip.as_ref().map(|z| extract_ids(z)).unwrap_or_default();

    let format = s(&m["format"]);
    let is_movie = format.as_deref() == Some("MOVIE");
    let episode_count = m["episodes"].as_u64().map(|x| x as u32).or_else(|| {
        // En emisión: AniList aún no sabe el total → contamos lo emitido (ani.zip).
        anizip
            .as_ref()
            .ok()
            .and_then(|z| z["episodes"].as_object().map(|e| {
                e.keys().filter(|k| k.parse::<u32>().is_ok()).count() as u32
            }))
            .filter(|n| *n > 0)
    });

    let cast: Vec<PersonMini> = m["characters"]["edges"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let name = s(&e["node"]["name"]["full"])?;
                    let va = e["voiceActors"]
                        .as_array()
                        .and_then(|v| v.first())
                        .and_then(|v| s(&v["name"]["full"]));
                    Some(PersonMini {
                        id: 0, // sin página de persona TMDb: la UI no abre ficha
                        name,
                        profile_path: s(&e["node"]["image"]["large"]),
                        character: va, // se muestra como subtítulo: seiyū
                        job: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let directors: Vec<PersonMini> = m["studios"]["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|n| s(&n["name"]))
                .map(|name| PersonMini {
                    id: 0,
                    name,
                    profile_path: None,
                    character: None,
                    job: Some("Estudio".into()),
                })
                .collect()
        })
        .unwrap_or_default();

    let trailer_youtube = (s(&m["trailer"]["site"]).as_deref() == Some("youtube"))
        .then(|| s(&m["trailer"]["id"]))
        .flatten();

    let poster = s(&m["coverImage"]["extraLarge"]).or_else(|| s(&m["coverImage"]["large"]));
    let banner = s(&m["bannerImage"]);
    let year = m["startDate"]["year"].as_u64().map(|y| y.to_string()).unwrap_or_default();

    // AniList solo tiene sinopsis en inglés: si TMDb la tiene en español, gana.
    let overview = overview_es_or(
        s(&m["description"]).map(|d| strip_html(&d)).unwrap_or_default(),
        &ids,
        api_key,
    )
    .await;

    // Solo vale la pena subir la cadena de PREQUELs para series: una
    // película no tiene "temporada".
    let season_num = if is_movie { 1 } else { climb_season_number(m).await };

    Ok(AnimeDetail {
        id: m["id"].as_u64().unwrap_or(id),
        media_type: if is_movie { "movie" } else { "tv" }.into(),
        title: media_title(m),
        overview,
        poster_path: poster.clone(),
        backdrop_path: banner.or(poster),
        vote_average: m["averageScore"].as_f64().map(|x| x / 10.0).unwrap_or(0.0) as f32,
        year: year.clone(),
        imdb_id: ids.imdb_id.clone(),
        runtime: m["duration"].as_u64().map(|x| x as u32),
        genres: m["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| g.as_str().map(genre_es)).collect())
            .unwrap_or_default(),
        directors,
        cast,
        images: Vec::new(),
        number_of_seasons: (!is_movie).then_some(1),
        seasons: if is_movie {
            Vec::new()
        } else {
            vec![SeasonMini {
                season_number: season_num,
                episode_count: episode_count.unwrap_or(0),
                name: "Episodios".into(),
                air_date: (!year.is_empty()).then(|| format!("{year}-01-01")),
                poster_path: None,
            }]
        },
        status: s(&m["status"]).map(|st| status_es(&st)),
        original_title: s(&m["title"]["romaji"]),
        is_anime: true,
        mal_id: id_num(&m["idMal"]).or(ids.mal_id),
        kitsu_id: ids.kitsu_id,
        anidb_id: ids.anidb_id,
        trailer_youtube,
        format,
    })
}

/// Géneros AniList → etiqueta en español neutro para la UI.
pub(crate) fn genre_es(g: &str) -> String {
    match g {
        "Action" => "Acción",
        "Adventure" => "Aventura",
        "Comedy" => "Comedia",
        "Drama" => "Drama",
        "Ecchi" => "Ecchi",
        "Fantasy" => "Fantasía",
        "Horror" => "Terror",
        "Mahou Shoujo" => "Mahou Shoujo",
        "Mecha" => "Mecha",
        "Music" => "Música",
        "Mystery" => "Misterio",
        "Psychological" => "Psicológico",
        "Romance" => "Romance",
        "Sci-Fi" => "Ciencia ficción",
        "Slice of Life" => "Recuentos de la vida",
        "Sports" => "Deportes",
        "Supernatural" => "Sobrenatural",
        "Thriller" => "Suspenso",
        other => other,
    }
    .to_string()
}

fn status_es(st: &str) -> String {
    match st {
        "RELEASING" => "En emisión",
        "FINISHED" => "Finalizado",
        "NOT_YET_RELEASED" => "Próximamente",
        "CANCELLED" => "Cancelado",
        "HIATUS" => "En pausa",
        other => other,
    }
    .to_string()
}

// ========================================================================
// Smoke test del respaldo, contra la red. Ver nota en `kitsu.rs::tests`.
// cargo test --lib anilist -- --ignored --nocapture
// ========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// "Qué ver después" tiene que responder con AniList sana o caída: con la
    /// API apagada, ani.zip traduce el id y contesta Kitsu. 16498 = Attack on
    /// Titan (id de AniList).
    #[tokio::test]
    #[ignore]
    async fn relacionados_responde_con_anilist_caida_o_no() {
        let r = anilist_relacionados(16498).await.expect("relacionados");
        assert!(!r.results.is_empty(), "sin nada que ofrecer al terminar");
        for it in &r.results {
            assert_ne!(it.id, 16498, "no se recomienda a sí mismo");
            assert!(it.name.as_deref().is_some_and(|n| !n.is_empty()));
        }
        let fuente = if crate::kitsu::split_id(r.results[0].id).is_some() {
            "kitsu"
        } else {
            "anilist"
        };
        eprintln!(
            "OK  fuente={fuente} | {} sugerencias | 1ª: {}",
            r.results.len(),
            r.results[0].name.clone().unwrap_or_default()
        );
    }

    /// La invariante que importa: la pestaña anime devuelve resultados, sirva
    /// quien sirva. Con AniList caída tiene que responder Kitsu; si AniList
    /// revive, responde ella. El test pasa en los dos casos a propósito — lo
    /// que NO puede pasar es que la pestaña quede vacía.
    #[tokio::test]
    #[ignore]
    async fn el_catalogo_anime_responde_igual() {
        // El comando en sí pide un AppHandle (cachea la página en disco), que
        // no existe en un test. Se replica su ruta sin cache: AniList y, si no
        // contesta, Kitsu.
        let sort = Some("POPULARITY_DESC".to_string());
        let r = match discover_anilist(1, sort.clone(), None, None, None, None).await {
            Ok(r) if !r.results.is_empty() => r,
            _ => crate::kitsu::discover(1, sort, None, None, None, None)
                .await
                .expect("catálogo"),
        };
        assert!(!r.results.is_empty(), "pestaña anime vacía");

        let first = r.results[0].id;
        let fuente = if crate::kitsu::split_id(first).is_some() { "kitsu" } else { "anilist" };

        // El detalle tiene que enrutar al MISMO catálogo que sirvió la lista.
        // Sin key de TMDb: la sinopsis queda en inglés, que es justo lo que
        // este test NO mira. La traducción tiene su propio test.
        let d = anilist_detail(first, None).await.expect("detalle");
        assert_eq!(d.id, first, "el detalle cambió de id → se enrutó a la fuente equivocada");
        assert!(!d.title.is_empty());

        eprintln!("OK  fuente={fuente} | {} | {} | kitsu_id={:?}", d.title, d.year, d.kitsu_id);
    }

    /// El caso que motivó `climb_season_number`: AniList trata cada temporada
    /// como un Media aparte, así que "Skeleton Knight... Season 2" (185542)
    /// venía marcado como "Temporada 1" a secas. 185542 → PREQUEL TV 132474
    /// (temporada 1, sin PREQUEL propio) → la cadena tiene que dar 2.
    #[tokio::test]
    #[ignore]
    async fn temporada_2_de_una_saga_no_se_muestra_como_1() {
        let d = detail_anilist(185542, "").await.expect("detalle");
        assert_eq!(d.title, "Skeleton Knight in Another World Season 2");
        assert_eq!(
            d.seasons.first().map(|s| s.season_number),
            Some(2),
            "no subió la cadena de PREQUEL TV"
        );
    }
}
