import { getOsInfo } from "$lib/os";
import { invoke } from "@tauri-apps/api/core";

export type Lang = "es-CL" | "es-ES" | "en-US";
export type ModeOverride = "auto" | "kiosk" | "desktop";
// "dub" = doblado al español; "sub" = original subtitulado en español.
export type SubMode = "dub" | "sub";
// Cómo se elige la fuente al reproducir con debrid:
// "auto" = reproduce la mejor al instante; "manual" = siempre muestra el selector.
export type SourceSelect = "auto" | "manual";

export const LANGS: { id: Lang; label: string }[] = [
	{ id: "es-CL", label: "Español (Chile)" },
	{ id: "es-ES", label: "Español (España)" },
	{ id: "en-US", label: "English (US)" },
];

export const SCREENING_MIN = 1;
export const SCREENING_MAX = 4;
export const SCREENING_DEFAULT = 1;

function clampConcurrency(n: number): number {
	if (!Number.isFinite(n)) return SCREENING_DEFAULT;
	return Math.min(SCREENING_MAX, Math.max(SCREENING_MIN, Math.round(n)));
}

// Auto-limpieza de torrents en la cuenta RD. 0 = nunca. Borra de la lista los
// más viejos que N horas (no afecta reproducción ni la caché de RD).
export const RD_CLEANUP_OPTIONS: { h: number; label: string }[] = [
	{ h: 0, label: "Nunca" },
	{ h: 12, label: "12 horas" },
	{ h: 24, label: "24 horas" },
	{ h: 48, label: "48 horas" },
	{ h: 72, label: "72 horas" },
];

// Regiones de ROM aceptadas en Juegos. El nombre No-Intro trae la región
// entre paréntesis: "Juego (USA)", "(Europe)", "(Japan)", "(World)"…
// Playlist M3U por defecto (iptv-org, libre y mantenida por la comunidad).
export const IPTV_DEFAULT_URL = "https://iptv-org.github.io/iptv/index.m3u";

export type IptvList = { name: string; url: string };
export const IPTV_DEFAULT_LIST: IptvList = {
	name: "iptv-org (global)",
	url: IPTV_DEFAULT_URL,
};
// Lista en español (idioma) de iptv-org: ~1600 canales hispanohablantes de
// todos los países, CORS habilitado (github.io).
export const IPTV_ES_LIST: IptvList = {
	name: "Español (todas)",
	url: "https://iptv-org.github.io/iptv/languages/spa.m3u",
};
// Por defecto: español primero, global de respaldo.
export const IPTV_DEFAULT_LISTS: IptvList[] = [
	{ ...IPTV_ES_LIST },
	{ ...IPTV_DEFAULT_LIST },
];

// Tope de calidad para la descarga local. Con debrid da igual el peso (RD sirve
// a velocidad de fibra), pero bajando del swarm hay que sostener el bitrate del
// archivo o el video se corta: un 4K de 60 GB en 2 h pide ~67 Mbps constantes,
// un 1080p normal de 4 GB pide ~4,5 Mbps. Por eso el plan B baja el techo.
export type TorrentQuality = "p2160" | "p1080" | "p720";
export const TORRENT_QUALITY_OPTIONS: {
	id: TorrentQuality;
	label: string;
	hint: string;
}[] = [
	{ id: "p720", label: "720p", hint: "Para internet lento. Arranca casi siempre." },
	{ id: "p1080", label: "1080p", hint: "Recomendado. Buen equilibrio." },
	{ id: "p2160", label: "4K", hint: "Solo con fibra y torrents muy compartidos." },
];

// Peso máximo del archivo aceptado para bajar en local, en GB.
// Cacheo de red durante la reproducción. Los tres valores van juntos porque
// mueven la misma palanca desde ángulos distintos: `wait` es cuántos segundos
// junta mpv antes de reanudar tras un corte (su default de 1s es el que hace
// que con red mala corte cada dos por tres), `readahead` cuánto video adelanta
// y `maxMb` el techo en bytes de ese adelanto.
export type CachePreset = {
	id: string;
	label: string;
	hint: string;
	wait: number;
	readahead: number;
	maxMb: number;
};
export const CACHE_PRESETS: CachePreset[] = [
	{
		id: "normal",
		label: "Normal",
		hint: "Red estable. Arranca rápido.",
		wait: 2,
		readahead: 20,
		maxMb: 150,
	},
	{
		id: "lenta",
		label: "Red lenta",
		hint: "Corta de a ratos. Junta más antes de seguir.",
		wait: 15,
		readahead: 60,
		maxMb: 400,
	},
	{
		id: "mala",
		label: "Red muy mala",
		hint: "Pausas largas pero pocas. Tarda más en arrancar.",
		wait: 40,
		readahead: 120,
		maxMb: 800,
	},
];
export const CACHE_WAIT_MAX_DEFAULT = 60;
export const CACHE_WAIT_MIN = 1;
export const CACHE_WAIT_MAX = 180;

