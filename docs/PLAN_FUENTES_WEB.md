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

1. Descarga HTTP de lo que da RD (así el anime de jkanime también se baja).
2. mega1080 como fuente torrent en español (magnets + idioma).
3. Lista de fuentes con salud automática en Configuración.
4. Más sitios, tomando los canales activos de Alfa como mapa.
