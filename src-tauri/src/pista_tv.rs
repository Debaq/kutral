// Elegir la pista de audio que suena en la TV.
//
// Ni Google Cast (Default Media Receiver) ni DLNA dejan elegir el audio de un
// MKV/MP4 suelto: tocan la PRIMERA pista de audio que lista el archivo. Cast
// incluso acepta EDIT_TRACKS_INFO, contesta que cambió… y sigue sonando la
// misma. Probado con una LG por Cast (2026-10-17):
//   - mover solo la marca de "predeterminada" → la TV la ignora;
//   - poner la pista elegida PRIMERA entre los audios (+ la marca) → suena esa,
//     también con códec distinto (EAC3 5.1 detrás de un AAC) y al saltar.
//
// Así que la pasarela de lan.rs reescribe esa parte de la cabecera mientras
// reenvía el video. Solo cambia el ORDEN de la lista (MKV: TrackEntry dentro
// de Tracks; MP4: los trak dentro de moov) y las marcas; el archivo queda del
// MISMO tamaño y nada más se mueve, así que los rangos, los saltos y los
// índices (Cues, stco) siguen sirviendo tal cual. Sin ffmpeg y sin re-mux.

use std::io::Read;
use std::sync::Arc;

/// Bytes que reemplazan a los del archivo desde `ini`.
#[derive(Debug, Clone, PartialEq)]
pub struct Parche {
    pub ini: u64,
    pub bytes: Vec<u8>,
}

/// Lee `largo` bytes desde `ini` (puede devolver menos al final del archivo).
pub type Lector<'a> = dyn FnMut(u64, usize) -> Result<Vec<u8>, String> + 'a;

/// Arma el parche para que suene la pista de audio `n` (1 = la primera que
/// lista el archivo, el mismo orden que usa mpv para `aid`). None si no hace
/// falta (ya es la primera) o si la cabecera no se deja (se transmite igual,
/// con el audio de siempre).
pub fn preparar(leer: &mut Lector, mkv: bool, n: usize) -> Option<Parche> {
    if n <= 1 {
        return None;
    }
    if mkv {
        // Tracks viene en los primeros KB (mkvmerge ~8 KB, ffmpeg <1 KB).
        let cab = leer(0, 512 << 10).ok()?;
        parche_mkv(&cab, n)
    } else {
        let (ini, moov) = buscar_moov(leer)?;
        parche_mp4(&moov, ini, n)
    }
}

// ---- MKV (EBML) -------------------------------------------------------------

const SEGMENT: u64 = 0x1853_8067;
const TRACKS: u64 = 0x1654_AE6B;
const CLUSTER: u64 = 0x1F43_B675;
const TRACK_ENTRY: u64 = 0xAE;
const TRACK_TYPE: u64 = 0x83;
const FLAG_DEFAULT: u64 = 0x88;
const VOID: u64 = 0xEC;
const CRC32: u64 = 0xBF;

/// (valor, largo). `id` deja el marcador de largo puesto (así se escriben los
/// ID de EBML); los tamaños lo sacan.
fn vint(b: &[u8], p: usize, id: bool) -> Option<(u64, usize)> {
    let f = *b.get(p)?;
    if f == 0 {
        return None;
    }
    let l = f.leading_zeros() as usize + 1;
    let mut v = if id { f as u64 } else { (f as u64) & ((0x80 >> (l - 1)) - 1) };
    for i in 1..l {
        v = (v << 8) | *b.get(p + i)? as u64;
    }
    Some((v, l))
}

struct Elem {
    id: u64,
    /// Inicio del elemento (su ID).
    ini: usize,
    /// Inicio del contenido.
    dato: usize,
    /// Largo del contenido. None = tamaño "desconocido" (Segment en vivo).
    largo: Option<usize>,
}

impl Elem {
    fn fin(&self) -> Option<usize> {
        self.largo.map(|l| self.dato + l)
    }
}

