# Vera — snapshot B5–B8 (estado actual, vivo)

Estado del recomendador Vera al cierre de B8. Reemplaza al snapshot anterior
(`vera-b1-b4a-snapshot.md`) que documentaba una dirección cancelada. Lo que
está acá ESTÁ en el código, funciona y compila verde.

## Flujo actual (B6)

```
entrada → contexto → intencion → CALIFICAR (8 cartas 1x1) → RANKING (navegable)
                                       │                         │
                                       │                         └→ Enter → handoff a home/?play=...&type=movie
                                       │
                                       └→ Enter avanza, ↓ salta carta (-1), ↑ toggle vista
```

### Pantalla por pantalla

- **entrada**: marca "Vera" con gradient arcoíris, "and Chill" naranja brasa, slogan, botón Empezar.
- **contexto**: 4 opciones (Solo / Pareja / Amigos / Niños). Niños incluye nota "con un adulto cerca" (regla de marca).
- **intencion**: 4 opciones (Liviano / Denso / Adrenalina / Sorpresa). Confirmar dispara la carga del pool TMDb + entra a calificar.
- **calificar**: una carta a la vez, 8 en total. Muestra ficha completa (poster, descripción, director, actores, trivia) + escala -5..+5 visible + estado actual ("Tu interés" o "Tu juicio" si la marcaste vista).
- **ranking**: la #1 destacada arriba grande, resto debajo en filas chicas. Backdrop al fondo. Botón "▶ Descubrir" dispara handoff a home.

## Atajos (B8, consistentes en toda la app)

| Tecla | Función global |
|---|---|
| **Esc** | Volver atrás un paso. Si ya estás en la primera pantalla, sale a home (`/`). |
| **Backspace** | Salir directo a la home `/`, sin importar la pantalla. |
| **I-I** | Doble tap rápido = abrir/cerrar overlay de ayuda. |
| **Modales** | Esc y Backspace cierran cualquier modal (ayuda, person, unavailable, exit confirm). |

### Por pantalla (Vera)

| Pantalla | Específicos |
|---|---|
| entrada | Enter empezar |
| contexto | ← → ↑ ↓ mover, Enter confirmar |
| intencion | ← → elegir, Enter confirmar (dispara carga) |
| calificar | ← → ajustar -5..+5, ↑ toggle vista, ↓ saltar (asigna -1), Enter siguiente carta |
| ranking | ↑ ↓ navegar pelis, Enter Descubrir |

### Home

| Pantalla | Específicos |
|---|---|
| browse | ← → ↑ ↓ navegar cards, Enter abrir/descubrir. Esc/Backspace no hacen nada (raíz). |
| discover (iframe) | Esc/Backspace cierran player y vuelven a browse. |
| modales (person/unavailable/key) | Esc/Backspace cierran. |

### Lo que NO existe (decisiones explícitas)

- **"Ir al ranking ya" desde calificar**: NO hay atajo. Calificás las 8 sí o sí (o saltás con ↓).
- **"Otra ronda" desde ranking**: NO hay atajo. Salís a home con Backspace, reentrás a Vera para nuevo pool.
- **Player propio de Vera**: NO existe. Descubrir hace handoff a home y home reproduce con iframe playimdb.
- **Trailers en calificar**: NO están. Se decidió γ (sin trailer en B6). Pendiente futuro.

## Modelo de datos

### `src/lib/vera/tipos.ts`

```ts
type Tono = "liviano" | "denso";
type Contexto = "solo" | "pareja" | "amigos" | "ninos";
type Intencion = "liviano" | "denso" | "adrenalina" | "sorpresa";
type Procedencia = "discover" | "reco" | "diversidad" | "fallback";

interface Reaccion {
  pelicula: Pelicula;
  interes: number;        // -5..+5
  juicio: number | null;  // -5..+5 si vista; null si no
  // "Visto" = derivado: (juicio !== null). Sin campo flag separado.
  // Regla: juicio MANDA sobre interes cuando ambos existen.
}

interface Pelicula {
  id, titulo, generos, tono, familyFriendly, rating, popularidad, pais,
  poster, gancho, descripcion, director, actores, anio, runtime,
  posterPath, backdropPath, imagenes (extras), trivia, procedencia
}

interface RankingPeli { pelicula: Pelicula; score: number; }
```

### Persistencia (localStorage)

- `tmdb_key` — API key TMDb.
- `vera_reacciones_v2` — `{ [peliId]: { tmdb, interes, juicio } }`. Sobrevive entre sesiones.
- `vera_generos_cache` — `{ [tmdbId]: string[] }`. Géneros cacheados por id, sin TTL. Alimenta perfil histórico sin pegarle a TMDb.

### Caches viejas (NO usar)

- `vera_catalogo_v2` (B1 fijo) — purgada one-shot.
- `vera_pool_*` (B5 hash-based) — purgada one-shot.
- `vera_ratings_v1` (pre-B5) — abandonada, sin uso, no se lee.

