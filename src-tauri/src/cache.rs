//! Caché en disco del catálogo: evita repetir peticiones caras al paginar.
//!
//! Dos almacenes, elegidos por la forma del dato:
//!
//!   - SQLite (`kutral.db`, el mismo archivo que usa el resto de la app):
//!     muchas filas chiquitas consultadas por clave — el status por título y
//!     los premios por imdb. Leer una fila no obliga a cargar todo.
//!   - JSON en `app_cache_dir/listas/`: páginas enteras de discover, que son
//!     blobs grandes y se leen de una sola vez.
//!
//! Las tablas se crean acá con IF NOT EXISTS y NO en la lista de migraciones
//! de tauri-plugin-sql: esas corren recién cuando el front hace Database.load,
//! y estos comandos pueden pegarle a la DB antes de que eso pase.
//!
//! Regla: un fallo de caché NUNCA rompe el comando. Todo devuelve Option y
//! los errores se loguean y se ignoran — peor caso, se pega a la red igual.

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Status por título (imdb/trailer/temporadas): TMDb casi no lo cambia.
pub const TTL_STATUS_MS: i64 = 30 * 24 * 3600 * 1000;
/// Premios de Wikidata: NO vencen. Un premio ya entregado no se des-entrega y
/// una nominación tampoco deja de haber ocurrido. Lo único que se revalida es
/// la fila que dio 0 y 0: puede ser un título del año que todavía no pasó por
/// la temporada de premios, o que Wikidata aún no tenga cargado.
pub const TTL_AWARDS_VACIO_MS: i64 = 7 * 24 * 3600 * 1000;
/// Páginas de catálogo: el orden por popularidad se mueve a diario.
pub const TTL_LISTA_MS: i64 = 12 * 3600 * 1000;

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
        .map_err(|e| format!("config_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let conn = rusqlite::Connection::open(dir.join("kutral.db"))
        .map_err(|e| format!("open db: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cache_item_status (
            media_type   TEXT    NOT NULL,
            id           INTEGER NOT NULL,
            has_imdb     INTEGER NOT NULL,
            imdb_id      TEXT,
            has_trailer  INTEGER NOT NULL,
            seasons      INTEGER,
            ts           INTEGER NOT NULL,
            PRIMARY KEY (media_type, id)
         );
         CREATE TABLE IF NOT EXISTS cache_awards (
            imdb_id      TEXT PRIMARY KEY,
            wins         INTEGER NOT NULL,
            nominations  INTEGER NOT NULL,
            ts           INTEGER NOT NULL
         );",
    )
    .map_err(|e| format!("create tables: {e}"))?;
    Ok(conn)
}

// --- status por título -----------------------------------------------------

pub struct StatusRow {
    pub has_imdb: bool,
    pub imdb_id: Option<String>,
    pub has_trailer: bool,
    pub seasons: Option<u32>,
}

pub fn status_get(app: &AppHandle, media_type: &str, id: u64) -> Option<StatusRow> {
    let conn = open_db(app).ok()?;
    let corte = now_ms() - TTL_STATUS_MS;
    conn.query_row(
        "SELECT has_imdb, imdb_id, has_trailer, seasons
           FROM cache_item_status
          WHERE media_type = ?1 AND id = ?2 AND ts >= ?3",
        rusqlite::params![media_type, id as i64, corte],
        |r| {
            Ok(StatusRow {
                has_imdb: r.get::<_, i64>(0)? != 0,
                imdb_id: r.get::<_, Option<String>>(1)?,
                has_trailer: r.get::<_, i64>(2)? != 0,
                seasons: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
            })
        },
    )
    .ok()
}

pub fn status_put(app: &AppHandle, media_type: &str, id: u64, row: &StatusRow) {
    let Ok(conn) = open_db(app) else { return };
    let r = conn.execute(
        "INSERT OR REPLACE INTO cache_item_status
            (media_type, id, has_imdb, imdb_id, has_trailer, seasons, ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            media_type,
            id as i64,
            row.has_imdb as i64,
            row.imdb_id,
            row.has_trailer as i64,
            row.seasons.map(|v| v as i64),
            now_ms(),
        ],
    );
    if let Err(e) = r {
        eprintln!("[cache] status_put: {e}");
    }
}

// --- premios (Wikidata) ----------------------------------------------------

pub fn awards_get(app: &AppHandle, imdb_id: &str) -> Option<(u32, u32)> {
    let conn = open_db(app).ok()?;
    let corte = now_ms() - TTL_AWARDS_VACIO_MS;
    conn.query_row(
        "SELECT wins, nominations
           FROM cache_awards
          WHERE imdb_id = ?1
            AND (wins > 0 OR nominations > 0 OR ts >= ?2)",
        rusqlite::params![imdb_id, corte],
        |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u32)),
    )
    .ok()
}

pub fn awards_put(app: &AppHandle, imdb_id: &str, wins: u32, nominations: u32) {
    let Ok(conn) = open_db(app) else { return };
    let r = conn.execute(
        "INSERT OR REPLACE INTO cache_awards (imdb_id, wins, nominations, ts)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![imdb_id, wins as i64, nominations as i64, now_ms()],
    );
    if let Err(e) = r {
        eprintln!("[cache] awards_put: {e}");
    }
}

// --- páginas de catálogo ---------------------------------------------------

/// Clave estable a partir de las partes que identifican la consulta.
/// Va hasheada: las partes pueden traer la api key y no queremos verla en un
/// nombre de archivo.
pub fn clave(partes: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in partes {
        h.update(p.as_bytes());
        h.update([0u8]); // separador: evita que ("ab","c") y ("a","bc") colisionen
    }
    hex::encode(h.finalize())
}

fn dir_listas(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_cache_dir().ok()?.join("listas");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// JSON crudo de la página cacheada, si existe y no venció.
pub fn lista_get(app: &AppHandle, clave: &str) -> Option<String> {
    let f = dir_listas(app)?.join(format!("{clave}.json"));
    let txt = std::fs::read_to_string(&f).ok()?;
    let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let ts = v.get("ts")?.as_i64()?;
    if now_ms() - ts > TTL_LISTA_MS {
        return None;
    }
    Some(v.get("data")?.to_string())
}

pub fn lista_put(app: &AppHandle, clave: &str, json: &str) {
    let Some(dir) = dir_listas(app) else { return };
    let data: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[cache] lista_put json inválido: {e}");
            return;
        }
    };
    let envuelto = serde_json::json!({ "ts": now_ms(), "data": data });
    if let Err(e) = std::fs::write(dir.join(format!("{clave}.json")), envuelto.to_string()) {
        eprintln!("[cache] lista_put: {e}");
    }
}

/// Borra páginas vencidas. Se llama una vez al arrancar: el directorio crece
/// con cada combinación de filtros y nadie más lo limpia.
pub fn purgar(app: &AppHandle) {
    let Some(dir) = dir_listas(app) else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let corte = now_ms() - TTL_LISTA_MS;
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let vencida = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("ts").and_then(|t| t.as_i64()))
            .map(|ts| ts < corte)
            // Archivo ilegible o sin ts: no sirve para nada, fuera.
            .unwrap_or(true);
        if vencida {
            let _ = std::fs::remove_file(&path);
        }
    }
}
