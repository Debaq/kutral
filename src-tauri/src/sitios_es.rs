// sitios_es.rs — torrents en español desde sitios sin API: elitetorrent
// (latino y castellano) y grantorrent (castellano, catálogo reciente).
//
// No traen IMDb ni TMDb: se empareja por título normalizado y, en películas,
// por el AÑO de la ficha (hay varias "Superman"). Sin año pedido, solo se
// acepta una coincidencia única. Los dominios vigentes salen de Balandro
// (dominios.rs); el mapa de cada sitio se sacó leyendo cómo lo resuelve
// Balandro, sin copiar su código.
//
// Los links de descarga pasan por acortadores con anuncios, pero el destino
// va ofuscado en la propia página y se decodifica sin visitarlos:
//   - elitetorrent: `s.php?i=` → base64 en capas → rot13 → magnet.
//   - grantorrent:  `data-src` (base64) → ruta del .torrent, que se baja del
//     dominio vigente para calcular el info_hash (SHA-1 del dict `info`).
//
// Best-effort como todo: si el sitio cambia, devuelve vacío o falla y la
// salud de la fuente lo anota.

use crate::kodios::{Quality, Source, VACIO};
use sha1::{Digest, Sha1};

// ---- Utilidades ------------------------------------------------------------

/// Minúsculas, sin acentos ni puntuación.
pub(crate) fn norm(s: &str) -> String {
    tokens(s).join("")
}

fn sin_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
        'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
        'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
        'ñ' | 'Ñ' => 'n',
        other => other,
    }
}

