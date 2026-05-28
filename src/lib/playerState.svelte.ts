export const player = $state({
	playing: false,
});

export function setPlaying(v: boolean) {
	player.playing = v;
}
