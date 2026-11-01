// Audiolibros: LibriVox a través de Internet Archive.
//
// Dominio público, sin clave. Archive responde con CORS abierto (`*`) y el CSP
// del webview ya deja pedir https, así que todo va desde el front, sin backend.
//
//   - Catálogo: advancedsearch, colección `librivoxaudio`, filtrada por idioma.
//   - Ficha: /metadata/<id> → lista de archivos. Un mp3 por capítulo, con
//     `title` y `length`. Portada en /services/img/<id>.
//   - Audio: /download/<id>/<archivo> → 302 a un dnXXX.archive.org que sirve
//     `audio/mpeg` con rangos (se puede saltar dentro del capítulo).
//
// Plan y números del sondeo: docs/PLAN_AUDIOLIBROS_VIDEOCLIPS.md.

export type Idioma = "es" | "en";
export type FiltroIdioma = Idioma | "todos";

export type Libro = {
  id: string;
  titulo: string;
  autor: string;
  /** null si Archive no dice un idioma que reconozcamos. */
  idioma: Idioma | null;
  portada: string;
};

export type Capitulo = {
  /** 1-based, en el orden en que se escucha. */
  n: number;
  titulo: string;
  url: string;
  segundos: number | null;
};

export type Ficha = Libro & {
  descripcion: string;
  capitulos: Capitulo[];
  /** Suma de los capítulos con duración conocida; null si ninguno la trae. */
  segundosTotal: number | null;
};

export type PaginaCatalogo = { total: number; libros: Libro[] };

const ARCHIVE = "https://archive.org";
export const POR_PAGINA = 48;

// Archive guarda el idioma como texto libre: "spa", "Spanish", "spanish",
// "Español"… La consulta los cubre todos y `normalizarIdioma` los unifica.
const VARIANTES: Record<Idioma, string[]> = {
  es: ["spanish", "spa", "español", "espanol", "es"],
  en: ["english", "eng", "en"],
};

const CONSULTA_IDIOMA: Record<FiltroIdioma, string> = {
  es: "language:(spanish OR Spanish OR spa OR Español)",
  en: "language:(english OR English OR eng)",
  todos: "language:(spanish OR Spanish OR spa OR Español OR english OR English OR eng)",
};

export function normalizarIdioma(v: unknown): Idioma | null {
  const lista = Array.isArray(v) ? v : [v];
  for (const x of lista) {
    if (typeof x !== "string") continue;
    const s = x.trim().toLowerCase();
    for (const id of ["es", "en"] as Idioma[]) {
      if (VARIANTES[id].includes(s)) return id;
    }
  }
  return null;
}

/** "ES" / "EN" para la etiqueta de la tarjeta. */
export function etiquetaIdioma(i: Idioma | null): string {
  return i ? i.toUpperCase() : "";
}

function texto(v: unknown): string {
  if (Array.isArray(v)) return v.filter((x) => typeof x === "string").join(", ");
  return typeof v === "string" ? v : "";
}

/**
 * Lo que escribe el usuario, sin la sintaxis de Lucene (comillas, paréntesis,
 * dos puntos…): una búsqueda con "Quijote: vol 1" no debe romper la consulta
 * ni filtrar por un campo inventado.
 */
