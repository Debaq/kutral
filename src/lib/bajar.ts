// Bajar una fuente para después: por Real-Debrid (HTTP) o por torrent.
//
// Real-Debrid primero, si está vinculado y el usuario no lo apagó: baja a toda
// velocidad desde sus servidores y nadie ve tu IP. Si RD no puede con esa
// versión (DMCA, hoster caído) y la descarga local está activada, se baja del
// swarm como siempre. Las copias en hosters (Mega, Mediafire…) solo existen
// por RD.

import { config } from "$lib/config.svelte";
import { pistaDe } from "$lib/pack";
import { addTorrent, httpDescargar, type TorrentAdded } from "$lib/torrents.svelte";
import type { Src } from "$lib/fuentes";

/** ¿Hay alguna forma de bajar? (torrent local, o Real-Debrid por HTTP) */
export function puedeBajar(): boolean {
  return config.torrentLocal || (config.rdLinked && config.bajarConRd);
}

/** ¿Esta fuente se puede bajar con lo que hay configurado? */
export function sePuedeBajar(s: Src): boolean {
  const rd = config.rdLinked && config.bajarConRd;
  if (s.links?.length) return rd;
  if (!s.magnet) return false;
  return rd || config.torrentLocal;
}

export async function bajarFuente(
  s: Src,
  nombre: string,
  season: number | null,
  episode: number | null,
): Promise<TorrentAdded> {
  const rd = config.rdLinked && config.bajarConRd;
  if (s.links?.length) {
    if (!rd) throw new Error("Esa versión solo se baja con Real-Debrid.");
    return httpDescargar({ tipo: "links", links: s.links }, nombre);
  }
  if (!s.magnet) {
    throw new Error("Esa fuente es streaming directo, no un archivo: no se puede bajar.");
  }
  const pista = pistaDe(s, season, episode);
  if (rd) {
    try {
      return await httpDescargar({ tipo: "magnet", magnet: s.magnet, ...pista }, nombre);
    } catch (e) {
      // DMCA o sin caché: por torrent sí se puede, si el usuario lo permite.
      if (!config.torrentLocal) throw e;
    }
  }
  if (!config.torrentLocal) {
    throw new Error("Activa la descarga local o Real-Debrid para bajar.");
  }
  return addTorrent(s.magnet, nombre, config.torrentBufferMb, pista);
}
