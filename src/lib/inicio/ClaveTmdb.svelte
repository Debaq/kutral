<script lang="ts">
  // Primer uso (o cambio de clave): pedir la clave de API de TMDb. Con el servidor
  // web arriba se ofrece un QR para escribirla desde el celular.
  import RemoteQr from "$lib/RemoteQr.svelte";

  let {
    valor = $bindable(""),
    urlTeclado,
    onGuardar,
  }: {
    valor?: string;
    urlTeclado: string | null;
    onGuardar: () => void;
  } = $props();

  // Con control remoto solo se llega a lo que tiene data-nav: sin foco
  // inicial ni data-nav, el campo quedaba inalcanzable con flechas.
  function enfocar(el: HTMLInputElement) {
    setTimeout(() => el.focus(), 50);
  }
</script>

<div class="key-box">
  <h3>Clave de TMDb</h3>
  <p class="hint">
    Consíguela en <code>themoviedb.org/settings/api</code> (gratis).
  </p>
  <input
    data-nav
    use:enfocar
    type="password"
    bind:value={valor}
    placeholder="Pega aquí tu clave"
    onkeydown={(e) => e.key === "Enter" && onGuardar()}
  />
  <button data-nav onclick={onGuardar} disabled={!valor.trim()}>Guardar</button>

  {#if urlTeclado}
    <div class="key-phone">
      <RemoteQr url={urlTeclado} label="Teclado" size={110} />
      <div class="key-phone-txt">
        <strong>¿Difícil escribir con el control?</strong>
        <span>
          Escanea este QR con el celular. Desde ahí puedes escribir,
          pegar o escanear la clave con la cámara: aparece sola en el campo
          de arriba. Luego pulsa Guardar.
        </span>
      </div>
    </div>
  {:else}
    <p class="key-phone-wait">Preparando teclado por celular…</p>
  {/if}
</div>

<style>
  .key-box { padding: 20px; }
  .key-box h3 { margin: 0 0 8px; }
  .key-box .hint { font-size: 12px; color: #888; margin: 0 0 12px; }
  .key-box code { background: #1a1a22; padding: 2px 5px; border-radius: 3px; font-size: 11px; }
  .key-box input { width: 100%; padding: 8px; background: #1a1a22; border: 1px solid #2a2a35; color: #eee; border-radius: 4px; margin-bottom: 8px; box-sizing: border-box; }
  .key-box button { width: 100%; padding: 8px; background: #f5c518; color: #000; border: 0; border-radius: 4px; font-weight: 700; cursor: pointer; }
  .key-box button:disabled { opacity: 0.4; cursor: not-allowed; }
  .key-phone {
    display: flex;
    gap: 14px;
    align-items: center;
    margin-top: 18px;
    padding-top: 16px;
    border-top: 1px solid #23232c;
  }
  .key-phone-txt { display: flex; flex-direction: column; gap: 4px; }
  .key-phone-txt strong { font-size: 13px; color: #f5c518; }
  .key-phone-txt span { font-size: 12px; color: #9a9aa4; line-height: 1.45; }
  .key-phone-wait { margin: 16px 0 0; font-size: 11.5px; color: #6e6e78; }
</style>
