// Qué pista de subtítulos del archivo sirve para el idioma elegido en
// Configuración. Sin dependencias, para poder probarlo.

export type Pista = { id: number; lang: string; title: string; forced?: boolean };

// Código ISO 639-1 (el de Configuración) → cómo aparece en los contenedores:
// 639-1, 639-2 (B y T) y nombres en el título de la pista, que muchos
// releases usan en vez del código ("Latino", "English SDH").
const IDIOMAS: Record<string, { codigos: string[]; nombres: RegExp }> = {
  es: { codigos: ["es", "spa", "esp"], nombres: /espa[ñn]ol|spanish|latino|castellano/i },
  en: { codigos: ["en", "eng"], nombres: /english|ingl[eé]s/i },
  fr: { codigos: ["fr", "fre", "fra"], nombres: /fran[cç]ais|french|franc[eé]s/i },
  pt: { codigos: ["pt", "por"], nombres: /portugu|brasil|brazil/i },
  it: { codigos: ["it", "ita"], nombres: /italian/i },
  de: { codigos: ["de", "ger", "deu"], nombres: /deutsch|german|alem[aá]n/i },
  ja: { codigos: ["ja", "jpn"], nombres: /japanese|japon[eé]s|日本/i },
  ko: { codigos: ["ko", "kor"], nombres: /korean|coreano|한국/i },
  zh: { codigos: ["zh", "chi", "zho"], nombres: /chinese|chino|中文/i },
};

/** ¿La pista está en `idioma` (ISO 639-1)? Por código ("es-419" cuenta como
 *  "es"; "est", estonio, no) y, de respaldo, por el título. */
export function pistaDelIdioma(t: Pista, idioma: string): boolean {
  const def = IDIOMAS[idioma];
  if (!def) return false;
  const base = (t.lang || "").toLowerCase().trim().split(/[-_]/)[0];
  if (base && def.codigos.includes(base)) return true;
  return def.nombres.test(t.title || "");
}

/** Pista "forzada": solo carteles y diálogos en otro idioma, no la película.
 *  Muchos releases no la marcan y lo dicen en el título. */
export function esForzada(t: Pista): boolean {
  return !!t.forced || /forced|forzad|signs/i.test(t.title || "");
}

/** Qué hacer con los subtítulos del archivo:
 *  - `completa`: la pista del idioma que traduce todo (usarla, no bajar nada).
 *  - `forzada`: solo hay forzada; bajar uno completo y, si no hay, usar esta.
 *  Ninguna de las dos → bajar uno externo. */
export function elegirPistaSub(
  subs: Pista[],
  idioma: string,
): { completa?: Pista; forzada?: Pista } {
  const delIdioma = subs.filter((t) => pistaDelIdioma(t, idioma));
  return {
    completa: delIdioma.find((t) => !esForzada(t)),
    forzada: delIdioma.find(esForzada),
  };
}
