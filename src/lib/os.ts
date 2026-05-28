import { invoke } from '@tauri-apps/api/core';

export type OsInfo = {
	is_kutral_os: boolean;
	platform: 'windows' | 'macos' | 'linux' | 'other';
	version: string | null;
};

let cached: OsInfo | null = null;
let inFlight: Promise<OsInfo> | null = null;

export async function getOsInfo(): Promise<OsInfo> {
	if (cached) return cached;
	if (inFlight) return inFlight;
	inFlight = invoke<OsInfo>('os_info').then((info) => {
		cached = info;
		inFlight = null;
		return info;
	});
	return inFlight;
}

export async function isKutralOs(): Promise<boolean> {
	const info = await getOsInfo();
	return info.is_kutral_os;
}
