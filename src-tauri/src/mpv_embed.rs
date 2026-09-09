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

use std::cell::{Cell, RefCell};
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
/// Sesión VIVA pero fuera de pantalla: Esc vuelve a los menús sin matar el
/// archivo (película pausada / canal IPTV sonando de fondo). La pill de la UI
/// la retoma con `resume()`; un `play()` nuevo la reemplaza.
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// La sesión actual es una playlist IPTV (vivo): al suspender NO se pausa,
/// pausar un stream en directo lo dejaría atrasado del aire.
static LIVE: AtomicBool = AtomicBool::new(false);
/// Lo que está en pantalla es un trailer: no es una sesión "de verdad", así que
/// Esc la cierra en vez de dejarla esperando en la pill.
static TRAILER: AtomicBool = AtomicBool::new(false);
/// Se está abriendo un archivo nuevo y todavía no llegó su primer frame. Sin
/// esto la GLArea sigue mostrando el ÚLTIMO FRAME congelado de lo anterior
/// mientras el stream abre y llena el búfer (varios segundos en torrent/debrid),
/// que es justo lo que se veía feo al cambiar de película.
static LOADING: AtomicBool = AtomicBool::new(false);
/// Qué se está abriendo (para escribirlo en la portada de carga). No se lee de
/// `media-title` porque durante la carga eso todavía puede ser lo anterior.
static LOADING_TITLE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Reproducción que el trailer dejó en pausa para restaurarla después.
static PENDING: std::sync::Mutex<Option<Pending>> = std::sync::Mutex::new(None);

/// Sesión guardada mientras se ve un trailer.
#[derive(Clone)]
struct Pending {
    path: String,
    pos: f64,
    title: String,
    live: bool,
}

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

/// Manda un evento de tecla/botón a mpv (action = keydown/keyup/keypress).
fn mpv_key(action: &str, key: &str) {
    if let Some(mpv) = MPV.get() {
        let _ = mpv.command(action, &[key]);
    }
}

// ───────────────── modo dual: bar de acciones (teclado-first) ──────────────
//
// uosc es mouse-first (sus botones no se navegan con d-pad). Para teclado-first
// dibujamos NUESTRO bar de acciones con un osd-overlay ASS y manejamos el foco
// en Rust: reproduciendo ←/→ = seek; pausado ←/→ = mover foco, OK = activar.

/// Acciones del bar pausado. La lista es DINÁMICA: "Video" solo aparece si el
/// archivo trae 2+ pistas de video (ver build_bar_actions).
#[derive(Clone, Copy, PartialEq)]
enum BarAction {
    SeekBack,
    Resume,
    SeekFwd,
    Subs,
    Audio,
    Video,
    Sources,
    Exit,
}

impl BarAction {
    fn label(self) -> &'static str {
        match self {
            BarAction::SeekBack => "Retroceder 10s",
            BarAction::Resume => "Reanudar",
            BarAction::SeekFwd => "Avanzar 10s",
            BarAction::Subs => "Subtítulos",
            BarAction::Audio => "Audio",
            BarAction::Video => "Video",
            BarAction::Sources => "Cambiar fuente",
            BarAction::Exit => "Salir",
        }
    }

    /// Trazos del icono en una caja de 100×100 centrada en el origen. Cada
    /// elemento es un polígono CERRADO y RELLENO — nada de contornos con
    /// hueco, que dependen de la regla de relleno de libass.
    ///
    /// Son vectores, no glifos, y esa es toda la gracia: dibujarlos con una
    /// fuente de iconos ató la UI a que un .otf estuviera presente Y que
    /// fontconfig lo resolviera igual en cada máquina. No pasó: unos veían los
    /// nombres escritos ("play_arrow") y otros caracteres corruptos. El
    /// dibujo ASS no depende de fuentes, ni de fontconfig, ni del locale, ni
    /// de qué empaquetó linuxdeploy — es el mismo camino con el que ya se
    /// pintan el fondo de la pill y el riel, que sí se ven en todas partes.
    fn shapes(self) -> Vec<Vec<(f64, f64)>> {
        match self {
            // ◀◀ — dos triángulos a la izquierda.
            BarAction::SeekBack => vec![
                vec![(-4.0, -32.0), (-42.0, 0.0), (-4.0, 32.0)],
                vec![(40.0, -32.0), (2.0, 0.0), (40.0, 32.0)],
            ],
            // ▶ — un triángulo a la derecha.
            BarAction::Resume => vec![vec![(-28.0, -38.0), (36.0, 0.0), (-28.0, 38.0)]],
            // ▶▶ — espejo de SeekBack.
            BarAction::SeekFwd => vec![
                vec![(4.0, -32.0), (42.0, 0.0), (4.0, 32.0)],
                vec![(-40.0, -32.0), (-2.0, 0.0), (-40.0, 32.0)],
            ],
            // Marco de subtítulos: cuatro barras (no un contorno hueco) y dos
            // líneas de texto dentro.
            BarAction::Subs => vec![
                vec![(-44.0, -32.0), (44.0, -32.0), (44.0, -25.0), (-44.0, -25.0)],
                vec![(-44.0, 25.0), (44.0, 25.0), (44.0, 32.0), (-44.0, 32.0)],
                vec![(-44.0, -32.0), (-37.0, -32.0), (-37.0, 32.0), (-44.0, 32.0)],
                vec![(37.0, -32.0), (44.0, -32.0), (44.0, 32.0), (37.0, 32.0)],
                vec![(-30.0, 2.0), (0.0, 2.0), (0.0, 12.0), (-30.0, 12.0)],
                vec![(8.0, 2.0), (30.0, 2.0), (30.0, 12.0), (8.0, 12.0)],
            ],
            // Ecualizador: cuatro barras de distinta altura, alineadas abajo.
            BarAction::Audio => vec![
                vec![(-39.0, 8.0), (-27.0, 8.0), (-27.0, 34.0), (-39.0, 34.0)],
                vec![(-17.0, -20.0), (-5.0, -20.0), (-5.0, 34.0), (-17.0, 34.0)],
                vec![(5.0, -4.0), (17.0, -4.0), (17.0, 34.0), (5.0, 34.0)],
                vec![(27.0, -28.0), (39.0, -28.0), (39.0, 34.0), (27.0, 34.0)],
            ],
            // Claqueta: cuerpo + barra inclinada. Se eligió sobre un marco con
            // triángulo porque a 42 px eso se confundía con el de subtítulos.
            BarAction::Video => vec![
                vec![(-44.0, -10.0), (44.0, -10.0), (44.0, 32.0), (-44.0, 32.0)],
                vec![(-44.0, -32.0), (40.0, -32.0), (44.0, -14.0), (-40.0, -14.0)],
            ],
            // ⇄ — dos flechas opuestas (mango + punta, sin solaparse entre sí):
            // "esta fuente no me sirve, dame otra".
            BarAction::Sources => vec![
                vec![(-40.0, -25.0), (8.0, -25.0), (8.0, -13.0), (-40.0, -13.0)],
                vec![(8.0, -37.0), (42.0, -19.0), (8.0, -1.0)],
                vec![(-8.0, 13.0), (40.0, 13.0), (40.0, 25.0), (-8.0, 25.0)],
                vec![(-8.0, 1.0), (-42.0, 19.0), (-8.0, 37.0)],
            ],
            // ✕ — dos barras cruzadas a 45°, como rectángulos girados.
            BarAction::Exit => vec![
                vec![(-28.3, -39.7), (39.7, 28.3), (28.3, 39.7), (-39.7, -28.3)],
                vec![(39.7, -28.3), (-28.3, 39.7), (-39.7, 28.3), (28.3, -39.7)],
            ],
        }
    }
}

struct PlayerUi {
    paused: bool,
    /// El bar está en pantalla. Con teclado va de la mano de `paused`; con el
    /// mouse aparece sin pausar (y se auto-oculta a los 4s sin movimiento).
    bar_visible: bool,
    focus: usize,
    /// Acciones visibles del bar (se reconstruyen al mostrarlo según las pistas).
    bar: Vec<BarAction>,
}

thread_local! {
    static PLAYER_UI: RefCell<PlayerUi> = const {
        RefCell::new(PlayerUi { paused: false, bar_visible: false, focus: 1, bar: Vec::new() })
    };
    /// Generación del autohide del bar (mismo truco que CURSOR_GEN).
    static BAR_GEN: Cell<u64> = const { Cell::new(0) };
    /// Menú de pistas abierto (audio/subs). None = no hay menú (modo bar).
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
    /// Ya hay un tick de la barra de progreso corriendo (evita apilar timers).
    static SEEK_TICK: Cell<bool> = const { Cell::new(false) };
    /// Ya hay un tick de la portada de carga corriendo (evita apilar timers).
    static LOAD_TICK: Cell<bool> = const { Cell::new(false) };
}

/// Cuenta pistas de un tipo ("video"/"audio"/"sub").
fn count_tracks(kind: &str) -> i64 {
    let Some(mpv) = MPV.get() else { return 0 };
    let count = mpv.get_property::<i64>("track-list/count").unwrap_or(0);
    (0..count)
        .filter(|i| get_string(&format!("track-list/{i}/type")) == kind)
        .count() as i64
}

