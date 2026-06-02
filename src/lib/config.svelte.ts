import { getOsInfo } from "$lib/os";

export type Lang = "es-CL" | "es-ES" | "en-US";
export type ModeOverride = "auto" | "kiosk" | "desktop";

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
export const GAME_REGIONS: { id: string; label: string; tags: string[] }[] = [
	{ id: "usa", label: "USA", tags: ["USA"] },
	{ id: "eu", label: "Europa", tags: ["Europe"] },
	{ id: "jp", label: "Japón", tags: ["Japan"] },
	{ id: "en", label: "Inglés / Mundial", tags: ["World", "(En", ",En"] },
	{ id: "latam", label: "Latinoamérica", tags: ["Latin America", "Brazil", "Spain"] },
];

const GAME_REGIONS_DEFAULT = ["eu", "usa"];

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

// Todos los tags conocidos (para detectar si un nombre trae región).
const ALL_REGION_TAGS = GAME_REGIONS.flatMap((r) => r.tags);

// ¿El nombre del juego cae en alguna región seleccionada? Los sin región
// reconocible (homebrew, protos) se muestran siempre.
export function nameInRegions(name: string, regions: string[]): boolean {
	if (!regions.length) return true;
	const hasKnown = ALL_REGION_TAGS.some((t) => name.includes(t));
	if (!hasKnown) return true;
	return regions.some((id) => {
		const r = GAME_REGIONS.find((x) => x.id === id);
		return r ? r.tags.some((t) => name.includes(t)) : false;
	});
}

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
	rdKey: "",
	omdbKey: "",
	modeOverride: "auto" as ModeOverride,
	screeningConcurrency: SCREENING_DEFAULT,
	// Subtítulos: idioma preferido (ISO 639-1) y API key de Wyzie para buscar.
	// Sin wyzieKey: el player intenta auto-buscar en OpenSubtitles (cuota limitada).
	subsLang: "es",
	wyzieKey: "",
	// Tamaño del subtítulo en el player. 50–200% (100 = base). El valor se
	// inyecta al iframe via postMessage STORAGE_INIT como playerSubStyle.
	subSize: 100,
	// Servidor web (control remoto). Si webAutoStart=true se levanta al iniciar
	// la app sobre webPort. Permite usar el celular como mando sin abrir el panel.
	webAutoStart: false,
	webPort: 8080,
	// Horas tras las cuales auto-eliminar torrents de la lista RD. 0 = nunca.
	rdCleanupHours: 0,
	// Regiones de ROM aceptadas en Juegos (ids de GAME_REGIONS).
	gameRegions: [...GAME_REGIONS_DEFAULT] as string[],
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
	config.rdKey = localStorage.getItem("realdebrid_key") || "";
	config.omdbKey = localStorage.getItem("omdb_key") || "";
	const m = (localStorage.getItem("kiosk_mode") || "auto") as ModeOverride;
	config.modeOverride = ["auto", "kiosk", "desktop"].includes(m) ? m : "auto";
	const sc = parseInt(localStorage.getItem("screening_concurrency") || "", 10);
	config.screeningConcurrency = clampConcurrency(sc);
	const sl = localStorage.getItem("subs_lang") || "es";
	config.subsLang = SUB_LANGS.some((l) => l.id === sl) ? sl : "es";
	config.wyzieKey = localStorage.getItem("wyzie_key") || "";
	const ss = parseInt(localStorage.getItem("sub_size") || "", 10);
	config.subSize = Number.isFinite(ss)
		? Math.min(200, Math.max(50, ss))
		: 100;
	config.webAutoStart = localStorage.getItem("web_autostart") === "1";
	const wp = parseInt(localStorage.getItem("web_port") || "", 10);
	config.webPort = Number.isFinite(wp) && wp >= 1024 && wp <= 65535 ? wp : 8080;
	const rch = parseInt(localStorage.getItem("rd_cleanup_hours") || "", 10);
	config.rdCleanupHours = RD_CLEANUP_OPTIONS.some((o) => o.h === rch) ? rch : 0;
	const gr = localStorage.getItem("game_regions");
	if (gr !== null) {
		const ids = gr.split(",").filter((x) => GAME_REGIONS.some((r) => r.id === x));
		config.gameRegions = ids;
	} else {
		config.gameRegions = [...GAME_REGIONS_DEFAULT];
	}
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
	localStorage.setItem("realdebrid_key", config.rdKey);
	localStorage.setItem("omdb_key", config.omdbKey.trim());
	localStorage.setItem("kiosk_mode", config.modeOverride);
	localStorage.setItem(
		"screening_concurrency",
		String(clampConcurrency(config.screeningConcurrency)),
	);
	localStorage.setItem("subs_lang", config.subsLang);
	localStorage.setItem("wyzie_key", config.wyzieKey.trim());
	localStorage.setItem("sub_size", String(config.subSize));
	localStorage.setItem("web_autostart", config.webAutoStart ? "1" : "0");
	localStorage.setItem("web_port", String(config.webPort));
	localStorage.setItem("rd_cleanup_hours", String(config.rdCleanupHours));
	localStorage.setItem("game_regions", config.gameRegions.join(","));
	const listas = (config.iptvLists.length ? config.iptvLists : IPTV_DEFAULT_LISTS)
		.filter((l) => l.url.trim())
		.map((l) => ({ name: (l.name || "Lista").trim(), url: l.url.trim() }));
	localStorage.setItem("iptv_lists", JSON.stringify(listas));
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
