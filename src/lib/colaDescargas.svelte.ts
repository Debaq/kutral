// Cola de descargas pendientes: lo que pediste bajar y todavía no empezó.
//
// Por qué existe: el cliente torrent baja TODO lo que le tires en paralelo. Una
// temporada de 24 capítulos encolada de golpe se reparte la conexión y los
// seeds entre 24 descargas, y no termina ninguna hasta el final. Acá esperan
// turno y salen de a pocas (config.torrentMaxParalelas).
//
// Qué NO se guarda: el magnet. La búsqueda se hace al llegar el turno, porque
// un magnet de hace tres días puede estar sin seeds y porque así la elección
// respeta la config de idioma/calidad del momento. Lo que se guarda es QUÉ
// capítulo es, que no caduca.
//
// El motor vive en el frontend a propósito: es el mismo lado que ya sabe
// buscar fuentes (kodios_search) y ordenarlas (fuentes.ts). Si la app se
// cierra, la cola queda en SQLite y sigue en el próximo arranque.

import { invoke } from "@tauri-apps/api/core";
import { getDb } from "$lib/vera/db";
import { config } from "$lib/config.svelte";
import { notify } from "$lib/notifStore.svelte";
import { ordenarFuentes, stack, cabeEnLocal, fmtSize, type Src, type Kind } from "$lib/fuentes";
import { addTorrent, torrents, refreshQueue, startQueuePoll } from "$lib/torrents.svelte";
import { registrarDescarga, estadoDescarga } from "$lib/descargas.svelte";

export type FilaPendiente = {
  id: number;
  clave: string;
  season: number;
  episode: number;
  title: string;
  etiqueta: string;
  imdb_id: string;
  kind: Kind;
  original_title: string | null;
  kitsu_id: number | null;
  estado: "espera" | "buscando" | "error";
  error: string | null;
  added_at: number;
};

/** Lo que hace falta para encolar algo. El resto lo pone la cola. */
export type Pedido = {
  clave: string;
  season?: number | null;
  episode?: number | null;
  title: string;
  etiqueta?: string;
  imdbId?: string;
  kind: Kind;
  originalTitle?: string | null;
  kitsuId?: number | null;
};

export const cola = $state({
  filas: [] as FilaPendiente[],
  cargado: false,
});

/** Cuántas descargas están corriendo de verdad (las que ocupan ancho de banda). */
export function activas(): number {
  return torrents.list.filter(
    (t) => !t.finished && t.state !== "error" && !t.paused,
  ).length;
}

export function enEspera(): number {
  return cola.filas.filter((f) => f.estado !== "error").length;
}

// ---- Carga y escritura ---------------------------------------------------

export async function cargarCola(): Promise<void> {
  const db = await getDb();
  if (!db) {
    cola.cargado = true;
    return;
  }
  try {
    // 'buscando' que sobrevivió a un cierre de la app es una búsqueda que
    // nunca terminó: vuelve a la fila en vez de quedar trabada para siempre.
    await db.execute(
      "UPDATE descargas_pendientes SET estado = 'espera' WHERE estado = 'buscando'",
    );
    cola.filas = await db.select<FilaPendiente[]>(
      "SELECT * FROM descargas_pendientes ORDER BY added_at ASC, id ASC",
    );
  } catch (e) {
    console.warn("[cola] no se pudo cargar:", e);
  }
  cola.cargado = true;
}

/**
 * Mete pedidos a la cola. Devuelve cuántos entraron: los que ya están bajados,
 * bajando o encolados se saltan en silencio (pedir la temporada entera dos
 * veces es lo normal, no un error).
 */
export async function encolar(pedidos: Pedido[]): Promise<number> {
  const db = await getDb();
  if (!db) return 0;
  let n = 0;
  const ahora = Math.floor(Date.now() / 1000);
  for (const p of pedidos) {
    const season = p.season ?? -1;
    const episode = p.episode ?? -1;
    if (estadoDescarga(p.clave, season, episode)) continue; // ya está o ya baja
    if (yaEnCola(p.clave, season, episode)) continue;
    try {
      await db.execute(
        `INSERT INTO descargas_pendientes
           (clave, season, episode, title, etiqueta, imdb_id, kind,
            original_title, kitsu_id, estado, added_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'espera', $10)
         ON CONFLICT(clave, season, episode) DO NOTHING`,
        [
          p.clave, season, episode, p.title, p.etiqueta ?? "", p.imdbId ?? "",
          p.kind, p.originalTitle ?? null, p.kitsuId ?? null, ahora,
        ],
      );
      n++;
    } catch (e) {
      console.warn("[cola] no se pudo encolar:", e);
    }
  }
  if (n) {
    await cargarCola();
    arrancarMotor();
  }
  return n;
}

export function yaEnCola(clave: string, season = -1, episode = -1): boolean {
  return cola.filas.some(
    (f) => f.clave === clave && f.season === season && f.episode === episode,
  );
}

export async function quitarDeCola(id: number): Promise<void> {
  const db = await getDb();
  if (db) {
    try {
      await db.execute("DELETE FROM descargas_pendientes WHERE id = $1", [id]);
    } catch (e) {
      console.warn("[cola] no se pudo quitar:", e);
      return;
    }
  }
  cola.filas = cola.filas.filter((f) => f.id !== id);
}