/// Construye la lista de acciones del bar. "Video" solo con 2+ pistas de video.
fn build_bar_actions() -> Vec<BarAction> {
    let mut v = vec![
        BarAction::SeekBack,
        BarAction::Resume,
        BarAction::SeekFwd,
        BarAction::Subs,
        BarAction::Audio,
    ];
    if count_tracks("video") >= 2 {
        v.push(BarAction::Video);
    }
    // Cambiar de fuente solo tiene sentido en una película/capítulo: un canal
    // IPTV o un trailer no salieron de una lista de fuentes.
    if !LIVE.load(Ordering::SeqCst) && !TRAILER.load(Ordering::SeqCst) {
        v.push(BarAction::Sources);
    }
    v.push(BarAction::Exit);
    v
}

/// Acción al elegir un ítem del menú.
#[derive(Clone)]
enum MenuAct {
    Aid(i64),
    Sid(i64),
    Vid(i64),
    SubOff,
    Download,
    /// Ítem de un picker provisto por el frontend: al elegir emite
    /// "player:menu-pick" con este id para que el front actúe (ej. descargar).
    Pick(String),
}

struct MenuItem {
    label: String,
    act: MenuAct,
}

struct Menu {
    title: String,
    items: Vec<MenuItem>,
    sel: usize,
}

/// Ejecuta un comando mpv (helper corto).
fn mpv_cmd2(name: &str, args: &[&str]) {
    if let Some(mpv) = MPV.get() {
        let _ = mpv.command(name, args);
    }
}

// osd-overlay ids: 46 = fondo (pill), 47 = textos, 48 = barra de progreso,
// 49 = portada de carga.
const BAR_BG_ID: &str = "46";
const BAR_FG_ID: &str = "47";
const BAR_SEEK_ID: &str = "48";
/// Portada de carga: tapa el frame anterior hasta que el nuevo esté en pantalla.
const LOAD_ID: &str = "49";

// ─────────────────────── geometría real del OSD ───────────────────────────
//
// El bar y los menús se dibujan en PÍXELES REALES del OSD (osd-width/height),
// no en un canvas fijo: la ventana cambia de tamaño y de proporción según el
// equipo, y con un canvas 1280×720 estirado el click caía corrido. El diseño
// se define en unidades de 720p y se multiplica por k = alto_real / 720, así
// se ve igual en 720p, 1080p, 4K o una ventana pequeña y arbitraria.

/// Alto del diseño de referencia. Todo lo demás se escala contra esto.
const DESIGN_H: f64 = 720.0;

#[derive(Clone, Copy)]
struct Osd {
    w: f64,
    h: f64,
    /// Factor de escala del diseño (1.0 = 720p).
    k: f64,
}

/// Tamaño físico del GLArea (px reales = lógico × scale). Respaldo cuando mpv
/// aún no reporta OSD (antes del primer frame).
fn surface_px() -> (f64, f64) {
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

/// Dimensiones reales del OSD según mpv (las mismas que usa libass al componer
/// el overlay), con su factor de escala de diseño.
fn osd() -> Osd {
    let (mut w, mut h) = (0.0, 0.0);
    if let Some(mpv) = MPV.get() {
        w = mpv.get_property::<i64>("osd-width").unwrap_or(0) as f64;
        h = mpv.get_property::<i64>("osd-height").unwrap_or(0) as f64;
    }
    if w < 2.0 || h < 2.0 {
        let (fw, fh) = surface_px();
        w = fw;
        h = fh;
    }
    let k = (h / DESIGN_H).clamp(0.3, 6.0);
    Osd { w, h, k }
}

/// Pasa un punto del widget (coords lógicas GTK) a píxeles del OSD, que es el
/// sistema en el que dibujamos: proporción sobre el área real, sin suponer
/// ninguna relación de aspecto.
fn widget_to_osd(area: &gtk::GLArea, x: f64, y: f64) -> (f64, f64) {
    let o = osd();
    let w = area.allocated_width().max(1) as f64;
    let h = area.allocated_height().max(1) as f64;
    (x / w * o.w, y / h * o.h)
}

/// Posición y duración actuales, o None si no hay nada que buscar: en vivo
/// (IPTV) `duration` es 0 y dibujar una barra de progreso ahí sería mentira.
fn seek_info() -> Option<(f64, f64)> {
    let dur = get_f64("duration");
    if dur <= 0.5 {
        return None;
    }
    Some((get_f64("time-pos").clamp(0.0, dur), dur))
}

/// "12:34" o "1:23:45" — la hora solo aparece si de verdad hay horas.
fn fmt_hms(t: f64) -> String {
    let t = t.max(0.0) as u64;
    let (h, m, sec) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// Medidas de la barra de progreso, en píxeles del OSD.
#[derive(Clone, Copy)]
struct SeekGeom {
    /// Centro vertical de la fila (riel y tiempos comparten esta línea).
    cy: f64,
    track_l: f64,
    track_r: f64,
    /// Grosor del riel.
    h: f64,
    pos: f64,
    dur: f64,
}

/// Medidas del bar ya resueltas en píxeles del OSD.
#[derive(Clone, Copy)]
struct BarGeom {
    n: usize,
    step: f64,
    top: f64,
    bot: f64,
    icon_cy: f64,
    label_y: f64,
    center_x: f64,
    k: f64,
    osd: Osd,
    /// None en directo: sin duración no hay barra y la pill queda más baja.
    seek: Option<SeekGeom>,
}

fn bar_geom() -> BarGeom {
    let o = osd();
    let k = o.k;
    let n = PLAYER_UI.with(|u| u.borrow().bar.len());
    let si = seek_info();
    // Anclado al borde inferior real (54 y 160 son las medidas del diseño).
    // Con barra de progreso la pill crece hacia ARRIBA: los iconos y la
    // etiqueta no se mueven, así que el músculo memoria del usuario se mantiene.
    let bot = o.h - 54.0 * k;
    let top = bot - if si.is_some() { 208.0 } else { 160.0 } * k;
    // En ventanas angostas los iconos se juntan antes que salirse de pantalla.
    let step = if n > 0 {
        (96.0 * k).min((o.w - 48.0 * k) / n as f64)
    } else {
        96.0 * k
    };
    let mut g = BarGeom {
        n,
        step,
        top,
        bot,
        icon_cy: bot - 100.0 * k,
        label_y: bot - 38.0 * k,
        center_x: o.w / 2.0,
        k,
        osd: o,
        seek: None,
    };
    if let Some((pos, dur)) = si {
        let (l, r) = pill_x(&g, true);
        // Reserva a cada lado para los tiempos: "1:23:45" a fs 22 ≈ 80 px.
        let pad = 96.0 * k;
        g.seek = Some(SeekGeom {
            cy: top + 34.0 * k,
            track_l: l + pad,
            track_r: (r - pad).max(l + pad + 20.0 * k),
            h: 6.0 * k,
            pos,
            dur,
        });
    }
    g
}

/// Extremos horizontales de la pill. Lo comparten el fondo y el riel: si cada
/// uno calculara los suyos, el riel se saldría del fondo en cuanto cambie uno.
fn pill_x(g: &BarGeom, has_seek: bool) -> (f64, f64) {
    let mut half = (g.n.max(1) as f64 * g.step) / 2.0 + 34.0 * g.k;
    // Con riel la pill nunca es angosta: una barra de 200 px no se puede
    // apuntar con el mouse ni con el control.
    if has_seek {
        half = half.max(330.0 * g.k);
    }
    let l = (g.center_x - half).max(4.0);
    let r = (g.center_x + half).min(g.osd.w - 4.0);
    (l, r)
}

/// Centro X del icono `i` (fila centrada horizontalmente).
fn bar_icon_cx(g: &BarGeom, i: usize) -> f64 {
    g.center_x + (i as f64 - (g.n as f64 - 1.0) / 2.0) * g.step
}

/// ¿Sobre qué icono cae el punto (px del OSD)? None = fuera del bar.
fn bar_hit(px: f64, py: f64) -> Option<usize> {
    let g = bar_geom();
    if py < g.top || py > g.bot {
        return None;
    }
    // Sin esto, un click en el riel caería en la columna del icono que tenga
    // debajo y en vez de saltar en el tiempo abriría el menú de subtítulos.
    if seek_hit(&g, px, py).is_some() {
        return None;
    }
    (0..g.n).find(|&i| (px - bar_icon_cx(&g, i)).abs() <= g.step / 2.0)
}

/// Tamaño de fuente ASS escalado (nunca ilegible por redondeo).
fn fs(k: f64, design: f64) -> i32 {
    ((design * k).round() as i32).max(8)
}

/// Recorta un texto al ancho disponible (px del OSD). Aproxima el avance medio
/// de la fuente en 0.5 × el tamaño: no es tipografía exacta, pero evita que un
/// título largo se salga del panel en ventanas angostas.
fn ellipsize(text: &str, width_px: f64, size: i32) -> String {
    let max_chars = ((width_px / (size as f64 * 0.5)).floor() as usize).max(6);
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// Rectángulo de esquinas redondeadas en modo dibujo ASS (\p1), con curvas
/// Bézier. Con rad = alto/2 = ancho/2 sale un círculo (la perilla del riel).
fn ass_round_rect(l: i32, t: i32, r: i32, b: i32, rad: i32) -> String {
    let rad = rad.min((r - l) / 2).min((b - t) / 2).max(0);
    format!(
        "m {l} {tr} b {l} {t} {lr} {t} {lr} {t} \
         l {rr} {t} b {r} {t} {r} {t} {r} {tr} \
         l {r} {br} b {r} {b} {rr} {b} {rr} {b} \
         l {lr} {b} b {l} {b} {l} {b} {l} {br}",
        l = l,
        r = r,
        t = t,
        b = b,
        lr = l + rad,
        rr = r - rad,
        tr = t + rad,
        br = b - rad,
    )
}

/// Fondo: pill inferior oscuro semi-transparente, ancha según cuántos iconos hay.
fn build_bar_bg() -> String {
    let g = bar_geom();
    let (l, r) = pill_x(&g, g.seek.is_some());
    let rad = (20.0 * g.k).round() as i32;
    // \1a = transparencia (00 opaco … FF transp). Dark #0A0F12 → &H120F0A&.
    format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H120F0A&\\1a&H30&\\p1}}{}{{\\p0}}",
        ass_round_rect(l as i32, g.top as i32, r as i32, g.bot as i32, rad)
    )
}

