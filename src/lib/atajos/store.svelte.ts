// Estado global de la ayuda de teclado.
// Cada ruta registra los atajos activos con setAtajos().
// El componente <Ayuda /> en +layout.svelte lo renderiza al apretar I.

export interface AtajoLinea {
  tecla: string;
  desc: string;
}

class AyudaStore {
  visible = $state(false);
  pantalla = $state<string>("");
  lineas = $state<AtajoLinea[]>([]);

  set(pantalla: string, lineas: AtajoLinea[]): void {
    this.pantalla = pantalla;
    this.lineas = lineas;
  }

  toggle(): void {
    this.visible = !this.visible;
  }

  close(): void {
    this.visible = false;
  }

  open(): void {
    this.visible = true;
  }
}

export const ayuda = new AyudaStore();
