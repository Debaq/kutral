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

# Player en Windows: OSD y controles propios

> **Actualización 2026-10-14: preferencia por A.** Mantener dos reproductores
> duplica cada función (la sincronía automática de subtítulos, `29b66ed`, fueron
> ~270 líneas solo para Windows). Con A, Windows hereda tal cual la barra, los
> menús, el panel de ajuste, la portada y la sincronía, y se borran el vigía de
> IPC, uosc y el `input.conf` de Windows. Además resuelve "Salir del reproductor
> en Windows" (más abajo): Esc = `stop()`, como en Linux.
>
> **Spike hecho (2026-10-15, `mpv_win.rs`, `spike-windows.yml`): A es viable.**
> En el runner de Windows, con capturas: libmpv enlaza con MSVC (`build.rs`
> arma `mpv.lib` desde el DLL), el video se ve dentro de la ventana de Kütral
> encima de WebView2, el `osd-overlay` se dibuja encima del video, y con
> `input-cursor-passthrough` + `input-vo-keyboard=no` el teclado y el mouse
> llegan a nuestra ventana hija en el hilo principal (como el GLArea): Esc
> oculta el video y la interfaz recupera el foco. Un binding de Esc en mpv
> NO sirve (mpv nunca tiene el foco). Falta: DPI ≠ 100 %, GPU real,
> empaquetar el DLL junto al exe.
>
> Plan: (1) separar `mpv_embed.rs` en núcleo (barra, menús, panel, sincronía,
> portada) + superficie GTK, sin cambiar Linux — **hecho** (`bfb2e75`); (2)
> superficie Win32 (`mpv_embed/win32.rs`, timers con `SetTimer`) y
> `player.rs` de Windows delega como en Linux, detrás de `embed-win` —
> **hecho**: `prueba-windows.yml` maneja la barra con teclas y mouse y las
> capturas salen como en Linux (con z empatado el mpv nuevo dibujaba el fondo
> encima de los iconos: z = id); (3) DLL en el instalador (junto al exe),
> `embed-win` por defecto, borrar IPC + uosc + mpv.exe. Pendiente además:
> fuente de la barra (en Windows cae a Arial), DPI ≠ 100 %, GPU real.
>
> El spike original pedía: video de libmpv en un
> HWND hijo de la ventana Tauri, encima de WebView2, y Esc que devuelve el foco
> a la interfaz; más enlazar `libmpv-2.dll` con MSVC (el zip de shinchiro trae
> `libmpv.dll.a`, MSVC necesita un `.lib` generado con `lib /def`) y
> empaquetarla en la CI. Aquí no hay Windows: cada iteración la prueba el
> usuario. Si el spike sale bien: `mpv_embed.rs` se separa en núcleo portable
> (overlays, menús, sincronía) + superficie por plataforma (GTK / Win32).

Bloque aparte, **sin decidir**. Salió al planificar el aviso por OSD de la
escalada de cache: en Windows el reproductor no tiene la interfaz que tiene en
Linux, y queremos que la tenga. Acá queda el mapa para elegir camino.

## Qué hay hoy

Windows compila y se empaqueta (`.github/workflows/release.yml:212`, NSIS+MSI)
y sí lleva uosc: `fetch-windows.ps1` vendoriza `mpv.exe` y `spawn_mpv`
(`player.rs:400`) le pasa `--config-dir` con nuestro `mpv-config`. O sea, OSD
hay — el de uosc, que es **mouse-first**. Lo que no hay es lo nuestro.

| | Linux (embed) | Windows (proceso) |
|---|---|---|
| Ventana | una sola, mpv dentro del GtkGLArea | dos: mpv fullscreen sobre la app |
| Barra de control | nuestra, ASS, navegable con d-pad/mando | uosc, solo mouse |
| Menú de pistas/subs | nuestro (`mpv_embed.rs:1149` `open_menu`) | el de uosc |
| Picker de subs del front | `mpv_open_picker` | menú de uosc (`kutral-pick`) |
| `mpv:fin` → PostCreditos, próximo capítulo | sí (`mpv_embed.rs:2137`) | sí, por el vigía de IPC (`player.rs:watch_events`) |
| `player:cambiar-fuente` | botón en la barra (`mpv_embed.rs:1045`) | menú de uosc (`kutral-fuente`) |
| Sincronía de subs (memoria + por audio) | panel de ajuste | menú de uosc (`29b66ed`) |
| Portada de carga | tapa el frame viejo | no |
| Brillo, pill, gamepad | sí | parcial |

