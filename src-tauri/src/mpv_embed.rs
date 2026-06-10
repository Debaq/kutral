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
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
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
/// libepoxy.so cargada (leak): resuelve punteros GL para mpv y para el FBO.
static EPOXY_LIB: AtomicPtr<libloading::Library> = AtomicPtr::new(ptr::null_mut());
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
    /// RenderContext de mpv; se crea en "realize" (cuando hay GL current).
    /// Mantiene viva una referencia fuerte (las closures tienen otras clones).
    #[allow(dead_code)]
    render: Rc<RefCell<Option<RenderContext<'static>>>>,
}

// ───────────────────────── resolución de GL (libepoxy) ─────────────────────

/// Carga libepoxy una vez y guarda el puntero para resolver símbolos GL.
fn ensure_epoxy() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let lib = unsafe {
            libloading::Library::new("libepoxy.so.0")
                .or_else(|_| libloading::Library::new("libepoxy.so"))
        }
        .expect("no se pudo cargar libepoxy.so");
        let leaked: &'static libloading::Library = Box::leak(Box::new(lib));
        EPOXY_LIB.store(leaked as *const _ as *mut _, Ordering::Release);
    });
}

/// get_proc_address que pide libmpv: resuelve `name` (ej "glActiveTexture") en
/// libepoxy. Plain fn (no closure) porque OpenGLInitParams exige `fn`.
///
/// libepoxy NO exporta los símbolos GL planos (`glGetString`): exporta los
/// dispatchers como variables-puntero `epoxy_glGetString`. Así que buscamos
/// `epoxy_<name>` y devolvemos el valor del puntero (el trampolín que resuelve
/// la función real del contexto GL actual en la primera llamada).
fn gl_get_proc(_ctx: &(), name: &str) -> *mut c_void {
    let lib = EPOXY_LIB.load(Ordering::Acquire);
    if lib.is_null() {
        return ptr::null_mut();
    }
    let lib: &libloading::Library = unsafe { &*lib };
    // 1) nombre plano (por si alguna build de epoxy sí lo exporta).
    // 2) fallback: epoxy_<name> (lo normal en Linux).
    for candidate in [name.to_string(), format!("epoxy_{name}")] {
        let Ok(cname) = CString::new(candidate) else {
            continue;
        };
        if let Ok(sym) = unsafe { lib.get::<*const c_void>(cname.as_bytes_with_nul()) } {
            let p = *sym;
            if !p.is_null() {
                return p as *mut c_void;
            }
        }
    }
    ptr::null_mut()
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
    ensure_epoxy();

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

/// Reparenta lo que haya en el vbox (el webview) dentro de un GtkOverlay y le
/// superpone el GtkGLArea (oculto hasta reproducir).
#[allow(deprecated)] // glib::MainContext::channel está deprecado pero vigente en 0.18
fn build_surface(vbox: &gtk::Box) {
    let overlay = gtk::Overlay::new();
    // Sacar al webview del vbox → hijo base del overlay.
    for child in vbox.children() {
        vbox.remove(&child);
        overlay.add(&child);
    }

    let glarea = gtk::GLArea::new();
    glarea.set_hexpand(true);
    glarea.set_vexpand(true);
    // auto_render(true): GTK emite ::render al mapear/exponer → ahí creamos el
    // render context de forma perezosa (contexto GL garantizado current). Los
    // frames nuevos de mpv los empujamos con queue_render.
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

    // render: crea el RenderContext en la PRIMERA llamada (contexto GL current,
    // a diferencia de realize donde mpv→glGetString crasheaba) y dibuja el
    // frame de mpv en el FBO del GLArea (tamaño en px reales = lógico * scale,
    // arregla el escalado HiDPI de raíz).
    {
        let render = render.clone();
        let tx = tx.clone();
        glarea.connect_render(move |area, _gl| {
            let need_init = render.borrow().is_none();
            if need_init {
                if let Some(mpv) = MPV.get() {
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
                        Err(e) => {
                            eprintln!("[mpv-embed] create_render_context: {e}");
                            return glib::Propagation::Proceed;
                        }
                    }
                }
            }
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

    vbox.add(&overlay);
    overlay.add_overlay(&glarea);
    overlay.show_all();
    glarea.hide(); // arranca oculto; se muestra al reproducir

    SURFACE.with(|s| {
        *s.borrow_mut() = Some(Surface { glarea, render });
    });
}

// ───────────────────────────── mostrar / ocultar ──────────────────────────

fn show_surface() {
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(|| {
            SURFACE.with(|s| {
                if let Some(surf) = s.borrow().as_ref() {
                    surf.glarea.show();
                    surf.glarea.grab_focus();
                    surf.glarea.queue_render();
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
                    surf.glarea.hide();
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
    mpv.command("loadfile", &[url, "replace"])
        .map_err(|e| format!("loadfile: {e}"))?;
    show_surface();
    notify_state(true);
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
