export const player = $state({
	playing: false,
	/** Sesión viva pero fuera de pantalla (Esc): la pill ofrece volver. */
	suspended: false,
	title: "",
	pos: 0,
	duration: 0,
	/** IPTV: sigue sonando de fondo mientras navegas, no está pausado. */
	live: false,
	/** Qué está cargado en mpv: lo necesita el buscador de subtítulos, que
	 *  vive en el layout (no en el picker, que se desmonta al salir). */
	nowPlaying: { imdbId: "", title: "", season: null as number | null, episode: null as number | null },
});

export function setPlaying(v: boolean) {
	player.playing = v;
}

export type MpvSession = {
	suspended: boolean;
	title: string;
	pos: number;
	duration: number;
	live: boolean;
};

export function setSession(s: MpvSession | null) {
	player.suspended = s?.suspended ?? false;
	player.title = s?.title ?? "";
	player.pos = s?.pos ?? 0;
	player.duration = s?.duration ?? 0;
	player.live = s?.live ?? false;
}

/** Contexto de lo que se manda a reproducir (para buscar subtítulos después).
 *  IPTV lo limpia: un canal en vivo no tiene imdb con el que buscar. */
export function setNowPlaying(
	imdbId: string,
	title: string,
	season: number | null = null,
	episode: number | null = null,
) {
	player.nowPlaying = { imdbId, title, season, episode };
}
