// hosters_es.rs — sitios de streaming en español cuyas copias viven en
// hosters que Real-Debrid abre (Mega, Mediafire, 1fichier, VOE, Mixdrop…).
//
// No traen torrents: cada título/capítulo lista copias del MISMO archivo en
// varios hosters. Se devuelven como una fuente con `links` (igual que
// jkanime): el picker las prueba con rd_unrestrict hasta que una abra, y el
// descargador HTTP las baja. rd_unrestrict además descarta los .rar.
//
//  - henaojara y animeflv (.one): mismo motor. Anime con sub español. La
//    página del capítulo trae `data-dwn` (JSON con links directos) y una
//    lista AJAX de reproductores cuyo dato es la URL en hex.
//  - peliculaspro: películas y series (latino/castellano). La tabla de
//    descargas apunta a `?trdownload=…`, que responde un 302 al hoster.
//
// Lo viejo suele estar caído (los hosters borran); lo reciente vive. Por eso
// se juntan TODAS las copias y se prueban en orden.

use crate::kodios::{Source, VACIO};
use crate::sitios_es::{anio_ok, calidad, coincide, des_html, espejos, idioma, norm, pagina, sin_tags, sxe};

/// Hoster → link que Real-Debrid abre, con su preferencia (menor = antes).
/// None si RD no lo abre o es un comprimido a la vista.
fn para_rd(url: &str) -> Option<(u8, String)> {
    let u = url.trim();
    let l = u.to_ascii_lowercase();
    if [".rar", ".zip", ".7z"].iter().any(|e| l.contains(e)) {
        return None;
    }
    let id_tras = |marca: &str| -> Option<String> {
        let i = l.find(marca)? + marca.len();
        let id: String = u[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        (!id.is_empty()).then_some(id)
    };
    if l.contains("mega.nz") || l.contains("mega.co.nz") {
        // Formatos: /file/ID#KEY, /embed/ID#KEY, /#!ID!KEY, /embed#!ID!KEY.
        let r = u.split_once("mega").map(|(_, r)| r).unwrap_or("");
        let r = r.split_once('/').map(|(_, r)| r).unwrap_or("");
        let r = r.trim_start_matches("file/").trim_start_matches("embed/").trim_start_matches("embed");
        let r = r.trim_start_matches('#').trim_start_matches('!').trim_start_matches('/');
        let (id, key) = r.split_once(['#', '!'])?;
        let key = key.trim_start_matches('!');
        if id.is_empty() || key.is_empty() {
            return None;
        }
        return Some((0, format!("https://mega.nz/file/{id}#{key}")));
    }
    if l.contains("mediafire.com") {
        return Some((1, u.to_string()));
    }
    if l.contains("1fichier.com") {
        return Some((2, u.to_string()));
    }
    if l.contains("mixdrop") {
        let id = id_tras("/f/").or_else(|| id_tras("/e/"))?;
        return Some((3, format!("https://mixdrop.ag/f/{id}")));
    }
    if l.contains("voe.sx") {
        let id = id_tras("/e/").or_else(|| id_tras("voe.sx/"))?;
        return Some((4, format!("https://voe.sx/{id}")));
    }
    if l.contains("streamtape") {
        let id = id_tras("/e/").or_else(|| id_tras("/v/"))?;
        return Some((5, format!("https://streamtape.com/v/{id}/")));
    }
    for (dom, p) in [("rapidgator", 6), ("nitroflare", 6), ("katfile", 7), ("ddownload", 7), ("vidoza", 8)] {
        if l.contains(dom) {
            return Some((p, u.to_string()));
        }
    }
    None
}

/// Ordena por preferencia y saca repetidos.
fn ordenar(crudos: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<(u8, String)> = crudos.into_iter().filter_map(|u| para_rd(&u)).collect();
    v.sort_by_key(|(p, _)| *p);
    let mut out: Vec<String> = Vec::new();
    for (_, l) in v {
        if !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

fn hex_texto(h: &str) -> Option<String> {
    let h = h.trim();
    if h.is_empty() || !h.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect();
    String::from_utf8(bytes?).ok()
}

fn fuente(sitio: &str, titulo: String, q: &str, hardsub: Option<&str>, links: Vec<String>) -> Source {
    Source {
        source: sitio.into(),
        title: titulo,
        magnet: None,
        url: None,
        info_hash: None,
        size_bytes: None,
        seeders: None,
        quality: calidad(q),
        rd_cached: None,
        hardsub: hardsub.map(String::from),
        file_idx: None,
        filename: None,
        links: Some(links),
    }
}

/// Valores de `attr="…"` en todo el html.
fn atributos<'a>(html: &'a str, attr: &str) -> Vec<&'a str> {
    let marca = format!("{attr}=\"");
    html.match_indices(&marca)
        .filter_map(|(i, _)| {
            let r = &html[i + marca.len()..];
            r.find('"').map(|f| &r[..f])
        })
        .collect()
}

// ---- henaojara / animeflv --------------------------------------------------

pub struct SitioFlv {
    /// Id del buscador (Configuración → Fuentes).
    pub id: &'static str,
    /// Canal en Balandro (dominio vigente).
    pub canal: &'static str,
    pub espejos: &'static [&'static str],
    /// Ruta del AJAX de reproductores (`/hj` en henaojara, `/flv` en animeflv).
    pub ajax: &'static str,
}