export const TORRENT_MAX_GB_DEFAULT = 6;
// Descargas que corren a la vez. Más no es más rápido: el cliente reparte la
// conexión y los seeds entre todas, así que 12 capítulos en paralelo tardan lo
// mismo que de a dos... pero ninguno queda listo hasta el final.
export const TORRENT_PARALELAS_DEFAULT = 2;
export const TORRENT_PARALELAS_MIN = 1;
export const TORRENT_PARALELAS_MAX = 5;
export const TORRENT_MAX_GB_MIN = 1;
export const TORRENT_MAX_GB_MAX = 80;

// Buffer inicial (MB) del torrent local antes de lanzar el reproductor. Más
// buffer = arranque más lento pero menos cortes si el swarm es irregular.
export const TORRENT_BUFFER_DEFAULT = 24;
export const TORRENT_BUFFER_MIN = 8;
export const TORRENT_BUFFER_MAX = 200;

// Presets del buffer. Los MB no le dicen nada a nadie; el arranque y los
// cortes sí. El campo numérico sigue ahí para afinar.
export const TORRENT_BUFFER_OPTIONS: { mb: number; label: string; hint: string }[] = [
	{ mb: 12, label: "Arranque rápido", hint: "Empieza antes. Se corta si hay poca gente compartiendo." },
	{ mb: 24, label: "Equilibrado", hint: "Recomendado." },
	{ mb: 64, label: "Sin cortes", hint: "Tarda más en empezar y aguanta los bajones." },
];

export const SUB_LANGS: { id: string; label: string }[] = [
	{ id: "es", label: "Español" },
	{ id: "en", label: "English" },
	{ id: "fr", label: "Français" },
	{ id: "pt", label: "Português" },
	{ id: "it", label: "Italiano" },
	{ id: "de", label: "Deutsch" },
	{ id: "ja", label: "日本語" },
	{ id: "ko", label: "한국어" },
	{ id: "zh", label: "中文" },
	{ id: "off", label: "Sin subtítulos" },
];

