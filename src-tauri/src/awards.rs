// Premios y nominaciones via Wikidata SPARQL.
// Dado un imdb_id (tt...), consulta la película en Wikidata por P345 (IMDB ID)
// y cuenta:
//   - wins:        P166 award received
//   - nominations: P1411 nominated for
//
// Sin key. UA propio (Wikidata bloquea defaults).
// Cachea en disco (crate::cache): los premios de una peli cambian una vez al
// año como mucho, y esto se dispara por cada card del grid.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AwardsSummary {
    pub wins: u32,
    pub nominations: u32,
}

const SPARQL_URL: &str = "https://query.wikidata.org/sparql";
const UA: &str = "Kutral/26.5 (https://github.com/Debaq/kutral; contacto@kutral.app)";

#[tauri::command]
pub async fn wikidata_awards(
    app: tauri::AppHandle,
    imdb_id: String,
) -> Result<AwardsSummary, String> {
    // Validación mínima: imdb_id empieza con "tt" y tiene 7+ dígitos.
    if !imdb_id.starts_with("tt") || imdb_id.len() < 5 {
        return Ok(AwardsSummary { wins: 0, nominations: 0 });
    }
    // Bloquear caracteres raros para evitar inyección en el literal SPARQL.
    if !imdb_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(AwardsSummary { wins: 0, nominations: 0 });
    }

    if let Some((wins, nominations)) = crate::cache::awards_get(&app, &imdb_id) {
        return Ok(AwardsSummary { wins, nominations });
    }

    let query = format!(
        r#"SELECT (COUNT(DISTINCT ?win) AS ?wins) (COUNT(DISTINCT ?nom) AS ?noms) WHERE {{
  ?film wdt:P345 "{}" .
  OPTIONAL {{ ?film wdt:P166 ?win }}
  OPTIONAL {{ ?film wdt:P1411 ?nom }}
}}"#,
        imdb_id
    );

    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client: {}", e))?;

    let resp: serde_json::Value = client
        .get(SPARQL_URL)
        .header("Accept", "application/sparql-results+json")
        .query(&[("query", query.as_str()), ("format", "json")])
        .send()
        .await
        .map_err(|e| format!("wikidata http: {}", e))?
        .error_for_status()
        .map_err(|e| format!("wikidata status: {}", e))?
        .json()
        .await
        .map_err(|e| format!("wikidata json: {}", e))?;

    let binds = resp
        .pointer("/results/bindings/0")
        .ok_or_else(|| "wikidata: sin bindings".to_string())?;

    let read_u32 = |k: &str| -> u32 {
        binds
            .get(k)
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    };

    let out = AwardsSummary {
        wins: read_u32("wins"),
        nominations: read_u32("noms"),
    };
    crate::cache::awards_put(&app, &imdb_id, out.wins, out.nominations);
    Ok(out)
}
