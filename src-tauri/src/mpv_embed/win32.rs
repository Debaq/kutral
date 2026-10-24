// mpv_embed/win32.rs — Superficie de Windows: el video de libmpv en una
// ventana hija de la de Kütral (`wid`), encima de WebView2. Una sola ventana,
// como en Linux.
//
// Probado en el spike (CI de Windows, con capturas): mpv dibuja solo en la
// hija (d3d11), el osd-overlay se ve encima, y el input tiene que llegar a
// NUESTRA ventana, no a la de mpv: con un binding de Esc en mpv no llegaba
// nunca (mpv no tiene el foco). Por eso mpv deja pasar el puntero
// (`input-cursor-passthrough`) y no lee el teclado (`input-vo-keyboard=no`):
// la hija recibe todo en el hilo principal, como el GLArea en Linux, y se lo
// pasa al núcleo. Las teclas que no son nuestras van a mpv con `keypress`.
//
// Los temporizadores son SetTimer sobre la hija (también hilo principal).

use std::cell::{Cell, RefCell};
use std::os::windows::ffi::OsStrExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_BACK, VK_CONTROL,
    VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, KillTimer, LoadCursorW, RegisterClassW,
    SetCursor, SetTimer, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOP, IDC_ARROW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_CHAR, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_SETCURSOR, WM_SIZE, WM_TIMER,
    WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
};

/// windows-sys no lo trae en WindowsAndMessaging (lo manda TrackMouseEvent).
const WM_MOUSELEAVE: u32 = 0x02A3;

/// La ventana hija (HWND como número: el puntero no es Send).
static HIJO: AtomicUsize = AtomicUsize::new(0);

fn hijo() -> HWND {
    HIJO.load(Ordering::SeqCst) as HWND
}

thread_local! {
    /// Temporizadores vivos: id de SetTimer → función (ver `cada`).
    static TIMERS: RefCell<HashMap<usize, Box<dyn FnMut() -> bool>>> = RefCell::new(HashMap::new());
    static PROX_TIMER: Cell<usize> = const { Cell::new(1) };
    /// Cursor oculto mientras se reproduce (se muestra al mover el mouse).
    static CURSOR_OCULTO: Cell<bool> = const { Cell::new(false) };
    static CURSOR_GEN: Cell<u64> = const { Cell::new(0) };
    /// Ya se pidió WM_MOUSELEAVE (hay que pedirlo de nuevo tras cada salida).
    static SIGUIENDO_MOUSE: Cell<bool> = const { Cell::new(false) };
    /// Último punto de WM_MOUSEMOVE (lParam crudo). Windows manda movimientos
    /// sintéticos sin que el mouse se mueva (cambia algo bajo el cursor, se
    /// oculta el puntero…): sin comparar, la barra aparecía sola cada tanto.
    static ULTIMO_MOUSE: Cell<Option<LPARAM>> = const { Cell::new(None) };
    static VISIBLE: Cell<bool> = const { Cell::new(false) };
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ───────────────────────── interfaz con el núcleo ─────────────────────────

/// libmpv-2.dll va enlazado en diferido (ver build.rs): hay que cargarlo de
/// vendor/mpv-dev/ antes de la primera llamada a mpv. Ya cargado, el
/// cargador diferido lo encuentra por nombre.
fn cargar_libmpv(window: &tauri::WebviewWindow) -> Result<(), String> {
    use tauri::Manager;
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = window.app_handle().path().resource_dir() {
        cands.push(res.join("vendor").join("mpv-dev").join("libmpv-2.dll"));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        cands.push(dir.join("libmpv-2.dll"));
        cands.push(dir.join("vendor").join("mpv-dev").join("libmpv-2.dll"));
    }
    cands.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("vendor")
            .join("mpv-dev")
            .join("libmpv-2.dll"),
    );
    for c in &cands {
        if !c.exists() {
            continue;
        }
        let ruta: Vec<u16> = c.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        if !unsafe { LoadLibraryW(ruta.as_ptr()) }.is_null() {
            eprintln!("[mpv-embed] libmpv: {}", c.display());
            return Ok(());
        }
        eprintln!("[mpv-embed] libmpv no cargó: {}", c.display());
    }
    Err(format!("no encontré libmpv-2.dll (busqué en {cands:?})"))
}

