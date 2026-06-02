// Controles — fuente única de verdad.
//
// Los TRES métodos de entrada convergen en el mismo set de teclas canónicas:
//   - Teclado físico  → la tecla nativa.
//   - Mando web        → remote.html postea la tecla → layout la dispatcha.
//   - Mando físico     → el bridge global (layout) dispatcha la misma tecla.
//
// Así toda la app reacciona a un solo conjunto de teclas, y aquí se documenta
// y se configura el mapeo del mando físico (lo único realmente remapeable sin
// tocar cada handler de ruta).

export type Accion = {
  id: string;
  label: string;
  key: string; // tecla canónica que dispatcha (KeyboardEvent.key)
  web: boolean; // ¿hay botón en el mando web?
  defBtn: number; // botón estándar del gamepad por defecto (-1 = ninguno)
  hint?: string; // qué hace
};

// Botones estándar del Gamepad API: 0=A 1=B 2=X 3=Y 4=LB 5=RB 8=Select 9=Start
// 12=↑ 13=↓ 14=← 15=→.
export const ACCIONES: Accion[] = [
  { id: "up", label: "Arriba", key: "ArrowUp", web: true, defBtn: 12 },
  { id: "down", label: "Abajo", key: "ArrowDown", web: true, defBtn: 13 },
  { id: "left", label: "Izquierda", key: "ArrowLeft", web: true, defBtn: 14 },
  { id: "right", label: "Derecha", key: "ArrowRight", web: true, defBtn: 15 },
  { id: "accept", label: "Aceptar / Jugar", key: "Enter", web: true, defBtn: 0, hint: "Abre / Descubre" },
  { id: "back", label: "Volver", key: "Backspace", web: true, defBtn: 1, hint: "Cierra / atrás" },
  { id: "play", label: "Reproducir / Pausa", key: " ", web: true, defBtn: 2 },
  { id: "help", label: "Ayuda", key: "i", web: true, defBtn: 8, hint: "Muestra atajos" },
  { id: "menu", label: "Menú", key: "m", web: true, defBtn: 9 },
  { id: "prev", label: "Anterior (sistema/tab)", key: "[", web: false, defBtn: 4, hint: "LB" },
  { id: "next", label: "Siguiente (sistema/tab)", key: "]", web: false, defBtn: 5, hint: "RB" },
];

export type GamepadMap = Record<string, number>; // accionId → índice de botón

export function defaultGamepadMap(): GamepadMap {
  const m: GamepadMap = {};
  for (const a of ACCIONES) if (a.defBtn >= 0) m[a.id] = a.defBtn;
  return m;
}

export function loadGamepadMap(): GamepadMap {
  if (typeof localStorage === "undefined") return defaultGamepadMap();
  try {
    const raw = JSON.parse(localStorage.getItem("gamepad_map") || "null");
    if (raw && typeof raw === "object") return { ...defaultGamepadMap(), ...raw };
  } catch {}
  return defaultGamepadMap();
}

export function saveGamepadMap(m: GamepadMap): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem("gamepad_map", JSON.stringify(m));
}

// Captura: mientras /config prueba/remapea el mando, el bridge global no debe
// navegar (si no, pulsar B saldría de config). Singleton de módulo.
let _capture = false;
export function setGamepadCapture(v: boolean): void {
  _capture = v;
}
export function gamepadCaptured(): boolean {
  return _capture;
}

// Nombre legible de un botón estándar de gamepad.
export function nombreBoton(i: number): string {
  const n: Record<number, string> = {
    0: "A", 1: "B", 2: "X", 3: "Y", 4: "LB", 5: "RB", 6: "LT", 7: "RT",
    8: "Select", 9: "Start", 10: "L3", 11: "R3", 12: "D-pad ↑", 13: "D-pad ↓",
    14: "D-pad ←", 15: "D-pad →", 16: "Home",
  };
  return n[i] ?? `Botón ${i}`;
}
