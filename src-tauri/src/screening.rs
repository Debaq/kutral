// Screening worker — detección de títulos no disponibles.
//
// Vía: API interna del proveedor (descubierta por análisis estático del
// player.min.js, no por sniff de network).
//
//   GET https://streamdata.vaplayer.ru/api.php?imdb={imdb_id}&type={movie|tv}
//
// Respuesta:
//   - existe → `{"status_code":"200","data":{"stream_urls":[...]}}` o `data.eps` (series)
//   - NO existe → `{"status_code":404}`
//
// Sin webview, sin CORS, sin JS. Solo HTTP desde Rust con reqwest.
// Concurrencia limitada para no martillar el provider.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

fn norm_kind(k: &str) -> &'static str {
    if k == "tv" { "tv" } else { "movie" }
}

const API_URL: &str = "https://streamdata.vaplayer.ru/api.php";
const REFERER: &str = "https://brightpathsignals.com/";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const DEFAULT_MAX_INFLIGHT: usize = 1;
const MIN_INFLIGHT: usize = 1;
const MAX_INFLIGHT_CAP: usize = 4;
const REQ_TIMEOUT_S: u64 = 8;
// Pausa entre batches: evita martillar el provider y deja ancho de banda
// libre para el iframe del player que está reproduciendo en paralelo.
const BATCH_GAP_MS: u64 = 800;
// Cuando el user está viendo una peli (mode=discover), el worker espera
// para no robarle red ni CPU al stream.
const PAUSED_POLL_MS: u64 = 1000;

pub struct ScreeningState {
    // (imdb_id, kind) — kind ∈ {"movie","tv"}
    pub queue: Mutex<Vec<(String, String)>>,
    // key = "{kind}:{imdb_id}" — un mismo imdb no puede correr dos veces en paralelo
    pub inflight: Mutex<HashSet<String>>,
    pub started: Mutex<bool>,
    pub paused: Mutex<bool>,
    pub max_inflight: Mutex<usize>,
}

impl Default for ScreeningState {
    fn default() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            inflight: Mutex::new(HashSet::new()),
            started: Mutex::new(false),
            paused: Mutex::new(false),
            max_inflight: Mutex::new(DEFAULT_MAX_INFLIGHT),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct ScreeningResult {
    #[serde(rename = "imdbId")]
    pub imdb_id: String,
    pub disponible: bool,
    pub reason: String,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn open_db(app: &AppHandle) -> Result<rusqlite::Connection, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config_dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
    let path = dir.join("kutral.db");
    rusqlite::Connection::open(&path).map_err(|e| format!("open db: {}", e))
}

fn already_marked(app: &AppHandle, kind: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(conn) = open_db(app) else { return out };
    let Ok(mut stmt) =
        conn.prepare("SELECT imdb_id FROM unavailable_items WHERE kind = ?1")
    else {
        return out;
    };
    let Ok(rows) = stmt.query_map([kind], |r| r.get::<_, String>(0)) else {
        return out;
    };
    for r in rows.flatten() {
        out.insert(r);
    }
    out
}

#[tauri::command]
pub async fn screening_get_unavailable(app: AppHandle) -> Result<Vec<String>, String> {
    // Devolvemos ambos sets fusionados: en frontend basta saber si un imdb
    // está marcado (un imdb no se reutiliza entre tipos).
    let mut all = already_marked(&app, "movie");
    all.extend(already_marked(&app, "tv"));
    Ok(all.into_iter().collect())
}

#[tauri::command]
pub async fn screening_set_paused(
    state: tauri::State<'_, ScreeningState>,
    paused: bool,
) -> Result<(), String> {
    *state.paused.lock().unwrap() = paused;
    eprintln!("[screening] paused={}", paused);
    Ok(())
}

#[tauri::command]
pub async fn screening_set_concurrency(
    state: tauri::State<'_, ScreeningState>,
    n: usize,
) -> Result<(), String> {
    let clamped = n.clamp(MIN_INFLIGHT, MAX_INFLIGHT_CAP);
    *state.max_inflight.lock().unwrap() = clamped;
    eprintln!("[screening] concurrency={}", clamped);
    Ok(())
}

#[tauri::command]
pub async fn screening_enqueue(
    app: AppHandle,
    state: tauri::State<'_, ScreeningState>,
    ids: Vec<String>,
    kind: Option<String>,
) -> Result<(), String> {
    let k = norm_kind(kind.as_deref().unwrap_or("movie"));
    let already = already_marked(&app, k);
    {
        let inflight = state.inflight.lock().unwrap();
        let mut q = state.queue.lock().unwrap();
        for id in ids {
            if id.is_empty() || !id.starts_with("tt") || already.contains(&id) {
                continue;
            }
            let inflight_key = format!("{}:{}", k, id);
            if inflight.contains(&inflight_key) {
                continue;
            }
            if q.iter().any(|(qid, qk)| qid == &id && qk == k) {
                continue;
            }
            q.push((id, k.to_string()));
        }
    }
    let mut s = state.started.lock().unwrap();
    if *s {
        return Ok(());
    }
    *s = true;
    drop(s);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        worker_loop(app_clone).await;
    });
    Ok(())
}

