# Audiolibros y videoclips en Kütral

Pedido externo (2026-11-01). Sondeo + prueba de lo riesgoso; nada implementado.

## Audiolibros — plan

### Fuente: LibriVox vía Internet Archive

Dominio público, sin clave, API JSON estable.

- Catálogo: `archive.org/advancedsearch.php`,
  `q=collection:librivoxaudio AND language:(spanish OR Spanish OR spa)` (o
  `english OR English OR eng`).
  **565 libros en español y 19.159 en inglés** (21.923 en total). Orden por `downloads desc` da
  buenos "destacados": Quijote, Fábulas de Esopo, Alarcón, Grimm…
- Ficha: `archive.org/metadata/<identifier>` → archivos. Cada libro trae:
  - un mp3 por capítulo (`64Kbps MP3`, con `title` y `length`),
  - portada `.jpg` (o `archive.org/services/img/<id>`),
  - a veces `.m4b` con capítulos y `.m3u`.
- Descarga: `archive.org/download/<id>/<archivo>` → 302 a `dnXXX.archive.org`,
  `audio/mpeg`, `accept-ranges: bytes`, CORS abierto. Sirve para `<audio>`,
  mpv y Chromecast tal cual.
- Niños: `subject` es texto libre ("cuentos; cuentos de hadas; grimm"), solo
  77 en español con etiquetas de niños. Mejor la API de LibriVox
  (`librivox.org/api/feed/audiobooks?format=json`, trae `genres`, p. ej.
  "Children's Fiction") cruzada por `url_iarchive`. Mismo criterio que
  `ninos.ts`: grueso a propósito.

Fuera del plan: audiolibros comerciales (AudioBookBay + RD). Zona gris, sin
API, scraping frágil. Que quede claro a quien lo pidió: **clásicos, no
novedades**.

### Fases

1. **Catálogo y escucha básica**
   - `src-tauri/src/audiolibros.rs`: `audiolibros_destacados(idioma, pagina)`,
     `audiolibros_buscar(q, idioma, pagina)`, `audiolibro_detalle(id)` →
     título, autor, descripción (HTML limpio), portada, capítulos
     `[{titulo, url, segundos}]`. Reusar `client`/`fetch_json` y la caché.
   - Ruta propia `src/routes/audiolibros/+page.svelte`, como `/iptv` (NO dentro
     de `+page.svelte`, que ya pasa las 3.900 líneas). Tarjeta en el inicio
     junto a la de IPTV.
   - Reproducción en el webview con `<audio>` (como IPTV usa `<video>`), sin
     mpv: no hay video, y esquiva el WebView2 que no se oculta en Windows.
     `inhibirReposo("audiolibro", …)` mientras suena.
   - Siguiente capítulo solo. Navegable con flechas/Enter (`mejor` de nav.ts).
2. **Escucha cómoda**
   - Velocidad 0,8×–2×, ±30 s, temporizador para dormir (15/30/60 min, fin de
     capítulo).
   - Retomar: `watch_history` ya sirve sin migración →
     clave `archive:<identifier>`, `season = -1`, `episode = n.º capítulo`,
     `media_type = "audiolibro"`. **Ojo**: filtrar `media_type` en
     "Continuar viendo" y demás lectores del historial, que hoy asumen
     TMDb/AniList.
   - Carrusel "Seguir escuchando" en `/audiolibros`.
   - Perfil de niños (géneros LibriVox).
3. **TV y sin conexión**
   - Cast: `cast.rs::content_type` hoy cae en `video/mp4` para todo lo que no
     reconoce → agregar `.mp3` → `audio/mpeg`, `.m4b` → `audio/mp4`.
     Chromecast muestra la portada (`imagen`). DLNA: archive es https → pasarela.
   - Descargar libro para escuchar sin conexión (cola de descargas existente).

### Decisiones (usuario, 2026-11-01)

- Botón: **"Descubrir"**, igual que en el resto de la app.
- Idiomas: **español e inglés** (565 + 19.159 en LibriVox). Cada tarjeta y
  ficha muestra su idioma (etiqueta "ES"/"EN"), y el catálogo se puede
  filtrar por idioma. `language` en Archive viene en variantes
  (`spa`/`Spanish`/`spanish`, `eng`/`English`) → normalizar en el backend.

## Videoclips — prueba (2026-11-01)

yt-dlp vendorizado 2026.10.26, desde esta red (casa, 192.168.100.x).

### Aguante de YouTube: OK

10 videoclips seguidos (Shakira, Los Prisioneros, Bad Bunny, Queen, Mon
Laferte, Daft Punk, Café Tacvba, Billie Eilish, Cerati, Rosalía) = 30
llamadas a yt-dlp en ~3,5 min. **10/10 encontrados y resueltos, cero
bloqueos.** El primer resultado de `ytsearch1:<artista> <tema> official video`
fue siempre el video oficial.

| Paso | Promedio | Máx |
|---|---|---|
| Buscar (`ytsearch1`, `--flat-playlist`) | 3,6 s | 3,8 s |
| Resolver 360p progresivo (`-f 18 -g`) | 7,5 s | 7,9 s |
| Resolver 1080p DASH (`bv*[height<=1080]+ba`) | 7,4 s | 8 s |

- 7,5 s por clic es mucho: resolver todo en una sola llamada (`-J`) y
  precargar al enfocar la tarjeta (como `precarga.ts`). Cachear por id
  (las URLs de googlevideo duran ~6 h).
- En el PC el 1080p ya funciona: mpv + `ytdl_hook` mezcla video y audio
  (es lo que usan los trailers, `mpv_embed/mod.rs`).
- Una sola tanda no prueba el uso real: el bloqueo de YouTube aparece por IP
  y por volumen. El riesgo sigue (ya pasó con los trailers).

### TV: sin probar (el usuario prefirió no usar la TV), pero el panorama quedó claro

- **360p (formato 18)**: único progresivo con audio. Debería andar directo
  por Cast (mp4 H.264/AAC, igual que las URLs de RD). Calidad baja.
- **HLS**: YouTube da HLS hasta 2160p (avc1 y vp9), pero el video viene
  **sin audio** (audio en grupo aparte del maestro) y el manifiesto **no trae
  cabeceras CORS**. El Default Media Receiver de Chromecast carga HLS por
  XHR → casi seguro falla directo. Opción: servir maestro + segmentos por la
  pasarela 8765 agregando CORS (el PC hace de relevo).
- **Regla "una sola versión PC/TV"**: si el PC suena en 1080p, mandar 360p a
  la TV es saltar de versión → no vale en silencio. O la TV recibe el HLS por
  la pasarela, o se avisa ("en la TV solo 360p") y se deja elegir.
- **Unir DASH en el PC con ffmpeg**: en Windows no hay ffmpeg → descartado
  salvo empaquetarlo.
- Prueba pendiente (10 min, con la TV libre, `catt` en un venv basta):
  1. `-f 18 -g` → `catt cast <url>`; 2. maestro HLS directo; 3. maestro HLS
  por un proxy con CORS.

### Catálogo para navegar (sin probar)

Buscar en YouTube no arma carruseles. Deezer API (sin clave: charts por país,
artistas, portadas) → por cada tema `ytsearch1:"<artista> <tema> official
video"`. Resolver el id solo al abrir la ficha, no para todo el carrusel.