fn elem(b: &[u8], p: usize) -> Option<Elem> {
    let (id, il) = vint(b, p, true)?;
    let (s, sl) = vint(b, p + il, false)?;
    let desconocido = s == (1u64 << (7 * sl)) - 1;
    Some(Elem {
        id,
        ini: p,
        dato: p + il + sl,
        largo: if desconocido { None } else { Some(s as usize) },
    })
}

/// Hijos de [ini, fin). Corta al primer elemento roto o que no entra.
fn hijos(b: &[u8], mut p: usize, fin: usize) -> Vec<Elem> {
    let mut v = Vec::new();
    while p < fin {
        let Some(e) = elem(b, p) else { break };
        let Some(f) = e.fin() else { break };
        if f > fin {
            break;
        }
        p = f;
        v.push(e);
    }
    v
}

/// Tamaño EBML en `l` bytes (None si no cabe).
fn tam(n: usize, l: usize) -> Option<Vec<u8>> {
    // El valor con todos los bits en 1 está reservado para "desconocido".
    if l == 0 || l > 8 || (n as u64) >= (1u64 << (7 * l)) - 1 {
        return None;
    }
    let v = (n as u64) | (1u64 << (7 * l));
    Some(v.to_be_bytes()[8 - l..].to_vec())
}

fn tam_min(n: usize) -> Vec<u8> {
    (1..=8).find_map(|l| tam(n, l)).expect("tamaño EBML")
}

fn id_bytes(id: u64) -> Vec<u8> {
    let b = id.to_be_bytes();
    let desde = b.iter().position(|&x| x != 0).unwrap_or(7);
    b[desde..].to_vec()
}

fn con_tam(id: u64, dato: &[u8], l: Option<usize>) -> Option<Vec<u8>> {
    let mut v = id_bytes(id);
    v.extend(match l {
        Some(l) => tam(dato.len(), l)?,
        None => tam_min(dato.len()),
    });
    v.extend_from_slice(dato);
    Some(v)
}

/// Relleno (Void) de exactamente `n` bytes. n = 1 no existe.
fn relleno(n: usize) -> Option<Vec<u8>> {
    match n {
        0 => Some(Vec::new()),
        1 => None,
        2..=127 => con_tam(VOID, &vec![0; n - 2], Some(1)),
        _ => con_tam(VOID, &vec![0; n - 9], Some(8)),
    }
}

