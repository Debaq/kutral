// Packs de temporada: qué archivo del torrent es el capítulo pedido.
//
// Un torrent de temporada trae un video por capítulo. Elegir "el más grande"
// (lo que se hacía antes) reproduce cualquiera: pedir el E05 podía abrir el
// E01. El orden de confianza para elegir:
//
//   1. `filename` del addon (Torrentio/MediaFusion lo mandan por stream);
//   2. `fileIdx` del addon, salvo que el nombre de ese archivo diga otro
//      capítulo;
//   3. el capítulo leído del nombre de cada archivo (S01E05, 1x05, - 05…);
//   4. el video más grande, solo si el torrent NO es un pack: en un pack eso
//      sería con seguridad otro capítulo, y mejor fallar y probar otra fuente.
//
// Los índices son los del torrent (0 en adelante, en el orden del .torrent):
// es lo que mandan los addons como `fileIdx` y lo que usa librqbit. En
// RealDebrid el id del archivo es ese índice + 1.

use serde::{Deserialize, Serialize};

/// Lo que se sabe del archivo que se quiere, además del magnet.
#[derive(Deserialize, Default, Clone, Debug)]
pub struct Pista {
    #[serde(default)]
    pub file_idx: Option<u32>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
}

/// Un capítulo dentro del pack, para el menú "Capítulos" y para pedir el
/// siguiente sin buscar fuentes de nuevo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArchivoPack {
    pub idx: u32,
    /// Nombre del archivo sin carpetas.
    pub nombre: String,
    pub bytes: u64,
    pub season: Option<u32>,
    pub episode: u32,
}

/// Un archivo del torrent tal como lo describe RD o librqbit.
pub struct Archivo<'a> {
    pub idx: u32,
    pub ruta: &'a str,
    pub bytes: u64,
}

const VIDEO_EXT: [&str; 7] = [".mkv", ".mp4", ".avi", ".mov", ".m4v", ".ts", ".webm"];

/// Por debajo de esto no es un capítulo: muestras, intros sueltas, extras.
const MIN_BYTES: u64 = 5 * 1024 * 1024;

pub fn es_video(ruta: &str) -> bool {
    let l = ruta.to_lowercase();
    VIDEO_EXT.iter().any(|e| l.ends_with(e))
}

fn base(ruta: &str) -> &str {
    ruta.rsplit(['/', '\\']).next().unwrap_or(ruta)
}

/// Video que puede ser un capítulo (descarta muestras y archivos minúsculos).
fn candidato(a: &Archivo) -> bool {
    es_video(a.ruta) && a.bytes >= MIN_BYTES && !base(a.ruta).to_lowercase().contains("sample")
}

/// Número de `min..=max` dígitos que empieza en `i` y no sigue con otro
/// dígito. Devuelve el valor y dónde termina.
fn num(b: &[u8], i: usize, min: usize, max: usize) -> Option<(u32, usize)> {
    let mut j = i;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    let n = j - i;
    if n < min || n > max {
        return None;
    }
    let v = std::str::from_utf8(&b[i..j]).ok()?.parse().ok()?;
    Some((v, j))
}

fn sep(c: u8) -> bool {
    matches!(c, b' ' | b'.' | b'_' | b'-')
}

/// Empieza una "palabra" en `i`: lo de antes no es letra ni número.
fn inicio(b: &[u8], i: usize) -> bool {
    i == 0 || !b[i - 1].is_ascii_alphanumeric()
}

