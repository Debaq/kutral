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
const ANIZIP_URL: &str = "https://api.ani.zip/mappings?anilist_id=";
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
// GraphQL plumbing
// ========================================================================

async fn gql(query: &str, variables: serde_json::Value) -> Result<serde_json::Value, String> {
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

fn s(v: &serde_json::Value) -> Option<String> {
    v.as_str().filter(|x| !x.is_empty()).map(|x| x.to_string())
}

/// AniList manda descripciones con HTML liviano (<br>, <i>…). Lo limpiamos.
fn strip_html(t: &str) -> String {
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
    }
}

// ========================================================================
// Comando: catálogo / búsqueda
// ========================================================================

/// Catálogo anime vía AniList. `sort` es un MediaSort de AniList
/// (POPULARITY_DESC, SCORE_DESC, TRENDING_DESC, START_DATE_DESC, …).
/// `genres` es CSV de géneros AniList en inglés ("Action,Romance").
/// Con `search`, AniList ordena por relevancia y se ignora `sort`.
#[tauri::command]
pub async fn anilist_discover(
    page: u32,
    sort: Option<String>,
    genres: Option<String>,
    search: Option<String>,
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

fn anizip_cache() -> &'static Mutex<HashMap<u64, AnizipEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, AnizipEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn anizip_fetch(anilist_id: u64) -> Result<serde_json::Value, String> {
    if let Some(e) = anizip_cache().lock().unwrap().get(&anilist_id) {
        if e.at.elapsed() < Duration::from_secs(ANIZIP_TTL_H * 3600) {
            return Ok(e.data.clone());
        }
    }
    let url = format!("{ANIZIP_URL}{anilist_id}");
    let resp = http()?.get(&url).send().await.map_err(|e| format!("anizip red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("anizip {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("anizip parse: {e}"))?;
    anizip_cache()
        .lock()
        .unwrap()
        .insert(anilist_id, AnizipEntry { at: Instant::now(), data: v.clone() });
    Ok(v)
}

/// IDs cruzados que entienden los scrapers (kitsu → Torrentio, imdb → resto).
#[derive(Debug, Default, Clone, Serialize)]
pub struct AnimeIds {
    pub mal_id: Option<u64>,
    pub kitsu_id: Option<u64>,
    pub anidb_id: Option<u64>,
    pub imdb_id: Option<String>,
}

fn id_num(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|x| x.parse().ok()))
}

fn extract_ids(anizip: &serde_json::Value) -> AnimeIds {
    let m = &anizip["mappings"];
    AnimeIds {
        mal_id: id_num(&m["mal_id"]),
        kitsu_id: id_num(&m["kitsu_id"]),
        anidb_id: id_num(&m["anidb_id"]),
        imdb_id: s(&m["imdb_id"]).filter(|i| i.starts_with("tt")),
    }
}

/// Lista de episodios para el EpisodePicker (mismo shape que tmdb_season).
/// still_path lleva URL completa. Solo episodios regulares (sin specials).
#[tauri::command]
pub async fn anizip_episodes(anilist_id: u64) -> Result<Vec<EpisodeMini>, String> {
    let data = anizip_fetch(anilist_id).await?;
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
    Ok(out)
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

#[tauri::command]
pub async fn anilist_detail(id: u64) -> Result<AnimeDetail, String> {
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

    Ok(AnimeDetail {
        id: m["id"].as_u64().unwrap_or(id),
        media_type: if is_movie { "movie" } else { "tv" }.into(),
        title: media_title(m),
        overview: s(&m["description"]).map(|d| strip_html(&d)).unwrap_or_default(),
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
                season_number: 1,
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
fn genre_es(g: &str) -> String {
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