pub const HENAOJARA: SitioFlv =
    SitioFlv { id: "henaojara", canal: "henaojaran", espejos: &["https://ww1.henaojara.net"], ajax: "hj" };
pub const ANIMEFLV: SitioFlv =
    SitioFlv { id: "animeflv", canal: "animeflvone", espejos: &["https://vww.animeflv.one"], ajax: "flv" };

/// Resultados de la búsqueda: (slug, título).
fn flv_resultados(html: &str) -> Vec<(String, String)> {
    let marca = "<h3 class=\"h\"><a href=\"./anime/";
    html.match_indices(marca)
        .filter_map(|(i, _)| {
            let r = &html[i + marca.len()..];
            let slug = &r[..r.find('"')?];
            let a = r.find('>')?;
            let b = r[a..].find("</a>")?;
            Some((slug.to_string(), des_html(r[a + 1..a + b].trim())))
        })
        .collect()
}

pub async fn flv_search(sitio: &SitioFlv, titulos: &[String], episode: Option<u32>) -> Result<Vec<Source>, String> {
    let n = episode.unwrap_or(1);
    let esp = espejos(sitio.canal, sitio.espejos).await;
    let mut elegido: Option<(String, String, String)> = None;
    for t in titulos.iter().filter(|t| !t.trim().is_empty()) {
        let q = urlencoding::encode(t.trim());
        let (base, html) = pagina(&esp, &format!("/animes?buscar={q}")).await?;
        let r = flv_resultados(&html);
        // Título exacto, o slug igual al título (el sitio usa el romaji).
        if let Some((slug, tit)) =
            r.into_iter().find(|(s, tit)| coincide(tit, titulos) || titulos.iter().any(|x| norm(x) == norm(s)))
        {
            elegido = Some((base, slug, tit));
            break;
        }
    }
    let (base, slug, tit) = elegido.ok_or(format!("{VACIO}{} sin coincidencia", sitio.id))?;
    let base = base.trim_end_matches('/').to_string();
    let url = format!("{base}/ver/{slug}-{n}");
    let html = crate::net::get_text(&url, Some(&base)).await?;

    let mut crudos: Vec<String> = Vec::new();
    // A) data-dwn: JSON con links directos a hosters.
    for v in atributos(&html, "data-dwn") {
        if let Ok(lista) = serde_json::from_str::<Vec<String>>(&des_html(v)) {
            crudos.extend(lista);
        }
    }
    // B) Reproductores por AJAX: cada opción es la URL del embed en hex.
    if let Some(hex) = atributos(&html, "data-encrypt").first() {
        let cli = crate::net::scrape_client()?;
        let r = cli
            .post(format!("{base}/{}", sitio.ajax))
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Referer", &url)
            .form(&[("acc", "opt"), ("i", *hex)])
            .send()
            .await;
        match r {
            Ok(resp) => {
                if let Ok(cuerpo) = resp.text().await {
                    crudos.extend(atributos(&cuerpo, "encrypt").into_iter().filter_map(hex_texto));
                }
            }
            Err(e) => eprintln!("[{}] reproductores: {e}", sitio.id),
        }
    }
    let links = ordenar(crudos);
    eprintln!("[{}] {slug} ep {n}: {} copias para RD", sitio.id, links.len());
    if links.is_empty() {
        return Err(format!("{VACIO}{} {slug}/{n}: sin copias en hosters que abra Real-Debrid", sitio.id));
    }
    // Sub español quemado: el sitio no distingue por opción.
    Ok(vec![fuente(sitio.id, format!("{tit} — Episodio {n} [Subtitulado español]"), "", Some("lat"), links)])
}

// ---- peliculaspro ----------------------------------------------------------

const PELISPRO: &[&str] = &["https://pelispro.net"];

