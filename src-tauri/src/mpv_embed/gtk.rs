// mpv_embed/gtk.rs — Superficie de Linux: el video de libmpv en un GtkGLArea
// dentro del MISMO toplevel del webview (una sola ventana), con la Render API
// (OpenGL). Recibe teclado y mouse de GTK y se los pasa al núcleo (mod.rs).
//
// Hilos: el RenderContext y el GtkGLArea viven SOLO en el hilo main (GTK), en
// un thread_local. libmpv avisa "frame listo" desde cualquier hilo (update
// callback) → se reenvía al hilo main por un glib channel que hace
// queue_render.

use std::cell::{Cell, RefCell};
use std::ffi::{c_void, CString};
use std::ptr;
use std::rc::Rc;
use std::sync::OnceLock;

use gtk::prelude::*;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

/// glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING) → el FBO al que dibuja el GLArea.
const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// Resolvedores de punteros GL REALES del driver (no los trampolines de
/// libepoxy, que crashean/cuelgan al llamarse desde mpv en Arch): eglGetProc
/// (Wayland/EGL) y glXGetProcAddressARB (X11/GLX). Se prueba EGL y luego GLX.
type GetProcFn = unsafe extern "C" fn(*const std::os::raw::c_char) -> *mut c_void;
static EGL_GET_PROC: OnceLock<Option<GetProcFn>> = OnceLock::new();
static GLX_GET_PROC: OnceLock<Option<GetProcFn>> = OnceLock::new();

type GlGetIntegervFn = unsafe extern "C" fn(u32, *mut i32);
static GL_GET_INTEGERV: OnceLock<GlGetIntegervFn> = OnceLock::new();

thread_local! {
    /// Superficie GTK — SOLO hilo main.
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
}

struct Surface {
    glarea: gtk::GLArea,
    /// El webview (hijo original del box). Lo ocultamos al reproducir y lo
    /// mostramos al volver a menús (no lo reparentamos: eso lo rompe).
    webview: Option<gtk::Widget>,
    /// RenderContext de mpv; se crea en "realize" (cuando hay GL current).
    /// Mantiene viva una referencia fuerte (las closures tienen otras clones).
    #[allow(dead_code)]
    render: Rc<RefCell<Option<RenderContext<'static>>>>,
}

// ──────────────────── resolución de GL (driver real, vía EGL/GLX) ──────────

/// Carga libEGL/libGL una vez y cachea eglGetProcAddress y glXGetProcAddressARB.
/// Devuelven punteros a las funciones GL REALES del driver para el contexto
/// current (lo que mpv necesita), evitando los trampolines de libepoxy.
fn ensure_gl_loaders() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // EGL (Wayland y también X11 moderno).
        let egl = unsafe { libloading::Library::new("libEGL.so.1") }
            .ok()
            .and_then(|lib| {
                let lib: &'static libloading::Library = Box::leak(Box::new(lib));
                unsafe {
                    lib.get::<GetProcFn>(b"eglGetProcAddress\0")
                        .ok()
                        .map(|s| *s)
                }
            });
        let _ = EGL_GET_PROC.set(egl);

        // GLX (X11 clásico) como fallback.
        let glx = unsafe { libloading::Library::new("libGL.so.1") }
            .ok()
            .and_then(|lib| {
                let lib: &'static libloading::Library = Box::leak(Box::new(lib));
                unsafe {
                    lib.get::<GetProcFn>(b"glXGetProcAddressARB\0")
                        .ok()
                        .map(|s| *s)
                }
            });
        let _ = GLX_GET_PROC.set(glx);

        eprintln!(
            "[mpv-embed] GL loaders: egl={} glx={}",
            EGL_GET_PROC.get().map(|o| o.is_some()).unwrap_or(false),
            GLX_GET_PROC.get().map(|o| o.is_some()).unwrap_or(false),
        );
    });
}

/// get_proc_address que pide libmpv: resuelve `name` a la función GL real del
/// driver. Prueba EGL y luego GLX. Plain fn porque OpenGLInitParams exige `fn`.
fn gl_get_proc(_ctx: &(), name: &str) -> *mut c_void {
    let Ok(cname) = CString::new(name) else {
        return ptr::null_mut();
    };
    for slot in [EGL_GET_PROC.get(), GLX_GET_PROC.get()] {
        if let Some(Some(getproc)) = slot {
            let p = unsafe { getproc(cname.as_ptr()) };
            if !p.is_null() {
                return p;
            }
        }
    }
    ptr::null_mut()
}

