// Screening worker — detección de pelis no disponibles.
//
// Vía: API interna del proveedor (descubierta por análisis estático del
// player.min.js, no por sniff de network).
//
//   GET https://streamdata.vaplayer.ru/api.php?imdb={imdb_id}&type=movie
//
// Respuesta:
//   - peli existe → `{"status_code":"200","data":{"stream_urls":[...]}}`
//   - peli NO existe → `{"status_code":404}`
//
// Sin webview, sin CORS, sin JS. Solo HTTP desde Rust con reqwest.
// Concurrencia limitada para no martillar el provider.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

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
    pub queue: Mutex<Vec<String>>,
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

fn already_marked(app: &AppHandle) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(conn) = open_db(app) else { return out };
    let Ok(mut stmt) = conn.prepare("SELECT imdb_id FROM unavailable_items") else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
        return out;
    };
    for r in rows.flatten() {
        out.insert(r);
    }
    out
}

#[tauri::command]
pub async fn screening_get_unavailable(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(already_marked(&app).into_iter().collect())
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
) -> Result<(), String> {
    let already = already_marked(&app);
    {
        let inflight = state.inflight.lock().unwrap();
        let mut q = state.queue.lock().unwrap();
        for id in ids {
            if id.is_empty()
                || !id.starts_with("tt")
                || already.contains(&id)
                || q.contains(&id)
                || inflight.contains(&id)
            {
                continue;
            }
            q.push(id);
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
        let batch: Vec<String> = {
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
            let drained: Vec<String> = q.drain(..n).collect();
            for id in &drained {
                inflight.insert(id.clone());
            }
            drained
        };

        // Procesar batch en paralelo
        let mut tasks = Vec::with_capacity(batch.len());
        for id in batch {
            let app_c = app.clone();
            let cli = client.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                process_one(&app_c, &cli, &id).await;
                let st = app_c.state::<ScreeningState>();
                st.inflight.lock().unwrap().remove(&id);
            }));
        }
        for t in tasks {
            let _ = t.await;
        }

        // Throttle entre batches para no martillar al provider.
        tokio::time::sleep(std::time::Duration::from_millis(BATCH_GAP_MS)).await;
    }
}

async fn process_one(app: &AppHandle, cli: &reqwest::Client, imdb_id: &str) {
    let url = format!("{}?imdb={}&type=movie", API_URL, imdb_id);
    let res = cli
        .get(&url)
        .header("Referer", REFERER)
        .header("Accept", "application/json, text/plain, */*")
        .send()
        .await;

    let (disponible, reason) = match res {
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            // Parseamos lazy: status_code puede venir como "200" (string) o 200 (number).
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let sc = &v["status_code"];
            let ok = sc.as_str() == Some("200") || sc.as_i64() == Some(200);
            let has_streams = v["data"]["stream_urls"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            // TV series: data.eps puede traer la cosa, también vale.
            let has_eps = v["data"]["eps"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if ok && (has_streams || has_eps) {
                (true, format!("api 200 ({} bytes)", body.len()))
            } else {
                (false, format!("api {} (sin streams)", status))
            }
        }
        Err(e) => {
            // Error de red: NO marcamos. No queremos falsos positivos por wifi malo.
            eprintln!("[screening] {} red error: {}", imdb_id, e);
            return;
        }
    };

    if !disponible {
        if let Ok(conn) = open_db(app) {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO unavailable_items (imdb_id, detected_at) VALUES (?1, ?2)",
                rusqlite::params![imdb_id, now_ms()],
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
