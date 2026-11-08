# Fuentes web y hosters vía Real-Debrid

Estado 2026-11-07. Anime hecho (29341e0); películas/series y la lista en
Configuración, por hacer.

## Hecho: anime (jkanime)

- La página de capítulo trae `var servers = [{remote: base64(url), server, lang, size}]`.
- RD abre Mega (`/embed/` → `/file/`), Mediafire, Mixdrop (espejo → `mixdrop.ag`) y VOE.
  No abre Mp4upload, Streamwish, Vidhide, Filemoon ni Doodstream.
- `Source.links` = copias en orden; el picker prueba `rd_unrestrict` hasta que una abre.
- Falta: **descargar** por HTTP (hoy la cola solo baja torrents) → fase aparte.

## Sondeo: películas y series

### mega1080.me (probado)

WordPress con tema "hackstore" y una API JSON propia en `/api/rest`, sin Cloudflare challenge:

| Endpoint | Qué da |
|---|---|
| `search?post_type=movies\|tvshows\|animes&query=` | `_id`, `slug`, `title`, `original_title`, `release_date` (emparejar con TMDb). Query de 3 a 15 letras. |
| `downloads?post_id=` | por versión: `lang` ("Latino AC3 5.1 - Ingles…"), `quality`, `size`, `format`, `url`, `server` |
| `player?post_id=` | embeds (vimeos.net…), no sirven para RD |
| `episodes?post_id=` | capítulos de series (sin probar) |

Lo que vale: **magnets con el idioma bien etiquetado** (latino, dual) → otra fuente
torrent en español, buena para el filtro de idioma.

Lo que no sirve: hosters. Mega caído incluso en un estreno de 2025; 1fichier
abre pero es `.rar`; Mediafire y megaup también `.rar`. Un `.rar` no se reproduce.

### Alfa (addon de Kodi, `alfa-addon/addon`)

- Vivo (último cambio 2026-08). Es el proyecto hispano que "mantiene los scrapers".
- 254 canales (`plugin.video.alfa/channels/*.json`) y 202 servidores
  (`servers/*.json`). Cada uno trae `active`, `language` (cast/lat),
  `categories` (movie, tvshow, torrent, direct…).
- Hoy: **71 canales activos** (no adultos): 12 torrent (dontorrent,
  mejortorrent, elitetorrent, grantorrent, divxtotal, pelispanda, wolfmax4k…)
  y 7 directos (cinecalidad, pelisplus, poseidonhd…).
- **Licencia GPL-3.0**: no copiar su código (Kütral evita GPL, igual que con
  alass). Sí se puede **leer sus JSON como dato**: qué canal está activo y qué
  dominio usa. Ver el scraper de cada canal sirve para aprender, el nuestro se
  escribe desde cero.

### Prueba con Alfa (2026-11-07): interpretar sus modelos NO sirve

- De los 71 canales activos, 31 usan el modelo declarativo `finds`, y **ninguno**
  funciona solo con él: todos suman 2 a 5 funciones Python a mano (`*_matches`)
  para sacar cada resultado y cada link. dontorrent además exige una prueba de
  trabajo (SHA-256) para entregar el torrent.
- Lo que sí es dato puro y se puede leer solo: `canonical` → `host_alt`
  (dominio vigente y espejos) y `host_black_list` (dominios muertos), más el
  `active` del `.json`. Es justo lo que más cambia.
- Conclusión: **scrapers propios por sitio** + **Alfa como alimentador
  automático de dominios y estado**.

Dominios vigentes según Alfa que respondieron (2026-11-07): elitetorrent,
mitorrent, pelispanda, divxtotal, homecine, allcalidad, genteclic,
cinelibreonline (200); dontorrent, grantorrent, hacktorrent, wolfmax4k (301);
mejortorrent (Cloudflare); cinecalidad, poseidonhd (TLS roto: ¿bloqueo DNS del
operador? probar con el DoH de net.rs); pelisplus (no resuelve).

### Puente para ejecutar Alfa en el PC: DESCARTADO (2026-11-07)

