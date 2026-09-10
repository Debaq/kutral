// Estado del asistente inicial. Vive en localStorage como el resto de la
// config del frontend.
//
// `onboarding_done` se marca cuando el usuario termina O salta el asistente:
// en los dos casos ya tomó la decisión y no queremos volver a interrumpirlo.
// Para repetirlo está el botón de Configuración, que llama a `resetOnboarding`.

const CLAVE = "onboarding_done";

export function onboardingHecho(): boolean {
	if (typeof localStorage === "undefined") return true;
	return localStorage.getItem(CLAVE) === "1";
}

export function marcarOnboarding() {
	if (typeof localStorage === "undefined") return;
	localStorage.setItem(CLAVE, "1");
}

export function resetOnboarding() {
	if (typeof localStorage === "undefined") return;
	localStorage.removeItem(CLAVE);
}

/**
 * ¿Falta lo mínimo para que la app sirva de algo?
 *
 * Catálogo: sin key de TMDb no cargan Pelis ni Series. Anime, IPTV y Juegos sí,
 * así que no es un bloqueo total, pero es medio Kütral apagado.
 *
 * Reproducción: hace falta un debrid vinculado o la descarga local activada.
 * Sin ninguna de las dos no hay forma de ver nada.
 */
export function faltaLoMinimo(opts: {
	tmdbKey: string;
	rdLinked: boolean;
	torrentLocal: boolean;
}): boolean {
	const sinCatalogo = !opts.tmdbKey.trim();
	const sinReproduccion = !opts.rdLinked && !opts.torrentLocal;
	return sinCatalogo || sinReproduccion;
}