/// Tarjetas de la búsqueda: (url, título, año).
fn pro_resultados(html: &str) -> Vec<(String, String, Option<u32>)> {
    html.split("<article class=\"post")
        .skip(1)
        .filter_map(|bloque| {
            let bloque = &bloque[..bloque.find("</article>").unwrap_or(bloque.len())];
            let lnk = bloque.find("class=\"lnk-blk\"")?;
            let h = bloque[..lnk].rfind("href=\"")? + 6;
            let url = bloque[h..].split('"').next()?.to_string();
            let t = bloque.find("entry-title\">")? + 13;
            let titulo = des_html(bloque[t..].split('<').next()?.trim());
            let anio = bloque
                .find("class=\"year\">")
                .and_then(|i| bloque[i + 13..].split('<').next())
                .and_then(|y| y.trim().parse().ok());
            Some((url, titulo, anio))
        })
        .collect()
}

/// Filas de la tabla de descargas: (hoster, idioma, calidad, url trdownload).
fn pro_filas(html: &str) -> Vec<(String, String, String, String)> {
    html.split("<span class=\"num\">")
        .skip(1)
        .filter_map(|parte| {
            let fila = &parte[..parte.find("</tr>").unwrap_or(parte.len())];
            let celdas: Vec<String> = fila.split("<td").map(|c| sin_tags(&format!("<td{c}"))).collect();
            let href = atributos(fila, "href").into_iter().find(|h| h.contains("trdownload"))?;
            let hoster = celdas.first()?.trim_start_matches(|c: char| c == '#' || c.is_ascii_digit()).trim().to_string();
            Some((hoster, celdas.get(1).cloned().unwrap_or_default(), celdas.get(2).cloned().unwrap_or_default(), des_html(href)))
        })
        .collect()
}

/// Sigue un `?trdownload=` hasta el hoster (sin seguir la redirección).
async fn destino(url: &str, referer: &str) -> Option<String> {
    let cli = crate::net::scrape_client_sin_redireccion().ok()?;
    let r = cli.get(url).header("Referer", referer).send().await.ok()?;
    r.headers().get(reqwest::header::LOCATION)?.to_str().ok().map(String::from)
}

