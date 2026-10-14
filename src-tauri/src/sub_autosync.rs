// Sincronización automática de subtítulos por audio.
//
// Idea (validada en un spike con Tears of Steel): dónde hay voz en el audio y
// dónde hay subtítulo en pantalla son dos máscaras en el tiempo; el desfase
// es el corrimiento que mejor las superpone.
//
// - Voz: VAD de WebRTC en modo 3 (el más estricto). Uno por energía falla:
//   la música cuenta como voz. Modos más permisivos dieron falsos positivos
//   con confianza alta.
// - Ventana: ≥240 s de audio. Con 120-180 s el pico no se distingue.
// - Confianza: pico contra el segundo mejor pico a más de 1 s. En el spike
//   los aciertos dieron ≥1.31 y los fallos ≤1.20: bajo el umbral NO se aplica.
//
// Solo corrige un desfase constante (lo común: otro release, logos al inicio).
// Un desfase que crece (25 vs 23.976 fps) necesita otra cosa.
//
// Algoritmo propio a propósito: alass hace esto y más, pero es GPL-3.0.

/// Resolución de las máscaras: 10 ms (lo que pide el VAD por trama a 16 kHz).
const HOP: f64 = 0.01;
pub const SAMPLE_RATE: u32 = 16_000;
const MUESTRAS_HOP: usize = (SAMPLE_RATE as usize) / 100;
/// Desfase máximo que se busca, en segundos, para cada lado.
const RANGO: f64 = 30.0;
/// Audio que se analiza. Menos no alcanza (ver arriba).
pub const VENTANA_S: f64 = 240.0;
/// Bajo esto no se aplica: mejor no tocar que poner un desfase equivocado.
pub const CONFIANZA_MIN: f64 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resultado {
    /// Cuánto van corridos los subtítulos respecto del audio (+ = tarde).
    /// Para corregir: sub-delay = -desfase.
    pub desfase: f64,
    pub confianza: f64,
}

impl Resultado {
    pub fn confiable(&self) -> bool {
        self.confianza >= CONFIANZA_MIN
    }
}

// ───────────────────────────── subtítulos ──────────────────────────────────

