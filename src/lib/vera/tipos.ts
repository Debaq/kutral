// Tipos del motor Vera and Chill.
// Mantener simples — el motor es determinista.

export type Tono = "liviano" | "denso";
// Regla de marca: "ninos" SIEMPRE implica adulto presente.
// No existe "niños solos" como contexto en Kütral.
export type Contexto = "solo" | "pareja" | "amigos" | "ninos";

// Qué busca el usuario hoy. Cada uno mapea a géneros TMDb concretos
// y dispara un discover en vivo (pool distinto en cada sesión).
export type Intencion = "liviano" | "denso" | "adrenalina" | "sorpresa";

// Voz de Vera. Se persiste en vera_setup.personality y solo afecta copy.
export type Personalidad = "calida" | "directa" | "seca";

// Postura frente a la animación. Es su propio eje y no una exclusión de género
// más: "no me gustan" se podría expresar excluyendo Animación, pero "me
// encantan" no tiene forma de decirse con una lista de vetos.
export type PrefAnimacion = "gusta" | "indiferente" | "no";

// De dónde vino una peli al pool. Señal de oro para el motor.
//   - discover:   matchea el intent puro (peso neutro).
//   - reco:       vino de /movie/{id}/recommendations de una semilla del
//                 usuario (algo que le gustó antes), peso alto.
//   - diversidad: vino de la fuente garantizada de representación LGBT+
//                 (keyword 158718 en TMDb). Trato neutro como discover.
//   - local:      salió del catálogo local (vera_titles), ya filtrado por
//                 las exclusiones del perfil. Mismo trato que discover.
//   - fallback:   SEMILLA_FALLBACK, último recurso, peso bajo.
export type Procedencia =
  | "discover"
  | "reco"
  | "diversidad"
  | "local"
  | "fallback";

// Perfil histórico del usuario (vía historial.ts.getPerfilHistorico).
// Se calcula a partir de reacciones persistidas + watch_history, cruzando
// con el cache de géneros que pobla mapear() en tmdb.ts.
export interface PerfilHistorico {
  // Peso acumulado por género (nombre español TMDb).
  // Positivo = el usuario tiende a gustar de ese género.
  generosPesos: Map<string, number>;
  // Igual pero por década ("1990", "2020"): captura "me gusta el cine viejo".
  decadasPesos: Map<string, number>;
  // Igual pero por director y por actor. Señal fuerte y barata: si calificaste
  // +5 tres pelis de Villeneuve, la cuarta debería subir sola.
  directoresPesos: Map<string, number>;
  actoresPesos: Map<string, number>;
  // Top 3 géneros por peso descendente.
  top3: string[];
  // Cuántas reacciones reales sostienen este perfil. La UI la usa para decidir
  // cuánta calibración pedir (ver config.CARTAS_POR_MADUREZ).
  muestras: number;
}

// Una película tal como la consume el flujo Vera.
export interface Pelicula {
  id: string;
  titulo: string;
  generos: string[];
  tono: Tono;
  familyFriendly: boolean;
  rating: number;       // 0..10 (vote_average TMDb)
  votos: number;        // vote_count TMDb — sin esto rating no significa nada
  popularidad: number;  // popularity TMDb cruda (no normalizada)
  pais: string;
  // Idioma original en ISO 639-1 ("ko", "fr"). "" = desconocido; nunca se
  // excluye por un idioma que no sabemos.
  idiomaOriginal: string;
  poster: string;       // color hex de fallback (si no hay imagen)
  gancho: string;       // frase corta emocional (no es la sinopsis larga)

  // Metadata enriquecida (TMDb o equivalente).
  descripcion: string;      // overview / sinopsis
  director: string;         // primer director
  actores: string[];        // top 4 del reparto
  anio: string;             // "2024"
  runtime: number | null;   // minutos
  posterPath: string | null;   // path TMDb tipo "/abc.jpg" (sin host)
  backdropPath: string | null; // path TMDb backdrop (imagen de escena)
  imagenes: string[];          // backdrops extra (fotogramas), paths TMDb sin host
  trivia: string;           // una frase generada por reglas

  // imdb_id de TMDb. Proxy de "reproducible": sin imdb_id ningún scraper la
  // encuentra. null = no sabemos todavía (peli no enriquecida).
  imdbId: string | null;

  // Temas sensibles (slugs de vera_theme_list) detectados vía keywords TMDb
  // o traídos del catálogo local. Vacío tiene DOS significados distintos según
  // `enriquecida`: si es false, "todavía no miramos"; si es true, "no hay".
  temasSensibles: string[];

