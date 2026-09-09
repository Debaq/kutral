#!/usr/bin/env bash
# Build local del flatpak de Kütral.
#
#   ./flatpak/build.sh            → construye e instala en --user
#   ./flatpak/build.sh --repo     → además exporta a flatpak/repo/ (repo propio)
#
# Requiere: flatpak, org.flatpak.Builder, org.gnome.{Platform,Sdk}//49,
# org.freedesktop.Sdk.Extension.{rust-stable,node22}//25.08
set -euo pipefail

cd "$(dirname "$0")"
MANIFEST=app.kutral.Kutral.yml
BUILDDIR=.build
STATEDIR=.flatpak-builder

BUILDER=(flatpak run --filesystem=host org.flatpak.Builder)

# --install y --repo en la MISMA pasada: separarlas recompila todo dos veces.
REPO_ARGS=()
if [ "${1:-}" = "--repo" ]; then
  REPO_ARGS=(--repo=repo)
fi

"${BUILDER[@]}" \
  --user --install --force-clean \
  "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" \
  --state-dir "$STATEDIR" \
  "$BUILDDIR" "$MANIFEST"

if [ "${1:-}" = "--repo" ]; then
  flatpak build-bundle repo kutral.flatpak app.kutral.Kutral
  echo ">> kutral.flatpak listo"
fi

echo ">> correr con: flatpak run app.kutral.Kutral"
