// Elegir el subtítulo que calza con el video que se reproduce.
//
// Un subtítulo se sincroniza contra UN release: el mismo grupo y el mismo
// origen (WEB vs BluRay) casi siempre comparten tiempos, y otro corte no. Por
// eso, antes que el más descargado, va el que coincide con la fuente:
//
// 1. moviehash de OpenSubtitles (tamaño + primeros y últimos 64 KB): el
//    subtítulo se subió para ESE archivo exacto. La API no filtra por hash,
//    solo marca `moviehash_match` en cada resultado.
// 2. Parecido del nombre del release (grupo, origen, plataforma, resolución).

use std::collections::HashMap;
use std::sync::Mutex;

/// Desde este puntaje se considera "mismo release" (grupo + origen).
pub const MISMO_RELEASE: i32 = 65;

/// Origen del video. Dos releases de la misma familia suelen compartir
/// tiempos; entre familias (un BluRay contra un WEB) cambian cortes y logos.
fn familia(tok: &str) -> Option<&'static str> {
    match tok {
        "bluray" | "bdrip" | "brrip" | "bdremux" | "remux" | "bd" | "bdmv" => Some("bluray"),
        "web" | "webdl" | "webrip" => Some("web"),
        "hdtv" | "pdtv" | "dsr" => Some("tv"),
        "dvdrip" | "dvd" | "dvdr" | "dvd9" | "dvd5" => Some("dvd"),
        "cam" | "ts" | "telesync" | "hdcam" | "tc" => Some("cam"),
        _ => None,
    }
}

/// Plataforma de origen de un WEB: misma plataforma = mismo master.
fn plataforma(tok: &str) -> Option<&'static str> {
    match tok {
        "amzn" | "amazon" => Some("amzn"),
        "nf" | "netflix" => Some("nf"),
        "dsnp" | "disney" => Some("dsnp"),
        "hmax" | "max" => Some("max"),
        "atvp" => Some("atvp"),
        "hulu" => Some("hulu"),
        "pcok" => Some("pcok"),
        "cr" | "crunchyroll" => Some("cr"),
        "pmtp" => Some("pmtp"),
        _ => None,
    }
}

fn resolucion(tok: &str) -> Option<&'static str> {
    match tok {
        "2160p" | "4k" | "uhd" => Some("2160p"),
        "1080p" | "1080i" => Some("1080p"),
        "720p" => Some("720p"),
        "480p" | "576p" => Some("sd"),
        _ => None,
    }
}

/// Saca extensión y sufijos de idioma: "X-RARBG.es.srt" → "X-RARBG".
fn sin_sufijos(nombre: &str) -> &str {
    const SUFIJOS: &[&str] = &[
        "srt", "ass", "ssa", "sub", "vtt", "mkv", "mp4", "avi", "es", "spa", "esp", "lat", "en",
        "eng", "forced",
    ];
    let mut s = nombre.trim();
    loop {
        let Some((base, ext)) = s.rsplit_once('.') else { return s };
        let ext = ext.to_ascii_lowercase();
        // ".14" de "X-RARBG.14.es.srt": numeración del uploader, no del release.
        if SUFIJOS.contains(&ext.as_str()) || (ext.len() <= 2 && ext.chars().all(|c| c.is_ascii_digit())) {
            s = base;
        } else {
            return s;
        }
    }
}

/// Grupo del release: "[SubsPlease] Show - 05" (anime) o "Show.1080p-GRUPO".
pub(crate) fn grupo(nombre: &str) -> Option<String> {
    let n = sin_sufijos(nombre);
    if let Some(rest) = n.strip_prefix('[') {
        let g = rest.split(']').next()?.trim();
        return (!g.is_empty()).then(|| g.to_ascii_lowercase());
    }
    let (_, g) = n.rsplit_once('-')?;
    let g = g.split(|c: char| c == '[' || c == '(' || c.is_whitespace()).next()?.trim();
    // "- 05" de un capítulo o "-1" no son grupos.
    if g.len() < 2 || g.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(g.to_ascii_lowercase())
}

