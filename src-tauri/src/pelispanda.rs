// pelispanda.rs — torrents en español (latino y dual) desde pelispanda.org.
//
// El sitio es un WordPress con API JSON propia (wp-json/wpreact/v1): sin
// Cloudflare, sin HTML que raspar. Lo valioso es que cada versión trae el
// IDIOMA bien etiquetado ("Latino/Inglés", "Latino") y la búsqueda devuelve
// el `tmdb_id`, así que emparejar con el título de Kütral es exacto.
//
//   search?query=…        → [{slug, type: pelicula|serie|anime, year, tmdb_id}]
//   movie/{slug}/         → {downloads: [{quality, size, language, subs, download_link}]}
//   serie/{slug}/related/ → {downloads: [… + season, episode]}
//
// Como jkanime, es BEST-EFFORT: si el sitio cae o cambia, devuelve vacío y el
// resto de las fuentes sigue. Los magnets entran al picker como cualquier
// torrent (Real-Debrid, torrent local o descarga).

use crate::kodios::{Quality, Source};
use serde_json::Value;

/// Dominios en orden de preferencia (el sitio cambia de dominio cada tanto).
const MIRRORS: &[&str] = &["https://pelispanda.org"];

/// Minúsculas, sin acentos ni puntuación: "Superman: Hijo rojo" y
/// "superman hijo rojo" colapsan al mismo valor.
fn norm(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let c = match c {
                'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
                'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
                'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
                'ñ' | 'Ñ' => 'n',
                other => other,
            };
            let c = c.to_ascii_lowercase();
            c.is_ascii_alphanumeric().then_some(c)
        })
        .collect()
}

/// El sitio manda números como string ("1061474", "1") y a veces como número.
fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn texto(v: &Value) -> &str {
    v.as_str().unwrap_or("").trim()
}

/// Un resultado de la búsqueda.
#[derive(Debug, Clone, PartialEq)]
struct Ficha {
    slug: String,
    tipo: String,
    title: String,
    year: String,
}

/// Elige la ficha de la búsqueda que corresponde al título pedido.
/// Con `tmdb_id` el emparejamiento es exacto. Sin él (la cola de descargas no
/// lo guarda) solo se acepta una coincidencia de título exacta y ÚNICA: un
/// remake con el mismo nombre no puede colarse.
fn elegir(resultados: &[Value], tmdb_id: Option<u64>, serie: bool, titulos: &[String]) -> Option<Ficha> {
    let tipo_ok = |t: &str| if serie { t == "serie" || t == "anime" } else { t == "pelicula" };
    let ficha = |r: &Value| Ficha {
        slug: texto(&r["slug"]).to_string(),
        tipo: texto(&r["type"]).to_string(),
        title: texto(&r["title"]).to_string(),
        year: texto(&r["year"]).to_string(),
    };
    let candidatos = resultados.iter().filter(|r| tipo_ok(texto(&r["type"])) && !texto(&r["slug"]).is_empty());
    if let Some(id) = tmdb_id {
        return candidatos.map(|r| (r, num(&r["tmdb_id"]))).find(|(_, t)| *t == Some(id)).map(|(r, _)| ficha(r));
    }
    let claves: Vec<String> = titulos.iter().map(|t| norm(t)).filter(|t| !t.is_empty()).collect();
    let iguales: Vec<&Value> = candidatos
        .filter(|r| {
            let (t, o) = (norm(texto(&r["title"])), norm(texto(&r["original_title"])));
            claves.iter().any(|k| *k == t || *k == o)
        })
        .collect();
    match iguales.as_slice() {
        [uno] => Some(ficha(uno)),
        _ => None,
    }
}

