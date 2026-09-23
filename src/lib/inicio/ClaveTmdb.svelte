<script lang="ts">
  // Primer uso (o cambio de key): pedir la API key de TMDb. Con el servidor
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
</script>

<div class="key-box">
  <h3>TMDb API Key</h3>
  <p class="hint">
    Consíguela en <code>themoviedb.org/settings/api</code> (gratis).
  </p>
  <input
    type="password"
    bind:value={valor}
    placeholder="32 chars hex"
    onkeydown={(e) => e.key === "Enter" && onGuardar()}
  />
  <button onclick={onGuardar} disabled={!valor.trim()}>Guardar</button>

  {#if urlTeclado}
    <div class="key-phone">
      <RemoteQr url={urlTeclado} label="Teclado" size={110} />
      <div class="key-phone-txt">
        <strong>¿Difícil escribir con el control?</strong>
        <span>
          Escanea este QR con el celular. Desde ahí puedes escribir,
          pegar o escanear la key con la cámara: aparece sola en el campo
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