fn tokens(nombre: &str) -> Vec<String> {
    sin_sufijos(nombre)
        .to_ascii_lowercase()
        .replace("web-dl", "webdl")
        .replace("blu-ray", "bluray")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalizado(nombre: &str) -> String {
    tokens(nombre).join(".")
}

fn rasgo<'a>(toks: &'a [String], f: fn(&str) -> Option<&'static str>) -> Option<&'static str> {
    toks.iter().find_map(|t| f(t))
}

/// Qué tanto calza un subtítulo (`sub`, su nombre de release) con el video
/// (`video`, nombre del release que se reproduce). 0 = nada en común.
pub fn release_score(video: &str, sub: &str) -> i32 {
    if video.trim().is_empty() || sub.trim().is_empty() {
        return 0;
    }
    if normalizado(video) == normalizado(sub) {
        return 100;
    }
    let (tv, ts) = (tokens(video), tokens(sub));
    let mut score = 0;
    if let (Some(a), Some(b)) = (grupo(video), grupo(sub)) {
        if a == b {
            score += 40;
        }
    }
    match (rasgo(&tv, familia), rasgo(&ts, familia)) {
        (Some(a), Some(b)) if a == b => score += 25,
        // Otro origen = otro corte casi seguro: que pierda frente a uno sin dato.
        (Some(_), Some(_)) => score -= 15,
        _ => {}
    }
    if let (Some(a), Some(b)) = (rasgo(&tv, plataforma), rasgo(&ts, plataforma)) {
        if a == b {
            score += 15;
        }
    }
    if let (Some(a), Some(b)) = (rasgo(&tv, resolucion), rasgo(&ts, resolucion)) {
        if a == b {
            score += 5;
        }
    }
    score
}

// ─────────────────────────────── moviehash ──────────────────────────────────

const HASH_CHUNK: u64 = 64 * 1024;

/// Hash de OpenSubtitles: tamaño + suma (u64, little endian, con desborde) de
/// las palabras de los primeros y los últimos 64 KB.
fn osdb_hash(size: u64, head: &[u8], tail: &[u8]) -> String {
    let mut h = size;
    for chunk in [head, tail] {
        for w in chunk.chunks_exact(8) {
            h = h.wrapping_add(u64::from_le_bytes(w.try_into().unwrap()));
        }
    }
    format!("{h:016x}")
}

static HASH_CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// Some(resultado) si ya se calculó para esta URL (aunque haya fallado).
/// Aparte y síncrono: el guard no puede cruzar un `.await`.
fn hash_cacheado(url: &str) -> Option<Option<String>> {
    HASH_CACHE.lock().ok()?.as_ref()?.get(url).cloned()
}

/// moviehash del video en `url` (http/https o ruta local). None si no se pudo:
/// servidor sin Range, archivo chico o red caída. Se cachea por URL: el mismo
/// video se consulta al buscar y otra vez al abrir la lista.
pub async fn movie_hash(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    if let Some(v) = hash_cacheado(url) {
        return v;
    }
    let h = if url.starts_with("http://") || url.starts_with("https://") {
        hash_http(url).await
    } else {
        hash_local(url.strip_prefix("file://").unwrap_or(url))
    };
    if let Ok(mut c) = HASH_CACHE.lock() {
        c.get_or_insert_with(HashMap::new).insert(url.to_string(), h.clone());
    }
    h
}

fn hash_local(path: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    if size < 2 * HASH_CHUNK {
        return None;
    }
    let mut head = vec![0u8; HASH_CHUNK as usize];
    let mut tail = vec![0u8; HASH_CHUNK as usize];
    f.read_exact(&mut head).ok()?;
    f.seek(SeekFrom::Start(size - HASH_CHUNK)).ok()?;
    f.read_exact(&mut tail).ok()?;
    Some(osdb_hash(size, &head, &tail))
}

/// Pide los dos extremos con Range. El tamaño sale del Content-Range de la
/// primera respuesta. Un torrent local (127.0.0.1) también sirve Range; pedir
/// el final solo adelanta la última pieza, que mpv igual lee al abrir.
async fn hash_http(url: &str) -> Option<String> {
    // Con User-Agent: hay servidores que rechazan con 403 a quien no manda uno.
    let cli = reqwest::Client::builder()
        .user_agent(concat!("kutral v", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let rango = |a: u64, b: u64| format!("bytes={a}-{b}");
    let r = cli
        .get(url)
        .header("Range", rango(0, HASH_CHUNK - 1))
        .send()
        .await
        .ok()?;
    if r.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None; // sin Range bajaríamos el archivo entero
    }
    let size: u64 = r
        .headers()
        .get("content-range")?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .parse()
        .ok()?;
    if size < 2 * HASH_CHUNK {
        return None;
    }
    let head = r.bytes().await.ok()?;
    let t = cli
        .get(url)
        .header("Range", rango(size - HASH_CHUNK, size - 1))
        .send()
        .await
        .ok()?;
    if t.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let tail = t.bytes().await.ok()?;
    if head.len() as u64 != HASH_CHUNK || tail.len() as u64 != HASH_CHUNK {
        return None;
    }
    Some(osdb_hash(size, &head, &tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismo_release_con_sufijos_de_subtitulo() {
        let v = "Inception.2010.1080p.BluRay.H264.AAC-RARBG";
        assert_eq!(release_score(v, "Inception.2010.1080p.BluRay.H264.AAC-RARBG.es.srt"), 100);
        assert_eq!(release_score(v, "Inception.2010.1080p.BluRay.H264.AAC-RARBG.14.es.srt"), 100);
    }

    #[test]
    fn grupo_y_origen_alcanzan_para_mismo_release() {
        let v = "Show.S01E02.1080p.WEB-DL.DDP5.1.H.264-NTb";
        let s = "Show.S01E02.720p.WEBRip.x264-NTb";
        assert!(release_score(v, s) >= MISMO_RELEASE, "{}", release_score(v, s));
    }

    #[test]
    fn otro_origen_pierde_contra_uno_sin_datos() {
        let v = "Movie.2020.1080p.WEB-DL-GRP";
        let bluray = "Movie.2020.1080p.BluRay.x264-OTRO";
        let sin_datos = "Movie (2020) español";
        assert!(release_score(v, bluray) < release_score(v, sin_datos));
    }

    #[test]
    fn plataforma_suma() {
        let v = "Show.S01E01.1080p.AMZN.WEB-DL-A";
        assert!(release_score(v, "Show.S01E01.AMZN.WEBRip-B") > release_score(v, "Show.S01E01.NF.WEBRip-B"));
    }

    #[test]
    fn grupo_de_anime_entre_corchetes() {
        assert_eq!(grupo("[SubsPlease] Frieren - 05 (1080p) [ABCD1234].mkv").as_deref(), Some("subsplease"));
        // "- 05" es el capítulo, no un grupo.
        assert_eq!(grupo("Frieren - 05"), None);
    }

    #[test]
    fn sin_nombre_no_puntua() {
        assert_eq!(release_score("", "X-GRP"), 0);
        assert_eq!(release_score("X-GRP", " "), 0);
    }

    #[test]
    fn osdb_hash_suma_tamano_y_extremos() {
        // Tamaño solo, extremos en cero.
        let cero = vec![0u8; HASH_CHUNK as usize];
        assert_eq!(osdb_hash(0x1234, &cero, &cero), "0000000000001234");
        // Una palabra en cada extremo + desborde de u64.
        let mut a = cero.clone();
        a[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut b = cero.clone();
        b[..8].copy_from_slice(&2u64.to_le_bytes());
        assert_eq!(osdb_hash(0, &a, &b), "0000000000000001");
    }

    /// Vector oficial de OpenSubtitles. Necesita el archivo (12 MB):
    ///   curl -LO https://static.opensubtitles.org/addons/avi/breakdance.avi
    ///   BREAKDANCE_AVI=breakdance.avi cargo test -- --ignored vector_oficial
    #[test]
    #[ignore]
    fn vector_oficial_breakdance() {
        let p = std::env::var("BREAKDANCE_AVI").expect("BREAKDANCE_AVI=ruta al archivo");
        assert_eq!(hash_local(&p).as_deref(), Some("8e245d9679d31e12"));
    }

    /// Camino HTTP (Range) contra un archivo público. Red: va con --ignored.
    /// El valor esperado se sacó con una implementación aparte (Python).
    #[tokio::test]
    #[ignore]
    async fn hash_por_http_con_range() {
        let u = "https://download.blender.org/demo/movies/ToS/tears_of_steel_720p.mov";
        assert_eq!(hash_http(u).await.as_deref(), Some("280f9539b3fc185d"));
    }

    #[test]
    fn hash_local_de_archivo_conocido() {
        let dir = std::env::temp_dir().join(format!("kutral-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("v.bin");
        // 200 KB de bytes 0x01: cada palabra vale 0x0101010101010101.
        std::fs::write(&p, vec![1u8; 200 * 1024]).unwrap();
        let palabras = 2 * HASH_CHUNK / 8;
        let esperado = (200u64 * 1024).wrapping_add(palabras.wrapping_mul(0x0101_0101_0101_0101));
        assert_eq!(hash_local(p.to_str().unwrap()), Some(format!("{esperado:016x}")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
