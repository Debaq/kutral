# TODO

Ordenado de más simple a más complejo de implementar.

---

## 2. Control web nuevo — *hecho*

Control vertical en `GET /` (`src-tauri/src/control.html`).

**Pestaña Botones**: cruceta + OK, Atrás, Menú, Ayuda, Teclado. Auto-repeat al
mantener una flecha, solo cuando la tecla va a la interfaz (`/key` responde
`ok`).

**Pestaña Catálogo**: buscador y tendencias. El celular no tiene la key de
TMDb, así que busca el backend: `GET /buscar?tipo=&q=` y `GET /catalogo?tipo=`
usan `tmdb_buscar` / `tmdb_trending` (`lib.rs`) con la key que el front empuja
por `web_set_tmdb_key` al cargar y cada vez que cambia. Tocar una card manda
`POST /abrir`, el backend emite `remote_open` y `+layout.svelte` corta mpv y
navega a `/?play=<id>&type=<tipo>`: el mismo handoff que usa Vera.

**Sección Reproduciendo** (aparece sola cuando `/mpv` reporta algo corriendo):
título y progreso, ±60s, ver/ocultar subs, pista de audio (`a` → `cycle audio`),
pista de subtítulos (`j` → `cycle sub`) y cambiar fuente (`POST /accion` →
evento `player:cambiar-fuente`). Los rótulos de la cruceta cambian a ±10s y
volumen porque el backend ya manda las flechas a mpv cuando reproduce.

---

# Onboarding y API keys

Bloque aparte del de arriba: sale de la planificación sobre qué puede hacer un
usuario sin ninguna key, y cómo pedírselas. Ordenado igual, de más simple a más
complejo. **Nada de esto está implementado — es planificación.**

## Qué exige key hoy

| Función | Key | Sin ella |
|---|---|---|
| Catálogo Pelis/Series, detalle, búsqueda, personas | **TMDb** (`tmdb_key`, localStorage) | La pantalla `key-box` bloquea todo (`+page.svelte:2671`) |
| `imdb_id` → scrapers | **TMDb** (vía `item_status`) | Sin imdb no hay fuentes que buscar |
| Resolver magnet → URL reproducible | **RealDebrid** (device flow) | `SourcePicker.svelte:486,508` descarta todo magnet |
| Ratings y sinopsis extra | OMDb (opcional) | Se omite |
| Subtítulos Wyzie | opcional | Cae a OpenSubtitles |
| Subtítulos OpenSubtitles | **key de app ya embebida** (`opensubtitles.rs:19`) | Funciona sin nada |
| Anime (AniList + ani.zip + kitsu) | ninguna | Funciona |
| Scrapers (torrentio, mediafusion, yts, eztv, nyaa, animetosho) | ninguna | Funcionan |
| Premios (Wikidata), screening, IPTV | ninguna | Funcionan |

El dato que ordena todo: `+page.svelte:1409` hace
`if (!apiKey && tab !== "anime") return;`. **El tab Anime ya es un modo sin-key
completo y funcional.** El patrón se sostiene; falta extenderlo.

`opensubtitles.rs:19` es el precedente de embeber una key de app en el binario.

## 4. RD por API key, además del device flow — *chico*

Hoy RealDebrid se vincula solo por device flow (`lib.rs:2253` `rd_device_start`,
`RD_CLIENT_ID` público en `lib.rs:2227`). Falta aceptar el token de API que
RealDebrid da en `real-debrid.com/apitoken`.

Casi está: `rd_creds_save` (`creds.rs:65`) ya recibe un `access_token` suelto.

Trabajo:
- Campo `kind: "oauth" | "apikey"` en `RdCreds` (`creds.rs:11`), default `oauth`
  para no romper lo ya guardado.
- `rd_refresh` (`rd.rs:411`) no debe intentar renovar ni borrar credenciales si
  `kind == "apikey"`: ese token no expira ni tiene refresh.
- `rd_creds_status` devuelve el `kind` para que la UI diga "vinculado por token"
  en vez de "por cuenta".
- Campo en Configuración y en el paso 4 del asistente, con el QR de teclado.

Sirve para la ISO y para kioscos sin navegador cómodo, y permite provisionar
por script.

## 5. Modo sin cuenta: catálogo sin key de TMDb — *medio*

Sin TMDb el único agujero real es el **catálogo de pelis y series**: scrapers,
subtítulos, premios y disponibilidad ya viven sin key.

Opciones evaluadas:
- **Cinemeta** (`v3-cinemeta.strem.io`) — la recomendada. Sin key, sin registro,
  IDs IMDb nativos, lo que además elimina el paso `item_status` → `imdb_id`.
  Se pierde: discover fino (`sort_by`, `vote_count`, keywords), personas,
  géneros de TMDb, trailers de TMDb, títulos en es-CL.
