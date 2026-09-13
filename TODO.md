# TODO

Ordenado de más simple a más complejo de implementar.

---

## 1. Descarga de juegos desde archive.org — *medio*

Hoy las ROMs bajan de Myrient, no de archive.org, y falla seguido.

- `src-tauri/src/emu.rs:491-570` — `emu_download`: arma la URL fija
  `https://myrient.erista.me/files/No-Intro/{folder}/{name}.zip` con `myrient_folder()` (`emu.rs:143`).
- Problema real: el nombre tiene que calzar exacto con el del set No-Intro; cualquier diferencia = 404 y el download queda colgado.

Trabajo:
- Fuente archive.org: resolver ítem por sistema y buscar el archivo dentro (`https://archive.org/metadata/<item>`), en vez de construir la URL a ciegas.
- Mapa sistema → ítem de archive.org (equivalente a `myrient_folder`).
- Fallback: si archive.org no tiene el juego, intentar Myrient (o al revés) antes de dar error.
- Errores visibles: hoy un 404 no se distingue de una caída de red. Emitir motivo por `emu_download_progress` / evento de error y mostrarlo en la card.
- Reanudar descargas cortadas (`Range`) y limpiar el `.zip` a medias.

---

## 2. Control web nuevo — *medio/alto*

El control actual (`src-tauri/src/remote.html`, 654 líneas) es un gamepad de retroarch: D-pad, A/B/X/Y, L/R, Select/Start, skins de color. Sirve para emulación, no para manejar Kütral.

- Servidor: `src-tauri/src/webserver.rs` (695 líneas) — sirve el HTML y recibe las teclas.
- UI de arranque: `src/lib/WebServerControl.svelte`, QR en `src/lib/RemoteQr.svelte`.
- Mapeo tecla → retropad: `src-tauri/src/emu.rs:789-799`.

Definir primero **qué tiene que hacer** el control nuevo antes de codear. Candidatos:
- Navegar el catálogo y mandar a reproducir desde el celu.
- Controles de reproducción reales: pausa, ±10s, volumen, subtítulos, cambiar fuente.
- Buscar por texto (teclado del celu en vez de deletrear con el D-pad).
- Ver qué se está reproduciendo (ya existe el bloque `#np` en `remote.html:420-428`).

El gamepad se conserva pero como modo aparte, activo solo cuando hay un juego corriendo.

---

## 3. Mando del emulador: lo que hay no alcanza — *medio*

Lo implementado solo asigna botones de un **mando físico USB** leyendo
/dev/input (`src-tauri/src/padmap.rs`, sección "Mando para juegos" en
Configuración). Falta lo que la gente realmente usa para jugar:

- **Teclado**: no se puede remapear nada. RetroArch usa sus teclas por defecto
  (flechas + Z/X/A/S) y Kütral no escribe ninguna línea `input_player1_<accion>`
  (sin sufijo `_btn`, que es la forma teclado). Sin mando enchufado, la pantalla
  de configuración no sirve para nada: ni siquiera aparece.
- **Control remoto web (celular)**: sus botones están fijos en
  `key_to_retropad` (`src-tauri/src/emu.rs:789-799`) y no pasan por el mapa.
  Jugar desde el celu con la distribución que uno quiera no se puede. Ojo: ese
  camino va por el Network Gamepad (UDP), no por la config de RetroArch, así
  que necesita su propio mapeo (retropad → tecla del control web) y probable
  reordenar los botones de `remote.html`.
- **No hay forma de probar** el mapeo sin abrir un juego: la pantalla no muestra
  qué se está pulsando en vivo (el mapeo de la interfaz sí lo hace, con
  `Gamepad.svelte` iluminando el botón).
- **La UI es una lista de 14 filas** con "Asignar" una por una. Debería ser un
  asistente que recorra los botones solo, sobre el dibujo del mando que ya
  existe (`src/lib/Gamepad.svelte`).
- **Sin probar con hardware**: no había ningún mando conectado al escribirlo.
  Hay que verificar que los índices que calcula `padmap.rs` son los mismos que
  usa RetroArch, con un pad real.
- **Solo Linux** y necesita que el usuario esté en el grupo `input`. En la ISO
  se puede dar por hecho; en un escritorio cualquiera, no.
- `input_autodetect_enable = "false"` deja fuera todo lo no asignado: si alguien
  asigna un botón y se olvida del resto, el mando queda medio muerto. Sería
  mejor escribir el mapa completo partiendo del autoconfig, o no apagar la
  autodetección y remapear al nivel del retropad (archivos `.rmp`).

Antes de seguir metiendo mano, decidir el alcance: **teclado y control web son
más importantes que el mando USB** para cómo se usa Kütral hoy.

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
| Premios (Wikidata), screening, IPTV, Juegos | ninguna | Funcionan |

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

Propuesta: **modo "Sin cuenta" = Cinemeta + AniList + IPTV + Juegos**, y TMDb
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
| `mpv:fin` → PostCreditos, próximo capítulo | sí (`mpv_embed.rs:2137`) | **no se emite** |
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

**B. Misma UI, por IPC, sin embeber.** mpv sigue siendo proceso aparte, pero:
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