/// Barra de progreso: riel, porción reproducida, perilla y los dos tiempos.
/// Vive en su propio overlay para poder refrescarla sola 4 veces por segundo
/// sin volver a componer los iconos, que no cambian.
fn build_seek_ass(g: &BarGeom) -> String {
    let Some(s) = g.seek else { return String::new() };
    let frac = (s.pos / s.dur).clamp(0.0, 1.0);
    let (l, r) = (s.track_l as i32, s.track_r as i32);
    let (t, b) = ((s.cy - s.h / 2.0) as i32, (s.cy + s.h / 2.0) as i32);
    let cy = s.cy as i32;
    let rad = (s.h / 2.0) as i32;
    let fill_r = (s.track_l + (s.track_r - s.track_l) * frac) as i32;
    let mut ev: Vec<String> = Vec::with_capacity(5);
    // Riel: cream muy transparente.
    ev.push(format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&HE8E0D0&\\1a&HA0&\\p1}}{}{{\\p0}}",
        ass_round_rect(l, t, r, b, rad)
    ));
    // Reproducido: naranja f97316 → &H1673F9&.
    if fill_r > l {
        ev.push(format!(
            "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H1673F9&\\p1}}{}{{\\p0}}",
            ass_round_rect(l, t, fill_r, b, rad)
        ));
    }
    // Perilla: círculo en la punta de lo reproducido.
    let kr = (9.0 * g.k).round().max(3.0) as i32;
    ev.push(format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad1.5\\4c&H000000&\\1c&H1673F9&\\p1}}{}{{\\p0}}",
        ass_round_rect(fill_r - kr, cy - kr, fill_r + kr, cy + kr, kr)
    ));
    // Tiempos: transcurrido pegado al inicio del riel, total al final.
    let size = fs(g.k, 22.0);
    ev.push(format!(
        "{{\\an6\\pos({},{cy})\\fs{size}\\1c&HE8E0D0&\\bord0\\shad1.5\\4c&H000000&}}{}",
        (s.track_l - 14.0 * g.k) as i32,
        fmt_hms(s.pos)
    ));
    ev.push(format!(
        "{{\\an4\\pos({},{cy})\\fs{size}\\1c&HB9B3A6&\\bord0\\shad1.5\\4c&H000000&}}{}",
        (s.track_r + 14.0 * g.k) as i32,
        fmt_hms(s.dur)
    ));
    ev.join("\n")
}

/// Fracción [0,1] si el punto cae en la fila del riel. La banda de acierto es
/// mucho más alta que el riel (6 px de diseño): con mouse o control remoto hay
/// que poder apuntarlo sin precisión de cirujano.
fn seek_hit(g: &BarGeom, px: f64, py: f64) -> Option<f64> {
    let s = g.seek?;
    if (py - s.cy).abs() > 24.0 * g.k {
        return None;
    }
    let w = s.track_r - s.track_l;
    if w <= 1.0 {
        return None;
    }
    Some(((px - s.track_l) / w).clamp(0.0, 1.0))
}

/// Salta a una fracción del archivo. `exact` para caer donde el usuario apuntó
/// y no en el keyframe más cercano, que en un anime de 24 min se nota.
fn seek_to_fraction(frac: f64) {
    let pct = format!("{:.4}", frac.clamp(0.0, 1.0) * 100.0);
    mpv_cmd2("seek", &[&pct, "absolute-percent", "exact"]);
}

/// Refresca SOLO el overlay del riel mientras el bar está en pantalla.
/// Redibujar el bar entero a este ritmo sería desperdicio: los iconos no
/// cambian. Se corta solo al ocultarse el bar, al abrirse un menú o en directo.
fn start_seek_tick() {
    if SEEK_TICK.with(|c| c.get()) {
        return;
    }
    SEEK_TICK.with(|c| c.set(true));
    glib::timeout_add_local(std::time::Duration::from_millis(250), || {
        let visible = PLAYER_UI.with(|u| u.borrow().bar_visible);
        let menu_open = MENU.with(|m| m.borrow().is_some());
        let g = bar_geom();
        if !visible || menu_open || g.seek.is_none() {
            SEEK_TICK.with(|c| c.set(false));
            return glib::ControlFlow::Break;
        }
        put_overlay(BAR_SEEK_ID, &build_seek_ass(&g));
        glib::ControlFlow::Continue
    });
}

/// Escala los trazos de un icono (caja 100×100 centrada) a su posición real y
/// los emite como comandos de dibujo ASS. `size` es el lado de la caja en
/// píxeles del OSD.
fn icon_drawing(act: BarAction, cx: f64, cy: f64, size: f64) -> String {
    let s = size / 100.0;
    let mut out = String::new();
    for poly in act.shapes() {
        for (i, (x, y)) in poly.iter().enumerate() {
            let (px, py) = (cx + x * s, cy + y * s);
            // `m` abre un contorno, `l` encadena vértices. Un contorno por
            // polígono: libass los rellena todos en el mismo evento.
            out.push_str(&format!("{} {:.1} {:.1} ", if i == 0 { "m" } else { "l" }, px, py));
        }
    }
    out.trim_end().to_string()
}

/// Bar con ICONOS VECTORIALES: un evento ASS por icono en su posición real
/// (ver bar_icon_cx) + la etiqueta del enfocado debajo. El enfocado va naranja
/// y más grande; con mouse el "foco" lo pone el hover.
fn build_bar_ass(focus: usize) -> String {
    // Colores ASS = &HBBGGRR&. Naranja f97316 → &H1673F9&. Cream → &HE8E0D0&.
    let g = bar_geom();
    let bar = PLAYER_UI.with(|u| u.borrow().bar.clone());
    let mut ev: Vec<String> = Vec::with_capacity(bar.len() + 1);
    for (i, act) in bar.iter().enumerate() {
        let cx = bar_icon_cx(&g, i) as i32;
        let cy = g.icon_cy as i32;
        // Antes eran tamaños de FUENTE; ahora son el lado de la caja del
        // dibujo. Los valores bajan porque un glifo solo ocupa ~0.8 del em.
        let (size, color) = if i == focus {
            (56.0 * g.k, "&H1673F9&")
        } else {
            (40.0 * g.k, "&HB9B3A6&")
        };
        ev.push(format!(
            "{{\\an7\\pos(0,0)\\bord0\\shad1.5\\4c&H000000&\\1c{color}\\p1}}{}{{\\p0}}",
            icon_drawing(*act, cx as f64, cy as f64, size)
        ));
    }
    let size = fs(g.k, 26.0);
    let label = ellipsize(
        bar.get(focus).map(|a| a.label()).unwrap_or(""),
        g.osd.w - 40.0 * g.k,
        size,
    );
    ev.push(format!(
        "{{\\an5\\pos({},{})\\fs{size}\\1c&HE8E0D0&\\b1\\bord0\\shad1.5\\4c&H000000&}}{label}",
        g.center_x as i32, g.label_y as i32,
    ));
    ev.join("\n")
}

/// Manda un overlay ASS declarando la resolución REAL del OSD, para que una
/// unidad del dibujo sea un píxel y el hit-test del mouse coincida.
fn put_overlay(id: &str, data: &str) {
    let o = osd();
    let (rx, ry) = ((o.w as i64).to_string(), (o.h as i64).to_string());
    mpv_cmd2(
        "osd-overlay",
        &[id, "ass-events", data, &rx, &ry, "0", "no", "no"],
    );
}

/// Dibuja/actualiza el bar (fondo + textos).
fn draw_bar() {
    let focus = PLAYER_UI.with(|u| u.borrow().focus);
    let g = bar_geom();
    put_overlay(BAR_BG_ID, &build_bar_bg());
    put_overlay(BAR_FG_ID, &build_bar_ass(focus));
    if g.seek.is_some() {
        put_overlay(BAR_SEEK_ID, &build_seek_ass(&g));
        start_seek_tick();
    } else {
        // En directo no hay riel: hay que borrarlo por si veníamos de un
        // archivo con duración (la sesión de mpv se reutiliza entre videos).
        hide_overlay(BAR_SEEK_ID);
    }
}

/// Apaga un overlay por id.
fn hide_overlay(id: &str) {
    mpv_cmd2("osd-overlay", &[id, "none", "", "0", "0", "0", "no", "no"]);
}

/// Quita el bar (fondo + textos).
fn clear_bar() {
    for id in [BAR_BG_ID, BAR_FG_ID, BAR_SEEK_ID] {
        hide_overlay(id);
    }
}

// ─────────────────────────── portada de carga ──────────────────────────────

