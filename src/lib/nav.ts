// Navegación espacial por teclado/mando.
//
// Misma idea que la del catálogo (routes/+page.svelte): en vez de un orden
// lineal de tabulación, cada flecha busca el elemento `[data-nav]` visible que
// esté MÁS CERCA en esa dirección geométrica. Es lo que hace que un grid se
// recorra como un grid y no como una lista.
//
// `data-section` agrupa: primero se buscan candidatos dentro de la misma
// sección (el foco no se escapa de una fila de filtros al primer ArrowRight) y
// solo si no hay ninguno se permite saltar a otra.
//
// La home tiene su propia copia con reglas cautivas propias (el panel de info
// no sale verticalmente, ArrowDown en la galería dispara la carga de más
// páginas). Esta versión es la genérica, sin esos casos.

export type Dir = "up" | "down" | "left" | "right";

function seccionDe(el: HTMLElement | null): string | null {
  let nodo: HTMLElement | null = el;
  while (nodo && nodo !== document.body) {
    const s = nodo.dataset?.section;
    if (s) return s;
    nodo = nodo.parentElement;
  }
  return null;
}

// Distancia dirigida: manda el avance en el eje de la flecha, y la desviación
// en el otro eje penaliza (x1.4) para que no salte en diagonal si hay algo
// derecho al frente.
function mejor(actual: HTMLElement, candidatos: HTMLElement[], dir: Dir): HTMLElement | null {
  const r = actual.getBoundingClientRect();
  const cx = r.left + r.width / 2;
  const cy = r.top + r.height / 2;
  let elegido: HTMLElement | null = null;
  let mejorDist = Infinity;
  for (const el of candidatos) {
    if (el === actual) continue;
    const er = el.getBoundingClientRect();
    const dx = er.left + er.width / 2 - cx;
    const dy = er.top + er.height / 2 - cy;
    let principal = 0;
    let lateral = 0;
    let sirve = false;
    if (dir === "right") {
      sirve = dx > 6;
      principal = dx;
      lateral = Math.abs(dy);
    } else if (dir === "left") {
      sirve = dx < -6;
      principal = -dx;
      lateral = Math.abs(dy);
    } else if (dir === "down") {
      sirve = dy > 6;
      principal = dy;
      lateral = Math.abs(dx);
    } else {
      sirve = dy < -6;
      principal = -dy;
      lateral = Math.abs(dx);
    }
    if (!sirve) continue;
    // ¿Está al frente? Si los rectángulos se cruzan en el eje perpendicular, el
    // desvío casi no cuenta; si no, castiga fuerte. Con un factor fijo, un
    // elemento lejano pero bien centrado le ganaba a uno pegado y corrido.
    const alFrente =
      dir === "left" || dir === "right"
        ? er.bottom > r.top + 6 && er.top < r.bottom - 6
        : er.right > r.left + 6 && er.left < r.right - 6;
    const dist = principal + lateral * (alFrente ? 0.2 : 2.5);
    if (dist < mejorDist) {
      mejorDist = dist;
      elegido = el;
    }
  }
  return elegido;
}

/**
 * Mueve el foco en la dirección pedida dentro de `raiz` (por defecto, todo el
 * documento). Devuelve true si el foco se movió.
 */
export function navegar(dir: Dir, raiz: ParentNode = document): boolean {
  const todos = Array.from(
    raiz.querySelectorAll<HTMLElement>("[data-nav]:not([disabled])"),
  ).filter((el) => el.offsetParent !== null);
  if (!todos.length) return false;
  const actual = document.activeElement as HTMLElement | null;
  if (!actual || !actual.matches?.("[data-nav]")) {
    todos[0].focus();
    todos[0].scrollIntoView({ block: "nearest" });
    return true;
  }
  const seccion = seccionDe(actual);
  let destino: HTMLElement | null = null;
  if (seccion) {
    destino = mejor(
      actual,
      todos.filter((el) => seccionDe(el) === seccion),
      dir,
    );
  }
  if (!destino) destino = mejor(actual, todos, dir);
  if (!destino) return false;
  destino.focus({ preventScroll: false });
  destino.scrollIntoView({ block: "nearest", behavior: "smooth" });
  return true;
}

/** Enfoca el primer `[data-nav]` visible de `raiz`. */
export function enfocarPrimero(raiz: ParentNode = document): void {
  const el = Array.from(raiz.querySelectorAll<HTMLElement>("[data-nav]:not([disabled])")).find(
    (n) => n.offsetParent !== null,
  );
  el?.focus();
}
