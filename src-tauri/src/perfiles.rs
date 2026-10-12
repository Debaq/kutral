// Perfiles: varias personas en un mismo equipo, cada una con su historial,
// favoritos y listas. Funcionan sin cuenta; con cuenta, cada perfil se
// sincroniza por separado (ver cuenta.rs).
//
// Lo que es del equipo y no de la persona sigue compartido: descargas,
// Real-Debrid, configuración y lo que aprendió Vera.
//
// El perfil activo lo elige el front (perfiles.svelte.ts) y viaja en cada
// consulta. Aquí solo vive el esquema y lo que conviene hacer en una sola
// transacción, como borrar un perfil con todo lo suyo.

use rusqlite::{params, Connection};
use serde::Serialize;

pub const PRINCIPAL: &str = "principal";

/// Migración v13 de kutral.db (la registra lib.rs). Agrega la columna `perfil`
/// a lo personal; todo lo que había queda en el perfil "principal".
///
/// SQLite no cambia una PK con ALTER: cada tabla se reconstruye y se copia
/// (igual que la v7). Al borrar las tablas viejas caen sus triggers de
/// sincronización, así que se vuelven a crear, ahora con el perfil. Los ids de
/// lista son por perfil: "quiero-ver" existe una vez en cada uno.
pub const MIGRACION_SQL: &str = "
    CREATE TABLE IF NOT EXISTS perfiles (
        id TEXT PRIMARY KEY,
        nombre TEXT NOT NULL,
        color TEXT NOT NULL DEFAULT '#f5c518',
        creado INTEGER NOT NULL,
        actualizado INTEGER NOT NULL
    );
    INSERT OR IGNORE INTO perfiles (id, nombre, color, creado, actualizado)
    VALUES ('principal', 'Principal', '#f5c518',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));

    CREATE TABLE favorites_nuevo (
        perfil TEXT NOT NULL DEFAULT 'principal',
        imdb_id TEXT NOT NULL,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (perfil, imdb_id)
    );
    INSERT INTO favorites_nuevo (perfil, imdb_id, tmdb_id, media_type, title, poster_path, added_at)
        SELECT 'principal', imdb_id, tmdb_id, media_type, title, poster_path, added_at FROM favorites;
    DROP TABLE favorites;
    ALTER TABLE favorites_nuevo RENAME TO favorites;
    CREATE INDEX IF NOT EXISTS idx_fav_added ON favorites(perfil, added_at);

    CREATE TABLE watch_history_nuevo (
        perfil TEXT NOT NULL DEFAULT 'principal',
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
        PRIMARY KEY (perfil, imdb_id, season, episode)
    );
    INSERT INTO watch_history_nuevo
        (perfil, imdb_id, season, episode, tmdb_id, media_type, title, poster_path, still_path,
         episode_title, watched_seconds, runtime_seconds, progress_real, completed, last_watched)
        SELECT 'principal', imdb_id, season, episode, tmdb_id, media_type, title, poster_path, still_path,
               episode_title, watched_seconds, runtime_seconds, progress_real, completed, last_watched
          FROM watch_history;
    DROP TABLE watch_history;
    ALTER TABLE watch_history_nuevo RENAME TO watch_history;
    CREATE INDEX IF NOT EXISTS idx_wh_last ON watch_history(perfil, last_watched);
    CREATE INDEX IF NOT EXISTS idx_wh_imdb ON watch_history(perfil, imdb_id);

    CREATE TABLE listas_nuevo (
        perfil TEXT NOT NULL DEFAULT 'principal',
        id TEXT NOT NULL,
        nombre TEXT NOT NULL,
        creada INTEGER NOT NULL,
        actualizada INTEGER NOT NULL,
        PRIMARY KEY (perfil, id)
    );
    INSERT INTO listas_nuevo (perfil, id, nombre, creada, actualizada)
        SELECT 'principal', id, nombre, creada, actualizada FROM listas;
    DROP TABLE listas;
    ALTER TABLE listas_nuevo RENAME TO listas;

    CREATE TABLE lista_items_nuevo (
        perfil TEXT NOT NULL DEFAULT 'principal',
        lista_id TEXT NOT NULL,
        clave TEXT NOT NULL,
        tmdb_id INTEGER NOT NULL,
        media_type TEXT NOT NULL,
        title TEXT NOT NULL,
        poster_path TEXT,
        posicion REAL NOT NULL,
        agregado INTEGER NOT NULL,
        PRIMARY KEY (perfil, lista_id, clave)
    );
    INSERT INTO lista_items_nuevo
        (perfil, lista_id, clave, tmdb_id, media_type, title, poster_path, posicion, agregado)
        SELECT 'principal', lista_id, clave, tmdb_id, media_type, title, poster_path, posicion, agregado
          FROM lista_items;
    DROP TABLE lista_items;
    ALTER TABLE lista_items_nuevo RENAME TO lista_items;

    CREATE TABLE sync_salida_nuevo (
        perfil TEXT NOT NULL DEFAULT 'principal',
        coleccion TEXT NOT NULL,
        clave TEXT NOT NULL,
        cambiado INTEGER NOT NULL,
        PRIMARY KEY (perfil, coleccion, clave)
    );
    INSERT INTO sync_salida_nuevo (perfil, coleccion, clave, cambiado)
        SELECT 'principal', coleccion, clave, cambiado FROM sync_salida;
    DROP TABLE sync_salida;
    ALTER TABLE sync_salida_nuevo RENAME TO sync_salida;
