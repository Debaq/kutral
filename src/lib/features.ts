// Flags de build. Sin persistencia ni UI: se cambian acá y se recompila.

/**
 * Player WEB (iframe playimdb / vidapi) — APAGADO.
 *
 * Dejó de funcionar: la fuente ya no sirve reproducción. Se esconde en vez de
 * borrarse porque el flujo completo sigue escrito y sirve si aparece un
 * proveedor equivalente. Para revivirlo: poner `true` acá y listo.
 *
 * Puertas que controla (las tres terminan en `startDiscover()` de
 * `routes/+page.svelte`, que monta el iframe):
 *   - PlayMenu       → "🌐 Ver en web" / "▶ Descubrir" / "Continuar" / "Empezar de nuevo"
 *   - SourcePicker   → botón "🌐 Ver en web" del pie
 *   - EpisodePicker  → botón "🌐 Ver en web" del encabezado
 *
 * Lo que NO toca (sigue vivo): el modo `discover` de `+page.svelte`, sus
 * handlers y el registro de progreso. Quedan inalcanzables desde la UI.
 */
export const WEB_PLAYER_ENABLED = false;