/// info_hash en hex minúscula, si el magnet lo trae así (40 hex). El base32
/// no se convierte: el merge por hash solo pierde la deduplicación.
fn hash_de(magnet: &str) -> Option<String> {
    let i = magnet.to_ascii_lowercase().find("btih:")? + 5;
    let h: String = magnet[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

/// "9.94 GB" / "882 MB" → bytes.
fn tamano(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let mult = match it.next()?.to_ascii_uppercase().as_str() {
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((n * mult) as u64)
}

fn calidad(q: &str) -> Quality {
    let q = q.to_ascii_lowercase();
    if q.contains("2160") || q.contains("4k") {
        Quality::P2160
    } else if q.contains("1080") {
        Quality::P1080
    } else if q.contains("720") {
        Quality::P720
    } else if q.contains("cam") || q.contains("ts-screener") {
        Quality::Cam
    } else if q.contains("dvdrip") || q.contains("480") || q.contains("sd") {
        Quality::Sd
    } else {
        Quality::Unknown
    }
}

/// Convierte una versión de `downloads` en fuente. `nombre` es lo que se ve
/// ("Superman (2025)" o "The Last of Us T1E3").
///
/// El título lleva el idioma tal cual lo pone el sitio ("Latino/Inglés"): el
/// picker lo lee (langScore) para ordenar según el idioma del usuario, y
/// "Dual" cuando trae dos audios.
fn fuente(d: &Value, nombre: &str) -> Option<Source> {
    let magnet = texto(&d["download_link"]);
    if !magnet.starts_with("magnet:") {
        return None;
    }
    let q = texto(&d["quality"]);
    let idioma = texto(&d["language"]);
    let dual = if idioma.contains('/') && !q.to_ascii_lowercase().contains("dual") { " Dual" } else { "" };
    let subs = if num(&d["subs"]).unwrap_or(0) > 0 { " +Subs" } else { "" };
    Some(Source {
        source: "pelispanda".into(),
        title: format!("{nombre} [{q}]{dual} [{idioma}]{subs}"),
        magnet: Some(magnet.to_string()),
        url: None,
        info_hash: hash_de(magnet),
        size_bytes: tamano(texto(&d["size"])),
        // El sitio no publica seeders. Son subidas propias del grupo y suelen
        // estar en caché de RD; sin dato, no inventamos uno.
        seeders: None,
        quality: calidad(q),
        rd_cached: None,
        hardsub: None,
        file_idx: None,
        filename: None,
        links: None,
    })
}

/// Fuentes de pelispanda para una película (sin temporada) o un capítulo.
pub async fn pelispanda_search(
    tmdb_id: Option<u64>,
    titulos: &[String],
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    let serie = season.is_some() && episode.is_some();
    let mut elegida: Option<Ficha> = None;
    let mut base = String::new();
    for t in titulos.iter().filter(|t| !t.trim().is_empty()) {
        let path = format!(
            "/wp-json/wpreact/v1/search?query={}&posts_per_page=20&page=1",
            urlencoding::encode(t.trim())
        );
        let (b, cuerpo) = crate::net::get_first(MIRRORS, &path).await?;
        let v: Value = serde_json::from_str(&cuerpo).map_err(|e| format!("pelispanda búsqueda: {e}"))?;
        let resultados = v["results"].as_array().cloned().unwrap_or_default();
        if let Some(f) = elegir(&resultados, tmdb_id, serie, titulos) {
            elegida = Some(f);
            base = b;
            break;
        }
    }
    let f = elegida.ok_or("pelispanda: sin coincidencia")?;
    eprintln!("[pelispanda] {} ({}) → {}/{}", f.title, f.year, f.tipo, f.slug);

    let ruta = match (serie, f.tipo.as_str()) {
        (false, _) => format!("/wp-json/wpreact/v1/movie/{}/", f.slug),
        (true, "anime") => format!("/wp-json/wpreact/v1/anime/{}/related/", f.slug),
        (true, _) => format!("/wp-json/wpreact/v1/serie/{}/related/", f.slug),
    };
    let url = format!("{}{}", base.trim_end_matches('/'), ruta);
    let cuerpo = crate::net::get_text(&url, Some(&base)).await?;
    let v: Value = serde_json::from_str(&cuerpo).map_err(|e| format!("pelispanda ficha: {e}"))?;
    let descargas = v["downloads"].as_array().cloned().unwrap_or_default();

    let out: Vec<Source> = if serie {
        let (s, e) = (season.unwrap_or(0) as u64, episode.unwrap_or(0) as u64);
        let nombre = format!("{} T{s}E{e}", f.title);
        descargas
            .iter()
            .filter(|d| num(&d["season"]) == Some(s) && num(&d["episode"]) == Some(e))
            .filter_map(|d| fuente(d, &nombre))
            .collect()
    } else {
        let nombre = if f.year.is_empty() { f.title.clone() } else { format!("{} ({})", f.title, f.year) };
        descargas.iter().filter_map(|d| fuente(d, &nombre)).collect()
    };
    if out.is_empty() {
        return Err(format!("pelispanda {}: sin magnets para lo pedido", f.slug));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resultados() -> Vec<Value> {
        vec![
            json!({"slug":"superman-2","title":"Superman","original_title":"Superman","year":"2025","tmdb_id":"1061474","type":"pelicula"}),
            json!({"slug":"superman-1978","title":"Superman","original_title":"Superman","year":"1978","tmdb_id":"1924","type":"pelicula"}),
            json!({"slug":"superman-hijo-rojo","title":"Superman: Hijo rojo","original_title":"Superman: Red Son","year":"2020","tmdb_id":618355,"type":"pelicula"}),
            json!({"slug":"the-last-of-us","title":"The Last of Us","original_title":"The Last of Us","year":"2023","tmdb_id":"100088","type":"serie"}),
        ]
    }

    #[test]
    fn elige_por_tmdb_exacto() {
        let r = resultados();
        assert_eq!(elegir(&r, Some(1061474), false, &[]).unwrap().slug, "superman-2");
        assert_eq!(elegir(&r, Some(618355), false, &[]).unwrap().slug, "superman-hijo-rojo");
        // El id de una serie no sirve buscando película.
        assert!(elegir(&r, Some(100088), false, &[]).is_none());
        assert_eq!(elegir(&r, Some(100088), true, &[]).unwrap().slug, "the-last-of-us");
    }

    #[test]
    fn sin_tmdb_solo_titulo_unico() {
        let r = resultados();
        // Dos "Superman": ambiguo, no se arriesga.
        assert!(elegir(&r, None, false, &["Superman".into()]).is_none());
        let f = elegir(&r, None, false, &["Superman: Red Son".into()]).unwrap();
        assert_eq!(f.slug, "superman-hijo-rojo");
        assert_eq!(elegir(&r, None, true, &["the last of us".into()]).unwrap().slug, "the-last-of-us");
    }

    #[test]
    fn fuente_lleva_idioma_calidad_y_hash() {
        let d = json!({"quality":"WEB-DL 1080p","size":"9.94 GB","subs":"1","language":"Latino/Inglés",
            "download_link":"magnet:?xt=urn:btih:0761280FC0AD8139100A3E44A50AF0D4D8BCDAAA&dn=x"});
        let s = fuente(&d, "Superman (2025)").unwrap();
        assert_eq!(s.title, "Superman (2025) [WEB-DL 1080p] Dual [Latino/Inglés] +Subs");
        assert_eq!(s.quality, Quality::P1080);
        assert_eq!(s.size_bytes, Some(9_940_000_000));
        assert_eq!(s.info_hash.as_deref(), Some("0761280fc0ad8139100a3e44a50af0d4d8bcdaaa"));
        // "Dual 720p" ya dice Dual: no se repite.
        let d2 = json!({"quality":"Dual 720p","size":"1.56 GB","subs":"0","language":"Latino/Inglés","download_link":"magnet:?xt=urn:btih:abc"});
        let s2 = fuente(&d2, "X").unwrap();
        assert_eq!(s2.title, "X [Dual 720p] [Latino/Inglés]");
        assert!(s2.info_hash.is_none());
        // Lo que no es magnet no entra.
        assert!(fuente(&json!({"download_link":"https://x"}), "X").is_none());
    }

    #[test]
    fn calidades() {
        assert_eq!(calidad("WEB-DL 4k HDR"), Quality::P2160);
        assert_eq!(calidad("DVDRip"), Quality::Sd);
        assert_eq!(calidad("Dual 720p"), Quality::P720);
        assert_eq!(calidad("HDRip"), Quality::Unknown);
    }

    /// Humo contra el sitio REAL (ignorado: depende de la red):
    ///   cargo test --lib pelispanda_e2e -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn pelispanda_e2e() {
        let peli = pelispanda_search(Some(1061474), &["Superman".into()], None, None).await.unwrap();
        assert!(peli.iter().any(|s| s.title.contains("Latino")));
        let cap = pelispanda_search(Some(100088), &["The Last of Us".into()], Some(1), Some(1)).await.unwrap();
        assert!(!cap.is_empty());
        for s in peli.iter().chain(&cap) {
            println!("{} {:?}", s.title, s.size_bytes);
        }
    }
}
