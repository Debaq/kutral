// Memoria de la sincronía de subtítulos.
//
// El desfase no es de la película: es de la PAREJA archivo + subtítulo. El
// mismo subtítulo sobre otro release (otro corte, otro logo al inicio) trae
// otro desfase, y otro subtítulo sobre el mismo archivo también. Por eso la
// clave es (qué se ve, de qué fuente, con qué subtítulo).
//
// Sin dependencias de plataforma: el reproductor decide CUÁNDO buscar y
// guardar (al cambiar de pista, al salir); acá vive el QUÉ.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Contexto que manda el frontend al reproducir.
#[derive(Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncCtx {
    /// Qué se ve: "tt123", "tt123:1:2" (capítulo) o "t:<título>" sin id.
    pub media: String,
    /// Serie y temporada ("tt123:1"): habilita heredar del capítulo anterior.
    pub serie: String,
    /// De dónde: infohash, nombre del release o ruta local.
    pub fuente: String,
    /// Nombre del release, para sacar el grupo.
    pub release: String,
    /// "fuente" (exacta), "titulo" (cualquier fuente del mismo título) o "no".
    pub modo: String,
    /// Sin dato exacto, usar el del capítulo anterior de la misma fuente.
    pub capitulo_anterior: bool,
    /// Sincronía por audio (sub_autosync.rs): "boton" (en el panel de
    /// ajuste), "siempre" (al cargar un subtítulo externo sin sincronía
    /// recordada) o "no".
    pub auto: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Entrada {
    media: String,
    #[serde(default)]
    serie: String,
    /// Grupo del release (ver subs_match::grupo): "misma fuente" entre
    /// capítulos aunque cada uno sea otro torrent.
    #[serde(default)]
    grupo: String,
    fuente: String,
    sub: String,
    delay: f64,
}

/// De dónde salió el desfase aplicado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origen {
    /// Este mismo archivo con este mismo subtítulo.
    Exacta,
    /// Mismo título, otra fuente o subtítulo (modo "titulo").
    Titulo,
    /// Capítulo anterior de la misma serie, temporada y grupo.
    CapituloAnterior,
}

/// Cuántos ajustes se recuerdan (los más recientes).
const MAX_ENTRADAS: usize = 500;

static DIR: OnceLock<PathBuf> = OnceLock::new();
/// Serializa lectura+escritura del archivo (se guarda desde varios hilos).
static IO: Mutex<()> = Mutex::new(());

pub fn init(config_dir: PathBuf) {
    // La primera versión recordaba por título en otro archivo: ese criterio
    // aplicaba el desfase de una fuente a otra, así que no se migra.
    let _ = std::fs::remove_file(config_dir.join("sub-delay.json"));
    let _ = DIR.set(config_dir);
}

fn archivo() -> Option<PathBuf> {
    DIR.get().map(|d| d.join("sub-sync.json"))
}

