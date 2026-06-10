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
    Exit,
}

impl BarAction {
    fn label(self) -> &'static str {
        match self {
            BarAction::SeekBack => "« 10s",
            BarAction::Resume => "Reanudar",
            BarAction::SeekFwd => "10s »",
            BarAction::Subs => "Subtítulos",
            BarAction::Audio => "Audio",
            BarAction::Video => "Video",
            BarAction::Exit => "Salir",
        }
    }
}

struct PlayerUi {
    paused: bool,
    focus: usize,
    /// Acciones visibles del bar (se reconstruyen al pausar según las pistas).
    bar: Vec<BarAction>,
}

thread_local! {
    static PLAYER_UI: RefCell<PlayerUi> =
        const { RefCell::new(PlayerUi { paused: false, focus: 1, bar: Vec::new() }) };
    /// Menú de pistas abierto (audio/subs). None = no hay menú (modo bar).
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
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

// osd-overlay ids: 46 = fondo (pill), 47 = textos. Canvas virtual 1280×720.
const BAR_BG_ID: &str = "46";
const BAR_FG_ID: &str = "47";

/// Fondo: pill inferior oscuro semi-transparente (ASS drawing con esquinas).
fn build_bar_bg() -> String {
    // \1a = transparencia (00 opaco … FF transp). Dark #0A0F12 → &H120F0A&.
    "{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H120F0A&\\1a&H35&\\p1}\
     m 150 560 b 150 542 164 528 182 528 l 1098 528 b 1116 528 1130 542 1130 560 \
     l 1130 612 b 1130 630 1116 644 1098 644 l 182 644 b 164 644 150 630 150 612{\\p0}"
        .to_string()
}

/// Textos del bar: acción enfocada en naranja+negrita, resto cream. Separadores
/// tenues. Sombra para legibilidad sobre el video.
fn build_bar_ass(focus: usize) -> String {
    // Colores ASS = &HBBGGRR&. Naranja f97316 → &H1673F9&. Cream → &HE8E0D0&.
    let bar = PLAYER_UI.with(|u| u.borrow().bar.clone());
    let mut s = String::from("{\\an5\\pos(640,586)\\fs30\\bord0\\shad1.2\\4c&H000000&}");
    for (i, act) in bar.iter().enumerate() {
        if i > 0 {
            s.push_str("{\\1c&H6B6256&\\b0}   ·   ");
        }
        let label = act.label();
        if i == focus {
            s.push_str(&format!("{{\\1c&H1673F9&\\b1}}{label}"));
        } else {
            s.push_str(&format!("{{\\1c&HE8E0D0&\\b0}}{label}"));
        }
    }
    s
}

/// Dibuja/actualiza el bar (fondo + textos).
fn draw_bar() {
    let focus = PLAYER_UI.with(|u| u.borrow().focus);
    let bg = build_bar_bg();
    mpv_cmd2("osd-overlay", &[BAR_BG_ID, "ass-events", &bg, "1280", "720", "0", "no", "no"]);
    let fg = build_bar_ass(focus);
    mpv_cmd2("osd-overlay", &[BAR_FG_ID, "ass-events", &fg, "1280", "720", "0", "no", "no"]);
}

/// Quita el bar (fondo + textos).
fn clear_bar() {
    mpv_cmd2("osd-overlay", &[BAR_BG_ID, "none", "", "1280", "720", "0", "no", "no"]);
    mpv_cmd2("osd-overlay", &[BAR_FG_ID, "none", "", "1280", "720", "0", "no", "no"]);
}

/// Pausa y muestra el bar (foco en Reanudar). Reconstruye las acciones según
/// las pistas actuales (ej. añade "Video" si hay 2+).
fn enter_paused() {
    let bar = build_bar_actions();
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        b.paused = true;
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
    PLAYER_UI.with(|u| u.borrow_mut().paused = false);
    if let Some(mpv) = MPV.get() {
        let _ = mpv.set_property("pause", false);
    }
    clear_bar();
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
        Some(BarAction::Resume) => resume_play(),
        Some(BarAction::SeekFwd) => {
            mpv_cmd2("seek", &["10"]);
            mpv_cmd2("show-text", &["⏩ +10s   ${time-pos}", "1200"]);
        }
        Some(BarAction::Subs) => open_sub_menu(),
        Some(BarAction::Audio) => open_audio_menu(),
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

/// Fondo del menú: panel centrado, alto según filas visibles.
fn build_menu_bg(rows: usize) -> String {
    let h = ((rows as i32) * 44 + 40).clamp(160, 660);
    let cy = 360;
    let (top, bot) = (cy - h / 2, cy + h / 2);
    format!(
        "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H120F0A&\\1a&H22&\\p1}}\
         m 300 {top} l 980 {top} 980 {bot} 300 {bot}{{\\p0}}"
    )
}

/// Texto del menú: título + ventana de ítems (scroll) con indicadores ▲/▼.
fn build_menu_ass(menu: &Menu) -> String {
    let n = menu.items.len();
    let (start, end) = menu_window(menu);
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("{{\\fs32\\1c&H3BA1F9&\\b1}}{}", menu.title));
    lines.push(String::new());
    if start > 0 {
        lines.push(format!("{{\\fs22\\1c&H9A9488&\\b0}}▲  {} más", start));
    }
    for i in start..end {
        let it = &menu.items[i];
        if i == menu.sel {
            lines.push(format!("{{\\fs28\\1c&H1673F9&\\b1}}▸  {}", it.label));
        } else {
            lines.push(format!("{{\\fs28\\1c&HE8E0D0&\\b0}}     {}", it.label));
        }
    }
    if end < n {
        lines.push(format!("{{\\fs22\\1c&H9A9488&\\b0}}▼  {} más", n - end));
    }
    format!(
        "{{\\an5\\pos(640,360)\\bord0\\shad1.2\\4c&H000000&}}{}",
        lines.join("\\N")
    )
}

fn draw_menu() {
    let drawn = MENU.with(|m| {
        m.borrow().as_ref().map(|menu| {
            let n = menu.items.len();
            let (start, end) = menu_window(menu);
            let rows = 2 + (start > 0) as usize + (end - start) + (end < n) as usize;
            (build_menu_bg(rows), build_menu_ass(menu))
        })
    });
    if let Some((bg, fg)) = drawn {
        mpv_cmd2("osd-overlay", &[BAR_BG_ID, "ass-events", &bg, "1280", "720", "0", "no", "no"]);
        mpv_cmd2("osd-overlay", &[BAR_FG_ID, "ass-events", &fg, "1280", "720", "0", "no", "no"]);
    }
}

/// Resetea el estado del bar al empezar una reproducción (hilo main).
fn reset_player_ui() {
    PLAYER_UI.with(|u| {
        let mut b = u.borrow_mut();
        b.paused = false;
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
        // Esc/Backspace: salir del video a menús (no van a mpv → no lo matan).
        if matches!(kn, "Escape" | "BackSpace") {
            let _ = stop();
            return glib::Propagation::Stop;
        }
        // Espacio: alterna pausa/bar en cualquier modo.
        if kn == "space" {
            toggle_pause();
            return glib::Propagation::Stop;
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
