// Bóveda de la cuenta: lo tuyo que no es historial ni listas. Claves de API
// (TMDb, OMDb, Wyzie), ajustes del front y las vinculaciones de Real-Debrid y
// OpenSubtitles. Así, entrar con la cuenta en un Kutral recién instalado lo
// deja igual que el de siempre.
//
// Viaja cifrada: la llave sale de la contraseña (Argon2id) al entrar, se
// guarda en cuenta.json (0600) y el servidor solo ve un texto opaco. Cambiar
// la contraseña cierra todas las sesiones y borra la bóveda del servidor; el
// primer equipo que vuelve a entrar la sube con la llave nueva.
//
// Aquí está lo que no toca la red (cifrar, mezclar, archivos); la ida y
// vuelta al servidor vive en cuenta.rs.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

pub type Datos = Map<String, Value>;

/// Vinculaciones que viven en archivos 0600 del backend: nombre en la bóveda
/// y archivo en la carpeta de config (los de creds.rs y opensubtitles.rs).
pub const CREDENCIALES: [(&str, &str); 2] = [("rd", "rd_creds.json"), ("os", "os_creds.json")];

const SOBRE: &str = "1:";

/// Llave de 32 bytes (hex) para el usuario. La sal sale del nombre: así todos
/// los equipos llegan a la misma llave sin guardar nada en el servidor.
pub fn derivar_llave(usuario: &str, clave: &str) -> Result<String, String> {
    let sal = Sha256::digest(format!("kutral-boveda:{}", usuario.trim().to_lowercase()));
    // Parámetros mínimos que recomienda OWASP para Argon2id: corre una vez
    // por inicio de sesión.
    let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|e| e.to_string())?;
    let mut llave = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(clave.as_bytes(), &sal[..16], &mut llave)
        .map_err(|e| format!("argon2: {e}"))?;
    Ok(hex::encode(llave))
}

fn cifrador(llave_hex: &str) -> Option<ChaCha20Poly1305> {
    let llave = hex::decode(llave_hex).ok().filter(|l| l.len() == 32)?;
    Some(ChaCha20Poly1305::new(Key::from_slice(&llave)))
}

pub fn cifrar(llave_hex: &str, datos: &Datos) -> Result<String, String> {
    let c = cifrador(llave_hex).ok_or("llave de la bóveda inválida")?;
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("getrandom: {e}"))?;
    let claro = serde_json::to_vec(datos).map_err(|e| e.to_string())?;
    let cifrado = c
        .encrypt(Nonce::from_slice(&nonce), claro.as_slice())
        .map_err(|_| "no se pudo cifrar la bóveda".to_string())?;
    Ok(format!("{SOBRE}{}{}", hex::encode(nonce), hex::encode(cifrado)))
}

/// None si no es de esta llave (o viene dañada).
pub fn descifrar(llave_hex: &str, sobre: &str) -> Option<Datos> {
    let crudo = hex::decode(sobre.strip_prefix(SOBRE)?).ok()?;
    if crudo.len() < 12 {
        return None;
    }
    let (nonce, cifrado) = crudo.split_at(12);
    let claro = cifrador(llave_hex)?.decrypt(Nonce::from_slice(nonce), cifrado).ok()?;
    serde_json::from_slice(&claro).ok()
}

fn igual(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.unwrap_or(&Value::Null) == b.unwrap_or(&Value::Null)
}

