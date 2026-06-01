#!/usr/bin/env bash
# Baja retroarch + cores libretro a vendor/ (Linux x86_64).
# Se ejecuta una vez antes de `npm run tauri build`. Los binarios NO se
# versionan en git (ver vendor/.gitignore); este script los reproduce.
set -euo pipefail
cd "$(dirname "$0")"

CORES="https://buildbot.libretro.com/nightly/linux/x86_64/latest"
RA="https://buildbot.libretro.com/nightly/linux/x86_64/RetroArch.7z"

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

echo ">> listo"
ls -lh retroarch cores/*.so
