// Trailers: TMDb (claves de YouTube), yt-dlp para sacar el mp4 directo (el
// iframe de YouTube no funciona en el webview) y iTunes como respaldo.

use serde::{Deserialize, Serialize};

use crate::{client, fetch_json, winproc, LANG, TMDB_BASE};

#[derive(Serialize, Deserialize)]
pub struct VideoItem {
    pub key: String,
    pub name: String,
    pub site: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub official: bool,
}

#[derive(Deserialize)]
pub(crate) struct VideosResp {
    pub(crate) results: Vec<VideoItem>,
}

#[tauri::command]
pub async fn tmdb_videos(
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<Vec<VideoItem>, String> {
    if api_key.is_empty() {
        return Err("falta api key".into());
    }
    if media_type != "movie" && media_type != "tv" {
        return Err("media_type inválido".into());
    }
    // 1ra pasada: idioma local
    let url_local = format!(
        "{}/{}/{}/videos?api_key={}&language={}",
        TMDB_BASE, media_type, id, api_key, LANG
    );
    let mut vids: VideosResp = fetch_json(&url_local).await.unwrap_or(VideosResp { results: vec![] });
    // Fallback inglés si no hay nada (común para trailers)
    if vids.results.is_empty() {
        let url_en = format!(
            "{}/{}/{}/videos?api_key={}&language=en-US",
            TMDB_BASE, media_type, id, api_key
        );
        if let Ok(v) = fetch_json::<VideosResp>(&url_en).await {
            vids = v;
        }
    }
    // Filtro: solo YouTube + Trailer/Teaser, preferir oficiales
    let mut filtered: Vec<VideoItem> = vids
        .results
        .into_iter()
        .filter(|v| v.site == "YouTube" && (v.kind == "Trailer" || v.kind == "Teaser"))
        .collect();
    filtered.sort_by(|a, b| {
        let order = |k: &str| if k == "Trailer" { 0 } else { 1 };
        b.official.cmp(&a.official).then(order(&a.kind).cmp(&order(&b.kind)))
    });
    Ok(filtered)
}

// ---------- Trailers ----------

/// Quita acentos de los caracteres latinos comunes. No cubre todo Unicode:
/// solo lo que aparece en títulos de TMDb en es-ES / en-US.
fn deaccent(c: char) -> Option<&'static str> {
    Some(match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => "a",
        'é' | 'è' | 'ë' | 'ê' => "e",
        'í' | 'ì' | 'ï' | 'î' => "i",
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => "o",
        'ú' | 'ù' | 'ü' | 'û' => "u",
        'ñ' => "n",
        'ç' => "c",
        'ø' => "o",
        'æ' => "ae",
        'ß' => "ss",
        _ => return None,
    })
}

/// Normaliza un título para comparar: minúsculas, sin acentos, sin puntuación,
/// espacios colapsados. "Amélie: Edición Especial" → "amelie edicion especial".
fn norm_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.to_lowercase().chars() {
        let mapped = deaccent(ch);
        let piece: &str = match mapped {
            Some(m) => m,
            None if ch.is_alphanumeric() => {
                out.push(ch);
                prev_space = false;
                continue;
            }
            None => {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
                continue;
            }
        };
        out.push_str(piece);
        prev_space = false;
    }
    out.trim().to_string()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 1.0 = idénticos, 0.0 = nada en común.
fn similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let d = levenshtein(&ac, &bc) as f32;
    1.0 - d / ac.len().max(bc.len()) as f32
}

/// ¿El video de YouTube se puede embeber?
///
/// oEmbed responde 200 solo si existe Y permite embed. 401/403 = embed
/// deshabilitado por el dueño (caso típico de trailers de distribuidoras, que
/// dentro del webview aparecen como "error 153"), 404 = borrado o privado.
/// Sin esta comprobación el iframe carga una pantalla negra y la app cree que
/// el trailer se está viendo.
/// (existe, permite embed). 200 = ambos; 401/403 = existe pero el dueño
/// bloqueó el embed (yt-dlp igual lo puede reproducir); el resto = borrado,
/// privado o key inválida.
async fn yt_probe(key: &str) -> (bool, bool) {
    let url = format!(
        "https://www.youtube.com/oembed?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{}&format=json",
        urlencoding::encode(key)
    );
    let c = match client() {
        Ok(c) => c,
        // Sin cliente HTTP no podemos descartar nada: lo damos por existente.
        Err(_) => return (true, false),
    };
    match c.get(&url).send().await {
        Ok(r) if r.status().is_success() => (true, true),
        Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => (true, false),
        Ok(_) => (false, false),
        // Fallo de red: no asumir que el video no existe.
        Err(_) => (true, false),
    }
}

