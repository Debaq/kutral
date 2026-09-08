#!/usr/bin/env bash
# Baja retroarch + cores libretro + mpv a vendor/ (Linux x86_64).
# Se ejecuta una vez antes de `npm run tauri build` (y en la CI). Los binarios
# NO se versionan en git (ver vendor/.gitignore); este script los reproduce.
set -euo pipefail
cd "$(dirname "$0")"

CORES="https://buildbot.libretro.com/nightly/linux/x86_64/latest"
RA="https://buildbot.libretro.com/nightly/linux/x86_64/RetroArch.7z"
# mpv self-contained (AppImage); resolvemos el asset del último release.
MPV_API="https://api.github.com/repos/pkgforge-dev/mpv-AppImage/releases/latest"

echo ">> cores"
mkdir -p cores
for c in fceumm snes9x mgba gambatte melonds; do
  if [ -f "cores/${c}_libretro.so" ]; then echo "  ya está: $c"; continue; fi
  echo "  bajando: $c"
  curl -sSL -o "cores/$c.zip" "$CORES/${c}_libretro.so.zip"
  unzip -o -q "cores/$c.zip" -d cores
  rm "cores/$c.zip"
done

echo ">> retroarch (AppImage)"
if [ -f retroarch ]; then
  echo "  ya está"
else
  command -v 7z >/dev/null || { echo "falta 7z (p7zip)"; exit 1; }
  tmp=$(mktemp -d)
  curl -sSL -o "$tmp/RetroArch.7z" "$RA"
  7z e -y "$tmp/RetroArch.7z" \
    "RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage" -o"$tmp" >/dev/null
  mv "$tmp/RetroArch-Linux-x86_64.AppImage" retroarch
  chmod +x retroarch
  rm -rf "$tmp"
fi

echo ">> mpv (AppImage)"
if [ -f mpv ]; then
  echo "  ya está"
else
  url=$(curl -sSL "$MPV_API" \
    | grep -oE '"browser_download_url": *"[^"]*anylinux-x86_64\.AppImage"' \
    | head -1 | sed -E 's/.*"(https[^"]+)"$/\1/')
  if [ -z "$url" ]; then echo "  no encontré el AppImage de mpv"; exit 1; fi
  echo "  bajando: $url"
  curl -sSL -o mpv "$url"
  chmod +x mpv
fi

echo ">> yt-dlp (trailers de YouTube)"
# El iframe de YouTube no funciona dentro del webview de Tauri (origen
# tauri://localhost → "error 153" siempre), así que los trailers se resuelven a
# un mp4 progresivo con yt-dlp. Binario self-contained, sin dependencias.
if [ -f yt-dlp ]; then
  echo "  ya está"
else
  curl -sSL -o yt-dlp \
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
  uurl=$(curl -sSL "https://api.github.com/repos/tomasklaen/uosc/releases/latest" \
    | grep -oE '"browser_download_url": *"[^"]*uosc\.zip"' \
    | head -1 | sed -E 's/.*"(https[^"]+)"$/\1/')
  if [ -z "$uurl" ]; then echo "  no encontré uosc.zip"; exit 1; fi
  echo "  bajando: $uurl"
  mkdir -p mpv-config
  curl -sSL -o /tmp/uosc.zip "$uurl"
  unzip -o -q /tmp/uosc.zip -d mpv-config
  rm /tmp/uosc.zip
fi

echo ">> Material Icons Round (iconos del bar del reproductor)"
# El bar del player embebido dibuja sus iconos con ASS (\fnMaterial Icons Round).
# Si el archivo falta, libass cae a una fuente normal y en pantalla salen los
# NOMBRES de los iconos en vez de los iconos. mpv-config/fonts/ está en
# .gitignore, así que este script es la ÚNICA vía por la que llega a un build
# limpio o a la CI. Apache-2.0.
MI_OTF="mpv-config/fonts/MaterialIconsRound-Regular.otf"
if [ -f "$MI_OTF" ]; then
  echo "  ya está"
else
  mkdir -p mpv-config/fonts
  curl -sSL -o "$MI_OTF" \
    "https://raw.githubusercontent.com/google/material-design-icons/master/font/MaterialIconsRound-Regular.otf"
fi

echo ">> listo"
ls -lh retroarch mpv yt-dlp cores/*.so
