import { getOsInfo } from "$lib/os";

export type Lang = "es-CL" | "es-ES" | "en-US";
export type ModeOverride = "auto" | "kiosk" | "desktop";

export const LANGS: { id: Lang; label: string }[] = [
	{ id: "es-CL", label: "Español (Chile)" },
	{ id: "es-ES", label: "Español (España)" },
	{ id: "en-US", label: "English (US)" },
];

export const config = $state({
	lang: "es-CL" as Lang,
	tmdbKey: "",
	rdKey: "",
	modeOverride: "auto" as ModeOverride,
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
	config.loaded = true;
}

export function saveConfig() {
	if (typeof localStorage === "undefined") return;
	localStorage.setItem("app_lang", config.lang);
	localStorage.setItem("tmdb_key", config.tmdbKey);
	localStorage.setItem("realdebrid_key", config.rdKey);
	localStorage.setItem("kiosk_mode", config.modeOverride);
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