fn parche_mkv(b: &[u8], n: usize) -> Option<Parche> {
    let seg = hijos_segmento(b)?;
    let i = seg.iter().position(|e| e.id == TRACKS)?;
    let tracks = &seg[i];
    let fin_tracks = tracks.fin()?;
    // Zona que se puede reescribir: Tracks + el Void que lo siga (mkvmerge
    // deja uno, por si la lista nueva sale más larga).
    let fin_zona = match seg.get(i + 1) {
        Some(v) if v.id == VOID => v.fin()?,
        _ => fin_tracks,
    };
    let zona = fin_zona - tracks.ini;

    // Entradas en el orden del archivo: (es audio, bytes nuevos).
    let mut entradas: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut otros: Vec<u8> = Vec::new();
    for e in hijos(b, tracks.dato, fin_tracks) {
        match e.id {
            // El CRC ya no cuadraría con la lista nueva (es opcional).
            VOID | CRC32 => {}
            TRACK_ENTRY => {
                let campos = hijos(b, e.dato, e.fin()?);
                let audio = campos
                    .iter()
                    .any(|c| c.id == TRACK_TYPE && c.fin().is_some_and(|f| b[c.dato..f] == [2]));
                let mut dato = Vec::new();
                for c in &campos {
                    // En audio la marca se rehace abajo. En las demás se deja:
                    // si falta vale 1, no hay que borrarla.
                    if matches!(c.id, VOID | CRC32) || (audio && c.id == FLAG_DEFAULT) {
                        continue;
                    }
                    dato.extend_from_slice(&b[c.ini..c.fin()?]);
                }
                entradas.push((audio, dato));
            }
            _ => otros.extend_from_slice(&b[e.ini..e.fin()?]),
        }
    }
    let audios: Vec<usize> = (0..entradas.len()).filter(|&k| entradas[k].0).collect();
    let elegida = *audios.get(n - 1)?;
    for &k in &audios {
        let marca = if k == elegida { 1 } else { 0 };
        entradas[k].1.extend_from_slice(&[0x88, 0x81, marca]);
    }
    // La elegida pasa al lugar del primer audio; el resto corre uno.
    let e = entradas.remove(elegida);
    entradas.insert(audios[0], e);

    // Tracks nuevo, con su tamaño escrito en los mismos bytes que el original
    // (el ID y lo de antes no se mueven). Lo que sobre va como Void: dentro de
    // Tracks si no hay uno después, o achicando el de después. Si sobra justo
    // 1 byte (ningún Void mide 1) se escribe la primera entrada con su tamaño
    // un byte más largo.
    let l_tracks = tracks.dato - tracks.ini - id_bytes(TRACKS).len();
    let hay_void_despues = fin_zona > fin_tracks;
    for estirar in [false, true] {
        let mut cuerpo = otros.clone();
        for (k, (_, dato)) in entradas.iter().enumerate() {
            let l = (estirar && k == 0).then(|| tam_min(dato.len()).len() + 1);
            cuerpo.extend(con_tam(TRACK_ENTRY, dato, l)?);
        }
        if !hay_void_despues {
            let sobra = (fin_tracks - tracks.dato).checked_sub(cuerpo.len())?;
            let Some(r) = relleno(sobra) else { continue };
            cuerpo.extend(r);
        }
        let mut nuevo = con_tam(TRACKS, &cuerpo, Some(l_tracks))?;
        let Some(r) = relleno(zona.checked_sub(nuevo.len())?) else { continue };
        nuevo.extend(r);
        return Some(Parche {
            ini: tracks.ini as u64,
            bytes: nuevo,
        });
    }
    None
}

/// Elementos de primer nivel del Segment hasta el primer Cluster.
fn hijos_segmento(b: &[u8]) -> Option<Vec<Elem>> {
    let ebml = elem(b, 0)?;
    let seg = elem(b, ebml.fin()?)?;
    if seg.id != SEGMENT {
        return None;
    }
    let fin = seg.fin().unwrap_or(usize::MAX).min(b.len());
    let mut v = Vec::new();
    let mut p = seg.dato;
    while p < fin {
        let Some(e) = elem(b, p) else { break };
        if e.id == CLUSTER {
            break;
        }
        let Some(f) = e.fin() else { break };
        if f > fin {
            break;
        }
        p = f;
        v.push(e);
    }
    Some(v)
}

// ---- MP4 (ISO BMFF) ---------------------------------------------------------

/// Tope para leer moov entero (una película larga anda en 1-5 MB).
const TOPE_MOOV: u64 = 64 << 20;

/// (offset, moov completo). Salta de caja en caja leyendo solo cabeceras:
/// moov puede estar al final, detrás de un mdat de varios GB.
fn buscar_moov(leer: &mut Lector) -> Option<(u64, Vec<u8>)> {
    let mut p = 0u64;
    for _ in 0..64 {
        let c = leer(p, 16).ok()?;
        if c.len() < 8 {
            return None;
        }
        let mut largo = u32::from_be_bytes(c[0..4].try_into().ok()?) as u64;
        if largo == 1 {
            largo = u64::from_be_bytes(c.get(8..16)?.try_into().ok()?);
        }
        if largo < 8 {
            return None; // 0 = "hasta el final": no puede ser moov con mdat detrás
        }
        if &c[4..8] == b"moov" {
            if largo > TOPE_MOOV {
                return None;
            }
            let m = leer(p, largo as usize).ok()?;
            return (m.len() as u64 == largo).then_some((p, m));
        }
        p += largo;
    }
    None
}