";

/// Triggers de sincronización con perfil (migración v14).
///
/// UPSERT y no `INSERT OR REPLACE`: cuando la sentencia que dispara el trigger
/// es a su vez un UPSERT (guardarProgreso, alternarFavorito), SQLite cambia el
/// OR REPLACE del trigger por ABORT y, si el cambio anterior todavía no se
/// había subido, la escritura entera fallaba por la clave repetida.
pub const TRIGGERS_SQL: &str = "
    CREATE TRIGGER IF NOT EXISTS sync_fav_ins AFTER INSERT ON favorites
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'favoritos', NEW.imdb_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_fav_upd AFTER UPDATE ON favorites
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'favoritos', NEW.imdb_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_fav_del AFTER DELETE ON favorites
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (OLD.perfil, 'favoritos', OLD.imdb_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;

    CREATE TRIGGER IF NOT EXISTS sync_hist_ins AFTER INSERT ON watch_history
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'historial', NEW.imdb_id || '|' || NEW.season || '|' || NEW.episode,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_hist_upd AFTER UPDATE ON watch_history
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'historial', NEW.imdb_id || '|' || NEW.season || '|' || NEW.episode,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_hist_del AFTER DELETE ON watch_history
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (OLD.perfil, 'historial', OLD.imdb_id || '|' || OLD.season || '|' || OLD.episode,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;

    CREATE TRIGGER IF NOT EXISTS sync_lista_ins AFTER INSERT ON listas
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'listas', NEW.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_lista_upd AFTER UPDATE ON listas
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'listas', NEW.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_lista_del AFTER DELETE ON listas
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (OLD.perfil, 'listas', OLD.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;

    CREATE TRIGGER IF NOT EXISTS sync_item_ins AFTER INSERT ON lista_items
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'lista_items', NEW.lista_id || '|' || NEW.clave,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_item_upd AFTER UPDATE ON lista_items
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (NEW.perfil, 'lista_items', NEW.lista_id || '|' || NEW.clave,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
    CREATE TRIGGER IF NOT EXISTS sync_item_del AFTER DELETE ON lista_items
    WHEN (SELECT activa AND NOT aplicando FROM sync_estado WHERE id = 1)
    BEGIN
        INSERT INTO sync_salida (perfil, coleccion, clave, cambiado)
        VALUES (OLD.perfil, 'lista_items', OLD.lista_id || '|' || OLD.clave,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
        ON CONFLICT(perfil, coleccion, clave) DO UPDATE SET cambiado = excluded.cambiado;
    END;
";

#[derive(Serialize, Debug, Clone)]
pub struct Perfil {
    pub id: String,
    pub nombre: String,
    pub color: String,
    /// ms del último cambio de nombre o color: con cuenta, gana el más nuevo.
    pub actualizado: i64,
}

pub fn listar(db: &Connection) -> Result<Vec<Perfil>, String> {
    let mut st = db
        .prepare("SELECT id, nombre, color, actualizado FROM perfiles ORDER BY creado")
        .map_err(|e| e.to_string())?;
    let filas = st
        .query_map([], |r| {
            Ok(Perfil { id: r.get(0)?, nombre: r.get(1)?, color: r.get(2)?, actualizado: r.get(3)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    filas
}

pub fn actualizar(db: &Connection, id: &str, nombre: &str, color: &str, actualizado: i64) -> Result<(), String> {
    db.execute(
        "UPDATE perfiles SET nombre = ?2, color = ?3, actualizado = ?4 WHERE id = ?1",
        params![id, nombre, color, actualizado],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

// Lo que se escribe aquí viene del servidor o ya se borró allí: con
// `aplicando` los triggers no lo anotan para subir.
fn sin_anotar<T>(
    db: &mut Connection,
    f: impl FnOnce(&rusqlite::Transaction) -> rusqlite::Result<T>,
) -> Result<T, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE sync_estado SET aplicando = 1 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    let r = f(&tx).map_err(|e| e.to_string())?;
    tx.execute("UPDATE sync_estado SET aplicando = 0 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(r)
}

/// Perfil creado en otro equipo. Nace con su "Quiero ver", como todos.
pub fn crear_local(db: &mut Connection, id: &str, nombre: &str, color: &str, actualizado: i64) -> Result<(), String> {
    sin_anotar(db, |tx| {
        tx.execute(
            "INSERT OR IGNORE INTO perfiles (id, nombre, color, creado, actualizado) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, nombre, color, actualizado],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO listas (perfil, id, nombre, creada, actualizada)
             VALUES (?1, 'quiero-ver', 'Quiero ver', ?2, ?2)",
            params![id, actualizado],
        )?;
        Ok(())
    })
}

/// Borra el perfil y todo lo suyo en este equipo.
pub fn borrar_local(db: &mut Connection, id: &str) -> Result<(), String> {
    sin_anotar(db, |tx| {
        for t in ["favorites", "watch_history", "lista_items", "listas", "sync_salida"] {
            tx.execute(&format!("DELETE FROM {t} WHERE perfil = ?1"), params![id])?;
        }
        tx.execute("DELETE FROM perfiles WHERE id = ?1", params![id])?;
        Ok(())
    })
}
