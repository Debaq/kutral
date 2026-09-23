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
| Picker de subs del front | `mpv_open_picker` | **stub no-op** (`player.rs:519`) |
| `mpv:fin` → PostCreditos, próximo capítulo | sí (`mpv_embed.rs:2137`) | sí, por el vigía de IPC (`player.rs:watch_events`) |
| `player:cambiar-fuente` | botón en la barra (`mpv_embed.rs:1045`) | **no existe** |
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

# Trailers de YouTube: yt-dlp al día y bloqueos

Ya resuelto (commit `1ed678c`): el trailer se resolvía dos veces con yt-dlp —una
para preguntar si era reproducible, otra dentro de mpv vía ytdl_hook—, y la
segunda falla no tenía salida visible, así que quedaba el QR con trailers que sí
servían. Ahora `yt_trailer_src` (`lib.rs:832`) resuelve una sola vez y entrega
las URLs directas.

Queda pendiente, sin urgencia:

## A. yt-dlp se queda viejo dentro del paquete — *medio*

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

## B. Videos que YouTube bloquea — *bajo*

Age-gate, bloqueo regional y el "Sign in to confirm you're not a bot" por IP.
Varía por video y por día, así que se ve como "a veces no funciona".

- Mitigación barata: si el primer intento falla, reintentar con otro cliente
  (`--extractor-args "youtube:player_client=tv"`, o `ios`). Destraba bastantes.
- El age-gate duro no tiene arreglo sin cookies de una cuenta, y eso no
  corresponde en un equipo de living.
- Fallback ya existente: trailer de Apple, y si no, QR.
