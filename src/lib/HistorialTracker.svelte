<script lang="ts">
  // Persiste en `watch_history` lo que mpv está reproduciendo.
  //
  // Antes solo el player web (iframe) escribía progreso, y ese player está
  // apagado (features.ts): en la práctica nada del catálogo quedaba registrado
  // aunque lo vieras entero. Este componente cierra ese hueco.
  //
  // Vive en el layout porque la reproducción sobrevive a los cambios de ruta:
  // SourcePicker se desmonta al salir, mpv sigue corriendo.
  //
  // Qué se guarda lo decide `setContexto()` (historial.svelte.ts). Sin contexto
  // no se escribe nada: un trailer o un canal IPTV no son "algo que viste".
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { contextoActual, guardarProgreso } from "$lib/historial.svelte";

  type MpvStatus = {
    running: boolean;
    pos: number;
    duration: number;
    pause: boolean;
    title: string;
  };

  // 15 s: suficiente para no perder más de un cuarto de minuto si se corta la
  // luz, y lo bastante espaciado para no castigar el IPC en una Raspberry.
  const INTERVALO_MS = 15_000;

  let unlisten: UnlistenFn[] = [];
  let timer: ReturnType<typeof setInterval> | null = null;
  // Última posición vista por el poll: la usa el guardado final, cuando mpv ya
  // cerró y no queda a quién preguntarle.
  let ultimaPos = 0;
  let ultimaDur = 0;

  async function muestrear(): Promise<void> {
    const ctx = contextoActual();
    if (!ctx) return;
    let st: MpvStatus;
    try {
      st = await invoke<MpvStatus>("mpv_status");
    } catch {
      return;
    }
    if (!st.running) return;
    // duration <= 0 es un stream sin fin (IPTV): no hay progreso que medir.
    if (!isFinite(st.pos) || st.pos <= 0 || st.duration <= 0) return;
    ultimaPos = st.pos;
    ultimaDur = st.duration;
    await guardarProgreso(ctx, {
      watched: st.pos,
      runtime: Math.floor(st.duration),
      real: st.pos / st.duration,
    });
  }

  // Esc (suspender) apaga RUNNING antes de emitir el evento: para entonces
  // `mpv_status` ya devuelve vacío. `mpv_session` sí conserva la posición de lo
  // que quedó esperando, y es la lectura exacta del momento en que salió — sin
  // ella el corte perdería hasta un intervalo entero de poll.
  async function guardarFinal(): Promise<void> {
    const ctx = contextoActual();
    if (!ctx) return;
    let pos = ultimaPos;
    let dur = ultimaDur;
    try {
      const s = await invoke<{ suspended: boolean; pos: number; duration: number; live: boolean }>(
        "mpv_session",
      );
      if (s.suspended && !s.live && s.pos > 0 && s.duration > 0) {
        pos = s.pos;
        dur = s.duration;
      }
    } catch {
      /* sin sesión: nos quedamos con lo último que vio el poll */
    }
    if (pos <= 0 || dur <= 0) return;
    await guardarProgreso(ctx, {
      watched: pos,
      runtime: Math.floor(dur),
      real: pos / dur,
    });
  }

  function arrancar(): void {
    detener();
    ultimaPos = 0;
    ultimaDur = 0;
    timer = setInterval(() => void muestrear(), INTERVALO_MS);
  }

  function detener(): void {
    if (timer) clearInterval(timer);
    timer = null;
  }

  onMount(() => {
    void listen<boolean>("mpv:state", (e) => {
      if (e.payload) {
        arrancar();
      } else {
        // mpv cerró: un último muestreo ya no sirve (el proceso no responde),
        // guardamos lo último que sabíamos.
        detener();
        void guardarFinal();
      }
    }).then((u) => unlisten.push(u));

    // Esc suspende sin cerrar: es el momento más probable de "dejarla a medias".
    void listen<boolean>("mpv:suspended", (e) => {
      if (e.payload) void muestrear();
    }).then((u) => unlisten.push(u));
  });

  onDestroy(() => {
    detener();
    unlisten.forEach((u) => u());
    unlisten = [];
  });
</script>