- **Key de app embebida en el binario**, como OpenSubtitles. Un click menos,
  pero cuota compartida entre todos los usuarios y zona gris con los ToS de
  TMDb. Solo como fallback silencioso, no como default.
- **Proxy propio** — cuesta infraestructura. Descartada.

Propuesta: **modo "Sin cuenta" = Cinemeta + AniList + IPTV**, y TMDb
pasa de requisito a mejora ("catálogo completo, filtros, actores, español de
Chile"). Vera (`src/lib/vera/tmdb.ts`) queda atada a TMDb: su motor pide
discover fino y Cinemeta no lo sustituye.

Para reproducir sin debrid ya existe `torrentLocal` (opt-in, expone la IP en el
swarm). El asistente tiene que decirlo en una frase clara, no esconderlo.

## 6. Asistente inicial — *medio*

Ruta `/bienvenida` u overlay en `+layout.svelte`, con flag `onboarding_done` en
localStorage. Cada paso salteable y reversible, navegable con mando (`nav.ts` ya
lo da), y reusando `RemoteQr` como teclado por celular — ya funciona así en la
`key-box` actual.

0. **Idioma y modo de interfaz** — `initDetection()` ya sabe si es Kütral OS.
1. **Qué es y qué necesita** — tabla corta: qué funciona sin nada, qué pide cuenta.
2. **Red** — solo si falta; `WifiManager.svelte` ya existe.
3. **Catálogo** → `[Empezar sin cuenta]` (Cinemeta) · `[Tengo key de TMDb]` (QR).
4. **Reproducción** → `[Vincular debrid]` (QR del device flow) · `[Pegar token]`
   (punto 4 de arriba) · `[Sin debrid]`, que explica el torrent local y el
   riesgo de IP.
5. **Opcionales**, colapsado: OMDb, Wyzie, cuenta de OpenSubtitles, doblado vs
   subtitulado, calidad.
6. **Listo** — checklist verde/gris y "todo esto se cambia en Configuración".

Reentrada: botón "Repetir asistente" en Configuración. Se abre solo si falta lo
mínimo (ni catálogo ni forma de reproducir).

## 7. Más proveedores de debrid — *alto*

Hoy está todo cableado a RealDebrid: `rd.rs` (440 líneas), `creds.rs`
(`rd_creds.json`), y `SourcePicker.svelte` llamando `rd_resolve`,
`rd_instant_available`, `rd_account` y `rd_cleanup_torrents`.

Trabajo:
- Trait `Debrid` con `instant_available(hashes)`, `resolve(magnet)`,
  `unrestrict(link)`, `account()`, `cleanup(hours)`. `rd.rs` pasa a ser una
  implementación más.
- Store `debrid_creds.json` (0600, mismo patrón que hoy):
  `{ active: "realdebrid", providers: { realdebrid: {...}, alldebrid: {...} } }`.
- Comandos `debrid_*(magnet, provider?)`; los `rd_*` quedan de alias una versión.
- `config.rdLinked` → `config.debridLinked: string[]`.
- **Cadena de fallback**: si el proveedor activo devuelve `BLOQUEADO_DMCA`
  (el 451 de `rd.rs:68`), probar el siguiente vinculado antes de caer al torrent
  local. Hoy ese 451 salta directo al plan B.

Proveedores y su forma de autenticar:
- **RealDebrid**: device flow (hoy) o token de API (punto 4).
- **AllDebrid**: apikey en query (`?agent=kutral&apikey=`), más un flujo de PIN.
- **TorBox**: apikey por bearer.
- **Premiumize**: apikey u OAuth.
- **Debrid-Link**: OAuth device.

Ojo: cada uno consulta lo cacheado a su manera, y hay que **verificar si el
`instantAvailability` de RealDebrid sigue vivo** — `rd.rs:198` lo usa y RD lo
marcó para deprecar. Si ya cayó, el "instantáneo" de hoy puede estar mintiendo.

---

# Player en Windows: el mismo de Linux — *hecho (octubre 2026)*

Windows usa el reproductor embebido de Linux (`mpv_embed`): núcleo común
(`mod.rs`: barra, menús, panel de ajuste, sincronía, portada) + superficie por
plataforma (`gtk.rs` / `win32.rs`, ventana hija con libmpv en `wid`). El
camino viejo (mpv.exe aparte + IPC + uosc) se borró; si hace falta volver, está
en el historial antes del commit "feat(player): Windows usa el reproductor
embebido".

- libmpv-2.dll va en `vendor/mpv-dev/` (fetch-windows.ps1), enlazado en
  diferido: build.rs arma `mpv.lib` con `lib.exe` y `/DELAYLOAD`; win32.rs lo
  carga antes de la primera llamada a mpv.
- `prueba-windows.yml` (manual) maneja la barra en el runner y sube capturas.
- Gotchas: Esc por un binding de mpv nunca llega (mpv no tiene el foco) →
  `input-cursor-passthrough` + `input-vo-keyboard=no` y el input en la hija;
  z de `osd-overlay` empatado → el mpv nuevo apila mal (z = id).
- Sin probar en un Windows real: DPI ≠ 100 %, GPU real, instalador.

---

# Packs de temporada — *hecho (octubre 2026)*

- **Capítulo correcto** (`src-tauri/src/pack.rs`): Torrentio/MediaFusion mandan
  `fileIdx` y `behaviorHints.filename`; viajan en `Source` hasta `rd_resolver`
  y `torrent_add` como `Pista`. Orden: nombre del addon → `fileIdx` (salvo que
  el nombre diga otro capítulo) → capítulo leído del nombre (S01E05, 1x05,
  Ep 05, Capitulo 5, anime `- 05`) → el más grande solo si NO es un pack. Un
  pack que no trae el capítulo da error y se prueba otra fuente.
- **Siguiente capítulo del mismo torrent** (`$lib/pack.ts`, `desdePack` en
  SourcePicker): RD elige el pack entero si lo tiene cacheado (queda
  `downloaded` al instante; si no, vuelve a "solo el capítulo") y el
  siguiente sale de ese mismo torrent de la cuenta, sin `addMagnet` (~1,3 s).
  Local: el magnet repetido devuelve el torrent de la sesión y el archivo se
  suma (`bajar_archivo`).
- **Menú "Capítulos"** en el bar del reproductor (`BarAction::Episodes`,
  `mpv_capitulos`), con los nombres de TMDb y el actual enfocado. Funciona
  también con la lista de fuentes cerrada (pill, Esc).
- Probado en Xvfb contra RD real (Breaking Bad T1: E05 → E06 desde el menú);
  local con el test `e2e_pack_mismo_torrent`. Sin probar: fin de capítulo →
  "siguiente" con un pack local de verdad.

## Visto al probar

- `torrents/instantAvailability` de RD responde 403 `disabled_endpoint`: el ⚡
  "instantáneo" de la lista ya no se marca nunca.
- En X11 puro (Xvfb) el Esc dentro de un menú del reproductor lo toma el atajo
  global de la página (`registerBackShortcuts`): cierra la lista de fuentes por
  debajo y el menú sigue abierto. En Wayland no se vio.

---

# Transmitir a la TV: controles completos

El control mientras se transmite tiene que ser **el área de control**: todo lo
que tiene el reproductor normal. Hoy (`SourcePicker.svelte:1665`, `castBtns`
`:674`) hay pausa, ±30 s, subtítulos externos (recarga el LOAD), "No se oye",
seguir aquí y detener; la `CastPill` solo pausa y detiene.

## Fácil

- Barra de progreso clicable / seek absoluto (`cast_control("saltar", seg)` ya
  existe para Cast y DLNA) y saltos de ±10 s además de ±30 s.
- Volumen y silencio: el backend los tiene (`cast.rs:803`, `dlna.rs:313`) y
  `CastStatus` ya devuelve `volumen`/`silencio`; falta la UI.
- Al terminar un capítulo, ofrecer el siguiente en la TV (hoy cierra,
  `cast.svelte.ts:409`).

## Medio

- Subtítulos **incluidos en el video**: extraerlos (ffmpeg → VTT) y servirlos
  por `/sub-N.vtt` de la pasarela (`lan.rs:101`, `publicar_subs`).
- Cast: mandar todas las pistas de texto en el LOAD y cambiar con
  `EDIT_TRACKS_INFO`, sin recargar (hoy va una sola, `cast.rs:639`).

## Difícil: pista de audio

El Default Media Receiver de Cast solo cambia audio en HLS/DASH; con MKV/MP4
multipista suena la principal. DLNA no tiene selección de pista. La única vía es
**remux en la pasarela**: ffmpeg `-map 0:v -map 0:a:N -c copy` en streaming.
Costos: el video deja de ir directo RD → TV y pasa por el PC, y cada seek
reinicia ffmpeg con `-ss` (el Range sobre el remux no sirve).

**Spike con la LG y un Chromecast antes de planificar.**

---

# Trailers de YouTube: yt-dlp al día y bloqueos

Ya resuelto (commit `1ed678c`): el trailer se resolvía dos veces con yt-dlp —una
para preguntar si era reproducible, otra dentro de mpv vía ytdl_hook—, y la
segunda falla no tenía salida visible, así que quedaba el QR con trailers que sí
servían. Ahora `yt_trailer_src` (`lib.rs:832`) resuelve una sola vez y entrega
las URLs directas.

Hecho después: A y B abajo (`src-tauri/src/ytdlp.rs` y `yt_trailer_src` en
`trailers.rs`). Se deja la descripción como referencia.

## A. yt-dlp se queda viejo dentro del paquete — *hecho*

`vendor/fetch.sh:68` y `fetch-windows.ps1` bajan yt-dlp una vez y lo saltan si ya
está; el binario que sale en el paquete es el del día del build y nunca cambia.
YouTube rompe extractors cada pocas semanas, así que la app envejece sola:
funciona al lanzarla y deja de funcionar un mes después, sin que nadie toque
nada.

`yt-dlp -U` no sirve: se reemplaza en su propia ruta y `/app` del flatpak es solo
lectura (Program Files igual, sin admin).

Trabajo:
- Copia escribible en el directorio de datos del usuario, con prioridad sobre
  `vendor/` en `ytdlp_bin()` (`lib.rs:806`) y `ytdlp_path()` (`mpv_embed.rs:1532`,
  `player.rs:405` — las tres buscan igual, conviene unificarlas).
- Consultar la última release de GitHub como máximo una vez al día, en segundo
  plano, sin bloquear el arranque.
- Validar lo bajado con `--version` antes de usarlo; si falla, seguir con el
  vendorizado. Nunca quedar sin yt-dlp por una descarga a medias.
- Costo: ~40 MB por actualización en Linux, ~17 MB en Windows.

## B. Videos que YouTube bloquea — *reintento con tv/ios hecho*

Age-gate, bloqueo regional y el "Sign in to confirm you're not a bot" por IP.
Varía por video y por día, así que se ve como "a veces no funciona".

- Mitigación barata: si el primer intento falla, reintentar con otro cliente
  (`--extractor-args "youtube:player_client=tv"`, o `ios`). Destraba bastantes.
- El age-gate duro no tiene arreglo sin cookies de una cuenta, y eso no
  corresponde en un equipo de living.
- Fallback ya existente: trailer de Apple, y si no, QR.

---

# Transmitir a la TV: Miracast como respaldo

Surgió con una LG UK6200 (2018): sin Google Cast, solo DLNA. La TV trae
Miracast ("Screen Share"). **No está implementado — es una idea anotada.**

## Qué es y en qué se diferencia de lo que hay

DLNA/Cast le pasan a la TV la URL del archivo y la TV lo decodifica. Miracast
es un espejo: el PC reproduce, captura su pantalla, la vuelve a comprimir
(H.264, en general hasta 1080p) y la manda por Wi-Fi Direct.

- **A favor**: suena y se ve todo (decodifica el PC: DTS, TrueHD, cualquier
  códec), los subtítulos son los de mpv con su menú, y no depende del router
  ni del firewall (Wi-Fi Direct va directo PC ↔ TV; tampoco del multicast).
- **En contra**: peor imagen (recompresión), posibles saltos y desfase de
  audio según el wifi, y el PC tiene que mostrar la película todo el rato.

Sirve como **respaldo**: cuando la TV no puede con el formato ("No se oye",
versiones descartadas por la TV) o cuando se quieren los subtítulos de mpv.

## El mismo estándar con otro nombre según la marca

| Marca | Nombre en la TV |
|---|---|
| LG | Screen Share |
| Samsung | Screen Mirroring / Smart View |
| Sony (Bravia viejas) | Screen mirroring |
| Philips | Miracast / Screen mirroring |
| Hisense | Anyview Cast |
| TCL | Screen Share / Miracast |
| Roku TV | Screen mirroring |

Google TV / Android TV modernas en general **no** lo traen (Google lo sacó):
ahí va Google Cast, que ya está.

## Caminos para integrarlo

- **Windows**: Miracast viene en el sistema (Win + K). Kütral podría abrir ese
  panel con un botón "Ver en la TV por espejo" y dejar mpv en pantalla
  completa. Falta confirmar qué URI/atajo abre el panel directo.
- **Linux**: no hay nada del sistema. `gnome-network-displays` (funciona en KDE,
  captura por el portal de PipeWire) necesita una tarjeta con Wi-Fi Direct
  (`nmcli` lista un `p2p-dev-*`). Dependencia externa: detectarla y ofrecerla,
  no empaquetarla.
- **Kütral OS / flatpak**: evaluar aparte (portal de captura dentro del sandbox).
