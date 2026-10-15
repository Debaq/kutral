// Reproductor embebido en Windows — SPIKE (feature `embed-win`).
//
// Prueba lo riesgoso antes de portar mpv_embed.rs: libmpv dibujando con `wid`
// en una ventana hija de la de Kütral, encima de WebView2; el `osd-overlay`
// (con eso se dibujan la barra y los menús de Linux) visible encima del video;
// y Esc que oculta el video y le devuelve el foco a la interfaz.
//
// Se activa con KUTRAL_SPIKE_EMBED=<url> (lo corre .github/workflows/
// spike-windows.yml, que saca capturas de pantalla). Lo que pasa se anota en
// stderr con el prefijo [spike].

use libmpv2::{events::Event, Mpv};
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassW, SetWindowPos, ShowWindow,
    CS_HREDRAW, CS_VREDRAW, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};

static MPV: OnceLock<Arc<Mpv>> = OnceLock::new();

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ventana hija negra del tamaño del área cliente, arriba de todo.
fn crear_superficie(padre: HWND) -> HWND {
    unsafe {
        let clase = w("KutralVideo");
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
            lpszClassName: clase.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(padre, &mut rc);
        eprintln!("[spike] área cliente {}x{}", rc.right, rc.bottom);
        let hijo = CreateWindowExW(
            0,
            clase.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            rc.right,
            rc.bottom,
            padre,
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        );
        SetWindowPos(hijo, HWND_TOP, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);
        hijo
    }
}

pub fn spike(app: &tauri::AppHandle, win: &tauri::WebviewWindow, url: String) {
    let padre = match win.hwnd() {
        Ok(h) => h.0 as HWND,
        Err(e) => return eprintln!("[spike] sin HWND: {e}"),
    };
    let hijo = crear_superficie(padre);
    eprintln!("[spike] superficie creada: {}", !hijo.is_null());
    let hijo_n = hijo as usize;

    // Esc → script-message, igual que haría el reproductor de verdad.
    let conf = std::env::temp_dir().join("kutral-spike-input.conf");
    let _ = std::fs::write(&conf, "ESC script-message kutral-esc\n");
    let capturas = std::env::var("KUTRAL_SPIKE_DIR").unwrap_or_else(|_| ".".into());
    let mpv = Mpv::with_initializer(|i| {
        i.set_option("wid", hijo_n as i64)?;
        i.set_option("config", "no")?;
        i.set_option("terminal", "yes")?;
        i.set_option("msg-level", "all=info")?;
        i.set_option("idle", "yes")?;
        i.set_option("keep-open", "yes")?;
        i.set_option("osc", "no")?;
        i.set_option("vo", "gpu")?;
        i.set_option("hwdec", "auto-safe")?;
        i.set_option("input-default-bindings", "yes")?;
        i.set_option("input-vo-keyboard", "yes")?;
        i.set_option("input-conf", conf.to_string_lossy().as_ref())?;
        i.set_option("screenshot-directory", capturas.as_str())?;
        Ok(())
    });
    let mpv = match mpv {
        Ok(m) => Arc::new(m),
        Err(e) => return eprintln!("[spike] libmpv no arrancó: {e}"),
    };
    eprintln!("[spike] libmpv arrancó");
    let _ = MPV.set(mpv.clone());
    if let Err(e) = mpv.command("loadfile", &[&url]) {
        eprintln!("[spike] loadfile: {e}");
    }

    // El área cliente cambia (pantalla completa, maximizar): la superficie va
    // detrás.
    win.on_window_event(move |ev| {
        if let tauri::WindowEvent::Resized(t) = ev {
            unsafe {
                SetWindowPos(
                    hijo_n as HWND,
                    HWND_TOP,
                    0,
                    0,
                    t.width as i32,
                    t.height as i32,
                    SWP_NOACTIVATE,
                );
            }
        }
    });

    let app = app.clone();
    std::thread::spawn(move || {
        let Some(mpv) = MPV.get() else { return };
        let mut primera = true;
        loop {
            let Some(ev) = mpv.wait_event(1.0) else { continue };
            match ev {
                Ok(Event::PlaybackRestart) if primera => {
                    primera = false;
                    eprintln!("[spike] video reproduciendo");
                    let _ = mpv.command(
                        "osd-overlay",
                        &[
                            "1",
                            "ass-events",
                            "{\\an7\\pos(40,40)\\fs48\\1c&H1673F9&\\b1}KÜTRAL · osd-overlay sobre el video",
                            "1280",
                            "720",
                            "0",
                            "no",
                            "no",
                        ],
                    );
                    // Captura de mpv (lo que decodifica, sin el escritorio).
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let _ = mpv.command("screenshot", &["subtitles"]);
                    eprintln!("[spike] captura de mpv pedida");
                }
                Ok(Event::ClientMessage(args)) if args.first() == Some(&"kutral-esc") => {
                    eprintln!("[spike] Esc recibido por mpv → oculto el video");
                    let _ = mpv.set_property("pause", true);
                    unsafe {
                        ShowWindow(hijo_n as HWND, SW_HIDE);
                    }
                    let app = app.clone();
                    let _ = app.clone().run_on_main_thread(move || {
                        if let Some(w) = tauri::Manager::get_webview_window(&app, "main") {
                            let _ = w.set_focus();
                            eprintln!("[spike] foco devuelto a la interfaz");
                        }
                    });
                }
                Ok(Event::EndFile(r)) => eprintln!("[spike] fin de archivo: {r:?}"),
                Ok(Event::Shutdown) => break,
                Err(e) => eprintln!("[spike] evento con error: {e}"),
                _ => {}
            }
        }
    });
}
