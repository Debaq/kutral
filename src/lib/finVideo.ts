// Fin de reproducción: lo que manda el backend en "mpv:fin" y cómo se lee.
//
// mpv queda en idle tanto cuando el archivo TERMINA como cuando el stream se
// muere a mitad (debrid caído, torrent sin seeds). Se distinguen por la
// posición: sin esto, un corte en el minuto 12 abriría la pantalla de "qué ver
// después" de una película que nadie vio.

export type FinPayload = { pos: number; duration: number };

/** Umbral de "terminada": los créditos ya son parte del final. */
const FIN_DESDE = 0.9;

/** ¿Esto fue el final de verdad, y no un corte del stream? */
export function esFinReal(p: FinPayload | null | undefined): boolean {
  if (!p || !isFinite(p.pos) || !isFinite(p.duration)) return false;
  // Sin duración conocida no hay con qué comparar: se toma por terminado
  // (es lo que pasaba antes de medir, y quedarse colgado es peor).
  if (p.duration <= 0) return true;
  return p.pos / p.duration >= FIN_DESDE;
}