Causa: `mpv_embed.rs` (2525 líneas) está atado a GTK — `GtkGLArea` + EGL para
el render, señales GTK para mouse/teclado, `glib::timeout_add_local` para todos
los relojes.

**El dibujo NO es lo atado.** La barra y los menús se pintan con el comando
`osd-overlay` de mpv (`put_overlay`, `mpv_embed.rs:777`): ASS puro, idéntico en
cualquier plataforma y disponible **también por IPC**. Lo atado a GTK es el
render, el input y los timers.

## Caminos

**A. Portar el embed a Windows.** libmpv con `--wid` sobre un HWND hijo de la
ventana Tauri; en Windows no hace falta el camino OpenGL, mpv se pinta solo en
ese HWND. Hay que reescribir la capa de input (Win32/Tauri en vez de señales
GTK) y los timers. Da ventana única y paridad total. El más caro y el de más
riesgo: foco, z-order y DPI conviviendo con WebView2.

**B. Misma UI, por IPC, sin embeber.** *(en curso: B1 hecho)* mpv sigue siendo proceso aparte, pero:
un hilo lector del pipe que parsee eventos (`end-file`, `observe_property`) —
de ahí salen `mpv:fin`, el picker y `cambiar-fuente` — y nuestra barra ASS
mandada con `osd-overlay` por el mismo pipe. Los constructores de ASS
(`build_bar_ass`, `build_seek_ass`, `menu_geom`…) son funciones puras de
string: se extraen de `mpv_embed.rs` a un módulo compartido y las usan las dos
plataformas. Quedan las dos ventanas, pero se recupera todo lo demás de la
tabla. Bastante menos trabajo que A y no toca el camino de Linux.

**C. libmpv en proceso, con ventana propia de mpv.** Mismo acceso directo a
propiedades y eventos que en Linux, sin pelear con el HWND. Ahorra el IPC pero
tampoco da ventana única.

## Estado de B, por etapas

Cada etapa sirve sola; se hacen en este orden porque la última es la cara.

- **B1 — eventos (hecho, commit `6558374`).** Hilo lector del pipe: observa
  `time-pos`/`duration` y traduce `end-file` a `mpv:fin`, más `mpv:state` al
  cerrarse mpv. Con esto Windows ya tiene PostCréditos y próximo capítulo.
- **B2 — la barra y los menús por `osd-overlay`.** Extraer de `mpv_embed.rs` los
  constructores de ASS (`build_bar_ass`, `build_seek_ass`, `build_menu_ass`,
  `build_loading_ass`, `bar_geom`, `menu_geom`, los hit-tests) a un módulo
  compartido. No son tan puros como parecen: hoy leen estado por globales y por
  `get_f64`/`get_string` contra mpv, así que hay que pasarles el estado en vez
  de que lo busquen solas. Es refactor sobre el Linux que funciona → reprobar
  Linux entero. ~800-1.000 líneas movidas.
- **B3 — el input (lo caro).** En Windows mpv tiene su propia ventana fullscreen
  **con el foco**: las teclas y el mando los recibe mpv, no la app. La barra
  navegable con d-pad no se porta tal cual; hay que mapear cada tecla en
  `input.conf` a un `script-message` y leerlo del lado del vigía. Es diseño
  nuevo, no traducción. Mientras no esté, uosc sigue puesto y cubre el mouse.

**Recomendación: B primero.** Recupera el OSD y los controles navegables con
mando sin el riesgo de A, y deja A como paso posterior si se quiere la ventana
única — porque después de B, A solo cambia *dónde* se pinta, no *qué*.

## Lo que esto NO bloquea

