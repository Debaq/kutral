use serde::{Deserialize, Serialize};

// ============================================================
// Subtítulos: Wyzie (sub.wyzie.io)
// ============================================================
// Endpoint observado: GET https://sub.wyzie.io/search?id={imdb}&language={lang}&key={key}
// Series: &season=&episode= (sin ellos trae subtítulos de cualquier capítulo).
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
pub async fn wyzie_search(
    imdb_id: String,
    language: String,
    api_key: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<WyzieSubtitle>, String> {
    if api_key.is_empty() {
        return Err("falta wyzie key".into());
    }
    if imdb_id.is_empty() {
        return Err("imdb_id vacío".into());
    }
    let mut url = format!(
        "https://sub.wyzie.io/search?id={}&language={}&key={}",
        urlencoding::encode(&imdb_id),
        urlencoding::encode(&language),
        urlencoding::encode(&api_key)
    );
    if let (Some(t), Some(e)) = (season, episode) {
        url.push_str(&format!("&season={t}&episode={e}"));
    }
    let cli = crate::client()?;
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