/// Antes de crear mpv (hilo main): carga libmpv, y la hija tiene que existir
/// para pasarle su HWND a mpv como `wid`.
pub(super) fn preparar(window: &tauri::WebviewWindow) -> Result<(), String> {
    cargar_libmpv(window)?;
    let padre = window.hwnd().map_err(|e| format!("hwnd: {e}"))?.0 as HWND;
    let h = unsafe {
        let clase = w("KutralVideo");
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
            lpszClassName: clase.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(padre, &mut rc);
        // Sin WS_VISIBLE: se muestra al reproducir.
        CreateWindowExW(
            0,
            clase.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            rc.right,
            rc.bottom,
            padre,
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        )
    };
    if h.is_null() {
        return Err("no se pudo crear la ventana del video".into());
    }
    HIJO.store(h as usize, Ordering::SeqCst);

    window.on_window_event(|ev| match ev {
        // La hija sigue al área cliente (maximizar, pantalla completa).
        tauri::WindowEvent::Resized(t) => unsafe {
            SetWindowPos(hijo(), HWND_TOP, 0, 0, t.width as i32, t.height as i32, SWP_NOACTIVATE);
        },
        // Al volver a Kütral (Alt+Tab), Windows le devuelve el foco al
        // webview aunque el video esté encima: el teclado tiene que ir al
        // video.
        tauri::WindowEvent::Focused(true) if VISIBLE.with(|v| v.get()) => unsafe {
            SetFocus(hijo());
        },
        _ => {}
    });
    Ok(())
}

/// Opciones de mpv propias de esta superficie (ver arriba por qué el input).
pub(super) fn opciones(init: &libmpv2::MpvInitializer) -> libmpv2::Result<()> {
    init.set_option("wid", HIJO.load(Ordering::SeqCst) as i64)?;
    init.set_option("input-vo-keyboard", "no")?;
    init.set_option("input-cursor-passthrough", "yes")?;
    // La barra y los menús: sin esto libass cae a Arial (Linux usa la sans
    // del sistema, Noto).
    init.set_option("osd-font", "Segoe UI")?;
    Ok(())
}