// ───────────────────────── helpers de input (uosc) ────────────────────────

// ── Auto-ocultar el cursor durante la reproducción ──
thread_local! {
    static CURSOR_GEN: Cell<u64> = const { Cell::new(0) };
}

fn set_cursor_hidden(area: &gtk::GLArea, hidden: bool) {
    if let Some(win) = area.window() {
        let cursor = if hidden {
            gtk::gdk::Cursor::from_name(&win.display(), "none")
        } else {
            None // None = cursor por defecto (visible)
        };
        win.set_cursor(cursor.as_ref());
    }
}

/// Muestra el cursor y programa ocultarlo tras 1.5s sin movimiento (generación:
/// cada movimiento invalida el timer anterior sin tener que cancelarlo).
fn cursor_activity(area: &gtk::GLArea) {
    set_cursor_hidden(area, false);
    let gen = CURSOR_GEN.with(|g| {
        let n = g.get().wrapping_add(1);
        g.set(n);
        n
    });
    let area = area.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
        if CURSOR_GEN.with(|g| g.get()) == gen {
            set_cursor_hidden(&area, true);
        }
        glib::ControlFlow::Break
    });
}

// ───────────────────────── interfaz con el núcleo ─────────────────────────

/// Antes de crear mpv (hilo main).
pub(super) fn preparar(_window: &tauri::WebviewWindow) -> Result<(), String> {
    ensure_gl_loaders();
    // libmpv exige LC_NUMERIC="C" (decimal con punto). GTK ya fijó el locale del
    // usuario (es_CL → coma) y eso hace que mpv_create() devuelva NULL. Reseteamos
    // SOLO esa categoría, sin tocar el resto del locale de la UI.
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }
    Ok(())
}

/// Opciones de mpv propias de esta superficie.
pub(super) fn opciones(init: &libmpv2::MpvInitializer) -> libmpv2::Result<()> {
    // vo=libmpv activa la Render API.
    init.set_option("vo", "libmpv")?;
    // CLAVE: sin esto, render() bloquea el hilo main hasta el tiempo de
    // display de cada frame (la doc de la Render API lo dice) → con red
    // lenta la UI se cuelga. En 0 no bloquea; el ritmo lo da queue_render.
    init.set_option("video-timing-offset", "0")?;
    Ok(())
}

/// Con mpv ya creado: arma la superficie (hilo main).
pub(super) fn montar(window: &tauri::WebviewWindow) -> Result<(), String> {
    let vbox = window.default_vbox().map_err(|e| format!("default_vbox: {e}"))?;
    build_surface(&vbox);
    Ok(())
}

