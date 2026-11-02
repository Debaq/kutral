# Roadmap UX (temporal)

Auditoría de diseño, GUI y dinámicas del 2026-10-31. Archivo temporal: se borra
cuando esté todo hecho o descartado. Marcar `[x]` al terminar, `[~]` si se
descarta (con motivo). Líneas aproximadas a la fecha de la auditoría.

Orden: Fase 1 → 5.

---

## Fase 1 — Bugs reales (verificados en código)

- [x] **A. Esc no cancela el intento de fuente.** Solo pone `resolving=false`;
  el bucle de `playFrom` solo mira `vivo` y mpv abre igual. Elegir otra fuente
  deja dos `playFrom` concurrentes. Fix: token de generación
  (`const mio = ++intento`), cada `await` y `launch` salen si `mio !== intento`;
  Esc incrementa. `SourcePicker.svelte:2419`, `:1411`, `:644`
- [x] **B. Enter durante `playing` corta el video.** Doble Enter del control
  mata lo recién abierto. Fix: quitar Enter (dejar Esc/Backspace) o ignorarlo
  ~1.5 s tras `playing=true`. `SourcePicker.svelte:2362`
- [x] **C. Carrera en `pick()`.** Respuesta tardía de A pisa la ficha de B
  enfocada; Enter abre la peli equivocada. Fix: `pickSeq`, descartar si no es el
  último; en Enter validar `d.id === it.id`. `+page.svelte:1585-1608`
- [x] **D. Updater de Kütral OS instala y reinicia solo** al arrancar, aunque se
  esté viendo algo. Fix: no correr con reproducción activa + aviso 10 s
  "Actualizar ahora / Más tarde". `Updater.svelte:45`
- [x] **E. "Ya probada" se marca por título, no por hash.** Marca fuentes
  equivocadas y se olvida entre capítulos. Fix: clave `info_hash || url`,
  recordar por serie durante la sesión. `SourcePicker.svelte:286, 2281, 2787`
- [x] **F. El grid de carátulas se queda pegado al bajar.** (fix en webkit_scroll.rs, falta prueba real) Reportado por el
  usuario (2026-10-31), versión actual. En cualquier grid de carátulas, al ir
  bajando llega un punto en que no baja más **ni vuelve a subir**: queda
  trabado. Pasa **con el mouse** (rueda). Queda congelado y **no aparece
  nada**: ni "Cargando más…" ni chispas.
  **Toda la app deja de responder** (ni hover ni botones del header): no es un
  overlay, es el hilo de la UI bloqueado (bucle JS o `$effect` que se
  realimenta al agregar una página) o el proceso del webview colgado (muchas
  imágenes/memoria). Primer paso: ver consola/devtools o `top` del proceso
  WebKit al trabarse (¿CPU al 100% = bucle; memoria disparada = imágenes?).
  **Diagnóstico en vivo (2026-10-31, v26.10.13, WebKitGTK 2.54.2, Linux):** se
  trabó a los ~30 s de abrir. WebKitWebProcess: RSS estable ~400 MB, 0% CPU,
  hilo principal en `__futex_wait` con `utime` congelado (no avanza nada).
  Kütral (UI) en `poll`, ocioso. ⇒ **no es bucle JS ni memoria: es un deadlock
  del hilo principal de WebKit.** Hay 3 hilos "Core: Scrolling" + 3
  ThreadedCompositor. Como solo pasa con la rueda y el grid es un
  `overflow-y: auto` (`.grid-wrap`), sospecha fuerte: **scroll asíncrono de
  WebKitGTK 2.54** (hilo de scrolling ↔ hilo principal). No hay ninguna
  variable `WEBKIT_*` puesta. Pila no obtenida (ptrace_scope=1); confirmar con
  `sudo eu-stack -p <pid WebKitWebProcess>`.
  **CONFIRMADO con `sudo eu-stack` (pila en `~/pila_webkit.txt`):** deadlock
  ABBA dentro de WebKitGTK 2.54.2, no es código nuestro.
  - Hilo principal: `WebPage::finalizeRenderingUpdate` → `flushCompositingState`
    → `recursiveCommitChanges` → `commitLayerChanges` →
    `LayerTreeHost::attachLayer` → `lockSlow` (espera).
  - Hilo "Core: Scrolling": `ThreadedScrollingTree::displayDidRefresh` →
    `ScrollingTreeCoordinated::applyLayerPositionsInternal` →
    `LayerTreeHost::requestComposition` → `CoordinatedSceneState::flushPendingState`
    → `lockSlow` (espera).
  Cada uno tiene el lock que el otro quiere. Se dispara cuando se **crean capas
  nuevas** (`attachLayer`: entran cards nuevas por el prefetch al 60%) mientras
  el hilo de scroll aplica posiciones (rueda). Windows (WebView2) no afectado.
  Flags presentes en la lib: `AsyncOverflowScrollingEnabled`,
  `AsyncFrameScrollingEnabled`, `ThreadedScrollingEnabled`.
  **Fix propuesto:** en el bloque Linux de `lib.rs:603` (`with_webview`, junto a
  MSE) apagar esos features con `webkit_settings_get_all_features` +
  `webkit_settings_set_feature_enabled`. El crate `webkit2gtk =2.0.2` no los
  expone → FFI con `dlsym` (API existe desde WebKitGTK 2.42; con dlsym no rompe
  en sistemas más viejos). Probar en real: rueda rápida mientras carga páginas.
  Si no alcanza, reportar a bugs.webkit.org con la pila.
  Arreglos candidatos previos (probar en ese orden): desactivar features
  `AsyncOverflowScrollingEnabled` / `AsyncFrameScrollingEnabled` con
  `webkit_settings_set_feature_enabled` en el setup Linux; o
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` / `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
  Si no, revisar el lado JS:
  Puntos a mirar en `+page.svelte`:
  `onscroll` → `prefetchSiCorresponde` (`:2995`, `:1401`, dispara al 60%),
  `loadMore`/`loadList(true)` (`:1391`), observer del sentinel que hace
  unobserve/observe tras cargar (`:1559-1566`), `obrero()` de
  `checkItemStatus` con concurrencia 2 (`:1385`), `ghostCount`/sparkles.