/// Cajas hijas de [ini, fin): (tipo, inicio, fin).
fn cajas(b: &[u8], mut p: usize, fin: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut v = Vec::new();
    while p + 8 <= fin {
        let largo = u32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as usize;
        let tipo: [u8; 4] = b[p + 4..p + 8].try_into().unwrap();
        // largo 1 (64 bits) no aparece dentro de moov; 0 o roto = cortar.
        if largo < 8 || p + largo > fin {
            break;
        }
        v.push((tipo, p, p + largo));
        p += largo;
    }
    v
}

fn caja<'a>(b: &'a [u8], tipo: &[u8; 4]) -> Option<&'a [u8]> {
    cajas(b, 8, b.len())
        .into_iter()
        .find(|(t, _, _)| t == tipo)
        .map(|(_, i, f)| &b[i..f])
}

fn es_audio(trak: &[u8]) -> bool {
    let hdlr = caja(trak, b"mdia").and_then(|m| caja(m, b"hdlr"));
    // hdlr: cabecera(8) + versión/flags(4) + pre_defined(4) + handler_type(4)
    hdlr.and_then(|h| h.get(16..20)) == Some(b"soun")
}

fn parche_mp4(moov: &[u8], ini: u64, n: usize) -> Option<Parche> {
    let hijos = cajas(moov, 8, moov.len());
    if hijos.last().map(|h| h.2) != Some(moov.len()) {
        return None;
    }
    let mut partes: Vec<(bool, Vec<u8>)> = hijos
        .iter()
        .map(|&(t, i, f)| (&t == b"trak" && es_audio(&moov[i..f]), moov[i..f].to_vec()))
        .collect();
    let audios: Vec<usize> = (0..partes.len()).filter(|&k| partes[k].0).collect();
    let elegida = *audios.get(n - 1)?;
    for &k in &audios {
        let trak = &mut partes[k].1;
        let (_, ti, _) = cajas(trak, 8, trak.len())
            .into_iter()
            .find(|(t, _, _)| t == b"tkhd")?;
        // tkhd: cabecera(8) + versión(1) + flags(3); bit 0 = pista habilitada.
        let flags = trak.get_mut(ti + 11)?;
        *flags = if k == elegida { *flags | 1 } else { *flags & !1 };
    }
    let e = partes.remove(elegida);
    partes.insert(audios[0], e);
    let mut nuevo = moov[..8].to_vec();
    for (_, p) in partes {
        nuevo.extend(p);
    }
    (nuevo.len() == moov.len()).then_some(Parche { ini, bytes: nuevo })
}

// ---- Lectura con el parche puesto -------------------------------------------

/// Envuelve el cuerpo de una respuesta que empieza en el byte `pos` del
/// archivo y pisa los bytes del parche al pasar.
pub struct ConParche<R> {
    pub interno: R,
    pub pos: u64,
    pub parche: Arc<Parche>,
}

impl<R: Read> Read for ConParche<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.interno.read(buf)?;
        let (a, b) = (self.pos, self.pos + n as u64);
        let (pa, pb) = (self.parche.ini, self.parche.ini + self.parche.bytes.len() as u64);
        let (ini, fin) = (a.max(pa), b.min(pb));
        if ini < fin {
            let dst = (ini - a) as usize..(fin - a) as usize;
            let src = (ini - pa) as usize..(fin - pa) as usize;
            buf[dst].copy_from_slice(&self.parche.bytes[src]);
        }
        self.pos = b;
        Ok(n)
    }
}

