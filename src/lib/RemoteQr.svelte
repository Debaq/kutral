<script lang="ts">
  import QRCode from "qrcode";

  let { url, size = 72 }: { url: string; size?: number } = $props();
  let dataUrl = $state("");

  $effect(() => {
    if (!url) { dataUrl = ""; return; }
    QRCode.toDataURL(url, {
      errorCorrectionLevel: "M",
      margin: 1,
      width: size * 3,
      color: { dark: "#0b0b0f", light: "#ffffff" },
    })
      .then((d) => { dataUrl = d; })
      .catch((e) => { console.warn("[qr]", e); dataUrl = ""; });
  });
</script>

{#if dataUrl}
  <div class="qr-badge" title="Escanea para usar tu celular como mando: {url}">
    <img
      src={dataUrl}
      alt="QR mando"
      style:width="{size}px"
      style:height="{size}px"
    />
    <span class="qr-label">Mando</span>
  </div>
{/if}

<style>
  .qr-badge {
    /* En flujo normal: empuja la carátula hacia abajo, no la tapa. */
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    background: #fff;
    padding: 6px 6px 4px;
    border-radius: 8px;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.55);
    margin: 0 0 12px;
    pointer-events: auto;
  }
  .qr-badge img {
    display: block;
    border-radius: 3px;
  }
  .qr-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: #0b0b0f;
  }
</style>