/// Pantalla negra con "Cargando…" y el título de lo que se está abriendo.
/// Es un overlay ASS opaco a pantalla completa: tapa el frame congelado de lo
/// anterior sin tocar el video, así no hace falta parar mpv ni recrear nada.
fn build_loading_ass(dots: usize) -> String {
    let o = osd();
    let (w, h) = (o.w as i32, o.h as i32);
    let (cx, cy) = ((o.w / 2.0) as i32, (o.h / 2.0) as i32);
    let big = fs(o.k, 30.0);
    let small = fs(o.k, 20.0);
    let puntos = ".".repeat(dots + 1);
    let mut out = format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H000000&\\1a&H00&\\p1}}\
         m 0 0 l {w} 0 l {w} {h} l 0 {h}{{\\p0}}\n\
         {{\\an5\\pos({cx},{cy})\\bord0\\shad0\\fs{big}\\1c&HFFFFFF&}}Cargando{puntos}"
    );
    let title = LOADING_TITLE.lock().map(|t| t.clone()).unwrap_or_default();
    let title = ellipsize(title.trim(), o.w * 0.7, small);
    if !title.is_empty() {
        let ty = cy + (44.0 * o.k) as i32;
        out.push_str(&format!(
            "\n{{\\an5\\pos({cx},{ty})\\bord0\\shad0\\fs{small}\\1c&HB0B0B0&}}{title}"
        ));
    }
    out
}

/// ¿Lo nuevo ya está en pantalla? `core-idle` es falso solo cuando mpv está
/// sacando frames de verdad (sirve igual para video que para audio), y
/// `paused-for-cache`/`seeking` cubren el rato en que ya hay imagen pero
/// todavía se llena el búfer o se salta al minuto de "seguir viendo". Si el
/// usuario pausó a mano durante la carga, el frame ya es del archivo nuevo:
/// tapar más sería esconderle su propia pausa.
fn loading_ready() -> bool {
    if get_bool("idle-active") {
        return false;
    }
    if get_bool("pause") && get_f64("dwidth") > 0.0 {
        return true;
    }
    !get_bool("core-idle") && !get_bool("paused-for-cache") && !get_bool("seeking")
}

/// Levanta la portada. Va ANTES del `loadfile`, para tapar también el momento
/// en que mpv suelta el archivo viejo. `reset_player_ui()` (que corre al
/// mostrar la superficie) solo borra los overlays del bar, no éste.
fn begin_loading(title: Option<&str>) {
    if let Ok(mut t) = LOADING_TITLE.lock() {
        *t = title.unwrap_or("").to_string();
    }
    LOADING.store(true, Ordering::SeqCst);
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(|| {
            put_overlay(LOAD_ID, &build_loading_ass(0));
            start_loading_tick();
        });
    }
}

/// Baja la portada (primer frame listo, o se salió del video antes).
fn end_loading() {
    if !LOADING.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(|| hide_overlay(LOAD_ID));
    }
}

/// Anima los puntos y vigila el primer frame. Sin bucle de eventos de libmpv
/// acá, se sondea desde el hilo GTK (mismo patrón que el trailer).
///
/// Los primeros tics no miran el estado: justo después del `loadfile` mpv
/// todavía puede estar reproduciendo el archivo viejo (lo procesa en su propio
/// bucle), y creerle ahí bajaría la portada sobre el frame anterior.
fn start_loading_tick() {
    if LOAD_TICK.with(|c| c.get()) {
        return;
    }
    LOAD_TICK.with(|c| c.set(true));
    let mut n: usize = 0;
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        n += 1;
        let listo = n > 3 && loading_ready();
        if !LOADING.load(Ordering::SeqCst) || listo {
            LOAD_TICK.with(|c| c.set(false));
            LOADING.store(false, Ordering::SeqCst);
            hide_overlay(LOAD_ID);
            return glib::ControlFlow::Break;
        }
        put_overlay(LOAD_ID, &build_loading_ass((n / 2) % 3));
        glib::ControlFlow::Continue
    });
}

/// Pausa y muestra el bar (foco en Reanudar). Reconstruye las acciones según
/// las pistas actuales (ej. añade "Video" si hay 2+).
fn enter_paused() {
    let bar = build_bar_actions();
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        b.paused = true;
        b.bar_visible = true;
        b.focus = 1;
        b.bar = bar;
    });
    if let Some(mpv) = MPV.get() {
        let _ = mpv.set_property("pause", true);
    }
    draw_bar();
}

/// Reanuda y oculta el bar.
fn resume_play() {
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        b.paused = false;
        b.bar_visible = false;
    });
    if let Some(mpv) = MPV.get() {
        let _ = mpv.set_property("pause", false);
    }
    clear_bar();
}

/// Muestra el bar SIN pausar (mouse) y programa su auto-ocultado. Si ya está
/// visible solo reprograma el timer. Con la reproducción pausada el bar queda
/// fijo: no se auto-oculta.
fn show_bar_transient() {
    let redraw = PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        if !b.bar_visible {
            b.bar_visible = true;
            b.bar = Vec::new(); // se rellena abajo (necesita leer las pistas)
            true
        } else {
            false
        }
    });
    if redraw {
        let bar = build_bar_actions();
        PLAYER_UI.with(|u| {
            let mut b = u.borrow_mut();
            b.focus = b.focus.min(bar.len().saturating_sub(1));
            b.bar = bar;
        });
    }
    draw_bar();
    schedule_bar_hide();
}

/// Oculta el bar (si no estamos pausados ni con un menú abierto).
fn hide_bar_transient() {
    let paused = PLAYER_UI.with(|u| u.borrow().paused);
    let menu_open = MENU.with(|m| m.borrow().is_some());
    if paused || menu_open {
        return;
    }
    PLAYER_UI.with(|u| u.borrow_mut().bar_visible = false);
    clear_bar();
}

/// Reprograma el auto-ocultado del bar a 4s sin actividad de mouse.
fn schedule_bar_hide() {
    let gen = BAR_GEN.with(|g| {
        let n = g.get().wrapping_add(1);
        g.set(n);
        n
    });
    glib::timeout_add_local(std::time::Duration::from_millis(4000), move || {
        if BAR_GEN.with(|g| g.get()) == gen {
            hide_bar_transient();
        }
        glib::ControlFlow::Break
    });
}

/// Space: alterna pausa/reanudar (y el bar).
fn toggle_pause() {
    let paused = PLAYER_UI.with(|u| u.borrow().paused);
    if paused {
        resume_play();
    } else {
        enter_paused();
    }
}

/// Mueve el foco del bar (con wrap) y redibuja.
fn move_focus(delta: i32) {
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        let n = b.bar.len() as i32;
        if n > 0 {
            b.focus = (((b.focus as i32 + delta) % n + n) % n) as usize;
        }
    });
    draw_bar();
}

/// Activa la acción enfocada (con feedback OSD visible).
fn activate_focus() {
    let act = PLAYER_UI.with(|u| {
        let b = u.borrow();
        b.bar.get(b.focus).copied()
    });
    match act {
        Some(BarAction::SeekBack) => {
            mpv_cmd2("seek", &["-10"]);
            mpv_cmd2("show-text", &["⏪ -10s   ${time-pos}", "1200"]);
        }
        Some(BarAction::Resume) => {
            // Bar abierto con el mouse sin pausar → este icono pausa.
            if PLAYER_UI.with(|u| u.borrow().paused) {
                resume_play();
            } else {
                enter_paused();
            }
        }
        Some(BarAction::SeekFwd) => {
            mpv_cmd2("seek", &["10"]);
            mpv_cmd2("show-text", &["⏩ +10s   ${time-pos}", "1200"]);
        }
        Some(BarAction::Subs) => open_sub_menu(),
        Some(BarAction::Audio) => open_audio_menu(),
        // La lista de fuentes vive en el webview: se avisa ANTES de cerrar para
        // que el front sepa que esto no es un "salir" normal (que lo devolvería
        // al catálogo) sino un "volver a elegir fuente".
        Some(BarAction::Sources) => {
            if let Some(app) = APP.get() {
                use tauri::Emitter;
                let _ = app.emit("player:cambiar-fuente", ());
            }
            let _ = stop();
        }
        Some(BarAction::Video) => open_video_menu(),
        Some(BarAction::Exit) => {
            let _ = stop();
        }
        None => {}
    }
}

/// Construye los ítems del menú leyendo la track-list real con etiquetas ricas:
/// numera cada pista y muestra idioma + título + códec + forzado/default, para
/// distinguirlas aunque NO traigan código de idioma.
fn build_track_items(want: &str) -> Vec<MenuItem> {
    let mpv = match MPV.get() {
        Some(m) => m,
        None => return Vec::new(),
    };
    let count = mpv.get_property::<i64>("track-list/count").unwrap_or(0);
    let mut items = Vec::new();
    let mut n = 0;
    for i in 0..count {
        if get_string(&format!("track-list/{i}/type")) != want {
            continue;
        }
        n += 1;
        let id = get_i64(&format!("track-list/{i}/id"));
        let lang = get_string(&format!("track-list/{i}/lang"));
        let title = get_string(&format!("track-list/{i}/title"));
        let codec = get_string(&format!("track-list/{i}/codec"));
        let forced = get_bool(&format!("track-list/{i}/forced"));
        let default = get_bool(&format!("track-list/{i}/default"));
        let sel = get_bool(&format!("track-list/{i}/selected"));

        let mark = if sel { "● " } else { "    " };
        let lang_u = lang.trim().to_uppercase();
        let mut head = if lang_u.is_empty() {
            format!("Pista {n}")
        } else {
            lang_u
        };
        let t = title.trim();
        if !t.is_empty() {
            head.push_str(" — ");
            head.push_str(t);
        }
        let mut extra: Vec<String> = Vec::new();
        let c = codec.trim();
        if !c.is_empty() {
            extra.push(c.to_string());
        }
        if forced {
            extra.push("forzado".into());
        }
        if default {
            extra.push("default".into());
        }
        let label = if extra.is_empty() {
            format!("{mark}{head}")
        } else {
            format!("{mark}{head}  ·  {}", extra.join(" · "))
        };

        let act = match want {
            "audio" => MenuAct::Aid(id),
            "video" => MenuAct::Vid(id),
            _ => MenuAct::Sid(id),
        };
        items.push(MenuItem { label, act });
    }
    items
}

