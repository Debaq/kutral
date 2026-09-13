// Lanzar subprocesos en Windows sin que parpadee una consola.
//
// La app se compila con `windows_subsystem = "windows"` (main.rs), así que no
// tiene consola propia. Cuando lanza un binario de subsistema consola (mpv,
// yt-dlp, ffprobe) Windows le crea una ventana de consola nueva: el usuario ve
// un cuadro negro que aparece y desaparece. `--no-terminal` de mpv no lo evita,
// porque la consola la crea CreateProcess, no mpv.
//
// CREATE_NO_WINDOW la suprime sin tocar la ventana de video (esa es una ventana
// gráfica normal, no la consola). En el resto de plataformas no hace nada.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Sin consola para `std::process::Command`.
pub fn hide_console(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Sin consola para `tokio::process::Command`.
pub fn hide_console_tokio(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    // `creation_flags` acá es método propio de tokio (no el trait de std).
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
