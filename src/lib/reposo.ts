import { invoke } from "@tauri-apps/api/core";

// Pide al escritorio que no se duerma mientras `fuente` reproduce. mpv lo
// hace solo desde el backend; esto es para lo que corre en el webview.
export function inhibirReposo(fuente: string, on: boolean) {
  void invoke("reposo_inhibir", { fuente, on }).catch((e) => {
    console.warn("[reposo]", e);
  });
}
