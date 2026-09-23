// "Porque viste X": recomendaciones para la pantalla de fin del video.
// Anime → AniList (secuela primero); películas y series → TMDb.
import { invoke } from "@tauri-apps/api/core";
import { art } from "$lib/imagenes.svelte";
import { estadoDe, claveMedio } from "$lib/historial.svelte";
import type { ListItem, ListResp } from "$lib/tipos";

export type SugerenciaFin = { id: number; title: string; posterUrl: string | null; year: string };
export type SiguienteFin = { season: number; episode: number; nombre: string; stillUrl: string | null };

const MAX_SUGERENCIAS = 8;

type Pedido = {
  id: number;
  esAnime: boolean;
  mediaType: "movie" | "tv";
  apiKey: string;
  noDisponibles: Set<string>;
};

/**
 * Sugerencias ya filtradas, junto con el ítem completo de cada una: `pick()`
 * lo necesita para abrir la ficha.
 */
export async function sugerenciasPara(
  p: Pedido,
): Promise<{ sugerencias: SugerenciaFin[]; items: Map<number, ListItem> }> {
  const vacio = { sugerencias: [], items: new Map<number, ListItem>() };
  try {
    let items: ListItem[] = [];
    if (p.esAnime) {
      const r = await invoke<ListResp>("anilist_relacionados", { id: p.id });
      items = r.results ?? [];
    } else {
      if (!p.apiKey) return vacio;
      items = await recomendacionesTmdb(p);
    }
    const elegidas = await filtrarSugerencias(items, p);
    return {
      sugerencias: elegidas.map(aSugerencia),
      items: new Map(elegidas.map((it) => [it.id, it])),
    };
  } catch (e) {
    console.warn("[fin] sugerencias", e);
    return vacio;
  }
}

async function recomendacionesTmdb(p: Pedido): Promise<ListItem[]> {
  const pedir = async (kind: "recommendations" | "similar") => {
    const r = await invoke<ListResp>("tmdb_recommendations", {
      mediaType: p.mediaType,
      id: p.id,
      page: 1,
      apiKey: p.apiKey,
      kind,
    });
    return r.results ?? [];
  };
  const recs = await pedir("recommendations");
  // TMDb devuelve pocas (o ninguna) recomendación en títulos de nicho:
  // "similar" es el respaldo, con el mismo shape.
  if (recs.length >= 4) return recs;
  const sim = await pedir("similar").catch(() => [] as ListItem[]);
  return [...recs, ...sim.filter((x) => !recs.some((y) => y.id === x.id))];
}

// Deja fuera lo que no sirve ofrecer: lo ya visto, lo marcado como no
// disponible y (en TMDb) lo que no tiene imdb, que es lo que necesitan los
// scrapers para encontrar fuentes.
async function filtrarSugerencias(items: ListItem[], p: Pedido): Promise<ListItem[]> {
  const out: ListItem[] = [];
  if (p.esAnime) {
    for (const it of items) {
      if (out.length >= MAX_SUGERENCIAS) break;
      if (estadoDe(claveMedio(null, it.id))?.visto) continue;
      out.push(it);
    }
    return out;
  }
  // TMDb: el imdb sale de item_status (una llamada por título). Se miran solo
  // las primeras candidatas, en paralelo, para no encadenar 20 llamadas.
  const candidatas = items.slice(0, MAX_SUGERENCIAS + 6);
  const estados = await Promise.all(
    candidatas.map((it) =>
      invoke<{ has_imdb: boolean; imdb_id: string | null }>("item_status", {
        mediaType: p.mediaType,
        id: it.id,
        apiKey: p.apiKey,
      }).catch(() => null),
    ),
  );
  for (let i = 0; i < candidatas.length && out.length < MAX_SUGERENCIAS; i++) {
    const st = estados[i];
    if (!st?.has_imdb || !st.imdb_id) continue; // sin imdb no hay fuentes
    if (p.noDisponibles.has(st.imdb_id)) continue;
    if (estadoDe(claveMedio(st.imdb_id))?.visto) continue;
    out.push(candidatas[i]);
  }
  return out;
}

function aSugerencia(it: ListItem): SugerenciaFin {
  const fecha = it.release_date || it.first_air_date || "";
  return {
    id: it.id,
    title: it.title || it.name || "",
    posterUrl: it.poster_path ? art(it.poster_path, "w342", 342) : null,
    year: fecha.slice(0, 4),
  };
}
