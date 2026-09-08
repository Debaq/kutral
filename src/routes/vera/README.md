# Vera and Chill

Vera elige qué ver. Se le dice con quién estás y qué buscas, se califican unas
pocas películas, y devuelve un ranking explicado.

Vive en `/vera`. La importación de catálogo (admin) está en `/vera/catalog`.

## Flujo

```
entrada ──► contexto ──► intencion ──► calificar ──► ranking
   │
   └──► setup (opcional, una vez)
```

1. **Entrada** — `Empezar` o `Configurar a Vera`.
2. **Setup** (opcional) — 8 pasos: plataformas, postura frente a la animación,
   idiomas en los que te sientes cómodo, idiomas que no toleras oír, géneros que
   no quieres ver nunca, temas sensibles a evitar, tope de duración y voz de
   Vera. Se guarda en `vera_setup` y se aplica en todas las sesiones siguientes.
   Cada paso se salta con `Enter`.
3. **Contexto** — `Solo` / `En pareja` / `Con amigos o familia` / `Con niños`.
   Regla de marca: "con niños" SIEMPRE implica un adulto presente. `Con niños`
   además activa el filtro duro `familyFriendly`.
4. **Intención** — `Algo liviano` / `Algo denso` / `Adrenalina` / `Sorpresa`.
   De acá salen los géneros de `/discover` y el tono que el motor respeta.
5. **Calificar** — N cartas, una por vez. N sale de `cartasParaMuestras()`: 8 la
   primera vez, 3 cuando Vera ya tiene perfil. Se priorizan pelis que nunca
   calificaste.
6. **Ranking** — la #1 destacada con el porqué, y el resto navegable. Si ni la
   mejor llega a `UMBRAL_NO_MATCH`, Vera lo dice en vez de empujar la menos mala.

## Atajos (obligatorio — todo se maneja sin ratón)

| Pantalla | Tecla | Acción |
|---|---|---|
| Entrada | `←` `→` | Empezar / configurar |
| Setup | `←` `→` `↑` `↓` | Mover |
| Setup | `Espacio` | Marcar / desmarcar (pasos de selección múltiple) |
| Setup | `Enter` | Siguiente paso / guardar |
| Contexto · Intención | `←` `→` `↑` `↓` | Mover |
| Contexto · Intención | `Enter` | Confirmar |
| Calificar | `←` `→` | Ajustar puntaje (−5..+5) |
| Calificar | `↑` | Marcar / desmarcar como vista |
| Calificar | `↓` | Saltar (puntaje −1 automático) |
| Calificar | `Enter` | Siguiente carta |
| Ranking | `↑` `↓` | Navegar |
| Ranking | `Enter` | **Descubrir** |
| Ranking | `Espacio` | Otra ronda (pool nuevo) |
| **Cualquiera** | `Esc` | Paso atrás (desde entrada, sale a home) |
| **Cualquiera** | `Backspace` | Salir directo a la home |
| **Cualquiera** | `I-I` | Ayuda |

Palabra de marca: la acción de reproducir se llama **Descubrir**. Nunca
"Reproducir" ni "Play".

## Cómo califica el usuario

Dos escalas, una sola UI:

- `interes` −5..+5 — intuición previa, antes de verla.
- `juicio` −5..+5 o `null` — `null` significa que no la vio.

"Visto" no es un campo, es derivado: `juicio !== null`. Cuando hay juicio,
**manda el juicio** y el interés previo deja de contar para esa peli. Si
intuiste +3 y tras verla pusiste −4, el +3 ya no significa nada.

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
- `/recommendations` sobre tus semillas (lo que calificaste alto).
- `/discover` con la keyword LGBT+ `158718`, presencia garantizada. Sin marcar,
  sin sección aparte: mezclada.

Si el pool queda corto: catálogo local importado; después `SEMILLA_FALLBACK`.
Sin API key pero con catálogo importado, Vera propone igual desde lo local.

**Enriquecimiento perezoso.** Los listados de TMDb ya traen título, sinopsis,
póster, `vote_average`, `vote_count` y `genre_ids` — todo lo que el motor
necesita para rankear. `/detail` se pide solo para las pelis que vas a mirar:
las cartas de calibración antes de mostrarlas, y el resto del pool en segundo
plano mientras calificas. Antes eran ~60 `/detail` antes de pintar la primera
carta.

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
| `CARTAS_POR_MADUREZ` | cuántas cartas pedir según cuánto te conoce Vera |
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
  historial.ts       vistas, semillas y perfil histórico
  ratings.ts         reacciones en localStorage
  probes.ts          semilla de respaldo hand-picked
  voz.ts             copy según personalidad
  import.ts          importador TMDb (usado por /vera/catalog)
src/routes/vera/
  +page.svelte           orquestador del flujo
  catalog/+page.svelte   admin de importación TMDb
  README.md              este archivo
```
