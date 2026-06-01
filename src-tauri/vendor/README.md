# vendor/ — binarios embebidos en el bundle

Todo lo de aquí se copia dentro del AppImage / NSIS / MSI vía
`bundle.resources` (ver tauri.conf.json) y se resuelve en runtime desde
`resource_dir()/vendor/`. **Nada se baja ni se instala aparte.**

## Cómo obtenerlos

```
./fetch.sh
```

Baja `retroarch` (AppImage self-contained) + los 5 cores desde el buildbot
oficial de libretro. **No se versionan en git** (ver `.gitignore`); el script
los reproduce antes de `npm run tauri build`.

## Qué queda

```
vendor/
  retroarch              ← binario RetroArch (Linux)   | retroarch.exe (Windows)
  cores/
    fceumm_libretro.so   ← NES        (.dll en Windows)
    snes9x_libretro.so   ← SNES
    mgba_libretro.so     ← GBA
    gambatte_libretro.so ← GB Color
    melonds_libretro.so  ← DS
```

`emu.rs` busca exactamente estas rutas/nombres. Si falta el binario,
`emu_play` devuelve "retroarch embebido no encontrado en el bundle".

## Notas de empaquetado

- RetroArch es GPL: se puede redistribuir junto al programa.
- El binario Linux debe traer sus libs (build estático o AppImage de
  RetroArch desempacado). Probar `./retroarch -L cores/fceumm_libretro.so rom.nes`.
- En dev (sin bundle) `emu.rs` cae al PATH y a `~/.config/retroarch/cores`
  como fallback, así se puede desarrollar sin tener todo embebido aún.
