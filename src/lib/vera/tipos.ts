// Tipos del prototipo Vera and Chill.
// Mantener simples — el motor es determinista.

export type Tono = "liviano" | "denso";
// Regla de marca: "ninos" SIEMPRE implica adulto presente.
// No existe "niños solos" como contexto en Kütral.
export type Contexto = "solo" | "pareja" | "amigos" | "ninos";

// Qué busca el usuario hoy. Cada uno mapea a géneros TMDb concretos
// y dispara un discover en vivo (pool distinto en cada sesión).
export type Intencion = "liviano" | "denso" | "adrenalina" | "sorpresa";

// De dónde vino una peli al pool. Señal de oro para el motor.
//   - discover:   matchea el intent puro (peso neutro).
//   - reco:       vino de /movie/{id}/recommendations de una semilla del
//                 usuario (algo que le gustó antes), peso alto.
//   - diversidad: vino de la fuente garantizada de representación LGBT+
//                 (keyword 158718 en TMDb). Trato neutro como discover.
//   - fallback:   SEMILLA_FALLBACK, último recurso, peso bajo.
export type Procedencia = "discover" | "reco" | "diversidad" | "fallback";

// Perfil histórico del usuario (vía historial.ts.getPerfilHistorico).
// Se calcula a partir de reacciones persistidas + watch_history, cruzando
// con el cache de géneros que pobla mapear() en tmdb.ts.
export interface PerfilHistorico {
  // Peso acumulado por género (nombre español TMDb).
  // Positivo = el usuario tiende a gustar de ese género.
  generosPesos: Map<string, number>;
  // Top 3 géneros por peso descendente.
  top3: string[];
}

// Una película tal como la consume el flujo Vera.
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
  descripcion: string;      // overview / sinopsis
  director: string;         // primer director
  actores: string[];        // top 4 del reparto
  anio: string;             // "2024"
  runtime: number | null;   // minutos
  posterPath: string | null;   // path TMDb tipo "/abc.jpg" (sin host)
  backdropPath: string | null; // path TMDb backdrop (imagen de escena)
  imagenes: string[];          // backdrops extra (fotogramas), paths TMDb sin host
  trivia: string;           // una frase generada por reglas

  // De qué fuente vino al pool. La asigna construirPool al mergear; mapear()
  // la deja en "discover" por default.
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
}

// Resultado del motor (B5): lista entera del pool rankeada por score desc.
// La UI muestra la #1 destacada arriba y el resto navegable abajo.
// Sin más "segura/distinta/sorpresa" — el usuario navega y elige.
export interface RankingPeli {
  pelicula: Pelicula;
  score: number;
}