/// Palabras normalizadas ("Superman: Hijo rojo" → [superman, hijo, rojo]).
fn tokens(s: &str) -> Vec<String> {
    s.chars()
        .map(|c| sin_acento(c).to_ascii_lowercase())
        .collect::<String>()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Palabras de calidad/idioma que los sitios pegan al título de la ficha
/// ("Superman Hijo Rojo 1080p Castellano"): no son parte del nombre.
const RELLENO: &[&str] = &[
    "1080p", "720p", "480p", "2160p", "4k", "uhd", "hdr", "hd", "sd", "castellano", "latino", "espanol",
    "subtitulado", "subtitulada", "vose", "dual", "microhd", "bdrip", "bluray", "webdl", "web", "dl", "hdtv",
    "ts", "hq", "cam", "screener", "full", "3d", "torrent", "bittorrent", "descargar",
];

/// Título del sitio sin el relleno final ni un año suelto al final.
pub(crate) fn titulo_limpio(s: &str) -> String {
    let mut t = tokens(s);
    while let Some(u) = t.last() {
        let anio = u.len() == 4 && u.chars().all(|c| c.is_ascii_digit()) && t.len() > 1;
        if RELLENO.contains(&u.as_str()) || anio {
            t.pop();
        } else {
            break;
        }
    }
    t.join("")
}

/// ¿El título del sitio es alguno de los que buscamos?
pub(crate) fn coincide(del_sitio: &str, titulos: &[String]) -> bool {
    let a = titulo_limpio(del_sitio);
    !a.is_empty() && titulos.iter().any(|t| norm(t) == a)
}

/// Entidades HTML que aparecen en estos títulos.
pub(crate) fn des_html(s: &str) -> String {
    s.replace("&#215;", "×")
        .replace("&times;", "×")
        .replace("&amp;", "&")
        .replace("&#8211;", "-")
        .replace("&#8217;", "'")
        .replace("&#039;", "'")
        .replace("&quot;", "\"")
        .replace("&ntilde;", "ñ")
        .replace("&nbsp;", " ")
        .replace("&#038;", "&")
}

/// Texto sin etiquetas, con espacios colapsados.
pub(crate) fn sin_tags(s: &str) -> String {
    let mut out = String::new();
    let mut dentro = false;
    for c in s.chars() {
        match c {
            '<' => dentro = true,
            '>' => {
                dentro = false;
                out.push(' ');
            }
            _ if !dentro => out.push(c),
            _ => {}
        }
    }
    des_html(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Temporada 2×1" / "2x01" al final de un texto → (2, 1).
pub(crate) fn sxe(s: &str) -> Option<(u32, u32)> {
    let t = des_html(s).replace('×', "x").to_ascii_lowercase();
    let b = t.as_bytes();
    // Busca desde el final el último "N x M".
    let mut i = b.len();
    while i > 0 {
        i -= 1;
        if b[i] != b'x' {
            continue;
        }
        let izq: String = t[..i].trim_end().chars().rev().take_while(|c| c.is_ascii_digit()).collect();
        let der: String = t[i + 1..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        if !izq.is_empty() && !der.is_empty() {
            let izq: String = izq.chars().rev().collect();
            return Some((izq.parse().ok()?, der.parse().ok()?));
        }
    }
    None
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='m' | 'A'..='M' => ((c as u8) + 13) as char,
            'n'..='z' | 'N'..='Z' => ((c as u8) - 13) as char,
            _ => c,
        })
        .collect()
}

fn b64(s: &str) -> Option<Vec<u8>> {
    // Solo el prefijo válido, y sin un último carácter suelto (len % 4 == 1).
    let limpio: String = s.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_')).collect();
    let sin_pad = limpio.trim_end_matches('=');
    let corte = if sin_pad.len() % 4 == 1 { sin_pad.len() - 1 } else { sin_pad.len() };
    crate::anime_web::b64_decode(&sin_pad[..corte])
}

/// Destino de un link `s.php?i=…` de elitetorrent: capas de base64 hasta que
/// sale rot13 de "http" o "magn".
fn destino_acortador(i: &str) -> Option<String> {
    let mut actual = i.to_string();
    for _ in 0..8 {
        let bytes = b64(&actual)?;
        let txt = String::from_utf8_lossy(&bytes).to_string();
        let ascii: String = txt.chars().take_while(|c| c.is_ascii() && !c.is_ascii_control()).collect();
        if ascii.starts_with("uggc") || ascii.starts_with("znta") {
            return Some(rot13(&ascii));
        }
        if ascii.starts_with("http") || ascii.starts_with("magnet:") {
            return Some(ascii);
        }
        actual = ascii;
    }
    None
}

/// Hasta `n` caracteres de `s` desde el byte `i` (sin cortar una letra a la mitad).
pub(crate) fn trozo(s: &str, i: usize, n: usize) -> &str {
    let r = s.get(i..).unwrap_or("");
    let fin = r.char_indices().nth(n).map(|(j, _)| j).unwrap_or(r.len());
    &r[..fin]
}

/// Texto del campo que sigue a `etiqueta` (`<b>Idioma:</b> Español Latino`),
/// saltando las etiquetas que haya entre medio.
fn campo(html: &str, etiqueta: &str) -> Option<String> {
    let i = html.find(etiqueta)? + etiqueta.len();
    let mut resto = trozo(html, i, 300);
    loop {
        resto = resto.trim_start();
        if resto.starts_with('<') {
            resto = &resto[resto.find('>')? + 1..];
        } else {
            break;
        }
    }
    let fin = resto.find('<').unwrap_or(resto.len());
    let v = des_html(resto[..fin].trim());
    (!v.is_empty()).then_some(v)
}

/// Primer año de 4 cifras (1900-2099) del texto.
pub(crate) fn anio_de(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3)).find_map(|i| {
        let w = &b[i..i + 4];
        let borde_izq = i == 0 || !b[i - 1].is_ascii_digit();
        let borde_der = i + 4 >= b.len() || !b[i + 4].is_ascii_digit();
        if !(w.iter().all(u8::is_ascii_digit) && borde_izq && borde_der) {
            return None;
        }
        let y: u32 = std::str::from_utf8(w).ok()?.parse().ok()?;
        (1900..2100).contains(&y).then_some(y)
    })
}

pub(crate) fn anio_ok(pedido: Option<u32>, del_sitio: Option<u32>) -> bool {
    match (pedido, del_sitio) {
        (Some(a), Some(b)) => a.abs_diff(b) <= 1,
        _ => true,
    }
}

