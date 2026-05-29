import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ScreeningEvent = {
  imdbId: string;
  disponible: boolean;
  reason: string;
};

let unsubscribe: UnlistenFn | null = null;

export async function cargarNoDisponiblesIniciales(): Promise<string[]> {
  try {
    return await invoke<string[]>("screening_get_unavailable");
  } catch (e) {
    console.warn("[screening] cargar inicial fail:", e);
    return [];
  }
}

export async function suscribirScreening(
  cb: (e: ScreeningEvent) => void,
): Promise<() => void> {
  if (unsubscribe) unsubscribe();
  unsubscribe = await listen<ScreeningEvent>("screening-result", (e) =>
    cb(e.payload),
  );
  return () => {
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
  };
}

export async function encolarScreening(ids: string[]): Promise<void> {
  const limpios = ids.filter((s) => !!s && s.startsWith("tt"));
  if (!limpios.length) return;
  try {
    await invoke("screening_enqueue", { ids: limpios });
  } catch (e) {
    console.warn("[screening] encolar fail:", e);
  }
}