fn open_video_menu() {
    let mut items = build_track_items("video");
    if items.is_empty() {
        items.push(MenuItem { label: "(sin pistas de video)".into(), act: MenuAct::SubOff });
    }
    open_menu("Video", items);
}

fn open_audio_menu() {
    let mut items = build_track_items("audio");
    if items.is_empty() {
        items.push(MenuItem { label: "(sin pistas de audio)".into(), act: MenuAct::SubOff });
    }
    open_menu("Audio", items);
}

fn open_sub_menu() {
    let mut items = vec![MenuItem {
        label: "Desactivar subtítulos".into(),
        act: MenuAct::SubOff,
    }];
    items.extend(build_track_items("sub"));
    items.push(MenuItem {
        label: "⬇  Descargar subtítulos…".into(),
        act: MenuAct::Download,
    });
    open_menu("Subtítulos", items);
}

fn open_menu(title: &str, items: Vec<MenuItem>) {
    // Por defecto, foco en el ítem activo (●) si lo hay.
    let sel = items.iter().position(|i| i.label.starts_with("● ")).unwrap_or(0);
    MENU.with(|m| {
        *m.borrow_mut() = Some(Menu {
            title: title.to_string(),
            items,
            sel,
        })
    });
    draw_menu();
}

fn close_menu() {
    MENU.with(|m| *m.borrow_mut() = None);
    draw_bar(); // volvemos al bar (seguimos pausados)
}

fn menu_move(delta: i32) {
    MENU.with(|m| {
        if let Some(menu) = m.borrow_mut().as_mut() {
            let n = menu.items.len() as i32;
            if n > 0 {
                menu.sel = (((menu.sel as i32 + delta) % n + n) % n) as usize;
            }
        }
    });
    draw_menu();
}

fn menu_activate() {
    let act = MENU.with(|m| {
        m.borrow()
            .as_ref()
            .and_then(|menu| menu.items.get(menu.sel).map(|it| it.act.clone()))
    });
    match act {
        Some(MenuAct::Aid(id)) => {
            mpv_cmd2("set", &["aid", &id.to_string()]);
            close_menu();
        }
        Some(MenuAct::Sid(id)) => {
            mpv_cmd2("set", &["sid", &id.to_string()]);
            mpv_cmd2("set", &["sub-visibility", "yes"]);
            close_menu();
        }
        Some(MenuAct::Vid(id)) => {
            mpv_cmd2("set", &["vid", &id.to_string()]);
            close_menu();
        }
        Some(MenuAct::SubOff) => {
            mpv_cmd2("set", &["sid", "no"]);
            close_menu();
        }
        Some(MenuAct::Download) => {
            if let Some(app) = APP.get() {
                use tauri::Emitter;
                let _ = app.emit("player:download-subs", ());
            }
            close_menu();
        }
        Some(MenuAct::Pick(id)) => {
            if let Some(app) = APP.get() {
                use tauri::Emitter;
                let _ = app.emit("player:menu-pick", id);
            }
            close_menu();
        }
        None => {}
    }
}

/// Abre un picker provisto por el frontend (lista de (label, id)). Al elegir,
/// se emite "player:menu-pick" con el id. Llamable desde un comando (hilo
/// worker) → salta al hilo main.
pub fn open_picker(title: String, items: Vec<(String, String)>) {
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(move || {
            let menu_items = items
                .into_iter()
                .map(|(label, id)| MenuItem {
                    label,
                    act: MenuAct::Pick(id),
                })
                .collect::<Vec<_>>();
            if menu_items.is_empty() {
                return;
            }
            // Mostrar el video pausado + el picker encima.
            PLAYER_UI.with(|u| u.borrow_mut().paused = true);
            if let Some(mpv) = MPV.get() {
                let _ = mpv.set_property("pause", true);
            }
            MENU.with(|m| {
                *m.borrow_mut() = Some(Menu {
                    title,
                    items: menu_items,
                    sel: 0,
                })
            });
            draw_menu();
        });
    }
}

/// Máximo de ítems visibles a la vez (el resto hace scroll).
const MENU_MAX_VISIBLE: usize = 12;

/// Ventana visible [start, end) centrada en el seleccionado.
fn menu_window(menu: &Menu) -> (usize, usize) {
    let n = menu.items.len();
    if n <= MENU_MAX_VISIBLE {
        return (0, n);
    }
    let start = menu
        .sel
        .saturating_sub(MENU_MAX_VISIBLE / 2)
        .min(n - MENU_MAX_VISIBLE);
    (start, start + MENU_MAX_VISIBLE)
}

// Geometría del menú, también en píxeles reales del OSD (ver `osd()`): filas de
// alto fijo escalado, centradas en la ventana, para que hover y click caigan
// donde el usuario ve cada ítem sin importar el tamaño ni la proporción.

/// Medidas del menú resueltas en píxeles del OSD.
#[derive(Clone, Copy)]
struct MenuGeom {
    row_h: f64,
    cy: f64,
    l: f64,
    r: f64,
    k: f64,
    start: usize,
    end: usize,
    rows: usize,
    /// Fila en la que empieza la lista de ítems (tras título y separador).
    first: usize,
}

fn menu_geom(menu: &Menu) -> MenuGeom {
    let o = osd();
    let n = menu.items.len();
    let (start, end) = menu_window(menu);
    let first = 2 + (start > 0) as usize; // título + blanco + [▲]
    let rows = first + (end - start) + (end < n) as usize;
    // Panel de 680 de ancho en el diseño, sin salirse en ventanas angostas.
    let half = (340.0 * o.k).min(o.w / 2.0 - 12.0);
    MenuGeom {
        row_h: 44.0 * o.k,
        cy: o.h / 2.0,
        l: o.w / 2.0 - half,
        r: o.w / 2.0 + half,
        k: o.k,
        start,
        end,
        rows,
        first,
    }
}

/// Centro Y de la fila `k`.
fn menu_row_cy(g: &MenuGeom, k: usize) -> f64 {
    g.cy - (g.rows as f64 - 1.0) * g.row_h / 2.0 + k as f64 * g.row_h
}

/// ¿Sobre qué ítem del menú cae el punto (px del OSD)? None = fuera.
fn menu_hit(px: f64, py: f64) -> Option<usize> {
    MENU.with(|m| {
        let b = m.borrow();
        let menu = b.as_ref()?;
        let g = menu_geom(menu);
        if px < g.l || px > g.r {
            return None;
        }
        for (j, i) in (g.start..g.end).enumerate() {
            let y = menu_row_cy(&g, g.first + j);
            if (py - y).abs() <= g.row_h / 2.0 {
                return Some(i);
            }
        }
        None
    })
}

/// Fondo del menú: panel centrado, alto según filas visibles.
fn build_menu_bg(g: &MenuGeom) -> String {
    let h = (g.rows as f64 * g.row_h + 40.0 * g.k).min(g.cy * 2.0 - 16.0);
    let top = (g.cy - h / 2.0) as i32;
    let bot = (g.cy + h / 2.0) as i32;
    let (l, r) = (g.l as i32, g.r as i32);
    format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H120F0A&\\1a&H22&\\p1}}\
         m {l} {top} l {r} {top} {r} {bot} {l} {bot}{{\\p0}}"
    )
}

/// Texto del menú: un evento ASS por fila en su Y real (título + ventana de
/// ítems con indicadores ▲/▼), para que hover y click sepan dónde caen.
fn build_menu_ass(menu: &Menu, g: &MenuGeom) -> String {
    let n = menu.items.len();
    let cx = ((g.l + g.r) / 2.0) as i32;
    let mut ev: Vec<String> = Vec::new();
    let row = |k: usize, body: String| {
        format!(
            "{{\\an5\\pos({cx},{})\\bord0\\shad1.2\\4c&H000000&}}{body}",
            menu_row_cy(g, k) as i32
        )
    };
    let title_fs = fs(g.k, 32.0);
    ev.push(row(
        0,
        format!(
            "{{\\fs{title_fs}\\1c&H3BA1F9&\\b1}}{}",
            ellipsize(&menu.title, (g.r - g.l) - 40.0 * g.k, title_fs)
        ),
    ));
    if g.start > 0 {
        ev.push(row(
            2,
            format!("{{\\fs{}\\1c&H9A9488&\\b0}}▲  {} más", fs(g.k, 22.0), g.start),
        ));
    }
    let item_fs = fs(g.k, 28.0);
    let item_w = (g.r - g.l) - 40.0 * g.k;
    for (j, i) in (g.start..g.end).enumerate() {
        let label = ellipsize(&menu.items[i].label, item_w, item_fs);
        let body = if i == menu.sel {
            format!("{{\\fs{item_fs}\\1c&H1673F9&\\b1}}▸  {label}")
        } else {
            format!("{{\\fs{item_fs}\\1c&HE8E0D0&\\b0}}     {label}")
        };
        ev.push(row(g.first + j, body));
    }
    if g.end < n {
        ev.push(row(
            g.first + (g.end - g.start),
            format!(
                "{{\\fs{}\\1c&H9A9488&\\b0}}▼  {} más",
                fs(g.k, 22.0),
                n - g.end
            ),
        ));
    }
    ev.join("\n")
}