### SQLite (compartido con home)

- `watch_history` — escrita por home al reproducir. Vera la lee para perfil histórico (proxy débil: `completed=1` aporta +0.5 por género).
- `unavailable_items` — escrita por home cuando una peli falla en playimdb. Vera NO la lee todavía (B7 pendiente).

## Motor (`src/lib/vera/motor.ts`)

Devuelve `RankingPeli[]` (lista entera del pool ordenada por score).

```
score = 0.55 × perfil_norm + 0.30 × vote_average/10 + 0.15 × procedencia
        + sesgo_nocturno (±0.3 si tono coincide con horario)

perfil = histórico (de vera_reacciones_v2 + watch_history) + sesión actual
  pesoReaccionSesion(r) = juicio (si existe) | interes * 0.6
  Tono y género reciben el mismo peso (decisión documentada, simplificación
  vs B4a donde tono tenía 0.7 del peso de género).

procedencia: reco=1.0, discover/diversidad=0.5, fallback=0.0

Tie-break: mulberry32 seeded por hash FNV de ids del pool (determinista).
```

Cero `Math.random` en el motor. Filtros duros: solo `familyFriendly` cuando contexto=niños. Las vistas aparecen en ranking con badge `✓`.

## Pool dinámico (`src/lib/vera/tmdb.ts` + `intenciones.ts`)

### Sin cache (B6)

Cada entrada a Vera = pool fresco. No hay TTL ni cache de pool. La variedad entre sesiones se garantiza con:

- `pageAleatoria()` → 1-5 random.
- `sortAleatorio()` → entre `popularity.desc`, `vote_average.desc`, `revenue.desc` (para denso solo entre popularity y vote_average).

### Géneros TMDb por intent (pipe = OR)

| Intent | with_genres |
|---|---|
| liviano | `35\|16\|10751` (Comedia, Animación, Familia — recortados, sin Aventura/Romance que abrían a denso) |
| denso | `18\|99\|36\|9648` (Drama, Documental, Historia, Misterio) |
| adrenalina | `28\|53\|27\|878\|10752` (Acción, Suspense, Terror, Sci-Fi, Bélica) |
| sorpresa | `""` (sin género) |

### 4 fuentes del pool

1. **discover** — `tmdb_discover` con filtros del intent.
2. **reco** — `tmdb_recommendations` sobre semillas del usuario (pelis con juicio≥2 o interes≥3). Filtradas por compatibilidad con intent (matchea género AND tono esperado).
3. **diversidad** — `tmdb_discover?with_keywords=158718` (LGBT+). Garantiza 1 peli mínimo, mezclada sin etiquetar.
4. **fallback** — `SEMILLA_FALLBACK` (14 IDs hand-picked) solo si pool < 8.

Merge: disc primero, reco fills, diversidad inyecta, fallback rellena si falta.

### Filtro de gravedad (B3 fix)

- `PESOS_TONO` ponderado: Comedia/Animación/Familia +2, Romance/Aventura/Música/Fantasía +1, Drama/Doc/Historia/Misterio/Sci-Fi/Western −1, Acción/Crimen/Suspense −2, Terror/Bélica −3.
- `determinarTono(generos)` = suma de pesos, ≥0 → liviano, <0 → denso.
- `compatibleConIntencion(p, idsIntencion)` requiere AMBAS: género en común con intent AND tono coincide con el esperado del intent.

## Comandos Rust (`src-tauri/src/lib.rs`)

| Comando | Uso |
|---|---|
| `tmdb_discover` | Extendido con params Vera: `with_keywords`, `vote_average_gte`, `vote_count_gte`, `primary_release_date_*`, `with_original_language`. Coma=AND, pipe=OR. |
| `tmdb_recommendations` | Nuevo en B5. Soporta `/recommendations` y `/similar` via `kind`. |
| `tmdb_detail` | Sin cambios. Trae cast, director, runtime, géneros, imdb_id. |
| `tmdb_genres`, `tmdb_search`, `tmdb_videos`, `tmdb_person` | Existentes, sin cambios. |
| `inspect_player`, `sim_*` | Existen pero no se invocan desde frontend (debug removido en B8). |

## Handoff Vera → home

```
Vera ranking → Enter → goto("/?play=<tmdb_id>&type=movie")
                            ↓
                       home afterNavigate
                            ↓
                       parser ?play=
                            ↓
                       history.replaceState({}, "", "/")  ← antes de invocar
                            ↓
                       handoffPlay(id, "movie")
                            ↓
                       tmdb_detail → selected = d
                            ↓
                       loadProgressForSelected()
                            ↓
                       startDiscover() → mode="discover" → iframe playimdb
```

Race fix aplicado: `apiKey` se lee sync al principio de `onMount` para que `afterNavigate` la tenga disponible.

Si `tmdb_detail` falla en handoff: catch deja `selected=null, mode="browse"`. Si playimdb no tiene la peli: `check_url` la marca unavailable, mode→"unavailable". Pero hay un bug compuesto (B7.1): home cae al `selected` viejo persistido en memoria, reproduce la anterior. Pendiente.

