<script lang="ts">
  // Mando reutilizable — MISMO diseño que el que se sirve por la red
  // (remote.html): dos grips redondos a los lados (agarres), cintura con la
  // marca y Select/Start, hombros arriba. Interactivo: resalta lo pulsado en
  // vivo y permite clicar para reasignar. Índices estándar del Gamepad API.

  let {
    actionByBtn = {},
    pressed = [],
    selected = null,
    onpick = (_i: number) => {},
  }: {
    actionByBtn?: Record<number, string>;
    pressed?: number[];
    selected?: number | null;
    onpick?: (index: number) => void;
  } = $props();

  const on = (i: number) => pressed.includes(i);
  const lbl = (i: number) => actionByBtn[i] ?? "";
</script>

<div class="gamepad">
  <!-- Hombros (gatillos L/R) -->
  <button
    class="shoulder r1" class:press={on(4)} class:sel={selected === 4}
    title={lbl(4)} onclick={() => onpick(4)}>L</button>
  <button
    class="shoulder r2" class:press={on(5)} class:sel={selected === 5}
    title={lbl(5)} onclick={() => onpick(5)}>R</button>

  <!-- Grip izquierdo: cruceta -->
  <div class="grip">
    <div class="disc">
      <div class="dpad">
        <button class="up"    class:press={on(12)} class:sel={selected === 12} title={lbl(12)} onclick={() => onpick(12)}>▲</button>
        <button class="left"  class:press={on(14)} class:sel={selected === 14} title={lbl(14)} onclick={() => onpick(14)}>◀</button>
        <button class="mid" disabled tabindex="-1" aria-label="centro"></button>
        <button class="right" class:press={on(15)} class:sel={selected === 15} title={lbl(15)} onclick={() => onpick(15)}>▶</button>
        <button class="down"  class:press={on(13)} class:sel={selected === 13} title={lbl(13)} onclick={() => onpick(13)}>▼</button>
      </div>
    </div>
  </div>

  <!-- Cintura: marca + Select/Start -->
  <div class="waist">
    <span class="brand">Kütral</span>
    <div class="center-col">
      <button class="small-btn" class:press={on(8)} class:sel={selected === 8} title={lbl(8)} onclick={() => onpick(8)}>Select</button>
      <button class="small-btn" class:press={on(9)} class:sel={selected === 9} title={lbl(9)} onclick={() => onpick(9)}>Start</button>
    </div>
  </div>

  <!-- Grip derecho: A B X Y -->
  <div class="grip">
    <div class="disc">
      <div class="abxy">
        <button class="btn-x" class:press={on(2)} class:sel={selected === 2} title={lbl(2)} onclick={() => onpick(2)}>X</button>
        <button class="btn-y" class:press={on(3)} class:sel={selected === 3} title={lbl(3)} onclick={() => onpick(3)}>Y</button>
        <button class="btn-a" class:press={on(0)} class:sel={selected === 0} title={lbl(0)} onclick={() => onpick(0)}>A</button>
        <button class="btn-b" class:press={on(1)} class:sel={selected === 1} title={lbl(1)} onclick={() => onpick(1)}>B</button>
      </div>
    </div>
  </div>
</div>