/// Temporada (si la dice) y capítulo que indica el nombre de un archivo.
pub fn episodio(ruta: &str) -> Option<(Option<u32>, u32)> {
    let nombre = base(ruta).to_lowercase();
    let b = nombre.as_bytes();

    // S01E05, s1e5, S01.E05, S01E05E06 (se queda con el primero).
    for i in 0..b.len() {
        if b[i] == b's' && inicio(b, i) {
            let Some((s, mut j)) = num(b, i + 1, 1, 2) else { continue };
            if j < b.len() && sep(b[j]) {
                j += 1;
            }
            if j < b.len() && b[j] == b'e' {
                if let Some((e, _)) = num(b, j + 1, 1, 4) {
                    return Some((Some(s), e));
                }
            }
        }
    }

    // 1x05. El número de antes tiene que ser corto y suelto: así 1920x1080 no
    // cuenta.
    for i in 0..b.len() {
        if b[i].is_ascii_digit() && inicio(b, i) {
            let Some((s, j)) = num(b, i, 1, 2) else { continue };
            if j < b.len() && b[j] == b'x' {
                if let Some((e, _)) = num(b, j + 1, 2, 3) {
                    return Some((Some(s), e));
                }
            }
        }
    }

    // "Episode 5", "Ep05", "Capitulo 5", "E05".
    for clave in ["episodio", "episode", "capitulo", "cap", "ep", "e"] {
        let k = clave.as_bytes();
        let mut i = 0;
        while i + k.len() <= b.len() {
            if &b[i..i + k.len()] == k && inicio(b, i) {
                let mut j = i + k.len();
                // "e" sola exige el número pegado: "e 5" es demasiado común.
                if clave != "e" && j < b.len() && sep(b[j]) {
                    j += 1;
                }
                let min = if clave == "e" { 2 } else { 1 };
                if let Some((e, _)) = num(b, j, min, 4) {
                    return Some((None, e));
                }
            }
            i += 1;
        }
    }

    // Anime: "Frieren - 05 (1080p)", "- 05v2". Un año no es un capítulo.
    let mut i = 0;
    while i + 3 <= b.len() {
        if &b[i..i + 3] == b" - " {
            if let Some((e, j)) = num(b, i + 3, 1, 4) {
                let fin = j >= b.len()
                    || matches!(b[j], b' ' | b'.' | b'[' | b'(' | b'_')
                    || (b[j] == b'v' && b.get(j + 1).is_some_and(|c| c.is_ascii_digit()));
                if fin && !(1950..=2099).contains(&e) {
                    return Some((None, e));
                }
            }
        }
        i += 1;
    }
    None
}

/// Los capítulos del torrent, si es un pack (2+ capítulos distintos). Vacío
/// si es un archivo suelto.
pub fn listado(archivos: &[Archivo]) -> Vec<ArchivoPack> {
    let mut v: Vec<ArchivoPack> = archivos
        .iter()
        .filter(|a| candidato(a))
        .filter_map(|a| {
            let (season, episode) = episodio(a.ruta)?;
            Some(ArchivoPack {
                idx: a.idx,
                nombre: base(a.ruta).to_string(),
                bytes: a.bytes,
                season,
                episode,
            })
        })
        .collect();
    v.sort_by_key(|a| (a.season.unwrap_or(0), a.episode, std::cmp::Reverse(a.bytes)));
    // El mismo capítulo dos veces (versión extendida, otro idioma): queda el
    // más grande.
    v.dedup_by_key(|a| (a.season, a.episode));
    if v.len() < 2 {
        v.clear();
    }
    v
}

/// ¿El capítulo de este archivo puede ser el pedido? Sin dato del nombre o
/// del pedido, no se descarta.
fn coincide(ruta: &str, pista: &Pista) -> Option<bool> {
    let (s, e) = episodio(ruta)?;
    let pe = pista.episode?;
    let temporada_ok = match (s, pista.season) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };
    Some(e == pe && temporada_ok)
}