## Reglas de marca

Memorias persistentes registradas en `~/.claude/projects/-home-nick-Escritorio-Proyectos-kutral/memory/`:

- **brand-no-kids-alone**: "con niños" implica adulto presente. Nunca "niño solo" como flujo.
- **brand-descubrir**: la acción de reproducir se llama "Descubrir". NUNCA "Reproducir" ni "Play".
- **feedback-spanish-neutral**: tú/eres/tienes, no vos/sos/tenés.

## Pendientes activos

### B7 — Filtrar no disponibles en Vera

Vera ofrece pelis sin saber si playimdb las tiene. Resultado: el handoff lleva la peli a home, home falla en `check_url`, y por bug compuesto (B7.1) reproduce la peli vieja.

Fix planificado:
1. `tipos.ts`: `Pelicula` gana `imdbId: string | null`.
2. `tmdb.ts mapear()`: extrae `d.imdb_id`.
3. `historial.ts`: nueva `getNoDisponibles(): Promise<Set<string>>` lee SQLite `unavailable_items`.
4. `tmdb.ts construirPool`: filtra pool por `!noDisponibles.has(p.imdbId)`.
5. Pelis sin imdb_id pasan (no descartar a ciegas).
6. La lista crece con uso — cada peli que falla en home queda marcada y no vuelve.

### B7.1 — Home no debe caer a `selected` viejo

Cuando una peli del handoff no está disponible, home muestra/reproduce la peli previa porque `selected` persiste en memoria. Fix: limpiar `selected = null` al inicio de `handoffPlay` antes de tmdb_detail, o al detectar unavailable.

Atacar DESPUÉS de B7 — si Vera filtra bien, B7.1 se dispara mucho menos.

### Trailers en calificar (B6.x, opcional)

TMDb `/movie/{id}/videos` está disponible, command `tmdb_videos` existe. Opciones:
- α (auto-muted): trailer Netflix-style sin sonido.
- β (con sonido + tecla Space): rompe la cota de 8 teclas, suma 9na.
- γ (sin trailer): aplicado en B6.

User flagged que los embeds YouTube fallan a veces (provider bloquea con "embed disabled"). Pre-fix: validar que los embeds funcionen antes de meter trailers.

## Bugs conocidos no relacionados

- `VolumeControl.svelte` / `BrightnessControl.svelte`: 6+ errores Svelte 5 (uso de runes como stores). No los toqué — no son de Vera. Probablemente alguien los introdujo en otra sesión y nunca compilaron limpios. svelte-check anteriormente los reportaba, ahora no aparecen en el conteo final — pueden haber sido arreglados o están fuera del path actual.

## Estructura de archivos al cierre B8

```
src/lib/vera/
  tipos.ts          ← modelo B5 + Procedencia diversidad + RankingPeli
  intenciones.ts    ← page+sort random, géneros pipe (OR), liviano recortado
  tmdb.ts           ← sin cache, 4 fuentes, compatibleConIntencion con tono
  motor.ts          ← scoring 0.55/0.30/0.15, mulberry32 seeded
  ratings.ts        ← vera_reacciones_v2 (interes + juicio)
  historial.ts      ← getPerfilHistorico, getSemillas, getVistas, getDb cacheado
  probes.ts         ← SEMILLA_FALLBACK 14 ids, fallback only
  config.ts         ← CARTAS_MAZO (no usado en B6 — calificar usa CARTAS_OBJETIVO inline)

src/lib/habla/
  Habla.svelte      ← burbuja, no se usa en B6 actualmente
  frases.ts         ← plantillas por contexto

src/lib/atajos/
  Ayuda.svelte      ← overlay global I-I (doble tap), Esc/Backspace cierran
  store.svelte.ts   ← ayuda store

src/routes/vera/
  +page.svelte      ← B6 + B8 (calificar/ranking, Esc/Backspace consistentes)
  catalog/+page.svelte ← importador TMDb (admin)

src/routes/+page.svelte (home)
  Handoff Vera (afterNavigate), debug removido (B8), modales con Esc+Backspace

src-tauri/src/lib.rs
  tmdb_discover (+ params Vera), tmdb_recommendations (nuevo)
```

## Comandos para retomar

```bash
# Type check
npx svelte-check --tsconfig ./tsconfig.json

# Rust check
cd src-tauri && cargo check

# Levantar app
npm run tauri dev
```

Estado svelte-check al guardar: **0 errores**, 3 warnings (1 tsconfig preexistente + 2 a11y warnings preexistentes en modales).

## Memorias en MEMORY.md (cross-session)

```
- [Fallback fuente datos](data_source_fallback.md)
- [Estrategia disponibilidad](availability_strategy.md)
- [Español neutro siempre](feedback_spanish_neutral.md)
- [Niños nunca solos](brand_no_kids_alone.md)
- [Descubrir, no play](brand_descubrir.md)
```