fn draw_menu() {
    let drawn = MENU.with(|m| {
        m.borrow().as_ref().map(|menu| {
            let g = menu_geom(menu);
            (build_menu_bg(&g), build_menu_ass(menu, &g))
        })
    });
    if let Some((bg, fg)) = drawn {
        put_overlay(BAR_BG_ID, &bg);
        put_overlay(BAR_FG_ID, &fg);
        // El menú reusa los ids 46/47, pero el riel es el 48: sin esto quedaría
        // flotando encima del panel de pistas. close_menu lo repone.
        hide_overlay(BAR_SEEK_ID);
    }
}

/// Mueve la selección del menú a un ítem concreto (hover del mouse).
fn menu_select(i: usize) {
    let changed = MENU.with(|m| {
        let mut b = m.borrow_mut();
        match b.as_mut() {
            Some(menu) if menu.sel != i && i < menu.items.len() => {
                menu.sel = i;
                true
            }
            _ => false,
        }
    });
    if changed {
        draw_menu();
    }
}

/// Resetea el estado del bar al empezar una reproducción (hilo main).
fn reset_player_ui() {
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        b.paused = false;
        b.bar_visible = false;
        b.focus = 1;
    });
    MENU.with(|m| *m.borrow_mut() = None);
    clear_bar();
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
    let found = cands.into_iter().find(|c| c.exists());
    // Se avisa SIEMPRE. Que esto cayera en None sin decir nada costó caro: el
    // 2º candidato es el CARGO_MANIFEST_DIR compilado, que existe en la máquina
    // de desarrollo y en ninguna otra. Repartiendo el binario suelto sin
    // vendor/ al lado, mpv arrancaba sin mpv.conf y sin fuentes, y el síntoma
    // aparecía a kilómetros del origen.
    match &found {
        Some(p) => eprintln!("[mpv] config-dir: {}", p.display()),
        None => eprintln!(
            "[mpv] SIN config-dir: no encontré vendor/mpv-config. mpv arranca sin \
             mpv.conf (sin network-timeout ni cache). Deja vendor/ junto al ejecutable."
        ),
    }
    found
}