export const config = $state({
	lang: "es-CL" as Lang,
	tmdbKey: "",
	// Estado de vínculo Real-Debrid. El token NO vive aquí ni en localStorage:
	// está en el store 0600 del backend. Esto es solo el indicador booleano.
	rdLinked: false,
	omdbKey: "",
	modeOverride: "auto" as ModeOverride,
	// Modo escritorio: abrir en pantalla completa. Lo cambia el botón ⛶ del
	// header o /config, y se recuerda entre sesiones. En kiosko no aplica
	// (siempre es pantalla completa).
	pantallaCompleta: false,
	screeningConcurrency: SCREENING_DEFAULT,
	// Subtítulos: idioma preferido (ISO 639-1) y API key de Wyzie para buscar.
	// Sin wyzieKey: el player intenta auto-buscar en OpenSubtitles (cuota limitada).
	subsLang: "es",
	wyzieKey: "",
	// Preferencia de reproducción: "dub" = audio doblado al español primero;
	// "sub" = audio original (VO) + subtítulos en español. Ordena las fuentes y
	// decide qué pista auto-seleccionar en mpv.
	subMode: "dub" as SubMode,
	// Selección de fuente al "Ver con debrid": "auto" reproduce la mejor al
	// instante; "manual" siempre muestra el selector para elegir.
	sourceSelect: "auto" as SourceSelect,
	// Verificación REAL de pistas antes de reproducir: ffprobe abre el header
	// del torrent ya resuelto (sin bajarlo) y confirma si trae audio/subtítulos
	// en español. Si no trae, salta a la siguiente fuente. Cuesta unos segundos
	// extra por fuente inspeccionada. Requiere ffprobe instalado.
	verifyEsTracks: false,
	// Pista de texto (nombre de release/grupo, ej. "fullscrabe") que sube esa
	// fuente al tope del orden. En modo auto, es la que se reproduce. Vacío = off.
	preferredSourceMovie: "",
	preferredSourceSeries: "",
	preferredSourceAnime: "",
	// Lista negra por tipo (ej. "CAM", "HDTS"): si el nombre del release o el
	// proveedor la contiene, esa fuente se HUNDE al fondo del orden (nunca se
	// auto-reproduce salvo que no haya otra). Varias por coma. Vacío = off.
	blockedSourceMovie: "",
	blockedSourceSeries: "",
	blockedSourceAnime: "",
	// Estado de la cuenta OpenSubtitles (subs externos, 20/día con cuenta). El
	// token NO vive aquí: está en el store 0600 del backend. Solo el indicador.
	osLinked: false,
	osUser: "",
	osHasKey: false,
	// Tamaño del subtítulo en el player. 50–200% (100 = base). El valor se
	// inyecta al iframe via postMessage STORAGE_INIT como playerSubStyle.
	subSize: 100,
	// Servidor web (control remoto). Si webAutoStart=true se levanta al iniciar
	// la app sobre webPort. Permite usar el celular como mando sin abrir el panel.
	webAutoStart: false,
	webPort: 8080,
	// Horas tras las cuales auto-eliminar torrents de la lista RD. 0 = nunca.
	rdCleanupHours: 0,
	// Bajar el torrent con el cliente local y reproducirlo mientras se descarga.
	// Con debrid es el plan B del 451/DMCA; sin debrid vinculado es la única vía
	// de reproducción. OPT-IN en los dos casos: sin el debrid de intermediario
	// tu IP queda expuesta en el swarm.
	torrentLocal: false,
	// MB a bufferear antes de abrir el reproductor con el torrent local.
	torrentBufferMb: TORRENT_BUFFER_DEFAULT,
	// Techo de calidad y peso SOLO para la descarga local. Con debrid no aplica:
	// ahí el peso no cuesta nada porque no lo bajas tú.
	torrentMaxQuality: "p1080" as TorrentQuality,
	torrentMaxGb: TORRENT_MAX_GB_DEFAULT,
	// Cuántas descargas de la cola corren a la vez (ver colaDescargas).
	torrentMaxParalelas: TORRENT_PARALELAS_DEFAULT,
	// Cacheo de red: preset base, espera efectiva (la que puede subir sola) y
	// tope de la escalada automática.
	cachePreset: "normal",
	cacheWait: 2,
	cacheAuto: true,
	cacheWaitMax: CACHE_WAIT_MAX_DEFAULT,
	// Carpeta de descarga. Vacío = la que propone el sistema (Descargas/Kutral).
	torrentDir: "",
	// IPTV: varias playlists M3U que alimentan /iptv. Editables en config.
	iptvLists: IPTV_DEFAULT_LISTS.map((l) => ({ ...l })) as IptvList[],
	loaded: false,
	detectedKutral: false,
});

