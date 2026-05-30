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
	modeOverride: "auto" as ModeOverride,
	screeningConcurrency: SCREENING_DEFAULT,
	// Subtítulos: idioma preferido (ISO 639-1) y API key de Wyzie para buscar.
	// Sin wyzieKey: el player intenta auto-buscar en OpenSubtitles (cuota limitada).
	subsLang: "es",
	wyzieKey: "",
	// Tamaño del subtítulo en el player. 50–200% (100 = base). El valor se
	// inyecta al iframe via postMessage STORAGE_INIT como playerSubStyle.
	subSize: 100,
	loaded: false,
	detectedKutral: false,
});

export function loadConfig() {
	if (typeof localStorage === "undefined") return;
	const lang = localStorage.getItem("app_lang") as Lang | null;
	if (lang && LANGS.some((l) => l.id === lang)) config.lang = lang;
	config.tmdbKey = localStorage.getItem("tmdb_key") || "";
	config.rdKey = localStorage.getItem("realdebrid_key") || "";
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
	config.loaded = true;
}

export function saveConfig() {
	if (typeof localStorage === "undefined") return;
	localStorage.setItem("app_lang", config.lang);
	localStorage.setItem("tmdb_key", config.tmdbKey);
	localStorage.setItem("realdebrid_key", config.rdKey);
	localStorage.setItem("kiosk_mode", config.modeOverride);
	localStorage.setItem(
		"screening_concurrency",
		String(clampConcurrency(config.screeningConcurrency)),
	);
	localStorage.setItem("subs_lang", config.subsLang);
	localStorage.setItem("wyzie_key", config.wyzieKey.trim());
	localStorage.setItem("sub_size", String(config.subSize));
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