/** Vacía la cola entera, o solo lo de un título. */
export async function vaciarCola(clave?: string): Promise<void> {
  const db = await getDb();
  if (db) {
    try {
      if (clave) {
        await db.execute("DELETE FROM descargas_pendientes WHERE clave = $1", [clave]);
      } else {
        await db.execute("DELETE FROM descargas_pendientes");
      }
    } catch (e) {
      console.warn("[cola] no se pudo vaciar:", e);
      return;
    }
  }
  cola.filas = clave ? cola.filas.filter((f) => f.clave !== clave) : [];
}

/** Devuelve a la fila algo que falló, para volver a intentarlo. */
export async function reintentar(id: number): Promise<void> {
  await setEstado(id, "espera", null);
  arrancarMotor();
}

async function setEstado(
  id: number,
  estado: FilaPendiente["estado"],
  error: string | null,
): Promise<void> {
  const db = await getDb();
  if (db) {
    try {
      await db.execute(
        "UPDATE descargas_pendientes SET estado = $1, error = $2 WHERE id = $3",
        [estado, error, id],
      );
    } catch (e) {
      console.warn("[cola] no se pudo actualizar:", e);
    }
  }
  cola.filas = cola.filas.map((f) => (f.id === id ? { ...f, estado, error } : f));
}

// ---- Motor ---------------------------------------------------------------

let timer: ReturnType<typeof setInterval> | null = null;
let corriendo = false;

/**
 * Enciende el motor. Idempotente: repetirlo no crea dos timers. Se apaga solo
 * cuando no queda nada en espera, así que en uso normal no cuesta nada.
 */
export function arrancarMotor(ms = 5000): void {
  void tick();
  if (timer !== null) return;
  timer = setInterval(() => void tick(), ms);
}

export function pararMotor(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

async function tick(): Promise<void> {
  if (corriendo) return;
  if (!config.torrentLocal) return pararMotor();
  const pendientes = cola.filas.filter((f) => f.estado === "espera");
  if (!pendientes.length) {
    // Nada en espera: el motor se duerme hasta que se encole algo.
    if (!cola.filas.some((f) => f.estado === "buscando")) pararMotor();
    return;
  }
  corriendo = true;
  try {
    // El estado real de la cola manda: si el usuario pausó o borró descargas
    // desde el popover, acá hay que verlo antes de decidir si hay lugar.
    await refreshQueue();
    // Las recién arrancadas se cuentan a mano: `torrents.list` no las tiene
    // hasta el próximo refresh, y sin esto una sola pasada podría arrancar
    // toda la cola de golpe — justo lo que la cola viene a evitar.
    let arrancadas = 0;
    for (const f of pendientes) {
      if (activas() + arrancadas >= config.torrentMaxParalelas) break;
      if (await procesar(f)) arrancadas++;
    }
  } finally {
    corriendo = false;
  }
}

/** true si la descarga quedó arrancando (ocupa un lugar). */
async function procesar(f: FilaPendiente): Promise<boolean> {
  await setEstado(f.id, "buscando", null);
  try {
    const srcs = await invoke<Src[]>("kodios_search", {
      imdbId: f.imdb_id,
      kind: f.kind,
      title: f.title,
      originalTitle: f.original_title ?? undefined,
      season: f.season >= 0 ? f.season : undefined,
      episode: f.episode >= 0 ? f.episode : undefined,
      kitsuId: f.kitsu_id ?? undefined,
    });
    ordenarFuentes(srcs, f.kind);
    // Solo torrents (un enlace directo no se puede bajar) y solo lo que cabe
    // en el techo de calidad/peso que el usuario puso en Configuración.
    const cands = stack(srcs).filter((s) => s.magnet && cabeEnLocal(s));
    if (!cands.length) {
      throw new Error("sin torrents dentro de tu tope de calidad y peso");
    }
    const s = cands[0];
    await verificarEspacio(s.size_bytes);

    const nombre = f.etiqueta ? `${f.title} · ${f.etiqueta}` : f.title;
    const added = await addTorrent(s.magnet!, nombre, config.torrentBufferMb);
    await registrarDescarga({
      clave: f.clave,
      season: f.season,
      episode: f.episode,
      infoHash: added.info_hash,
      ruta: added.path,
      releaseTitle: s.title || added.name,
      quality: s.quality,
      sizeBytes: added.size_bytes,
    });
    await quitarDeCola(f.id);
    startQueuePoll();
    void invoke("ui_log", { msg: `[cola] bajando ${nombre} (${added.name})` });
    return true;
  } catch (e) {
    const msg = String(e).replace(/^Error:\s*/, "");
    await setEstado(f.id, "error", msg);
    notify(
      "error",
      "No se pudo bajar",
      `${f.title}${f.etiqueta ? ` · ${f.etiqueta}` : ""}: ${msg}`,
    );
    return false;
  }
}

/**
 * Llenar el disco rompe bastante más que la descarga, así que se mira antes
 * de cada una. Margen del 10%: el archivo no es lo único que el sistema va a
 * querer escribir mientras baja.
 */
async function verificarEspacio(size: number | null): Promise<void> {
  if (!size) return;
  let libre: number;
  try {
    libre = await invoke<number>("disk_free", {
      dir: config.torrentDir.trim() || null,
    });
  } catch {
    return; // si no se puede saber, no bloqueamos: el cliente avisará al fallar
  }
  if (libre < size * 1.1) {
    throw new Error(
      `no queda espacio: hacen falta ${fmtSize(size)} y libres hay ${fmtSize(libre)}`,
    );
  }
}
