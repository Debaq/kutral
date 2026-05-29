// Screening worker — detección automática de pelis no disponibles en el
// provider de iframe (playimdb → streamimdb). El HTML inicial es idéntico
// para reales y fakes, así que sniffeamos qué hace el JS de la página:
// - una ventana oculta off-screen carga la URL del provider,
// - un init_script override-ea fetch + XHR y reporta status code,
// - a los 3500ms hace probe del DOM buscando <video> / <source>,
// - si tras PROBE_MS no hubo señal de video → marcamos no disponible.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const REF: &str = "hm_tpks_i_2_pd_tp1_pbr_ic";
const PROBE_MS: u64 = 6000;
const WIN_LABEL: &str = "screening";

pub struct ScreeningState {
    pub queue: Mutex<Vec<String>>,
    pub started: Mutex<bool>,
    pub current_id: Mutex<Option<String>>,
    pub current_bad: Mutex<bool>,
    pub current_has_video: Mutex<bool>,
}

impl Default for ScreeningState {
    fn default() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            started: Mutex::new(false),
            current_id: Mutex::new(None),
            current_bad: Mutex::new(false),
            current_has_video: Mutex::new(false),
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
pub async fn screening_enqueue(
    app: AppHandle,
    state: tauri::State<'_, ScreeningState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let already = already_marked(&app);
    {
        let mut q = state.queue.lock().unwrap();
        for id in ids {
            if id.is_empty() || already.contains(&id) || q.contains(&id) {
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

#[derive(Deserialize)]
pub struct ScreeningReport {
    pub kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub url: String,
    #[serde(default)]
    pub status: u16,
    #[serde(rename = "contentType", default)]
    pub content_type: String,
}

#[tauri::command]
pub async fn screening_report(
    state: tauri::State<'_, ScreeningState>,
    payload: ScreeningReport,
) -> Result<(), String> {
    eprintln!(
        "[screening-report] kind={} status={} ct={}",
        payload.kind, payload.status, payload.content_type
    );
    let ct = payload.content_type.to_lowercase();
    if payload.status == 404 || payload.status == 410 {
        *state.current_bad.lock().unwrap() = true;
    }
    if ct.starts_with("video/")
        || ct.contains("mpegurl")
        || ct.contains("m3u8")
        || ct.contains("octet-stream")
    {
        *state.current_has_video.lock().unwrap() = true;
    }
    if payload.kind == "dom" && payload.status == 1 {
        *state.current_has_video.lock().unwrap() = true;
    }
    Ok(())
}

const INIT_SCRIPT: &str = r#"
(() => {
    if (window.__SCREENING_INSTALLED) return;
    window.__SCREENING_INSTALLED = true;
    const invoke = (cmd, args) => {
        try {
            const ti = window.__TAURI_INTERNALS__;
            if (ti && ti.invoke) return ti.invoke(cmd, args);
        } catch (e) {}
        return Promise.resolve();
    };
    const report = (kind, url, status, ct) => {
        invoke('screening_report', {
            payload: {
                kind: String(kind || ''),
                url: String(url || ''),
                status: (status | 0),
                contentType: String(ct || '')
            }
        });
    };
    const origFetch = window.fetch;
    if (origFetch) {
        window.fetch = async (...args) => {
            try {
                const r = await origFetch(...args);
                report('fetch', args[0], r.status, r.headers.get('content-type'));
                return r;
            } catch (e) {
                report('fetch', args[0], 0, '');
                throw e;
            }
        };
    }
    const origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url, ...rest) {
        this.addEventListener('loadend', () => {
            try {
                report('xhr', url, this.status, this.getResponseHeader('content-type') || '');
            } catch (e) {}
        });
        return origOpen.call(this, method, url, ...rest);
    };
    setTimeout(() => {
        try {
            const has = !!document.querySelector(
                'video, source, [src*=".m3u8"], [src*=".mp4"]'
            );
            report('dom', location.href, has ? 1 : 0, '');
        } catch (e) {}
    }, 3500);
})();
"#;

async fn ensure_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(WIN_LABEL) {
        return Ok(w);
    }
    let win = WebviewWindowBuilder::new(
        app,
        WIN_LABEL,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .visible(false)
    .decorations(false)
    .skip_taskbar(true)
    .inner_size(800.0, 600.0)
    .position(-9999.0, -9999.0)
    .initialization_script(INIT_SCRIPT)
    .build()
    .map_err(|e| format!("ventana screening: {}", e))?;
    Ok(win)
}

async fn worker_loop(app: AppHandle) {
    loop {
        let id = {
            let state = app.state::<ScreeningState>();
            let mut q = state.queue.lock().unwrap();
            if q.is_empty() {
                let mut s = state.started.lock().unwrap();
                *s = false;
                eprintln!("[screening] cola vacía, worker termina");
                return;
            }
            q.remove(0)
        };
        process_one(&app, &id).await;
    }
}

async fn process_one(app: &AppHandle, imdb_id: &str) {
    eprintln!("[screening] start {}", imdb_id);
    let state = app.state::<ScreeningState>();
    *state.current_id.lock().unwrap() = Some(imdb_id.to_string());
    *state.current_bad.lock().unwrap() = false;
    *state.current_has_video.lock().unwrap() = false;

    let url = format!(
        "https://www.playimdb.com/es/title/{}/?ref_={}",
        imdb_id, REF
    );

    let win = match ensure_window(app).await {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[screening] ensure_window err: {}", e);
            return;
        }
    };

    if let Err(e) = win.eval(&format!("window.location.href = {:?};", url)) {
        eprintln!("[screening] eval navigate fail: {}", e);
        return;
    }

    tokio::time::sleep(std::time::Duration::from_millis(PROBE_MS)).await;

    let bad = *state.current_bad.lock().unwrap();
    let has_video = *state.current_has_video.lock().unwrap();

    // CONSERVADOR: solo marcamos fake con evidencia clara (404/410 visto).
    // Si no llegan reportes (porque __TAURI_INTERNALS__ no se inyectó en la
    // URL externa, por ejemplo), asumimos disponible. Mejor falso positivo
    // que falso negativo: el user puede explorar y descubrir él si rompe.
    let (disponible, reason) = if bad {
        (false, "404/410 detectado".to_string())
    } else {
        (true, format!("sin evidencia de fake (video={})", has_video))
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

    *state.current_id.lock().unwrap() = None;
    eprintln!(
        "[screening] {} → disponible={} ({})",
        imdb_id, disponible, reason
    );
}