  // Plataformas donde está (slugs de vera_platform_list). Solo se puebla desde
  // el catálogo local — TMDb detail no trae watch/providers en este flujo.
  plataformas: string[];

  // true cuando la peli pasó por tmdb_detail (tiene cast, director, runtime,
  // imdbId y temas verificados). false = viene del listado, datos parciales.
  enriquecida: boolean;

  // De qué fuente vino al pool. La asigna construirPool al mergear.
  procedencia: Procedencia;
}

// Reacción del usuario a una peli (modelo B5).
//
// Dos escalas, una sola UI:
//   - interes: -5..+5, intuición previa (cursor horizontal en la lista).
//   - juicio:  -5..+5 o null. null = no la vio. Si juicio !== null, fue vista.
//
// "Visto" NO es campo, es derivado: (juicio !== null). Evita el bug clásico
// de dos campos sincronizados a mano.
//
// Regla de modelado: cuando hay juicio, MANDA juicio e interes se vuelve
// irrelevante para esa peli (no promediar). Si intuí +3 y tras verla puse
// -4, el +3 ya no significa nada — el juicio post-vista lo reemplaza.
export interface Reaccion {
  pelicula: Pelicula;
  interes: number;        // -5..+5
  juicio: number | null;  // -5..+5 si vista; null si no.
}

// Perfil persistente del usuario (tabla vera_setup, fila única id=1).
// Lo llena el asistente de configuración; se puede reabrir y editar.
export interface PerfilSetup {
  plataformas: string[];        // slugs vera_platform_list
  generosExcluidos: string[];   // slugs vera_genre_list — nunca me los muestres
  temasExcluidos: string[];     // slugs vera_theme_list — trigger warnings
  personalidad: Personalidad;
  // Tope de duración en minutos. null = sin tope. Es PREFERENCIA: lo más largo
  // baja en el ranking, no se excluye.
  duracionMax: number | null;
  animacion: PrefAnimacion;
  // Idiomas en los que la persona se mueve cómoda (ISO 639-1).
  // [] = sin restricción. PREFERENCIA: lo de afuera baja, no desaparece —
  // es sobre leer subtítulos, no sobre rechazar la película.
  idiomasComodos: string[];
  // Idiomas que no tolera escuchar. EXCLUSIÓN dura: acá no hay ranking que
  // valga, si no soportas cómo suena una lengua no la vas a ver igual.
  idiomasEvitados: string[];
}

// Estado global que se va llenando durante el flujo.
//
// vistas / interesadas se removieron — son derivados de reacciones:
//   vistas      = reacciones.filter(r => r.juicio !== null).map(r => r.pelicula.id)
//   interesadas = reacciones.filter(r => r.interes >= 3 || (r.juicio ?? 0) >= 3)
// Si algún consumer las necesita, las calcula on the fly.
export interface EstadoVera {
  contexto: Contexto | null;
  // Lo que el usuario eligió en la pantalla "intencion".
  // null mientras no haya pasado por esa pantalla.
  intencion: Intencion | null;
  reacciones: Reaccion[];
  horaActual: number;    // 0..23
  // Perfil persistente. null si el usuario nunca configuró a Vera — el motor
  // funciona igual, solo sin exclusiones.
  setup: PerfilSetup | null;
}

// Resultado del motor: lista entera del pool rankeada.
// La UI muestra la #1 destacada arriba y el resto navegable abajo.
export interface RankingPeli {
  pelicula: Pelicula;
  score: number;
  // Desglose del score, para el "por qué te la propongo" de la ficha.
  // Los componentes están en 0..1 ANTES de multiplicarse por su peso.
  porQue: MotivoRanking;
}

// Por qué esta peli quedó donde quedó. La UI lo traduce a lenguaje humano;
// el motor solo reporta números y las etiquetas que dispararon afinidad.
export interface MotivoRanking {
  afinidad: number;      // 0..1 — match con tu perfil
  calidad: number;       // 0..1 — rating bayesiano
  procedencia: number;   // 0..1 — de dónde vino
  // Penalizaciones aplicadas (números negativos ya sumados al score).
  penalVista: number;
  penalDuracion: number;
  // Ajuste por contexto del sillón (puede ser + o -).
  ajusteContexto: number;
  // Etiquetas que más aportaron: géneros/director/década que el usuario ya
  // demostró que le gustan y esta peli tiene.
  razones: string[];
}
