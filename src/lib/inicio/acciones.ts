// Acciones `use:` compartidas por las piezas del catálogo.

/** Enfoca el primer botón navegable al montar (menús y modales). */
export function autofocusFirst(node: HTMLElement) {
  setTimeout(() => {
    const btn = node.querySelector<HTMLElement>(".btn-primary, [data-nav]");
    btn?.focus();
  }, 30);
  return {};
}