/// Índice (del torrent) del archivo a reproducir.
pub fn elegir(archivos: &[Archivo], pista: &Pista) -> Result<u32, String> {
    let videos: Vec<&Archivo> = archivos.iter().filter(|a| candidato(a)).collect();
    if videos.is_empty() {
        // Sin extensiones reconocibles: lo más grande a secas.
        return archivos
            .iter()
            .max_by_key(|a| a.bytes)
            .map(|a| a.idx)
            .ok_or_else(|| "el torrent no trae archivos".into());
    }

    if let Some(f) = pista.filename.as_deref().filter(|f| !f.is_empty()) {
        let f = base(f);
        if let Some(a) = videos.iter().find(|a| base(a.ruta).eq_ignore_ascii_case(f)) {
            return Ok(a.idx);
        }
    }

    if let Some(i) = pista.file_idx {
        if let Some(a) = videos.iter().find(|a| a.idx == i) {
            if coincide(a.ruta, pista) != Some(false) {
                return Ok(a.idx);
            }
        }
    }

    if pista.episode.is_some() {
        if let Some(a) = videos
            .iter()
            .filter(|a| coincide(a.ruta, pista) == Some(true))
            .max_by_key(|a| a.bytes)
        {
            return Ok(a.idx);
        }
        if !listado(archivos).is_empty() {
            let cap = match (pista.season, pista.episode) {
                (Some(s), Some(e)) => format!("T{s} E{e}"),
                (_, Some(e)) => format!("el capítulo {e}"),
                _ => "ese capítulo".into(),
            };
            return Err(format!("el pack no trae {cap}"));
        }
    }

    Ok(videos.iter().max_by_key(|a| a.bytes).map(|a| a.idx).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Caso = (&'static str, Option<(Option<u32>, u32)>);

    #[test]
    fn lee_el_capitulo_del_nombre() {
        let casos: &[Caso] = &[
            ("Breaking.Bad.S01E05.Gray.Matter.1080p.mkv", Some((Some(1), 5))),
            ("Breaking Bad  S01E05  Gray Matter.mkv", Some((Some(1), 5))),
            ("Breaking.Bad.S01.2160p/Breaking.Bad.S01E05.2160p.mkv", Some((Some(1), 5))),
            ("show.s2.e10.mkv", Some((Some(2), 10))),
            ("Show.S01E01E02.mkv", Some((Some(1), 1))),
            ("Show 1x05 Title.avi", Some((Some(1), 5))),
            ("[SubsPlease] Sousou no Frieren - 05 (1080p) [ABCD1234].mkv", Some((None, 5))),
            ("Frieren - 05v2 [1080p].mkv", Some((None, 5))),
            ("One Piece - 1100.mkv", Some((None, 1100))),
            ("Show.Episode.5.mkv", Some((None, 5))),
            ("Serie Capitulo 7.mp4", Some((None, 7))),
            ("Show.E05.720p.mkv", Some((None, 5))),
            ("Movie.2160p.x265.HEVC-GRP.mkv", None),
            ("Show.1920x1080.mkv", None),
            ("Show - 2024 - Special.mkv", None),
            ("Pelicula.E-AC3.mkv", None),
        ];
        for (nombre, esperado) in casos {
            assert_eq!(episodio(nombre), *esperado, "{nombre}");
        }
    }

    fn pack() -> Vec<(u32, String, u64)> {
        let mut v: Vec<(u32, String, u64)> = (1..=7)
            .map(|e| (e - 1, format!("BB.S01/Breaking.Bad.S01E0{e}.1080p.mkv"), 600 << 20))
            .collect();
        // El E01 es el más grande: elegir "el mayor" daría siempre este.
        v[0].2 = 900 << 20;
        v.push((7, "PSArips.com.txt".into(), 100));
        v.push((8, "BB.S01/Sample/sample.mkv".into(), 20 << 20));
        v
    }

    fn archivos(v: &[(u32, String, u64)]) -> Vec<Archivo<'_>> {
        v.iter().map(|(i, r, b)| Archivo { idx: *i, ruta: r, bytes: *b }).collect()
    }

    fn pista(idx: Option<u32>, nombre: Option<&str>, s: Option<u32>, e: Option<u32>) -> Pista {
        Pista { file_idx: idx, filename: nombre.map(String::from), season: s, episode: e }
    }

    #[test]
    fn elige_el_capitulo_pedido_en_un_pack() {
        let v = pack();
        let a = archivos(&v);
        // Por nombre del addon, aunque el índice esté mal.
        assert_eq!(elegir(&a, &pista(Some(0), Some("Breaking.Bad.S01E05.1080p.mkv"), None, None)), Ok(4));
        // Por índice.
        assert_eq!(elegir(&a, &pista(Some(2), None, Some(1), Some(3))), Ok(2));
        // Índice que contradice el capítulo pedido: manda el nombre.
        assert_eq!(elegir(&a, &pista(Some(0), None, Some(1), Some(6))), Ok(5));
        // Sin ayuda del addon: por el nombre de los archivos.
        assert_eq!(elegir(&a, &pista(None, None, Some(1), Some(7))), Ok(6));
        // Capítulo que el pack no trae: error, no el más grande.
        assert!(elegir(&a, &pista(None, None, Some(1), Some(9))).is_err());
        assert!(elegir(&a, &pista(None, None, Some(2), Some(1))).is_err());
        // Película o sin capítulo: el más grande.
        assert_eq!(elegir(&a, &Pista::default()), Ok(0));
    }

    #[test]
    fn archivo_suelto_no_es_pack() {
        let v = vec![
            (0, "Show.S01E05.mkv".to_string(), 700 << 20),
            (1, "Show.S01E05.sample.mkv".to_string(), 30 << 20),
        ];
        let a = archivos(&v);
        assert!(listado(&a).is_empty());
        // Un scraper que mandó el capítulo equivocado sigue reproduciendo lo
        // que hay (como antes): no hay con qué compararlo.
        assert_eq!(elegir(&a, &pista(None, None, Some(1), Some(6))), Ok(0));
    }

    #[test]
    fn listado_del_pack() {
        let v = pack();
        let l = listado(&archivos(&v));
        assert_eq!(l.len(), 7);
        assert_eq!(l[0].episode, 1);
        assert_eq!(l[4], ArchivoPack {
            idx: 4,
            nombre: "Breaking.Bad.S01E05.1080p.mkv".into(),
            bytes: 600 << 20,
            season: Some(1),
            episode: 5,
        });
    }
}
