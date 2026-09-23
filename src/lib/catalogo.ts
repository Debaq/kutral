// Snapshot del catálogo del home para sobrevivir a salir de la ruta "/".
//
// El home es una ruta más: ir a /config, /vera o /iptv desmonta
// +page.svelte. Al volver, onMount llamaba a resetAndLoad() y todo empezaba
// de cero — página 1, foco en la primera card, scroll arriba y re-chequeo de
// disponibilidad y premios de cada título. Guardamos el estado al desmontar
// y lo restauramos al montar.
//
// A propósito NO usa runas: es un cofre, no un store reactivo. La página
// sigue siendo dueña de su $state; acá solo se copia al salir y se lee al
// entrar. Así no hay dos fuentes de verdad ni efectos cruzados.

export type CatalogTab = "movie" | "tv" | "anime";
export type CardStatus = "checking" | "ok" | "trailer" | "none";
export type AwardsSummary = { wins: number; nominations: number };

// Misma forma que ListItem en +page.svelte (TS es estructural: no hace falta
// que sea el mismo tipo nominal para asignar en los dos sentidos).
export type CatalogItem = {
  id: number;
  title?: string;
  name?: string;
  poster_path?: string;
  overview: string;
  vote_average: number;
  release_date?: string;
  first_air_date?: string;
};

export type CatalogSnapshot = {
  // Con qué key se armó. Si el user la cambió en /config, el catálogo viejo
  // no sirve y se descarta.
  apiKey: string;
  tab: CatalogTab;
  query: string;
  debouncedQ: string;
  page: number;
  totalPages: number;
  hasMore: boolean;
  sortId: string;
  seasonId: string;
  genres: { id: number; name: string }[];
  selectedGenres: number[];
  items: CatalogItem[];
  focusedIdx: number;
  scrollTop: number;
  statusMap: [number, CardStatus][];
  imdbIdMap: [number, string][];
  seasonsMap: [number, number][];
  awardsMap: [string, AwardsSummary][];
};

let snapshot: CatalogSnapshot | null = null;

export function guardarCatalogo(s: CatalogSnapshot) {
  snapshot = s;
}

// Devuelve el snapshot si sigue siendo usable. Sin items no hay nada que
// restaurar (mejor recargar que mostrar un grid vacío).
export function leerCatalogo(apiKey: string): CatalogSnapshot | null {
  const s = snapshot;
  if (!s) return null;
  if (s.apiKey !== apiKey) {
    snapshot = null;
    return null;
  }
  if (!s.items.length) return null;
  return s;
}

export function limpiarCatalogo() {
  snapshot = null;
}

// --- Filtros recordados entre sesiones -------------------------------------
//
// Uno por tab: el orden y la temporada de anime no tienen nada que ver con los
// de películas. Van a localStorage (no al snapshot de arriba) porque tienen que
// sobrevivir al cierre de la app, no solo a un cambio de ruta.
//
// Acá NO se valida contra las listas de opciones: eso lo hace la página, que es
// la que sabe qué órdenes existen en cada tab y qué temporadas de anime siguen
// vigentes (se calculan por fecha).

export type FiltrosGuardados = {
  sortId: string;
  seasonId: string;
  genres: number[];
};

const CLAVE_FILTROS = "kutral:filtros:";

export function leerFiltros(tab: CatalogTab): FiltrosGuardados | null {
  try {
    const raw = localStorage.getItem(CLAVE_FILTROS + tab);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<FiltrosGuardados>;
    return {
      sortId: typeof v.sortId === "string" ? v.sortId : "",
      seasonId: typeof v.seasonId === "string" ? v.seasonId : "",
      genres: Array.isArray(v.genres)
        ? v.genres.filter((g): g is number => typeof g === "number")
        : [],
    };
  } catch {
    return null;
  }
}

export function guardarFiltros(tab: CatalogTab, f: FiltrosGuardados) {
  try {
    localStorage.setItem(CLAVE_FILTROS + tab, JSON.stringify(f));
  } catch {
    /* localStorage lleno o bloqueado: los filtros no se recuerdan, nada más */
  }
}

// --- Primera página lista al arrancar --------------------------------------
//
// Stale-while-revalidate: al abrir la app se pinta la página 1 que quedó
// guardada y en paralelo se pide la de verdad. El catálogo aparece sin espera
// visible aunque la red esté lenta.
//
// Solo la primera página, y solo sin búsqueda: es lo que el user ve al abrir.

type PrimeraPagina = { firma: string; items: CatalogItem[]; ts: number };

const CLAVE_P1 = "kutral:pagina1";
// Más viejo que esto ya no se muestra: preferimos el "Cargando…" honesto a un
// catálogo de hace un mes.
const MAX_EDAD_P1_MS = 7 * 24 * 3600 * 1000;

export function guardarPrimeraPagina(firma: string, items: CatalogItem[]) {
  if (!items.length) return;
  try {
    const v: PrimeraPagina = { firma, items, ts: Date.now() };
    localStorage.setItem(CLAVE_P1, JSON.stringify(v));
  } catch {
    /* lleno o bloqueado: se carga de red como siempre */
  }
}

// Devuelve los items guardados solo si son de la MISMA consulta (tab, orden,
// temporada y géneros). Con otros filtros, mostrarlos sería mentir.
export function leerPrimeraPagina(firma: string): CatalogItem[] | null {
  try {
    const raw = localStorage.getItem(CLAVE_P1);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<PrimeraPagina>;
    if (v.firma !== firma || !Array.isArray(v.items) || !v.items.length) return null;
    if (typeof v.ts !== "number" || Date.now() - v.ts > MAX_EDAD_P1_MS) return null;
    return v.items as CatalogItem[];
  } catch {
    return null;
  }
}
