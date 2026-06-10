// mpv_embed.rs — Reproductor libmpv embebido (Linux).
//
// En vez de spawnear mpv como proceso aparte (dos ventanas, sin embedding real
// bajo Wayland), linkeamos libmpv y renderizamos su salida con la Render API
// (OpenGL) dentro de un GtkGLArea que vive en el MISMO toplevel del webview.
// Resultado: una sola ventana. uosc / subs / decoders intactos (es mpv full).
//
// Hilos:
//   - El handle `Mpv` es Send+Sync → los comandos Tauri (hilos worker) llaman
//     command()/get_property() directo, sin IPC.
//   - El RenderContext y el GtkGLArea viven SOLO en el hilo main (GTK); se
//     guardan en un thread_local del main. Mostrar/ocultar la superficie desde
//     un comando salta al hilo main vía AppHandle::run_on_main_thread.
//   - libmpv avisa "frame listo" desde cualquier hilo (update callback) → se
//     reenvía al hilo main por un glib channel que hace queue_render.

#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use gtk::prelude::*;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;

/// glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING) → el FBO al que dibuja el GLArea.
const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// Handle mpv global (vive todo el proceso). Send+Sync → comandos desde
/// cualquier hilo.
static MPV: OnceLock<Mpv> = OnceLock::new();
/// AppHandle para saltar al hilo main (mostrar/ocultar superficie, emitir).
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// Resolvedores de punteros GL REALES del driver (no los trampolines de
/// libepoxy, que crashean/cuelgan al llamarse desde mpv en Arch): eglGetProc
/// (Wayland/EGL) y glXGetProcAddressARB (X11/GLX). Se prueba EGL y luego GLX.
type GetProcFn = unsafe extern "C" fn(*const std::os::raw::c_char) -> *mut c_void;
static EGL_GET_PROC: OnceLock<Option<GetProcFn>> = OnceLock::new();
static GLX_GET_PROC: OnceLock<Option<GetProcFn>> = OnceLock::new();
/// ¿Hay algo cargado/reproduciéndose? (equivalente al "alive" del proceso).
static RUNNING: AtomicBool = AtomicBool::new(false);

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

/// Manda la posición del mouse a mpv en coords OSD (px reales = lógico×scale).
fn mpv_mouse_move(x: f64, y: f64, scale: i32) {
    if let Some(mpv) = MPV.get() {
        let xs = ((x * scale as f64).round() as i64).to_string();
        let ys = ((y * scale as f64).round() as i64).to_string();
        let _ = mpv.command("mouse", &[xs.as_str(), ys.as_str()]);
    }
}

/// Nombre de botón mpv para un botón GDK (1=izq, 2=medio, 3=der).
fn mpv_btn_name(button: u32) -> Option<&'static str> {
    match button {
        1 => Some("MBTN_LEFT"),
        2 => Some("MBTN_MID"),
        3 => Some("MBTN_RIGHT"),
        _ => None,
    }
}

/// Manda un evento de tecla/botón a mpv (action = keydown/keyup/keypress).
fn mpv_key(action: &str, key: &str) {
    if let Some(mpv) = MPV.get() {
        let _ = mpv.command(action, &[key]);
    }
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

// ───────────────────────────── init / superficie ──────────────────────────

/// Dir de config embebido (uosc + mpv.conf + input.conf), igual que el player
/// de proceso. Se resuelve relativo a resource_dir / manifest / exe.
fn config_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join("mpv-config"));
    }
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join("mpv-config"));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join("mpv-config"));
        }
    }
    cands.into_iter().find(|c| c.exists())
}

