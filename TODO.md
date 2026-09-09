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