pub async fn pelispro_search(
    titulos: &[String],
    anio: Option<u32>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Source>, String> {
    let serie = season.zip(episode);
    let esp = espejos("peliculaspro", PELISPRO).await;
    let mut ficha: Option<(String, String, String)> = None; // (base, url, título)
    for t in titulos.iter().filter(|t| !t.trim().is_empty()) {
        let q = urlencoding::encode(t.trim());
        let (base, html) = pagina(&esp, &format!("/?s={q}")).await?;
        let cands: Vec<(String, String, Option<u32>)> = pro_resultados(&html)
            .into_iter()
            .filter(|(u, tit, a)| match serie {
                None => u.contains("/pelicula/") && coincide(tit, titulos) && anio_ok(anio, *a),
                Some(_) => u.contains("/serie/") && coincide(tit, titulos),
            })
            .collect();
        // Sin año, dos películas homónimas no se distinguen: solo si es única.
        if serie.is_none() && anio.is_none() && cands.len() > 1 {
            continue;
        }
        if let Some((u, tit, _)) = cands.into_iter().next() {
            ficha = Some((base.trim_end_matches('/').to_string(), u, tit));
            break;
        }
    }
    let (base, mut url, tit) = ficha.ok_or(format!("{VACIO}peliculaspro sin coincidencia"))?;
    let mut html = crate::net::get_text(&url, Some(&base)).await?;
    let mut nombre = tit.clone();
    if let Some((s, e)) = serie {
        // Capítulos de la temporada por AJAX; el link del episodio sigue a "SxE".
        let post = atributos(&html, "data-post").first().map(|p| p.to_string()).ok_or("peliculaspro: sin data-post")?;
        let cli = crate::net::scrape_client()?;
        let temporada = s.to_string();
        let lista = cli
            .post(format!("{base}/wp-admin/admin-ajax.php"))
            .header("Referer", &url)
            .form(&[("action", "action_select_season"), ("season", temporada.as_str()), ("post", post.as_str())])
            .send()
            .await
            .map_err(|e| format!("peliculaspro temporada: {e}"))?
            .text()
            .await
            .map_err(|e| format!("peliculaspro temporada: {e}"))?;
        let ep_url = lista
            .split("num-epi\">")
            .skip(1)
            .find(|b| sxe(b.split('<').next().unwrap_or("")) == Some((s, e)))
            .and_then(|b| atributos(b, "href").into_iter().next().map(String::from))
            .ok_or(format!("{VACIO}peliculaspro sin el capítulo {s}x{e}"))?;
        html = crate::net::get_text(&ep_url, Some(&url)).await?;
        url = ep_url;
        nombre = format!("{tit} T{s}E{e}");
    }

    // Se resuelven en paralelo: cada fila es un 302 al hoster.
    let filas = pro_filas(&html);
    let mut tareas = tokio::task::JoinSet::new();
    for (i, (_, idi, q, td)) in filas.into_iter().enumerate().take(12) {
        let referer = url.clone();
        tareas.spawn(async move { (i, idi, q, destino(&td, &referer).await) });
    }
    let mut resueltas: Vec<(usize, String, String, String)> = Vec::new();
    while let Some(r) = tareas.join_next().await {
        if let Ok((i, idi, q, Some(d))) = r {
            resueltas.push((i, idi, q, d));
        }
    }
    resueltas.sort_by_key(|r| r.0);

    // Una fuente por idioma: son copias del mismo release.
    let mut grupos: Vec<(String, String, Vec<String>)> = Vec::new();
    for (_, idi, q, d) in resueltas {
        let idi = idioma(&idi);
        match grupos.iter_mut().find(|g| g.0 == idi) {
            Some(g) => g.2.push(d),
            None => grupos.push((idi, q, vec![d])),
        }
    }
    let out: Vec<Source> = grupos
        .into_iter()
        .filter_map(|(idi, q, crudos)| {
            let links = ordenar(crudos);
            (!links.is_empty())
                .then(|| fuente("peliculaspro", format!("{nombre} [{}] [{idi}]", if q.is_empty() { "?" } else { &q }), &q, None, links))
        })
        .collect();
    if out.is_empty() {
        return Err(format!("{VACIO}peliculaspro: sin copias que abra Real-Debrid"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosters_se_normalizan_para_rd() {
        assert_eq!(para_rd("https://mega.nz/#!FadkTLxJ!Nsge5u").unwrap().1, "https://mega.nz/file/FadkTLxJ#Nsge5u");
        assert_eq!(para_rd("https://mega.nz/embed#!AbC!kEy").unwrap().1, "https://mega.nz/file/AbC#kEy");
        assert_eq!(para_rd("https://mega.nz/file/4NtzXJQC#hLvEb").unwrap().1, "https://mega.nz/file/4NtzXJQC#hLvEb");
        assert_eq!(para_rd("https://voe.sx/e/xaescdskxic9").unwrap().1, "https://voe.sx/xaescdskxic9");
        assert_eq!(para_rd("https://voe.sx/mlgqhambnbc0").unwrap().1, "https://voe.sx/mlgqhambnbc0");
        assert_eq!(para_rd("https://mixdrop.ps/e/k0p4691xs3jdvr").unwrap().1, "https://mixdrop.ag/f/k0p4691xs3jdvr");
        assert_eq!(para_rd("https://streamtape.com/e/m3BZ/x.mkv").unwrap().1, "https://streamtape.com/v/m3BZ/");
        assert!(para_rd("https://www.mediafire.com/file/x/SPRM4N2025-1080p.rar/file").is_none());
        assert!(para_rd("https://bysesukior.com/d/qmfrdinabp92").is_none());
        assert!(para_rd("https://doodstream.com/d/3at0").is_none());
    }

    #[test]
    fn orden_y_repetidos() {
        let v = ordenar([
            "https://voe.sx/e/a".to_string(),
            "https://mega.nz/#!X!Y".to_string(),
            "https://voe.sx/a".to_string(),
            "https://filemoon.sx/e/z".to_string(),
        ]);
        assert_eq!(v, ["https://mega.nz/file/X#Y", "https://voe.sx/a"]);
    }

    #[test]
    fn hex_de_reproductores() {
        assert_eq!(hex_texto("333837302d31").as_deref(), Some("3870-1"));
        assert!(hex_texto("abc").is_none());
    }

    #[test]
    fn parseo_de_paginas() {
        let b = r#"<article class="li"><h3 class="h"><a href="./anime/sousou-no-frieren" title="Ver">Sousou no Frieren</a></h3></article>"#;
        assert_eq!(flv_resultados(b), [("sousou-no-frieren".to_string(), "Sousou no Frieren".to_string())]);
        let p = r#"<article class="post dfx fcl movies"><h2 class="entry-title">Superman</h2><span class="year">2025</span>
                   <a href="https://pelispro.net/pelicula/superman-2" class="lnk-blk"></a></article>"#;
        assert_eq!(pro_resultados(p), [("https://pelispro.net/pelicula/superman-2".to_string(), "Superman".to_string(), Some(2025))]);
        let f = r#"<tr><td><span class="num">#01</span> Mega</td><td>Latino</td><td><span>1080P</span></td>
                   <td><a href="https://pelispro.net/?trdownload=0&#038;trid=29012">Descargar</a></td></tr>"#;
        let filas = pro_filas(f);
        assert_eq!(filas.len(), 1);
        assert_eq!(filas[0].0, "Mega");
        assert_eq!(filas[0].1, "Latino");
        assert_eq!(filas[0].3, "https://pelispro.net/?trdownload=0&trid=29012");
    }
}
