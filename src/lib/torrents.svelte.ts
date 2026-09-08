// Cola de descargas del cliente torrent local (plan B ante el 451/DMCA de RD).
//
// El backend (src-tauri/src/torrent.rs) mantiene la sesión y sirve el archivo
// por HTTP en 127.0.0.1 mientras se baja. Acá vive el estado que ve la UI: la
// lista de descargas y el poll que la refresca. El poll solo corre si hay algo
// en la cola, así que en uso normal no cuesta nada.

import { invoke } from "@tauri-apps/api/core";
import { notify } from "$lib/notifStore.svelte";
import { config } from "$lib/config.svelte";

/** Carpeta elegida por el usuario, o null para que el backend use la suya. */
function dirElegida(): string | null {
	return config.torrentDir.trim() || null;
}

export type TorrentStatus = {
	id: number;
	title: string;
	name: string;
	stream_url: string;
	size_bytes: number;
	progress_bytes: number;
	pct: number;
	download_bps: number;
	upload_bps: number;
	peers: number;
	eta_secs: number | null;
	state: "initializing" | "live" | "paused" | "error";
	error: string | null;
	finished: boolean;
	paused: boolean;
	buffered_bytes: number;
	buffer_target: number;
	buffer_ready: boolean;
	added_at: number;
};

export type TorrentAdded = {
	id: number;
	file_id: number;
	name: string;
	size_bytes: number;
	stream_url: string;
};

export const torrents = $state({
	list: [] as TorrentStatus[],
	/** Se llenó al menos una vez (para no parpadear "sin descargas" al abrir). */
	loaded: false,
});

// ---- Comandos ------------------------------------------------------------

export function addTorrent(
	magnet: string,
	title: string,
	bufferMb: number,
): Promise<TorrentAdded> {
	return invoke<TorrentAdded>("torrent_add", {
		magnet,
		title,
		bufferMb,
		dir: dirElegida(),
	});
}

export function torrentStatus(id: number): Promise<TorrentStatus> {
	return invoke<TorrentStatus>("torrent_status", { id });
}

export function pauseTorrent(id: number): Promise<void> {
	return invoke("torrent_pause", { id });
}

export function resumeTorrent(id: number): Promise<void> {
	return invoke("torrent_resume", { id });
}

export function removeTorrent(id: number, deleteFiles: boolean): Promise<void> {
	return invoke("torrent_remove", { id, deleteFiles });
}

/** Carpeta que se usaría si el usuario no elige ninguna. */
export function torrentDefaultDir(): Promise<string> {
	return invoke<string>("torrent_default_dir");
}

/**
 * Comprueba que se pueda escribir en `dir` (la crea si falta). Devuelve la ruta
 * final; lanza con el motivo si no sirve.
 */
export function torrentCheckDir(dir: string): Promise<string> {
	return invoke<string>("torrent_check_dir", { dir: dir.trim() || null });
}

/**
 * Levanta la sesión torrent y restaura la cola persistida. Devuelve cuántas
 * descargas quedaron. Solo se llama si el usuario tiene activada la descarga
 * local: sin eso la app no abre ningún socket de BitTorrent.
 */
export function initTorrentSession(): Promise<number> {
	return invoke<number>("torrent_init", { dir: dirElegida() });
}

// ---- Poll de la cola -----------------------------------------------------

let timer: ReturnType<typeof setInterval> | null = null;
// Ids ya avisados como terminados: el aviso se da UNA vez por descarga.
const avisados = new Set<number>();

export async function refreshQueue(): Promise<void> {
	try {
		const list = await invoke<TorrentStatus[]>("torrent_list");
		for (const t of list) {
			if (t.finished && !avisados.has(t.id)) {
				avisados.add(t.id);
				notify("success", "Descarga lista", t.title || t.name);
			}
			if (t.state === "error" && !avisados.has(t.id)) {
				avisados.add(t.id);
				notify("error", "Descarga con error", t.error || t.title || t.name);
			}
		}
		torrents.list = list;
		torrents.loaded = true;
		// Nada pendiente: el poll se apaga solo hasta que se agregue algo.
		if (!list.some((t) => !t.finished && !t.paused)) stopQueuePoll();
	} catch {
		// La sesión torrent se crea perezosamente: si aún no existe, no hay cola.
		torrents.loaded = true;
	}
}

export function startQueuePoll(ms = 1500): void {
	void refreshQueue();
	if (timer !== null) return;
	timer = setInterval(() => void refreshQueue(), ms);
}

export function stopQueuePoll(): void {
	if (timer !== null) {
		clearInterval(timer);
		timer = null;
	}
}

/** ¿Hay descargas activas? (para el badge del encabezado) */
export function activeCount(): number {
	return torrents.list.filter((t) => !t.finished && t.state !== "error").length;
}

// ---- Formato -------------------------------------------------------------

export function fmtBytes(n: number): string {
	if (!n) return "0 B";
	const u = ["B", "KB", "MB", "GB", "TB"];
	const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
	return `${(n / 1024 ** i).toFixed(i >= 2 ? 1 : 0)} ${u[i]}`;
}

export function fmtSpeed(bps: number): string {
	return bps > 0 ? `${fmtBytes(bps)}/s` : "—";
}

export function fmtEta(secs: number | null): string {
	if (secs === null || !Number.isFinite(secs)) return "—";
	if (secs < 60) return `${Math.round(secs)} s`;
	const m = Math.floor(secs / 60);
	if (m < 60) return `${m} min`;
	const h = Math.floor(m / 60);
	return `${h} h ${m % 60} min`;
}
