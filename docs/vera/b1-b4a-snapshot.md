# Vera — snapshot del rediseño B1–B4a (cancelado a mitad de B4a)

Este documento captura el estado del recomendador Vera al momento de cancelar
el trabajo. Sirve como referencia si en el futuro se retoma este camino, o
como contexto para decidir uno nuevo.

## Por qué se canceló

Después de varios fixes incrementales (recortar géneros, cruzar con tono,
ponderar `determinarTono`), el sistema seguía siendo frágil: el clasificador
de tono dependía de heurísticas, los filtros peleaban contra los géneros que
TMDb devuelve, y aparecía un bug nuevo (B4a.1) sobre el caso degenerado
"todo el mazo calificado 5★ → motor no decide". La dirección general
funciona, pero el camino se volvió de remiendos sucesivos.

## Lo que sí quedó funcional

- **Catálogo dinámico desde TMDb** vía `/discover/movie` (B1) con filtros
  parametrizables (`with_genres`, `vote_count_gte`, etc).
- **Pool fresco por sesión** con cache 24h por hash(filtros + semillas),
  purga de entradas vencidas, migración de cache vieja.
- **Recomendaciones por semilla** vía `/movie/{id}/recommendations` (B1).
- **Señales del usuario conectadas** (B2): `vera_ratings_v1` (localStorage)
  + `watch_history` (SQLite) alimentan semillas y exclusiones.
- **Pantalla de intent** (B3) entre contexto y mazo: liviano / denso /
  adrenalina / sorpresa. Cada uno mapea a `FiltrosDiscover`.
- **Merge con prioridad disc → reco** (B3 fix tras detectar contaminación):
  el intent específico manda sobre el historial. Solo "sorpresa" deja al
  historial dominar.
- **Filtro `compatibleConIntencion`** sobre reco y fallback para que no
  cuelen pelis de género incompatible con el intent.
- **Cache de géneros local** (`vera_generos_cache`) poblada por `mapear()`,
  consumida por `getPerfilHistorico()` sin llamadas extras a TMDb.
- **Procedencia (`discover` / `reco` / `fallback`) marcada en cada peli**
  (B4a), usada por el motor como señal.
- **Motor con scoring combinado** (B4a):
  `score = 0.55·perfil + 0.30·vote_average/10 + 0.15·procedencia` + sesgo
  nocturno reducido (0.3). Tie-break determinista con `mulberry32` seeded
  por el pool, cero `Math.random`.
- **Selección segura/distinta/sorpresa** con la regla "sorpresa = género
  fuera del top-3 del perfil AND vote_average ≥ 7".
- **Verificación de boot** de la tabla canónica `GENEROS_TMDB` contra los
  sets locales.
- **UX intacta**: flujo entrada → contexto → intencion → mazo → tadán,
  atajos de 7 teclas (← → ↑ ↓ Enter Esc I), overlay de ayuda global,
  burbuja Habla.

## Bugs conocidos al momento de cancelar

1. **Contaminación de tono en pool de "liviano"** (causa raíz del cancel).
   Aplicamos en orden:
   - coma → pipe en `with_genres` (TMDb OR en vez de AND): mejoró pero
     trajo Aventura/Sci-fi/Acción puras.
   - Recortar liviano a `35|16|10751` (Comedia + Animación + Familia): dejó
     fuera Mortal Kombat / Avatar / Interstellar, pero permitió Acción-
     Comedia (Accident Man, 48 horas más) porque tienen Comedia como
     subgénero.
   - **Pendiente**: `PESOS_TONO` ponderado + cruce de `compatibleConIntencion`
     con tono. Se aplicó código pero NO se validó (cancelado antes del
     rerun final).

2. **B4a.1 — caso degenerado "todo el mazo 5★"**. Si el usuario califica
   todas las cartas con la nota máxima, el perfil de género queda plano y
   el motor no diferencia bien segura/distinta/sorpresa. **Diagnóstico
   pendiente**: no se aisló el síntoma exacto (¿devuelve null? ¿elige al
   azar por seed? ¿segura y distinta son la misma peli?). No hay fix
   propuesto todavía — esperaba evidencia del usuario.

3. **Fallback puede quedar contaminado**: `SEMILLA_FALLBACK` se filtra con
   `compatibleConIntencion`, pero si todas las semillas fallback son
   incompatibles, el pool queda por debajo de `MIN_POOL` sin mecanismo de
   relleno secundario. Aceptado como "honesto: mejor 5 livianos genuinos
   que 8 mezclados".

