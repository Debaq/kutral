// anime_web.rs — fuentes web de anime con subtítulo QUEMADO (hardsub) en
// español. Complementan la ruta torrent, que en español latino tiene mala
// cobertura: acá el sub ya viene dentro del video, no hay que buscar nada.
//
// jkanime sirve HLS FIRMADO desde su propio reproductor (m3u8 con token de
// expiración) y mpv lo reproduce directo: sin debrid, sin yt-dlp. Los
// servidores de terceros (ok.ru, mega, streamtape) necesitan un resolvedor
// aparte y quedan fuera por ahora — se descartan en silencio.
//
// Todo el módulo es BEST-EFFORT: si el sitio cambió el HTML o está bloqueado,
// devuelve vacío y el picker sigue mostrando torrents. Nunca es la única vía.
//
// Sale por net::scrape_client (DoH + UA de navegador) para sobrevivir a los
// bloqueos DNS de operador en España/Italia/Portugal/Argentina.

use crate::kodios::{Quality, Source};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Dominios de jkanime, en orden de preferencia. Si el primero cae (o está
/// bloqueado por IP, que DoH no saltea), se prueba el siguiente.
const JK_MIRRORS: &[&str] = &["https://jkanime.net"];

// ---- Utilidades de parseo (sin crate de regex: el HTML es estable y chico) --

/// Normaliza un título para comparar: minúsculas, sin acentos ni puntuación.
/// "Sousou no Frieren 2nd Season" y "sousou-no-frieren-2nd-season" colapsan al
/// mismo valor, que es justo lo que necesita el match contra el slug.
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
            if c.is_ascii_alphanumeric() { Some(c) } else { None }
        })
        .collect()
}

/// Valor de un atributo HTML (`src="..."`) dentro de un fragmento, con las
/// entidades que importan ya decodificadas — las URLs de jkanime traen `&amp;`
/// y sin decodificar el token de firma sale roto.
fn attr(frag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = frag.find(&key)? + key.len();
    let end = frag[start..].find('"')? + start;
    Some(frag[start..end].replace("&amp;", "&"))
}

/// Primera URL absoluta del texto que contenga `needle` (p. ej. ".m3u8").
fn find_url(hay: &str, needle: &str) -> Option<String> {
    for (i, _) in hay.match_indices("http") {
        let rest = &hay[i..];
        let end = rest
            .find(['"', '\'', ' ', '<', '\\', '\n'])
            .unwrap_or(rest.len());
        let url = &rest[..end];
        if url.contains(needle) {
            return Some(url.replace("&amp;", "&"));
        }
    }
    None
}

// ---- jkanime -------------------------------------------------------------

/// Caché título normalizado → slug. La búsqueda cuesta ~25 KB de HTML; sin
/// caché serían dos requests por cada click en un episodio de la misma serie.
fn slug_cache() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Busca el slug de jkanime probando los títulos en orden. IMPORTANTE: el
/// sitio indexa por ROMAJI, no por título en inglés — "Frieren: Beyond
/// Journey's End" no devuelve nada, "Sousou no Frieren" sí. Por eso el llamador
/// debe pasar `original_title` (romaji de AniList) primero.
async fn jk_find_slug(titles: &[String]) -> Option<(String, String)> {
    for t in titles.iter().filter(|t| !t.trim().is_empty()) {
        let key = norm(t);
        if key.is_empty() {
            continue;
        }
        if let Some(slug) = slug_cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some((slug, t.clone()));
        }
        let path = format!("/buscar/{}/", urlencoding::encode(t.trim()));
        let (_, html) = match crate::net::get_first(JK_MIRRORS, &path).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[jkanime] búsqueda '{t}' falló: {e}");
                continue;
            }
        };
        // Resultados: <h5><a  href="https://jkanime.net/{slug}/">{título}</a>
        let mut best: Option<String> = None;
        for (i, _) in html.match_indices("<h5><a") {
            let frag = &html[i..];
            let frag = &frag[..frag.find("</a>").map(|e| e + 4).unwrap_or(frag.len().min(400))];
            let Some(href) = attr(frag, "href") else { continue };
            let Some(slug) = href.trim_end_matches('/').rsplit('/').next() else { continue };
            if slug.is_empty() || href.contains("/buscar/") {
                continue;
            }
            let label = frag.rsplit_once('>').map(|(_, r)| r).unwrap_or("");
            let label = label.trim_end_matches("</a>").trim();
            // Coincidencia exacta gana; si no, el primer resultado (jkanime
            // ordena por relevancia). Comparamos contra el texto Y el slug
            // porque el título mostrado a veces trae sufijos.
            if norm(label) == key || norm(slug) == key {
                best = Some(slug.to_string());
                break;
            }
            if best.is_none() {
                best = Some(slug.to_string());
            }
        }
        if let Some(slug) = best {
            if let Ok(mut c) = slug_cache().lock() {
                c.insert(key, slug.clone());
            }
            return Some((slug, t.clone()));
        }
    }
    None
}

