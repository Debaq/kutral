# vendor/ — binarios embebidos en el bundle

Todo lo de aquí se copia dentro del AppImage / NSIS / MSI vía
`bundle.resources` (ver tauri.conf.json) y se resuelve en runtime desde
`resource_dir()/vendor/`. **Nada se baja ni se instala aparte.**

## Cómo obtenerlos

```
./fetch.sh              # Linux
./fetch-windows.ps1     # Windows
```

**No se versionan en git** (ver `.gitignore`); los scripts los reproducen
antes de `pnpm tauri build`.

## Qué queda

```
vendor/
  mpv            ← reproductor externo (solo el camino no-Linux lo usa) | mpv.exe
  yt-dlp         ← trailers de YouTube a mp4                            | yt-dlp.exe
  mpv-config/    ← mpv.conf, input.conf y uosc (scripts/ y fonts/ los baja fetch)
```
