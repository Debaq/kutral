// dominios.rs — dominio vigente de cada sitio web, sin sacar versión nueva.
//
// Los sitios de fuentes cambian de dominio seguido (dontorrent lleva casi 200).
// Los humanos de Balandro (addon de Kodi) los siguen al día; un workflow de
// este repo (fuentes-balandro.yml) lee sus datos una vez al día y publica
// `balandro.json` en la rama `fuentes`. Acá se lee ese JSON y se arma la lista
// de espejos de un canal: el dominio vigente primero, después los fijos del
// código como respaldo, y fuera los que Balandro ya dio por muertos.
//
// Todo es best-effort: sin red o sin el JSON, quedan los dominios fijos.

use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const URL: &str = "https://raw.githubusercontent.com/Debaq/kutral/fuentes/balandro.json";
/// Cada cuánto se vuelve a pedir el JSON con la app abierta.
const VIGENCIA: Duration = Duration::from_secs(6 * 3600);
/// Si falló, no se reintenta en cada búsqueda: un rato de gracia.
const GRACIA_FALLA: Duration = Duration::from_secs(10 * 60);

static CACHE: Mutex<Option<(Instant, Duration, Value)>> = Mutex::new(None);

async fn lista() -> Value {
    if let Ok(c) = CACHE.lock() {
        if let Some((t, vida, v)) = c.as_ref() {
            if t.elapsed() < *vida {
                return v.clone();
            }
        }
    }
    let nuevo = bajar().await;
    let (vida, v) = match nuevo {
        Ok(v) => (VIGENCIA, v),
        Err(e) => {
            eprintln!("[dominios] sin lista de Balandro: {e}");
            (GRACIA_FALLA, Value::Null)
        }
    };
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), vida, v.clone()));
    }
    v
}

async fn bajar() -> Result<Value, String> {
    let cli = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;
    let r = cli.get(URL).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    r.json::<Value>().await.map_err(|e| e.to_string())
}

fn limpio(d: &str) -> String {
    d.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Espejos de `canal`: vigente según Balandro, luego `fijos`, sin muertos ni
/// repetidos. Puro, para poder probarlo sin red.
fn armar(lista: &Value, canal: &str, fijos: &[&str]) -> Vec<String> {
    let c = &lista["canales"][canal];
    let muertos: Vec<String> = c["muertos"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m.as_str()).map(limpio).collect())
        .unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let vigente = c["host"].as_str().map(limpio).filter(|h| h.starts_with("http"));
    for d in vigente.into_iter().chain(fijos.iter().map(|f| limpio(f))) {
        if !muertos.contains(&d) && !out.contains(&d) {
            out.push(d);
        }
    }
    // Si Balandro diera por muertos a todos, mejor probar los fijos igual que
    // quedarse sin nada.
    if out.is_empty() {
        out = fijos.iter().map(|f| limpio(f)).collect();
    }
    out
}

/// Lo que Balandro dice de `canal` (activo, nota, host, idiomas…), o null.
pub async fn canal(canal: &str) -> Value {
    lista().await["canales"][canal].clone()
}

/// Espejos a probar para `canal`, en orden.
pub async fn espejos(canal: &str, fijos: &[&str]) -> Vec<String> {
    armar(&lista().await, canal, fijos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vigente_primero_sin_muertos_ni_repetidos() {
        let l = json!({"canales": {"dontorrents": {
            "host": "https://dontorrent.tips/",
            "muertos": ["https://dontorrent.foo/", "https://DonTorrent.zip/"]
        }}});
        let e = armar(&l, "dontorrents", &["https://dontorrent.foo", "https://dontorrent.tips", "https://x.org/"]);
        assert_eq!(e, ["https://dontorrent.tips", "https://x.org"]);
    }

    #[test]
    fn sin_lista_quedan_los_fijos() {
        assert_eq!(armar(&Value::Null, "pelispanda", &["https://pelispanda.org"]), ["https://pelispanda.org"]);
        let l = json!({"canales": {"pelispanda": {"host": "", "muertos": []}}});
        assert_eq!(armar(&l, "pelispanda", &["https://pelispanda.org/"]), ["https://pelispanda.org"]);
    }

    #[test]
    fn todos_muertos_igual_prueba_los_fijos() {
        let l = json!({"canales": {"a": {"host": "https://a.com", "muertos": ["https://a.com", "https://b.com"]}}});
        assert_eq!(armar(&l, "a", &["https://b.com"]), ["https://b.com"]);
    }
}
