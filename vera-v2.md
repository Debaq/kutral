# Vera — Wizard de recomendaciones

**Versión 2.0 · Rediseño para público general con modo cinéfilo equivalente**

---

## Tabla de contenidos

1. [Cambio de enfoque](#cambio-de-enfoque)
2. [Personalidad ajustable](#personalidad-ajustable)
3. [Perfil de profundidad](#perfil-de-profundidad)
4. [Setup inicial actualizado](#setup-inicial-actualizado)
5. [Flujo del wizard — Capa 1: Intención](#capa-1-intención)
6. [Flujo del wizard — Capa 2: Forma](#capa-2-forma)
7. [Flujo del wizard — Capa 3: Filtros](#capa-3-filtros)
8. [Presentación de resultados](#presentación-de-resultados)
9. [Recomendaciones guardadas](#recomendaciones-guardadas)
10. [Conversación libre](#conversación-libre)
11. [Modo grupo](#modo-grupo)
12. [Feedback post-visionado](#feedback-post-visionado)
13. [Inteligencia silenciosa](#inteligencia-silenciosa)
14. [Capacidades adicionales](#capacidades-adicionales)
15. [Lineamientos de estética visual](#lineamientos-de-estética-visual)
16. [Glosario interno: lenguaje accesible](#glosario-interno-lenguaje-accesible)

---

## Cambio de enfoque

La versión 1.0 quedó orientada implícitamente a un perfil cinéfilo: la estética era de revista de cine, el vocabulario asumía cierto bagaje (cine de autor, antológico, tono visual), y los ejemplos eran títulos que solo reconoce gente muy metida en el tema.

Esta versión 2.0 parte de la premisa contraria: **el wizard debe servir tanto a alguien que quiere "algo entretenido para esta noche" como a alguien que busca una película de un director específico**, sin que ninguno de los dos se sienta fuera de lugar.

La solución no es bajar el techo, sino **ofrecer dos caminos equivalentes** que conviven en el mismo producto:

- **Modo general** (default para todos): lenguaje cotidiano, ejemplos masivos, estética cálida y amigable, recomendaciones que privilegian accesibilidad.
- **Modo cinéfilo** (opt-in, no escondido): lenguaje más técnico, ejemplos más diversos, filtros avanzados disponibles, recomendaciones que se animan a propuestas más exigentes.

Ambos modos comparten la misma estructura de capas y las mismas capacidades. Lo que cambia es el vocabulario, los ejemplos y la profundidad de las opciones.

---

## Personalidad ajustable

Aparte de la profundidad del perfil, el wizard tiene tres personalidades de comunicación que el usuario elige:

### Modo directo

Ideal para quien quiere eficiencia y no decoración.

> "Te recomiendo *Schitt's Creek*. Comedia, 6 temporadas de 20 minutos, Netflix. Te va a hacer reír sin pedirte nada."

### Modo cálido (default)

Conversacional, cercano, sin solemnidad. Funciona como un amigo que sabe del tema pero no te lo restriega.

> "Tengo algo que creo que te va a gustar para esta noche. Se llama *Schitt's Creek*, es una comedia muy querida, está en Netflix y los capítulos son cortos. Buena opción para reírte un rato sin complicaciones."

### Modo cinéfilo

Más técnico, más expresivo, asume conocimiento previo. Solo aparece si el usuario lo elige.

> "Te propongo *Schitt's Creek* — sitcom canadiense de Eugene y Dan Levy, con una construcción de personajes notable. Slow burn cómico, evoluciona mucho desde la primera temporada. En Netflix."

La personalidad se cambia en cualquier momento desde Ajustes. Aplica al tono de las preguntas, las recomendaciones y la conversación libre.

---

## Perfil de profundidad

En el setup inicial se hace una pregunta breve que determina qué tan exigentes serán las recomendaciones por defecto. No define al usuario para siempre: se puede cambiar y el sistema aprende.

**Pregunta:** ¿Qué lugar ocupan las películas y series en tu vida?

| Opción | Recomendaciones por defecto | Filtros avanzados visibles |
|---|---|---|
| Es mi entretenimiento, nada más | Mainstream, taquilleras, populares | No |
| Me interesa, pero sin pretensiones | Mix de masivo y curado, sin extremos | Algunos |
| Soy bastante exigente, me importa la calidad | Más cine de autor accesible, series premiadas | Sí |
| Soy cinéfilo, dame opciones raras | Cine independiente, internacional, festivales | Todos |

Cada nivel también modifica qué es "la opción rara" en las recomendaciones de resultado:

- Nivel 1: la rara es algo de otro género dentro del mainstream.
- Nivel 2: la rara puede ser una película independiente o de otro país.
- Nivel 3: la rara puede ser cine de autor reconocido.
- Nivel 4: la rara puede ser experimental, de festival, de directores poco conocidos.

---

## Setup inicial actualizado

Cinco preguntas, una sola vez. Diseñadas para ser rápidas y no intimidar.

### Pantalla 1 — Bienvenida

> Hola. Soy Vera.
> Te ayudo a elegir qué ver cuando no sabes qué quieres ver.
> Cinco preguntas rápidas y empezamos.

Botones: **Empezar** / **Saltar y configurar después**

### Pantalla 2 — Modo de interacción

> ¿Cómo prefieres usarme?
> - Tocando y leyendo
> - Hablando y escuchando
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

Nota debajo: *Esto define qué cosas te sirven para "ver de fondo" mientras haces otra cosa.*

### Pantalla 5 — Doblaje vs subtítulos

> Cuando algo está en un idioma que no entiendes, ¿qué prefieres?
> - Subtítulos siempre
> - Doblaje siempre
> - Depende del tipo de contenido
> - Me da igual

### Pantalla 6 — Plataformas disponibles

> ¿A qué plataformas tienes acceso?
> (Selección múltiple: Netflix, Prime Video, Disney+, HBO Max, Apple TV+, Paramount+, Mubi, Filmin, otras)

---

## Capa 1: Intención

Una sola pregunta. Misma estructura para todos los perfiles, con vocabulario adaptado.

### Versión modo general

> ¿Qué tipo de experiencia buscas hoy?
>
> - **Algo acorde a cómo me siento** — Que acompañe tu ánimo actual.
> - **Algo que me saque de cómo me siento** — Para cambiar de ánimo.
> - **Algo para dejar de fondo** — Mientras cocinas, trabajas, te duermes.
> - **Algo que me deje pensando** — Cuando tienes ganas de algo profundo.
> - **Algo liviano para pasarla bien** — Sin complicaciones.
> - **Sorpréndeme** — Sin más preguntas, tres opciones.
> - **Elige tú por mí** — Sin preguntas, una sola opción.

### Versión modo cinéfilo

Misma estructura, pero la cuarta opción se reformula:

> **Algo que me marque** — Una experiencia cinematográfica fuerte.

Y se agrega una opción extra:

> **Algo de un director, actor o saga específicos** — Búsqueda dirigida.

---

## Capa 2: Forma

Las preguntas cambian según el camino elegido en la Capa 1. Máximo dos o tres preguntas. Lenguaje adaptado al perfil del usuario.

### Camino A: Algo acorde a cómo me siento

**Pregunta 1 — Estado:**

> ¿Cómo te sientes ahora mismo?
> - Bajoneado · Tranquilo · Nervioso · Contento · Cansado · Con nostalgia · Irritado · Vacío · Otro

(En modo voz: respuesta libre.)

**Pregunta 2 — Formato:**

> ¿Película o serie?
> - Película
> - Serie
> - Algo corto (un capítulo suelto, un cortometraje)
> - Lo que sea, mientras encaje

**Pregunta 3 — Tiempo disponible:**

> ¿Cuánto tiempo tienes?
> - Menos de una hora
> - Una película (90 a 150 minutos)
> - Toda la tarde o noche
> - Quiero empezar algo para los próximos días

### Camino B: Algo que me saque de cómo me siento

**Pregunta 1 — Desplazamiento emocional:**

> ¿Desde dónde y hacia dónde?
> - Estoy triste, quiero reírme
> - Estoy nervioso, quiero calmarme
> - Estoy aburrido, quiero engancharme
> - Estoy cansado, quiero energía
> - Otro (describir)

**Pregunta 2 y 3:** mismas que Camino A (formato y tiempo).

### Camino C: Algo para dejar de fondo

**Pregunta 1 — Qué estarás haciendo:**

> ¿Qué vas a estar haciendo mientras?
> - Cocinando o tareas domésticas
> - Trabajando
> - Comiendo
> - Por dormirme

**Pregunta 2 — Nuevo o conocido:**

> ¿Algo nuevo o algo que ya conoces?
> - Algo nuevo
> - Algo que ya vi y me gusta
> - Me da igual

**Pregunta 3 — Formato:**

> ¿Cómo lo prefieres?
> - Una película
> - Capítulos sueltos de una sitcom
> - Una serie para ir avanzando
> - Me da igual

### Camino D: Algo que me deje pensando / Algo que me marque

**Pregunta 1 — Energía emocional:**

> ¿Cuánta energía emocional tienes ahora?
> - Mucha — quiero algo intenso
> - Media — algo profundo pero sin agotarme
> - Poca — algo que me llegue, pero suave

**Pregunta 2 — Tipo de obra:**

Versión modo general:

> ¿Qué tipo de historia buscas?
> - Una historia humana fuerte
> - Algo visualmente impactante
> - Un documental o historia real
> - Una película famosa que tenía pendiente
> - Sin preferencia

Versión modo cinéfilo:

> ¿Qué tipo de obra estás buscando?
> - Drama humano denso
> - Cine visualmente experimental o autoral
> - No ficción ensayística
> - Una obra canónica que tenía pendiente
> - Sin preferencia, sorpréndeme

**Pregunta 3 — Formato:** mismo que Camino A.

### Camino E: Algo liviano para pasarla bien

**Pregunta 1 — Tono:**

> ¿Qué te apetece?
> - Comedia
> - Aventura o acción
> - Romance
> - Algo relajante, no importa el género

**Pregunta 2 — Compañía:**

> ¿Con quién lo vas a ver?
> - Solo
> - En pareja
> - Con amigos
> - Con familia (incluye niños)

**Pregunta 3 — Formato:** mismo que Camino A.

### Camino F: Sorpréndeme

Sin preguntas. Resultado directo con tres opciones.

### Camino G: Elige tú por mí

Sin preguntas. Una sola opción al final, sin alternativas visibles.

### Camino H (solo modo cinéfilo): Búsqueda dirigida

**Pregunta única:**

> ¿Qué buscas exactamente?
> - Películas de un director (campo abierto)
> - Filmografía de un actor o actriz (campo abierto)
> - Algo parecido a una película o serie que me gustó (campo abierto)
> - Lo contrario a algo que odié (campo abierto)
> - Una saga o universo específico

---

## Capa 3: Filtros

Plegada por defecto. Se abre con un botón discreto: *¿Quieres afinar más? (opcional)*

### Filtros disponibles en modo general

- **Plataformas activas** (preselecciona las del setup)
- **Año mínimo** (slider: desde tal año en adelante)
- **Duración del compromiso (para series):** miniserie / serie corta / serie larga
- **Excluir lo que ya vi** (activado por defecto)
- **Excluir géneros:** terror, romance, deportes, etc.

### Filtros adicionales en modo cinéfilo

Todo lo anterior, más:

- **Estructura narrativa (series):** capítulos independientes / hay que ver en orden / cada temporada es otra historia
- **Origen:** Hollywood / cine de autor internacional / asiático / europeo / latinoamericano / independiente
- **Época:** estreno reciente / últimos 5 años / desde 2000 / clásico moderno (1970-2000) / clásico (anterior a 1970)
- **Tono general:** más realista / más fantasioso / más artístico
- **Tipo de contenido:** ficción / documental / animación / híbrido / especial o standup

**Importante:** el modo general puede acceder a los filtros del modo cinéfilo desde un botón "ver filtros avanzados", sin tener que cambiar de modo. La diferencia es solo qué aparece por defecto.

---

## Presentación de resultados

### Modo normal: tres opciones

Cada opción se presenta con el mismo patrón:

> **[Título]**
> *Año · duración o número de temporadas · plataforma · idioma original*
> Una frase corta explicando por qué encaja con lo que pediste.
> Botones: **Verlo ahora** · **Tráiler** · **Más info** · **Guardar** · **Saltar**

Las tres opciones siguen siempre la misma lógica, ajustada al perfil del usuario:

1. **La segura** — Alta probabilidad de gustar según lo pedido.
2. **La distinta** — Cumple lo pedido por un camino menos obvio.
3. **La sorpresa** — Algo fuera del molde habitual del usuario.

### Ejemplo modo general (perfil casual)

Para alguien que pidió algo liviano para ver con su pareja:

| Tipo | Título | Por qué |
|---|---|---|
| La segura | *Ted Lasso* | Comedia cálida, episodios cortos, todo el mundo termina queriéndola. |
| La distinta | *Modern Family* | Si nunca la viste, son 11 temporadas para tener siempre algo a mano. Más liviana imposible. |
| La sorpresa | *Paddington 2* | Sí, la película del oso. Es genuinamente buena y te va a sacar una sonrisa garantizada. |

### Ejemplo modo cinéfilo (mismo pedido base)

| Tipo | Título | Por qué |
|---|---|---|
| La segura | *Fleabag* | Comedia con capas, dos temporadas perfectas, Phoebe Waller-Bridge en su mejor forma. |
| La distinta | *The Bear* | Más tensa que típica comedia pero sigue siendo divertida. Personajes magníficos. |
| La sorpresa | *I Think You Should Leave* | Sketches absurdos de Tim Robinson. Si encaja, vas a llorar de la risa. |

### Modo "Elige tú por mí"

Una sola opción, presentada con seguridad y sin alternativas visibles. Botón pequeño *no, otra* por si la rechaza.

### Modo voz

Vera lee cada opción en una frase corta (título, formato, plataforma, una razón breve). Espera respuesta. Profundiza solo si se le pide.

---

## Recomendaciones guardadas

Una entrada propia desde la home. No es solo una watchlist: cada título conserva el contexto en que se sugirió.

### Estructura de cada elemento guardado

- Título y datos básicos
- Estado: pendiente / empezada / vista / abandonada
- Fecha en que se recomendó
- **Contexto:** la frase original por la que Vera la sugirió
- Plataforma actual (verificada en tiempo real, avisa si cambia)

### Vistas de ordenamiento

- **Por fecha** — Más reciente primero. Para "lo que acepté hace poco y no llegué a ver".
- **Por estado** — Pendientes primero, luego empezadas, luego vistas. Para "qué tengo a medias".
- **Por contexto actual** — Vera filtra automáticamente las guardadas que encajan con lo que estás pidiendo en este momento.

### Integración con el flujo

Si entras al wizard y eliges una intención, antes de seguir con las preguntas Vera revisa si tienes guardadas que encajen. Si hay, las ofrece primero:

> Tienes tres guardadas que aceptaste para este tipo de momento. ¿Quieres verlas antes de buscar algo nuevo?

### Acciones disponibles

- Verla ahora (lleva a la plataforma)
- Marcar como vista
- Marcar como abandonada (con motivo opcional: no me enganchó, muy lenta, no era lo que esperaba)
- Quitar de la lista
- Recordar contexto (botón que muestra la frase original con la que se sugirió)

### Caducidad opcional

Las pendientes nunca empezadas se mueven a "archivadas" después de seis meses, con aviso suave: *aceptaste esto hace mucho y nunca lo viste, ¿sigue interesándote?*

---

## Conversación libre

Para usuarios que prefieren hablar sin estructura. Acceso desde la home.

Vera interpreta lo que el usuario dice (hablado o escrito) y responde con recomendaciones contextuales. Sin formularios. Sin preguntas obligatorias.

### Ejemplo de conversación (modo cálido)

> **Vera:** Cuéntame qué tienes en la cabeza. Lo que sea, en desorden está bien.
>
> **Usuario:** Estoy raro hoy, vi una peli la semana pasada que me dejó pensando, no sé qué quiero ver ahora. Algo que no me cambie el ánimo pero tampoco que me hunda más.
>
> **Vera:** Entendido. Algo que respete cómo te dejó esa peli pero sin profundizar la sensación. ¿Te abro tres opciones o prefieres que te tire una sola?
>
> **Usuario:** Una, sorpréndeme.
>
> **Vera:** *Columbus*, una peli muy tranquila sobre arquitectura, padres y distancia. Mira con cuidado, no apura. Está en Mubi. ¿Hablamos de ella después si quieres?

Vera detecta:
- Mención a otra película → guarda la referencia para futuras recomendaciones.
- Estado emocional implícito → ajusta tono y selección.
- Preferencia de cantidad → respeta si el usuario pide menos o más opciones.

---

## Modo grupo

Para decidir entre dos o más personas.

### Flujo

1. Una persona crea una sala (se genera un código corto).
2. Las demás se unen desde sus propios celulares.
3. Cada uno responde el wizard desde su perspectiva.
4. Vera busca la intersección de preferencias y muestra opciones compatibles para todos.

### Pantalla del organizador

Muestra:
- Lista de participantes con estado (respondiendo, listo)
- Intersección preliminar a medida que llegan respuestas
- Recomendaciones finales cuando todos terminan

### Resolución de conflictos

Si no hay intersección posible:
- Vera lo dice claro: "no encontré nada que les guste a los tres. ¿Quieren ceder en algo?"
- Ofrece tres caminos: votar entre opciones imperfectas, que alguien ceda explícitamente, o partir en dos sesiones separadas.

### Perfiles compartidos

Aparte de las sesiones puntuales, se pueden crear perfiles compartidos guardados ("pareja", "amigos del viernes", "familia"). Cada perfil tiene su propia historia de recomendaciones y aprendizaje.

---

## Feedback post-visionado

Cuando el usuario vuelve después de haber visto algo recomendado, Vera pregunta brevemente.

### Pregunta inicial (un toque)

> ¿Te dejó como esperabas?
> 😶 / 🥲 / 🫶 / ✨ / 🤯

Suficiente con elegir una. Alimenta el aprendizaje silencioso.

### Preguntas adicionales (opcionales)

> - ¿La terminaste?
> - ¿La recomendarías a alguien en tu mismo estado de ese día?
> - ¿Algo que no funcionó? (campo libre, opcional)

### Lo que el sistema aprende

- Si lo terminaste o lo abandonaste y en qué punto.
- Si el contexto en que se recomendó funcionó (estado de ánimo, momento, compañía).
- Patrones de aceptación vs visualización efectiva.

---

## Inteligencia silenciosa

Cosas que el wizard hace sin preguntar:

### Ajustes automáticos por contexto

- Si el modo es "para dejar de fondo" y el contenido está en un idioma que entiendes de oído, no menciona subtítulos ni doblaje.
- Si el contenido NO está en un idioma que entiendes, aplica tu preferencia de doblaje del setup o descarta el contenido.
- Si dijiste "tengo 90 minutos exactos", se omite la pregunta película/serie (es película).
- Si dijiste "quiero empezar algo para las próximas semanas", se omite (es serie).
- Si rechazas tres recomendaciones seguidas, Vera cambia de estrategia y pregunta directamente qué no funcionó.

### Aprendizaje acumulativo

- Si rechazas sistemáticamente series largas, dejan de aparecer por defecto.
- Si nunca aceptas películas en blanco y negro, se filtran (pero se pueden reactivar).
- Si aceptas mucho contenido de cierto país o director, se inclina a recomendar más.
- Si aceptas mucho pero ves poco, Vera lo nota y se vuelve más selectivo.

### Memoria temporal

- Lo último recomendado y aceptado no se vuelve a sugerir en al menos 30 días.
- Lo abandonado no se vuelve a sugerir, salvo que el usuario lo pida.
- Las series en pausa se ofrecen para retomar después de un tiempo prudencial.

### Detección de patrones

De vez en cuando, Vera muestra patrones que el usuario no notó:

> Las películas que más te gustaron este año tienen tres cosas en común: protagonistas femeninas, ambientación europea, finales abiertos. ¿Quieres profundizar en esa dirección o probar lo opuesto?

---

## Capacidades adicionales

### Disponibilidad en tiempo real

Antes de recomendar, Vera verifica que el contenido siga disponible en alguna plataforma del usuario. Nada de sugerir algo que ya salió de catálogo.

### Alertas de salida de catálogo

> *First Cow* se va de Mubi en 11 días. La guardaste hace dos meses.

### Modo "tengo X horas"

Cuando hay restricción de tiempo dura, calcula si una serie corta entra completa en el rato disponible, o si una película tiene una duración aproximada.

### Modo "cuándo"

Recomendaciones según día de la semana, época del año, momento del día. Algo de Navidad en diciembre, algo de terror en octubre, algo soleado en pleno invierno.

### Modo "ocasión especial"

Aniversario, primera cita, noche con amigos, cena familiar incómoda donde hay que poner algo que no ofenda a nadie. Cada ocasión tiene su lógica.

### Continuidad inteligente

Si dejaste una serie a la mitad hace tiempo, Vera te pregunta si quieres retomarla y te da un mini-recordatorio de dónde ibas (sin spoilers más allá de lo que ya viste).

### Aviso de contenido sensible

Marca contenido que incluya cosas que el usuario pidió evitar (violencia gráfica, escenas con animales, suicidio, etc.). Sin censurar, solo avisar.

### Accesibilidad

- Filtrar por contenido con audiodescripción o subtítulos descriptivos disponibles.
- Modo lectura simplificada para preguntas y descripciones.
- Compatibilidad con lectores de pantalla.
- Tamaño de texto ajustable.

### Compartir y diario

- Botón para compartir una recomendación con alguien, incluyendo el contexto de por qué se la mandas.
- Diario opcional de visualizaciones, con espacio para una línea propia. Con el tiempo se vuelve un objeto valioso, sin obligación de mantenerlo.

### Rabbit hole guiado

Después de ver algo, Vera propone un siguiente paso lógico, ajustado al perfil:

- Modo general: "te gustó esta, te puede gustar esta otra parecida".
- Modo cinéfilo: "viste esta del director X, ¿quieres seguir con su filmografía o saltar a otras pelis con esta atmósfera?"

---

## Lineamientos de estética visual

La versión 2.0 abandona el look de revista de cine y busca algo más cálido y universal, sin caer en el extremo opuesto (genérico, infantil, app cualquiera).

### Paleta

**Tema claro (default):**
- Fondo: blanco hueso o crema muy suave
- Texto principal: gris oscuro
- Acento: un solo color cálido (naranja, durazno, terracota suave)
- Detalles: dorado o verde oliva sutiles

**Tema oscuro (opcional):**
- Fondo: gris muy oscuro, no negro puro
- Texto: crema cálida
- Acentos iguales pero más saturados

### Tipografía

- **Una sola fuente principal**, legible, con personalidad pero no excéntrica. Geist, General Sans o similar. Nada de fuentes serif dramáticas como Fraunces en todos lados.
- Cursivas solo para detalles puntuales (citas, énfasis suave).
- Tamaños generosos, no apretados.

### Tono visual

- Mucho espacio en blanco.
- Bordes redondeados suaves (no estridentes, no rectos).
- Iconos lineales simples, no monoespaciados ni decorativos.
- Animaciones discretas y funcionales.
- Sin números grandes en mayúsculas tipo "01 · PREGUNTA".
- Sin cursivas dramáticas en cada pantalla.

### Componentes

- Botones grandes, fáciles de tocar.
- Tarjetas claramente diferenciadas.
- Estado siempre visible (qué pregunta es, qué eligió antes, dónde va).
- Lenguaje en los botones siempre activo y simple ("Siguiente", "Listo", "Mostrar opciones").

### Lo que se mantiene de la v1

- Estructura de capas (sigue siendo correcta).
- Modo voz con onda visual al hablar.
- Idea de "tres opciones con razón" para los resultados.
- Personalización profunda sin sobrecargar al usuario casual.

---

## Glosario interno: lenguaje accesible

Tabla de reemplazos para evitar jerga innecesaria. La columna derecha es lo que ve el usuario por defecto. La izquierda solo aparece en modo cinéfilo.

| Modo cinéfilo | Modo general |
|---|---|
| Cine de autor | Película más artística |
| Autoconclusiva (serie) | Capítulos que se pueden ver sueltos |
| Serializada | Hay que verla en orden |
| Antológica | Cada temporada es una historia distinta |
| Tono onírico | Más fantasiosa, irreal |
| Tono crudo | Más áspera, dura |
| Tono estilizado | Visualmente muy cuidada |
| Obra canónica | Película famosa que mucha gente vio |
| Slow burn | Avanza lento pero atrapa |
| Drama denso | Drama fuerte |
| No ficción ensayística | Documental que te hace pensar |
| Filmografía | Películas de tal director |
| Cine experimental | Algo muy distinto a lo habitual |
| Festival / arthouse | Película premiada en festivales |

---

## Resumen del rediseño

| Aspecto | Versión 1.0 | Versión 2.0 |
|---|---|---|
| Público | Implícitamente cinéfilo | General, con modo cinéfilo equivalente |
| Tono visual | Editorial oscuro tipo revista de cine | Cálido, claro, amigable, con opción oscura |
| Tipografía | Fraunces (serif expresiva) en todos lados | Una fuente legible, cursiva solo para énfasis |
| Lenguaje | Jerga cinéfila por default | Lenguaje cotidiano por default, jerga opt-in |
| Ejemplos | Mubi-centric (Past Lives, Aftersun, Columbus) | Mix (Ted Lasso, Paddington 2, Modern Family) en general; cinéfilos en su modo |
| Personalidad de Vera | Una sola voz poética | Tres modos: directo, cálido, cinéfilo |
| Profundidad | Sin distinción | Cuatro niveles configurables |
| Filtros avanzados | Visibles para todos | Por defecto solo los básicos, avanzados accesibles |

---

**Vera v2.0** — Documento de diseño completo
*Wizard de recomendaciones de películas y series para todo público*