export function limpiarBusqueda(q: string): string {
  return q
    .replace(/[+\-!(){}[\]^"~*?:\\/&|]/g, " ")
    .replace(/\b(AND|OR|NOT)\b/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function urlCatalogo(idioma: FiltroIdioma, q: string, pagina: number): string {
  const busqueda = limpiarBusqueda(q);
  let consulta = `collection:librivoxaudio AND ${CONSULTA_IDIOMA[idioma]}`;
  if (busqueda) consulta += ` AND (${busqueda})`;
  const p = new URLSearchParams();
  p.set("q", consulta);
  for (const f of ["identifier", "title", "creator", "language"]) p.append("fl[]", f);
  // Sin búsqueda, "lo más escuchado" hace de destacados. Con búsqueda manda
  // la relevancia (el orden por defecto de Archive).
  if (!busqueda) p.append("sort[]", "downloads desc");
  p.set("rows", String(POR_PAGINA));
  p.set("page", String(Math.max(1, pagina)));
  p.set("output", "json");
  return `${ARCHIVE}/advancedsearch.php?${p.toString()}`;
}

export function portadaDe(id: string): string {
  return `${ARCHIVE}/services/img/${encodeURIComponent(id)}`;
}

type DocArchive = { identifier?: string; title?: unknown; creator?: unknown; language?: unknown };

export function libroDeDoc(d: DocArchive): Libro | null {
  if (!d.identifier) return null;
  return {
    id: d.identifier,
    titulo: texto(d.title) || d.identifier,
    autor: texto(d.creator),
    idioma: normalizarIdioma(d.language),
    portada: portadaDe(d.identifier),
  };
}

export async function buscarLibros(
  idioma: FiltroIdioma,
  q: string,
  pagina: number,
  signal?: AbortSignal,
): Promise<PaginaCatalogo> {
  const r = await fetch(urlCatalogo(idioma, q, pagina), { signal });
  if (!r.ok) throw new Error(`Archive respondió ${r.status}`);
  const j = await r.json();
  const docs: DocArchive[] = j?.response?.docs ?? [];
  return {
    total: Number(j?.response?.numFound ?? 0),
    libros: docs.map(libroDeDoc).filter((x): x is Libro => x !== null),
  };
}

/** "07:00" → 420 · "1:02:03" → 3723 · "420.5" → 420.5 · basura → null. */
export function parseDuracion(v: unknown): number | null {
  if (typeof v === "number") return Number.isFinite(v) && v > 0 ? v : null;
  if (typeof v !== "string" || !v.trim()) return null;
  const partes = v.trim().split(":");
  if (partes.length > 3) return null;
  let s = 0;
  for (const p of partes) {
    const n = Number(p);
    if (!Number.isFinite(n) || n < 0) return null;
    s = s * 60 + n;
  }
  return s > 0 ? s : null;
}

/** 3723 → "1:02:03" · 420 → "7:00". */
export function fmtDuracion(seg: number | null | undefined): string {
  if (seg == null || !Number.isFinite(seg) || seg < 0) return "";
  const t = Math.floor(seg);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** "12 h 5 min" para la ficha. */
export function fmtLargo(seg: number | null | undefined): string {
  if (seg == null || !Number.isFinite(seg) || seg <= 0) return "";
  const h = Math.floor(seg / 3600);
  const m = Math.round((seg % 3600) / 60);
  if (h === 0) return `${Math.max(1, m)} min`;
  return m ? `${h} h ${m} min` : `${h} h`;
}

const ENTIDADES: Record<string, string> = {
  amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ",
  aacute: "á", eacute: "é", iacute: "í", oacute: "ó", uacute: "ú", ntilde: "ñ",
  Aacute: "Á", Eacute: "É", Iacute: "Í", Oacute: "Ó", Uacute: "Ú", Ntilde: "Ñ",
  uuml: "ü", iexcl: "¡", iquest: "¿", laquo: "«", raquo: "»", mdash: "—", ndash: "–",
};

/** La descripción de LibriVox viene en HTML: a texto plano, con párrafos. */
export function limpiarHtml(html: string): string {
  return html
    .replace(/<\s*br\s*\/?>/gi, "\n")
    .replace(/<\/\s*p\s*>/gi, "\n\n")
    .replace(/<[^>]*>/g, "")
    .replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n)))
    .replace(/&#x([0-9a-f]+);/gi, (_, n) => String.fromCodePoint(parseInt(n, 16)))
    .replace(/&([a-z]+);/gi, (m, e) => ENTIDADES[e] ?? m)
    .replace(/ /g, " ")
    .replace(/[ \t]+/g, " ")
    .replace(/ *\n */g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

type ArchivoArchive = {
  name?: string;
  format?: string;
  title?: string;
  length?: unknown;
  track?: unknown;
};

// Un libro trae el mismo capítulo en varios formatos. Se elige UNO, el
// primero que exista de esta lista: 64 kbps alcanza para voz y pesa la mitad.
const FORMATOS = ["64Kbps MP3", "VBR MP3", "128Kbps MP3", "MP3"];

function urlArchivo(id: string, nombre: string): string {
  // El nombre puede traer subcarpetas: se codifica cada tramo, no las barras.
  const ruta = nombre.split("/").map(encodeURIComponent).join("/");
  return `${ARCHIVE}/download/${encodeURIComponent(id)}/${ruta}`;
}

function numeroPista(v: unknown): number | null {
  const n = parseInt(String(v ?? ""), 10); // "3/37" → 3
  return Number.isFinite(n) ? n : null;
}

export function capitulosDe(id: string, archivos: ArchivoArchive[]): Capitulo[] {
  const formato = FORMATOS.find((f) => archivos.some((a) => a.format === f && a.name));
  const elegidos = formato
    ? archivos.filter((a) => a.format === formato && a.name)
    : archivos.filter((a) => a.name?.toLowerCase().endsWith(".mp3"));
  const orden = [...elegidos].sort((a, b) => {
    const ta = numeroPista(a.track);
    const tb = numeroPista(b.track);
    if (ta !== null && tb !== null && ta !== tb) return ta - tb;
    return (a.name ?? "").localeCompare(b.name ?? "", undefined, { numeric: true });
  });
  return orden.map((a, i) => ({
    n: i + 1,
    titulo: (a.title ?? "").trim() || (a.name ?? "").replace(/\.mp3$/i, "").replace(/_/g, " "),
    url: urlArchivo(id, a.name!),
    segundos: parseDuracion(a.length),
  }));
}

export function fichaDeMetadata(id: string, j: unknown): Ficha {
  const m = (j as { metadata?: Record<string, unknown> })?.metadata ?? {};
  const archivos = ((j as { files?: ArchivoArchive[] })?.files ?? []) as ArchivoArchive[];
  const capitulos = capitulosDe(id, archivos);
  const conDuracion = capitulos.filter((c) => c.segundos != null);
  return {
    id,
    titulo: texto(m.title) || id,
    autor: texto(m.creator),
    idioma: normalizarIdioma(m.language),
    portada: portadaDe(id),
    descripcion: limpiarHtml(texto(m.description)),
    capitulos,
    segundosTotal: conDuracion.length
      ? conDuracion.reduce((s, c) => s + (c.segundos ?? 0), 0)
      : null,
  };
}

// La ficha no cambia: se recuerda mientras la app esté abierta.
const fichas = new Map<string, Ficha>();

export async function fichaLibro(id: string, signal?: AbortSignal): Promise<Ficha> {
  const ya = fichas.get(id);
  if (ya) return ya;
  const r = await fetch(`${ARCHIVE}/metadata/${encodeURIComponent(id)}`, { signal });
  if (!r.ok) throw new Error(`Archive respondió ${r.status}`);
  const f = fichaDeMetadata(id, await r.json());
  if (f.capitulos.length) fichas.set(id, f);
  return f;
}
