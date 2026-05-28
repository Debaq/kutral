// Tipos del prototipo Vera and Chill.
// Mantener simples — el motor es determinista.

export type Tono = "liviano" | "denso";
// Regla de marca: "ninos" SIEMPRE implica adulto presente.
// No existe "niños solos" como contexto en Kütral.
export type Contexto = "solo" | "pareja" | "amigos" | "ninos";
// Solo señales positivas: vista (con o sin rating) o me llama.
// "pasar" avanza sin generar señal. Vera nunca penaliza por descarte —
// hoy podés rechazar algo que mañana querés ver.
export type Gesto = "pasar" | "vista" | "interes";

// Qué busca el usuario hoy. Cada uno mapea a géneros TMDb concretos
// y dispara un discover en vivo (pool distinto en cada sesión).
export type Intencion = "liviano" | "denso" | "adrenalina" | "sorpresa";

// Una película tal como la consume el flujo Vera.
// La fuente puede ser dummy o TMDb (ver src/lib/vera/tmdb.ts).
export interface Pelicula {
  id: string;
  titulo: string;
  generos: string[];
  tono: Tono;
  familyFriendly: boolean;
  rating: number;       // 0..10
  popularidad: number;  // 0..1 (proxy si la fuente no lo da directo)
  pais: string;
  poster: string;       // color hex de fallback (si no hay imagen)
  gancho: string;       // frase corta emocional (no es la sinopsis larga)

  // Metadata enriquecida (TMDb o equivalente).
  // Si la fuente es dummy, estos campos vienen vacíos o derivados.
  descripcion: string;      // overview / sinopsis
  director: string;         // primer director
  actores: string[];        // top 4 del reparto
  anio: string;             // "2024"
  runtime: number | null;   // minutos
  posterPath: string | null;   // path TMDb tipo "/abc.jpg" (sin host)
  backdropPath: string | null; // path TMDb backdrop (imagen de escena)
  trivia: string;           // una frase generada por reglas
}

// Cada reacción en el mazo deja un trazo: qué peli, qué gesto.
// userRating solo aplica al gesto "vista" — el usuario calificó 1-5.
export interface Reaccion {
  pelicula: Pelicula;
  gesto: Gesto;
  userRating?: number | null; // 1-5 si calificó, null/undefined si no
}

// Estado global que se va llenando durante el flujo.
export interface EstadoVera {
  contexto: Contexto | null;
  // Lo que el usuario eligió en la pantalla "intencion".
  // null mientras no haya pasado por esa pantalla.
  intencion: Intencion | null;
  reacciones: Reaccion[];
  vistas: string[];      // ids
  interesadas: string[]; // ids
  horaActual: number;    // 0..23
}

// Resultado del motor: tres opciones internas. Vera muestra la segura primero.
export interface Recomendaciones {
  segura: { pelicula: Pelicula; score: number } | null;
  distinta: { pelicula: Pelicula; score: number } | null;
  sorpresa: { pelicula: Pelicula; score: number } | null;
}