- Se intentó correr Alfa sin Kodi (reemplazos del mediaserver + un comodín para
  lo que falta). Llega hasta importar los canales, pero Alfa carga módulos
  **ofuscados** (`lib/alfaresolver*.py`, `lib/planb*.py`, 200-400 KB, bytecode
  marshal en base64 ejecutado con `exec`, uno por versión de Python).
  `generictools` los importa, y casi todos los canales dependen de él.
- Ejecutar código cerrado y autoactualizable en el PC de cada usuario (que
  guarda tokens RD y la bóveda) no es aceptable.
- Correrlo en el servidor tampoco: el hosting es PHP 7.4 compartido, sin
  Python, y todas las búsquedas saldrían desde tecmedhub.org (exposición
  legal, y los sitios bloquean la IP del servidor).
- Lo que se usa de Alfa: sus archivos como DATO (dominios, activo, lista negra)
  y como AVISO (si cambia un canal que usamos, revisar nuestro scraper).

### pelispanda.org (probado): la mejor candidata

API JSON de WordPress, sin Cloudflare:

| Endpoint | Qué da |
|---|---|
| `wp-json/wpreact/v1/search?query=&posts_per_page=&page=` | `slug`, `type` (pelicula/serie/anime), `year`, **`tmdb_id`** |
| `wp-json/wpreact/v1/movie/{slug}/` | `downloads[]`: `quality`, `size`, `language` ("Latino/Inglés"), `subs`, `download_link` (magnet) |
| `wp-json/wpreact/v1/serie/{slug}/related/` (o `anime/`) | `downloads[]` por capítulo: `season`, `episode`, ídem |

Emparejamiento EXACTO por `tmdb_id`, idioma bien etiquetado, películas, series
y anime. Mismos magnets que mega1080 (mismo grupo), pero con mejor API.

## Propuesta: lista de fuentes en Configuración

1. **Ajustes → Fuentes**: cada scraper (Torrentio, MediaFusion, YTS, EZTV, Nyaa,
   AnimeTosho, jkanime, mega1080, …) con interruptor, idioma y "último resultado".
2. **Salud automática**: cada búsqueda anota éxito o falla por fuente. Tras N
   fallas seguidas se da de baja sola ("en pausa: no responde desde el 12/11")
   y se reintenta cada X horas. El usuario puede forzarla.
3. **Dominios cambiantes**: dominio configurable por fuente, con espejos. Lo
   mismo que hace `JK_MIRRORS`, pero editable.
4. (Opcional) Feed de Alfa: leer sus `channels/*.json` para saber si un sitio
   murió o cambió de dominio, sin tocar su código.

## Orden sugerido

1. ✅ pelispanda como fuente torrent en español (por tmdb_id) — 72d5fd8.
2. ✅ Lista de sitios vivos desde **Balandro** (repobal/base + repobal/fix; Alfa
   descartado): `scripts/fuentes_balandro.py` + workflow diario
   `fuentes-balandro.yml` → rama de datos `fuentes` (`balandro.json`) + issue
   cuando cambia un canal vigilado (pelispanda, jkanime). Kütral toma de ahí el
   dominio vigente (`src-tauri/src/dominios.rs`).
3. ✅ Configuración → Fuentes (ff82525): interruptor por buscador, salud por
   equipo (ok/vacío/falla/timeout; 3 fallas seguidas → pausa 6 h), datos de
   Balandro. Orden: en doblado el audio en tu idioma gana a "al instante"; el
   merge por hash conserva el título con idioma; proveedor legible en la fila.
4. ✅ Descarga HTTP con Real-Debrid (e539d4d): descarga_http.rs, en la misma
   cola que los torrents; RD primero si está vinculado, torrent si RD no puede.
5. ✅ hacktorrent, elitetorrent, grantorrent (860850d). Sondeados y aplazados:
   divxtotal y subtorrents (HTML por título, .torrent; fáciles con sitios_es),
   mitorrent (mismos magnets que pelispanda, exige clave AES de Balandro),
   series de hacktorrent (acortador cifrado).
6. Anime en español: Balandro tiene ~25 canales activos (animeflv, animeav1,
   tioanime, latanime, henaojara…), casi todos streaming con hosters: mismo
   camino que jkanime (copias vía Real-Debrid).