/// Llama `f` cada `ms` en el hilo main mientras devuelva true.
pub(super) fn cada(ms: u64, mut f: impl FnMut() -> bool + 'static) {
    glib::timeout_add_local(std::time::Duration::from_millis(ms), move || {
        if f() {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

/// Oculta el webview y muestra el video (hilo main).
pub(super) fn mostrar() {
    SURFACE.with(|s| {
        if let Some(surf) = s.borrow().as_ref() {
            if let Some(wv) = &surf.webview {
                wv.hide();
            }
            surf.glarea.show();
            surf.glarea.grab_focus();
            surf.glarea.queue_render();
            set_cursor_hidden(&surf.glarea, true); // arranca sin cursor
        }
    });
}

/// Oculta el video y vuelve a mostrar el webview (menús), con el teclado.
pub(super) fn ocultar() {
    SURFACE.with(|s| {
        if let Some(surf) = s.borrow().as_ref() {
            set_cursor_hidden(&surf.glarea, false); // restaura cursor
            surf.glarea.hide();
            if let Some(wv) = &surf.webview {
                wv.show();
                wv.grab_focus(); // teclado vuelve a navegar la UI
            }
        }
    });
}

/// Tamaño físico del GLArea (px reales = lógico × scale). Respaldo cuando mpv
/// aún no reporta OSD (antes del primer frame).
pub(super) fn tamano_px() -> (f64, f64) {
    SURFACE
        .with(|s| {
            s.borrow().as_ref().map(|surf| {
                let sc = surf.glarea.scale_factor() as f64;
                (
                    surf.glarea.allocated_width().max(1) as f64 * sc,
                    surf.glarea.allocated_height().max(1) as f64 * sc,
                )
            })
        })
        .unwrap_or((1280.0, 720.0))
}

/// Pasa un punto del widget (coords lógicas GTK) a píxeles del OSD, que es el
/// sistema en el que dibujamos: proporción sobre el área real, sin suponer
/// ninguna relación de aspecto.
fn widget_to_osd(area: &gtk::GLArea, x: f64, y: f64) -> (f64, f64) {
    let o = super::osd();
    let w = area.allocated_width().max(1) as f64;
    let h = area.allocated_height().max(1) as f64;
    (x / w * o.w, y / h * o.h)
}

/// Traduce una tecla GDK al nombre que entiende mpv (LEFT, SPACE, ESC, "m"…),
/// con prefijos de modificador (Ctrl+/Alt+). Devuelve None si no se mapea.
fn gdk_to_mpv_key(ev: &gtk::gdk::EventKey) -> Option<String> {
    let gname = ev.keyval().name()?;
    let base: String = match gname.as_str() {
        "Left" => "LEFT".into(),
        "Right" => "RIGHT".into(),
        "Up" => "UP".into(),
        "Down" => "DOWN".into(),
        "space" => "SPACE".into(),
        "Return" | "KP_Enter" => "ENTER".into(),
        "Tab" => "TAB".into(),
        "Page_Up" => "PGUP".into(),
        "Page_Down" => "PGDWN".into(),
        "Home" => "HOME".into(),
        "End" => "END".into(),
        "Delete" => "DEL".into(),
        "Insert" => "INS".into(),
        other => {
            // Char imprimible único (a, M, 1, [, ]…) o Fn → tal cual.
            let is_fn = other.len() >= 2
                && other.starts_with('F')
                && other[1..].chars().all(|c| c.is_ascii_digit());
            if other.chars().count() == 1 || is_fn {
                other.to_string()
            } else {
                return None;
            }
        }
    };
    let st = ev.state();
    let mut out = String::new();
    if st.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        out.push_str("Ctrl+");
    }
    if st.contains(gtk::gdk::ModifierType::MOD1_MASK) {
        out.push_str("Alt+");
    }
    out.push_str(&base);
    Some(out)
}

/// Puntero a glGetIntegerv (cacheado) para leer el FBO destino del GLArea.
fn gl_get_integerv() -> Option<GlGetIntegervFn> {
    if let Some(f) = GL_GET_INTEGERV.get() {
        return Some(*f);
    }
    let p = gl_get_proc(&(), "glGetIntegerv");
    if p.is_null() {
        return None;
    }
    let f: GlGetIntegervFn = unsafe { std::mem::transmute(p) };
    let _ = GL_GET_INTEGERV.set(f);
    Some(f)
}

/// Mete el GtkGLArea como segundo hijo del MISMO GtkBox del webview y alterna
/// visibilidad (no reparenta nada).
///
/// CLAVE: Tauri (con decorations:false) tiene un resize-handler que EXIGE la
/// jerarquía `webview → GtkBox → GtkWindow` (hace `webview.parent().parent()`
/// y downcast a Window). Cualquier reparent (mover el webview, o insertar un
/// Overlay entre box y window) revienta ese downcast → panic. Por eso NO
/// tocamos la jerarquía: solo añadimos el GLArea al box y mostramos uno u otro.
#[allow(deprecated)] // glib::MainContext::channel está deprecado pero vigente en 0.18
fn build_surface(vbox: &gtk::Box) {
    // El webview es el hijo actual del box (lo capturamos ANTES de añadir nada).
    let webview: Option<gtk::Widget> = vbox.children().into_iter().next();

    let glarea = gtk::GLArea::new();
    glarea.set_hexpand(true);
    glarea.set_vexpand(true);
    glarea.set_auto_render(true);
    glarea.set_use_es(false);
    glarea.set_has_stencil_buffer(true);

    let render: Rc<RefCell<Option<RenderContext<'static>>>> = Rc::new(RefCell::new(None));

    // Canal: update callback (cualquier hilo) → queue_render (hilo main).
    let (tx, rx) = glib::MainContext::channel::<()>(glib::Priority::DEFAULT);
    {
        let ga = glarea.clone();
        rx.attach(None, move |_| {
            ga.queue_render();
            glib::ControlFlow::Continue
        });
    }

    // realize: crea el RenderContext de mpv EAGER, al arrancar (forzamos
    // realize() abajo), ANTES de cualquier loadfile. Si se crea perezoso tras
    // loadfile hay deadlock: mpv (vo=libmpv) espera el render context ↔ el
    // render espera a mpv. Aquí el contexto GL ya es current (make_current).
    {
        let render = render.clone();
        let tx = tx.clone();
        glarea.connect_realize(move |area| {
            area.make_current();
            if let Some(err) = area.error() {
                eprintln!("[mpv-embed] GLArea realize error: {err}");
                return;
            }
            let Some(mpv) = super::MPV.get() else { return };
            let params = [
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address: gl_get_proc,
                    ctx: (),
                }),
            ];
            match mpv.create_render_context(params) {
                Ok(mut ctx) => {
                    let tx = tx.clone();
                    ctx.set_update_callback(move || {
                        let _ = tx.send(());
                    });
                    *render.borrow_mut() = Some(ctx);
                    eprintln!("[mpv-embed] render context creado");
                }
                Err(e) => eprintln!("[mpv-embed] create_render_context: {e}"),
            }
        });
    }

    // render: dibuja el frame de mpv en el FBO del GLArea (px reales = lógico *
    // scale → arregla HiDPI). El contexto ya existe (creado en realize).
    {
        let render = render.clone();
        glarea.connect_render(move |area, _gl| {
            let scale = area.scale_factor();
            let w = (area.allocated_width() * scale).max(1);
            let h = (area.allocated_height() * scale).max(1);
            let mut fbo: i32 = 0;
            if let Some(get) = gl_get_integerv() {
                unsafe { get(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
            }
            if let Some(ctx) = render.borrow().as_ref() {
                let _ = ctx.render::<()>(fbo, w, h, true);
            }
            glib::Propagation::Proceed
        });
    }

    // ── Forward de input a mpv/uosc ──
    // Con la Render API mpv NO recibe eventos del SO: se los pasamos nosotros
    // desde el GLArea para que uosc se esconda/expanda, haga seek en la
    // timeline, abra menús, etc. Coordenadas en px reales (× scale) = espacio
    // OSD de mpv (igual que el FBO que renderizamos).
    glarea.set_can_focus(true);
    glarea.add_events(
        gtk::gdk::EventMask::POINTER_MOTION_MASK
            | gtk::gdk::EventMask::BUTTON_PRESS_MASK
            | gtk::gdk::EventMask::BUTTON_RELEASE_MASK
            | gtk::gdk::EventMask::SCROLL_MASK
            | gtk::gdk::EventMask::LEAVE_NOTIFY_MASK
            | gtk::gdk::EventMask::KEY_PRESS_MASK
            | gtk::gdk::EventMask::KEY_RELEASE_MASK,
    );
    // Teclado: la app es teclado-first y durante la reproducción el foco lo
    // tiene el GLArea (no el webview). La lógica es del núcleo (`tecla`);
    // ESC/Atrás NO van a mpv (quit mataría libmpv).
    glarea.connect_key_press_event(|_area, ev| {
        let kn = ev.keyval().name();
        super::tecla(kn.as_deref().unwrap_or(""), gdk_to_mpv_key(ev));
        glib::Propagation::Stop
    });
    glarea.connect_motion_notify_event(|area, ev| {
        let (x, y) = ev.position();
        let (cx, cy) = widget_to_osd(area, x, y);
        cursor_activity(area); // muestra cursor y reprograma su autohide
        super::raton_mueve(cx, cy);
        glib::Propagation::Stop
    });
    glarea.connect_button_press_event(|area, ev| {
        area.grab_focus();
        if ev.button() == 1 {
            let (x, y) = ev.position();
            let (cx, cy) = widget_to_osd(area, x, y);
            super::raton_clic(cx, cy);
        }
        glib::Propagation::Stop
    });
    glarea.connect_scroll_event(|_area, ev| {
        match ev.direction() {
            gtk::gdk::ScrollDirection::Up => super::rueda(true),
            gtk::gdk::ScrollDirection::Down => super::rueda(false),
            _ => {}
        }
        glib::Propagation::Stop
    });
    glarea.connect_leave_notify_event(|_area, _ev| {
        super::raton_sale();
        glib::Propagation::Proceed
    });
    // Diferido: el resize corre dentro del render.
    glarea.connect_resize(|_area, _w, _h| {
        glib::idle_add_local_once(super::redimensionado);
    });

    // Segundo hijo del box, expandido. Solo uno (webview o glarea) visible a la
    // vez → el visible ocupa todo el box.
    vbox.pack_start(&glarea, true, true, 0);
    // Forzar realize YA (sin mostrar) para crear el render context al arranque,
    // antes del primer loadfile → evita el deadlock. El window ya está realizado.
    glarea.realize();
    glarea.hide(); // realizado pero oculto; se muestra al reproducir

    SURFACE.with(|s| {
        *s.borrow_mut() = Some(Surface { glarea, webview, render });
    });
}
