// Vera: catálogo local de TMDb para el recomendador (importador y listas de
// opciones). El motor en sí vive en el front (src/lib/vera).

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::{client, fetch_json, ExternalIds, TmdbListResp, LANG, TMDB_BASE};

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
pub fn vera_intent_options() -> Vec<VeraOption> {
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
pub fn vera_genre_list() -> Vec<VeraOption> {
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
pub fn vera_theme_list() -> Vec<VeraOption> {
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
pub(crate) fn map_keyword_to_themes(kw_lower: &str) -> Vec<&'static str> {
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
pub(crate) struct KeywordsField {
    #[serde(default)]
    keywords: Vec<KeywordItem>,
    #[serde(default)]
    results: Vec<KeywordItem>,
}

impl KeywordsField {
    /// Nombres de las keywords, venga la respuesta de /movie o de /tv.
    pub(crate) fn nombres(&self) -> impl Iterator<Item = &str> {
        let items = if !self.keywords.is_empty() { &self.keywords } else { &self.results };
        items.iter().map(|k| k.name.as_str())
    }
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
pub async fn vera_import_catalog(
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
pub fn vera_catalog_count(app: tauri::AppHandle) -> Result<CatalogCount, String> {
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
pub fn vera_platform_list() -> Vec<VeraOption> {
    [
        ("netflix", "Netflix"), ("prime", "Prime Video"), ("disney", "Disney+"),
        ("hbo", "Max"), ("apple", "Apple TV+"), ("mubi", "Mubi"),
        ("paramount", "Paramount+"), ("star", "Star+"), ("crunchyroll", "Crunchyroll"),
        ("youtube", "YouTube"), ("free_tv", "TV abierta"),
    ].iter().map(|(id, label)| opt(id, label, None)).collect()
}