#[derive(Serialize)]
pub struct TrailerKey {
    pub key: String,
    pub embeddable: bool,
}

/// Primer trailer de TMDb que además se puede embeber.
///
/// Si ninguno de los candidatos pasa la prueba devuelve el primero con
/// `embeddable: false`: el frontend lo abre en el navegador externo en vez de
/// mostrar un iframe roto.
#[tauri::command]
pub async fn tmdb_trailer_key(
    media_type: String,
    id: u64,
    api_key: String,
) -> Result<Option<TrailerKey>, String> {
    let vids = tmdb_videos(media_type, id, api_key).await?;
    let mut first: Option<String> = None;
    // Tope de 5: cada comprobación es un request extra y la lista viene
    // ordenada por relevancia (oficial + Trailer antes que Teaser). Se salta
    // solo lo borrado: un video sin permiso de embed sigue sirviendo, porque
    // el camino normal de reproducción es yt-dlp, no el iframe.
    for v in vids.iter().take(5) {
        if first.is_none() {
            first = Some(v.key.clone());
        }
        let (exists, embeddable) = yt_probe(&v.key).await;
        if exists {
            return Ok(Some(TrailerKey { key: v.key.clone(), embeddable }));
        }
    }
    Ok(first.map(|key| TrailerKey { key, embeddable: false }))
}

/// Ruta del binario yt-dlp: vendor/ del bundle primero, PATH después.
fn ytdlp_bin(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    #[cfg(windows)]
    let exe = "yt-dlp.exe";
    #[cfg(not(windows))]
    let exe = "yt-dlp";

    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join(exe));
    }
    cands.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join(exe));
        }
    }
    for c in cands {
        if c.exists() {
            return c.to_string_lossy().into_owned();
        }
    }
    exe.to_string()
}

/// URLs directas del trailer de YouTube, resueltas con yt-dlp.
///
/// Una sola ejecución de yt-dlp por trailer: la misma llamada dice si el video
/// es reproducible (antes `yt_playable`) y entrega las URLs que se le pasan a
/// mpv. Correrlo dos veces —una para preguntar, otra dentro de mpv vía
/// ytdl_hook— duplicaba la espera y las chances de topar el rate-limit de
/// YouTube, que es lo que dejaba el QR en pantalla con trailers que sí
/// funcionaban.
///
/// YouTube ya casi no entrega formatos progresivos: video y audio vienen en
/// streams DASH separados, así que `-g` devuelve dos líneas y mpv las junta con
/// `--audio-file` (proceso) o `audio-files` (embed).
///
/// `video` vacío = reproducible pero hay que dejárselo a ytdl_hook: las listas
/// de mpv se separan por comas y una URL con coma literal las rompería.
/// Err = no reproducible (bloqueo de edad, región, DRM, o yt-dlp caído); el
/// front cae a Apple y si no, al QR.
#[derive(Serialize)]
pub struct TrailerSrc {
    pub video: String,
    pub audio: String,
}

#[tauri::command]
pub async fn yt_trailer_src(app: tauri::AppHandle, key: String) -> Result<TrailerSrc, String> {
    let k = key.trim().to_string();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("key inválida".into());
    }
    let bin = ytdlp_bin(&app);
    let url = format!("https://www.youtube.com/watch?v={}", k);

    let mut cmd = tokio::process::Command::new(&bin);
    winproc::hide_console_tokio(&mut cmd);
    let fut = cmd
        .args([
            "--no-playlist",
            "--no-warnings",
            "--socket-timeout", "10",
            "-f", "bv*+ba/b",
            "-g",
            &url,
        ])
        .kill_on_drop(true)
        .output();
    // Tope duro: si yt-dlp se cuelga, el menú no puede quedarse esperando.
    let out = match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(r) => r.map_err(|e| format!("yt-dlp: {}", e))?,
        Err(_) => return Err("yt-dlp: timeout".into()),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("yt-dlp: {}", err.lines().next().unwrap_or("").trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let urls: Vec<&str> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("http"))
        .collect();
    let video = match urls.first() {
        Some(v) => v.to_string(),
        None => return Err("yt-dlp: sin URL".into()),
    };
    let audio = urls.get(1).map(|a| a.to_string()).unwrap_or_default();
    // Coma literal: no se puede meter en una lista de mpv. Reproducible igual,
    // pero por el camino de ytdl_hook.
    if video.contains(',') || audio.contains(',') {
        return Ok(TrailerSrc { video: String::new(), audio: String::new() });
    }
    Ok(TrailerSrc { video, audio })
}

