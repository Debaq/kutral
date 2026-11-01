//! Apaga el scroll asíncrono de WebKitGTK.
//!
//! WebKitGTK 2.54 se cuelga entero (deadlock) al bajar con la rueda por el
//! grid de carátulas: el hilo principal agrega capas nuevas
//! (`LayerTreeHost::attachLayer`, entran cards por el scroll infinito) y
//! espera un candado que tiene el hilo "Core: Scrolling"
//! (`ScrollingTreeCoordinated::applyLayerPositionsInternal` →
//! `LayerTreeHost::requestComposition`), que a su vez espera el del hilo
//! principal. Confirmado con `eu-stack` (2026-10-31). Sin scroll asíncrono el
//! `overflow` del grid se desplaza en el hilo principal y ese cruce no existe.
//!
//! El crate `webkit2gtk` (=2.0.2) no expone la API de features (WebKitGTK
//! 2.42+), así que se busca con `dlsym`: en sistemas con un WebKit más viejo no
//! está y no se hace nada (tampoco tienen este scroll).
//!
//! `KUTRAL_WEBKIT_SCROLL_ASYNC=1` lo deja como viene, para comparar.

use std::ffi::{c_char, c_int, c_void, CStr};

type GetAll = unsafe extern "C" fn() -> *mut c_void;
type ListLen = unsafe extern "C" fn(*mut c_void) -> usize;
type ListGet = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
type Ident = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type SetEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int);
type ListUnref = unsafe extern "C" fn(*mut c_void);

const APAGAR: &[&str] = &[
    "ThreadedScrollingEnabled",
    "AsyncOverflowScrollingEnabled",
    "AsyncFrameScrollingEnabled",
];

/// Puntero a función por nombre, del WebKit ya cargado en el proceso.
fn simbolo(nombre: &CStr) -> Option<*mut c_void> {
    let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, nombre.as_ptr()) };
    (!p.is_null()).then_some(p)
}

/// Apaga los features de scroll asíncrono en `settings`.
///
/// # Safety
///
/// `settings` debe ser un `WebKitSettings*` válido (o nulo) y llamarse desde
/// el hilo principal de GTK.
pub unsafe fn apagar_scroll_asincrono(settings: *mut c_void) {
    if settings.is_null() || std::env::var_os("KUTRAL_WEBKIT_SCROLL_ASYNC").is_some() {
        return;
    }
    unsafe {
        let (Some(get_all), Some(len), Some(get), Some(ident), Some(set), Some(unref)) = (
            simbolo(c"webkit_settings_get_all_features"),
            simbolo(c"webkit_feature_list_get_length"),
            simbolo(c"webkit_feature_list_get"),
            simbolo(c"webkit_feature_get_identifier"),
            simbolo(c"webkit_settings_set_feature_enabled"),
            simbolo(c"webkit_feature_list_unref"),
        ) else {
            eprintln!("[webkit] sin API de features (WebKitGTK < 2.42): scroll sin tocar");
            return;
        };
        let get_all = std::mem::transmute::<*mut c_void, GetAll>(get_all);
        let len = std::mem::transmute::<*mut c_void, ListLen>(len);
        let get = std::mem::transmute::<*mut c_void, ListGet>(get);
        let ident = std::mem::transmute::<*mut c_void, Ident>(ident);
        let set = std::mem::transmute::<*mut c_void, SetEnabled>(set);
        let unref = std::mem::transmute::<*mut c_void, ListUnref>(unref);

        let lista = get_all();
        if lista.is_null() {
            return;
        }
        for i in 0..len(lista) {
            let f = get(lista, i);
            if f.is_null() {
                continue;
            }
            let id = ident(f);
            if id.is_null() {
                continue;
            }
            let id = CStr::from_ptr(id).to_string_lossy();
            if APAGAR.contains(&id.as_ref()) {
                set(settings, f, 0);
                eprintln!("[webkit] {id} apagado");
            }
        }
        unref(lista);
    }
}
