// Kitsu — catálogo anime de RESPALDO cuando AniList no sirve.
//
// AniList apaga su API cada tanto (403 "The AniList API has been temporarily
// disabled"). `anilist.rs` detecta la caída y redirige acá. Este módulo NO se
// usa mientras AniList esté sana.
//
// Diseño: acepta el MISMO vocabulario que ya manda el frontend (MediaSort de
// AniList, géneros en inglés, MediaSeason) y lo traduce a los parámetros
// JSON:API de Kitsu. Así el frontend no cambia ni una línea.
//
// Bonus: el id de Kitsu es JUSTO el que necesitan los scrapers de anime
// (torrentio-kitsu). Por la ruta AniList hay que pedirlo a ani.zip; por acá
// ya lo tenemos.

use crate::anilist::{
    anizip_fetch_by, anizip_to_episodes, extract_ids, genre_es, id_num, s, AnimeDetail,
};
use crate::{EpisodeMini, PersonMini, SeasonMini, TmdbItem, TmdbListResp};
use std::time::Duration;

const BASE: &str = "https://kitsu.io/api/edge/anime";
/// Relaciones entre títulos (secuela, precuela, spin-off…): recurso aparte.
const REL_BASE: &str = "https://kitsu.io/api/edge/media-relationships";
/// Tope duro de la API: `page[limit]` > 20 devuelve 400.
const PER_PAGE: u32 = 20;

// ========================================================================
// Espacio de ids
// ========================================================================
//
// Los ids de AniList y de Kitsu son universos distintos: el 21 de AniList es
// One Piece, el 21 de Kitsu es otra cosa. Los comandos del frontend mandan un
// solo `id` sin decir de dónde salió, así que le sumamos una base a los de
// Kitsu para que el rango identifique la fuente.
//
// Alternativa descartada: recordar "la fuente activa" en una variable global.
// Se rompe en el caso real que importa — listado servido por Kitsu, AniList
// revive, y el detalle de un id de Kitsu termina pedido a AniList.
//
// 100M deja lugar de sobra: Kitsu anda por los 60k ids y AniList por los 200k.
pub const KITSU_ID_BASE: u64 = 100_000_000;

/// Id público (el que ve el frontend) para un anime de Kitsu.
pub fn public_id(kitsu_id: u64) -> u64 {
    kitsu_id + KITSU_ID_BASE
}

/// `Some(kitsu_id)` si el id público vino del catálogo Kitsu; `None` si es de
/// AniList y lo tiene que atender el módulo de siempre.
pub fn split_id(public: u64) -> Option<u64> {
    public.checked_sub(KITSU_ID_BASE).filter(|_| public >= KITSU_ID_BASE)
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("kutral/0.1")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| format!("client: {e}"))
}

async fn get(url: &str) -> Result<serde_json::Value, String> {
    let resp = http()?.get(url).send().await.map_err(|e| format!("kitsu red: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("kitsu {}", resp.status()));
    }
    resp.json().await.map_err(|e| format!("kitsu parse: {e}"))
}

// ========================================================================
// Traducción de vocabulario AniList → Kitsu
// ========================================================================

/// MediaSort de AniList → `sort` de Kitsu.
///
/// OJO con TRENDING_DESC: Kitsu no tiene concepto de "tendencia" (qué se está
/// viendo ahora). Cae a `-userCount`, que es popularidad histórica. El orden
/// "Tendencia" de la UI sigue funcionando pero deja de ser en vivo mientras
/// dure el respaldo.
fn sort_kitsu(sort: Option<&str>) -> &'static str {
    match sort.unwrap_or("POPULARITY_DESC") {
        "SCORE_DESC" => "-averageRating",
        "FAVOURITES_DESC" => "-favoritesCount",
        "START_DATE_DESC" => "-startDate",
        "START_DATE" => "startDate",
        "TITLE_ENGLISH" => "slug",
        "TITLE_ENGLISH_DESC" => "-slug",
        // POPULARITY_DESC, TRENDING_DESC y cualquier otro.
        _ => "-userCount",
    }
}

