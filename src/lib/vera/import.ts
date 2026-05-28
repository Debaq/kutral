import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type MediaType = "movie" | "tv";

export interface ImportProgress {
  page: number;
  total_pages: number;
  item_in_page: number;
  page_items: number;
  inserted: number;
  skipped: number;
  current: string;
}

export interface ImportSummary {
  inserted: number;
  skipped: number;
}

export interface CatalogCount {
  total: number;
  movies: number;
  tv: number;
}

export async function importCatalog(
  apiKey: string,
  mediaType: MediaType,
  pages: number,
  watchRegion: string,
  onProgress?: (p: ImportProgress) => void,
): Promise<ImportSummary> {
  let unlisten: UnlistenFn | null = null;
  if (onProgress) {
    unlisten = await listen<ImportProgress>("vera:import:progress", (e) => onProgress(e.payload));
  }
  try {
    return await invoke<ImportSummary>("vera_import_catalog", {
      apiKey,
      mediaType,
      pages,
      watchRegion,
    });
  } finally {
    if (unlisten) unlisten();
  }
}

export async function catalogCount(): Promise<CatalogCount> {
  return invoke<CatalogCount>("vera_catalog_count");
}
