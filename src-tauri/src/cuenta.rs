// Cuenta de Kutral en un servidor propio (ver server/ en la raíz del repo).
//
// Opcional: sin cuenta todo sigue funcionando igual, solo en este equipo.
// Con cuenta, lo sincronizable (favoritos, historial con el minuto donde
// quedaste, listas y sus títulos) se sube y se baja de un servidor PHP cuya
// URL pone el usuario en Configuración.
//
// Cómo se sabe qué subir: triggers de SQLite (migraciones v10 y v12) anotan
// en `sync_salida` cada fila que cambia, sin importar quién la escribió
// (el front vía plugin-sql). Al sincronizar se sube lo anotado y se baja lo
// que otros equipos cambiaron desde el último cursor. Si dos equipos tocan lo
// mismo gana el cambio más reciente (`actualizado`, ms).
//
// Para que aplicar lo bajado no se vuelva a anotar como cambio local, se
// escribe dentro de una transacción con `sync_estado.aplicando = 1`; otras
// conexiones nunca ven ese 1 porque se vuelve a 0 antes del COMMIT.
//
// El token del equipo vive en `cuenta.json` (0600), como rd_creds.json: el
// webview nunca lo recibe. Ahí también la llave de la bóveda (boveda.rs):
// claves de API, ajustes y vinculaciones, cifrados con la contraseña.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

#[derive(Serialize, Deserialize, Default, Clone)]
struct Cuenta {
    #[serde(default)]
    servidor: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    usuario: String,
    /// Qué perfil del servidor corresponde a cada perfil de este equipo, y
    /// hasta dónde se bajó cada uno.
    #[serde(default)]
    perfiles: Vec<PerfilSync>,
    /// ms de la última sincronización completa. 0 = nunca.
    #[serde(default)]
    ultima_sync: i64,
    /// Llave de la bóveda (hex), sacada de la contraseña al entrar. Vacía en
    /// una sesión anterior a la bóveda: hasta confirmar la contraseña no se
    /// sincroniza.
    #[serde(default)]
    llave: String,
    // Antes de los perfiles (cuenta.json viejo): un solo perfil y su cursor.
    // `cargar` los pasa a `perfiles`.
    #[serde(default, skip_serializing)]
    perfil_id: i64,
    #[serde(default, skip_serializing)]
    cursor: i64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
struct PerfilSync {
    /// Id del perfil aquí (`perfiles.id`); en el servidor es su `uid`.
    local: String,
    /// Id numérico del perfil en el servidor.
    remoto: i64,
    /// Último `seq` del servidor ya aplicado a este perfil.
    cursor: i64,
}

// Todo recibe la carpeta de config y no el AppHandle: así los tests pueden
// simular dos equipos con dos carpetas.
fn dir_config(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    Ok(dir)
}

fn cargar(dir: &Path) -> Cuenta {
    let mut c: Cuenta = std::fs::read_to_string(dir.join("cuenta.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if c.perfiles.is_empty() && c.perfil_id > 0 {
        c.perfiles.push(PerfilSync {
            local: crate::perfiles::PRINCIPAL.into(),
            remoto: c.perfil_id,
            cursor: c.cursor,
        });
    }
    c
}

fn guardar(dir: &Path, c: &Cuenta) -> Result<(), String> {
    let path = dir.join("cuenta.json");
    let json = serde_json::to_string(c).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn abrir_db(dir: &Path) -> Result<Connection, String> {
    let path = dir.join("kutral.db");
    let db = Connection::open(&path).map_err(|e| format!("open db: {e}"))?;
    // El front tiene su propia conexión al mismo archivo: esperar en vez de
    // fallar si justo está escribiendo.
    db.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
    Ok(db)
}

fn ahora_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn client() -> Result<reqwest::Client, String> {
    static HTTP: std::sync::OnceLock<Result<reqwest::Client, String>> = std::sync::OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("Kutral/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(6))
            .build()
            .map_err(|e| e.to_string())
    })
    .clone()
}

/// Normaliza la URL del servidor y exige https, salvo en la red local (un
/// servidor casero en 192.168.x.x sin certificado es razonable; en internet,
/// el token viajaría en claro).
fn normalizar_servidor(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(s).map_err(|_| "URL inválida".to_string())?;
    match url.scheme() {
        "https" => {}
        "http" => {
            let local = match url.host() {
                Some(url::Host::Domain(d)) => d == "localhost" || d.ends_with(".local"),
                Some(url::Host::Ipv4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
                Some(url::Host::Ipv6(ip)) => {
                    ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00
                }
                None => false,
            };
            if !local {
                return Err("usa https:// (http solo se permite en la red local)".into());
            }
        }
        _ => return Err("la URL debe empezar con https://".into()),
    }
    Ok(s.to_string())
}

fn nombre_equipo() -> String {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "equipo".into());
    let so = if cfg!(windows) { "Windows" } else { "Linux" };
    let mut n = format!("{host} ({so})");
    n.truncate(64);
    n
}

/// Error del servidor: el mensaje que mandó, o el estado HTTP si no mandó nada.
#[derive(Debug)]
enum ErrApi {
    SinSesion,
    Otro(String),
}

impl From<String> for ErrApi {
    fn from(s: String) -> Self {
        ErrApi::Otro(s)
    }
}

impl ErrApi {
    fn texto(self) -> String {
        match self {
            ErrApi::SinSesion => "la sesión se cerró desde otro equipo; vuelve a entrar".into(),
            ErrApi::Otro(s) => s,
        }
    }
}

async fn api(
    servidor: &str,
    token: Option<&str>,
    metodo: reqwest::Method,
    ruta: &str,
    cuerpo: Option<&Value>,
) -> Result<Value, ErrApi> {
    let mut req = client()?.request(metodo, format!("{servidor}{ruta}"));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(b) = cuerpo {
        req = req.json(b);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ErrApi::Otro(format!("no se pudo conectar con el servidor: {e}")))?;
    let estado = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if estado.is_success() {
        return Ok(v);
    }
    if estado == reqwest::StatusCode::UNAUTHORIZED && token.is_some() {
        return Err(ErrApi::SinSesion);
    }
    Err(ErrApi::Otro(
        v.get("error")
            .and_then(|e| e.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("el servidor respondió {estado}")),
    ))
}

// ─── Lado SQLite ────────────────────────────────────────────────────────────

/// Migración v10 de kutral.db (la registra lib.rs). `sync_salida` anota qué
/// cambió aquí y falta subir; la llenan triggers para que ningún punto del
/// front que escriba favoritos se olvide de avisar. Solo anotan con la cuenta
/// activa y nunca mientras se aplica lo bajado del servidor.
pub const MIGRACION_SQL: &str = "CREATE TABLE IF NOT EXISTS sync_estado (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    activa INTEGER NOT NULL DEFAULT 0,
                    aplicando INTEGER NOT NULL DEFAULT 0
                );
                INSERT OR IGNORE INTO sync_estado (id) VALUES (1);

                CREATE TABLE IF NOT EXISTS sync_salida (
                    coleccion TEXT NOT NULL,
                    clave TEXT NOT NULL,
                    -- ms del último cambio local: es el `actualizado` que se sube.
                    cambiado INTEGER NOT NULL,
                    PRIMARY KEY (coleccion, clave)
                );