<style>
  /* Paleta (skin por defecto del mando servido). */
  .gamepad {
    --bg: #0b0b0f;
    --panel: #15151c;
    --panel-2: #1c1c26;
    --border: #2a2a36;
    --text: #e6e6ec;
    --accent: #f5c518;
    --a: #c44b4b;
    --b: #f3c544;
    --x: #4a8ed8;
    --y: #5fb86b;
    width: 100%;
    max-width: 936px;
    margin: 14px auto;
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
    filter: drop-shadow(0 22px 28px rgba(0, 0, 0, 0.85));
  }
  button { font-family: inherit; }

  .grip {
    position: relative;
    width: clamp(220px, 30vw, 300px);
    aspect-ratio: 1 / 1;
    border-radius: 50%;
    background:
      radial-gradient(circle at 28% 22%, color-mix(in srgb, var(--panel-2) 70%, white 6%), transparent 60%),
      linear-gradient(155deg, var(--panel-2), var(--panel) 55%, color-mix(in srgb, var(--panel) 60%, black));
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    z-index: 1;
  }
  .waist {
    align-self: center;
    flex: 1 1 auto;
    height: 90%;
    min-height: 218px;
    margin: 0 -84px;
    background: linear-gradient(180deg, var(--panel-2), var(--panel));
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 14px 22px;
    z-index: 0;
    border-top: 1px solid color-mix(in srgb, var(--text) 10%, transparent);
    border-bottom: 1px solid rgba(0, 0, 0, 0.4);
  }
  .shoulder {
    position: absolute;
    top: -8px;
    appearance: none;
    border: 0;
    background: linear-gradient(180deg, var(--panel-2), var(--panel));
    color: var(--text);
    font-weight: 700;
    font-size: 13px;
    letter-spacing: 0.14em;
    padding: 8px 22px 14px;
    border-radius: 16px 16px 4px 4px;
    cursor: pointer;
    box-shadow: 0 5px 0 rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.14), inset 0 -2px 4px rgba(0, 0, 0, 0.4);
    transition: transform 80ms, box-shadow 80ms, color 80ms;
    min-width: 58px;
    z-index: 4;
  }
  .shoulder.r1 { left: 14%; }
  .shoulder.r2 { right: 14%; }
  .shoulder.press {
    transform: translateY(3px);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.55), inset 0 1px 3px rgba(0, 0, 0, 0.5);
    color: var(--accent);
  }
  .center-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .brand {
    font-size: 14px;
    font-weight: 800;
    letter-spacing: 0.32em;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--accent) 70%, transparent);
    text-shadow: 0 1px 0 rgba(0, 0, 0, 0.6), 0 -1px 0 rgba(255, 255, 255, 0.08);
    margin-bottom: 8px;
    user-select: none;
  }
  .small-btn {
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 6px 16px;
    border-radius: 999px;
    font-size: 10px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    cursor: pointer;
    box-shadow: 0 3px 0 rgba(0, 0, 0, 0.5), inset 0 1px 0 rgba(255, 255, 255, 0.06);
    transition: transform 80ms, box-shadow 80ms;
  }
  .small-btn.press {
    transform: translateY(2px);
    color: var(--accent);
    border-color: var(--accent);
  }
  .disc {
    position: relative;
    width: 78%;
    aspect-ratio: 1 / 1;
    border-radius: 50%;
    background: radial-gradient(circle at 30% 30%, color-mix(in srgb, var(--panel-2) 80%, black), #0a0a0e 78%);
    box-shadow: inset 0 4px 14px rgba(0, 0, 0, 0.75), 0 6px 16px rgba(0, 0, 0, 0.5);
    flex: 0 0 auto;
  }
  .dpad {
    position: absolute;
    inset: 0;
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    grid-template-rows: 1fr 1fr 1fr;
    padding: 22%;
  }
  .dpad button {
    background: var(--panel);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 16px;
    cursor: pointer;
    transition: transform 60ms, background 60ms;
  }
  .dpad button.press {
    transform: scale(0.92);
    background: var(--panel-2);
    color: var(--accent);
    border-color: var(--accent);
  }
  .dpad .up    { grid-column: 2; grid-row: 1; border-radius: 8px 8px 0 0; }
  .dpad .left  { grid-column: 1; grid-row: 2; border-radius: 8px 0 0 8px; }
  .dpad .mid   { grid-column: 2; grid-row: 2; background: var(--bg); border: 1px solid var(--border); cursor: default; }
  .dpad .right { grid-column: 3; grid-row: 2; border-radius: 0 8px 8px 0; }
  .dpad .down  { grid-column: 2; grid-row: 3; border-radius: 0 0 8px 8px; }
  .abxy {
    position: absolute;
    inset: 0;
  }
  .abxy button {
    position: absolute;
    width: 28%;
    aspect-ratio: 1 / 1;
    border-radius: 50%;
    border: 0;
    color: #fff;
    font-weight: 700;
    font-size: 18px;
    cursor: pointer;
    box-shadow: 0 4px 0 rgba(0, 0, 0, 0.45), inset 0 -3px 6px rgba(0, 0, 0, 0.35), inset 0 2px 4px rgba(255, 255, 255, 0.18);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: transform 60ms, box-shadow 60ms;
  }
  .abxy button.press {
    transform: translateY(3px);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.45), inset 0 -1px 3px rgba(0, 0, 0, 0.5);
  }
  .abxy .btn-x { background: var(--x); top: 8%;  left: 36%; }
  .abxy .btn-y { background: var(--y); top: 36%; left: 8%; }
  .abxy .btn-a { background: var(--a); top: 36%; right: 8%; }
  .abxy .btn-b { background: var(--b); top: 64%; left: 36%; color: #2a2a10; }

  /* Resaltado de "seleccionado" (asignando) — anillo dorado. */
  .sel {
    outline: 3px solid var(--accent) !important;
    outline-offset: 2px;
    animation: pulso 1s infinite;
  }
  @keyframes pulso {
    50% { opacity: 0.55; }
  }
</style>