4. **`determinarTono` solo a partir de géneros TMDb**: una peli con
   géneros [Comedia, Crimen] queda en 0 → tono = liviano por default.
   Borderline. La versión ponderada lo arregla parcialmente pero no
   captura subtexto (ej. "68 Kill" es dark comedy, marca liviano).

## Pendientes que no se llegaron a tocar

- **B4b**: frecuencia de géneros históricos amplia, popularidad TMDb real,
  MMR para diversificación si "siempre 3 del mismo género" persiste.
- **B3.1**: re-mapeo de controles físicos (Backspace = atrás, R1/R2/Start/
  Select como entradas separadas). Solo decidido, no aplicado.
- **Player handoff** (Vera → home con `?play=<tmdb_id>`): diseño acordado
  (opción i = URL param con tmdb_id, home hace `tmdb_detail` y dispara
  `mode=discover`), `imdb_id` agregado a `Pelicula` pendiente.
- **Causa 1 nominal** (AND vs OR en TMDb): se trató con coma→pipe en
  `intenciones.ts`. Funciona con `|` raw sin URL-encode.

## Arquitectura final del recomendador (B4a)

```
+page.svelte (UX)
    │
    │ elegirIntencionActual()
    ▼
cargarCatalogoPorIntent(filtros)
    │
    ├─→ getSemillas(3)        ← vera_ratings_v1 + watch_history SQLite
    ├─→ getVistas()           ← idem
    │
    ▼
construirPool(filtros, semillas, vistas)
    │
    ├─→ fetchDiscover(filtros)         ← tmdb_discover (Rust)
    ├─→ fetchRecommendations(semillas) ← tmdb_recommendations (Rust)
    │
    ├─→ compatibleConIntencion(p, idsIntencion)  ← filtro tono + género
    │
    ├─→ merge: disc primero, reco rellena
    ├─→ fallback si pool < 8 (SEMILLA_FALLBACK enriquecido y filtrado)
    │
    └─→ cache localStorage[vera_pool_<hash>] (TTL 24h)

cerrarMazo() {
  recs = await recomendar(catalogo, estado)
}

recomendar(catalogo, estado) {
    │
    ├─→ filtrar (vistas, family si niños)
    ├─→ getPerfilHistorico()  ← cruza ratings + watch_history + cache géneros
    ├─→ combinarPerfil(historico, reaccionesSesion)
    │
    ├─→ scorePeli(p, generos, tonos, esNoche)
    │       = 0.55·perfil + 0.30·vote + 0.15·procedencia + sesgo_noche*
    │
    ├─→ sort por score, tie-break con mulberry32(seedDelPool(...))
    │
    └─→ pick segura / distinta (top-10, género distinto) /
              sorpresa (top-3 fuera del perfil AND vote ≥ 7, puede ser null)
}
```

## Mapa de archivos al momento del cancel

```
src/lib/vera/
  tipos.ts          ← + Procedencia, + PerfilHistorico, + intencion en EstadoVera
  intenciones.ts    ← tabla intent → FiltrosDiscover, sorpresa con sort_by random
  tmdb.ts           ← cliente TMDb + cache pool + cache géneros + PESOS_TONO
                      + compatibleConIntencion con tono
  probes.ts         ← SEMILLA_FALLBACK (14 IDs hand-picked, fallback only)
  historial.ts      ← getSemillas, getVistas, getPerfilHistorico
  ratings.ts        ← getRatingsMap, setRating (localStorage)
  motor.ts          ← recomendar() async, scoring B4a, mulberry32 seeded
  config.ts         ← CARTAS_MAZO, UMBRAL_NO_MATCH, HORA_NOCHE, etc

src/lib/habla/
  Habla.svelte      ← burbuja contextual (intacto desde flujo original)
  frases.ts         ← plantillas por contexto

src/lib/atajos/
  Ayuda.svelte      ← overlay global (I = ayuda)
  store.svelte.ts   ← ayuda store compartido

src/routes/vera/
  +page.svelte      ← flujo completo, 7 teclas, sin player real
  catalog/+page.svelte ← importador TMDb (admin, separado)

src-tauri/src/lib.rs
  tmdb_discover     ← extendido con 5 params opcionales (vote_average_gte,
                     vote_count_gte, primary_release_date_*, with_original_language)
  tmdb_recommendations ← NUEVO comando, soporta "recommendations" y "similar"
```

## Decisiones de producto registradas (algunas vigentes, otras a revisar)

