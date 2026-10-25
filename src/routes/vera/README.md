# Vera and Chill

Vera elige qué ver. Se le dice con quién estás y qué buscas, se califican unas
pocas películas, y devuelve un ranking explicado.

Vive en `/vera`. La importación de catálogo (admin) está en `/vera/catalog`.

## Flujo

Una pantalla, dos toques. Sin cuestionario.

1. **¿Con quién?** — Solo · En pareja · Con amigos · Con niños. Regla de marca:
   "con niños" SIEMPRE implica un adulto presente; activa el filtro familiar y
   la clasificación por edad en TMDb. En un perfil de niños queda fijo.
2. **¿Qué te tinca?** — Reírme · Emocionarme · Adrenalina · Algo para pensar ·
   Sorpréndeme. De acá salen los géneros de `/discover` y el tono que el motor
   respeta. Elegirlo ya busca.
   - Opcional: **¿Tienes poco tiempo?** (hasta 1 h 50) y **Lo que nunca quiero
     ver** (géneros y temas; se guarda en `vera_setup`).
3. **Tres cartas** (`tres.ts`), sacadas del ranking del motor:
   - **La segura:** lo que más calza contigo.
   - **Una distinta:** buena, pero de otro género que la segura.
   - **La sorpresa:** la menos conocida entre las bien valoradas.

   Cada una con una frase ("Porque viste *Coco*") y **Descubrir**, **Otra**
   (cambia solo esa) y **Ya la vi** (👍 / 👎 / ni fu ni fa, y se cambia). Abajo,
   **Otras tres** y volver a cambiar lo pedido. Lo que ya viste no se propone.

Antes de mostrar una carta se completa con el detalle de TMDb y se descarta si
no se puede ver (sin imdb), si choca con lo que nunca quieres ver (los temas se
saben recién con el detalle) o si pediste algo corto y no lo es.

## Cómo aprende

Sin calificar nada a propósito. Por perfil (el que está viendo):

| Señal | Peso |
|---|---|
| 👍 / 👎 ("Ya la vi", o al volver después de ver una que eligió) | ±4 |
| Favorita | +2 |
| Vista completa | +1 |
| Dejada a medias (5–40 %, sin volver en 2 días) | −0,75 |

Las favoritas y lo último que terminaste son las semillas de
`/recommendations`. Lo que se vio desde la home no pasó por Vera: sus géneros
se piden a TMDb una vez y quedan en `vera_generos_cache` (`asegurarGeneros`).
Lo calificado con la Vera vieja (−5..+5) se sigue usando.

Después de Descubrir, la peli queda anotada (`elegidas.ts`). La próxima vez que
se abre Vera, si ya se vio (terminada o más de la mitad), pregunta "¿Qué tal?"
una sola vez. Responder es opcional.

## Atajos (obligatorio — todo se maneja sin ratón)

| Tecla | Acción |
|---|---|
| `←` `→` `↑` `↓` | Moverse (navegación espacial) |
| `Enter` | Elegir · Descubrir |
| `Esc` | Paso atrás (desde las preguntas, sale a home) |
| `Backspace` | Salir directo a la home |
| `I-I` | Ayuda |

Palabra de marca: la acción de reproducir se llama **Descubrir**. Nunca
"Reproducir" ni "Play".

## El motor

Score en 0..1:

```
base   = 0.55·afinidad + 0.30·calidad + 0.15·procedencia
score  = base + contexto [+ sesgo nocturno] − penalización vista − penalización duración
```

- **Afinidad** — promedio ponderado por confianza de las señales disponibles
  (género, tono, director, reparto, década). Promedio, no suma: una peli con
  cuatro géneros no arranca con 4× la afinidad de una con uno.
- **Calidad** — nota bayesiana estilo IMDb: `(v/(v+m))·R + (m/(v+m))·C`. Sin
  esto, un 8.9 con 110 votos le gana a un 8.2 con 12.000.
- **Procedencia** — de dónde salió al pool: `reco` (recomendación de algo que
  te gustó) pesa más que `discover`, y `fallback` no pesa.
- **Contexto** — quién está en el sillón mueve el orden, con tope.
- **Sesgo nocturno** — solo si NO pediste un tono. Pedir "algo denso" a las 23h
  y que el motor te penalice lo denso era un bug.
- **Vistas** — bajan, no desaparecen. Cuánto bajan depende de cómo las
  calificaste: rever algo que te encantó es un plan, rever algo que te dejó
  frío no.
- **Duración** — el tope del perfil es preferencia, no prohibición: penaliza,
  no excluye.
- **Idioma** — dos ejes distintos, a propósito. Los idiomas **cómodos** son
  sobre leer subtítulos: lo que queda fuera baja pero sigue estando. Los
  idiomas **evitados** son sobre el sonido: eso sí excluye, porque si no
  soportas cómo suena una lengua no hay ranking que lo arregle. Una película
  cuyo idioma no conocemos nunca se castiga por esa duda.
- **Animación** — eje propio, no una exclusión de género más: "no me gustan"
  se podría expresar vetando el género, pero "me encantan" no, y esa asimetría
  es justo la que importa.
- **Diversidad** — re-ranking MMR sobre las primeras posiciones, para que el
  top no sean cinco veces la misma película con distinto título.