/// Parche para `origen` (URL http(s) o archivo del disco), leyendo solo la
/// cabecera. Bloquea: llamarlo fuera del hilo async.
pub fn para(origen: &str, mkv: bool, n: usize) -> Option<Parche> {
    let p = if origen.starts_with("http://") || origen.starts_with("https://") {
        let cli = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .ok()?;
        let mut leer = lector_http(&cli, origen);
        preparar(&mut leer, mkv, n)
    } else {
        let ruta = origen.strip_prefix("file://").unwrap_or(origen);
        preparar(&mut lector_archivo(ruta), mkv, n)
    };
    eprintln!(
        "[tv] audio {n}: {}",
        if p.is_some() { "cabecera lista" } else { "la cabecera no se deja, suena la primera" }
    );
    p
}

/// Lector por rangos HTTP (para `preparar`).
fn lector_http<'a>(cli: &'a reqwest::blocking::Client, url: &'a str) -> impl FnMut(u64, usize) -> Result<Vec<u8>, String> + 'a {
    move |ini, largo| {
        let r = cli
            .get(url)
            .header("Range", format!("bytes={}-{}", ini, ini + largo as u64 - 1))
            .send()
            .map_err(|e| e.to_string())?;
        let s = r.status().as_u16();
        // 200 = el servidor ignoró el rango y manda desde el principio.
        if s != 206 && !(s == 200 && ini == 0) {
            return Err(format!("HTTP {s}"));
        }
        let mut v = Vec::with_capacity(largo.min(8 << 20));
        r.take(largo as u64).read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    }
}