/// "00:01:02,345" / "00:01:02.345" / "01:02.345" / ASS "0:01:02.34".
fn tiempo(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    let partes: Vec<&str> = t.split(':').collect();
    let (h, m, s) = match partes.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Intervalos (inicio, fin) en segundos de un SRT, VTT o ASS/SSA.
pub fn intervalos(texto: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for linea in texto.lines() {
        let l = linea.trim();
        if let Some((a, b)) = l.split_once("-->") {
            // VTT puede traer ajustes de posición tras el fin: "… --> 00:02.000 line:90%".
            let b = b.split_whitespace().next().unwrap_or("");
            if let (Some(a), Some(b)) = (tiempo(a), tiempo(b)) {
                if b > a {
                    out.push((a, b));
                }
            }
        } else if let Some(resto) = l.strip_prefix("Dialogue:") {
            // ASS: "Dialogue: Capa,Inicio,Fin,Estilo,…"
            let campos: Vec<&str> = resto.splitn(4, ',').collect();
            if let [_, a, b, _] = campos.as_slice() {
                if let (Some(a), Some(b)) = (tiempo(a), tiempo(b)) {
                    if b > a {
                        out.push((a, b));
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────────── voz ───────────────────────────────────────

/// Máscara de voz cada 10 ms. Audio: s16 mono a 16 kHz.
pub fn mascara_voz(pcm: &[i16]) -> Vec<f32> {
    use webrtc_vad::{SampleRate, Vad, VadMode};
    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::VeryAggressive);
    pcm.as_chunks::<MUESTRAS_HOP>()
        .0
        .iter()
        .map(|t| f32::from(u8::from(vad.is_voice_segment(t).unwrap_or(false))))
        .collect()
}

// ───────────────────────────── correlación ─────────────────────────────────

/// Correlación de una ventana para cada corrimiento k (desfase = k·HOP −
/// RANGO). `voz` empieza en el segundo `t0` del video; `subs` son los tiempos
/// tal como vienen en el archivo (sin el sub-delay aplicado).
fn correlacion(voz: &[f32], subs: &[(f64, f64)], t0: f64) -> Option<Vec<f32>> {
    let n = voz.len();
    let pad = (RANGO / HOP).round() as usize;
    if n < 2 * pad || subs.is_empty() {
        return None;
    }
    // Máscara de subtítulos con margen de ±RANGO: el índice j es el segundo
    // t0 - RANGO + j·HOP del archivo de subtítulos.
    let largo = n + 2 * pad;
    let mut m = vec![0f32; largo];
    for &(a, b) in subs {
        let i = ((a - t0 + RANGO) / HOP).round();
        let j = ((b - t0 + RANGO) / HOP).round();
        let (i, j) = (i.max(0.0) as usize, (j.max(0.0) as usize).min(largo));
        if i < j {
            m[i..j].iter_mut().for_each(|x| *x = 1.0);
        }
    }
    if m.iter().all(|&x| x == 0.0) {
        return None; // ningún subtítulo cerca de la ventana
    }
    // Correlación de las señales centradas (como en el spike):
    //   c(k) = Σ (v_i - v̄)(m_{i+k} - m̄)
    //        = Σ v_i m_{i+k} - m̄ Σv - v̄ S(k) + n v̄ m̄,   S(k) = Σ_i m_{i+k}
    let v_media = voz.iter().sum::<f32>() / n as f32;
    let m_media = m.iter().sum::<f32>() / largo as f32;
    let suma_v: f32 = voz.iter().sum();
    let mut acumulada = vec![0f32; largo + 1];
    for (j, &x) in m.iter().enumerate() {
        acumulada[j + 1] = acumulada[j] + x;
    }
    // Solo importan los tramos con voz: recorrerlos es mucho menos que n.
    let con_voz: Vec<usize> = (0..n).filter(|&i| voz[i] > 0.0).collect();
    Some(
        (0..=2 * pad)
            .map(|k| {
                let cruzado: f32 = con_voz.iter().map(|&i| m[i + k]).sum();
                let s_k = acumulada[k + n] - acumulada[k];
                cruzado - m_media * suma_v - v_media * s_k + n as f32 * v_media * m_media
            })
            .collect(),
    )
}

/// Busca el desfase en una ventana (ver `alinear_varias`).
#[cfg(test)]
pub fn alinear(voz: &[f32], subs: &[(f64, f64)], t0: f64) -> Option<Resultado> {
    alinear_varias(&[(t0, voz.to_vec())], subs)
}

/// Busca el desfase sumando la correlación de varias ventanas `(t0, voz)`.
/// Una sola puede no alcanzar (música, poca conversación): con otra más el
/// pico verdadero se suma y los falsos no.
pub fn alinear_varias(ventanas: &[(f64, Vec<f32>)], subs: &[(f64, f64)]) -> Option<Resultado> {
    let mut c: Option<Vec<f32>> = None;
    for (t0, voz) in ventanas {
        let Some(cv) = correlacion(voz, subs, *t0) else { continue };
        match c.as_mut() {
            Some(acc) => acc.iter_mut().zip(cv).for_each(|(a, b)| *a += b),
            None => c = Some(cv),
        }
    }
    let c = c?;
    let (kmax, &pico) = c
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    let lejos = (1.0 / HOP) as usize;
    let segundo = c
        .iter()
        .enumerate()
        .filter(|(k, _)| k.abs_diff(kmax) > lejos)
        .map(|(_, &x)| x)
        .fold(f32::MIN, f32::max);
    // Con las señales centradas, el resto de los corrimientos puede quedar
    // negativo: ahí el pico no tiene competencia.
    let confianza = match (pico > 0.0, segundo > 0.0) {
        (true, true) => f64::from(pico / segundo),
        (true, false) => f64::INFINITY,
        _ => 0.0,
    };
    Some(Resultado {
        desfase: kmax as f64 * HOP - RANGO,
        confianza,
    })
}

/// Todo junto: ventanas de audio `(t0, s16 mono 16 kHz)` + texto del subtítulo.
pub fn sincronizar(audios: &[(f64, Vec<i16>)], texto_subs: &str) -> Option<Resultado> {
    let subs = intervalos(texto_subs);
    let ventanas: Vec<(f64, Vec<f32>)> =
        audios.iter().map(|(t0, pcm)| (*t0, mascara_voz(pcm))).collect();
    alinear_varias(&ventanas, &subs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_srt_vtt_y_ass() {
        let srt = "1\n00:00:23,000 --> 00:00:24,500\nHola\n\n2\n00:01:02,250 --> 00:01:03,000\nChao\n";
        assert_eq!(intervalos(srt), vec![(23.0, 24.5), (62.25, 63.0)]);
        let vtt = "WEBVTT\n\n00:05.000 --> 00:07.500 line:90%\nHola\n";
        assert_eq!(intervalos(vtt), vec![(5.0, 7.5)]);
        let ass = "[Events]\nDialogue: 0,0:00:01.50,0:00:03.00,Default,,0,0,0,,Hola, ¿qué tal?\n";
        assert_eq!(intervalos(ass), vec![(1.5, 3.0)]);
    }

    /// Voz sintética: tramos con voz donde hay subtítulo, corridos `real` s.
    fn caso(real: f64) -> (Vec<f32>, Vec<(f64, f64)>) {
        let subs: Vec<(f64, f64)> = (0..60)
            .map(|i| {
                // Duraciones y huecos irregulares: si fueran periódicos
                // habría varios picos iguales.
                let a = 3.0 + i as f64 * 4.0 + ((i * 7) % 5) as f64 * 0.37;
                (a, a + 1.2 + ((i * 3) % 4) as f64 * 0.4)
            })
            .collect();
        let n = (VENTANA_S / HOP) as usize;
        let mut voz = vec![0f32; n];
        for &(a, b) in &subs {
            // La voz ocurre `real` s ANTES que el subtítulo corrido.
            let (i, j) = (((a - real) / HOP) as isize, ((b - real) / HOP) as isize);
            for k in i.max(0)..j.min(n as isize) {
                voz[k as usize] = 1.0;
            }
        }
        (voz, subs)
    }

    #[test]
    fn encuentra_el_desfase_sintetico() {
        for real in [0.0, 2.3, -4.7, 12.0, -25.0] {
            let (voz, subs) = caso(real);
            let r = alinear(&voz, &subs, 0.0).expect("resultado");
            assert!((r.desfase - real).abs() < 0.05, "real {real} → {r:?}");
            assert!(r.confiable(), "real {real} → {r:?}");
        }
    }

    #[test]
    fn sin_relacion_no_es_confiable() {
        let (_, subs) = caso(0.0);
        // Voz que no tiene que ver: un tramo largo continuo.
        let mut voz = vec![0f32; (VENTANA_S / HOP) as usize];
        voz[5000..15000].iter_mut().for_each(|x| *x = 1.0);
        let r = alinear(&voz, &subs, 0.0).expect("resultado");
        assert!(!r.confiable(), "{r:?}");
    }

    #[test]
    fn ventana_corta_o_sin_subs_no_responde() {
        assert_eq!(alinear(&[1.0; 100], &[(0.0, 1.0)], 0.0), None);
        assert_eq!(alinear(&vec![1.0; 30_000], &[], 0.0), None);
        // Subs lejos de la ventana.
        assert_eq!(alinear(&vec![1.0; 30_000], &[(5000.0, 5001.0)], 0.0), None);
    }

    /// Audio real del spike (Tears of Steel, desde el segundo 0 y 330),
    /// con el .srt oficial en español corrido a propósito:
    ///   TOS_PCM=a.pcm TOS_PCM_330=m.pcm TOS_SRT=TOS-es.srt \
    ///   cargo test --release -- --ignored tears_of_steel
    #[test]
    #[ignore]
    fn tears_of_steel() {
        let leer_pcm = |var: &str, max_s: f64| -> Vec<i16> {
            let b = std::fs::read(std::env::var(var).expect(var)).unwrap();
            b.as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c))
                .take((max_s * SAMPLE_RATE as f64) as usize)
                .collect()
        };
        let srt = std::fs::read_to_string(std::env::var("TOS_SRT").expect("TOS_SRT")).unwrap();
        let base = intervalos(&srt);
        let correr = |d: f64| base.iter().map(|&(a, b)| (a + d, b + d)).collect::<Vec<_>>();
        let v0 = mascara_voz(&leer_pcm("TOS_PCM", VENTANA_S));
        let v330 = mascara_voz(&leer_pcm("TOS_PCM_330", VENTANA_S));
        let casos = [
            ("inicio", vec![(0.0, v0.clone())]),
            ("mitad", vec![(330.0, v330.clone())]),
            ("las dos", vec![(0.0, v0), (330.0, v330)]),
        ];
        for (nombre, ventanas) in &casos {
            for real in [0.0, 3.7, 15.0, -25.0] {
                let r = alinear_varias(ventanas, &correr(real)).unwrap();
                eprintln!("{nombre} real {real:+} → {:+.2} (confianza {:.2})", r.desfase, r.confianza);
                // Lo que importa: nunca un desfase equivocado con confianza.
                if r.confiable() {
                    assert!((r.desfase - real).abs() < 0.3, "{nombre} real {real} → {r:?}");
                }
            }
        }
    }
}