/// Género AniList (inglés) → slug de categoría de Kitsu.
fn genre_slug(g: &str) -> Option<&'static str> {
    Some(match g {
        "Action" => "action",
        "Adventure" => "adventure",
        "Comedy" => "comedy",
        "Drama" => "drama",
        "Fantasy" => "fantasy",
        "Horror" => "horror",
        "Mecha" => "mecha",
        "Music" => "music",
        "Mystery" => "mystery",
        "Psychological" => "psychological",
        "Romance" => "romance",
        "Sci-Fi" => "science-fiction",
        "Slice of Life" => "slice-of-life",
        "Sports" => "sports",
        "Supernatural" => "supernatural",
        "Thriller" => "thriller",
        _ => return None,
    })
}

/// Estado de Kitsu → etiqueta en español neutro (mismo criterio que AniList).
fn status_es(st: &str) -> String {
    match st {
        "current" => "En emisión",
        "finished" => "Finalizado",
        "tba" | "unreleased" | "upcoming" => "Próximamente",
        other => other,
    }
    .to_string()
}

fn attr(a: &serde_json::Value, k: &str) -> Option<String> {
    s(&a[k])
}

/// Kitsu da el rating como string 0-100 ("88.81"); la UI espera 0-10.
fn rating_10(a: &serde_json::Value) -> f32 {
    a["averageRating"]
        .as_str()
        .and_then(|x| x.parse::<f32>().ok())
        .map(|x| x / 10.0)
        .unwrap_or(0.0)
}

fn title_of(a: &serde_json::Value) -> String {
    attr(a, "canonicalTitle")
        .or_else(|| s(&a["titles"]["en"]))
        .or_else(|| s(&a["titles"]["en_jp"]))
        .unwrap_or_default()
}

fn poster_of(a: &serde_json::Value) -> Option<String> {
    s(&a["posterImage"]["large"]).or_else(|| s(&a["posterImage"]["original"]))
}

fn node_to_item(node: &serde_json::Value) -> TmdbItem {
    let a = &node["attributes"];
    TmdbItem {
        id: id_num(&node["id"]).map(public_id).unwrap_or(0),
        title: None,
        name: Some(title_of(a)),
        poster_path: poster_of(a),
        overview: attr(a, "synopsis").unwrap_or_default(),
        vote_average: rating_10(a),
        release_date: None,
        first_air_date: attr(a, "startDate"),
        // Vera no consume estas fuentes (solo TMDb); campos neutros.
        genre_ids: Vec::new(),
        vote_count: None,
        popularity: None,
        original_language: None,
    }
}

// ========================================================================
// Catálogo / búsqueda
// ========================================================================

pub async fn discover(
    page: u32,
    sort: Option<String>,
    genres: Option<String>,
    search: Option<String>,
    season: Option<String>,
    season_year: Option<u32>,
) -> Result<TmdbListResp, String> {
    let page = page.max(1);
    let offset = (page - 1) * PER_PAGE;
    let mut url = format!("{BASE}?page%5Blimit%5D={PER_PAGE}&page%5Boffset%5D={offset}");

    let search = search.filter(|q| !q.trim().is_empty());
    if let Some(q) = &search {
        // Con texto, Kitsu ordena por relevancia solo: no mandamos `sort`
        // (mismo criterio que SEARCH_MATCH en AniList).
        url.push_str(&format!("&filter%5Btext%5D={}", urlencoding::encode(q.trim())));
    } else {
        url.push_str(&format!("&sort={}", sort_kitsu(sort.as_deref())));
    }

    let slugs: Vec<&str> = genres
        .unwrap_or_default()
        .split(',')
        .filter_map(|g| genre_slug(g.trim()))
        .collect();
    if !slugs.is_empty() {
        // Coma = AND en Kitsu, igual que `genre_in` de AniList.
        url.push_str(&format!("&filter%5Bcategories%5D={}", slugs.join(",")));
    }

    // El cour solo aplica con temporada Y año, igual que en AniList.
    if let (Some(se), Some(sy)) = (season.filter(|x| !x.trim().is_empty()), season_year) {
        url.push_str(&format!(
            "&filter%5Bseason%5D={}&filter%5BseasonYear%5D={sy}",
            se.trim().to_lowercase()
        ));
    }

    let v = get(&url).await?;
    let results: Vec<TmdbItem> = v["data"]
        .as_array()
        .map(|a| a.iter().map(node_to_item).filter(|i| i.id > KITSU_ID_BASE).collect())
        .unwrap_or_default();
    let count = v["meta"]["count"].as_u64().unwrap_or(0);
    Ok(TmdbListResp {
        page: page as u64,
        total_pages: count.div_ceil(PER_PAGE as u64).max(1),
        results,
    })
}