## Fase 2 — Quick wins (esfuerzo S)

### Inicio / ficha
- [x] Series sin botón Descubrir en la ficha (solo Trailer). Agregar
  `▶ Descubrir` / `▶ Continuar T2E5` → `goDescubrir()`. Comentario de "lista de
  Temporadas abajo" está obsoleto. `FichaTitulo.svelte:107-114`
- [x] "Desde el inicio" y "Continuar" llaman ambos `onDescubrir`. Conectar
  "Desde el inicio" a `menuRestart` o eliminarlo; Continuar primero.
  `FichaTitulo.svelte:138-144`
- [x] Foco inicial de la ficha cae en "☆ Favoritos" (I + Enter marca fav sin
  querer). Subir `action-row` bajo la meta y foco inicial a Descubrir
  (`data-primary` que busque `toggleInfoFocus`). `FichaTitulo.svelte:68-148`,
  `+page.svelte:1460-1477`
- [x] ClaveTmdb inalcanzable con flechas: `data-nav` en input y botón, foco al
  montar, placeholder "Pega aquí tu clave" (no "32 chars hex"), QR del celular
  en primer plano. `ClaveTmdb.svelte:20-30`
- [x] Ficha se vacía ("Cargando…") en cada movimiento de foco → mantener la
  anterior atenuada hasta que llegue la nueva. `+page.svelte:2825`, `:1480`
- [x] Errores sin salida: banner con "Reintentar" (`data-nav`); separar
  `detailError` de `listError` (hoy un fallo de ficha parece fallo de lista).
  "Sin resultados" → mostrar lo buscado + "Limpiar búsqueda y filtros".
  `+page.svelte:2983`, `:1596`, `:3181`
- [x] Backspace en panel info → volver a la última card (`lastCardEl`) + pista
  fija discreta al pie de la ficha. `+page.svelte:2560-2575`

### Reproducción
- [x] Errores finales dicen "Prueba 🔄 Rebuscar fuentes" pero ese botón no está
  en esa pantalla. Agregar "Rebuscar" al pie (en el anillo de foco), también en
  lista vacía y error. `SourcePicker.svelte:1593-1601`, `:547`, `:2822`
- [x] Contador "Probando fuente 5/48" engañoso (se rinde a los 12). Mostrar
  `tried+1/min(MAX_TRIES, candidatas)` + línea con fuente y paso actual
  ("resolviendo en Real-Debrid" / "revisando idioma"). `SourcePicker.svelte:1452`
- [x] Barra de atajos fija al pie de EpisodePicker y SourcePicker (Enter
  Descubrir · D Bajar · T Dispositivo · ←→ Filtro). Hoy "D" solo en `title`.
  `EpisodePicker.svelte:236-247`, `SourcePicker.svelte:2403`
- [x] Error de capítulos muestra `String(e)` crudo sin reintentar. Componente de
  error común: frase humana + detalle plegable + "Reintentar".
  `EpisodePicker.svelte:350-353`, `SourcePicker.svelte:556`
