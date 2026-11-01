<script lang="ts">
  // Fondo de las pantallas de reproducción (menú, fuentes, capítulos,
  // post-créditos): el fanart difuminado y un velo encima para que el texto se
  // lea. Antes cada pantalla repetía el mismo bloque con su prefijo.
  // El padre tiene que ser position: fixed/relative con overflow: hidden.
  let {
    src = null,
    blur = 10,
    escala = 1.06,
    opacidad = 1,
    velo,
  }: {
    src?: string | null;
    blur?: number;
    escala?: number;
    opacidad?: number;
    /** `background` del velo (degradados). */
    velo: string;
  } = $props();
</script>

{#if src}
  <div
    class="fondo"
    style:background-image="url({src})"
    style:filter="blur({blur}px) saturate(1.1)"
    style:transform="scale({escala})"
    style:opacity={opacidad}
  ></div>
{/if}
<div class="velo" style:background={velo}></div>

<style>
  .fondo {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
  }
  .velo {
    position: absolute;
    inset: 0;
  }
</style>