La escalada de cache es portable desde el día uno:
- `show-text` funciona hoy en Windows por IPC (`mpv_cmd`).
- `read_props` (`player.rs:684`) ya lee propiedades por el pipe en Windows, así
  que `paused-for-cache`, `demuxer-cache-duration` y `cache-speed` se leen en
  las dos plataformas, y `set_property` sube `cache-pause-wait` igual.

Lo único que queda degradado en Windows hasta que se haga B es la pantalla de
"mejor bájala y la ves después": en Linux es overlay propio, en Windows un
`show-text` con una tecla.

---

# Salir del reproductor en Windows — *hecho, falta probar (`02a5425`)*

Arreglado: Esc sale de pantalla completa antes de minimizar (y retomar la
vuelve a poner), el vigía espera hasta 10 s el pipe (antes 2 s: sin vigía Esc
no hacía nada) y al cerrar mpv Kütral recupera el foco. Lo de abajo queda como
referencia.

Hoy es incómodo: hay que desmaximizar y cerrar, o Alt+F4. mpv abre
`--fullscreen` con `border=no` (`mpv.conf`), así que no hay ✕ visible; Esc/BS
mandan `kutral-salir`, que **suspende** (pausa + minimiza, no cierra), y para
cerrar de verdad solo están `q` o "Salir" en el menú de uosc, que nadie
encuentra.

Opciones:
- Mostrar los controles de ventana de uosc (`top_bar`, `top_bar_controls` en
  `script-opts/uosc.conf`) en pantalla completa.
- Una sola tecla evidente que cierre, guarde el progreso y devuelva el foco a
  Kütral (hoy Esc suspende para poder retomar desde la pill).

Se prueba en Linux con mpv + uosc local por IPC. Si se hace el camino A del
player, esto desaparece solo.

---

# Packs de temporada

## 1. Elegir el capítulo correcto dentro del pack — *chico, es un bug*

Torrentio y MediaFusion mandan `fileIdx` y `behaviorHints.filename` en cada
stream, pero `StremioStream` (`kodios.rs:262`) no los lee y `Source`
(`kodios.rs:49`) no tiene dónde guardarlos. Al resolver se elige **el video más
grande**: `resolve_magnet` en `rd.rs:157` (`max_by_key(bytes)` + `selectFiles`)
y `pick_file` en `torrent.rs:358`. Con un pack de temporada, pedir el E05 puede
reproducir otro capítulo.

Trabajo: leer `fileIdx`/`filename`, llevarlos por `Source` hasta `rd_resolve` y
`torrent_add`, y elegir ese archivo. Sin `fileIdx`, matching por nombre
(`S01E05`, `1x05`, `E05`, anime `- 05`) antes de caer al más grande.

## 2. El siguiente capítulo usa el mismo torrent — *medio*

Hoy `verSiguienteCapitulo` (`+page.svelte:2171`) cambia temporada/capítulo y
**busca fuentes desde cero**. Con un pack, el siguiente ya está en el mismo
torrent.

- RD: `selectFiles` de otro id sobre el mismo torrent. Ojo: `resolve_magnet`
  hace `addMagnet` cada vez (`rd.rs:146`) y crea un torrent nuevo en RD.
- Torrent local: `Entry.file_id` es único por torrent (`torrent.rs:624`), y el
  prebuffer y `stream_url` están atados a él; hace falta `update_only_files` +
  `stream(file_id)` sobre el mismo handle.
- Progreso, historial y contexto de sincronía dependen de `season`/`episode` del
  SourcePicker (`SourcePicker.svelte:481`): hay que actualizarlos a mano.

**Spike primero**: cambiar de archivo en el mismo torrent, en RD y en local.

## 3. Menú de capítulos en el reproductor — *medio*

Con el pack abierto, un menú "Capítulos" para saltar entre ellos: ítem en el bar
del embed (`BarAction`, `mpv_embed.rs:213`) y en el menú de uosc en Windows,
usando `mpv_open_picker` + `player:menu-pick` (hoy solo lo usa
`SubsService.svelte`).

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