/// Lector de un archivo del disco (para `preparar`).
fn lector_archivo(ruta: &str) -> impl FnMut(u64, usize) -> Result<Vec<u8>, String> + '_ {
    move |ini, largo| {
        use std::io::{Seek, SeekFrom};
        let mut f = std::fs::File::open(ruta).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(ini)).map_err(|e| e.to_string())?;
        let mut v = Vec::new();
        f.take(largo as u64).read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(id: u64, dato: &[u8]) -> Vec<u8> {
        con_tam(id, dato, None).unwrap()
    }

    fn entrada(num: u8, tipo: u8, nombre: &str, extra: &[u8]) -> Vec<u8> {
        let mut d = el(0xD7, &[num]);
        d.extend(el(0x536E, nombre.as_bytes()));
        d.extend(el(TRACK_TYPE, &[tipo]));
        d.extend_from_slice(extra);
        el(TRACK_ENTRY, &d)
    }

    /// EBML + Segment(Info, Tracks(CRC?, Void interno?, entradas), Void?, Cluster).
    fn mkv_con(crc: bool, entradas: &[Vec<u8>], void_interno: usize, void_despues: usize) -> Vec<u8> {
        let mut tracks = if crc { el(CRC32, &[1, 2, 3, 4]) } else { Vec::new() };
        tracks.extend(relleno(void_interno).unwrap());
        for e in entradas {
            tracks.extend_from_slice(e);
        }
        let mut seg = el(0x1549_A966, b"info");
        seg.extend(el(TRACKS, &tracks));
        seg.extend(relleno(void_despues).unwrap());
        seg.extend(el(CLUSTER, &[0xA3, 0x81, 0x00]));
        let mut b = el(0x1A45_DFA3, &[0x42, 0x86, 0x81, 0x01]);
        b.extend(el(SEGMENT, &seg));
        b
    }

    fn mkv(entradas: &[Vec<u8>]) -> Vec<u8> {
        mkv_con(true, entradas, 0, 0)
    }

    /// (número de pista, nombre, FlagDefault) de cada entrada, en orden.
    fn lista(b: &[u8]) -> Vec<(u8, String, Option<u8>)> {
        let seg = hijos_segmento(b).unwrap();
        let t = seg.iter().find(|e| e.id == TRACKS).unwrap();
        hijos(b, t.dato, t.fin().unwrap())
            .iter()
            .filter(|e| e.id == TRACK_ENTRY)
            .map(|e| {
                let c = hijos(b, e.dato, e.fin().unwrap());
                let v = |id| c.iter().find(|x| x.id == id).map(|x| b[x.dato..x.fin().unwrap()].to_vec());
                (
                    v(0xD7).unwrap()[0],
                    String::from_utf8(v(0x536E).unwrap()).unwrap(),
                    v(FLAG_DEFAULT).map(|f| f[0]),
                )
            })
            .collect()
    }

    fn aplicar(b: &[u8], p: &Parche) -> Vec<u8> {
        let mut v = b.to_vec();
        let i = p.ini as usize;
        v[i..i + p.bytes.len()].copy_from_slice(&p.bytes);
        v
    }

    fn base() -> Vec<Vec<u8>> {
        vec![
            entrada(1, 1, "video", &[]),
            entrada(2, 2, "ingles", &[]),
            entrada(3, 2, "latino", &[0x88, 0x81, 0x00]),
            entrada(4, 17, "subs", &[0x88, 0x81, 0x00]),
            entrada(5, 2, "castellano", &[0x88, 0x81, 0x00]),
        ]
    }

    #[test]
    fn mkv_elegida_pasa_primera_entre_los_audios() {
        let b = mkv(&base());
        let p = parche_mkv(&b, 3).expect("parche");
        let nuevo = aplicar(&b, &p);
        assert_eq!(nuevo.len(), b.len());
        assert_eq!(
            lista(&nuevo),
            vec![
                (1, "video".into(), None),
                (5, "castellano".into(), Some(1)),
                (2, "ingles".into(), Some(0)),
                (3, "latino".into(), Some(0)),
                // Los subtítulos conservan su marca (si faltara valdría 1).
                (4, "subs".into(), Some(0)),
            ]
        );
        // Lo de después (el Cluster) quedó intacto.
        assert_eq!(&nuevo[p.ini as usize + p.bytes.len()..], &b[p.ini as usize + p.bytes.len()..]);
    }

    #[test]
    fn mkv_lista_mas_larga_entra_en_los_void_o_no_se_toca() {
        // Sin CRC y con un audio sin marca, la lista nueva sale 3 bytes MÁS
        // larga: tiene que entrar en un Void, dentro de Tracks o después.
        // Con 4 bytes de Void sobra justo 1, que ningún Void llena.
        let e = vec![
            entrada(1, 1, "v", &[]),
            entrada(2, 2, "a", &[]),
            entrada(3, 2, "b", &[0x88, 0x81, 0x00]),
        ];
        for void in [0, 2, 3, 4, 5, 6, 130, 200] {
            for despues in [false, true] {
                let b = if despues { mkv_con(false, &e, 0, void) } else { mkv_con(false, &e, void, 0) };
                let p = parche_mkv(&b, 2);
                if void < 3 {
                    assert!(p.is_none(), "void {void}: no entra y no debe romper nada");
                    continue;
                }
                let p = p.unwrap_or_else(|| panic!("void {void} después={despues}"));
                let nuevo = aplicar(&b, &p);
                assert_eq!(nuevo.len(), b.len());
                assert_eq!(
                    lista(&nuevo),
                    vec![(1, "v".into(), None), (3, "b".into(), Some(1)), (2, "a".into(), Some(0))],
                    "void {void} después={despues}"
                );
                // El Cluster sigue donde estaba.
                let seg = hijos_segmento(&nuevo).unwrap();
                assert!(elem(&nuevo, seg.last().unwrap().fin().unwrap()).unwrap().id == CLUSTER);
            }
        }
    }

    #[test]
    fn mkv_sin_cambio_si_ya_es_la_primera_o_no_existe() {
        let b = mkv(&base());
        let mut leer = |i: u64, l: usize| Ok(b[i as usize..(i as usize + l).min(b.len())].to_vec());
        assert_eq!(preparar(&mut leer, true, 1), None);
        assert_eq!(preparar(&mut leer, true, 9), None);
        assert!(preparar(&mut leer, true, 2).is_some());
        assert_eq!(parche_mkv(b"basura", 2), None);
    }

    fn caja_mp4(tipo: &[u8; 4], dato: &[u8]) -> Vec<u8> {
        let mut v = ((dato.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(tipo);
        v.extend_from_slice(dato);
        v
    }

    fn trak(id: u8, handler: &[u8; 4]) -> Vec<u8> {
        let mut tkhd = vec![0, 0, 0, 3]; // versión 0, flags: habilitada + en película
        tkhd.extend([0; 8]);
        tkhd.extend([0, 0, 0, id]);
        tkhd.extend([0; 68]);
        let mut hdlr = vec![0; 8];
        hdlr.extend_from_slice(handler);
        hdlr.extend([0; 12]);
        let mdia = caja_mp4(b"mdia", &caja_mp4(b"hdlr", &hdlr));
        let mut d = caja_mp4(b"tkhd", &tkhd);
        d.extend(mdia);
        caja_mp4(b"trak", &d)
    }

    #[test]
    fn mp4_moov_al_final_reordena_y_habilita_solo_la_elegida() {
        let mut moov = caja_mp4(b"mvhd", &[0; 100]);
        for (id, h) in [(1, b"vide"), (2, b"soun"), (3, b"soun"), (4, b"soun")] {
            moov.extend(trak(id, h));
        }
        let moov = caja_mp4(b"moov", &moov);
        let mut b = caja_mp4(b"ftyp", b"isom");
        b.extend(caja_mp4(b"mdat", &[7; 5000]));
        let ini = b.len();
        b.extend(&moov);

        let mut pedidos = 0;
        let mut leer = |i: u64, l: usize| {
            pedidos += 1;
            Ok(b[i as usize..(i as usize + l).min(b.len())].to_vec())
        };
        let p = preparar(&mut leer, false, 3).expect("parche");
        assert!(pedidos <= 4, "solo cabeceras + moov, no el mdat");
        assert_eq!(p.ini as usize, ini);
        assert_eq!(p.bytes.len(), moov.len());
        let orden: Vec<(u8, u8)> = cajas(&p.bytes, 8, p.bytes.len())
            .into_iter()
            .filter(|(t, _, _)| t == b"trak")
            .map(|(_, i, _)| (p.bytes[i + 8 + 8 + 12 + 3], p.bytes[i + 8 + 8 + 3] & 1))
            .collect();
        // (track_ID, habilitada): video, 4 (elegida), 2, 3.
        assert_eq!(orden, vec![(1, 1), (4, 1), (2, 0), (3, 0)]);
    }

    #[test]
    fn la_lectura_pisa_solo_lo_del_parche() {
        let datos: Vec<u8> = (0..100).collect();
        let p = Arc::new(Parche { ini: 10, bytes: vec![0xFF; 5] });
        // Una respuesta que arranca en el byte 8 (rango) y se lee de a 3.
        let mut r = ConParche { interno: &datos[8..], pos: 8, parche: p };
        let mut fuera = Vec::new();
        let mut buf = [0u8; 3];
        loop {
            let n = r.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            fuera.extend_from_slice(&buf[..n]);
        }
        let mut esperado = datos[8..].to_vec();
        esperado[2..7].fill(0xFF);
        assert_eq!(fuera, esperado);
    }

    /// Con un MKV real: `KUTRAL_MKV=/ruta.mkv cargo test --lib
    /// pista_tv::tests::mkv_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn mkv_real() {
        let ruta = std::env::var("KUTRAL_MKV").expect("KUTRAL_MKV");
        let n: usize = std::env::var("KUTRAL_AUDIO").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
        let mut leer = lector_archivo(&ruta);
        let p = preparar(&mut leer, !ruta.ends_with(".mp4"), n).expect("parche");
        eprintln!("parche en {} ({} bytes)", p.ini, p.bytes.len());
        if let Ok(sal) = std::env::var("KUTRAL_SALIDA") {
            let mut b = std::fs::read(&ruta).unwrap();
            b[p.ini as usize..p.ini as usize + p.bytes.len()].copy_from_slice(&p.bytes);
            std::fs::write(sal, b).unwrap();
        }
    }
}