/// "3.19GB" / "1.95GB." / "882 MB" → bytes.
fn tamano(s: &str) -> Option<u64> {
    let t = s.to_ascii_uppercase().replace(',', ".");
    let num: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f64 = num.parse().ok()?;
    let mult = if t.contains("TB") {
        1e12
    } else if t.contains("GB") {
        1e9
    } else if t.contains("MB") {
        1e6
    } else {
        return None;
    };
    Some((n * mult) as u64)
}

pub(crate) fn calidad(q: &str) -> Quality {
    let q = q.to_ascii_lowercase();
    if q.contains("2160") || q.contains("4k") {
        Quality::P2160
    } else if q.contains("1080") {
        Quality::P1080
    } else if q.contains("720") {
        Quality::P720
    } else if q.contains("ts") || q.contains("cam") || q.contains("screener") {
        Quality::Cam
    } else if q.contains("dvd") || q.contains("hdtv") || q.contains("480") || q.contains("sd") {
        Quality::Sd
    } else {
        Quality::Unknown
    }
}

/// Idioma tal como lo lee el picker (langScore): Latino, Castellano, VOSE.
pub(crate) fn idioma(s: &str) -> String {
    let t = s.to_lowercase();
    if t.contains("latino") {
        "Latino".into()
    } else if t.contains("subtitul") || t.contains("vose") {
        "VOSE".into()
    } else if t.contains("castellano") || t.contains("español") || t.contains("espanol") {
        "Castellano".into()
    } else if t.contains("ingl") {
        "Inglés".into()
    } else {
        s.trim().to_string()
    }
}

/// Trackers vivos: los que traen estos .torrent (publicbt) murieron hace años.
const TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
];

fn magnet(hash: &str, nombre: &str) -> String {
    let mut m = format!("magnet:?xt=urn:btih:{hash}&dn={}", urlencoding::encode(nombre));
    for t in TRACKERS {
        m.push_str("&tr=");
        m.push_str(&urlencoding::encode(t));
    }
    m
}