- [x] Oferta DMCA/idioma: separar "qué pasa / qué se hace", barra de cuenta
  regresiva, pausar contador con flechas, letra más grande.
  `SourcePicker.svelte:1696-1701`, `:2662-2670`
- [x] SubsService: mensajes OSD truncados a 50 chars; "sin subtítulos" sin
  alternativa. Mensaje accionable + recordar atajo de sincronía.
  `SubsService.svelte:31-62`

### Pantallas secundarias
- [x] Historial: quitar solo con Supr (no existe en control remoto). Botón
  "Quitar"/⋯ en card enfocada; mostrar "‹ ›" en chips de pestañas (`[`/`]`).
  `historial/+page.svelte:457-461`, `:530`
- [x] QuienVe enfoca siempre el primer perfil → enfocar `.perfil.actual`.
  `QuienVe.svelte:46`
- [x] Mensaje de dev al usuario: "Configúrala en el código (módulo
  opensubtitles.rs)". Ocultar o "no disponible en esta versión".
  `TabCuenta.svelte:657`
- [x] (renombrado a "🛠 Soporte", se quedó en Cuenta junto a RD) Panel Diagnóstico RD (IMDb de prueba, hashes, 451) → mover a Sistema →
  Avanzado/Soporte. `TabCuenta.svelte:614-650`
- [x] Vera / ElegirCarita sin key: botón "Ir a Configuración" y texto único.
  `vera/+page.svelte:227`, `ElegirCarita.svelte:51`
- [x] `/config` sin foco visible en `.mode-card`, `.radio`, `.toggle-row`,
  `.btn-link`, `.btn-sec`… Regla de foco (se resuelve con Fase 3 si va antes).
  `ajustes/ajustes.css`
- [x] Enlace "📱 Escribir desde el celular" junto a cada campo de clave (TMDb,
  OMDb, Wyzie, token RD). `config/+page.svelte:143`, `TabCuenta.svelte:495`
- [x] Botón "Administrar listas" en cabecera de `/iptv`; renombrar bloque a
  "Canales de TV en vivo (listas M3U)". `TabTransmitir.svelte:175-234`

### Copy (reglas de marca)
- [x] Voseo: "revisá las URLs en config" (`iptv:102`), "Afiná la búsqueda"
  (`iptv:463`). "acá" → "aquí" (`historial:569`, `:834`)