/// Inicializa el reproductor embebido. Debe llamarse en el hilo main (setup()).
/// Crea el handle mpv, reparenta el webview en un Overlay y mete el GtkGLArea.
pub fn init(app: &tauri::AppHandle, window: &tauri::WebviewWindow) -> Result<(), String> {
    ensure_gl_loaders();

    // libmpv exige LC_NUMERIC="C" (decimal con punto). GTK ya fijó el locale del
    // usuario (es_CL → coma) y eso hace que mpv_create() devuelva NULL. Reseteamos
    // SOLO esa categoría, sin tocar el resto del locale de la UI.
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }

    // mpv: opciones ANTES de init. vo=libmpv activa la Render API; config=yes +
    // config-dir cargan mpv.conf y los scripts (uosc).
    let cfg = config_dir(app);
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vo", "libmpv")?;
        init.set_option("config", "yes")?;
        if let Some(dir) = &cfg {
            init.set_option("config-dir", dir.to_string_lossy().as_ref())?;
        }
        init.set_option("terminal", "no")?;
        init.set_option("force-window", "no")?;
        // El idle mantiene mpv vivo entre archivos sin cerrar el render context.
        init.set_option("idle", "yes")?;
        // CLAVE: sin esto, render() bloquea el hilo main hasta el tiempo de
        // display de cada frame (la doc de la Render API lo dice) → con red
        // lenta la UI se cuelga. En 0 no bloquea; el ritmo lo da queue_render.
        init.set_option("video-timing-offset", "0")?;
        Ok(())
    })
    .map_err(|e| format!("mpv init: {e}"))?;

    MPV.set(mpv).map_err(|_| "mpv ya inicializado".to_string())?;
    let _ = APP.set(app.clone());

    let vbox = window.default_vbox().map_err(|e| format!("default_vbox: {e}"))?;
    build_surface(&vbox);
    eprintln!("[mpv-embed] inicializado (cfg={:?})", cfg);
    Ok(())
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
            let Some(mpv) = MPV.get() else { return };
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
    // tiene el GLArea (no el webview), así que reenviamos las teclas a mpv para
    // que uosc/input.conf respondan. ESC/Atrás NO van a mpv (quit mataría
    // libmpv): cierran el video y vuelven a menús.
    glarea.connect_key_press_event(|_area, ev| {
        let name = ev.keyval().name();
        let name = name.as_deref().unwrap_or("");
        if matches!(name, "Escape" | "BackSpace") {
            let _ = stop();
            return glib::Propagation::Stop;
        }
        if let Some(k) = gdk_to_mpv_key(ev) {
            mpv_key("keypress", &k);
        }
        glib::Propagation::Stop
    });
    glarea.connect_motion_notify_event(|area, ev| {
        let (x, y) = ev.position();
        mpv_mouse_move(x, y, area.scale_factor());
        glib::Propagation::Proceed
    });
    glarea.connect_button_press_event(|area, ev| {
        let (x, y) = ev.position();
        mpv_mouse_move(x, y, area.scale_factor());
        if let Some(btn) = mpv_btn_name(ev.button()) {
            mpv_key("keydown", btn);
        }
        area.grab_focus();
        glib::Propagation::Proceed
    });
    glarea.connect_button_release_event(|_area, ev| {
        if let Some(btn) = mpv_btn_name(ev.button()) {
            mpv_key("keyup", btn);
        }
        glib::Propagation::Proceed
    });
    glarea.connect_scroll_event(|_area, ev| {
        let key = match ev.direction() {
            gtk::gdk::ScrollDirection::Up => Some("WHEEL_UP"),
            gtk::gdk::ScrollDirection::Down => Some("WHEEL_DOWN"),
            gtk::gdk::ScrollDirection::Left => Some("WHEEL_LEFT"),
            gtk::gdk::ScrollDirection::Right => Some("WHEEL_RIGHT"),
            _ => None,
        };
        if let Some(k) = key {
            mpv_key("keypress", k);
        }
        glib::Propagation::Proceed
    });
    glarea.connect_leave_notify_event(|_area, _ev| {
        // Saca el cursor del OSD → uosc se esconde.
        if let Some(mpv) = MPV.get() {
            let _ = mpv.command("mouse", &["-1", "-1"]);
        }
        glib::Propagation::Proceed
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

// ───────────────────────────── mostrar / ocultar ──────────────────────────

fn show_surface() {
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(|| {
            eprintln!("[mpv-embed] show_surface (main thread)");
            SURFACE.with(|s| {
                if let Some(surf) = s.borrow().as_ref() {
                    // Oculta el webview y muestra el video (ocupa todo el box).
                    if let Some(wv) = &surf.webview {
                        wv.hide();
                    }
                    surf.glarea.show();
                    surf.glarea.grab_focus();
                    surf.glarea.queue_render();
                    eprintln!("[mpv-embed] glarea.show + queue_render hechos");
                }
            });
        });
    }
}

fn hide_surface() {
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(|| {
            SURFACE.with(|s| {
                if let Some(surf) = s.borrow().as_ref() {
                    // Oculta el video y vuelve a mostrar el webview (menús).
                    surf.glarea.hide();
                    if let Some(wv) = &surf.webview {
                        wv.show();
                        wv.grab_focus(); // teclado vuelve a navegar la UI
                    }
                }
            });
        });
    }
}

