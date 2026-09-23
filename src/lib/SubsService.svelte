<script lang="ts">
  // Buscador/descargador de subtítulos del reproductor. Vive en el LAYOUT, no
  // en SourcePicker: ese se desmonta al salir del video con Esc (que ahora
  // suspende, no cierra) y se llevaba los listeners, dejando el ítem
  // "⬇ Descargar subtítulos…" sin nadie que respondiera. Acá sigue vivo
  // siempre, incluso al volver con la pill.
  //
  // Flujo: menú del bar → evento "player:download-subs" → buscamos en Wyzie +
  // OpenSubtitles → picker in-video (mpv_open_picker) → "player:menu-pick" →
  // descargamos el elegido y lo cargamos con sub-add.
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { player } from "$lib/playerState.svelte";
  import { config } from "$lib/config.svelte";
  import { buscarSubtitulos, enlaceSub, type OpcionSub } from "$lib/subtitulos";

  let subOptions: OpcionSub[] = [];
  let unlisten: UnlistenFn[] = [];

  const mpv = (args: unknown[]) => invoke("mpv_cmd", { args }).catch(() => {});

  async function onDownloadSubs() {
    const { imdbId, title, season, episode } = player.nowPlaying;
    if (!imdbId) {
      // IPTV u origen sin id: no hay con qué buscar en las APIs.
      await mpv(["show-text", "Sin subtítulos para buscar en este contenido", "2500"]);
      return;
    }
    await mpv(["show-text", "Buscando subtítulos…", "5000"]);
    const opts = await buscarSubtitulos(imdbId, { season, episode });
    if (opts.length === 0) {
      await mpv([
        "show-text",
        config.wyzieKey
          ? "No se encontraron subtítulos"
          : "No se encontraron subtítulos (sin API key de Wyzie)",
        "3000",
      ]);
      return;
    }
    subOptions = opts;
    void title;
    await invoke("mpv_open_picker", {
      title: "Elige un subtítulo",
      items: opts.map((o, i) => ({ label: o.label, id: String(i) })),
    });
  }

  // Eligió uno en el picker in-video → resolver URL, guardar y cargar en mpv.
  async function onPick(idx: string) {
    const o = subOptions[Number(idx)];
    if (!o) return;
    await mpv(["show-text", "Descargando subtítulo…", "8000"]);
    // OpenSubtitles entrega el enlace por fileId (acá sí gasta 1 de cuota,
    // solo del elegido). Wyzie ya lo trae en la búsqueda.
    let url = "";
    try {
      url = await enlaceSub(o);
    } catch (err) {
      await mpv(["show-text", `No se pudo bajar: ${String(err).slice(0, 50)}`, "3000"]);
      return;
    }
    if (!url) {
      await mpv(["show-text", "Subtítulo sin enlace", "2500"]);
      return;
    }
    let toLoad = url;
    try {
      const path = await invoke<string>("subtitle_save", {
        url,
        filename: `${player.nowPlaying.title || "subtitulo"} [${o.lang}]`,
      });
      if (path) toLoad = path;
    } catch (err) {
      console.warn("[subs] save fail, uso URL:", err);
    }
    await mpv(["sub-add", toLoad, "select"]);
    await mpv(["set", "sub-visibility", "yes"]);
    await mpv(["show-text", "✓ Subtítulo cargado", "2000"]);
  }

  onMount(() => {
    void listen("player:download-subs", () => void onDownloadSubs()).then((u) => unlisten.push(u));
    void listen<string>("player:menu-pick", (e) => void onPick(e.payload)).then((u) =>
      unlisten.push(u),
    );
  });

  onDestroy(() => {
    unlisten.forEach((u) => u());
    unlisten = [];
  });
</script>