fn hash_de_magnet(m: &str) -> Option<String> {
    let i = m.to_ascii_lowercase().find("btih:")? + 5;
    let h: String = m[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

// ---- Bencode: info_hash, nombre y tamaño de un .torrent --------------------

/// Fin (exclusivo) del valor bencode que empieza en `i`.
fn fin_valor(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'i' => Some(i + b[i..].iter().position(|&c| c == b'e')? + 1),
        b'l' | b'd' => {
            let mut j = i + 1;
            while *b.get(j)? != b'e' {
                j = fin_valor(b, j)?;
            }
            Some(j + 1)
        }
        b'0'..=b'9' => {
            let dos = i + b[i..].iter().position(|&c| c == b':')?;
            let n: usize = std::str::from_utf8(&b[i..dos]).ok()?.parse().ok()?;
            let fin = dos + 1 + n;
            (fin <= b.len()).then_some(fin)
        }
        _ => None,
    }
}

/// Recorre un dict bencode desde `i` y devuelve (clave, inicio, fin) de cada valor.
fn dict(b: &[u8], i: usize) -> Option<Vec<(Vec<u8>, usize, usize)>> {
    if *b.get(i)? != b'd' {
        return None;
    }
    let mut out = Vec::new();
    let mut j = i + 1;
    while *b.get(j)? != b'e' {
        let fin_k = fin_valor(b, j)?;
        let dos = j + b[j..].iter().position(|&c| c == b':')?;
        let clave = b[dos + 1..fin_k].to_vec();
        let fin_v = fin_valor(b, fin_k)?;
        out.push((clave, fin_k, fin_v));
        j = fin_v;
    }
    Some(out)
}

fn texto_ben(b: &[u8], i: usize, f: usize) -> Option<String> {
    let dos = i + b[i..f].iter().position(|&c| c == b':')?;
    Some(String::from_utf8_lossy(&b[dos + 1..f]).to_string())
}

fn entero_ben(b: &[u8], i: usize, f: usize) -> Option<u64> {
    std::str::from_utf8(b.get(i + 1..f - 1)?).ok()?.parse().ok()
}

/// (info_hash hex, nombre, tamaño total) de un .torrent.
fn info_torrent(b: &[u8]) -> Option<(String, String, u64)> {
    let raiz = dict(b, 0)?;
    let (_, ii, fi) = raiz.into_iter().find(|(k, _, _)| k == b"info")?;
    let hash = hex::encode(Sha1::digest(&b[ii..fi]));
    let info = dict(b, ii)?;
    let mut nombre = String::new();
    let mut tam = 0u64;
    for (k, i, f) in &info {
        match k.as_slice() {
            b"name" => nombre = texto_ben(b, *i, *f).unwrap_or_default(),
            b"length" => tam = entero_ben(b, *i, *f).unwrap_or(0),
            b"files" => {
                // Lista de dicts con "length": se suma todo.
                let mut j = i + 1;
                while b.get(j).is_some_and(|&c| c != b'e') {
                    let f2 = fin_valor(b, j)?;
                    if let Some(d) = dict(b, j) {
                        for (k2, i2, e2) in d {
                            if k2 == b"length" {
                                tam += entero_ben(b, i2, e2).unwrap_or(0);
                            }
                        }
                    }
                    j = f2;
                }
            }
            _ => {}
        }
    }
    Some((hash, nombre, tam))
}

async fn bajar_bytes(url: &str, referer: &str) -> Result<Vec<u8>, String> {
    let cli = crate::net::scrape_client()?;
    let r = cli.get(url).header("Referer", referer).send().await.map_err(|e| format!("GET {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("GET {url}: {}", r.status()));
    }
    r.bytes().await.map(|b| b.to_vec()).map_err(|e| format!("body {url}: {e}"))
}

pub(crate) async fn espejos(canal: &str, fijos: &[&str]) -> Vec<String> {
    crate::dominios::espejos(canal, fijos).await
}

pub(crate) async fn pagina(espejos: &[String], path: &str) -> Result<(String, String), String> {
    let refs: Vec<&str> = espejos.iter().map(String::as_str).collect();
    crate::net::get_first(&refs, path).await
}

fn fuente(sitio: &str, nombre: &str, q: &str, idioma: &str, magnet_: String, size: Option<u64>) -> Source {
    let size = size.or_else(|| {
        // El magnet de elitetorrent no trae tamaño; `xl=` cuando lo hay.
        let i = magnet_.find("xl=")? + 3;
        magnet_[i..].split('&').next()?.parse().ok()
    });
    Source {
        source: sitio.into(),
        title: format!("{nombre} [{q}] [{idioma}]"),
        info_hash: hash_de_magnet(&magnet_),
        magnet: Some(magnet_),
        url: None,
        size_bytes: size,
        seeders: None,
        quality: calidad(q),
        rd_cached: None,
        hardsub: None,
        file_idx: None,
        filename: None,
        links: None,
    }
}

// ---- elitetorrent ----------------------------------------------------------

const ELITE: &[&str] = &["https://www.elitetorrent.com"];

/// Resultados de la búsqueda: (url, título).
fn elite_resultados(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, _) in html.match_indices("class=nombre href=\"") {
        let resto = &html[i + "class=nombre href=\"".len()..];
        let Some(fin) = resto.find('"') else { continue };
        let url = resto[..fin].to_string();
        let Some(a) = resto.find('>') else { continue };
        let Some(b) = resto[a..].find("</a>") else { continue };
        let titulo = des_html(resto[a + 1..a + b].trim());
        if !out.iter().any(|(u, _)| *u == url) {
            out.push((url, titulo));
        }
    }
    out
}

/// Magnet de la ficha (decodificando los links del acortador). La ficha trae
/// también el .torrent; el magnet alcanza y evita una descarga más.
fn elite_magnet(html: &str) -> Option<String> {
    html.match_indices("s.php?i=").find_map(|(i, _)| {
        let resto = &html[i + "s.php?i=".len()..];
        let fin = resto.find(['"', '\'', '&', ' ', '>']).unwrap_or(resto.len());
        destino_acortador(&resto[..fin]).filter(|d| d.starts_with("magnet:"))
    })
}

pub async fn elitetorrent_search(
    titulos: &[String],
    anio: Option<u32>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    let serie = season.zip(episode);
    let esp = espejos("elitetorrent", ELITE).await;
    let mut out = Vec::new();
    let mut vistas: Vec<String> = Vec::new();
    for t in titulos.iter().filter(|t| !t.trim().is_empty()) {
        let q = urlencoding::encode(t.trim());
        let (base, html) = pagina(&esp, &format!("/?s={q}&x=0&y=0")).await?;
        let cands: Vec<(String, String)> = elite_resultados(&html)
            .into_iter()
            .filter(|(u, tit)| match serie {
                None => u.contains("/peliculas/") && coincide(tit, titulos),
                Some((s, e)) => u.contains("/series/") && sxe(tit) == Some((s, e)) && coincide(&quitar_sxe(tit), titulos),
            })
            .filter(|(u, _)| !vistas.contains(u))
            .take(if serie.is_some() { 1 } else { 4 })
            .collect();
        // Sin año no hay cómo distinguir dos películas homónimas: solo si es única.
        if anio.is_none() && serie.is_none() && cands.len() > 1 {
            continue;
        }
        for (url, tit) in cands {
            vistas.push(url.clone());
            let ficha = match crate::net::get_text(&url, Some(&base)).await {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("[elitetorrent] ficha {url}: {e}");
                    continue;
                }
            };
            if serie.is_none() && !anio_ok(anio, campo(&ficha, "Fecha:").as_deref().and_then(anio_de)) {
                continue;
            }
            let Some(m) = elite_magnet(&ficha) else { continue };
            let q = campo(&ficha, "Calidad:").unwrap_or_default();
            let idi = idioma(&campo(&ficha, "Idioma:").unwrap_or_default());
            let size = campo(&ficha, "Tamaño:").or_else(|| campo(&ficha, "Tama&ntilde;o:")).and_then(|s| tamano(&s));
            out.push(fuente("elitetorrent", &tit, &q, &idi, m, size));
        }
        if !out.is_empty() {
            break;
        }
    }
    if out.is_empty() {
        return Err(format!("{VACIO}elitetorrent sin coincidencia"));
    }
    Ok(out)
}