#[derive(Serialize)]
pub struct AppleTrailer {
    pub url: String,
    pub title: String,
    pub year: Option<String>,
}

/// Trailer desde iTunes (mp4 directo, sin restricciones de embed).
///
/// iTunes solo se puede consultar por texto — no tiene IDs de TMDb/IMDb — así
/// que el match tiene que ser estricto o devuelve cualquier cosa: su búsqueda
/// es difusa y para un título que no está en el catálogo US (cine coreano,
/// indio, europeo) responde 25 películas sin relación. Antes se aceptaba "el
/// primer candidato" y por eso salía el trailer de otra película.
///
/// Reglas: título normalizado con similitud >= 0.85 y año dentro de ±1. Sin
/// año confiable se exige similitud >= 0.95. Si nada cumple → `None`.
#[tauri::command]
pub async fn apple_trailer(
    title: String,
    original_title: String,
    year: String,
    media_type: String,
) -> Result<Option<AppleTrailer>, String> {
    #[derive(Deserialize)]
    struct Resp {
        results: Vec<Item>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Item {
        kind: Option<String>,
        track_name: Option<String>,
        preview_url: Option<String>,
        release_date: Option<String>,
    }

    // El título original va primero: iTunes US indexa en inglés / idioma
    // original, no con el título en español de TMDb.
    let mut queries: Vec<String> = Vec::new();
    for cand in [original_title.trim(), title.trim()] {
        if !cand.is_empty() && !queries.iter().any(|q| q.eq_ignore_ascii_case(cand)) {
            queries.push(cand.to_string());
        }
    }
    if queries.is_empty() {
        return Ok(None);
    }

    // Una serie nunca debe matchear una película homónima.
    let want_kinds: &[&str] = if media_type == "tv" {
        &["tv-episode", "tv-season"]
    } else {
        &["feature-movie"]
    };
    let want_year: Option<i32> = year.get(..4).and_then(|y| y.parse().ok());
    let targets: Vec<String> = queries
        .iter()
        .map(|q| norm_title(q))
        .filter(|s| !s.is_empty())
        .collect();
    let min_sim = if want_year.is_some() { 0.85 } else { 0.95 };

    for q in &queries {
        // Sin entity= : Apple bugea y devuelve vacío con entity=movie.
        let url = format!(
            "https://itunes.apple.com/search?term={}&country=us&limit=25",
            urlencoding::encode(q)
        );
        let r: Resp = match fetch_json::<Resp>(&url).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        let mut best: Option<(f32, &Item)> = None;
        for i in r.results.iter() {
            if i.preview_url.as_deref().is_none_or(|u| u.is_empty()) {
                continue;
            }
            if !i.kind.as_deref().is_some_and(|k| want_kinds.contains(&k)) {
                continue;
            }
            let name = norm_title(i.track_name.as_deref().unwrap_or(""));
            if name.is_empty() {
                continue;
            }
            let sim = targets
                .iter()
                .map(|t| similarity(t, &name))
                .fold(0.0f32, f32::max);
            if sim < min_sim {
                continue;
            }
            // iTunes fecha el estreno digital, no el de cines → tolerancia ±1.
            let iy: Option<i32> = i
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            let year_ok = match (want_year, iy) {
                (Some(w), Some(v)) => (w - v).abs() <= 1,
                (Some(_), None) => false,
                (None, _) => true,
            };
            if !year_ok {
                continue;
            }
            if best.is_none_or(|(s, _)| sim > s) {
                best = Some((sim, i));
            }
        }

        if let Some((_, found)) = best {
            return Ok(Some(AppleTrailer {
                url: found.preview_url.clone().unwrap_or_default(),
                title: found.track_name.clone().unwrap_or_default(),
                year: found
                    .release_date
                    .as_ref()
                    .and_then(|d| d.get(..4).map(|s| s.to_string())),
            }));
        }
    }

    Ok(None)
}