| # | Decisión | Estado |
|---|---|---|
| 1 | "Niños nunca solos" (regla de marca) | vigente |
| 2 | Mazo = 8 cartas calibración, no recomendación final | vigente |
| 3 | Vera no penaliza descartes ("pasar" no pesa) | vigente |
| 4 | Solo señales positivas (vista con rating + interes) | vigente |
| 5 | Intent es lo primero después del contexto | vigente |
| 6 | Cache 24h por hash | vigente |
| 7 | Determinismo total: cero Math.random en motor | vigente |
| 8 | Mejor pool chico genuino que pool grande contaminado | vigente |
| 9 | TMDb /recommendations apalanca su collaborative filtering | vigente |
| 10 | Top-3 del perfil para definir "género no visto" en sorpresa | vigente, pero perfil débil cuando historial pequeño |
| 11 | Procedencia (discover/reco/fallback) como señal en score | vigente |
| 12 | Pesos del score: 0.55 perfil / 0.30 vote / 0.15 procedencia | vigente, sin validar empíricamente |

## Lo que aprendimos en el camino (sirve para lo que venga)

- **TMDb géneros son demasiado coarse**. "Aventura" es Indiana Jones Y Up.
  Cualquier sistema que use géneros TMDb como única señal va a tener
  fronteras borrosas. La solución de remiendo (recortar géneros + ponderar
  + cruzar tono) parchea pero no resuelve.

- **El historial del usuario "filtra" la recomendación de forma
  asimétrica**: si vio mucho Acción en la home, todo `/recommendations` va
  a traer Acción. Si su intent actual es liviano, la prioridad debe
  invertirse (intent sobre historial). Quedó codificado en
  `compatibleConIntencion`.

- **Caso degenerado "todo me gusta" rompe los selectores**. No se llegó a
  diagnosticar pero claramente existe. Cualquier rediseño debe contemplar
  qué pasa cuando el perfil es plano (todo positivo) o vacío (usuario
  nuevo).

- **La UX de las 3 cartas (segura/distinta/sorpresa) presupone
  diferenciación**. Si el motor no puede diferenciar, la UX queda hueca.

- **Cache 24h con hash por filtros funciona bien** y simplifica mucho la
  vida vs cache por sesión o sin TTL.

- **El cache de géneros local** (`vera_generos_cache`) es la pieza clave
  que hace barato el perfil histórico. Pattern reutilizable: si necesitás
  metadata de N IDs y ya pasaste por sus detalles, cacheá los campos que
  vas a re-leer.

- **Logs `[B3-test]` y `[B4a-pool]` como instrumentación temporal** fueron
  útiles para validar sin levantar tests automatizados. Patrón:
  `// TODO REMOVE` + grep para borrar.

## Estado del código al cancelar

- **Tono ponderado aplicado pero no validado**: `PESOS_TONO` está en el
  código pero el último rerun del usuario para validar quedó pendiente.
- **Logs `[B4a-pool]` siguen activos** en `src/lib/vera/tmdb.ts` con
  marcador `// TODO REMOVE`. Borrar antes de cualquier merge a main.
- **`svelte-check` verde** al momento del cancel (0 errores en
  `src/lib/vera/*` y `src/routes/vera/*`).
- **Rust compila** (`cargo check` verde con los comandos nuevos).
- **`tmdb_recommendations` comando Rust nuevo** ya registrado y funcional.

## Cómo retomar si se quiere

Si en algún momento se quiere volver a esta dirección:

1. Borrar los `// TODO REMOVE [B4a-pool]` en `src/lib/vera/tmdb.ts`.
2. Aislar y arreglar B4a.1 (caso "todo 5★"): probable fix con un
   `umbralDiferenciacion` que devuelva null si segura/distinta empatan
   demasiado, o forzar tie-break por características externas (rating,
   procedencia).
3. Validar el fix de tono con el pool de "liviano" (eso es lo que quedó
   sin hacer).
4. Aplicar B3.1 (Backspace + controller mapping).
5. Aplicar player handoff (Vera → home con `?play=<tmdb_id>`).
6. Considerar B4b (popularidad TMDb real, frecuencias históricas amplias,
   MMR) solo si la diversificación sigue siendo problema.

## Lo que NO debe sobrevivir si se cambia de dirección

- Las heurísticas `determinarTono` / `compatibleConIntencion` con géneros
  TMDb. Son frágiles por naturaleza de la fuente.
- El supuesto de que "intent específico = filtrar duro por género". Si el
  usuario quiere "liviano" pero le encanta Pulp Fiction, el sistema lo
  trata como dos personas distintas. Algo que reconozca la coexistencia
  de gustos contradictorios sería superior.
- Los pesos hardcoded `0.55 / 0.30 / 0.15`. Sin validación empírica son
  estimaciones razonables pero no fundadas.
