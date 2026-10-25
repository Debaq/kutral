// Qué pista de subtítulos del archivo sirve para el idioma elegido en
// Configuración. Sin dependencias, para poder probarlo.

export type Pista = { id: number; lang: string; title: string; forced?: boolean };

// Código ISO 639-1 (el de Configuración) → cómo aparece en los contenedores:
// 639-1, 639-2 (B y T) y nombres en el título de la pista, que muchos
// releases usan en vez del código ("Latino", "English SDH").
const IDIOMAS: Record<string, { codigos: string[]; nombres: RegExp }> = {
  es: { codigos: ["es", "spa", "esp"], nombres: /espa[ñn]ol|spanish|latino|castellano|\blat\b/i },
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
export function pistaDelIdioma(t: Pick<Pista, "lang" | "title">, idioma: string): boolean {
  const def = IDIOMAS[idioma];
  if (!def) return false;
  const base = (t.lang || "").toLowerCase().trim().split(/[-_]/)[0];
  if (base && def.codigos.includes(base)) return true;
  return def.nombres.test(t.title || "");
}

/** Variante del español que prefiere la app: latino (Chile, Latinoamérica) o
 *  castellano (España). Los releases MULTI traen las dos, y el castellano
 *  suele venir primero. */
export type VarianteEs = "latino" | "castellano";

const LATINO = /latin|\blat\b|latam|m[eé]xic|419|hispanoam/i;
const CASTELLANO = /castellano|espa[ñn]a|spain|\bcast\b|\besp\b|europe/i;

function varianteDe(t: Pick<Pista, "lang" | "title">): VarianteEs | null {
  const lang = (t.lang || "").toLowerCase();
  if (/^es[-_](419|mx|la|ar|co|cl)/.test(lang)) return "latino";
  if (/^es[-_]es/.test(lang)) return "castellano";
  if (LATINO.test(t.title || "")) return "latino";
  if (CASTELLANO.test(t.title || "")) return "castellano";
  return null;
}

/**
 * Índice de la mejor pista de audio en `idioma` (-1 si no hay). Entre varias
 * del mismo idioma, en español gana la `variante` pedida aunque venga
 * después; las que no dicen cuál son, van en medio.
 */
export function mejorAudio(
  audios: Pick<Pista, "lang" | "title">[],
  idioma: string,
  variante?: VarianteEs,
): number {
  let mejor = -1;
  let puntos = -1;
  audios.forEach((t, i) => {
    if (!pistaDelIdioma(t, idioma)) return;
    let p = 1;
    if (idioma === "es" && variante) {
      const v = varianteDe(t);
      p = v === variante ? 2 : v === null ? 1 : 0;
    }
    if (p > puntos) {
      puntos = p;
      mejor = i;
    }
  });
  return mejor;
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
