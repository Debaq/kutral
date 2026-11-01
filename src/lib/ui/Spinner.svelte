<script lang="ts">
  // Ruedita de "cargando". Antes había cuatro iguales (sp-spin, ep-spin,
  // cfg-spin, spin), cada una con su keyframe.
  let {
    tam = 32,
    grosor = tam >= 24 ? 3 : 2,
    color = "var(--marca)",
  }: { tam?: number; grosor?: number; color?: string } = $props();
</script>

<span
  class="spinner"
  role="status"
  aria-label="Cargando"
  style:width="{tam}px"
  style:height="{tam}px"
  style:border-width="{grosor}px"
  style:border-top-color={color}
></span>

<style>
  .spinner {
    display: inline-block;
    vertical-align: middle;
    box-sizing: border-box;
    border-style: solid;
    border-color: var(--borde);
    border-radius: 50%;
    animation: girar 1s linear infinite;
    flex: none;
  }
  @keyframes girar {
    to {
      transform: rotate(360deg);
    }
  }
</style>
