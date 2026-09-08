// Idiomas para las preferencias del setup.
//
// Dos preguntas distintas, que la gente confunde pero el motor no:
//
//   COMODIDAD  — "¿en qué idiomas te mueves cómodo?". Es sobre leer subtítulos
//                y bancarse una película que no está en tu lengua. Quien elige
//                "solo español" no odia el coreano: no quiere leer dos horas.
//                Se trata como PREFERENCIA: lo demás baja, no desaparece.
//
//   RECHAZO    — "¿qué idiomas no toleras escuchar?". Es sobre el sonido, y es
//                categórico: hay gente a la que la fonética de una lengua le
//                resulta insoportable y no hay ranking que lo arregle.
//                Se trata como EXCLUSIÓN dura.
//
// Los códigos son ISO 639-1, que es lo que TMDb devuelve en original_language
// y lo que el importador guarda en vera_titles.languages.

export interface Idioma {
  codigo: string;
  nombre: string;
}

// Los idiomas con presencia real en catálogo de cine. No es la lista completa
// de ISO 639-1 a propósito: 180 chips no se navegan con flechas.
export const IDIOMAS: Idioma[] = [
  { codigo: "es", nombre: "Español" },
  { codigo: "en", nombre: "Inglés" },
  { codigo: "fr", nombre: "Francés" },
  { codigo: "it", nombre: "Italiano" },
  { codigo: "pt", nombre: "Portugués" },
  { codigo: "de", nombre: "Alemán" },
  { codigo: "ja", nombre: "Japonés" },
  { codigo: "ko", nombre: "Coreano" },
  { codigo: "zh", nombre: "Chino" },
  { codigo: "hi", nombre: "Hindi" },
  { codigo: "ru", nombre: "Ruso" },
  { codigo: "ar", nombre: "Árabe" },
  { codigo: "tr", nombre: "Turco" },
  { codigo: "th", nombre: "Tailandés" },
  { codigo: "sv", nombre: "Sueco" },
  { codigo: "da", nombre: "Danés" },
  { codigo: "pl", nombre: "Polaco" },
  { codigo: "nl", nombre: "Neerlandés" },
];

const PorCodigo = new Map(IDIOMAS.map((i) => [i.codigo, i.nombre]));

// Nombre legible de un código. Si TMDb devuelve uno que no está en la lista
// (pasa: hay cine en malayalam), se muestra el código en mayúsculas — es mejor
// que un hueco, y no vale la pena mantener 180 traducciones para eso.
export function nombreIdioma(codigo: string): string {
  return PorCodigo.get(codigo) ?? codigo.toUpperCase();
}

// Opciones del paso de comodidad. `idiomas: []` significa "sin restricción".
export const COMODIDAD_IDIOMAS: {
  id: string;
  label: string;
  desc: string;
  idiomas: string[];
}[] = [
  {
    id: "cualquiera",
    label: "Cualquier idioma",
    desc: "Los subtítulos no me molestan",
    idiomas: [],
  },
  {
    id: "es_en",
    label: "Español e inglés",
    desc: "Lo demás, mejor no",
    idiomas: ["es", "en"],
  },
  {
    id: "es",
    label: "Solo en español",
    desc: "No quiero leer",
    idiomas: ["es"],
  },
];

// Qué opción representa una lista guardada. Se compara como conjunto para no
// depender del orden en que se serializó.
export function comodidadDesde(idiomas: string[]): string {
  if (idiomas.length === 0) return "cualquiera";
  const s = new Set(idiomas);
  if (s.size === 1 && s.has("es")) return "es";
  if (s.size === 2 && s.has("es") && s.has("en")) return "es_en";
  return "cualquiera";
}
