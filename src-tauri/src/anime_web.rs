// anime_web.rs — fuentes web de anime con subtítulo QUEMADO (hardsub) en
// español. Complementan la ruta torrent, que en español latino tiene mala
// cobertura: acá el sub ya viene dentro del video, no hay que buscar nada.
//
// jkanime sirve HLS FIRMADO desde su propio reproductor (m3u8 con token de
// expiración) y mpv lo reproduce directo: sin debrid, sin yt-dlp.
//
// Además cada capítulo lista copias del MISMO archivo en hosters (Mega,
// Mediafire, Mixdrop, VOE…). Real-Debrid las convierte en un mp4 directo con
// /unrestrict/link: sirve de respaldo cuando el HLS falla, se puede adelantar
// y atrasar sin cortes, y el video sale de los servidores de RD (esquiva los
// bloqueos por país). Ver `jk_hosters`.
//
// Todo el módulo es BEST-EFFORT: si el sitio cambió el HTML o está bloqueado,
// devuelve vacío y el picker sigue mostrando torrents. Nunca es la única vía.
//
// Sale por net::scrape_client (DoH + UA de navegador) para sobrevivir a los
// bloqueos DNS de operador en España/Italia/Portugal/Argentina.

use crate::kodios::{Quality, Source};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Dominios de jkanime de respaldo. El vigente llega de la lista de Balandro
/// (dominios.rs); si uno cae (o está bloqueado por IP, que DoH no saltea), se
/// prueba el siguiente.
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
///
/// Err solo si el sitio no respondió a NINGUNA búsqueda (caído o bloqueado):
/// así la salud de la fuente no confunde "no tiene ese anime" con "está rota".
async fn jk_find_slug(titles: &[String]) -> Result<Option<(String, String)>, String> {
    let mut respondio = false;
    let mut ultimo_error = String::new();
    for t in titles.iter().filter(|t| !t.trim().is_empty()) {
        let key = norm(t);
        if key.is_empty() {
            continue;
        }
        if let Some(slug) = slug_cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(Some((slug, t.clone())));
        }
        let path = format!("/buscar/{}/", urlencoding::encode(t.trim()));
        let espejos = crate::dominios::espejos("jkanime", JK_MIRRORS).await;
        let espejos: Vec<&str> = espejos.iter().map(String::as_str).collect();
        let (_, html) = match crate::net::get_first(&espejos, &path).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[jkanime] búsqueda '{t}' falló: {e}");
                ultimo_error = e;
                continue;
            }
        };
        respondio = true;
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
            return Ok(Some((slug, t.clone())));
        }
    }
    if respondio || ultimo_error.is_empty() {
        Ok(None)
    } else {
        Err(ultimo_error)
    }
}

/// URLs de los reproductores incrustados de un episodio, en orden de aparición.
/// El HTML las mete en un array JS:
///   video[0] = '<iframe class="player_conte" src="https://jkanime.net/jkplayer/um?e=…"…>';
fn jk_episode_embeds(base: &str, html: &str) -> Vec<String> {
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
    out
}

// ---- Hosters vía Real-Debrid --------------------------------------------

/// Base64 estándar (con o sin relleno). El `remote` de jkanime viene así y no
/// vale una dependencia nueva por 20 líneas.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let k = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        for i in 0..k {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// Hoster de jkanime → link que Real-Debrid sabe abrir, o None si RD no lo
/// soporta. Probado contra RD real (2026-11-07):
///   - Mega: el embed `/embed/ID#KEY` hay que pasarlo a `/file/ID#KEY`.
///   - Mixdrop: jkanime usa dominios espejo que RD no conoce; el ID es el
///     mismo en mixdrop.ag.
///   - Mediafire, VOE, 1Fichier, Streamtape: tal cual.
///   - Mp4upload ("hoster_unsupported"), Streamwish, Vidhide, Filemoon y
///     Doodstream: RD no los abre.
///
/// El número es la preferencia: primero los que guardan el archivo original.
fn link_para_rd(server: &str, url: &str) -> Option<(u8, String)> {
    let url = url.trim();
    match server.to_ascii_lowercase().as_str() {
        "mega" => {
            let u = if let Some(r) = url.split_once("/embed/").map(|(_, r)| r) {
                format!("https://mega.nz/file/{r}")
            } else if let Some(r) = url.split_once("/embed#!").map(|(_, r)| r) {
                format!("https://mega.nz/#!{r}")
            } else {
                url.to_string()
            };
            Some((0, u))
        }
        "mediafire" => Some((1, url.to_string())),
        "1fichier" => Some((2, url.to_string())),
        "mixdrop" => {
            let id = url.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty())?;
            Some((3, format!("https://mixdrop.ag/f/{id}")))
        }
        "voe" => Some((4, url.to_string())),
        "streamtape" => Some((5, url.to_string())),
        _ => None,
    }
}