fn leer() -> Vec<Entrada> {
    archivo()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn escribir(lista: &[Entrada]) {
    let Some(f) = archivo() else { return };
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(json) = serde_json::to_string(lista) {
        let _ = std::fs::write(f, json);
    }
}

/// Tipo de subtítulo, para no heredar el desfase de uno embebido en uno
/// externo: "emb:spa", "os", "wy", "ext".
fn tipo_sub(sub: &str) -> &str {
    match sub.split_once('/') {
        Some((t, _)) if t.starts_with("emb:") => t,
        _ => sub.split(':').next().unwrap_or(""),
    }
}

fn grupo_de(ctx: &SyncCtx) -> String {
    crate::subs_match::grupo(&ctx.release).unwrap_or_default()
}

/// Regla de búsqueda sobre una lista (separada de la E/S para los tests).
fn elegir(lista: &[Entrada], ctx: &SyncCtx, sub: &str) -> Option<(f64, Origen)> {
    if ctx.modo == "no" || ctx.media.is_empty() {
        return None;
    }
    // Lo más reciente gana: la lista está ordenada de vieja a nueva.
    let rev = || lista.iter().rev();
    if let Some(e) = rev().find(|e| e.media == ctx.media && e.fuente == ctx.fuente && e.sub == sub) {
        return Some((e.delay, Origen::Exacta));
    }
    if ctx.modo == "titulo" {
        if let Some(e) = rev().find(|e| e.media == ctx.media) {
            return Some((e.delay, Origen::Titulo));
        }
    }
    let grupo = grupo_de(ctx);
    if ctx.capitulo_anterior && !ctx.serie.is_empty() && !grupo.is_empty() {
        let tipo = tipo_sub(sub);
        if let Some(e) = rev().find(|e| {
            e.serie == ctx.serie && e.grupo == grupo && e.media != ctx.media && tipo_sub(&e.sub) == tipo
        }) {
            return Some((e.delay, Origen::CapituloAnterior));
        }
    }
    None
}

/// Desfase recordado para este contexto y subtítulo.
pub fn buscar(ctx: &SyncCtx, sub: &str) -> Option<(f64, Origen)> {
    let _g = IO.lock().ok()?;
    elegir(&leer(), ctx, sub)
}

/// Inserta/actualiza en la lista. `aplicado` = lo que se puso al cargar la
/// pista: si no cambió y no había nada exacto, no se guarda (heredar del
/// capítulo anterior y no tocarlo no es "sincronizar este").
fn anotar(lista: &mut Vec<Entrada>, ctx: &SyncCtx, sub: &str, delay: f64, aplicado: f64) -> bool {
    let delay = (delay * 100.0).round() / 100.0;
    let pos = lista
        .iter()
        .position(|e| e.media == ctx.media && e.fuente == ctx.fuente && e.sub == sub);
    if pos.is_none() && (delay - aplicado).abs() < 0.005 {
        return false;
    }
    if let Some(i) = pos {
        lista.remove(i);
    }
    lista.push(Entrada {
        media: ctx.media.clone(),
        serie: ctx.serie.clone(),
        grupo: grupo_de(ctx),
        fuente: ctx.fuente.clone(),
        sub: sub.to_string(),
        delay,
    });
    let sobra = lista.len().saturating_sub(MAX_ENTRADAS);
    lista.drain(..sobra);
    true
}

/// Guarda el desfase de este subtítulo (ver `anotar`). Con modo "no" no se
/// recuerda nada.
pub fn guardar(ctx: &SyncCtx, sub: &str, delay: f64, aplicado: f64) {
    if ctx.modo == "no" || ctx.media.is_empty() || sub.is_empty() {
        return;
    }
    let Ok(_g) = IO.lock() else { return };
    let mut lista = leer();
    if anotar(&mut lista, ctx, sub, delay, aplicado) {
        escribir(&lista);
    }
}

/// Configuración → "Borrar sincronías guardadas".
#[tauri::command]
pub fn sub_sync_clear() -> Result<(), String> {
    let _g = IO.lock().map_err(|_| "ocupado")?;
    match archivo() {
        Some(f) if f.exists() => std::fs::remove_file(f).map_err(|e| format!("borrar: {e}")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(media: &str, fuente: &str) -> SyncCtx {
        SyncCtx {
            media: media.into(),
            serie: "tt1:1".into(),
            fuente: fuente.into(),
            release: "Show.S01.1080p.WEB-DL-GRP".into(),
            modo: "fuente".into(),
            capitulo_anterior: true,
            auto: "boton".into(),
        }
    }

    fn lista_con(c: &SyncCtx, sub: &str, delay: f64) -> Vec<Entrada> {
        let mut l = Vec::new();
        assert!(anotar(&mut l, c, sub, delay, 0.0));
        l
    }

    #[test]
    fn exacta_gana_y_otra_fuente_no_hereda() {
        let a = ctx("tt1:1:1", "hashA");
        let l = lista_con(&a, "os:1", 1.3);
        assert_eq!(elegir(&l, &a, "os:1"), Some((1.3, Origen::Exacta)));
        // Mismo capítulo, otra fuente: en modo "fuente" no se aplica.
        let b = SyncCtx { release: "Show.S01E01.BluRay-OTRO".into(), ..ctx("tt1:1:1", "hashB") };
        assert_eq!(elegir(&l, &b, "os:1"), None);
        // En modo "titulo", sí.
        let t = SyncCtx { modo: "titulo".into(), ..b };
        assert_eq!(elegir(&l, &t, "os:1"), Some((1.3, Origen::Titulo)));
    }

    #[test]
    fn capitulo_anterior_misma_fuente_y_mismo_tipo_de_sub() {
        let e1 = ctx("tt1:1:1", "hash1");
        let l = lista_con(&e1, "os:10", -2.0);
        // Otro torrent por capítulo, mismo grupo: hereda.
        let e2 = ctx("tt1:1:2", "hash2");
        assert_eq!(elegir(&l, &e2, "os:20"), Some((-2.0, Origen::CapituloAnterior)));
        // Embebido no hereda de externo.
        assert_eq!(elegir(&l, &e2, "emb:spa/x/subrip/3"), None);
        // Apagado en Configuración.
        let sin = SyncCtx { capitulo_anterior: false, ..e2.clone() };
        assert_eq!(elegir(&l, &sin, "os:20"), None);
        // Otro grupo no hereda.
        let otro = SyncCtx { release: "Show.S01E02.WEB-DL-OTRO".into(), ..e2 };
        assert_eq!(elegir(&l, &otro, "os:20"), None);
    }

    #[test]
    fn modo_no_ni_busca_ni_guarda() {
        let c = SyncCtx { modo: "no".into(), ..ctx("tt1:1:1", "h") };
        let l = lista_con(&ctx("tt1:1:1", "h"), "os:1", 1.0);
        assert_eq!(elegir(&l, &c, "os:1"), None);
    }

    #[test]
    fn heredado_sin_tocar_no_se_guarda_pero_corregido_si() {
        let c = ctx("tt1:1:2", "h2");
        let mut l = Vec::new();
        // Se aplicó -2.0 del capítulo anterior y quedó así: nada que guardar.
        assert!(!anotar(&mut l, &c, "os:2", -2.0, -2.0));
        // Lo corrigió a 0: se guarda aunque sea 0, para que la próxima vez
        // gane la exacta y no se vuelva a heredar.
        assert!(anotar(&mut l, &c, "os:2", 0.0, -2.0));
        assert_eq!(elegir(&l, &c, "os:2"), Some((0.0, Origen::Exacta)));
    }

    #[test]
    fn actualizar_mueve_al_final_y_no_duplica() {
        let a = ctx("tt1:1:1", "h");
        let mut l = lista_con(&a, "os:1", 1.0);
        anotar(&mut l, &ctx("tt1:1:5", "h5"), "os:5", 3.0, 0.0);
        anotar(&mut l, &a, "os:1", 1.5, 1.0);
        assert_eq!(l.len(), 2);
        assert_eq!(l.last().map(|e| e.delay), Some(1.5));
    }

    #[test]
    fn tipo_de_sub() {
        assert_eq!(tipo_sub("emb:spa/Latino/subrip/3"), "emb:spa");
        assert_eq!(tipo_sub("os:123"), "os");
        assert_eq!(tipo_sub("wy:https://x/y"), "wy");
        assert_eq!(tipo_sub("ext:archivo.srt"), "ext");
    }
}