fn notify_state(on: bool) {
    RUNNING.store(on, Ordering::SeqCst);
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit("mpv:state", on);
    }
}

// ───────────────────────────── API pública ────────────────────────────────

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Reproduce una URL única. Reemplaza lo que estuviera sonando.
pub fn play(url: &str, title: Option<&str>, start_secs: Option<u64>) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    if let Some(t) = title {
        let _ = mpv.set_property("force-media-title", t);
    }
    match start_secs {
        Some(s) if s > 0 => {
            let _ = mpv.set_property("start", format!("+{s}").as_str());
        }
        _ => {
            let _ = mpv.set_property("start", "none");
        }
    }
    eprintln!("[mpv-embed] play loadfile…");
    mpv.command("loadfile", &[url, "replace"])
        .map_err(|e| format!("loadfile: {e}"))?;
    eprintln!("[mpv-embed] play loadfile OK → show_surface");
    show_surface();
    notify_state(true);
    eprintln!("[mpv-embed] play listo");
    Ok(())
}

/// Reproduce un playlist m3u (IPTV) arrancando en `start`.
pub fn play_iptv(playlist_path: &str, start: usize) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    let _ = mpv.set_property("playlist-start", start as i64);
    mpv.command("loadlist", &[playlist_path, "replace"])
        .map_err(|e| format!("loadlist: {e}"))?;
    show_surface();
    notify_state(true);
    Ok(())
}

/// Para la reproducción y oculta la superficie (vuelve al webview).
pub fn stop() -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    let _ = mpv.command("stop", &[]);
    hide_surface();
    notify_state(false);
    Ok(())
}

/// Ejecuta un comando de mpv venido del frontend como array JSON, ej:
/// ["seek",10,"relative"], ["cycle","pause"], ["set_property","pause",true].
pub fn run_command(args: &[serde_json::Value]) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    let Some((name, rest)) = args.split_first() else {
        return Err("comando vacío".into());
    };
    let name = name.as_str().ok_or("comando sin nombre")?;
    let str_args: Vec<String> = rest.iter().map(json_to_arg).collect();
    let refs: Vec<&str> = str_args.iter().map(|s| s.as_str()).collect();
    mpv.command(name, &refs).map_err(|e| format!("cmd {name}: {e}"))
}

/// Convierte un valor JSON a argumento de comando mpv (texto).
fn json_to_arg(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn get_f64(name: &str) -> f64 {
    MPV.get().and_then(|m| m.get_property::<f64>(name).ok()).unwrap_or(0.0)
}
pub fn get_bool(name: &str) -> bool {
    MPV.get().and_then(|m| m.get_property::<bool>(name).ok()).unwrap_or(false)
}
pub fn get_string(name: &str) -> String {
    MPV.get().and_then(|m| m.get_property::<String>(name).ok()).unwrap_or_default()
}
fn get_i64(name: &str) -> i64 {
    MPV.get().and_then(|m| m.get_property::<i64>(name).ok()).unwrap_or(0)
}

/// Pistas reales (audio/sub) leídas de track-list/N/* con tipos simples.
pub fn tracks() -> Vec<(i64, String, String, String, bool)> {
    let mpv = match MPV.get() {
        Some(m) => m,
        None => return Vec::new(),
    };
    let n = mpv.get_property::<i64>("track-list/count").unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..n {
        let kind = get_string(&format!("track-list/{i}/type"));
        if kind != "audio" && kind != "sub" {
            continue;
        }
        let id = get_i64(&format!("track-list/{i}/id"));
        let lang = get_string(&format!("track-list/{i}/lang"));
        let title = get_string(&format!("track-list/{i}/title"));
        let selected = get_bool(&format!("track-list/{i}/selected"));
        out.push((id, kind, lang, title, selected));
    }
    out
}