/// Copias del capítulo en hosters, agrupadas por idioma (`lang` de jkanime:
/// 1 = japonés con sub español, otro = doblaje). El HTML trae:
///   var servers = [{"remote":"<base64 url>","server":"Mega","lang":1,"size":"52 MB",…}];
/// Devuelve (lang, tamaño en bytes, links ordenados para RD).
fn jk_hosters(html: &str) -> Vec<(u64, Option<u64>, Vec<String>)> {
    let Some(i) = html.find("var servers = ") else { return Vec::new() };
    let rest = &html[i + "var servers = ".len()..];
    let Some(end) = rest.find("];") else { return Vec::new() };
    let Ok(lista) = serde_json::from_str::<Vec<serde_json::Value>>(&rest[..end + 1]) else {
        eprintln!("[jkanime] var servers ilegible");
        return Vec::new();
    };
    // (lang, tamaño, links con su preferencia)
    type Grupo = (u64, Option<u64>, Vec<(u8, String)>);
    let mut grupos: Vec<Grupo> = Vec::new();
    for s in &lista {
        let server = s["server"].as_str().unwrap_or("");
        let Some(url) = s["remote"]
            .as_str()
            .and_then(b64_decode)
            .and_then(|b| String::from_utf8(b).ok())
        else {
            continue;
        };
        let Some(link) = link_para_rd(server, &url) else { continue };
        let lang = s["lang"].as_u64().unwrap_or(1);
        let size = s["size"].as_str().and_then(tamano_bytes);
        match grupos.iter_mut().find(|g| g.0 == lang) {
            Some(g) => {
                if g.1.is_none() {
                    g.1 = size;
                }
                if !g.2.iter().any(|(_, l)| *l == link.1) {
                    g.2.push(link);
                }
            }
            None => grupos.push((lang, size, vec![link])),
        }
    }
    grupos
        .into_iter()
        .map(|(lang, size, mut links)| {
            links.sort_by_key(|(p, _)| *p);
            (lang, size, links.into_iter().map(|(_, l)| l).collect())
        })
        .collect()
}