export function loadConfig() {
	if (typeof localStorage === "undefined") return;
	const lang = localStorage.getItem("app_lang") as Lang | null;
	if (lang && LANGS.some((l) => l.id === lang)) config.lang = lang;
	config.tmdbKey = localStorage.getItem("tmdb_key") || "";
	config.omdbKey = localStorage.getItem("omdb_key") || "";
	const m = (localStorage.getItem("kiosk_mode") || "auto") as ModeOverride;
	config.modeOverride = ["auto", "kiosk", "desktop"].includes(m) ? m : "auto";
	config.pantallaCompleta = localStorage.getItem("pantalla_completa") === "1";
	const sc = parseInt(localStorage.getItem("screening_concurrency") || "", 10);
	config.screeningConcurrency = clampConcurrency(sc);
	const sl = localStorage.getItem("subs_lang") || "es";
	config.subsLang = SUB_LANGS.some((l) => l.id === sl) ? sl : "es";
	config.wyzieKey = localStorage.getItem("wyzie_key") || "";
	const sm = localStorage.getItem("sub_mode");
	config.subMode = sm === "sub" ? "sub" : "dub";
	config.verifyEsTracks = localStorage.getItem("verify_es_tracks") === "1";
	const ssel = localStorage.getItem("source_select");
	config.sourceSelect = ssel === "manual" ? "manual" : "auto";
	// Fuente preferida por tipo. Migra desde el viejo `preferred_source`
	// (campo único): si no existe el nuevo por tipo, hereda el legacy.
	const legacyPref = localStorage.getItem("preferred_source") || "";
	config.preferredSourceMovie = localStorage.getItem("preferred_source_movie") ?? legacyPref;
	config.preferredSourceSeries = localStorage.getItem("preferred_source_series") ?? legacyPref;
	config.preferredSourceAnime = localStorage.getItem("preferred_source_anime") ?? legacyPref;
	config.blockedSourceMovie = localStorage.getItem("blocked_source_movie") || "";
	config.blockedSourceSeries = localStorage.getItem("blocked_source_series") || "";
	config.blockedSourceAnime = localStorage.getItem("blocked_source_anime") || "";
	const ss = parseInt(localStorage.getItem("sub_size") || "", 10);
	config.subSize = Number.isFinite(ss)
		? Math.min(200, Math.max(50, ss))
		: 100;
	config.webAutoStart = localStorage.getItem("web_autostart") === "1";
	const wp = parseInt(localStorage.getItem("web_port") || "", 10);
	config.webPort = Number.isFinite(wp) && wp >= 1024 && wp <= 65535 ? wp : 8080;
	const rch = parseInt(localStorage.getItem("rd_cleanup_hours") || "", 10);
	config.rdCleanupHours = RD_CLEANUP_OPTIONS.some((o) => o.h === rch) ? rch : 0;
	config.torrentLocal = localStorage.getItem("torrent_local") === "1";
	const tbm = parseInt(localStorage.getItem("torrent_buffer_mb") || "", 10);
	config.torrentBufferMb = Number.isFinite(tbm)
		? Math.min(TORRENT_BUFFER_MAX, Math.max(TORRENT_BUFFER_MIN, tbm))
		: TORRENT_BUFFER_DEFAULT;
	const tq = localStorage.getItem("torrent_max_quality") as TorrentQuality | null;
	config.torrentMaxQuality = TORRENT_QUALITY_OPTIONS.some((o) => o.id === tq)
		? (tq as TorrentQuality)
		: "p1080";
	const tgb = parseFloat(localStorage.getItem("torrent_max_gb") || "");
	config.torrentMaxGb = Number.isFinite(tgb)
		? Math.min(TORRENT_MAX_GB_MAX, Math.max(TORRENT_MAX_GB_MIN, tgb))
		: TORRENT_MAX_GB_DEFAULT;
	const tpar = parseInt(localStorage.getItem("torrent_paralelas") || "", 10);
	config.torrentMaxParalelas = Number.isFinite(tpar)
		? Math.min(TORRENT_PARALELAS_MAX, Math.max(TORRENT_PARALELAS_MIN, tpar))
		: TORRENT_PARALELAS_DEFAULT;
	const cp = localStorage.getItem("cache_preset") || "normal";
	config.cachePreset = CACHE_PRESETS.some((x) => x.id === cp) ? cp : "normal";
	const cw = parseFloat(localStorage.getItem("cache_wait") || "");
	config.cacheWait = Number.isFinite(cw)
		? Math.min(CACHE_WAIT_MAX, Math.max(CACHE_WAIT_MIN, cw))
		: cachePresetActual().wait;
	config.cacheAuto = (localStorage.getItem("cache_auto") ?? "1") === "1";
	const cwm = parseFloat(localStorage.getItem("cache_wait_max") || "");
	config.cacheWaitMax = Number.isFinite(cwm)
		? Math.min(CACHE_WAIT_MAX, Math.max(CACHE_WAIT_MIN, cwm))
		: CACHE_WAIT_MAX_DEFAULT;
	config.torrentDir = localStorage.getItem("torrent_dir") || "";
	// IPTV: nuevo formato (lista de listas). Migra desde el viejo iptv_url.
	const rawLists = localStorage.getItem("iptv_lists");
	if (rawLists) {
		try {
			const arr = JSON.parse(rawLists);
			if (Array.isArray(arr)) {
				const limpio = arr
					.filter((x) => x && typeof x.url === "string" && x.url.trim())
					.map((x) => ({ name: String(x.name || "Lista").trim(), url: String(x.url).trim() }));
				config.iptvLists = limpio.length ? limpio : IPTV_DEFAULT_LISTS.map((l) => ({ ...l }));
			}
		} catch {
			config.iptvLists = IPTV_DEFAULT_LISTS.map((l) => ({ ...l }));
		}
	} else {
		const viejo = localStorage.getItem("iptv_url");
		config.iptvLists = viejo
			? [{ name: "Principal", url: viejo }, { ...IPTV_ES_LIST }]
			: IPTV_DEFAULT_LISTS.map((l) => ({ ...l }));
	}
	config.loaded = true;
}