fn vacio(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

/// Mezcla campo por campo. `base` es lo que quedó acordado en la última
/// sincronización de este equipo (None = nunca se sincronizó aquí):
/// - lo que cambió aquí desde `base` se queda;
/// - lo demás toma lo del servidor;
/// - la primera vez manda la cuenta, salvo donde ella no tiene nada;
/// - una vinculación que funciona aquí nunca se cambia por la de otro equipo:
///   el servidor solo llena el hueco.
pub fn mezclar(local: &Datos, base: Option<&Datos>, remoto: &Datos) -> Datos {
    let claves: BTreeSet<&String> = local.keys().chain(remoto.keys()).collect();
    let mut out = Datos::new();
    for k in claves {
        let (l, r) = (local.get(k), remoto.get(k));
        let cambio_local = base.is_some_and(|b| !igual(l, b.get(k)));
        let es_credencial = CREDENCIALES.iter().any(|(n, _)| n == k);
        let v = if cambio_local {
            l
        } else if es_credencial || base.is_none() {
            if vacio(l) { r } else if es_credencial || vacio(r) { l } else { r }
        } else {
            r
        };
        if let Some(v) = v.filter(|v| !v.is_null()) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

fn credencial_valida(v: &Value) -> bool {
    ["access_token", "refresh_token", "token"]
        .iter()
        .any(|k| v.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
}

/// Las vinculaciones de este equipo, tal cual están en sus archivos.
pub fn leer_credenciales(dir: &Path) -> Datos {
    let mut d = Datos::new();
    for (nombre, archivo) in CREDENCIALES {
        let v = std::fs::read_to_string(dir.join(archivo))
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .filter(credencial_valida);
        if let Some(v) = v {
            d.insert(nombre.into(), v);
        }
    }
    d
}

/// Escribe las vinculaciones que llegaron del servidor. true si hubo alguna.
pub fn escribir_credenciales(dir: &Path, local: &Datos, mezcla: &Datos) -> Result<bool, String> {
    let mut alguna = false;
    for (nombre, archivo) in CREDENCIALES {
        let Some(v) = mezcla.get(nombre).filter(|v| credencial_valida(v)) else { continue };
        if igual(local.get(nombre), Some(v)) {
            continue;
        }
        escribir_privado(&dir.join(archivo), &serde_json::to_string(v).map_err(|e| e.to_string())?)?;
        alguna = true;
    }
    Ok(alguna)
}

fn escribir_privado(path: &Path, contenido: &str) -> Result<(), String> {
    std::fs::write(path, contenido).map_err(|e| format!("write: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Hasta dónde llegó este equipo: versión del servidor y lo acordado ahí.
/// Lleva claves en claro, por eso va en un archivo 0600 como cuenta.json.
#[derive(Serialize, Deserialize, Default)]
pub struct Estado {
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub base: Option<Datos>,
}

const ESTADO: &str = "boveda.json";

pub fn cargar_estado(dir: &Path) -> Estado {
    std::fs::read_to_string(dir.join(ESTADO))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn guardar_estado(dir: &Path, e: &Estado) -> Result<(), String> {
    escribir_privado(&dir.join(ESTADO), &serde_json::to_string(e).map_err(|e| e.to_string())?)
}

pub fn borrar_estado(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(ESTADO));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(v: Value) -> Datos {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn cifra_y_descifra_solo_con_la_misma_clave() {
        let llave = derivar_llave("Ana", "contraseña-larga").unwrap();
        assert_eq!(llave, derivar_llave("ana", "contraseña-larga").unwrap());
        let datos = d(json!({ "tmdb_key": "abc", "rd": { "refresh_token": "r" } }));
        let sobre = cifrar(&llave, &datos).unwrap();
        assert!(!sobre.contains("abc"));
        assert_eq!(descifrar(&llave, &sobre), Some(datos));
        let otra = derivar_llave("ana", "otra-contraseña").unwrap();
        assert_eq!(descifrar(&otra, &sobre), None);
        assert_eq!(descifrar(&llave, "basura"), None);
    }

    #[test]
    fn primera_vez_manda_la_cuenta() {
        // Equipo nuevo: defaults locales, la cuenta con todo.
        let local = d(json!({ "tmdb_key": "", "subs_lang": "es", "omdb_key": null, "propia": "x" }));
        let remoto = d(json!({ "tmdb_key": "T", "subs_lang": "en", "rd": { "refresh_token": "R" } }));
        let m = mezclar(&local, None, &remoto);
        assert_eq!(
            Value::Object(m),
            json!({ "tmdb_key": "T", "subs_lang": "en", "propia": "x", "rd": { "refresh_token": "R" } })
        );
    }

    #[test]
    fn lo_cambiado_aqui_gana_y_lo_demas_baja() {
        let base = d(json!({ "subs_lang": "es", "tmdb_key": "T" }));
        let local = d(json!({ "subs_lang": "fr", "tmdb_key": "T" }));
        let remoto = d(json!({ "subs_lang": "en", "tmdb_key": "T2" }));
        let m = mezclar(&local, Some(&base), &remoto);
        assert_eq!(Value::Object(m), json!({ "subs_lang": "fr", "tmdb_key": "T2" }));
    }

    #[test]
    fn la_vinculacion_de_aqui_no_se_reemplaza() {
        let rd_a = json!({ "refresh_token": "A" });
        let rd_b = json!({ "refresh_token": "B" });
        let base = d(json!({ "rd": rd_a }));
        let local = d(json!({ "rd": rd_a }));
        let remoto = d(json!({ "rd": rd_b }));
        assert_eq!(mezclar(&local, Some(&base), &remoto)["rd"], rd_a);
        // Sin vinculación aquí, llega la del servidor.
        assert_eq!(mezclar(&Datos::new(), Some(&Datos::new()), &remoto)["rd"], rd_b);
        assert_eq!(mezclar(&Datos::new(), None, &remoto)["rd"], rd_b);
        // Pero si aquí se desvinculó a propósito, no vuelve.
        assert!(!mezclar(&Datos::new(), Some(&base), &remoto).contains_key("rd"));
    }

    #[test]
    fn credenciales_ida_y_vuelta_por_archivo() {
        let dir = std::env::temp_dir().join(format!("kutral-boveda-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rd_creds.json"), r#"{"access_token":"","refresh_token":""}"#).unwrap();
        // Una vinculación vacía cuenta como ninguna.
        assert!(leer_credenciales(&dir).is_empty());
        let mezcla = d(json!({ "rd": { "access_token": "a", "refresh_token": "r" } }));
        assert!(escribir_credenciales(&dir, &Datos::new(), &mezcla).unwrap());
        let leidas = leer_credenciales(&dir);
        assert_eq!(leidas["rd"]["refresh_token"], "r");
        assert!(!escribir_credenciales(&dir, &leidas, &mezcla).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