// ========================================================================
// Qué ver después (respaldo de las recomendaciones de AniList)
// ========================================================================

/// Secuela + parecidos, con lo que da Kitsu.
///
/// Kitsu no tiene "recomendaciones de la comunidad" como AniList, así que los
/// parecidos salen de sus categorías: mismos géneros, ordenados por
/// popularidad. Es más grueso que una recomendación real, pero mantiene la
/// pantalla de fin viva mientras AniList esté caída.
pub async fn relacionados(kitsu_id: u64) -> Result<TmdbListResp, String> {
    let mut results: Vec<TmdbItem> = Vec::new();
    let mut vistos: Vec<u64> = vec![public_id(kitsu_id)];

    // La continuación primero: es lo que se quiere al terminar una temporada.
    // La relación vive en su propio recurso (`/anime/{id}/mediaRelationships`
    // no existe: 404 Route Not Found).
    if let Ok(v) = get(&format!(
        "{REL_BASE}?filter%5Bsource_id%5D={kitsu_id}&filter%5Bsource_type%5D=Anime\
         &include=destination&page%5Blimit%5D=20"
    ))
    .await
    {
        // Los destinos vienen en `included`; `data` dice el rol de cada uno.
        let destinos: Vec<&serde_json::Value> = v["included"]
            .as_array()
            .map(|a| a.iter().filter(|n| n["type"] == "anime").collect())
            .unwrap_or_default();
        if let Some(rels) = v["data"].as_array() {
            for rel in rels
                .iter()
                .filter(|r| s(&r["attributes"]["role"]).as_deref() == Some("sequel"))
            {
                let dest = &rel["relationships"]["destination"]["data"];
                if dest["type"] != "anime" {
                    continue;
                }
                let Some(nodo) = destinos.iter().find(|n| n["id"] == dest["id"]) else { continue };
                empujar(nodo, &mut results, &mut vistos);
            }
        }
    }

    // Parecidos por categoría. Dos categorías como mucho: con más, el filtro
    // (que es AND) deja de devolver nada.
    let cats = categorias(kitsu_id).await;
    if !cats.is_empty() {
        let url = format!(
            "{BASE}?filter%5Bcategories%5D={}&sort=-userCount&page%5Blimit%5D={PER_PAGE}",
            cats.join(",")
        );
        if let Ok(v) = get(&url).await {
            if let Some(a) = v["data"].as_array() {
                for nodo in a {
                    empujar(nodo, &mut results, &mut vistos);
                }
            }
        }
    }
    Ok(TmdbListResp { page: 1, total_pages: 1, results })
}

fn empujar(nodo: &serde_json::Value, results: &mut Vec<TmdbItem>, vistos: &mut Vec<u64>) {
    let it = node_to_item(nodo);
    if it.id <= KITSU_ID_BASE || vistos.contains(&it.id) {
        return;
    }
    vistos.push(it.id);
    results.push(it);
}

/// Slugs de categoría con los que buscar parecidos.
///
/// Kitsu etiqueta con categorías MUY finas ("post-apocalypse", "violence"):
/// filtrar por esas devuelve cuatro títulos de nicho. Se prefieren los géneros
/// grandes (los mismos que traduce `genre_slug`), y solo si el anime no tiene
/// ninguno se cae a lo que haya.
async fn categorias(kitsu_id: u64) -> Vec<String> {
    let Ok(v) = get(&format!("{BASE}/{kitsu_id}?include=categories")).await else {
        return Vec::new();
    };
    let todas: Vec<String> = v["included"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| c["type"] == "categories")
                .filter_map(|c| s(&c["attributes"]["slug"]))
                .collect()
        })
        .unwrap_or_default();
    let amplias: Vec<String> = todas
        .iter()
        .filter(|slug| SLUGS_AMPLIOS.contains(&slug.as_str()))
        .take(2)
        .cloned()
        .collect();
    if !amplias.is_empty() {
        return amplias;
    }
    todas.into_iter().take(2).collect()
}

