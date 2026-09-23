// Navegación espacial por teclado (y el control web, que manda las mismas teclas).
//
// En vez de un orden lineal de tabulación, cada flecha busca el elemento
// `[data-nav]` visible que esté MÁS CERCA en esa dirección geométrica. Es lo
// que hace que un grid se recorra como un grid y no como una lista.
//
// `data-section` agrupa: primero se buscan candidatos dentro de la misma
// sección (el foco no se escapa de una fila de filtros al primer ArrowRight) y
// solo si no hay ninguno se permite saltar a otra.
//
// La home y la IPTV arman su propio `navegar` con reglas cautivas propias (el
// panel de info no sale verticalmente, un dropdown abierto atrapa el foco…),
// pero usan la misma geometría: `mejor` y `seccionDe`.

export type Dir = "up" | "down" | "left" | "right";

/** Lo que importa de un rectángulo para medir distancias. */
export type Rect = Pick<DOMRect, "left" | "right" | "top" | "bottom" | "width" | "height">;

export function seccionDe(el: HTMLElement | null): string | null {
  let nodo: HTMLElement | null = el;
  while (nodo && nodo !== document.body) {
    const s = nodo.dataset?.section;
    if (s) return s;
    nodo = nodo.parentElement;
  }
  return null;
}

/**
 * Distancia dirigida de `r` a `er` yendo hacia `dir`, o null si `er` no está
 * en esa dirección. Manda el avance en el eje de la flecha; la desviación en
 * el otro eje casi no cuenta si los rectángulos se cruzan (está al frente) y
 * castiga fuerte si no. Con un factor fijo, un elemento lejano pero bien
 * centrado le ganaba a uno pegado y corrido: en una serie, bajar desde
 * Trailer se saltaba las temporadas y aterrizaba en el director.
 */
export function distancia(r: Rect, er: Rect, dir: Dir): number | null {
  const dx = er.left + er.width / 2 - (r.left + r.width / 2);
  const dy = er.top + er.height / 2 - (r.top + r.height / 2);
  let principal: number;
  let lateral: number;
  if (dir === "right") {
    if (dx <= 6) return null;
    principal = dx;
    lateral = Math.abs(dy);
  } else if (dir === "left") {
    if (dx >= -6) return null;
    principal = -dx;
    lateral = Math.abs(dy);
  } else if (dir === "down") {
    if (dy <= 6) return null;
    principal = dy;
    lateral = Math.abs(dx);
  } else {
    if (dy >= -6) return null;
    principal = -dy;
    lateral = Math.abs(dx);
  }
  const alFrente =
    dir === "left" || dir === "right"
      ? er.bottom > r.top + 6 && er.top < r.bottom - 6
      : er.right > r.left + 6 && er.left < r.right - 6;
  return principal + lateral * (alFrente ? 0.2 : 2.5);
}

/** El candidato más cercano a `actual` en la dirección `dir`. */
export function mejor(actual: HTMLElement, candidatos: HTMLElement[], dir: Dir): HTMLElement | null {
  const r = actual.getBoundingClientRect();
  let elegido: HTMLElement | null = null;
  let mejorDist = Infinity;
  for (const el of candidatos) {
    if (el === actual) continue;
    const d = distancia(r, el.getBoundingClientRect(), dir);
    if (d !== null && d < mejorDist) {
      mejorDist = d;
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