- [x] "Reproducir"/"Play" visibles: `bienvenida:343,388`, `TabDescargas:66,76`,
  `TabReproduccion:62,151`, `iptv:489`, `iptv:508` ("Pausa / Play" → "Pausa /
  Seguir"). Pestaña "Reproducción" de config: decidir.
- [x] PlayMenu "⚡ Ver con debrid" → en torno a "Descubrir"; "debrid" solo en
  Configuración. `PlayMenu.svelte:151-161`
- [x] "Kutral" sin diéresis (`TabCuenta:682,688,191`); unificar "Real-Debrid" y
  "clave" (no "key").
- [x] RD descrito como "streaming premium sin publicidad" → "hace que los
  torrents carguen al instante y sin exponer tu IP". `TabCuenta.svelte:561`
- [x] PantallaNoDisponible: "Lo sentimos muchísimo", "La marcamos así no la
  sugerimos" (rota), "nuestra cartelera". AvisoNoDisponible filtra
  "playimdb.com" / "IMDb ID en TMDb". Unificar en un componente sin proveedores.
  `PantallaNoDisponible.svelte:26-29`, `AvisoNoDisponible.svelte:26`
- [x] Bienvenida: paso idioma → "Idioma de los títulos y el audio"; "Saltar" →
  "Saltar y configurar después"; aviso en "Listo" si saltó TMDb.
  `bienvenida/+page.svelte:297,304,351`

## Fase 3 — Sistema de diseño (base)

Cifras: 0 variables CSS globales, ~236 hex únicos en `src`, 6 naranjas de
marca (`#f5c518` x96, `#f3a951` x77, `#f97316`, `#ffae5c`, `#ffb86b`,
`#fbbf24`), 17 grises de texto, 44 font-sizes (todo px, 67 < 12px),
17 radios, 19 z-index, ~25 transiciones, `prefers-reduced-motion` en 2 sitios.

- [x] **`src/lib/tokens.css`** importado en `+layout.svelte`: `--brand`,
  `--brand-hot` (amarillo foco), superficies `--surface-0..3`, `--line`,
  `--text/-2/-3` (piso AA, nada bajo `#9a9aa4`), `--ok`, `--err`, `--scrim`,
  radios `--r-sm/md/lg/pill`, `--z-base/dropdown/modal/toast/splash`,
  `--t-fast/med`, `--ease`. Usar mismos nombres que `control.html`
  (`--bg --panel --panel-2 --border --text --muted --accent`).
- [x] **Foco global**: `[data-nav]:focus-visible { outline: 3px solid
  var(--brand-hot); outline-offset: 2px }`. Revisar los 36 `outline:none`
  (ResumePill:153,213 sin reemplazo; PlayMenu sí tiene glow).
- [x] **Escala tipográfica** (piso de 11px; escala para TV con "Tamaño de la interfaz" en Sistema: `setZoom` del webview, auto según `screen.width`/1920 de a 25 %, 100-200 %, por equipo, no viaja con la cuenta)
- [x] **Contraste**: eliminar `#555/#666/#6e6e78/#777` como color de texto.
- [x] **Reduced motion** global en tokens.css.
- [~] **Responsive** (hecho: `clamp(340px, 30vw, 560px)`; pendiente: ficha como overlay bajo 900px): ficha `clamp(340px, 28vw, 520px)` en vez de 480px fijos
  (`+page.svelte:3242`); < 900px ficha como overlay.
- [x] **z-index**: toast (850) bajo ResumePill (900) → a capa toast.
- [x] **control.html**: skin por defecto naranja (hoy rojo `#e23b3b`), textos
  10-11px → 12-13px.

## Fase 4 — Dinámicas (esfuerzo M)

- [x] **Fila de fuente en dos niveles**: L1 grande "1080p · Latino · 2.4 GB ·
  ⚡ al instante"; L2 chica códec/HDR/audio; nombre del release solo en la
  enfocada; máx. 3 badges (bloqueada > idioma > instantánea); "Recomendada" en
  la primera. `SourcePicker.svelte:2790-2822`, `fuentes.ts:260-283`
- [~] **Carga inicial con progreso** (hecho: etapas + segundos y aviso a los 12 s; "Seguir con las que llegaron" necesita que kodios_search emita resultados parciales desde Rust): etapa + N fuentes; a los ~15 s "Seguir con
  las N que llegaron". Requiere callback de progreso en `buscarFuentes`
  (`precarga.ts`). `SourcePicker.svelte:2608`
- [x] **Corte a mitad**: enfocar la siguiente no descartada + "Probar la
  siguiente y seguir en 40:12". `SourcePicker.svelte:2247-2290`
- [x] **Jerga**: "swarm", "DMCA", "debrid" → frase de consecuencia; detalle
  plegable. `SourcePicker.svelte:2616-2738`
- [x] **TabReproduccion**: Audio/subs arriba; resto plegado "Avanzado (fuentes y
  red)"; reescribir "Cacheo de red", "release/grupo", etc.
  `TabReproduccion.svelte:142-330`
- [x] (título y nota/temporadas/premios al enfocar; en reposo solo estado, no disponible, trailer, favorito) **Cards con título** (una línea bajo el póster o fallback en `.no-poster`)
  y máx. 2-3 badges. `+page.svelte:3110-3114`
- [x] **Pastillas** (ResumePill, CastPill) alcanzables con flechas, contenedor
  único apilado, `padding-bottom` del grid cuando hay píldora.
- [x] **Dropdowns orden/temporada**: cerrar con ←/→ o al perder foco; géneros
  ocultos sin indicador (max-height 78px) → "+N" o dropdown "Género ▾".
  `+page.svelte:2904-2919`, `:3388`
- [~] **Header en TV** (hecho: etiqueta del ícono al enfocar, botones ≥40px, descripción de accesos al enfocar; pendiente: agrupar íconos en un menú en modo TV): 8-10 íconos emoji sin texto antes del grid; agrupar en
  modo TV/kiosko; objetivos ≥ 44px. Subtítulo visible en cards de acceso (Vera,
  Vistos, IPTV). `Header.svelte:190-325`, `+page.svelte:3009-3085`
- [x] (favoritos y recientes como categorías + ★, botón y tecla F) **IPTV**: fila "Últimos canales" + ⭐ favoritos; "Quitar filtros" en vacío;
  aviso si falló una lista (hoy `console.warn`). `iptv/+page.svelte:96, 441`
- [~] **Bienvenida más corta** (hecho: "Qué necesitas" fundido en Catálogo, 6→5 pasos; opcionales queda porque trae doblado/subtitulado): fusionar "Qué necesitas" + "Catálogo"; opcionales
  tras "Más opciones"; saltar idioma si ya hay detectado.
