// Premios (Wikidata) por imdb, para la píldora de la esquina del póster.
//
// Cola con concurrencia fija: el catálogo pide uno por card visible y sin
// tope eran decenas de requests a la vez. El mapa vive a nivel de módulo: lo
// que llega después de salir de la ruta no se pierde.
import { invoke } from "@tauri-apps/api/core";
import type { AwardsSummary } from "$lib/catalogo";

let awardsMap = $state<Map<string, AwardsSummary | "loading">>(new Map());
const awardsQueue: string[] = [];
let awardsActive = 0;
const AWARDS_CONCURRENCY = 3;

export function premioDe(imdb: string): AwardsSummary | "loading" | undefined {
  return awardsMap.get(imdb);
}

/** Solo los resueltos: los "loading" no tienen sentido fuera de esta sesión. */
export function premiosResueltos(): [string, AwardsSummary][] {
  return Array.from(awardsMap).filter(
    (e): e is [string, AwardsSummary] => e[1] !== "loading",
  );
}

/** Suma lo del snapshot sin pisar lo que ya se sabe (o se está pidiendo). */
export function restaurarPremios(entradas: [string, AwardsSummary][]) {
  const m = new Map(awardsMap);
  for (const [k, v] of entradas) if (!m.has(k)) m.set(k, v);
  awardsMap = m;
}

// Una sola píldora: "🏆 3 · 5 nom.". Dos emojis de medalla apilados eran
// indistinguibles a 10px en la esquina del póster — y el title= es tooltip
// de mouse, que en la tele no existe. Sin premios ganados no va el trofeo:
// mentiría. La palabra "nom." es lo que desambigua.
export function awardsLabel(aw: AwardsSummary): string {
  const partes: string[] = [];
  if (aw.wins > 0) partes.push(`🏆 ${aw.wins}`);
  if (aw.nominations > 0) partes.push(`${aw.nominations} nom.`);
  return partes.join(" · ");
}

export function awardsTitle(aw: AwardsSummary): string {
  const partes: string[] = [];
  if (aw.wins > 0) partes.push(`${aw.wins} ${aw.wins === 1 ? "premio" : "premios"}`);
  if (aw.nominations > 0) {
    partes.push(
      `${aw.nominations} ${aw.nominations === 1 ? "nominación" : "nominaciones"}`,
    );
  }
  return partes.join(" · ");
}

export function enqueueAwards(imdb: string) {
  if (!imdb || awardsMap.has(imdb)) return;
  const m = new Map(awardsMap);
  m.set(imdb, "loading");
  awardsMap = m;
  awardsQueue.push(imdb);
  pumpAwards();
}

function pumpAwards() {
  while (awardsActive < AWARDS_CONCURRENCY && awardsQueue.length > 0) {
    const imdb = awardsQueue.shift()!;
    awardsActive++;
    void (async () => {
      try {
        const s: AwardsSummary = await invoke("wikidata_awards", { imdbId: imdb });
        const m = new Map(awardsMap);
        m.set(imdb, s);
        awardsMap = m;
      } catch (e) {
        console.warn("[awards]", imdb, e);
        const m = new Map(awardsMap);
        m.set(imdb, { wins: 0, nominations: 0 });
        awardsMap = m;
      } finally {
        awardsActive--;
        pumpAwards();
      }
    })();
  }
}

// OMDb manda los premios solo en inglés: "Won 3 Oscars. 12 wins & 45
// nominations total". Se traduce frase por frase; lo que no calce con los
// formatos conocidos queda tal cual antes que inventar.
const PREMIOS_OMDB: [RegExp, string, string][] = [
  [/^Oscars?$/, "Óscar", "Óscar"],
  [/^(Primetime )?Emmys?$/, "Emmy", "Emmy"],
  [/^Golden Globes?$/, "Globo de Oro", "Globos de Oro"],
  [/^BAFTA( Film| TV)? Awards?$/, "BAFTA", "BAFTA"],
];

function nombrePremio(n: number, nombre: string): string {
  const p = PREMIOS_OMDB.find(([re]) => re.test(nombre));
  if (!p) return `${n} ${nombre}`;
  return `${n} ${n === 1 ? p[1] : p[2]}`;
}

function plural(n: number, uno: string, varios: string): string {
  return `${n} ${n === 1 ? uno : varios}`;
}

function fraseOmdbEs(f: string): string {
  let m = f.match(/^Won (\d+) (.+)$/);
  if (m) return `Ganó ${nombrePremio(+m[1], m[2])}`;
  m = f.match(/^Nominated for (\d+) (.+)$/);
  if (m) return `Nominada a ${nombrePremio(+m[1], m[2])}`;
  m = f.match(/^(\d+) wins? & (\d+) nominations?( total)?$/);
  if (m) {
    return `${plural(+m[1], "premio", "premios")} y ${plural(+m[2], "nominación", "nominaciones")}${m[3] ? " en total" : ""}`;
  }
  m = f.match(/^(\d+) wins?( total)?$/);
  if (m) return `${plural(+m[1], "premio", "premios")}${m[2] ? " en total" : ""}`;
  m = f.match(/^(\d+) nominations?( total)?$/);
  if (m) return `${plural(+m[1], "nominación", "nominaciones")}${m[2] ? " en total" : ""}`;
  return f;
}

export function premiosOmdbEs(s: string): string {
  return s
    .split(/\.\s*/)
    .map((f) => f.trim())
    .filter(Boolean)
    .map(fraseOmdbEs)
    .join(". ");
}