/// Los géneros "de estante" de Kitsu (mismo conjunto que `genre_slug`).
const SLUGS_AMPLIOS: &[&str] = &[
    "action",
    "adventure",
    "comedy",
    "drama",
    "fantasy",
    "horror",
    "mecha",
    "music",
    "mystery",
    "psychological",
    "romance",
    "science-fiction",
    "slice-of-life",
    "sports",
    "supernatural",
    "thriller",
];

// ========================================================================
// Episodios
// ========================================================================

/// ani.zip acepta `kitsu_id` directo, así que la lista sale con los mismos
/// títulos y thumbnails que por la ruta AniList. Si ani.zip no tiene el anime,
/// caemos a los episodios del propio Kitsu (sin imagen en muchos casos).
pub async fn episodes(kitsu_id: u64) -> Result<Vec<EpisodeMini>, String> {
    if let Ok(z) = anizip_fetch_by("kitsu_id", kitsu_id).await {
        let eps = anizip_to_episodes(&z);
        if !eps.is_empty() {
            return Ok(eps);
        }
    }
    let url = format!("{BASE}/{kitsu_id}/episodes?page%5Blimit%5D={PER_PAGE}&sort=number");
    let v = get(&url).await?;
    let mut out: Vec<EpisodeMini> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let at = &e["attributes"];
                    let n = at["number"].as_u64()? as u32;
                    Some(EpisodeMini {
                        episode_number: n,
                        name: attr(at, "canonicalTitle")
                            .or_else(|| s(&at["titles"]["en_us"]))
                            .unwrap_or_else(|| format!("Episodio {n}")),
                        overview: attr(at, "synopsis").unwrap_or_default(),
                        still_path: s(&at["thumbnail"]["original"]),
                        air_date: attr(at, "airdate"),
                        runtime: at["length"].as_u64().map(|x| x as u32),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|e| e.episode_number);
    Ok(out)
}

// ========================================================================
// Detalle
// ========================================================================

pub async fn detail(kitsu_id: u64, api_key: &str) -> Result<AnimeDetail, String> {
    // Kitsu y ani.zip en paralelo: los IDs cruzados (imdb/mal/anidb) salen de
    // ani.zip, pero el detalle sirve igual si ani.zip cae — el kitsu_id, que
    // es el que habilita la ruta debrid, ya lo tenemos.
    let url = format!("{BASE}/{kitsu_id}?include=categories");
    let (v, anizip) = tokio::join!(get(&url), anizip_fetch_by("kitsu_id", kitsu_id));
    let v = v?;
    let a = &v["data"]["attributes"];
    if a.is_null() {
        return Err(format!("kitsu: anime {kitsu_id} no encontrado"));
    }
    let ids = anizip.as_ref().map(|z| extract_ids(z)).unwrap_or_default();

    // Kitsu tampoco tiene sinopsis en español: si TMDb la tiene, gana.
    let overview =
        crate::anilist::overview_es_or(attr(a, "synopsis").unwrap_or_default(), &ids, api_key).await;

    // "movie"/"special"/"OVA"… → ficha de película (sin lista de episodios).
    let subtype = attr(a, "subtype").unwrap_or_default();
    let is_movie = matches!(subtype.as_str(), "movie" | "special" | "ONA" | "OVA")
        && a["episodeCount"].as_u64().unwrap_or(0) <= 1;

    let year = attr(a, "startDate")
        .and_then(|d| d.get(0..4).map(str::to_string))
        .unwrap_or_default();
    let poster = poster_of(a);
    let banner = s(&a["coverImage"]["original"]).or_else(|| s(&a["coverImage"]["large"]));

    // Las categorías vienen en `included` (relación categories del include).
    let genres: Vec<String> = v["included"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| c["type"] == "categories")
                .filter_map(|c| s(&c["attributes"]["title"]))
                .map(|t| genre_es(&t))
                .collect()
        })
        .unwrap_or_default();

    let episode_count = a["episodeCount"].as_u64().map(|x| x as u32).or_else(|| {
        // En emisión: Kitsu aún no sabe el total → contamos lo emitido.
        anizip
            .as_ref()
            .ok()
            .and_then(|z| {
                z["episodes"]
                    .as_object()
                    .map(|e| e.keys().filter(|k| k.parse::<u32>().is_ok()).count() as u32)
            })
            .filter(|n| *n > 0)
    });

    Ok(AnimeDetail {
        id: public_id(kitsu_id),
        media_type: if is_movie { "movie" } else { "tv" }.into(),
        title: title_of(a),
        overview,
        poster_path: poster.clone(),
        backdrop_path: banner.or(poster),
        vote_average: rating_10(a),
        year: year.clone(),
        imdb_id: ids.imdb_id.clone(),
        runtime: a["episodeLength"].as_u64().map(|x| x as u32),
        genres,
        // Kitsu tiene staff y personajes, pero cada uno es otra llamada con
        // paginación propia. Como esto es modo degradado, se omiten: la ficha
        // se ve sin reparto y todo lo demás funciona igual.
        directors: Vec::<PersonMini>::new(),
        cast: Vec::<PersonMini>::new(),
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
        status: attr(a, "status").map(|st| status_es(&st)),
        original_title: s(&a["titles"]["en_jp"]).or_else(|| s(&a["titles"]["ja_jp"])),
        is_anime: true,
        mal_id: ids.mal_id,
        kitsu_id: Some(kitsu_id),
        anidb_id: ids.anidb_id,
        trailer_youtube: attr(a, "youtubeVideoId"),
        format: Some(subtype),
    })
}