export function saveConfig() {
	if (typeof localStorage === "undefined") return;
	localStorage.setItem("app_lang", config.lang);
	localStorage.setItem("tmdb_key", config.tmdbKey);
	localStorage.setItem("omdb_key", config.omdbKey.trim());
	localStorage.setItem("kiosk_mode", config.modeOverride);
	localStorage.setItem("pantalla_completa", config.pantallaCompleta ? "1" : "0");
	localStorage.setItem(
		"screening_concurrency",
		String(clampConcurrency(config.screeningConcurrency)),
	);
	localStorage.setItem("subs_lang", config.subsLang);
	localStorage.setItem("wyzie_key", config.wyzieKey.trim());
	localStorage.setItem("sub_mode", config.subMode);
	localStorage.setItem("source_select", config.sourceSelect);
	localStorage.setItem("verify_es_tracks", config.verifyEsTracks ? "1" : "0");
	localStorage.setItem("preferred_source_movie", config.preferredSourceMovie.trim());
	localStorage.setItem("preferred_source_series", config.preferredSourceSeries.trim());
	localStorage.setItem("preferred_source_anime", config.preferredSourceAnime.trim());
	localStorage.setItem("blocked_source_movie", config.blockedSourceMovie.trim());
	localStorage.setItem("blocked_source_series", config.blockedSourceSeries.trim());
	localStorage.setItem("blocked_source_anime", config.blockedSourceAnime.trim());
	localStorage.setItem("sub_size", String(config.subSize));
	localStorage.setItem("web_autostart", config.webAutoStart ? "1" : "0");
	localStorage.setItem("web_port", String(config.webPort));
	localStorage.setItem("rd_cleanup_hours", String(config.rdCleanupHours));
	localStorage.setItem("torrent_local", config.torrentLocal ? "1" : "0");
	localStorage.setItem("torrent_buffer_mb", String(config.torrentBufferMb));
	localStorage.setItem("torrent_max_quality", config.torrentMaxQuality);
	localStorage.setItem("torrent_max_gb", String(config.torrentMaxGb));
	localStorage.setItem("cache_preset", config.cachePreset);
	localStorage.setItem("cache_wait", String(config.cacheWait));
	localStorage.setItem("cache_auto", config.cacheAuto ? "1" : "0");
	localStorage.setItem("cache_wait_max", String(config.cacheWaitMax));
	localStorage.setItem("torrent_paralelas", String(config.torrentMaxParalelas));
	localStorage.setItem("torrent_dir", config.torrentDir.trim());
	const listas = (config.iptvLists.length ? config.iptvLists : IPTV_DEFAULT_LISTS)
		.filter((l) => l.url.trim())
		.map((l) => ({ name: (l.name || "Lista").trim(), url: l.url.trim() }));
	localStorage.setItem("iptv_lists", JSON.stringify(listas));
}

// Migra credenciales RD del viejo localStorage (texto plano) al store 0600 del
// backend, las borra de localStorage, y refresca el indicador rdLinked. Idempotente.
export async function initRd() {
	if (typeof localStorage !== "undefined") {
		const legacy = localStorage.getItem("realdebrid_key");
		if (legacy) {
			try {
				await invoke("rd_creds_save", {
					accessToken: legacy,
					refreshToken: localStorage.getItem("realdebrid_refresh") || "",
					clientId: localStorage.getItem("realdebrid_client_id") || "",
					clientSecret: localStorage.getItem("realdebrid_client_secret") || "",
				});
				localStorage.removeItem("realdebrid_key");
				localStorage.removeItem("realdebrid_refresh");
				localStorage.removeItem("realdebrid_client_id");
				localStorage.removeItem("realdebrid_client_secret");
			} catch {
				// Si el backend falla, dejamos el legacy donde está para reintentar.
			}
		}
	}
	await refreshRdLinked();
}

export async function refreshRdLinked() {
	try {
		config.rdLinked = await invoke<boolean>("rd_creds_status");
	} catch {
		config.rdLinked = false;
	}
}

type OsStatus = { linked: boolean; username: string; has_api_key: boolean };

// Refresca el indicador de cuenta OpenSubtitles (sin tocar el token).
export async function refreshOsStatus() {
	try {
		const s = await invoke<OsStatus>("os_status");
		config.osLinked = s.linked;
		config.osUser = s.username || "";
		config.osHasKey = s.has_api_key;
	} catch {
		config.osLinked = false;
		config.osUser = "";
		config.osHasKey = false;
	}
}

export async function initDetection() {
	try {
		const info = await getOsInfo();
		config.detectedKutral = info.is_kutral_os;
	} catch {
		config.detectedKutral = false;
	}
}

export function isKioskActive(): boolean {
	if (config.modeOverride === "kiosk") return true;
	if (config.modeOverride === "desktop") return false;
	return config.detectedKutral;
}

/** Preset de cacheo elegido (o el normal si el guardado ya no existe). */
export function cachePresetActual(): CachePreset {
	return CACHE_PRESETS.find((p) => p.id === config.cachePreset) ?? CACHE_PRESETS[0];
}