async fn worker_loop(app: AppHandle) {
    let client = match reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(REQ_TIMEOUT_S))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[screening] reqwest build fail: {}", e);
            let state = app.state::<ScreeningState>();
            *state.started.lock().unwrap() = false;
            return;
        }
    };

    loop {
        // Si el user está viendo una peli, no robamos red/CPU. Espera y reintenta.
        let is_paused = *app.state::<ScreeningState>().paused.lock().unwrap();
        if is_paused {
            tokio::time::sleep(std::time::Duration::from_millis(PAUSED_POLL_MS)).await;
            continue;
        }

        // Tomar hasta max_inflight items de la cola (config dinámica)
        let batch: Vec<(String, String)> = {
            let state = app.state::<ScreeningState>();
            let mut q = state.queue.lock().unwrap();
            let mut inflight = state.inflight.lock().unwrap();
            if q.is_empty() {
                let mut s = state.started.lock().unwrap();
                *s = false;
                eprintln!("[screening] cola vacía, worker termina");
                return;
            }
            let cur_max = *state.max_inflight.lock().unwrap();
            let n = q.len().min(cur_max);
            let drained: Vec<(String, String)> = q.drain(..n).collect();
            for (id, k) in &drained {
                inflight.insert(format!("{}:{}", k, id));
            }
            drained
        };

        // Procesar batch en paralelo
        let mut tasks = Vec::with_capacity(batch.len());
        for (id, kind) in batch {
            let app_c = app.clone();
            let cli = client.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                process_one(&app_c, &cli, &id, &kind).await;
                let st = app_c.state::<ScreeningState>();
                st.inflight
                    .lock()
                    .unwrap()
                    .remove(&format!("{}:{}", kind, id));
            }));
        }
        for t in tasks {
            let _ = t.await;
        }

        // Throttle entre batches para no martillar al provider.
        tokio::time::sleep(std::time::Duration::from_millis(BATCH_GAP_MS)).await;
    }
}

async fn process_one(app: &AppHandle, cli: &reqwest::Client, imdb_id: &str, kind: &str) {
    let k = norm_kind(kind);
    let url = format!("{}?imdb={}&type={}", API_URL, imdb_id, k);
    let res = cli
        .get(&url)
        .header("Referer", REFERER)
        .header("Accept", "application/json, text/plain, */*")
        .send()
        .await;

    // Tri-state: Some(true)=disponible, Some(false)=NO disponible (evidencia
    // positiva: 404 explícito), None=incierto (no marcar, dejar para reintento).
    // Antes marcábamos todo lo que no fuera "200+streams" como no disponible —
    // eso incluía rate-limit, 5xx, body HTML de CF challenge, etc., y dejaba
    // pelis sanas marcadas falsamente no disponibles.
    let (status_dbg, body_len, decision) = match res {
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let sc = &v["status_code"];
            let api_ok = sc.as_str() == Some("200") || sc.as_i64() == Some(200);
            let api_404 = sc.as_str() == Some("404") || sc.as_i64() == Some(404);
            let has_streams = v["data"]["stream_urls"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let has_eps = v["data"]["eps"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false);

            let decision: Option<bool> = if api_ok && (has_streams || has_eps) {
                Some(true)
            } else if api_404 {
                Some(false)
            } else {
                // Cualquier otra cosa (HTTP error, JSON inválido, CF challenge,
                // status_code raro): incierto. NO marcar.
                None
            };
            (status.to_string(), body.len(), decision)
        }
        Err(e) => {
            eprintln!("[screening] {} red error: {}", imdb_id, e);
            return;
        }
    };

    let (disponible, reason) = match decision {
        Some(d) => (d, format!("api {} ({} bytes)", status_dbg, body_len)),
        None => {
            eprintln!(
                "[screening] {} incierto (status={} body={}b) — no marca",
                imdb_id, status_dbg, body_len
            );
            return;
        }
    };

    if let Ok(conn) = open_db(app) {
        if !disponible {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO unavailable_items (imdb_id, kind, detected_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![imdb_id, k, now_ms()],
            );
        } else {
            // Si previamente estaba marcado (bug u oscilación del provider), limpiar.
            let _ = conn.execute(
                "DELETE FROM unavailable_items WHERE imdb_id = ?1 AND kind = ?2",
                rusqlite::params![imdb_id, k],
            );
        }
    }

    let _ = app.emit(
        "screening-result",
        ScreeningResult {
            imdb_id: imdb_id.to_string(),
            disponible,
            reason: reason.clone(),
        },
    );

    eprintln!(
        "[screening] {} → disponible={} ({})",
        imdb_id, disponible, reason
    );
}