Determinista: sin `Math.random`. Los empates los rompe un PRNG con semilla
derivada del pool, así que el mismo pool da siempre el mismo orden.

## De dónde salen las películas

Tres fuentes en paralelo, un request cada una:

- `/discover` con los géneros del intent (`page` y `sort_by` aleatorios, para
  que dos entradas seguidas no traigan lo mismo).
- `/recommendations` sobre tus semillas (lo que te gustó, tus favoritas y lo
  último que terminaste).
- `/discover` con la keyword LGBT+ `158718`, presencia garantizada. Sin marcar,
  sin sección aparte: mezclada.

Si el pool queda corto: catálogo local importado; después `SEMILLA_FALLBACK`.
Sin API key pero con catálogo importado, Vera propone igual desde lo local.

**Enriquecimiento perezoso.** Los listados de TMDb ya traen título, sinopsis,
póster, `vote_average`, `vote_count` y `genre_ids` — todo lo que el motor
necesita para rankear. `/detail` se pide solo para las pelis que vas a mirar:
las primeras del ranking antes de mostrar las cartas, y la que reemplaza a una
carta al pedir otra.

## Persistencia

| Dónde | Qué | Sobrevive a |
|---|---|---|
| `localStorage: vera_reacciones_v2` | interés y juicio por peli | reinicio de la app |
| `localStorage: vera_generos_cache` | géneros por tmdb_id | reinicio de la app |
| `SQLite: vera_setup` | perfil (plataformas, exclusiones, idiomas, animación, voz, duración) | todo |
| `SQLite: vera_weights` | memoria larga por etiqueta | borrar el navegador |
| `SQLite: vera_feedback` | qué elegiste descubrir | todo |
| `SQLite: vera_titles` | catálogo importado desde `/vera/catalog` | todo |
| `SQLite: watch_history` | lo que reprodujiste (lo escribe la home) | todo |

`vera_weights` acumula **deltas**, no totales: cuando cambia una reacción se
aplica `(pesoNuevo − pesoAnterior)` a cada etiqueta de esa peli. Si se aplicara
solo el peso nuevo, recalificar la misma peli la contaría dos veces.

## Temas sensibles

Los 24 temas de `vera_theme_list` salen de las keywords de TMDb vía
`map_keyword_to_themes` (Rust). El mismo mapeo lo usan el importador de
catálogo y `tmdb_detail`, así que el filtro funciona tanto para títulos
importados como para los que llegan en vivo.

Lo que el usuario marca en el setup **no aparece**. No se muestra tachado, no
se sugiere con advertencia: no está. El ranking sí dice cuántas quedaron fuera,
porque filtrar en silencio hace parecer que Vera no encontró nada. Lo mismo
vale para las otras exclusiones duras: idiomas que no tolera y animación
cuando la rechazó.

Un tema sensible solo se puede juzgar con la película enriquecida: en una peli
del listado `temasSensibles: []` significa "todavía no miramos", no "no tiene".
Por eso el filtro de temas corre al enriquecer, y lo que aparece ahí se
descarta del pool en caliente.

## Constantes ajustables

`src/lib/vera/config.ts`:

| constante | qué hace |
|---|---|
| `UMBRAL_NO_MATCH` | score mínimo de la #1 para no admitir que no hay match |
| `HORA_NOCHE` | desde qué hora sesga a liviano (solo sin tono pedido) |
| `VOTOS_MINIMOS` / `MEDIA_GLOBAL_TMDB` | parámetros del rating bayesiano |
| `LAMBDA_MMR` / `TOPE_MMR` | cuánta diversidad y hasta qué posición |
| `PENAL_VISTA_BASE` / `PENAL_DURACION_MAX` | cuánto bajan vistas y pelis largas |
| `PENAL_IDIOMA_INCOMODO` | cuánto baja un idioma fuera de tu zona cómoda |
| `BONUS_ANIMACION` | cuánto sube la animación si dijiste que te encanta |
| `LOTE_ENRIQUECIMIENTO` | cuántos `/detail` en paralelo por lote |

## Estructura

```
src/lib/vera/
  tipos.ts           tipos del flujo
  config.ts          constantes ajustables
  generos.ts         tabla canónica: id TMDb ↔ nombre es-ES ↔ slug v3
  idiomas.ts         idiomas del setup (comodidad vs rechazo)
  intenciones.ts     intents → géneros → filtros de discover
  motor.ts           scoring, penalizaciones y diversidad
  tmdb.ts            pool, enriquecimiento perezoso, exclusiones
  db.ts              handle SQLite compartido
  setup.ts           perfil persistente (vera_setup)
  catalogoLocal.ts   lectura de vera_titles
  aprendizaje.ts     memoria larga (vera_weights, vera_feedback)
  historial.ts       vistas, semillas y perfil histórico (por perfil)
  ratings.ts         reacciones en localStorage
  probes.ts          semilla de respaldo hand-picked
  tres.ts            las tres cartas: segura, distinta, sorpresa
  elegidas.ts        lo elegido en Vera, para el 👍/👎 de después
  import.ts          importador TMDb (usado por /vera/catalog)
src/routes/vera/
  +page.svelte           orquestador del flujo
  catalog/+page.svelte   admin de importación TMDb
  README.md              este archivo
```
