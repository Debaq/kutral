// El perfil con el que se está usando la app en este equipo. Lo leen las
// consultas de historial, favoritos y listas (cada fila lleva su `perfil`).
//
// Módulo sin dependencias a propósito: historial y listas lo importan, y
// perfiles.svelte.ts (que importa a esos dos para recargarlos al cambiar de
// perfil) también. El estado reactivo para la UI vive en perfiles.svelte.ts.

export const PRINCIPAL = "principal";

const CLAVE = "kutral_perfil";

function leer(): string {
  try {
    return localStorage.getItem(CLAVE) || PRINCIPAL;
  } catch {
    return PRINCIPAL; // sin localStorage (tests en Node)
  }
}

let actual = leer();

export function perfilActual(): string {
  return actual;
}

export function fijarPerfil(id: string): void {
  actual = id;
  try {
    localStorage.setItem(CLAVE, id);
  } catch {
    /* sin localStorage: dura lo que dure la sesión */
  }
}