/// "The Last of Us 2×1" → "The Last of Us".
fn quitar_sxe(s: &str) -> String {
    let t = des_html(s).replace('×', "x");
    let lower = t.to_ascii_lowercase();
    // Corta en el último dígito-x-dígito.
    let b = lower.as_bytes();
    for i in (1..b.len()).rev() {
        if b[i] == b'x' && b[i - 1].is_ascii_digit() {
            let ini = t[..i].trim_end_matches(|c: char| c.is_ascii_digit()).len();
            return t[..ini].trim().to_string();
        }
    }
    t
}

// ---- grantorrent -----------------------------------------------------------

const GRAN: &[&str] = &["https://grantorrent.foo"];

/// URLs de las tarjetas de la búsqueda.
fn gran_resultados(html: &str, base: &str) -> Vec<String> {
    let base = base.trim_end_matches('/');
    let mut out: Vec<String> = Vec::new();
    for (i, _) in html.match_indices("<a href=\"") {
        let resto = &html[i + "<a href=\"".len()..];
        let Some(fin) = resto.find('"') else { continue };
        if !resto[fin..].starts_with("\" x-on:mouseover") {
            continue;
        }
        let url = resto[..fin].to_string();
        if url.starts_with(base) && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// "superman-2" → "superman" (el sufijo con que el sitio desambigua slugs).
fn sin_numero(s: &str) -> &str {
    match s.rsplit_once('-') {
        Some((a, n)) if !a.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()) => a,
        _ => s,
    }
}

/// Slug de una URL ("…/series/the-last-of-us-temporada-1/" → "the-last-of-us-temporada-1").
fn slug(url: &str) -> &str {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// Filas de la tabla de versiones: (textos de las celdas, idioma, data-src).
fn gran_filas(html: &str) -> Vec<(Vec<String>, String, String)> {
    let mut out = Vec::new();
    for parte in html.split("<tr wire:key=").skip(1) {
        let fila = &parte[..parte.find("</tr>").unwrap_or(parte.len())];
        let idi = fila
            .find("title=\"")
            .map(|i| &fila[i + 7..])
            .and_then(|r| r.find('"').map(|f| r[..f].to_string()))
            .unwrap_or_default();
        let Some(ds) = fila.find("data-src=\"").map(|i| &fila[i + 10..]) else { continue };
        let Some(f) = ds.find('"') else { continue };
        let celdas: Vec<String> = fila.split("<td").skip(1).map(|c| sin_tags(&format!("<td{c}"))).collect();
        out.push((celdas, idi, ds[..f].to_string()));
    }
    out
}

pub async fn grantorrent_search(
    titulos: &[String],
    anio: Option<u32>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    let serie = season.zip(episode);
    let esp = espejos("grantorrent", GRAN).await;
    let claves: Vec<String> = titulos.iter().map(|t| norm(t)).filter(|t| !t.is_empty()).collect();
    for t in titulos.iter().filter(|t| !t.trim().is_empty()) {
        let q = urlencoding::encode(t.trim());
        let filtro = if serie.is_some() { "&filtro=series" } else { "" };
        let (base, html) = pagina(&esp, &format!("/?s={q}{filtro}")).await?;
        let base = base.trim_end_matches('/').to_string();
        let cands: Vec<String> = gran_resultados(&html, &base)
            .into_iter()
            .filter(|u| {
                let s = slug(u);
                match serie {
                    // "superman-2": el sitio numera los slugs repetidos; el año de la
                    // ficha decide cuál es.
                    None => {
                        !u.contains("/series/")
                            && (claves.contains(&titulo_limpio(s)) || claves.contains(&titulo_limpio(sin_numero(s))))
                    }
                    Some((temp, _)) => {
                        let sufijo = format!("-temporada-{temp}");
                        u.contains("/series/")
                            && s.ends_with(&sufijo)
                            && claves.contains(&norm(&s[..s.len() - sufijo.len()]))
                    }
                }
            })
            .take(3)
            .collect();
        if anio.is_none() && serie.is_none() && cands.len() > 1 {
            continue;
        }
        let mut out = Vec::new();
        for url in cands {
            let ficha = match crate::net::get_text(&url, Some(&base)).await {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("[grantorrent] ficha {url}: {e}");
                    continue;
                }
            };
            if serie.is_none() {
                let a = ficha.find("Año:").map(|i| trozo(&ficha, i, 120)).and_then(anio_de);
                if !anio_ok(anio, a) {
                    continue;
                }
            }
            for (celdas, idi, ds) in gran_filas(&ficha) {
                if let Some((s, e)) = serie {
                    if !celdas.iter().any(|c| sxe(c) == Some((s, e))) {
                        continue;
                    }
                }
                let Some(destino) = b64(&ds).and_then(|b| String::from_utf8(b).ok()) else { continue };
                // El host del data-src murió; el vigente sirve la misma ruta bajo /files.
                let Some(ruta) = destino.find("/torrents/").map(|i| &destino[i..]) else { continue };
                let tor_url = format!("{base}/files{ruta}");
                let bytes = match bajar_bytes(&tor_url, &url).await {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("[grantorrent] torrent: {e}");
                        continue;
                    }
                };
                let Some((hash, nombre, tam)) = info_torrent(&bytes) else { continue };
                // Celdas: idioma · formato · calidad · peso (películas).
                let q = celdas.get(2).cloned().unwrap_or_default();
                let tit = if serie.is_some() { nombre.clone() } else { titulos[0].clone() };
                let size = (tam > 0).then_some(tam).or_else(|| celdas.get(3).and_then(|c| tamano(c)));
                out.push(fuente("grantorrent", &tit, &q, &idioma(&idi), magnet(&hash, &nombre), size));
                if serie.is_some() {
                    break;
                }
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    Err(format!("{VACIO}grantorrent sin coincidencia"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulos_se_limpian_y_comparan() {
        assert_eq!(titulo_limpio("Superman Hijo Rojo 1080p Castellano"), "supermanhijorojo");
        assert_eq!(titulo_limpio("superman-2"), "superman2");
        assert_eq!(titulo_limpio("Superman (2025)"), "superman");
        assert!(coincide("Superman: Hijo rojo [HDR] Subtitulado", &["Superman: Hijo rojo".into()]));
        assert!(!coincide("Superman y Lois", &["Superman".into()]));
    }

    #[test]
    fn temporada_y_capitulo() {
        assert_eq!(sxe("The Last of Us 2&#215;1"), Some((2, 1)));
        assert_eq!(sxe("1x01 "), Some((1, 1)));
        assert_eq!(sxe("The Last of Us1x09 FINAL"), Some((1, 9)));
        assert_eq!(sxe("Superman"), None);
        assert_eq!(quitar_sxe("The Last of Us 2&#215;1"), "The Last of Us");
    }

    fn b64_enc(d: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for ch in d.chunks(3) {
            let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                if k <= ch.len() {
                    out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn acortador_de_elitetorrent() {
        // rot13("magnet:?xt=urn:btih:ab") en dos capas de base64, con basura al final.
        let r13 = rot13("magnet:?xt=urn:btih:ab");
        let capa1 = b64_enc(r13.as_bytes());
        let capa2 = b64_enc(format!("{capa1},\u{b5}").as_bytes());
        assert_eq!(destino_acortador(&capa2).as_deref(), Some("magnet:?xt=urn:btih:ab"));
    }

    #[test]
    fn info_hash_de_un_torrent() {
        // d4:infod6:lengthi10e4:name3:abcee
        let t = b"d8:announce3:urle4:infod6:lengthi10e4:name3:abce";
        // El bencode de arriba está mal cerrado a propósito: no debe explotar.
        assert!(info_torrent(t).is_none());
        let t = b"d8:announce3:url4:infod6:lengthi10e4:name3:abcee";
        let (hash, nombre, tam) = info_torrent(t).unwrap();
        assert_eq!(nombre, "abc");
        assert_eq!(tam, 10);
        assert_eq!(hash, hex::encode(Sha1::digest(b"d6:lengthi10e4:name3:abce")));
    }

    #[test]
    fn varios() {
        assert_eq!(anio_de("<b>Fecha:</b> 2025-07-09"), Some(2025));
        assert_eq!(anio_de("Año:</span> ñ 2025"), Some(2025));
        assert_eq!(campo("<b>Idioma:</b> <i>Español Latino</i>", "Idioma:").as_deref(), Some("Español Latino"));
        assert_eq!(campo("<b>Fecha:</b> 2025-07-09<br>", "Fecha:").as_deref(), Some("2025-07-09"));
        assert_eq!(sin_numero("superman-2"), "superman");
        assert_eq!(sin_numero("rocky"), "rocky");
        assert_eq!(anio_de("12345"), None);
        assert_eq!(tamano("1.95GB."), Some(1_950_000_000));
        assert_eq!(tamano("N/A"), None);
        assert_eq!(idioma("Español Latino"), "Latino");
        assert_eq!(idioma("Español (Castellano)"), "Castellano");
        assert!(magnet("ab", "a b").starts_with("magnet:?xt=urn:btih:ab&dn=a%20b&tr="));
    }

    #[test]
    fn resultados_de_las_busquedas() {
        let h = r#"<a class=nombre href="https://www.elitetorrent.com/peliculas/superman/" title=Superman>Superman</a>
                   <a class=nombre href="https://www.elitetorrent.com/series/the-last-of-us-21/" title=x>The Last of Us 2&#215;1</a>"#;
        let r = elite_resultados(h);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].1, "The Last of Us 2×1");
        let g = r#"<a href="https://grantorrent.foo/superman/" x-on:mouseover="a"><a href="https://otro.com/x/" x-on:mouseover="a"><a href="https://grantorrent.foo/page/2/">"#;
        assert_eq!(gran_resultados(g, "https://grantorrent.foo"), ["https://grantorrent.foo/superman/"]);
    }
}