// ========================================================================
// Smoke test manual contra la API en vivo.
// `#[ignore]` porque pega a la red: no corre en CI ni en `cargo test` pelado.
// Para correrlo:  cargo test --lib kitsu -- --ignored --nocapture
// ========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_van_y_vuelven() {
        assert_eq!(split_id(public_id(46474)), Some(46474));
        // Un id de AniList (rango bajo) no se confunde con uno de Kitsu.
        assert_eq!(split_id(154587), None);
        assert_eq!(split_id(0), None);
    }

    /// Red real: la pantalla de fin servida por el respaldo. Attack on Titan
    /// (7442) tiene secuela declarada y categorías de sobra.
    #[tokio::test]
    #[ignore]
    async fn relacionados_trae_secuela_y_parecidos() {
        let r = relacionados(7442).await.expect("relacionados");
        assert!(r.results.len() > 3, "muy pocos: {}", r.results.len());
        // La secuela va primero: es lo que se ofrece al terminar la temporada.
        let primero = r.results[0].name.clone().unwrap_or_default();
        assert!(
            primero.contains("Attack on Titan"),
            "el primero debería ser la continuación, fue: {primero}"
        );
        for it in &r.results {
            assert!(it.id > KITSU_ID_BASE, "id sin base: {}", it.id);
            assert_ne!(it.id, public_id(7442), "no se recomienda a sí mismo");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn catalogo_busqueda_y_detalle() {
        let r = discover(1, Some("POPULARITY_DESC".into()), None, None, None, None)
            .await
            .expect("discover");
        assert_eq!(r.results.len(), PER_PAGE as usize);
        assert!(r.total_pages > 1);
        for it in &r.results {
            assert!(it.id > KITSU_ID_BASE, "id sin base: {}", it.id);
            assert!(it.name.as_deref().is_some_and(|n| !n.is_empty()));
        }

        let b = discover(1, None, None, Some("frieren".into()), None, None)
            .await
            .expect("búsqueda");
        assert!(
            b.results.iter().any(|i| i.name.as_deref().unwrap_or("").contains("Frieren")),
            "la búsqueda no encontró Frieren: {:?}",
            b.results.iter().map(|i| i.name.clone()).collect::<Vec<_>>()
        );

        let kid = split_id(b.results[0].id).expect("id de kitsu");
        let d = detail(kid, "").await.expect("detalle");
        assert_eq!(d.id, b.results[0].id, "el detalle debe devolver el id público");
        assert_eq!(d.kitsu_id, Some(kid), "kitsu_id habilita la ruta debrid");
        assert!(!d.title.is_empty() && !d.genres.is_empty());

        let eps = episodes(kid).await.expect("episodios");
        assert!(!eps.is_empty(), "sin episodios");
        assert_eq!(eps[0].episode_number, 1);

        eprintln!(
            "OK  {} | {} | {} géneros | {} eps | imdb={:?} | tráiler={:?}",
            d.title, d.year, d.genres.len(), eps.len(), d.imdb_id, d.trailer_youtube
        );
    }
}
