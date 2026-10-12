// Cuenta de Kutral en un servidor propio (ver server/ en la raíz del repo).
//
// Opcional: sin cuenta todo sigue funcionando igual, solo en este equipo.
// Con cuenta, lo sincronizable (hoy: favoritos) se sube y se baja de un
// servidor PHP cuya URL pone el usuario en Configuración.
//
// Cómo se sabe qué subir: triggers de SQLite (migración v10) anotan en
// `sync_salida` cada favorito que cambia, sin importar quién lo escribió
// (el front vía plugin-sql). Al sincronizar se sube lo anotado y se baja lo
// que otros equipos cambiaron desde el último cursor. Si dos equipos tocan lo
// mismo gana el cambio más reciente (`actualizado`, ms).
//
// Para que aplicar lo bajado no se vuelva a anotar como cambio local, se
// escribe dentro de una transacción con `sync_estado.aplicando = 1`; otras
// conexiones nunca ven ese 1 porque se vuelve a 0 antes del COMMIT.
//
// El token del equipo vive en `cuenta.json` (0600), como rd_creds.json: el
// webview nunca lo recibe.

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
    #[serde(default)]
    perfil_id: i64,
    #[serde(default)]
    perfil_nombre: String,
    /// Último `seq` del servidor ya aplicado aquí.
    #[serde(default)]
    cursor: i64,
    /// ms de la última sincronización completa. 0 = nunca.
    #[serde(default)]
    ultima_sync: i64,
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
    std::fs::read_to_string(dir.join("cuenta.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
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

const FAVORITOS: &str = "favoritos";

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
    db.execute_batch(
        "DELETE FROM sync_salida;
         INSERT OR REPLACE INTO sync_salida (coleccion, clave, cambiado)
           SELECT 'favoritos', imdb_id, added_at FROM favorites;",
    )
    .map_err(|e| e.to_string())
}

/// Lo anotado para subir, con el estado actual de cada fila.
fn pendientes(db: &Connection) -> Result<Vec<Cambio>, String> {
    let mut st = db
        .prepare("SELECT coleccion, clave, cambiado FROM sync_salida ORDER BY cambiado")
        .map_err(|e| e.to_string())?;
    let filas: Vec<(String, String, i64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut fav = db
        .prepare(
            "SELECT tmdb_id, media_type, title, poster_path, added_at
               FROM favorites WHERE imdb_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(filas.len());
    for (coleccion, clave, cambiado) in filas {
        let datos = match coleccion.as_str() {
            FAVORITOS => fav
                .query_row(params![clave], |r| {
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
fn marcar_subidos(db: &Connection, subidos: &[Cambio]) -> Result<(), String> {
    let mut st = db
        .prepare("DELETE FROM sync_salida WHERE coleccion = ?1 AND clave = ?2 AND cambiado <= ?3")
        .map_err(|e| e.to_string())?;
    for c in subidos {
        st.execute(params![c.coleccion, c.clave, c.actualizado])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Aplica cambios del servidor. Devuelve cuántos tocaron algo local.
fn aplicar(db: &mut Connection, cambios: &[Cambio]) -> Result<usize, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE sync_estado SET aplicando = 1 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for c in cambios {
        // Un cambio local pendiente y más nuevo gana: se sube en la próxima.
        let local: Option<i64> = tx
            .query_row(
                "SELECT cambiado FROM sync_salida WHERE coleccion = ?1 AND clave = ?2",
                params![c.coleccion, c.clave],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if local.is_some_and(|l| l > c.actualizado) {
            continue;
        }
        let tocadas = match c.coleccion.as_str() {
            FAVORITOS => aplicar_favorito(&tx, c)?,
            _ => 0,
        };
        if local.is_some() {
            tx.execute(
                "DELETE FROM sync_salida WHERE coleccion = ?1 AND clave = ?2",
                params![c.coleccion, c.clave],
            )
            .map_err(|e| e.to_string())?;
        }
        n += tocadas;
    }
    tx.execute("UPDATE sync_estado SET aplicando = 0 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n)
}

fn aplicar_favorito(tx: &rusqlite::Transaction, c: &Cambio) -> Result<usize, String> {
    if c.borrado {
        return tx
            .execute("DELETE FROM favorites WHERE imdb_id = ?1", params![c.clave])
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
            "SELECT 1 FROM favorites WHERE imdb_id = ?1 AND tmdb_id = ?2 AND media_type = ?3
               AND title = ?4 AND poster_path IS ?5 AND added_at = ?6",
            params![c.clave, tmdb_id, media_type, title, s("poster_path"), added_at],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if igual {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO favorites (imdb_id, tmdb_id, media_type, title, poster_path, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(imdb_id) DO UPDATE SET
           tmdb_id = excluded.tmdb_id, media_type = excluded.media_type,
           title = excluded.title, poster_path = excluded.poster_path,
           added_at = excluded.added_at",
        params![c.clave, tmdb_id, media_type, title, s("poster_path"), added_at],
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

async fn sincronizar(dir: &Path) -> Result<ResultadoSync, ErrApi> {
    let mut c = cargar(dir);
    if c.token.is_empty() {
        return Err(ErrApi::Otro("no hay sesión".into()));
    }
    let mut db = abrir_db(dir)?;
    let mut res = ResultadoSync { subidos: 0, bajados: 0 };

    // 1. Subir.
    let pend = pendientes(&db)?;
    for lote in pend.chunks(LOTE) {
        let r = api(
            &c.servidor,
            Some(&c.token),
            reqwest::Method::POST,
            "/sync",
            Some(&json!({ "perfil": c.perfil_id, "cambios": lote })),
        )
        .await?;
        marcar_subidos(&db, lote)?;
        // Lo que el servidor rechazó por tener algo más nuevo: aplicarlo aquí.
        let vigentes: Vec<Cambio> = r["resultados"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| serde_json::from_value(x.get("vigente")?.clone()).ok())
            .collect();
        res.bajados += aplicar(&mut db, &vigentes)?;
        res.subidos += lote.len() - vigentes.len();
    }

    // 2. Bajar lo que cambió en otros equipos.
    loop {
        let r = api(
            &c.servidor,
            Some(&c.token),
            reqwest::Method::GET,
            &format!("/sync?perfil={}&desde={}", c.perfil_id, c.cursor),
            None,
        )
        .await?;
        let cambios: Vec<Cambio> = serde_json::from_value(r["cambios"].clone())
            .map_err(|e| ErrApi::Otro(format!("respuesta inválida: {e}")))?;
        res.bajados += aplicar(&mut db, &cambios)?;
        c.cursor = r["hasta"].as_i64().unwrap_or(c.cursor);
        guardar(dir, &c)?;
        if !r["mas"].as_bool().unwrap_or(false) {
            break;
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
    // Por ahora un solo perfil por cuenta: el primero.
    let perfil = v["perfiles"].get(0).ok_or("la cuenta no tiene perfiles")?;
    let c = Cuenta {
        servidor,
        token,
        usuario: v["usuario"].as_str().unwrap_or_default().to_string(),
        perfil_id: perfil["id"].as_i64().unwrap_or(0),
        perfil_nombre: perfil["nombre"].as_str().unwrap_or_default().to_string(),
        cursor: 0,
        ultima_sync: 0,
    };
    guardar(dir, &c)?;
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
    pub perfil: Option<String>,
    pub ultima_sync: i64,
}

#[tauri::command]
pub fn cuenta_estado(app: tauri::AppHandle) -> Result<EstadoCuenta, String> {
    let c = cargar(&dir_config(&app)?);
    let con_sesion = !c.token.is_empty();
    Ok(EstadoCuenta {
        servidor: c.servidor,
        usuario: con_sesion.then_some(c.usuario),
        perfil: con_sesion.then_some(c.perfil_nombre),
        ultima_sync: c.ultima_sync,
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

// ─── Amigos y recomendaciones ───────────────────────────────────────────────
//
// Pasan tal cual al front lo que responde el servidor: aquí solo se pone el
// token, que el webview nunca ve. Un servidor sin estas rutas (versión 1)
// responde 404 con "no existe ...".

async fn con_sesion(
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

// Prueba de punta a punta contra un servidor real (server/ del repo):
//
//   php -S 127.0.0.1:18790 -t server/public server/public/index.php
//   KUTRAL_TEST_SERVIDOR=http://127.0.0.1:18790 \
//   KUTRAL_TEST_INVITACION=$(php server/admin.php invitar) \
//     cargo test cuenta -- --ignored
//
// Dos carpetas = dos equipos de la misma cuenta. Lo que haría el front se
// simula con SQL directo sobre `favorites`, así los triggers son los reales.
#[cfg(test)]
mod tests {
    use super::*;

    const FAVORITES: &str = "CREATE TABLE favorites (
        imdb_id TEXT PRIMARY KEY,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        added_at INTEGER NOT NULL
    );";

    fn equipo(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kutral-cuenta-{nombre}-{}", ahora_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = abrir_db(&dir).unwrap();
        db.execute_batch(FAVORITES).unwrap();
        db.execute_batch(MIGRACION_SQL).unwrap();
        dir
    }

    fn fav(dir: &Path, clave: &str, title: &str) {
        abrir_db(dir)
            .unwrap()
            .execute(
                "INSERT INTO favorites (imdb_id, tmdb_id, media_type, title, poster_path, added_at)
                 VALUES (?1, 1, 'movie', ?2, NULL, ?3)
                 ON CONFLICT(imdb_id) DO UPDATE SET title = excluded.title, added_at = excluded.added_at",
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

    #[test]
    fn sin_cuenta_no_se_anota_nada() {
        let a = equipo("sincuenta");
        fav(&a, "tt1", "Uno");
        quitar(&a, "tt1");
        assert_eq!(pendientes_de(&a), 0);
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
}
