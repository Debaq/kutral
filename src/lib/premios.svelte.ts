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