                CREATE TRIGGER IF NOT EXISTS sync_fav_ins AFTER INSERT ON favorites
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('favoritos', NEW.imdb_id,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_fav_upd AFTER UPDATE ON favorites
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('favoritos', NEW.imdb_id,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_fav_del AFTER DELETE ON favorites
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('favoritos', OLD.imdb_id,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;
";

/// Migración v12 (después de la v11, que crea las listas): lo mismo que la v10
/// para el historial y las listas. Si la cuenta ya estaba activa, anota todo
/// lo que había para que suba en la próxima sincronización.
pub const MIGRACION_SQL_V12: &str = "
                CREATE TRIGGER IF NOT EXISTS sync_hist_ins AFTER INSERT ON watch_history
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('historial', NEW.imdb_id || '|' || NEW.season || '|' || NEW.episode,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_hist_upd AFTER UPDATE ON watch_history
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('historial', NEW.imdb_id || '|' || NEW.season || '|' || NEW.episode,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_hist_del AFTER DELETE ON watch_history
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('historial', OLD.imdb_id || '|' || OLD.season || '|' || OLD.episode,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_lista_ins AFTER INSERT ON listas
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('listas', NEW.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_lista_upd AFTER UPDATE ON listas
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('listas', NEW.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_lista_del AFTER DELETE ON listas
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('listas', OLD.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_item_ins AFTER INSERT ON lista_items
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('lista_items', NEW.lista_id || '|' || NEW.clave,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_item_upd AFTER UPDATE ON lista_items
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('lista_items', NEW.lista_id || '|' || NEW.clave,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                CREATE TRIGGER IF NOT EXISTS sync_item_del AFTER DELETE ON lista_items
                WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
                BEGIN
                    INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                    VALUES ('lista_items', OLD.lista_id || '|' || OLD.clave,
                            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
                END;

                INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                  SELECT 'historial', imdb_id || '|' || season || '|' || episode, last_watched
                    FROM watch_history WHERE (SELECT activa FROM sync_estado WHERE id = 1);
                INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                  SELECT 'listas', id, actualizada
                    FROM listas WHERE (SELECT activa FROM sync_estado WHERE id = 1);
                INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
                  SELECT 'lista_items', lista_id || '|' || clave, agregado
                    FROM lista_items WHERE (SELECT activa FROM sync_estado WHERE id = 1);
";

/// Anota para subir todo lo sincronizable que no son favoritos. Con su propia
/// fecha (no "ahora"): si otro equipo tiene algo más nuevo, gana ese.
const ANOTAR_RESTO: &str = "
         INSERT OR REPLACE INTO sync_salida (perfil, coleccion, clave, cambiado)
           SELECT perfil, 'historial', imdb_id || '|' || season || '|' || episode, last_watched FROM watch_history;
         INSERT OR REPLACE INTO sync_salida (perfil, coleccion, clave, cambiado)
           SELECT perfil, 'listas', id, actualizada FROM listas;
         INSERT OR REPLACE INTO sync_salida (perfil, coleccion, clave, cambiado)
           SELECT perfil, 'lista_items', lista_id || '|' || clave, agregado FROM lista_items;";

const FAVORITOS: &str = "favoritos";
const HISTORIAL: &str = "historial";
const LISTAS: &str = "listas";
const LISTA_ITEMS: &str = "lista_items";

#[derive(Serialize, Deserialize)]
struct Cambio {
    coleccion: String,
    clave: String,
    #[serde(default)]
    datos: Option<Value>,
    #[serde(default)]
    borrado: bool,
    actualizado: i64,
}

fn set_activa(db: &Connection, activa: bool) -> Result<(), String> {
    db.execute("UPDATE sync_estado SET activa = ?1 WHERE id = 1", params![activa as i64])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Al entrar: lo que ya había en este equipo se anota para subir. Se une con
/// lo de la cuenta en vez de pisarlo (si la cuenta tiene un borrado más nuevo,
/// ese gana al sincronizar).
fn anotar_todo(db: &Connection) -> Result<(), String> {
    db.execute_batch(&format!(
        "DELETE FROM sync_salida;
         INSERT OR REPLACE INTO sync_salida (perfil, coleccion, clave, cambiado)
           SELECT perfil, 'favoritos', imdb_id, added_at FROM favorites;
         {ANOTAR_RESTO}"
    ))
    .map_err(|e| e.to_string())
}

/// `imdb|season|episode` → partes. La clave de anime (`anilist:5`) no lleva `|`.
fn partir_historial(clave: &str) -> Option<(&str, i64, i64)> {
    let mut it = clave.rsplitn(3, '|');
    let episode = it.next()?.parse().ok()?;
    let season = it.next()?.parse().ok()?;
    Some((it.next()?, season, episode))
}

/// `lista_id|clave` → partes. El id de la lista (uuid) no lleva `|`.
fn partir_item(clave: &str) -> Option<(&str, &str)> {
    clave.split_once('|')
}

fn datos_historial(db: &Connection, perfil: &str, clave: &str) -> Result<Option<Value>, String> {
    let Some((imdb, season, episode)) = partir_historial(clave) else { return Ok(None) };
    db.query_row(
        "SELECT tmdb_id, media_type, title, poster_path, still_path, episode_title,
                watched_seconds, runtime_seconds, progress_real, completed, last_watched
           FROM watch_history WHERE perfil = ?4 AND imdb_id = ?1 AND season = ?2 AND episode = ?3",
        params![imdb, season, episode, perfil],
        |r| {
            Ok(json!({
                "tmdb_id": r.get::<_, i64>(0)?,
                "media_type": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "poster_path": r.get::<_, Option<String>>(3)?,
                "still_path": r.get::<_, Option<String>>(4)?,
                "episode_title": r.get::<_, Option<String>>(5)?,
                "watched_seconds": r.get::<_, i64>(6)?,
                "runtime_seconds": r.get::<_, Option<i64>>(7)?,
                "progress_real": r.get::<_, Option<f64>>(8)?,
                "completed": r.get::<_, i64>(9)?,
                "last_watched": r.get::<_, i64>(10)?,
            }))
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn datos_lista(db: &Connection, perfil: &str, id: &str) -> Result<Option<Value>, String> {
    db.query_row(
        "SELECT nombre, creada, actualizada FROM listas WHERE perfil = ?2 AND id = ?1",
        params![id, perfil],
        |r| {
            Ok(json!({
                "nombre": r.get::<_, String>(0)?,
                "creada": r.get::<_, i64>(1)?,
                "actualizada": r.get::<_, i64>(2)?,
            }))
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn datos_item(db: &Connection, perfil: &str, clave: &str) -> Result<Option<Value>, String> {
    let Some((lista, item)) = partir_item(clave) else { return Ok(None) };
    db.query_row(
        "SELECT tmdb_id, media_type, title, poster_path, posicion, agregado
           FROM lista_items WHERE perfil = ?3 AND lista_id = ?1 AND clave = ?2",
        params![lista, item, perfil],
        |r| {
            Ok(json!({
                "tmdb_id": r.get::<_, i64>(0)?,
                "media_type": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "poster_path": r.get::<_, Option<String>>(3)?,
                "posicion": r.get::<_, f64>(4)?,
                "agregado": r.get::<_, i64>(5)?,
            }))
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Lo anotado para subir de un perfil, con el estado actual de cada fila.
fn pendientes(db: &Connection, perfil: &str) -> Result<Vec<Cambio>, String> {
    let mut st = db
        .prepare("SELECT coleccion, clave, cambiado FROM sync_salida WHERE perfil = ?1 ORDER BY cambiado")
        .map_err(|e| e.to_string())?;
    let filas: Vec<(String, String, i64)> = st
        .query_map(params![perfil], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut fav = db
        .prepare(
            "SELECT tmdb_id, media_type, title, poster_path, added_at
               FROM favorites WHERE perfil = ?2 AND imdb_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(filas.len());
    for (coleccion, clave, cambiado) in filas {
        let datos = match coleccion.as_str() {
            FAVORITOS => fav
                .query_row(params![clave, perfil], |r| {
                    Ok(json!({
                        "tmdb_id": r.get::<_, i64>(0)?,
                        "media_type": r.get::<_, String>(1)?,
                        "title": r.get::<_, String>(2)?,
                        "poster_path": r.get::<_, Option<String>>(3)?,
                        "added_at": r.get::<_, i64>(4)?,
                    }))
                })
                .optional()
                .map_err(|e| e.to_string())?,
            HISTORIAL => datos_historial(db, perfil, &clave)?,
            LISTAS => datos_lista(db, perfil, &clave)?,
            LISTA_ITEMS => datos_item(db, perfil, &clave)?,
            // Colección que esta versión no conoce: no se sube.
            _ => continue,
        };
        out.push(Cambio {
            coleccion,
            clave,
            borrado: datos.is_none(),
            datos,
            actualizado: cambiado,
        });
    }
    Ok(out)
}

/// Quita de la salida lo ya subido, salvo que haya vuelto a cambiar mientras
/// tanto (su `cambiado` sería mayor que el que se subió).
fn marcar_subidos(db: &Connection, perfil: &str, subidos: &[Cambio]) -> Result<(), String> {
    let mut st = db
        .prepare(
            "DELETE FROM sync_salida WHERE perfil = ?4 AND coleccion = ?1 AND clave = ?2 AND cambiado <= ?3",
        )
        .map_err(|e| e.to_string())?;
    for c in subidos {
        st.execute(params![c.coleccion, c.clave, c.actualizado, perfil])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Aplica cambios del servidor a un perfil. Devuelve cuántos tocaron algo local.
fn aplicar(db: &mut Connection, perfil: &str, cambios: &[Cambio]) -> Result<usize, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE sync_estado SET aplicando = 1 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for c in cambios {
        // Un cambio local pendiente y más nuevo gana: se sube en la próxima.
        let local: Option<i64> = tx
            .query_row(
                "SELECT cambiado FROM sync_salida WHERE perfil = ?3 AND coleccion = ?1 AND clave = ?2",
                params![c.coleccion, c.clave, perfil],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if local.is_some_and(|l| l > c.actualizado) {
            continue;
        }
        // Antes de aplicar: aplicar_historial puede volver a anotarlo.
        if local.is_some() {
            tx.execute(
                "DELETE FROM sync_salida WHERE perfil = ?3 AND coleccion = ?1 AND clave = ?2",
                params![c.coleccion, c.clave, perfil],
            )
            .map_err(|e| e.to_string())?;
        }
        let tocadas = match c.coleccion.as_str() {
            FAVORITOS => aplicar_favorito(&tx, perfil, c)?,
            HISTORIAL => aplicar_historial(&tx, perfil, c)?,
            LISTAS => aplicar_lista(&tx, perfil, c)?,
            LISTA_ITEMS => aplicar_item(&tx, perfil, c)?,
            _ => 0,
        };
        n += tocadas;
    }
    tx.execute("UPDATE sync_estado SET aplicando = 0 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n)
}

fn aplicar_favorito(tx: &rusqlite::Transaction, perfil: &str, c: &Cambio) -> Result<usize, String> {
    if c.borrado {
        return tx
            .execute("DELETE FROM favorites WHERE perfil = ?2 AND imdb_id = ?1", params![c.clave, perfil])
            .map_err(|e| e.to_string());
    }
    let Some(d) = &c.datos else { return Ok(0) };
    let s = |k: &str| d.get(k).and_then(|v| v.as_str()).map(String::from);
    let i = |k: &str| d.get(k).and_then(|v| v.as_i64());
    let (Some(tmdb_id), Some(media_type), Some(title)) = (i("tmdb_id"), s("media_type"), s("title"))
    else {
        return Ok(0);
    };
    let added_at = i("added_at").unwrap_or(c.actualizado);
    // Si ya estaba igual no cuenta como cambio (evita recargar el front por
    // el eco de lo que este mismo equipo subió).
    let igual: bool = tx
        .query_row(
            "SELECT 1 FROM favorites WHERE perfil = ?7 AND imdb_id = ?1 AND tmdb_id = ?2 AND media_type = ?3
               AND title = ?4 AND poster_path IS ?5 AND added_at = ?6",
            params![c.clave, tmdb_id, media_type, title, s("poster_path"), added_at, perfil],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if igual {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO favorites (perfil, imdb_id, tmdb_id, media_type, title, poster_path, added_at)
         VALUES (?7, ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(perfil, imdb_id) DO UPDATE SET
           tmdb_id = excluded.tmdb_id, media_type = excluded.media_type,
           title = excluded.title, poster_path = excluded.poster_path,
           added_at = excluded.added_at",
        params![c.clave, tmdb_id, media_type, title, s("poster_path"), added_at, perfil],
    )
    .map_err(|e| e.to_string())
}

fn aplicar_historial(tx: &rusqlite::Transaction, perfil: &str, c: &Cambio) -> Result<usize, String> {
    let Some((imdb, season, episode)) = partir_historial(&c.clave) else { return Ok(0) };
    if c.borrado {
        return tx
            .execute(
                "DELETE FROM watch_history WHERE perfil = ?4 AND imdb_id = ?1 AND season = ?2 AND episode = ?3",
                params![imdb, season, episode, perfil],
            )
            .map_err(|e| e.to_string());
    }
    let Some(d) = &c.datos else { return Ok(0) };
    let s = |k: &str| d.get(k).and_then(|v| v.as_str()).map(String::from);
    let i = |k: &str| d.get(k).and_then(|v| v.as_i64());
    let (Some(tmdb_id), Some(media_type), Some(title)) = (i("tmdb_id"), s("media_type"), s("title"))
    else {
        return Ok(0);
    };
    let watched = i("watched_seconds").unwrap_or(0);
    let runtime = i("runtime_seconds");
    let real = d.get("progress_real").and_then(|v| v.as_f64());
    let completed = i("completed").unwrap_or(0);
    let last_watched = i("last_watched").unwrap_or(c.actualizado);
    let previo: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT completed, watched_seconds, last_watched FROM watch_history
              WHERE perfil = ?4 AND imdb_id = ?1 AND season = ?2 AND episode = ?3",
            params![imdb, season, episode, perfil],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    // "Terminado" no retrocede (la misma regla del front): si aquí ya estaba
    // terminado y lo bajado no, se conserva y se vuelve a subir para que el
    // otro equipo también lo tenga.
    let local_completo = previo.is_some_and(|p| p.0 == 1);
    let completed_final = completed.max(previo.map_or(0, |p| p.0));
    if previo == Some((completed_final, watched, last_watched)) {
        return Ok(0); // el eco de lo propio
    }
    tx.execute(
        "INSERT INTO watch_history
           (imdb_id, season, episode, tmdb_id, media_type, title, poster_path, still_path,
            episode_title, watched_seconds, runtime_seconds, progress_real, completed, last_watched, perfil)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
         ON CONFLICT(perfil, imdb_id, season, episode) DO UPDATE SET
           tmdb_id = excluded.tmdb_id, media_type = excluded.media_type, title = excluded.title,
           poster_path = COALESCE(excluded.poster_path, watch_history.poster_path),
           still_path = COALESCE(excluded.still_path, watch_history.still_path),
           episode_title = COALESCE(excluded.episode_title, watch_history.episode_title),
           watched_seconds = excluded.watched_seconds,
           runtime_seconds = COALESCE(excluded.runtime_seconds, watch_history.runtime_seconds),
           progress_real = excluded.progress_real,
           completed = excluded.completed,
           last_watched = excluded.last_watched",
        params![
            imdb,
            season,
            episode,
            tmdb_id,
            media_type,
            title,
            s("poster_path"),
            s("still_path"),
            s("episode_title"),
            watched,
            runtime,
            real,
            completed_final,
            last_watched,
            perfil
        ],
    )
    .map_err(|e| e.to_string())?;
    if local_completo && completed == 0 {
        tx.execute(
            "INSERT OR REPLACE INTO sync_salida (perfil, coleccion, clave, cambiado) VALUES (?4, ?1, ?2, ?3)",
            params![HISTORIAL, c.clave, ahora_ms(), perfil],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(1)
}

fn aplicar_lista(tx: &rusqlite::Transaction, perfil: &str, c: &Cambio) -> Result<usize, String> {
    if c.borrado {
        // "Quiero ver" existe en todos los equipos y no se borra.
        if c.clave == "quiero-ver" {
            return Ok(0);
        }
        return tx
            .execute("DELETE FROM listas WHERE perfil = ?2 AND id = ?1", params![c.clave, perfil])
            .map_err(|e| e.to_string());
    }
    let Some(d) = &c.datos else { return Ok(0) };
    let Some(nombre) = d.get("nombre").and_then(|v| v.as_str()) else { return Ok(0) };
    let creada = d.get("creada").and_then(|v| v.as_i64()).unwrap_or(c.actualizado);
    let actualizada = d.get("actualizada").and_then(|v| v.as_i64()).unwrap_or(c.actualizado);
    let igual: bool = tx
        .query_row(
            "SELECT 1 FROM listas WHERE perfil = ?4 AND id = ?1 AND nombre = ?2 AND actualizada = ?3",
            params![c.clave, nombre, actualizada, perfil],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if igual {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO listas (perfil, id, nombre, creada, actualizada) VALUES (?5, ?1, ?2, ?3, ?4)
         ON CONFLICT(perfil, id) DO UPDATE SET nombre = excluded.nombre, actualizada = excluded.actualizada",
        params![c.clave, nombre, creada, actualizada, perfil],
    )
    .map_err(|e| e.to_string())
}

fn aplicar_item(tx: &rusqlite::Transaction, perfil: &str, c: &Cambio) -> Result<usize, String> {
    let Some((lista, item)) = partir_item(&c.clave) else { return Ok(0) };
    if c.borrado {
        return tx
            .execute(
                "DELETE FROM lista_items WHERE perfil = ?3 AND lista_id = ?1 AND clave = ?2",
                params![lista, item, perfil],
            )
            .map_err(|e| e.to_string());
    }
    let Some(d) = &c.datos else { return Ok(0) };
    let s = |k: &str| d.get(k).and_then(|v| v.as_str()).map(String::from);
    let i = |k: &str| d.get(k).and_then(|v| v.as_i64());
    let (Some(tmdb_id), Some(media_type), Some(title)) = (i("tmdb_id"), s("media_type"), s("title"))
    else {
        return Ok(0);
    };
    let posicion = d.get("posicion").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let agregado = i("agregado").unwrap_or(c.actualizado);
    let igual: bool = tx
        .query_row(
            "SELECT 1 FROM lista_items WHERE perfil = ?6 AND lista_id = ?1 AND clave = ?2 AND posicion = ?3
               AND title = ?4 AND poster_path IS ?5",
            params![lista, item, posicion, title, s("poster_path"), perfil],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if igual {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO lista_items (lista_id, clave, tmdb_id, media_type, title, poster_path, posicion, agregado, perfil)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(perfil, lista_id, clave) DO UPDATE SET
           tmdb_id = excluded.tmdb_id, media_type = excluded.media_type, title = excluded.title,
           poster_path = excluded.poster_path, posicion = excluded.posicion",
        params![lista, item, tmdb_id, media_type, title, s("poster_path"), posicion, agregado, perfil],
    )
    .map_err(|e| e.to_string())
}

// ─── Sincronizar ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ResultadoSync {
    pub subidos: usize,
    /// Cambios bajados que modificaron algo local: si > 0 el front recarga.
    pub bajados: usize,
}

const LOTE: usize = 500;

#[derive(Deserialize, Serialize, Debug)]
struct PerfilRemoto {
    id: i64,
    uid: String,
    nombre: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    actualizado: i64,
}

/// Los perfiles mismos: los creados aquí suben, los creados en otro equipo
/// bajan, un cambio de nombre o color gana si es más nuevo y uno borrado en
/// otro equipo se borra aquí. Devuelve cuántos cambios locales hubo.
///
/// Borrar un perfil sincronizado exige red (ver perfil_borrar): por eso un
/// perfil que está en el servidor y no aquí es siempre uno nuevo de otro equipo.
async fn sincronizar_perfiles(c: &mut Cuenta, db: &mut Connection) -> Result<usize, ErrApi> {
    let v = api(&c.servidor, Some(&c.token), reqwest::Method::GET, "/perfiles", None).await?;
    let remotos: Vec<PerfilRemoto> = serde_json::from_value(v["perfiles"].clone())
        .map_err(|e| ErrApi::Otro(format!("respuesta inválida: {e}")))?;
    let locales = crate::perfiles::listar(db)?;
    let mut cambios = 0;
    let mut mapa = Vec::new();
    for l in &locales {
        let previo = c.perfiles.iter().find(|p| p.local == l.id).cloned();
        let subir = |l: &crate::perfiles::Perfil| {
            json!({ "uid": l.id, "nombre": l.nombre, "color": l.color, "actualizado": l.actualizado })
        };
        match remotos.iter().find(|r| r.uid == l.id) {
            Some(r) => {
                if l.actualizado > r.actualizado {
                    api(&c.servidor, Some(&c.token), reqwest::Method::POST, "/perfiles", Some(&subir(l))).await?;
                } else if r.actualizado > l.actualizado {
                    crate::perfiles::actualizar(db, &r.uid, &r.nombre, &r.color, r.actualizado)?;
                    // Solo la fecha (dos equipos con el mismo "Principal"): no
                    // es un cambio que el front tenga que recargar.
                    if r.nombre != l.nombre || r.color != l.color {
                        cambios += 1;
                    }
                }
                let cursor = previo.filter(|p| p.remoto == r.id).map_or(0, |p| p.cursor);
                mapa.push(PerfilSync { local: l.id.clone(), remoto: r.id, cursor });
            }
            // Ya estuvo en el servidor y no está más: lo borraron en otro equipo.
            None if previo.is_some() && l.id != crate::perfiles::PRINCIPAL => {
                crate::perfiles::borrar_local(db, &l.id)?;
                cambios += 1;
            }
            None => {
                let r = api(&c.servidor, Some(&c.token), reqwest::Method::POST, "/perfiles", Some(&subir(l)))
                    .await?;
                let id = r["id"].as_i64().ok_or_else(|| ErrApi::Otro("respuesta sin id de perfil".into()))?;
                mapa.push(PerfilSync { local: l.id.clone(), remoto: id, cursor: 0 });
            }
        }
    }
    for r in remotos.iter().filter(|r| !locales.iter().any(|l| l.id == r.uid)) {
        crate::perfiles::crear_local(db, &r.uid, &r.nombre, &r.color, r.actualizado)?;
        mapa.push(PerfilSync { local: r.uid.clone(), remoto: r.id, cursor: 0 });
        cambios += 1;
    }
    c.perfiles = mapa;
    Ok(cambios)
}

async fn sincronizar(dir: &Path) -> Result<ResultadoSync, ErrApi> {
    let mut c = cargar(dir);
    if c.token.is_empty() {
        return Err(ErrApi::Otro("no hay sesión".into()));
    }
    let mut db = abrir_db(dir)?;
    let mut res = ResultadoSync { subidos: 0, bajados: 0 };

    res.bajados += sincronizar_perfiles(&mut c, &mut db).await?;
    guardar(dir, &c)?;

    for i in 0..c.perfiles.len() {
        let PerfilSync { local, remoto, .. } = c.perfiles[i].clone();

        // 1. Subir.
        let pend = pendientes(&db, &local)?;
        for lote in pend.chunks(LOTE) {
            let r = api(
                &c.servidor,
                Some(&c.token),
                reqwest::Method::POST,
                "/sync",
                Some(&json!({ "perfil": remoto, "cambios": lote })),
            )
            .await?;
            marcar_subidos(&db, &local, lote)?;
            // Lo que el servidor rechazó por tener algo más nuevo: aplicarlo aquí.
            let vigentes: Vec<Cambio> = r["resultados"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| serde_json::from_value(x.get("vigente")?.clone()).ok())
                .collect();
            res.bajados += aplicar(&mut db, &local, &vigentes)?;
            res.subidos += lote.len() - vigentes.len();
        }

        // 2. Bajar lo que cambió en otros equipos.
        loop {
            let r = api(
                &c.servidor,
                Some(&c.token),
                reqwest::Method::GET,
                &format!("/sync?perfil={}&desde={}", remoto, c.perfiles[i].cursor),
                None,
            )
            .await?;
            let cambios: Vec<Cambio> = serde_json::from_value(r["cambios"].clone())
                .map_err(|e| ErrApi::Otro(format!("respuesta inválida: {e}")))?;
            res.bajados += aplicar(&mut db, &local, &cambios)?;
            c.perfiles[i].cursor = r["hasta"].as_i64().unwrap_or(c.perfiles[i].cursor);
            guardar(dir, &c)?;
            if !r["mas"].as_bool().unwrap_or(false) {
                break;
            }
        }
    }
    c.ultima_sync = ahora_ms();
    guardar(dir, &c)?;
    Ok(res)
}

/// Una sincronización a la vez: el arranque, el temporizador y un favorito
/// recién marcado pueden pedirla casi juntos.
static EN_CURSO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// Sesión cerrada en el servidor (otro equipo la quitó, o cambió la contraseña):
// se olvida el token y se deja de anotar cambios. La URL queda para reentrar.
fn olvidar_sesion(dir: &Path) {
    let c = cargar(dir);
    let _ = guardar(dir, &Cuenta { servidor: c.servidor, ..Default::default() });
    crate::boveda::borrar_estado(dir);
    if let Ok(db) = abrir_db(dir) {
        let _ = set_activa(&db, false);
        let _ = db.execute("DELETE FROM sync_salida", []);
    }
}

async fn entrar(
    dir: &Path,
    servidor: &str,
    nombre: &str,
    clave: &str,
    invitacion: Option<String>,
) -> Result<(), String> {
    let servidor = normalizar_servidor(servidor)?;
    let invitacion = invitacion.map(|i| i.trim().to_string()).filter(|i| !i.is_empty());
    let (ruta, mut cuerpo) = match &invitacion {
        Some(inv) => ("/registro", json!({ "invitacion": inv })),
        None => ("/login", json!({})),
    };
    cuerpo["nombre"] = json!(nombre.trim());
    cuerpo["clave"] = json!(clave);
    cuerpo["equipo"] = json!(nombre_equipo());
    let v = api(&servidor, None, reqwest::Method::POST, ruta, Some(&cuerpo))
        .await
        .map_err(ErrApi::texto)?;
    let token = v["token"].as_str().ok_or("respuesta sin token")?.to_string();
    let usuario = v["usuario"].as_str().unwrap_or_default().to_string();
    // Los perfiles se emparejan en la primera sincronización (por uid: el
    // "principal" de aquí con el de la cuenta).
    let c = Cuenta {
        servidor,
        token,
        llave: crate::boveda::derivar_llave(&usuario, clave)?,
        usuario,
        ..Default::default()
    };
    guardar(dir, &c)?;
    // Sesión nueva: la bóveda se mezcla como la primera vez (manda la cuenta).
    crate::boveda::borrar_estado(dir);
    let db = abrir_db(dir)?;
    set_activa(&db, true)?;
    anotar_todo(&db)
}

async fn salir(dir: &Path) {
    let c = cargar(dir);
    if !c.token.is_empty() {
        // Si el servidor no contesta igual se cierra aquí; el token queda
        // huérfano en el servidor hasta que se quite el equipo desde otro.
        let _ = api(&c.servidor, Some(&c.token), reqwest::Method::POST, "/salir", None).await;
    }
    olvidar_sesion(dir);
}

async fn sincronizar_o_olvidar(dir: &Path) -> Result<ResultadoSync, String> {
    match sincronizar(dir).await {
        Ok(r) => Ok(r),
        Err(ErrApi::SinSesion) => {
            olvidar_sesion(dir);
            Err(ErrApi::SinSesion.texto())
        }
        Err(e) => Err(e.texto()),
    }
}

// ─── Comandos Tauri ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EstadoCuenta {
    pub servidor: String,
    pub usuario: Option<String>,
    pub ultima_sync: i64,
    /// Hay llave de la bóveda en este equipo.
    pub boveda: bool,
}

#[tauri::command]
pub fn cuenta_estado(app: tauri::AppHandle) -> Result<EstadoCuenta, String> {
    let c = cargar(&dir_config(&app)?);
    let con_sesion = !c.token.is_empty();
    Ok(EstadoCuenta {
        servidor: c.servidor,
        usuario: con_sesion.then_some(c.usuario),
        ultima_sync: c.ultima_sync,
        boveda: !c.llave.is_empty(),
    })
}

/// Comprueba que la URL apunta a un servidor de cuentas de Kutral.
#[tauri::command]
pub async fn cuenta_probar(servidor: String) -> Result<i64, String> {
    let servidor = normalizar_servidor(&servidor)?;
    let v = api(&servidor, None, reqwest::Method::GET, "/ping", None)
        .await
        .map_err(ErrApi::texto)?;
    if v["servicio"] != "kutral-cuentas" {
        return Err("esa URL responde, pero no es un servidor de cuentas de Kutral".into());
    }
    Ok(v["version"].as_i64().unwrap_or(0))
}

/// Entra con una cuenta existente o, si viene `invitacion`, la crea.
#[tauri::command]
pub async fn cuenta_entrar(
    app: tauri::AppHandle,
    servidor: String,
    nombre: String,
    clave: String,
    invitacion: Option<String>,
) -> Result<EstadoCuenta, String> {
    let dir = dir_config(&app)?;
    {
        let _turno = EN_CURSO.lock().await;
        entrar(&dir, &servidor, &nombre, &clave, invitacion).await?;
    }
    cuenta_estado(app)
}

/// Cierra la sesión de este equipo. Lo guardado aquí se queda.
#[tauri::command]
pub async fn cuenta_salir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = dir_config(&app)?;
    let _turno = EN_CURSO.lock().await;
    salir(&dir).await;
    Ok(())
}

#[tauri::command]
pub async fn cuenta_sincronizar(app: tauri::AppHandle) -> Result<ResultadoSync, String> {
    let dir = dir_config(&app)?;
    let _turno = EN_CURSO.lock().await;
    sincronizar_o_olvidar(&dir).await
}

// ─── Bóveda ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Default, Debug)]
pub struct ResultadoBoveda {
    /// Ajustes del front (las mismas claves que mandó) si cambiaron aquí.
    ajustes: Option<crate::boveda::Datos>,
    /// Llegó una vinculación del servidor (Real-Debrid u OpenSubtitles).
    credenciales: bool,
    /// Sesión sin llave: falta confirmar la contraseña en este equipo.
    bloqueada: bool,
}

const BOVEDA_CAMBIADA: &str = "la bóveda cambió desde otro equipo";

/// Baja la bóveda, la mezcla con lo de aquí (`front` = ajustes del webview)
/// y sube el resultado si el servidor no lo tenía.
async fn sincronizar_boveda(dir: &Path, front: crate::boveda::Datos) -> Result<ResultadoBoveda, ErrApi> {
    use crate::boveda::{self as b, CREDENCIALES};
    let c = cargar(dir);
    if c.token.is_empty() {
        return Err(ErrApi::Otro("no hay sesión".into()));
    }
    if c.llave.is_empty() {
        return Ok(ResultadoBoveda { bloqueada: true, ..Default::default() });
    }
    // Las vinculaciones solo salen de sus archivos, nunca del webview.
    let front: b::Datos = front.into_iter().filter(|(k, _)| !CREDENCIALES.iter().any(|(n, _)| n == k)).collect();
    // Si otro equipo sube justo entre la bajada y la subida, el servidor
    // responde 409: se vuelve a bajar y mezclar.
    for _ in 0..3 {
        let estado = b::cargar_estado(dir);
        let mut local = b::leer_credenciales(dir);
        local.extend(front.clone());
        let v = match api(&c.servidor, Some(&c.token), reqwest::Method::GET, "/boveda", None).await {
            Err(ErrApi::Otro(e)) if e.starts_with("no existe ") => {
                return Err(ErrApi::Otro("tu servidor es de una versión anterior: sube la carpeta server/ nueva".into()))
            }
            r => r?,
        };
        let version = v["version"].as_i64().unwrap_or(0);
        // Lo que hay de verdad en el servidor. None = nada, o algo que esta
        // llave no abre (quedó de antes de un cambio de contraseña): se pisa.
        let remoto = if version == 0 {
            None
        } else if version == estado.version && estado.base.is_some() {
            estado.base.clone()
        } else {
            v["datos"].as_str().and_then(|s| b::descifrar(&c.llave, s))
        };
        let mezcla = b::mezclar(&local, estado.base.as_ref(), remoto.as_ref().unwrap_or(&local));
        let credenciales = b::escribir_credenciales(dir, &local, &mezcla)?;
        let version = if remoto.as_ref() == Some(&mezcla) {
            version
        } else {
            let cuerpo = json!({ "base": version, "datos": b::cifrar(&c.llave, &mezcla)? });
            match api(&c.servidor, Some(&c.token), reqwest::Method::PUT, "/boveda", Some(&cuerpo)).await {
                Ok(r) => r["version"].as_i64().unwrap_or(version + 1),
                Err(ErrApi::Otro(e)) if e == BOVEDA_CAMBIADA => continue,
                Err(e) => return Err(e),
            }
        };
        b::guardar_estado(dir, &b::Estado { version, base: Some(mezcla.clone()) })?;
        let cambio = front.iter().any(|(k, v)| mezcla.get(k).unwrap_or(&Value::Null) != v);
        let ajustes = cambio.then(|| {
            front.keys().map(|k| (k.clone(), mezcla.get(k).cloned().unwrap_or(Value::Null))).collect()
        });
        return Ok(ResultadoBoveda { ajustes, credenciales, bloqueada: false });
    }
    Err(ErrApi::Otro(format!("{BOVEDA_CAMBIADA}; se reintenta en la próxima sincronización")))
}

/// Sesión de antes de la bóveda: confirma la contraseña con el servidor y
/// guarda la llave que sale de ella.
async fn desbloquear(dir: &Path, clave: &str) -> Result<(), ErrApi> {
    let mut c = cargar(dir);
    if c.token.is_empty() {
        return Err(ErrApi::Otro("no hay sesión".into()));
    }
    match api(&c.servidor, Some(&c.token), reqwest::Method::POST, "/yo/clave", Some(&json!({ "clave": clave }))).await {
        Err(ErrApi::Otro(e)) if e.starts_with("no existe ") => {
            return Err(ErrApi::Otro("tu servidor es de una versión anterior: sube la carpeta server/ nueva".into()))
        }
        r => r?,
    };
    c.llave = crate::boveda::derivar_llave(&c.usuario, clave)?;
    guardar(dir, &c)?;
    crate::boveda::borrar_estado(dir);
    Ok(())
}

/// Sincroniza la bóveda. `ajustes` son los del webview (config.svelte.ts).
#[tauri::command]
pub async fn cuenta_boveda(
    app: tauri::AppHandle,
    ajustes: crate::boveda::Datos,
) -> Result<ResultadoBoveda, String> {
    let dir = dir_config(&app)?;
    let _turno = EN_CURSO.lock().await;
    match sincronizar_boveda(&dir, ajustes).await {
        Ok(r) => Ok(r),
        Err(ErrApi::SinSesion) => {
            olvidar_sesion(&dir);
            Err(ErrApi::SinSesion.texto())
        }
        Err(e) => Err(e.texto()),
    }
}

#[tauri::command]
pub async fn cuenta_desbloquear(app: tauri::AppHandle, clave: String) -> Result<EstadoCuenta, String> {
    let dir = dir_config(&app)?;
    {
        let _turno = EN_CURSO.lock().await;
        desbloquear(&dir, &clave).await.map_err(ErrApi::texto)?;
    }
    cuenta_estado(app)
}

// ─── Perfiles ───────────────────────────────────────────────────────────────

/// Borra un perfil con todo lo suyo. Con cuenta, primero en el servidor (así
/// los demás equipos se enteran): sin red no se puede borrar uno que ya se
/// sincronizó, para que no reaparezca en la próxima sincronización.
#[tauri::command]
pub async fn perfil_borrar(app: tauri::AppHandle, id: String) -> Result<(), String> {
    if id == crate::perfiles::PRINCIPAL {
        return Err("el perfil principal no se puede borrar".into());
    }
    let dir = dir_config(&app)?;
    let _turno = EN_CURSO.lock().await;
    let mut c = cargar(&dir);
    if let Some(m) = c.perfiles.iter().find(|p| p.local == id).cloned() {
        if !c.token.is_empty() {
            match api(&c.servidor, Some(&c.token), reqwest::Method::DELETE, &format!("/perfiles/{}", m.remoto), None)
                .await
            {
                Ok(_) => {}
                Err(ErrApi::Otro(e)) if e.starts_with("no se pudo conectar") => {
                    return Err("sin conexión con el servidor: para borrar un perfil sincronizado hace falta red".into())
                }
                Err(e) => return Err(e.texto()),
            }
        }
        c.perfiles.retain(|p| p.local != id);
        guardar(&dir, &c)?;
    }
    let mut db = abrir_db(&dir)?;
    crate::perfiles::borrar_local(&mut db, &id)
}

// ─── Amigos y recomendaciones ───────────────────────────────────────────────
//
// Pasan tal cual al front lo que responde el servidor: aquí solo se pone el
// token, que el webview nunca ve. Un servidor sin estas rutas (versión 1)
// responde 404 con "no existe ...".

/// ¿Hay sesión en este equipo? Sin red.
pub(crate) fn con_cuenta(app: &tauri::AppHandle) -> bool {
    dir_config(app).map(|d| !cargar(&d).token.is_empty()).unwrap_or(false)
}

pub(crate) async fn con_sesion(
    app: &tauri::AppHandle,
    metodo: reqwest::Method,
    ruta: &str,
    cuerpo: Option<&Value>,
) -> Result<Value, String> {
    let dir = dir_config(app)?;
    let c = cargar(&dir);
    if c.token.is_empty() {
        return Err("no hay sesión".into());
    }
    match api(&c.servidor, Some(&c.token), metodo, ruta, cuerpo).await {
        Ok(v) => Ok(v),
        Err(ErrApi::SinSesion) => {
            let _turno = EN_CURSO.lock().await;
            olvidar_sesion(&dir);
            Err(ErrApi::SinSesion.texto())
        }
        Err(ErrApi::Otro(e)) if e.starts_with("no existe ") => {
            Err("tu servidor es de una versión anterior: sube la carpeta server/ nueva".into())
        }
        Err(e) => Err(e.texto()),
    }
}

#[tauri::command]
pub async fn cuenta_amigos(app: tauri::AppHandle) -> Result<Value, String> {
    con_sesion(&app, reqwest::Method::GET, "/amigos", None).await
}

/// Pide la amistad, o la acepta si el otro ya la había pedido.
#[tauri::command]
pub async fn cuenta_amigo_agregar(app: tauri::AppHandle, nombre: String) -> Result<Value, String> {
    let cuerpo = json!({ "nombre": nombre.trim() });
    con_sesion(&app, reqwest::Method::POST, "/amigos", Some(&cuerpo)).await
}

/// Rechaza, cancela o deja de ser amigo: en el servidor es lo mismo.
#[tauri::command]
pub async fn cuenta_amigo_quitar(app: tauri::AppHandle, nombre: String) -> Result<Value, String> {
    let nombre = nombre.trim().to_lowercase();
    if nombre.is_empty() || !nombre.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) {
        return Err("nombre inválido".into());
    }
    con_sesion(&app, reqwest::Method::DELETE, &format!("/amigos/{nombre}"), None).await
}

#[tauri::command]
pub async fn cuenta_recomendaciones(app: tauri::AppHandle) -> Result<Value, String> {
    con_sesion(&app, reqwest::Method::GET, "/recomendaciones", None).await
}

#[tauri::command]
pub async fn cuenta_recomendar(
    app: tauri::AppHandle,
    para: Vec<String>,
    datos: Value,
    nota: Option<String>,
) -> Result<Value, String> {
    let cuerpo = json!({ "para": para, "datos": datos, "nota": nota });
    con_sesion(&app, reqwest::Method::POST, "/recomendaciones", Some(&cuerpo)).await
}

#[tauri::command]
pub async fn cuenta_recomendaciones_vistas(app: tauri::AppHandle) -> Result<Value, String> {
    con_sesion(&app, reqwest::Method::POST, "/recomendaciones/vistas", None).await
}

#[tauri::command]
pub async fn cuenta_recomendacion_quitar(app: tauri::AppHandle, id: i64) -> Result<Value, String> {
    con_sesion(&app, reqwest::Method::DELETE, &format!("/recomendaciones/{id}"), None).await
}

// ─── Listas compartidas ─────────────────────────────────────────────────────

fn codigo_valido(codigo: &str) -> Result<String, String> {
    let c = codigo.trim().to_uppercase();
    let ok = c.len() == 11
        && c.char_indices().all(|(i, ch)| if i == 5 { ch == '-' } else { ch.is_ascii_alphanumeric() });
    if ok {
        Ok(c)
    } else {
        Err("el código tiene la forma XXXXX-XXXXX".into())
    }
}

/// Las listas que esta cuenta comparte: `{publicas: [{codigo, lista_id, nombre, actualizada}]}`.
#[tauri::command]
pub async fn cuenta_mis_publicas(app: tauri::AppHandle) -> Result<Value, String> {
    con_sesion(&app, reqwest::Method::GET, "/listas-publicas", None).await
}

/// Publica o actualiza una lista; el código se mantiene.
#[tauri::command]
pub async fn cuenta_publicar_lista(
    app: tauri::AppHandle,
    lista_id: String,
    nombre: String,
    items: Value,
) -> Result<Value, String> {
    let cuerpo = json!({ "lista_id": lista_id, "nombre": nombre, "items": items });
    con_sesion(&app, reqwest::Method::POST, "/listas-publicas", Some(&cuerpo)).await
}

#[tauri::command]
pub async fn cuenta_despublicar_lista(app: tauri::AppHandle, codigo: String) -> Result<Value, String> {
    let c = codigo_valido(&codigo)?;
    con_sesion(&app, reqwest::Method::DELETE, &format!("/listas-publicas/{c}"), None).await
}

/// Lista compartida por código: `{codigo, nombre, de, items}`.
#[tauri::command]
pub async fn cuenta_lista_publica(app: tauri::AppHandle, codigo: String) -> Result<Value, String> {
    let c = codigo_valido(&codigo)?;
    con_sesion(&app, reqwest::Method::GET, &format!("/listas-publicas/{c}"), None).await
}

// Prueba de punta a punta contra un servidor real (server/ del repo):
//
//   php -S 127.0.0.1:18790 -t server/public server/public/index.php
//   KUTRAL_TEST_SERVIDOR=http://127.0.0.1:18790 \
//   KUTRAL_TEST_INVITACION=$(php server/admin.php invitar) \
//   KUTRAL_TEST_INVITACION_2=$(php server/admin.php invitar) \
//   KUTRAL_TEST_INVITACION_3=$(php server/admin.php invitar) \
//   KUTRAL_TEST_INVITACION_4=$(php server/admin.php invitar) \
//     cargo test cuenta -- --ignored
//
// Dos carpetas = dos equipos de la misma cuenta. Lo que haría el front se
// simula con SQL directo sobre `favorites`, así los triggers son los reales.
#[cfg(test)]
mod tests {
    use super::*;

    // Las tablas que crean las migraciones v7 y v11 de lib.rs.
    const FAVORITES: &str = "CREATE TABLE favorites (
        imdb_id TEXT PRIMARY KEY,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE watch_history (
        imdb_id TEXT NOT NULL,
        season INTEGER NOT NULL DEFAULT -1,
        episode INTEGER NOT NULL DEFAULT -1,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        still_path TEXT,
        episode_title TEXT,
        watched_seconds INTEGER NOT NULL DEFAULT 0,
        runtime_seconds INTEGER,
        progress_real REAL,
        completed INTEGER NOT NULL DEFAULT 0,
        last_watched INTEGER NOT NULL,
        PRIMARY KEY (imdb_id, season, episode)
    );
    CREATE TABLE listas (
        id TEXT PRIMARY KEY,
        nombre TEXT NOT NULL,
        creada INTEGER NOT NULL,
        actualizada INTEGER NOT NULL
    );
    CREATE TABLE lista_items (
        lista_id TEXT NOT NULL,
        clave TEXT NOT NULL,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        posicion REAL NOT NULL,
        agregado INTEGER NOT NULL,
        PRIMARY KEY (lista_id, clave)
    );";

    fn equipo(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kutral-cuenta-{nombre}-{}", ahora_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = abrir_db(&dir).unwrap();
        db.execute_batch(FAVORITES).unwrap();
        db.execute_batch(MIGRACION_SQL).unwrap();
        db.execute_batch(MIGRACION_SQL_V12).unwrap();
        db.execute_batch(crate::perfiles::MIGRACION_SQL).unwrap();
        db.execute_batch(crate::perfiles::TRIGGERS_SQL).unwrap();
        dir
    }

    // Lo que hace guardarProgreso en el front (mismo UPSERT con MAX(completed)).
    fn progreso(dir: &Path, clave: &str, season: i64, episode: i64, segundos: i64, completo: bool) {
        abrir_db(dir)
            .unwrap()
            .execute(
                "INSERT INTO watch_history (imdb_id, season, episode, tmdb_id, media_type, title,
                    watched_seconds, runtime_seconds, completed, last_watched)
                 VALUES (?1, ?2, ?3, 1, 'tv', 'Serie', ?4, 3000, ?5, ?6)
                 ON CONFLICT(perfil, imdb_id, season, episode) DO UPDATE SET
                   watched_seconds = excluded.watched_seconds,
                   completed = MAX(excluded.completed, watch_history.completed),
                   last_watched = excluded.last_watched",
                params![clave, season, episode, segundos, completo as i64, ahora_ms()],
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }

    fn historial_de(dir: &Path) -> Vec<(String, i64, i64, i64, i64)> {
        let db = abrir_db(dir).unwrap();
        let mut st = db
            .prepare(
                "SELECT imdb_id, season, episode, watched_seconds, completed FROM watch_history
                  ORDER BY imdb_id, season, episode",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn sql(dir: &Path, q: &str) {
        abrir_db(dir).unwrap().execute_batch(q).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }

    fn items_de(dir: &Path, lista: &str) -> Vec<(String, f64)> {
        let db = abrir_db(dir).unwrap();
        let mut st = db
            .prepare("SELECT clave, posicion FROM lista_items WHERE lista_id = ?1 ORDER BY posicion")
            .unwrap();
        st.query_map(params![lista], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn listas_de(dir: &Path) -> Vec<(String, String)> {
        let db = abrir_db(dir).unwrap();
        let mut st = db.prepare("SELECT id, nombre FROM listas ORDER BY id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn fav(dir: &Path, clave: &str, title: &str) {
        abrir_db(dir)
            .unwrap()
            .execute(
                "INSERT INTO favorites (imdb_id, tmdb_id, media_type, title, poster_path, added_at)
                 VALUES (?1, 1, 'movie', ?2, NULL, ?3)
                 ON CONFLICT(perfil, imdb_id) DO UPDATE SET title = excluded.title, added_at = excluded.added_at",
                params![clave, title, ahora_ms()],
            )
            .unwrap();
        // Dos cambios en el mismo ms no tendrían un ganador claro.
        std::thread::sleep(Duration::from_millis(5));
    }

    fn quitar(dir: &Path, clave: &str) {
        abrir_db(dir)
            .unwrap()
            .execute("DELETE FROM favorites WHERE imdb_id = ?1", params![clave])
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }

    fn favs(dir: &Path) -> Vec<(String, String)> {
        let db = abrir_db(dir).unwrap();
        let mut st = db.prepare("SELECT imdb_id, title FROM favorites ORDER BY imdb_id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn pendientes_de(dir: &Path) -> i64 {
        abrir_db(dir)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_salida", [], |r| r.get(0))
            .unwrap()
    }

    async fn sync(dir: &Path) -> (usize, usize) {
        let r = sincronizar_o_olvidar(dir).await.unwrap();
        (r.subidos, r.bajados)
    }

    fn par(clave: &str, title: &str) -> (String, String) {
        (clave.into(), title.into())
    }

    #[test]
    fn servidor_exige_https_fuera_de_la_red_local() {
        assert!(normalizar_servidor("https://kutral.ejemplo.cl/api/").is_ok());
        assert_eq!(
            normalizar_servidor(" http://192.168.1.20:8080/ ").unwrap(),
            "http://192.168.1.20:8080"
        );
        assert!(normalizar_servidor("http://localhost:18790").is_ok());
        assert!(normalizar_servidor("http://kutral.ejemplo.cl").is_err());
        assert!(normalizar_servidor("http://8.8.8.8").is_err());
        assert!(normalizar_servidor("ftp://192.168.1.2").is_err());
        assert!(normalizar_servidor("kutral.ejemplo.cl").is_err());
    }

    // Regresión: guardar el progreso dos veces sin sincronizar en medio (lo
    // normal mientras se ve algo) fallaba por la clave repetida en sync_salida.
    #[test]
    fn upsert_repetido_con_cuenta_no_falla() {
        let a = equipo("upsert");
        set_activa(&abrir_db(&a).unwrap(), true).unwrap();
        progreso(&a, "tt1", -1, -1, 100, false);
        progreso(&a, "tt1", -1, -1, 200, false);
        progreso(&a, "tt1", -1, -1, 300, false);
        fav(&a, "tt2", "Uno");
        fav(&a, "tt2", "Otra vez");
        assert_eq!(historial_de(&a)[0].3, 300);
        assert_eq!(pendientes_de(&a), 2, "una entrada por fila, con la fecha del último cambio");
        let _ = std::fs::remove_dir_all(&a);
    }

    #[test]
    fn sin_cuenta_no_se_anota_nada() {
        let a = equipo("sincuenta");
        fav(&a, "tt1", "Uno");
        quitar(&a, "tt1");
        assert_eq!(pendientes_de(&a), 0);
    }

    #[test]
    fn codigos_de_lista() {
        assert_eq!(codigo_valido(" r9nal-npjdr ").unwrap(), "R9NAL-NPJDR");
        assert!(codigo_valido("R9NAL/NPJDR").is_err());
        assert!(codigo_valido("R9NAL-NPJD").is_err());
        assert!(codigo_valido("../../admin").is_err());
    }

    #[test]
    fn claves_compuestas() {
        assert_eq!(partir_historial("tt1|-1|-1"), Some(("tt1", -1, -1)));
        assert_eq!(partir_historial("anilist:5|1|12"), Some(("anilist:5", 1, 12)));
        assert_eq!(partir_historial("tt1"), None);
        assert_eq!(partir_item("abc-123|anilist:5"), Some(("abc-123", "anilist:5")));
    }

    #[test]
    fn migracion_v12_anota_lo_previo_solo_con_cuenta() {
        for activa in [false, true] {
            let dir = std::env::temp_dir().join(format!("kutral-cuenta-v12-{activa}-{}", ahora_ms()));
            std::fs::create_dir_all(&dir).unwrap();
            let db = abrir_db(&dir).unwrap();
            db.execute_batch(FAVORITES).unwrap();
            db.execute_batch(MIGRACION_SQL).unwrap();
            set_activa(&db, activa).unwrap();
            db.execute_batch(
                "INSERT INTO watch_history (imdb_id, tmdb_id, media_type, title, last_watched)
                 VALUES ('tt1', 1, 'movie', 'Uno', 5)",
            )
            .unwrap();
            db.execute_batch(MIGRACION_SQL_V12).unwrap();
            let n: i64 = db.query_row("SELECT COUNT(*) FROM sync_salida", [], |r| r.get(0)).unwrap();
            assert_eq!(n, if activa { 1 } else { 0 }, "el historial previo, solo con cuenta");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[tokio::test]
    #[ignore = "necesita el servidor PHP corriendo (ver arriba)"]
    async fn historial_y_listas_entre_equipos() {
        let servidor = std::env::var("KUTRAL_TEST_SERVIDOR").expect("KUTRAL_TEST_SERVIDOR");
        let inv = std::env::var("KUTRAL_TEST_INVITACION_2").expect("KUTRAL_TEST_INVITACION_2");
        let usuario = format!("listas{}", ahora_ms() % 1_000_000);
        let (a, b) = (equipo("ha"), equipo("hb"));
        // Lo que crea la migración v11 en cada equipo.
        for e in [&a, &b] {
            sql(e, "INSERT INTO listas (id, nombre, creada, actualizada) VALUES ('quiero-ver', 'Quiero ver', 1, 1)");
        }

        // A tenía historial antes de la cuenta; al entrar se sube.
        progreso(&a, "tt9", 1, 1, 600, false);
        entrar(&a, &servidor, &usuario, "contraseña-larga", Some(inv)).await.unwrap();
        entrar(&b, &servidor, &usuario, "contraseña-larga", None).await.unwrap();
        sync(&a).await;
        sync(&b).await;
        assert_eq!(historial_de(&b), vec![("tt9".into(), 1, 1, 600, 0)], "el minuto viaja");

        // Sigue en B, termina el capítulo; A lo recibe.
        progreso(&b, "tt9", 1, 1, 2900, true);
        sync(&b).await;
        sync(&a).await;
        assert_eq!(historial_de(&a), vec![("tt9".into(), 1, 1, 2900, 1)]);

        // A, sin sincronizar, había retrocedido (avance más nuevo pero sin
        // terminar): "terminado" no se pierde en ningún lado.
        progreso(&a, "tt9", 1, 1, 100, false);
        assert_eq!(historial_de(&a)[0].4, 1, "en el propio equipo ya no retrocedía");
        sync(&a).await;
        sync(&b).await;
        assert_eq!(historial_de(&b)[0].4, 1);
        assert_eq!(historial_de(&b)[0].3, 100, "el minuto más nuevo sí gana");

        // Borrar del historial viaja.
        sql(&b, "DELETE FROM watch_history WHERE imdb_id = 'tt9'");
        sync(&b).await;
        sync(&a).await;
        assert!(historial_de(&a).is_empty());

        // Listas: crear, agregar y reordenar en A; renombrar en B.
        sql(
            &a,
            "INSERT INTO listas (id, nombre, creada, actualizada) VALUES ('l1', 'Nolan', 10, 10);
             INSERT INTO lista_items (lista_id, clave, tmdb_id, media_type, title, poster_path, posicion, agregado)
               VALUES ('l1', 'tt1', 1, 'movie', 'Memento', NULL, 1, 10),
                      ('l1', 'tt2', 2, 'movie', 'Tenet', NULL, 2, 11),
                      ('quiero-ver', 'anilist:5', 5, 'anime', 'Bebop', NULL, 1, 12);",
        );
        sync(&a).await;
        sync(&b).await;
        assert_eq!(items_de(&b, "l1"), vec![("tt1".into(), 1.0), ("tt2".into(), 2.0)]);
        assert_eq!(items_de(&b, "quiero-ver"), vec![("anilist:5".into(), 1.0)]);
        sql(&a, "UPDATE lista_items SET posicion = 3 WHERE lista_id = 'l1' AND clave = 'tt1'");
        sql(&b, "UPDATE listas SET nombre = 'Maratón Nolan', actualizada = 20 WHERE id = 'l1'");
        sync(&a).await;
        sync(&b).await;
        sync(&a).await;
        assert_eq!(items_de(&b, "l1"), vec![("tt2".into(), 2.0), ("tt1".into(), 3.0)]);
        assert!(listas_de(&a).contains(&("l1".into(), "Maratón Nolan".into())));

        // Borrar la lista viaja; Quiero ver nunca se borra.
        sql(&b, "DELETE FROM lista_items WHERE lista_id = 'l1'; DELETE FROM listas WHERE id = 'l1';");
        sync(&b).await;
        sync(&a).await;
        assert_eq!(listas_de(&a), vec![("quiero-ver".into(), "Quiero ver".into())]);
        assert!(items_de(&a, "l1").is_empty());
        assert_eq!(pendientes_de(&a), 0);
        assert_eq!(pendientes_de(&b), 0);

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    fn perfiles_de(dir: &Path) -> Vec<(String, String)> {
        let db = abrir_db(dir).unwrap();
        crate::perfiles::listar(&db).unwrap().into_iter().map(|p| (p.id, p.nombre)).collect()
    }

    fn favs_de(dir: &Path, perfil: &str) -> Vec<String> {
        let db = abrir_db(dir).unwrap();
        let mut st = db.prepare("SELECT imdb_id FROM favorites WHERE perfil = ?1 ORDER BY imdb_id").unwrap();
        st.query_map(params![perfil], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    }

    #[test]
    fn cuenta_json_viejo_pasa_a_perfiles() {
        let dir = std::env::temp_dir().join(format!("kutral-cuenta-json-{}", ahora_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cuenta.json"),
            r#"{"servidor":"https://x","token":"t","usuario":"nick","perfil_id":7,"perfil_nombre":"nick","cursor":42,"ultima_sync":1}"#,
        )
        .unwrap();
        let c = cargar(&dir);
        assert_eq!(c.perfiles, vec![PerfilSync { local: "principal".into(), remoto: 7, cursor: 42 }]);
        guardar(&dir, &c).unwrap();
        let raw = std::fs::read_to_string(dir.join("cuenta.json")).unwrap();
        assert!(!raw.contains("perfil_id"), "lo viejo no se vuelve a escribir");
        assert_eq!(cargar(&dir).perfiles, c.perfiles);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[ignore = "necesita el servidor PHP corriendo (ver arriba)"]
    async fn perfiles_entre_equipos() {
        let servidor = std::env::var("KUTRAL_TEST_SERVIDOR").expect("KUTRAL_TEST_SERVIDOR");
        let inv = std::env::var("KUTRAL_TEST_INVITACION_3").expect("KUTRAL_TEST_INVITACION_3");
        let usuario = format!("perf{}", ahora_ms() % 1_000_000);
        let (a, b) = (equipo("pa"), equipo("pb"));

        // A crea el perfil "Ana" sin cuenta todavía, con un favorito suyo.
        sql(
            &a,
            "INSERT INTO perfiles (id, nombre, color, creado, actualizado) VALUES ('ana1', 'Ana', '#ff0000', 5, 5);
             INSERT INTO favorites (perfil, imdb_id, tmdb_id, media_type, title, added_at)
               VALUES ('ana1', 'tt100', 1, 'movie', 'De Ana', 5);",
        );
        fav(&a, "tt200", "Del principal");
        entrar(&a, &servidor, &usuario, "contraseña-larga", Some(inv)).await.unwrap();
        entrar(&b, &servidor, &usuario, "contraseña-larga", None).await.unwrap();
        sync(&a).await;
        sync(&b).await;

        // B recibe el perfil y cada favorito queda en su perfil.
        assert!(perfiles_de(&b).contains(&("ana1".into(), "Ana".into())));
        assert_eq!(favs_de(&b, "ana1"), vec!["tt100".to_string()]);
        assert_eq!(favs_de(&b, "principal"), vec!["tt200".to_string()]);

        // Renombrar en B llega a A.
        sql(&b, &format!("UPDATE perfiles SET nombre = 'Anita', actualizado = {} WHERE id = 'ana1'", ahora_ms()));
        sync(&b).await;
        sync(&a).await;
        assert!(perfiles_de(&a).contains(&("ana1".into(), "Anita".into())));

        // Un favorito nuevo de Ana en B no aparece en el principal de A.
        sql(&b, "INSERT INTO favorites (perfil, imdb_id, tmdb_id, media_type, title, added_at)
                 VALUES ('ana1', 'tt300', 1, 'movie', 'Otro de Ana', 9)");
        sync(&b).await;
        sync(&a).await;
        assert_eq!(favs_de(&a, "ana1"), vec!["tt100".to_string(), "tt300".to_string()]);
        assert_eq!(favs_de(&a, "principal"), vec!["tt200".to_string()]);

        // Borrar el perfil en A (vía servidor) lo borra en B con todo lo suyo.
        let mut c = cargar(&a);
        let remoto = c.perfiles.iter().find(|p| p.local == "ana1").unwrap().remoto;
        api(&c.servidor, Some(&c.token), reqwest::Method::DELETE, &format!("/perfiles/{remoto}"), None)
            .await
            .unwrap();
        c.perfiles.retain(|p| p.local != "ana1");
        guardar(&a, &c).unwrap();
        crate::perfiles::borrar_local(&mut abrir_db(&a).unwrap(), "ana1").unwrap();
        sync(&a).await;
        sync(&b).await;
        assert!(!perfiles_de(&b).iter().any(|p| p.0 == "ana1"));
        assert!(favs_de(&b, "ana1").is_empty());
        assert_eq!(favs_de(&b, "principal"), vec!["tt200".to_string()]);
        assert_eq!(pendientes_de(&a), 0);
        assert_eq!(pendientes_de(&b), 0);

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[tokio::test]
    #[ignore = "necesita el servidor PHP corriendo (ver arriba)"]
    async fn dos_equipos_misma_cuenta() {
        let servidor = std::env::var("KUTRAL_TEST_SERVIDOR").expect("KUTRAL_TEST_SERVIDOR");
        let inv = std::env::var("KUTRAL_TEST_INVITACION").expect("KUTRAL_TEST_INVITACION");
        let usuario = format!("prueba{}", ahora_ms() % 1_000_000);
        let (a, b) = (equipo("a"), equipo("b"));

        // Cada equipo tenía lo suyo antes de tener cuenta.
        fav(&a, "tt0001", "De A");
        fav(&b, "tt0002", "De B");

        entrar(&a, &servidor, &usuario, "contraseña-larga", Some(inv)).await.unwrap();
        entrar(&b, &servidor, &usuario, "contraseña-larga", None).await.unwrap();
        assert_eq!(pendientes_de(&a), 1, "al entrar se anota lo que ya había");

        // Se unen, no se pisan.
        assert_eq!(sync(&a).await, (1, 0));
        assert_eq!(sync(&b).await, (1, 1));
        assert_eq!(sync(&a).await, (0, 1));
        let ambos = vec![par("tt0001", "De A"), par("tt0002", "De B")];
        assert_eq!(favs(&a), ambos);
        assert_eq!(favs(&b), ambos);
        assert_eq!(pendientes_de(&b), 0, "lo bajado no se vuelve a anotar");

        // El eco de lo propio no cuenta como cambio.
        assert_eq!(sync(&a).await, (0, 0));

        // Un borrado viaja.
        quitar(&a, "tt0001");
        assert_eq!(sync(&a).await, (1, 0));
        assert_eq!(sync(&b).await, (0, 1));
        assert_eq!(favs(&b), vec![par("tt0002", "De B")]);

        // Los dos tocan lo mismo sin sincronizar: gana el más reciente, aunque
        // el más viejo llegue después al servidor.
        fav(&b, "tt0003", "Viejo de B");
        fav(&a, "tt0003", "Nuevo de A");
        assert_eq!(sync(&a).await, (1, 0));
        assert_eq!(sync(&b).await, (0, 1), "el servidor rechaza lo viejo y devuelve lo vigente");
        assert_eq!(favs(&b), favs(&a));
        assert!(favs(&b).contains(&par("tt0003", "Nuevo de A")));
        assert_eq!(pendientes_de(&b), 0);

        // Cambio local nuevo que todavía no sube vs. cambio viejo del servidor:
        // el bajado no lo pisa y el local sale en la siguiente.
        fav(&a, "tt0004", "A primero");
        assert_eq!(sync(&a).await, (1, 0));
        fav(&b, "tt0004", "B después");
        assert_eq!(sync(&b).await, (1, 0));
        assert_eq!(sync(&a).await, (0, 1));
        assert!(favs(&a).contains(&par("tt0004", "B después")));

        // Salir en B: deja de anotar, lo local se queda y ya no sincroniza.
        salir(&b).await;
        let antes = favs(&b);
        fav(&b, "tt0005", "Sin cuenta");
        assert_eq!(pendientes_de(&b), 0);
        assert!(sincronizar_o_olvidar(&b).await.is_err());
        assert_eq!(favs(&b).len(), antes.len() + 1);
        assert_eq!(cargar(&b).servidor, servidor, "la URL se recuerda para reentrar");

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
    fn ajustes(v: Value) -> crate::boveda::Datos {
        v.as_object().unwrap().clone()
    }

    #[tokio::test]
    #[ignore = "necesita el servidor PHP corriendo (ver arriba)"]
    async fn boveda_entre_equipos() {
        let servidor = std::env::var("KUTRAL_TEST_SERVIDOR").expect("KUTRAL_TEST_SERVIDOR");
        let inv = std::env::var("KUTRAL_TEST_INVITACION_4").expect("KUTRAL_TEST_INVITACION_4");
        let usuario = format!("bov{}", ahora_ms() % 1_000_000);
        let (a, b) = (equipo("ba"), equipo("bb"));
        let rd = json!({ "access_token": "acc", "refresh_token": "ref", "client_id": "cid", "client_secret": "sec", "expires_at": 0 });
        std::fs::write(a.join("rd_creds.json"), rd.to_string()).unwrap();

        // A, el de siempre, sube sus claves.
        entrar(&a, &servidor, &usuario, "contraseña-larga", Some(inv)).await.unwrap();
        let front_a = ajustes(json!({ "tmdb_key": "clave-tmdb-secreta", "subs_lang": "en" }));
        let r = sincronizar_boveda(&a, front_a.clone()).await.unwrap();
        assert!(r.ajustes.is_none() && !r.credenciales);

        // El servidor solo tiene texto cifrado.
        let c = cargar(&a);
        let v = api(&c.servidor, Some(&c.token), reqwest::Method::GET, "/boveda", None).await.unwrap();
        let crudo = v["datos"].as_str().unwrap();
        assert!(!crudo.contains("clave-tmdb-secreta") && !crudo.contains("ref"));

        // B, recién instalado, entra y queda igual que A.
        entrar(&b, &servidor, &usuario, "contraseña-larga", None).await.unwrap();
        let front_b = ajustes(json!({ "tmdb_key": null, "subs_lang": "es" }));
        let r = sincronizar_boveda(&b, front_b).await.unwrap();
        assert!(r.credenciales);
        assert_eq!(Value::Object(r.ajustes.unwrap()), json!({ "tmdb_key": "clave-tmdb-secreta", "subs_lang": "en" }));
        let rd_b: Value = serde_json::from_str(&std::fs::read_to_string(b.join("rd_creds.json")).unwrap()).unwrap();
        assert_eq!(rd_b, rd);

        // Un cambio en B llega a A; lo que A no tocó no se pisa con nada viejo.
        let front_b = ajustes(json!({ "tmdb_key": "clave-tmdb-secreta", "subs_lang": "fr" }));
        assert!(sincronizar_boveda(&b, front_b.clone()).await.unwrap().ajustes.is_none());
        let r = sincronizar_boveda(&a, front_a).await.unwrap();
        assert_eq!(r.ajustes.unwrap()["subs_lang"], "fr");
        let front_a = ajustes(json!({ "tmdb_key": "clave-tmdb-secreta", "subs_lang": "fr" }));
        assert!(sincronizar_boveda(&a, front_a.clone()).await.unwrap().ajustes.is_none());
        assert!(sincronizar_boveda(&b, front_b).await.unwrap().ajustes.is_none());

        // Sesión de antes de la bóveda: bloqueada hasta confirmar la contraseña.
        let mut c = cargar(&a);
        c.llave.clear();
        guardar(&a, &c).unwrap();
        assert!(sincronizar_boveda(&a, front_a.clone()).await.unwrap().bloqueada);
        assert!(desbloquear(&a, "otra-contraseña").await.is_err());
        desbloquear(&a, "contraseña-larga").await.unwrap();
        let r = sincronizar_boveda(&a, front_a).await.unwrap();
        assert!(!r.bloqueada && r.ajustes.is_none());

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