/// Con mpv ya creado. Nada más que hacer: la hija se armó en `preparar`.
pub(super) fn montar(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

/// Llama `f` cada `ms` en el hilo main mientras devuelva true.
pub(super) fn cada(ms: u64, f: impl FnMut() -> bool + 'static) {
    if hijo().is_null() {
        return; // CreateWindowExW manda WM_SIZE antes de guardar la hija
    }
    let id = PROX_TIMER.with(|p| {
        let id = p.get();
        p.set(id + 1);
        id
    });
    TIMERS.with(|t| t.borrow_mut().insert(id, Box::new(f)));
    unsafe {
        SetTimer(hijo(), id, ms as u32, None);
    }
}

/// Muestra el video encima de la interfaz, con el teclado (hilo main).
pub(super) fn mostrar() {
    VISIBLE.with(|v| v.set(true));
    CURSOR_OCULTO.with(|c| c.set(true)); // arranca sin cursor
    unsafe {
        SetWindowPos(hijo(), HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        ShowWindow(hijo(), SW_SHOW);
        SetFocus(hijo());
    }
    // WebView2 debajo del video seguía dibujando (animaciones, sondeos) y
    // Windows componía las dos capas en cada cuadro: le quitaba GPU al video.
    // Oculto deja de pintar, igual que el webview en Linux (gtk.rs). Los
    // timers del front (cacheWatch, historial) siguen a su ritmo porque
    // `additionalBrowserArgs` de tauri.conf.json apaga el estrangulamiento de
    // Chromium para páginas ocultas (sin eso, a los 5 min: uno por minuto).
    if let Some(win) = super::APP.get().and_then(|a| tauri::Manager::get_webview_window(a, "main")) {
        let wv: &tauri::Webview = win.as_ref();
        let _ = wv.hide();
    }
}

/// Oculta el video; el teclado vuelve a la interfaz (hilo main).
pub(super) fn ocultar() {
    VISIBLE.with(|v| v.set(false));
    CURSOR_OCULTO.with(|c| c.set(false));
    unsafe {
        ShowWindow(hijo(), SW_HIDE);
    }
    if let Some(win) = super::APP.get().and_then(|a| tauri::Manager::get_webview_window(a, "main")) {
        let wv: &tauri::Webview = win.as_ref();
        let _ = wv.show();
        let _ = win.set_focus();
        let _ = wv.set_focus();
    }
}

/// Tamaño del área del video en píxeles reales.
pub(super) fn tamano_px() -> (f64, f64) {
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetClientRect(hijo(), &mut rc);
    }
    if rc.right < 2 || rc.bottom < 2 {
        return (1280.0, 720.0);
    }
    (rc.right as f64, rc.bottom as f64)
}

// ─────────────────────────────── input ────────────────────────────────────

/// Punto del área cliente → píxeles del OSD (mismo criterio que en GTK).
fn a_osd(l: LPARAM) -> (f64, f64) {
    let (x, y) = ((l & 0xffff) as i16 as f64, ((l >> 16) & 0xffff) as i16 as f64);
    let (w, h) = tamano_px();
    let o = super::osd();
    (x / w * o.w, y / h * o.h)
}

fn apretada(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}

/// Teclas especiales: (nombre GDK que usa el núcleo, nombre de mpv). Las de
/// texto llegan por WM_CHAR.
fn tecla_especial(vk: u16) -> Option<(String, String)> {
    let par = |g: &str, m: &str| Some((g.to_string(), m.to_string()));
    match vk {
        VK_LEFT => par("Left", "LEFT"),
        VK_RIGHT => par("Right", "RIGHT"),
        VK_UP => par("Up", "UP"),
        VK_DOWN => par("Down", "DOWN"),
        VK_RETURN => par("Return", "ENTER"),
        VK_ESCAPE => par("Escape", "ESC"),
        VK_BACK => par("BackSpace", "BS"),
        VK_SPACE => par("space", "SPACE"),
        VK_TAB => par("Tab", "TAB"),
        VK_PRIOR => par("Page_Up", "PGUP"),
        VK_NEXT => par("Page_Down", "PGDWN"),
        VK_HOME => par("Home", "HOME"),
        VK_END => par("End", "END"),
        VK_DELETE => par("Delete", "DEL"),
        VK_INSERT => par("Insert", "INS"),
        f if (VK_F1..=VK_F12).contains(&f) => {
            let n = format!("F{}", f - VK_F1 + 1);
            Some((n.clone(), n))
        }
        _ => None,
    }
}

fn mostrar_cursor() {
    CURSOR_OCULTO.with(|c| c.set(false));
    let gen = CURSOR_GEN.with(|g| {
        let n = g.get().wrapping_add(1);
        g.set(n);
        n
    });
    // Se oculta tras 1.5 s sin movimiento (cada movimiento invalida al anterior).
    cada(1500, move || {
        if CURSOR_GEN.with(|g| g.get()) == gen && VISIBLE.with(|v| v.get()) {
            CURSOR_OCULTO.with(|c| c.set(true));
            unsafe {
                SetCursor(std::ptr::null_mut());
            }
        }
        false
    });
}

/// Hilo principal (como las señales del GLArea en Linux).
unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_TIMER => {
            // Se saca del mapa mientras corre: puede programar otros.
            let id = wp;
            if let Some(mut f) = TIMERS.with(|t| t.borrow_mut().remove(&id)) {
                if f() {
                    TIMERS.with(|t| t.borrow_mut().insert(id, f));
                } else {
                    KillTimer(h, id);
                }
            } else {
                KillTimer(h, id);
            }
            return 0;
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            if let Some((kn, mpv)) = tecla_especial(vk) {
                super::tecla(&kn, Some(mpv));
                return 0;
            }
            // Ctrl/Alt + letra o número: WM_CHAR no trae la letra.
            let (ctrl, alt) = (apretada(VK_CONTROL), apretada(VK_MENU));
            if (ctrl || alt) && (vk as u8).is_ascii_alphanumeric() {
                let c = (vk as u8 as char).to_ascii_lowercase();
                let mut mpv = String::new();
                if ctrl {
                    mpv.push_str("Ctrl+");
                }
                if alt {
                    mpv.push_str("Alt+");
                }
                mpv.push(c);
                super::tecla("", Some(mpv));
                return 0;
            }
        }
        WM_CHAR => {
            // Texto (c, a, v, m, z, x, [, ]…). Las de control y el espacio
            // ya pasaron por WM_KEYDOWN.
            if let Some(c) = char::from_u32(wp as u32).filter(|c| !c.is_control() && *c != ' ') {
                let s = c.to_string();
                super::tecla(&s, Some(s.clone()));
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            if ULTIMO_MOUSE.with(|u| u.replace(Some(lp))) == Some(lp) {
                return 0;
            }
            if !SIGUIENDO_MOUSE.with(|s| s.replace(true)) {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut t);
            }
            mostrar_cursor();
            let (x, y) = a_osd(lp);
            super::raton_mueve(x, y);
            return 0;
        }
        WM_MOUSELEAVE => {
            SIGUIENDO_MOUSE.with(|s| s.set(false));
            // Volver a entrar por el mismo punto sí es movimiento.
            ULTIMO_MOUSE.with(|u| u.set(None));
            super::raton_sale();
            return 0;
        }
        WM_LBUTTONDOWN => {
            SetFocus(h);
            let (x, y) = a_osd(lp);
            super::raton_clic(x, y);
            return 0;
        }
        // Clic derecho = Volver, igual que en el resto de Kütral.
        WM_RBUTTONDOWN => {
            SetFocus(h);
            super::tecla("Escape", None);
            return 0;
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xffff) as i16;
            super::rueda(delta > 0);
            return 0;
        }
        WM_SETCURSOR if CURSOR_OCULTO.with(|c| c.get()) => {
            SetCursor(std::ptr::null_mut());
            return 1;
        }
        // Diferido: mpv todavía no actualizó osd-width/height.
        WM_SIZE => cada(50, || {
            super::redimensionado();
            false
        }),
        _ => {}
    }
    DefWindowProcW(h, m, wp, lp)
}
