# Vera — Wizard de recomendaciones

**Versión 3.0 · Arquitectura sin IA, con compañía, edades y exclusiones de género**

---

## Tabla de contenidos

1. [Cambios respecto a la v2](#cambios-respecto-a-la-v2)
2. [Arquitectura sin IA](#arquitectura-sin-ia)
3. [El catálogo enriquecido](#el-catálogo-enriquecido)
4. [Cómo funciona una recomendación, paso a paso](#cómo-funciona-una-recomendación-paso-a-paso)
5. [Limitaciones honestas de no usar IA](#limitaciones-honestas-de-no-usar-ia)
6. [Setup inicial actualizado](#setup-inicial-actualizado)
7. [Compañía: con quién está el usuario](#compañía-con-quién-está-el-usuario)
8. [Edades de los presentes](#edades-de-los-presentes)
9. [Géneros y temas excluidos](#géneros-y-temas-excluidos)
10. [Personalidad ajustable y perfil de profundidad](#personalidad-ajustable-y-perfil-de-profundidad)
11. [Flujo del wizard — Capa 1: Intención](#capa-1-intención)
12. [Flujo del wizard — Capa 2: Forma](#capa-2-forma)
13. [Flujo del wizard — Capa 3: Filtros](#capa-3-filtros)
14. [Presentación de resultados](#presentación-de-resultados)
15. [Recomendaciones guardadas](#recomendaciones-guardadas)
16. [Modo grupo](#modo-grupo)
17. [Feedback post-visionado](#feedback-post-visionado)
18. [Lógica determinista (sustituto del aprendizaje)](#lógica-determinista-sustituto-del-aprendizaje)
19. [Capacidades adicionales](#capacidades-adicionales)
20. [Lineamientos de estética visual](#lineamientos-de-estética-visual)
21. [Glosario interno: lenguaje accesible](#glosario-interno-lenguaje-accesible)

---

## Cambios respecto a la v2

Esta versión incorpora tres cambios principales:

1. **Arquitectura sin IA.** Todo el sistema funciona con bases de datos relacionales, reglas explícitas, filtros y plantillas. Nada de modelos de lenguaje, nada de aprendizaje automático, nada de embeddings.
2. **Nuevas preguntas integradas:** compañía, edades de los presentes y géneros o temas que no quiere ver el usuario.
3. **Reformulación de capacidades** que dependían de IA, sustituidas por mecanismos deterministas equivalentes donde es posible, eliminadas honestamente donde no.

---

## Arquitectura sin IA

El sistema funciona con tres componentes técnicos clásicos:

### 1. Base de datos del catálogo

Una base relacional (PostgreSQL, MySQL o similar) con una tabla principal de títulos. Cada título tiene:

- **Metadatos objetivos:** título, año, duración, plataformas disponibles, idioma original, idiomas de doblaje, idiomas de subtítulos, director, reparto principal, país de origen, clasificación por edad oficial.
- **Géneros estándar:** drama, comedia, terror, acción, romance, ciencia ficción, etc. (lista cerrada de 15 a 20 géneros).
- **Etiquetas de uso:** un conjunto fijo de etiquetas que describen para qué sirve el título. Ejemplos: "bueno para dejar de fondo", "comedia familiar segura", "drama duro", "para llorar", "para reír sin pensar", "engancha rápido", "lenta y contemplativa", "buena visualmente", "diálogos densos".
- **Etiquetas de tono:** tranquilo / tenso / melancólico / luminoso / oscuro / esperanzador / nostálgico, etc.
- **Temas sensibles presentes:** violencia gráfica, escenas sexuales explícitas, abuso, suicidio, sufrimiento de animales, enfermedad terminal, etc. (lista cerrada).
- **Apto para edades:** edad mínima recomendada por el sistema (puede diferir de la oficial).

Toda esta información se introduce manualmente o con procesos semiautomáticos basados en datos públicos (TMDB, JustWatch, Common Sense Media para clasificación familiar).

### 2. Tabla de respuestas del usuario

Cuando el usuario responde el wizard, sus respuestas se guardan como un conjunto estructurado: estado de ánimo elegido, formato preferido, tiempo disponible, compañía, edad mínima del grupo, géneros excluidos permanentes, géneros excluidos en esta sesión, etc.

### 3. Motor de coincidencia

Una función que, dadas las respuestas del usuario, hace dos cosas:

**Filtros duros (excluyentes):** descarta todo lo que el usuario no puede ver. Plataformas a las que no tiene acceso, géneros excluidos permanentemente, edad inadecuada para el grupo, temas sensibles que pidió evitar, idioma no compatible. Esto se hace con consultas SQL clásicas (WHERE).

**Puntuación por coincidencia:** sobre los títulos que quedaron, se calcula un puntaje según cuántas etiquetas coinciden con las respuestas del usuario. Cada respuesta del usuario activa ciertas etiquetas con un peso. Por ejemplo, "nostálgico sin hundirme" activa: tono nostálgico (+3), tono luminoso (+2), tono oscuro (-3), drama duro (-2). El sistema suma los puntos por título y ordena.

Los títulos con mayor puntaje son los candidatos. Las tres opciones finales (segura, distinta, sorpresa) se eligen aplicando reglas adicionales sobre esos candidatos:

- **Segura:** el de mayor puntaje absoluto.
- **Distinta:** entre los 10 mejores, el que tenga un género distinto al de la segura.
- **Sorpresa:** entre los 30 mejores, el que el usuario menos probable habría elegido (uno menos popular, o de un país que no suele consumir, o de un género que no marcó como favorito).

Todo esto es matemática simple y reglas claras. No hay aprendizaje, no hay inferencia, no hay generación.

---

## El catálogo enriquecido

El éxito del sistema depende de la calidad del etiquetado. Esto es trabajo intensivo pero hecho una sola vez por título.

### Estrategia de etiquetado por etapas

**Etapa 1 — Datos públicos automáticos:** se importan de TMDB y JustWatch los metadatos básicos (año, duración, géneros, plataformas, idiomas, reparto). Esto cubre cientos de miles de títulos sin trabajo manual.

**Etapa 2 — Etiquetas de tono y uso, manuales:** un equipo (al principio, los fundadores o curadores; después, eventualmente, voluntarios o trabajadores remotos) revisa los títulos más relevantes y agrega las etiquetas semánticas ricas. Se prioriza por popularidad y disponibilidad en las plataformas que cubre el producto.

**Etapa 3 — Temas sensibles, manuales o de fuentes específicas:** se puede usar Common Sense Media o DoesTheDogDie.com (sitio especializado en avisar sobre temas difíciles) como fuente, o etiquetar manualmente.

**Etapa 4 — Mantenimiento continuo:** cada vez que entra un título nuevo al catálogo, pasa por un proceso de etiquetado antes de aparecer en recomendaciones.

### Cantidad realista de etiquetas por título

Cada título termina con entre 15 y 30 etiquetas en total. Suena mucho pero la mayoría son binarias (sí/no) y se aplican rápido cuando se conoce el formato.

### Tamaño inicial razonable

No hace falta tener etiquetados 100.000 títulos para arrancar. Con 2.000 a 3.000 títulos bien etiquetados (los más populares en las plataformas que cubre el producto en el país objetivo) el sistema funciona y da buenas recomendaciones. Se va expandiendo con el tiempo.

---

## Cómo funciona una recomendación, paso a paso

Para que quede concreto, así fluye una recomendación de punta a punta sin IA en ningún momento:

**Input del usuario:** "Algo acorde a cómo me siento → nostálgico → serie → toda la noche → con mi pareja → solo adultos."

**Paso 1: filtros duros (SQL).**
```
SELECT * FROM titulos
WHERE plataformas IN (Netflix, Mubi, Prime)
AND formato = 'serie'
AND clasificacion_edad <= 'apto_adultos'
AND NOT contiene_tema_excluido(usuario_id)
AND NOT en_genero_excluido(usuario_id)
AND idioma_compatible(usuario_id)
```
Resultado: 847 series compatibles.

**Paso 2: puntuación por coincidencia.** Para cada serie, se calcula un puntaje:
- Tiene etiqueta "nostálgico" → +3
- Tiene etiqueta "luminoso" → +2 (porque dijo "sin hundirme")
- Tiene etiqueta "duración apropiada para una noche" → +1
- Tiene etiqueta "bueno para ver en pareja" → +2
- Tiene etiqueta "oscura y deprimente" → -3
- Tiene etiqueta "drama duro" → -2

Resultado: las 847 series quedan ordenadas por puntaje.

**Paso 3: selección de las tres opciones.**
- Segura: la de mayor puntaje (digamos *Normal People*).
- Distinta: entre las 10 mejores, una con género distinto pero puntaje alto (digamos *The Marvelous Mrs. Maisel*, que toca melancolía desde la comedia).
- Sorpresa: entre las 30 mejores, una menos esperable (digamos *Halt and Catch Fire*, drama tecnológico con tono nostálgico).

**Paso 4: generación de la frase explicativa con plantilla.**

Cada serie tiene su frase armada con plantillas. Por ejemplo, la plantilla para una recomendación "segura" es:

> "Encaja con [estado_de_animo] sin caer en [lo_que_el_usuario_quiso_evitar]. [Etiqueta_destacada_1] y [Etiqueta_destacada_2]."

Rellenada con datos de *Normal People*:

> "Encaja con la nostalgia sin caer en la tristeza pesada. Romance con melancolía luminosa y final no devastador."

Cada título tiene 3 o 4 frases-plantilla pre-armadas para distintos contextos (cuando es la "segura", cuando es la "distinta", cuando es para "ver de fondo", etc.). El sistema elige la plantilla según el contexto.

**Resultado entregado al usuario:** las tres opciones con sus frases, sin haber pasado por ningún modelo de lenguaje.

---

## Limitaciones honestas de no usar IA

Conviene marcarlas claras para que se sepa qué se está sacrificando:

### Lo que no se puede hacer sin IA

**Conversación libre interpretada.** El usuario no puede escribir "vi Aftersun y me dejó así, quiero algo parecido pero menos triste". El sistema no sabe interpretar texto libre. Se reemplaza por preguntas estructuradas: el usuario tiene que pasar por el wizard de capas, no hay atajo conversacional.

**Modo voz conversacional.** Sin IA, el modo voz se limita a reconocer comandos o palabras clave dentro de un menú (por ejemplo, "decir el número de la opción"). No puede entender una respuesta libre como "estoy bajón pero no quiero hundirme". Se mantiene como modo, pero el usuario habla sobre opciones predefinidas, no libremente.

**Frases de recomendación únicas y personalizadas.** Las explicaciones son combinaciones de plantillas con datos reales. Son correctas y útiles, pero menos vivas que las que generaría una IA. Para mitigar esto, conviene escribir muchas plantillas variadas (10 o 20 por contexto) y elegir aleatoriamente para que no se sientan repetitivas.

**Insights de patrones expresivos.** El sistema sí puede detectar patrones estadísticos ("aceptaste muchas películas europeas este mes") pero no puede armar la frase poética que conecta tres patrones en una observación elegante. Las observaciones son más mecánicas, tipo "el 70% de lo que aceptaste este mes fue serie".

**Búsqueda libre tipo "algo parecido a X".** Solo funciona si X está en el catálogo y tiene títulos asociados como "parecidos" en una tabla de relaciones manual. Se puede hacer, pero es trabajo de curación. No hay descubrimiento automático de similitudes.

### Lo que sí se hace perfecto sin IA

Todo el resto del producto descrito en este documento. Y notablemente mejor en algunos aspectos: más rápido, más barato, más privado, más predecible.

---

## Setup inicial actualizado

Ahora son siete preguntas (antes cinco), porque se agregaron las nuevas. Sigue siendo rápido.

### Pantalla 1 — Bienvenida

> Hola. Soy Vera.
> Te ayudo a elegir qué ver cuando no sabes qué quieres ver.
> Siete preguntas rápidas y empezamos.

### Pantalla 2 — Modo de interacción

> ¿Cómo prefieres usarme?
> - Tocando y leyendo
> - Hablando y escuchando (limitado a comandos)
> - Las dos, según el momento

### Pantalla 3 — Perfil de profundidad

> ¿Qué lugar ocupan las películas y series en tu vida?
> - Es mi entretenimiento, nada más
> - Me interesa, pero sin pretensiones
> - Soy exigente, me importa la calidad
> - Soy cinéfilo, dame opciones raras

### Pantalla 4 — Idiomas que entiendes de oído

> ¿En qué idiomas entiendes el audio sin necesidad de subtítulos? (Marca todos los que apliquen)
> - Español · Inglés · Portugués · Francés · Italiano · Otro

### Pantalla 5 — Doblaje vs subtítulos

> Cuando algo está en un idioma que no entiendes, ¿qué prefieres?
> - Subtítulos siempre · Doblaje siempre · Depende · Me da igual

### Pantalla 6 — Plataformas disponibles

> ¿A qué plataformas tienes acceso?
> (Selección múltiple)

### Pantalla 7 — Géneros y temas que nunca quieres ver (NUEVO)

> ¿Hay algo que nunca quieras ver, bajo ninguna circunstancia?
>
> **Géneros que excluyo siempre:**
> - Terror · Romance · Acción violenta · Bélico · Político · Religioso · Deportes · Realities · Otro
>
> **Temas que prefiero evitar:**
> - Violencia hacia animales · Suicidio · Abuso · Enfermedad terminal · Embarazo o pérdida · Violencia explícita · Contenido sexual explícito · Drogas
>
> Puedes saltar esta pregunta si nada te molesta especialmente.

Esta pantalla es importante: lo que se marca acá actúa como filtro duro permanente. Vera nunca recomendará nada que contenga esos géneros o temas, salvo que el usuario los desactive desde Ajustes.

---

## Compañía: con quién está el usuario

La pregunta de compañía deja de estar solo en el camino "liviano" y pasa a ser **una pregunta común de la Capa 2** que aparece en casi todos los caminos.

### Pregunta

> ¿Con quién vas a ver esto?
> - Solo
> - En pareja
> - Con amigos
> - Con familia
> - Otro grupo

### Cuándo aparece

- En caminos A (acorde a cómo me siento), B (sacarme del estado), D (algo que me marque), E (algo liviano): siempre.
- En camino C (para dejar de fondo): solo si es relevante (por defecto se asume "solo").
- En caminos F (sorpréndeme) y G (decide por mí): se asume "solo" o se infiere del último uso.

### Cómo modifica las recomendaciones

Cada opción de compañía activa pesos en las etiquetas del catálogo:

- **Solo:** sin filtros adicionales.
- **En pareja:** suma puntos a etiquetas "buena para ver en pareja", "no incómoda en compañía"; resta a "muy explícita sexualmente" (sin filtrar del todo, solo bajar prioridad).
- **Con amigos:** suma a "entretenida y comentable", "ritmo ágil", "buena para grupo"; resta a "drama denso lento".
- **Con familia:** activa la pregunta siguiente (edades) y ajusta filtros según resultado.
- **Otro grupo:** trata como "con amigos" por defecto, con opción de afinar.

---

## Edades de los presentes

Aparece solo si en la pregunta de compañía se eligió "Con familia" u "Otro grupo".

### Pregunta

> ¿Cuál es la edad más restrictiva del grupo?
> - Solo adultos (18+)
> - Adolescentes (13 a 17)
> - Niños (7 a 12)
> - Niños pequeños (menores de 7)

### Pregunta adicional opcional

> ¿Los niños van a estar viéndolo activamente o solo cerca?
> - Viendo activamente — filtrar fuerte
> - Solo cerca, no necesariamente atentos — filtrar moderado

Esta distinción es importante: una persona que pone algo mientras su hijo juega cerca no necesita los mismos filtros que si el niño está sentado al lado mirando.

### Cómo modifica los filtros

| Opción | Filtro aplicado |
|---|---|
| Solo adultos | Sin restricción adicional |
| Adolescentes + activos | Bloquea contenido adulto duro (sexo explícito, violencia gráfica) |
| Adolescentes + solo cerca | Bloquea solo lo más explícito |
| Niños + activos | Solo apto para menores (familiar, animación, infantil) |
| Niños + solo cerca | Bloquea adulto fuerte, permite drama o suspenso moderado |
| Niños pequeños + activos | Solo apto para preescolar/infantil |
| Niños pequeños + solo cerca | Bloquea cualquier contenido adulto, incluyendo lenguaje fuerte |

El sistema usa la columna "clasificación por edad" del catálogo y los temas sensibles etiquetados para aplicar estos filtros como SQL puro.

---

## Géneros y temas excluidos

Tres niveles de exclusión, cada uno con su propósito:

### Nivel 1: Exclusión permanente (setup inicial)

Se configura una vez en el setup (Pantalla 7) y se aplica siempre. Ejemplo: una persona que nunca ve terror lo marca y se acabó. Solo se cambia desde Ajustes.

### Nivel 2: Exclusión por compañía (automática)

Sin que el usuario haga nada, el sistema ajusta según con quién está. Por ejemplo, si la pareja del usuario tiene su propio perfil y marcó "nunca terror", al activar "en pareja" se aplican también esos filtros.

### Nivel 3: Exclusión de sesión (Capa 3)

Filtros que aplican solo en esta sesión específica. Aparece en la Capa 3 (filtros opcionales). Ejemplo: "hoy no tengo ganas de ver nada con muerte de mascotas" o "hoy nada bélico".

### Lista cerrada de géneros

Para que funcione el etiquetado, los géneros son una lista finita y estandarizada. Propuesta:

- Drama · Comedia · Romance · Acción · Aventura · Ciencia ficción · Fantasía · Terror · Suspenso · Thriller · Misterio · Crimen · Bélico · Histórico · Biográfico · Musical · Western · Documental · Animación · Familiar · Deportes · Reality · Realismo mágico · Antología

### Lista cerrada de temas sensibles

Etiquetas que el usuario puede pedir evitar:

- Violencia gráfica · Tortura · Contenido sexual explícito · Desnudez · Abuso sexual · Abuso infantil · Violencia doméstica · Violencia hacia animales · Muerte de mascotas · Suicidio · Autolesión · Trastornos alimentarios · Enfermedad terminal · Muerte de niños · Embarazo o pérdida · Aborto · Drogas · Adicciones · Lenguaje vulgar fuerte · Bullying · Discriminación racial · Discriminación homofóbica · Religión como tema central · Política partidaria

Cada título del catálogo tiene marcadas (sí/no) cuáles de estas etiquetas le aplican. Filtrar es directo.

---

## Personalidad ajustable y perfil de profundidad

Sin cambios respecto a la v2. Se mantienen los tres modos de personalidad (directo, cálido, cinéfilo) y los cuatro niveles de profundidad. Con la diferencia de que ahora todo se implementa con plantillas múltiples, no con generación de IA.

Cada personalidad tiene su propio set de plantillas para:

- Preguntas del wizard.
- Frases de explicación en recomendaciones.
- Confirmaciones y respuestas del sistema.

Por ejemplo, la confirmación después de elegir intención varía:

- **Directo:** "Listo. Siguiente pregunta."
- **Cálido:** "Perfecto. Vamos paso a paso."
- **Cinéfilo:** "Entendido. Te llevo por ese camino."

Es trabajo de redacción inicial. Una vez escritas las plantillas, el sistema solo elige cuál mostrar según el modo activo.

---

## Capa 1: Intención

Sin cambios funcionales respecto a la v2. Se mantienen las siete opciones (acorde a cómo me siento, sacarme del estado, dejar de fondo, algo que me marque, liviano, sorpréndeme, decide por mí) más la opción extra del modo cinéfilo (búsqueda dirigida por director/actor/parecido a X).

La búsqueda dirigida del modo cinéfilo, sin IA, funciona así: en el catálogo hay una tabla de relaciones manuales entre títulos ("películas relacionadas con") y una tabla de filmografía por director y actor. Cuando el usuario escribe un nombre o título, el sistema busca coincidencia exacta o parcial en esas tablas y devuelve resultados. No hay interpretación libre.

---

## Capa 2: Forma

Las preguntas son las mismas que en la v2 pero ahora con dos preguntas comunes integradas:

### Estructura general de la Capa 2

| Pregunta | Cuándo aparece |
|---|---|
| Estado emocional / desplazamiento / qué estás haciendo | Específica de cada camino |
| Formato (película / serie / corto / lo que sea) | Siempre, salvo que se infiera |
| Tiempo disponible | Siempre, salvo que se infiera |
| **Compañía (con quién)** | Caminos A, B, D, E siempre; C opcional |
| **Edades (si hay familia o grupo)** | Solo si compañía = familia o grupo |
| Tono o tipo de obra | Caminos D y E |

Algunas preguntas se saltean automáticamente:

- Si el tiempo disponible es "tengo 90 minutos exactos", el formato se asume "película".
- Si el tiempo disponible es "quiero empezar algo para semanas", se asume "serie".
- Si la última sesión fue hace menos de 2 horas, se recuerda la compañía de antes y se ofrece confirmarla en un solo toque ("¿Sigues con la misma gente?").

---

## Capa 3: Filtros

Plegada por defecto. Misma lógica que en v2, con dos cambios:

### Cambio 1: separación visual de exclusiones

Dentro de los filtros opcionales hay una sección especial llamada **"Excluir solo en esta sesión"** que muestra:

- Géneros excluidos hoy (si quieres sacar algo más allá de lo permanente)
- Temas sensibles a evitar hoy

### Cambio 2: filtros sin IA

Se eliminan filtros que requerían interpretación semántica como "tono visual onírico". Quedan filtros objetivos y verificables contra etiquetas del catálogo:

**Modo general:**
- Plataformas activas
- Año mínimo
- Duración (para series): corta / mediana / larga
- Excluir ya vistos
- Exclusiones de sesión (géneros + temas)

**Modo cinéfilo (todo lo anterior más):**
- Origen geográfico (Hollywood / Europa / Asia / Latinoamérica / Independiente)
- Época (último año / últimos 5 / desde 2000 / clásico moderno / clásico)
- Estructura narrativa (autoconclusiva / serializada / antológica)
- Tipo (ficción / documental / animación / híbrido)

---

## Presentación de resultados

Misma estructura que v2: tres opciones (segura, distinta, sorpresa) con sus frases.

### Diferencia clave sin IA

Las frases de explicación no se generan, se eligen. Cada título tiene en su ficha del catálogo entre 5 y 10 frases-plantilla pre-escritas para distintos contextos. El sistema elige la plantilla más apropiada según:

- Posición de la recomendación (segura / distinta / sorpresa)
- Personalidad del usuario (directo / cálido / cinéfilo)
- Camino del wizard que se siguió

Ejemplo de fichas de plantillas para *Normal People*:

| Contexto | Plantilla |
|---|---|
| Segura + cálido | "Encaja con tu nostalgia sin caer en tristeza pesada. Romance con melancolía luminosa." |
| Segura + directo | "Romance melancólico, 12 capítulos de 30 min, en BBC. Te va a llegar." |
| Segura + cinéfilo | "Adaptación de Sally Rooney por Lenny Abrahamson. Estética observacional, ritmo lento, gran ejercicio actoral." |
| Para dejar de fondo | "Diálogos importantes — esta es para verla atenta, no como ruido de fondo." (la descarta del contexto fondo) |

El trabajo de redacción de plantillas es importante pero una sola vez por título. Para empezar, alcanza con 2 o 3 plantillas y se va expandiendo.

### Modo "Elige tú por mí"

Una sola opción. La del puntaje más alto en la sesión, sin alternativas visibles. Botón "no, otra" para rechazar y pasar a la segunda en puntaje.

---

## Recomendaciones guardadas

Sin cambios respecto a la v2. Toda esta funcionalidad es lógica de base de datos pura, no necesitaba IA.

- Cada guardada conserva el contexto: la frase exacta con la que se sugirió (que es una plantilla ya rellenada).
- Tres vistas: por fecha, por estado, por contexto actual.
- Acciones: ver, marcar como vista, abandonar, quitar, recordar contexto.
- Caducidad opcional a los seis meses para pendientes nunca empezadas.

### Integración con el flujo

Cuando el usuario empieza un wizard, antes de la Capa 2, el sistema revisa sus guardadas. Si alguna tiene contexto que coincide con la intención de hoy (mediante coincidencia de etiquetas en su contexto guardado), Vera la ofrece primero:

> "Tienes 2 guardadas que aceptaste para este tipo de momento. ¿Las miramos antes?"

---

## Modo grupo

Funciona igual que en v2 pero con una mecánica determinista clara:

1. Cada miembro del grupo responde el wizard.
2. El sistema arma una **intersección de respuestas**: solo títulos compatibles con todos.
3. Para el ranking, suma los puntajes individuales: un título que es 8/10 para uno y 7/10 para otro tiene puntaje grupal 15.
4. Se descartan títulos que tengan puntaje muy bajo para alguno (si una persona lo puntúa 2/10, no se recomienda aunque la otra lo puntúe 10/10).

Resolución de conflictos sin IA:

- Si no hay intersección posible, el sistema dice claro: "no encontré nada compatible con todos."
- Ofrece tres caminos predefinidos: votar entre las mejores opciones imperfectas (cada miembro vota), que alguien ceda un género o tema, o dividir en dos sesiones.

---

## Feedback post-visionado

Misma idea que v2. Sin IA, los emojis se traducen en una puntuación numérica (1 a 5) y se guardan junto al título.

### Lo que se hace con esa puntuación

- Si el usuario puntúa una recomendación 5/5, esos atributos del título (etiquetas, género, director, plataforma) suben de peso en futuras recomendaciones.
- Si puntúa 1/5, esos atributos bajan de peso.
- Es ajuste de pesos por reglas, no aprendizaje automático.

### Preguntas adicionales opcionales

Mismo formato que v2:
- ¿La terminaste?
- ¿La recomendarías a alguien en tu mismo estado de ese día?
- ¿Qué no funcionó? (lista cerrada de opciones, no campo libre: muy lenta, no me enganchó, esperaba otra cosa, demasiado intensa, demasiado liviana)

La última pregunta es importante: las opciones son cerradas (no texto libre) para que el sistema pueda usarlas. Cada respuesta resta peso a una etiqueta específica.

---

## Lógica determinista (sustituto del aprendizaje)

Sin IA, no hay aprendizaje automático. Pero sí hay **ajuste de pesos por reglas explícitas** que produce un efecto parecido en el día a día.

### Sistema de pesos

Cada usuario tiene una tabla de pesos personalizada que parte de valores neutros (todos en 1.0) y se ajusta con cada interacción:

- Acepta una recomendación → +0.2 a sus etiquetas.
- Marca como vista y puntúa alto → +0.5 adicional.
- Rechaza → -0.3 a las etiquetas.
- Abandona → -0.5 a las etiquetas.
- Marca un tema sensible como "no quiero más de esto" → -2.0 (casi excluyente).

Estos números son ejemplos: se calibran probando.

### Cómo se usa

En el momento de calcular puntajes en una nueva recomendación, los pesos personalizados se multiplican por los pesos base de cada etiqueta. Si la etiqueta "ambientación europea" tiene peso base 2 para una respuesta determinada, y el usuario tiene peso personalizado 1.5 para esa etiqueta (porque aceptó varias), el peso efectivo es 3.

### Patrones detectables sin IA

Estadística simple:

- "El 70% de lo que aceptaste este mes fue serie." (Conteo.)
- "Aceptaste 5 películas con protagonistas femeninas, ninguna con masculinos." (Conteo por etiqueta.)
- "Rechazaste sistemáticamente todo lo anterior a los 2000." (Conteo por rango de año.)
- "Las películas que más te gustaron este mes son europeas." (Conteo por origen.)

Las observaciones que se muestran al usuario son frases-plantilla rellenadas con estos conteos. No son tan elegantes como las que generaría una IA, pero son claras y útiles.

---

## Capacidades adicionales

Recorrido por las capacidades del documento v2 marcando cuáles funcionan sin IA, cuáles cambian y cuáles se descartan:

| Capacidad | Sin IA |
|---|---|
| Disponibilidad en tiempo real | ✓ Sí, vía APIs externas (JustWatch). |
| Alertas de salida de catálogo | ✓ Sí, comparando fechas. |
| Modo "tengo X horas" | ✓ Sí, matemática simple. |
| Modo "cuándo" (día/temporada) | ✓ Sí, reglas por fecha del año. |
| Modo "ocasión especial" | ✓ Sí, con listas curadas para cada ocasión. |
| Continuidad inteligente (retomar serie) | ✓ Sí, base de datos de progreso. |
| Aviso de contenido sensible | ✓ Sí, etiquetas explícitas. |
| Accesibilidad (audiodescripción) | ✓ Sí, metadato del catálogo. |
| Compartir recomendación con contexto | ✓ Sí, texto plano. |
| Diario de visualizaciones | ✓ Sí, base de datos. |
| Rabbit hole guiado | ✓ Limitado: solo si las relaciones están curadas manualmente. |
| Conversación libre | ✗ No funciona sin IA. Reemplazada por wizard estructurado. |
| Modo voz conversacional | ✗ Limitado a comandos sobre menús. |
| "Algo parecido a X" libre | ✗ Solo si X está en catálogo con relaciones curadas. |
| Insights expresivos | ⚠ Funcionan pero más mecánicos (basados en conteo, no en interpretación). |

---

## Lineamientos de estética visual

Sin cambios respecto a la v2:

- Tema claro por defecto, tema oscuro opcional.
- Una sola fuente principal legible.
- Paleta cálida pero universal.
- Iconos simples lineales.
- Mucho espacio en blanco.
- Animaciones discretas.
- Botones grandes, lenguaje activo y simple.

---

## Glosario interno: lenguaje accesible

Sin cambios respecto a la v2. Tabla de reemplazos para evitar jerga cinéfila por defecto, accesible solo si el usuario está en modo cinéfilo:

| Modo cinéfilo | Modo general |
|---|---|
| Cine de autor | Película más artística |
| Autoconclusiva | Capítulos que se pueden ver sueltos |
| Serializada | Hay que verla en orden |
| Antológica | Cada temporada es una historia distinta |
| Slow burn | Avanza lento pero atrapa |
| Drama denso | Drama fuerte |
| No ficción ensayística | Documental que te hace pensar |
| Filmografía | Películas de tal director |
| Festival / arthouse | Película premiada en festivales |

---

## Resumen del rediseño v3

| Aspecto | Versión 2.0 | Versión 3.0 |
|---|---|---|
| Tecnología | IA generativa (RAG con LLM) | Cero IA. Base de datos relacional + reglas |
| Compañía | Solo en camino "liviano" | Pregunta común en casi todos los caminos |
| Edades | No contemplado | Pregunta dedicada cuando hay familia o grupo |
| Géneros excluidos | Solo en filtros de sesión | Tres niveles (permanente / por compañía / sesión) |
| Temas sensibles | Mencionado vagamente | Lista cerrada explícita y filtrable |
| Conversación libre | Sí | No (reemplazada por wizard estructurado) |
| Modo voz | Conversacional libre | Solo comandos sobre menús |
| Frases de explicación | Generadas | Plantillas pre-escritas elegidas |
| Aprendizaje | Implícito por embeddings | Ajuste de pesos por reglas explícitas |
| Costo operativo | Alto (LLM por consulta) | Bajo (solo base de datos) |
| Privacidad | Datos pasan por API externa | Todo local o en infraestructura propia |
| Velocidad de respuesta | 1-4 segundos | Milisegundos |

---

**Vera v3.0** — Documento de diseño completo
*Wizard de recomendaciones, sin IA, con compañía y exclusiones integradas*
