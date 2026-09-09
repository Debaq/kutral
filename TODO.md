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