/// URLs de los reproductores incrustados de un episodio, en orden de aparición.
/// El HTML las mete en un array JS:
///   video[0] = '<iframe class="player_conte" src="https://jkanime.net/jkplayer/um?e=…"…>';
async fn jk_episode_embeds(slug: &str, ep: u32) -> Result<Vec<String>, String> {
    let path = format!("/{slug}/{ep}/");
    let (base, html) = crate::net::get_first(JK_MIRRORS, &path).await?;
    let mut out = Vec::new();
    for (i, _) in html.match_indices("video[") {
        let frag = &html[i..];
        // La asignación termina en `';` — cortamos ahí para no leer el iframe
        // siguiente si el HTML viene en una sola línea.
        let frag = &frag[..frag.find("';").map(|e| e + 2).unwrap_or(frag.len().min(1200))];
        let Some(src) = attr(frag, "src") else { continue };
        // El HTML trae `src=` sueltos dentro de strings JS mal cerrados; solo
        // aceptamos lo que de verdad parece URL o ruta absoluta.
        if !(src.starts_with("http") || src.starts_with('/')) || src.contains('\'') {
            continue;
        }
        let url = if src.starts_with("http") { src } else { format!("{base}{src}") };
        if !out.contains(&url) {
            out.push(url);
        }
    }
    if out.is_empty() {
        return Err(format!("jkanime {slug}/{ep}: sin reproductores en el HTML"));
    }
    Ok(out)
}

/// Convierte un embed del reproductor PROPIO de jkanime en su HLS directo.
/// Los de terceros (ok.ru, mega…) devuelven None: necesitan un resolvedor que
/// todavía no existe, y una URL que mpv no pueda abrir es peor que nada.
async fn jk_resolve_embed(embed: &str, referer: &str) -> Option<String> {
    if !embed.contains("/jkplayer/") {
        eprintln!("[jkanime] servidor de terceros sin resolvedor: {embed}");
        return None;
    }
    let html = match crate::net::get_text(embed, Some(referer)).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[jkanime] embed falló: {e}");
            return None;
        }
    };
    find_url(&html, ".m3u8")
}

/// Fuentes de jkanime para un episodio. `titles` debe traer el romaji primero.
/// Devuelve como mucho una fuente: los distintos "servidores" del sitio apuntan
/// al mismo HLS, así que se deduplica por URL.
pub async fn jkanime_search(titles: &[String], episode: Option<u32>) -> Result<Vec<Source>, String> {
    let ep = episode.unwrap_or(1);
    let (slug, matched) = jk_find_slug(titles).await.ok_or("jkanime: sin coincidencia de título")?;
    eprintln!("[jkanime] '{matched}' → slug={slug} ep={ep}");
    let embeds = jk_episode_embeds(&slug, ep).await?;
    let referer = format!("{}/{slug}/{ep}/", JK_MIRRORS[0]);

    let mut urls: Vec<String> = Vec::new();
    for e in &embeds {
        if let Some(u) = jk_resolve_embed(e, &referer).await {
            // Los tokens de firma difieren entre servidores pero el archivo es
            // el mismo: deduplicamos por la ruta, ignorando el query.
            let key = |s: &str| s.split('?').next().unwrap_or(s).to_string();
            if !urls.iter().any(|x| key(x) == key(&u)) {
                urls.push(u);
            }
        }
    }
    if urls.is_empty() {
        return Err(format!("jkanime {slug}/{ep}: ningún servidor resolvió a HLS"));
    }
    Ok(urls
        .into_iter()
        .map(|url| Source {
            source: "jkanime".into(),
            // "Subtitulado" en el título no es decorativo: langScore del picker
            // lo lee para ordenar según el modo doblado/subtitulado del usuario.
            title: format!("{matched} — Episodio {ep} [Subtitulado español]"),
            magnet: None,
            url: Some(url),
            info_hash: None,
            size_bytes: None,
            seeders: None,
            // El HLS es una playlist de medios sin declarar resolución: no
            // inventamos una calidad que no sabemos.
            quality: Quality::Unknown,
            rd_cached: None,
            hardsub: Some("lat".into()),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_colapsa_acentos_y_puntuacion() {
        assert_eq!(norm("Sousou no Frieren 2nd Season"), norm("sousou-no-frieren-2nd-season"));
        assert_eq!(norm("Kimetsu no Yaiba: Yuukaku-hen"), "kimetsunoyaibayuukakuhen");
    }

    #[test]
    fn attr_decodifica_amp() {
        let frag = r#"<iframe src="https://x.tld/um?e=AA&amp;t=BB" width="565">"#;
        assert_eq!(attr(frag, "src").unwrap(), "https://x.tld/um?e=AA&t=BB");
    }

    #[test]
    fn find_url_agarra_el_m3u8_y_no_los_scripts() {
        let html = r#"<script src="https://cdn.tld/hls.min.js"></script>
                      var conf = { url: 'https://nika.tld/abc.m3u8?st=xyz&e=1', p2p: true };"#;
        assert_eq!(find_url(html, ".m3u8").unwrap(), "https://nika.tld/abc.m3u8?st=xyz&e=1");
    }

    #[test]
    fn find_url_devuelve_none_sin_coincidencia() {
        assert!(find_url("<p>sin video</p>", ".m3u8").is_none());
    }

    /// Humo contra el sitio REAL. Ignorado por defecto: depende de la red y de
    /// que jkanime no haya cambiado el HTML. Correr a mano cuando el scraper
    /// deje de traer fuentes:
    ///   cargo test --lib jkanime_e2e -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn jkanime_e2e() {
        let titles = vec!["Sousou no Frieren".to_string()];
        let out = jkanime_search(&titles, Some(1)).await.expect("jkanime sin fuentes");
        assert!(!out.is_empty());
        let url = out[0].url.as_deref().unwrap();
        println!("fuente: {} → {url}", out[0].title);
        assert!(url.contains(".m3u8"), "esperaba HLS, salió {url}");
        assert_eq!(out[0].hardsub.as_deref(), Some("lat"));
    }
}
