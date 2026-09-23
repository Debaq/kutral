// OMDb: ratings de IMDb, Rotten Tomatoes y Metacritic para la ficha.

use serde::{Deserialize, Serialize};

use crate::fetch_json;

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
pub async fn omdb_detail(imdb_id: String, api_key: String) -> Result<OmdbDetail, String> {
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
