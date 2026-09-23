#!/usr/bin/env bash
# Baja mpv + yt-dlp + uosc a vendor/ (Linux x86_64).
# Se ejecuta una vez antes de `pnpm tauri build` (y en la CI). Los binarios
# NO se versionan en git (ver vendor/.gitignore); este script los reproduce.
set -euo pipefail
cd "$(dirname "$0")"

# mpv self-contained (AppImage); resolvemos el asset del último release.
MPV_API="https://api.github.com/repos/pkgforge-dev/mpv-AppImage/releases/latest"

echo ">> mpv (AppImage)"
if [ -f mpv ]; then
  echo "  ya está"
else
  url=$(curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors "$MPV_API" \
    | grep -oE '"browser_download_url": *"[^"]*anylinux-x86_64\.AppImage"' \
    | head -1 | sed -E 's/.*"(https[^"]+)"$/\1/')
  if [ -z "$url" ]; then echo "  no encontré el AppImage de mpv"; exit 1; fi
  echo "  bajando: $url"
  curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors -o mpv "$url"
  chmod +x mpv
fi

echo ">> yt-dlp (trailers de YouTube)"
# El iframe de YouTube no funciona dentro del webview de Tauri (origen
# tauri://localhost → "error 153" siempre), así que los trailers se resuelven a
# un mp4 progresivo con yt-dlp. Binario self-contained, sin dependencias.
if [ -f yt-dlp ]; then
  echo "  ya está"
else
  curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors -o yt-dlp \
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
  chmod +x yt-dlp
fi

echo ">> uosc (UI moderna de mpv)"
# Scripts + fonts de uosc dentro de mpv-config/. Los .conf (mpv.conf,
# input.conf, iptv-input.conf) son nuestros y SÍ se versionan; uosc no.
if [ -d mpv-config/scripts/uosc ]; then
  echo "  ya está"
else
  command -v unzip >/dev/null || { echo "falta unzip"; exit 1; }
  uurl=$(curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors "https://api.github.com/repos/tomasklaen/uosc/releases/latest" \
    | grep -oE '"browser_download_url": *"[^"]*uosc\.zip"' \
    | head -1 | sed -E 's/.*"(https[^"]+)"$/\1/')
  if [ -z "$uurl" ]; then echo "  no encontré uosc.zip"; exit 1; fi
  echo "  bajando: $uurl"
  mkdir -p mpv-config
  curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors -o /tmp/uosc.zip "$uurl"
  unzip -o -q /tmp/uosc.zip -d mpv-config
  rm /tmp/uosc.zip
fi

# Cortar acá y no en la máquina del usuario. mpv.conf apaga el OSC y la osd-bar
# nativos porque uosc los reemplaza: si uosc falta, el build sale sin controles
# de reproductor y nadie se entera hasta que lo abre alguien más.
for req in mpv-config/scripts/uosc mpv-config/fonts/uosc_icons.otf; do
  [ -e "$req" ] || { echo "falta $req: el build saldría sin UI de reproductor"; exit 1; }
done

echo ">> listo"
ls -lh mpv yt-dlp