/// Ruta del yt-dlp vendorizado, para que ytdl_hook lo encuentre sin depender
/// del PATH del sistema. Sin esto los trailers de YouTube no cargan en una
/// máquina que no traiga yt-dlp instalado.
fn ytdlp_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    #[cfg(windows)]
    let exe = "yt-dlp.exe";
    #[cfg(not(windows))]
    let exe = "yt-dlp";
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("vendor").join(exe));
    }
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor").join(exe));
    if let Ok(p) = std::env::current_exe() {
        if let Some(dir) = p.parent() {
            cands.push(dir.join("vendor").join(exe));
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
    let ytdlp = ytdlp_path(app);
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vo", "libmpv")?;
        init.set_option("config", "yes")?;
        if let Some(dir) = &cfg {
            init.set_option("config-dir", dir.to_string_lossy().as_ref())?;
        }
        // uosc fuera: es mouse-first y duplicaba la UI (dos barras distintas,
        // y la suya no puede pedir subtítulos porque un script Lua no emite
        // eventos Tauri). Nuestro bar es ahora la única UI, mouse + teclado.
        // El camino no-Linux (mpv proceso externo) sí sigue cargando uosc.
        init.set_option("load-scripts", "no")?;
        init.set_option("terminal", "no")?;
        init.set_option("force-window", "no")?;
        // El idle mantiene mpv vivo entre archivos sin cerrar el render context.
        init.set_option("idle", "yes")?;
        // CLAVE: sin esto, render() bloquea el hilo main hasta el tiempo de
        // display de cada frame (la doc de la Render API lo dice) → con red
        // lenta la UI se cuelga. En 0 no bloquea; el ritmo lo da queue_render.
        init.set_option("video-timing-offset", "0")?;
        // Trailers de YouTube: ytdl_hook mezcla los streams DASH (video y audio
        // van por separado, ya casi no hay formatos progresivos) — es la única
        // vía que los reproduce completos. Le damos la ruta del yt-dlp propio.
        if let Some(y) = &ytdlp {
            init.set_option(
                "script-opts",
                format!("ytdl_hook-ytdl_path={}", y.to_string_lossy()).as_str(),
            )?;
        }
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
        let kn = ev.keyval().name();
        let kn = kn.as_deref().unwrap_or("");
        // Si hay un menú de pistas abierto, las teclas lo navegan (↑/↓/Enter),
        // y Esc/←/Backspace lo CIERRAN (vuelven al bar, sin salir del video).
        let menu_open = MENU.with(|m| m.borrow().is_some());
        if menu_open {
            match kn {
                "Up" => menu_move(-1),
                "Down" => menu_move(1),
                "Return" | "KP_Enter" => menu_activate(),
                "Escape" | "BackSpace" | "Left" => close_menu(),
                _ => {}
            }
            return glib::Propagation::Stop;
        }
        // Esc/Backspace: volver a los menús DEJANDO la sesión viva (pausada, o
        // sonando si es IPTV). No van a mpv → no lo matan. Cerrar del todo es
        // el botón ✕ Salir del bar.
        if matches!(kn, "Escape" | "BackSpace") {
            let _ = suspend();
            return glib::Propagation::Stop;
        }
        // Espacio: alterna pausa/bar en cualquier modo.
        if kn == "space" {
            toggle_pause();
            return glib::Propagation::Stop;
        }
        // Atajos directos a los menús propios (antes los servía uosc).
        match kn {
            "c" => {
                open_sub_menu();
                return glib::Propagation::Stop;
            }
            "a" => {
                open_audio_menu();
                return glib::Propagation::Stop;
            }
            "v" => {
                open_video_menu();
                return glib::Propagation::Stop;
            }
            _ => {}
        }
        let paused = PLAYER_UI.with(|u| u.borrow().paused);
        if !paused {
            // REPRODUCIENDO: flechas = seek/volumen, OK = pausar + bar.
            match kn {
                "Left" => mpv_cmd2("seek", &["-10"]),
                "Right" => mpv_cmd2("seek", &["10"]),
                "Up" => mpv_cmd2("add", &["volume", "5"]),
                "Down" => mpv_cmd2("add", &["volume", "-5"]),
                "Return" | "KP_Enter" => enter_paused(),
                _ => {
                    if let Some(k) = gdk_to_mpv_key(ev) {
                        mpv_key("keypress", &k);
                    }
                }
            }
        } else {
            // PAUSADO: ←/→ navegan el bar, OK activa (↑/↓ siguen volumen).
            match kn {
                "Left" => move_focus(-1),
                "Right" => move_focus(1),
                "Up" => mpv_cmd2("add", &["volume", "5"]),
                "Down" => mpv_cmd2("add", &["volume", "-5"]),
                "Return" | "KP_Enter" => activate_focus(),
                _ => {
                    if let Some(k) = gdk_to_mpv_key(ev) {
                        mpv_key("keypress", &k);
                    }
                }
            }
        }
        glib::Propagation::Stop
    });
    // Mouse: el bar (y el menú) son NUESTROS, dibujados en el canvas 1280×720
    // del OSD; convertimos la posición del widget a ese canvas y resolvemos el
    // hover/click acá. Mover el mouse saca el bar sin pausar (se auto-oculta).
    glarea.connect_motion_notify_event(|area, ev| {
        let (x, y) = ev.position();
        let (cx, cy) = widget_to_osd(area, x, y);
        cursor_activity(area); // muestra cursor y reprograma su autohide
        if MENU.with(|m| m.borrow().is_some()) {
            if let Some(i) = menu_hit(cx, cy) {
                menu_select(i);
            }
            return glib::Propagation::Stop;
        }
        show_bar_transient(); // cualquier movimiento lo muestra y reprograma
        if let Some(i) = bar_hit(cx, cy) {
            let changed = PLAYER_UI.with(|u| {
                let mut b = u.borrow_mut();
                let ch = b.focus != i;
                b.focus = i;
                ch
            });
            if changed {
                draw_bar();
            }
        }
        glib::Propagation::Stop
    });
    glarea.connect_button_press_event(|area, ev| {
        area.grab_focus();
        if ev.button() != 1 {
            return glib::Propagation::Stop;
        }
        let (x, y) = ev.position();
        let (cx, cy) = widget_to_osd(area, x, y);
        if MENU.with(|m| m.borrow().is_some()) {
            // Click en un ítem lo activa; fuera del panel cierra el menú.
            match menu_hit(cx, cy) {
                Some(i) => {
                    menu_select(i);
                    menu_activate();
                }
                None => close_menu(),
            }
            return glib::Propagation::Stop;
        }
        // Click en el riel = saltar ahí. Va ANTES que los iconos porque la
        // fila del riel cae dentro de la pill.
        if PLAYER_UI.with(|u| u.borrow().bar_visible) {
            if let Some(frac) = seek_hit(&bar_geom(), cx, cy) {
                seek_to_fraction(frac);
                draw_bar();
                schedule_bar_hide();
                return glib::Propagation::Stop;
            }
        }
        match bar_hit(cx, cy) {
            Some(i) => {
                PLAYER_UI.with(|u| u.borrow_mut().focus = i);
                draw_bar();
                activate_focus();
            }
            // Click en el video: pausa/reanuda, como cualquier reproductor.
            None => toggle_pause(),
        }
        glib::Propagation::Stop
    });
    // Rueda: volumen (con el menú abierto, recorre la lista).
    glarea.connect_scroll_event(|_area, ev| {
        let up = match ev.direction() {
            gtk::gdk::ScrollDirection::Up => true,
            gtk::gdk::ScrollDirection::Down => false,
            _ => return glib::Propagation::Stop,
        };
        if MENU.with(|m| m.borrow().is_some()) {
            menu_move(if up { -1 } else { 1 });
        } else {
            mpv_cmd2("add", &["volume", if up { "5" } else { "-5" }]);
            show_bar_transient();
        }
        glib::Propagation::Stop
    });
    glarea.connect_leave_notify_event(|_area, _ev| {
        // El puntero salió del video: el bar se va (salvo pausa o menú).
        hide_bar_transient();
        glib::Propagation::Proceed
    });

    // La geometría de bar y menús se calcula contra el OSD real, así que al
    // redimensionar hay que redibujar (diferido: el resize corre en el render).
    glarea.connect_resize(|_area, _w, _h| {
        glib::idle_add_local_once(|| {
            if MENU.with(|m| m.borrow().is_some()) {
                draw_menu();
            } else if PLAYER_UI.with(|u| u.borrow().bar_visible) {
                draw_bar();
            }
        });
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
            reset_player_ui(); // arranca reproduciendo, bar oculto
            SURFACE.with(|s| {
                if let Some(surf) = s.borrow().as_ref() {
                    // Oculta el webview y muestra el video (ocupa todo el box).
                    if let Some(wv) = &surf.webview {
                        wv.hide();
                    }
                    surf.glarea.show();
                    surf.glarea.grab_focus();
                    surf.glarea.queue_render();
                    set_cursor_hidden(&surf.glarea, true); // arranca sin cursor
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
                    set_cursor_hidden(&surf.glarea, false); // restaura cursor
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

/// `network-timeout` tal como lo dejó mpv.conf, leído la primera vez que se
/// reproduce algo. Ver el uso en `play()`.
static NET_TIMEOUT_CONF: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Reproduce una URL única. Reemplaza lo que estuviera sonando.
pub fn play(url: &str, title: Option<&str>, start_secs: Option<u64>) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    // Empezar algo nuevo descarta cualquier trailer y su sesión guardada.
    TRAILER.store(false, Ordering::SeqCst);
    clear_pending();
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
    // Stream del cliente torrent local (127.0.0.1): las pausas ahí NO son
    // problemas de red sino piezas que aún no llegan del swarm, y pueden durar
    // bastante más que el network-timeout=15 de mpv.conf. Sin esto mpv aborta
    // la reproducción a mitad de un tramo lento. Para todo lo demás (debrid,
    // IPTV) el timeout corto sigue siendo lo correcto: ahí un corte largo sí
    // es un servidor caído y conviene fallar rápido.
    let local = url.starts_with("http://127.0.0.1:");
    // El valor de mpv.conf se guarda la primera vez para poder restaurarlo sin
    // duplicar la constante acá.
    let por_defecto = NET_TIMEOUT_CONF.get_or_init(|| {
        mpv.get_property::<String>("network-timeout")
            .unwrap_or_else(|_| "15".into())
    });
    let _ = mpv.set_property(
        "network-timeout",
        if local { "0" } else { por_defecto.as_str() },
    );
    // Portada encima antes de soltar el archivo viejo: si no, se ve congelado
    // el último frame de la película anterior mientras el nuevo abre.
    begin_loading(title);
    eprintln!("[mpv-embed] play loadfile…");
    mpv.command("loadfile", &[url, "replace"])
        .map_err(|e| format!("loadfile: {e}"))?;
    eprintln!("[mpv-embed] play loadfile OK → show_surface");
    LIVE.store(false, Ordering::SeqCst);
    SUSPENDED.store(false, Ordering::SeqCst);
    let _ = mpv.set_property("pause", false);
    show_surface();
    notify_state(true);
    eprintln!("[mpv-embed] play listo");
    Ok(())
}

/// Reproduce un playlist m3u (IPTV) arrancando en `start`.
pub fn play_iptv(playlist_path: &str, start: usize) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    TRAILER.store(false, Ordering::SeqCst);
    clear_pending();
    let _ = mpv.set_property("playlist-start", start as i64);
    begin_loading(None);
    mpv.command("loadlist", &[playlist_path, "replace"])
        .map_err(|e| format!("loadlist: {e}"))?;
    LIVE.store(true, Ordering::SeqCst);
    SUSPENDED.store(false, Ordering::SeqCst);
    let _ = mpv.set_property("pause", false);
    show_surface();
    notify_state(true);
    Ok(())
}

/// Reproduce un trailer SIN destruir lo que estuviera en curso.
///
/// Un trailer no es una película: al salir no debe quedarse "en espera" en la
/// pill, y no puede llevarse por delante la película que sí estaba esperando.
/// Como mpv es una sola instancia, guardamos qué había cargado (ruta + posición)
/// y lo recargamos al terminar el trailer, otra vez en pausa y fuera de pantalla.
pub fn play_trailer(url: &str, title: Option<&str>) -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    // Un trailer sobre otro trailer no pisa la sesión guardada.
    if !TRAILER.load(Ordering::SeqCst) {
        let path = get_string("path");
        let pending = (RUNNING.load(Ordering::SeqCst) || SUSPENDED.load(Ordering::SeqCst))
            .then(|| ())
            .filter(|_| !path.is_empty())
            .map(|_| Pending {
                path,
                pos: get_f64("time-pos"),
                title: get_string("media-title"),
                live: LIVE.load(Ordering::SeqCst),
            });
        set_pending(pending);
    }
    TRAILER.store(true, Ordering::SeqCst);
    if let Some(t) = title {
        let _ = mpv.set_property("force-media-title", t);
    }
    let _ = mpv.set_property("start", "none");
    let _ = mpv.set_property("pause", false);
    begin_loading(title);
    mpv.command("loadfile", &[url, "replace"])
        .map_err(|e| format!("loadfile trailer: {e}"))?;
    LIVE.store(false, Ordering::SeqCst);
    SUSPENDED.store(false, Ordering::SeqCst);
    show_surface();
    notify_state(true);
    notify_session();
    watch_trailer_end();
    Ok(())
}

/// Cuando el trailer llega al final se cierra solo (mpv queda idle con pantalla
/// negra si no). No hay bucle de eventos de libmpv en este módulo, así que lo
/// sondeamos desde el hilo GTK una vez por segundo mientras dure el trailer.
fn watch_trailer_end() {
    let Some(app) = APP.get() else { return };
    let _ = app.run_on_main_thread(|| {
        glib::timeout_add_local(std::time::Duration::from_millis(1000), || {
            if !TRAILER.load(Ordering::SeqCst) {
                return glib::ControlFlow::Break;
            }
            if get_bool("idle-active") || get_bool("eof-reached") {
                let _ = end_trailer();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    });
}

/// Cierra el trailer: si había algo esperando lo devuelve a la pill (pausado en
/// su minuto), y si no, sale a los menús sin dejar nada colgando.
fn end_trailer() -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    TRAILER.store(false, Ordering::SeqCst);
    end_loading();
    // Overlays del bar/menú fuera antes de ocultar (viven en el hilo GTK).
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(reset_player_ui);
    }
    let Some(p) = take_pending() else {
        return stop_inner();
    };
    // Un canal IPTV vuelve sonando; una película vuelve pausada en su minuto.
    let _ = mpv.set_property("pause", !p.live);
    let _ = mpv.set_property("force-media-title", p.title.as_str());
    if p.live || p.pos <= 0.0 {
        let _ = mpv.set_property("start", "none");
    } else {
        let _ = mpv.set_property("start", format!("+{}", p.pos as u64).as_str());
    }
    mpv.command("loadfile", &[p.path.as_str(), "replace"])
        .map_err(|e| format!("loadfile restaurar: {e}"))?;
    LIVE.store(p.live, Ordering::SeqCst);
    SUSPENDED.store(true, Ordering::SeqCst);
    hide_surface();
    notify_state(false);
    notify_session();
    Ok(())
}

fn set_pending(p: Option<Pending>) {
    if let Ok(mut g) = PENDING.lock() {
        *g = p;
    }
}

fn take_pending() -> Option<Pending> {
    PENDING.lock().ok().and_then(|mut g| g.take())
}

fn clear_pending() {
    set_pending(None);
}

/// Para la reproducción y oculta la superficie (vuelve al webview).
///
/// Durante un trailer significa "cerrar el trailer": la película que estaba
/// esperando vuelve a la pill en vez de morir con él.
pub fn stop() -> Result<(), String> {
    if TRAILER.load(Ordering::SeqCst) {
        return end_trailer();
    }
    stop_inner()
}

fn stop_inner() -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    end_loading();
    let _ = mpv.command("stop", &[]);
    TRAILER.store(false, Ordering::SeqCst);
    clear_pending();
    SUSPENDED.store(false, Ordering::SeqCst);
    LIVE.store(false, Ordering::SeqCst);
    hide_surface();
    notify_state(false);
    notify_session();
    Ok(())
}

/// Esc: vuelve a los menús SIN cerrar el archivo. Película → pausa; IPTV →
/// sigue en vivo de fondo (se oye mientras navegas). La superficie se oculta y
/// el webview recupera el teclado, igual que en un stop, pero la sesión queda
/// lista para `resume()`.
pub fn suspend() -> Result<(), String> {
    // Esc en un trailer no lo deja esperando: lo cierra y punto.
    if TRAILER.load(Ordering::SeqCst) {
        return end_trailer();
    }
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    if !RUNNING.load(Ordering::SeqCst) {
        return Ok(());
    }
    if !LIVE.load(Ordering::SeqCst) {
        let _ = mpv.set_property("pause", true);
    }
    end_loading();
    // Overlays del bar/menú fuera antes de ocultar (viven en el hilo GTK).
    if let Some(app) = APP.get() {
        let _ = app.run_on_main_thread(reset_player_ui);
    }
    SUSPENDED.store(true, Ordering::SeqCst);
    hide_surface();
    notify_state(false);
    notify_session();
    Ok(())
}

/// Retoma la sesión suspendida: vuelve a mostrar el video y despausa.
pub fn resume() -> Result<(), String> {
    let mpv = MPV.get().ok_or("mpv no inicializado")?;
    if !SUSPENDED.load(Ordering::SeqCst) {
        return Err("no hay reproducción en pausa".into());
    }
    SUSPENDED.store(false, Ordering::SeqCst);
    let _ = mpv.set_property("pause", false);
    show_surface();
    notify_state(true);
    notify_session();
    Ok(())
}

/// ¿Hay una sesión suspendida esperando volver?
pub fn is_suspended() -> bool {
    SUSPENDED.load(Ordering::SeqCst)
}

/// ¿La sesión actual es IPTV (vivo)?
pub fn is_live() -> bool {
    LIVE.load(Ordering::SeqCst)
}

/// Avisa a la UI de que cambió la sesión suspendida (la pill se redibuja).
fn notify_session() {
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit("mpv:suspended", SUSPENDED.load(Ordering::SeqCst));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// BarGeom de laboratorio: 7 iconos en 1280×720 (k = 1), con riel.
    fn geom_con_riel(pos: f64, dur: f64) -> BarGeom {
        let osd = Osd { w: 1280.0, h: 720.0, k: 1.0 };
        let bot = 720.0 - 54.0;
        let top = bot - 208.0;
        let mut g = BarGeom {
            n: 7,
            step: 96.0,
            top,
            bot,
            icon_cy: bot - 100.0,
            label_y: bot - 38.0,
            center_x: 640.0,
            k: 1.0,
            osd,
            seek: None,
        };
        let (l, r) = pill_x(&g, true);
        g.seek = Some(SeekGeom {
            cy: top + 34.0,
            track_l: l + 96.0,
            track_r: r - 96.0,
            h: 6.0,
            pos,
            dur,
        });
        g
    }

    #[test]
    fn fmt_hms_omite_la_hora_si_no_hay() {
        assert_eq!(fmt_hms(0.0), "0:00");
        assert_eq!(fmt_hms(65.4), "1:05");
        assert_eq!(fmt_hms(3600.0), "1:00:00");
        assert_eq!(fmt_hms(5025.0), "1:23:45");
        // Un time-pos negativo no debe producir un desbordamiento.
        assert_eq!(fmt_hms(-3.0), "0:00");
    }

    #[test]
    fn seek_hit_mapea_los_extremos_y_el_centro() {
        let g = geom_con_riel(0.0, 100.0);
        let s = g.seek.unwrap();
        assert_eq!(seek_hit(&g, s.track_l, s.cy), Some(0.0));
        assert_eq!(seek_hit(&g, s.track_r, s.cy), Some(1.0));
        let mid = seek_hit(&g, (s.track_l + s.track_r) / 2.0, s.cy).unwrap();
        assert!((mid - 0.5).abs() < 1e-9, "centro dio {mid}");
    }

    #[test]
    fn seek_hit_recorta_fuera_del_riel_pero_no_fuera_de_la_fila() {
        let g = geom_con_riel(0.0, 100.0);
        let s = g.seek.unwrap();
        // A los costados del riel (sobre los tiempos) el valor se recorta,
        // no se sale del rango.
        assert_eq!(seek_hit(&g, s.track_l - 60.0, s.cy), Some(0.0));
        assert_eq!(seek_hit(&g, s.track_r + 60.0, s.cy), Some(1.0));
        // Fuera de la banda vertical no hay acierto: esa zona es de los iconos.
        assert!(seek_hit(&g, s.track_l, s.cy + 40.0).is_none());
        assert!(seek_hit(&g, s.track_l, g.icon_cy).is_none());
    }

    #[test]
    fn sin_riel_no_hay_acierto() {
        let mut g = geom_con_riel(0.0, 100.0);
        let cy = g.seek.unwrap().cy;
        g.seek = None;
        assert!(seek_hit(&g, 640.0, cy).is_none());
    }

    #[test]
    fn el_riel_cabe_dentro_de_la_pill() {
        let g = geom_con_riel(10.0, 100.0);
        let (l, r) = pill_x(&g, true);
        let s = g.seek.unwrap();
        assert!(s.track_l > l, "riel se sale por la izquierda");
        assert!(s.track_r < r, "riel se sale por la derecha");
        assert!(s.cy > g.top && s.cy < g.icon_cy, "la fila pisa los iconos");
    }

    #[test]
    fn todo_icono_tiene_trazos_dentro_de_su_caja() {
        for a in [
            BarAction::SeekBack,
            BarAction::Resume,
            BarAction::SeekFwd,
            BarAction::Subs,
            BarAction::Audio,
            BarAction::Video,
            BarAction::Exit,
        ] {
            let polys = a.shapes();
            assert!(!polys.is_empty(), "{:?} sin trazos", a.label());
            for p in &polys {
                // Menos de 3 vértices no rellena nada: sale un icono invisible.
                assert!(p.len() >= 3, "{} tiene un polígono de {} vértices", a.label(), p.len());
                for (x, y) in p {
                    assert!(
                        x.abs() <= 50.0 && y.abs() <= 50.0,
                        "{} se sale de la caja 100×100 en ({x},{y})",
                        a.label()
                    );
                }
            }
        }
    }

    #[test]
    fn icon_drawing_escala_y_centra() {
        // Caja de 100 px centrada en (500,300): el triángulo de play llega
        // hasta x = 500 + 36 y no más.
        let d = icon_drawing(BarAction::Resume, 500.0, 300.0, 100.0);
        assert!(d.starts_with("m 472.0 262.0"), "salió: {d}");
        assert!(d.contains("l 536.0 300.0"), "salió: {d}");
        // A la mitad de tamaño, la mitad de desplazamiento respecto al centro.
        let mitad = icon_drawing(BarAction::Resume, 500.0, 300.0, 50.0);
        assert!(mitad.starts_with("m 486.0 281.0"), "salió: {mitad}");
    }

    #[test]
    fn cada_poligono_abre_su_propio_contorno() {
        // Dos triángulos = dos `m`. Si se encadenaran con `l`, libass los
        // uniría en una figura sola y saldría un borrón.
        let d = icon_drawing(BarAction::SeekFwd, 0.0, 0.0, 100.0);
        assert_eq!(d.matches("m ").count(), 2, "salió: {d}");
    }

    #[test]
    fn ass_round_rect_recorta_el_radio_a_la_mitad_del_lado() {
        // Radio absurdo sobre una caja chica: no debe generar coordenadas
        // cruzadas (que libass dibujaría como un borrón).
        let d = ass_round_rect(0, 0, 10, 10, 999);
        assert!(d.starts_with("m 0 5"), "salió: {d}");
    }
}

#[cfg(test)]
mod icon_dump {
    use super::*;

    /// Escupe un .ass con los 7 iconos para mirarlos renderizados de verdad:
    ///   cargo test --lib volcar_iconos -- --ignored --nocapture
    ///   mpv --sub-file=/tmp/iconos.ass --vo=image ...
    #[test]
    #[ignore]
    fn volcar_iconos() {
        let acts = [
            BarAction::SeekBack,
            BarAction::Resume,
            BarAction::SeekFwd,
            BarAction::Subs,
            BarAction::Audio,
            BarAction::Video,
            BarAction::Exit,
        ];
        let mut ev = String::new();
        for (i, a) in acts.iter().enumerate() {
            let cx = 130.0 + i as f64 * 170.0;
            ev.push_str(&format!(
                "Dialogue: 0,0:00:00.00,0:00:05.00,D,,0,0,0,,{{\\an7\\pos(0,0)\\bord0\\shad1.5\\4c&H000000&\\1c&HE8E0D0&\\p1}}{}{{\\p0}}\n",
                icon_drawing(*a, cx, 200.0, 96.0)
            ));
        }
        let doc = format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 400\n\n\
             [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, Alignment, MarginL, MarginR, MarginV, Encoding\n\
             Style: D,Arial,40,&H00FFFFFF,7,0,0,0,1\n\n\
             [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{ev}"
        );
        std::fs::write("/tmp/iconos.ass", doc).unwrap();
        println!("escrito /tmp/iconos.ass");
    }
}