- [x] **Ajustes**: validar key TMDb al guardar ("✓ válida / ✗ no funciona");
  aviso de guardado abajo (hoy arriba a la derecha 1.6 s); confirmar modo
  Kiosko. `guardar.svelte.ts`, `TabSistema.svelte:120`
- [x] **Cuenta Kütral**: explicar qué es la URL del servidor; precargar default
  si hay. `TabCuenta.svelte:682-730`

## Fase 5 — Componentes compartidos (esfuerzo L)

- [ ] `Modal.svelte` (Esc/Backspace/click fuera/foco inicial, `role="dialog"
  aria-modal`) — 6 modales propios hoy.
- [x] (`src/lib/ui/FondoDifuminado.svelte`) `FondoDifuminado.svelte` — bloque repetido en `.sp-bg`, `.ep-bg`,
  `.pm-bg`, `.pc-bg`.
- [x] (`src/lib/ui/Spinner.svelte`, también los de /config) `Spinner.svelte` — 4 keyframes iguales (`ep-spin`, `sp-spin`, `cfg-spin`,
  `spin`).
- [~] (no hace falta: Notifications es la campana/historial, sin emergente; el toast del inicio es el único aviso pasajero) Unificar toast de `+page.svelte:3894` con `Notifications.svelte`.
- [ ] `Boton.svelte` (22 definiciones `.btn*` en 10 archivos), `Chip.svelte`,
  `PosterCard.svelte`.
- [ ] `Icono.svelte` SVG con `currentColor` (~25 íconos) en vez de ~254 emojis;
  empezar por Header y pestañas de config.
- [ ] Sacar CSS común de `+page.svelte` y `SourcePicker.svelte` a `src/lib/ui/`.

## Pedido: filtro de edad en el perfil de niños (2026-11-01)

Anotado, sin implementar. Hay animaciones que no son para niños y se cuelan.

**Cómo filtra hoy** (`ninos.ts`, `tmdb.rs::filtro_ninos`, `anilist.rs`):
- Pelis (discover): certificación de EE. UU. hasta PG, sin terror. OK.
- Series (discover): solo géneros infantil/familia, sin crimen/guerra/misterio.
  TMDb no filtra series por clasificación.
- Búsqueda y sugerencias: solo por géneros en el front (`aptoNinos`), **sin
  mirar la clasificación**: ahí se cuela lo animado con género "Familia".
- **Anime: el agujero grande.** Solo `isAdult: false` y fuera Ecchi, Hentai,
  Horror, Psychological, Thriller. Un shōnen violento (Action/Drama/Fantasy)
  pasa.

**De dónde sacar la edad (todo gratis, sin clave nueva):**
- Pelis: TMDb `movie/{id}/release_dates` → certificación por país (CL, MX,
  ES, US…). El discover ya acepta `certification.lte`.
- Series: TMDb `tv/{id}/content_ratings` → TV-Y, TV-Y7, TV-G, TV-PG, TV-14,
  TV-MA (y por país). No va en discover: un pedido por título, cacheable.
- Anime: **Kitsu** `anime/{id}` → `ageRating` (G, PG, R, R18) y
  `ageRatingGuide`. Ya tenemos `kitsu_id` de cada anime. AniList no da edad,
  solo `isAdult` y tags (Gore, Nudity, Violence con `rank`) como respaldo.

**Idea de diseño:** el perfil de niños guarda una edad tope (p. ej. "hasta 6",
"hasta 12"); cada fuente se mapea a esa escala (G/TV-Y → 0, PG/TV-Y7 → 7,
TV-PG → 10, PG-13/TV-14 → 13, R/TV-MA → 18). Sin clasificación conocida =
fuera (mismo criterio grueso de `ninos.ts`). Aplicarlo también a búsqueda,
sugerencias, Vera y audiolibros.

## A decidir por el usuario

- [~] (decidido 2026-11-01: NO, nada arranca solo) Siguiente capítulo automático con cuenta regresiva (opt-in, apagado por
  defecto). Choca con "nada arranca solo".
- [~] (decidido 2026-11-01: no por ahora) PIN para salir del perfil de niños / editar perfiles.
- [~] (decidido 2026-11-01: se queda así, es sustantivo) Pestaña de config "Reproducción": ¿renombrar?

## No tocar (ya está bien)

- Enter en card → Descubrir en un paso; foco amarillo 3px de cards.
- Lista que no se vacía al cargar (`grid-wrap.cargando`).
- Auto-elección de fuente (local primero, `rdFallas`, `desdePack`, TV no salta
  de versión); reanudación con colchón de 5 s.
- QR de teclado por celular; asistente con foco automático por paso.
- EpisodePicker arranca en el capítulo que sigue.