/// "52 MB" / "1.2 GB" → bytes.
fn tamano_bytes(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let mult = match it.next()?.to_ascii_uppercase().as_str() {
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        _ => return None,
    };
    Some((n * mult) as u64)
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
/// Hasta dos tipos: el HLS propio (los distintos "servidores" del sitio
/// apuntan al mismo HLS, así que se deduplica por URL) y una fuente por idioma
/// con las copias en hosters para Real-Debrid. Basta con que haya una.
pub async fn jkanime_search(titles: &[String], episode: Option<u32>) -> Result<Vec<Source>, String> {
    let ep = episode.unwrap_or(1);
    let (slug, matched) = jk_find_slug(titles)
        .await?
        .ok_or(format!("{}jkanime sin coincidencia de título", crate::kodios::VACIO))?;
    eprintln!("[jkanime] '{matched}' → slug={slug} ep={ep}");
    let espejos = crate::dominios::espejos("jkanime", JK_MIRRORS).await;
    let espejos: Vec<&str> = espejos.iter().map(String::as_str).collect();
    let (base, html) = crate::net::get_first(&espejos, &format!("/{slug}/{ep}/")).await?;
    let embeds = jk_episode_embeds(&base, &html);
    let referer = format!("{base}/{slug}/{ep}/");

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
    let hosters = jk_hosters(&html);
    if urls.is_empty() && hosters.is_empty() {
        return Err(format!("{}jkanime {slug}/{ep}: ni HLS ni hosters que sirvan", crate::kodios::VACIO));
    }
    let base_src = |title: String| Source {
        source: "jkanime".into(),
        title,
        magnet: None,
        url: None,
        info_hash: None,
        size_bytes: None,
        seeders: None,
        // Ni el HLS ni el sitio declaran resolución: no inventamos una calidad.
        quality: Quality::Unknown,
        rd_cached: None,
        hardsub: Some("lat".into()),
        file_idx: None,
        filename: None,
        links: None,
    };
    // "Subtitulado"/"Latino" en el título no es decorativo: langScore del
    // picker lo lee para ordenar según el modo doblado/subtitulado del usuario.
    let mut out: Vec<Source> = urls
        .into_iter()
        .map(|url| Source {
            url: Some(url),
            ..base_src(format!("{matched} — Episodio {ep} [Subtitulado español]"))
        })
        .collect();
    for (lang, size, links) in hosters {
        eprintln!("[jkanime] hosters lang={lang}: {}", links.len());
        let etiqueta = if lang == 1 { "Subtitulado español" } else { "Latino" };
        out.push(Source {
            source: "jkanime-rd".into(),
            size_bytes: size,
            links: Some(links),
            // El doblaje no trae sub quemado; el subtitulado sí.
            hardsub: if lang == 1 { Some("lat".into()) } else { None },
            ..base_src(format!("{matched} — Episodio {ep} [{etiqueta}] mp4"))
        });
    }
    Ok(out)
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
    fn b64_decodifica_remote_de_jkanime() {
        let b = b64_decode("aHR0cHM6Ly9tZWRpYWZpcmUuY29tL2ZpbGUvam9uYTJwaXJsNjd4cDRsLwo=").unwrap();
        assert_eq!(String::from_utf8(b).unwrap(), "https://mediafire.com/file/jona2pirl67xp4l/\n");
        assert_eq!(b64_decode("YQ").unwrap(), b"a");
        assert!(b64_decode("a$b").is_none());
    }

    #[test]
    fn link_para_rd_reescribe_mega_y_mixdrop() {
        assert_eq!(
            link_para_rd("Mega", "https://mega.nz/embed/I2sn2TTS#Yyt").unwrap().1,
            "https://mega.nz/file/I2sn2TTS#Yyt"
        );
        assert_eq!(
            link_para_rd("Mixdrop", "https://mdbekjwqa.pw/e/6qxol61li9o1xe").unwrap().1,
            "https://mixdrop.ag/f/6qxol61li9o1xe"
        );
        assert!(link_para_rd("Mp4upload", "https://www.mp4upload.com/embed-x.html").is_none());
        assert!(link_para_rd("Doodstream", "https://d-s.io/e/x").is_none());
    }

    #[test]
    fn jk_hosters_agrupa_por_idioma_y_ordena() {
        // Mega = https://mega.nz/embed/A#kk ; Voe = https://voe.sx/e/z ; Mp4upload no sirve
        let html = r#"bgservers();  var servers = [
            {"remote":"aHR0cHM6Ly92b2Uuc3gvZS96","slug":"s1","server":"VOE","lang":1,"size":"52 MB","append":0},
            {"remote":"aHR0cHM6Ly9tZWdhLm56L2VtYmVkL0Eja2s=","slug":"s2","server":"Mega","lang":1,"size":"52 MB","append":0},
            {"remote":"aHR0cHM6Ly93d3cubXA0dXBsb2FkLmNvbS9lbWJlZC14Lmh0bWw=","slug":"s3","server":"Mp4upload","lang":1,"size":"52 MB","append":0}
        ];  var remote = 'x';"#;
        let g = jk_hosters(html);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].0, 1);
        assert_eq!(g[0].1, Some(52_000_000));
        assert_eq!(g[0].2, ["https://mega.nz/file/A#kk", "https://voe.sx/e/z"]);
        assert!(jk_hosters("<p>sin lista</p>").is_empty());
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
