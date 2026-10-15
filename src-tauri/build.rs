#[path = "build/pe_exports.rs"]
mod pe_exports;

fn main() {
    // `cfg(embed)`: reproductor libmpv embebido (mpv_embed). Siempre en
    // Linux; en Windows con la feature embed-win.
    println!("cargo:rustc-check-cfg=cfg(embed)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" || (os == "windows" && std::env::var_os("CARGO_FEATURE_EMBED_WIN").is_some()) {
        println!("cargo:rustc-cfg=embed");
    }
    // Reproductor embebido en Windows (feature `embed-win`, en pruebas):
    // libmpv2-sys enlaza contra `mpv`, y con MSVC eso es un `mpv.lib`.
    if std::env::var_os("CARGO_FEATURE_EMBED_WIN").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    {
        mpv_lib_msvc();
    }
    tauri_build::build()
}

/// Arma `mpv.lib` desde `vendor/mpv-dev/libmpv-2.dll` (lo baja
/// fetch-windows.ps1): exportaciones → `.def` → `lib.exe /def`.
fn mpv_lib_msvc() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("vendor")
        .join("mpv-dev");
    let dll = dir.join("libmpv-2.dll");
    println!("cargo:rerun-if-changed={}", dll.display());
    let bytes = std::fs::read(&dll).unwrap_or_else(|e| {
        panic!("embed-win: falta {} ({e}); correr vendor/fetch-windows.ps1", dll.display())
    });
    let nombres = pe_exports::exportados(&bytes).expect("embed-win: libmpv-2.dll no es un DLL de 64 bits");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let def = out.join("mpv.def");
    let mut texto = String::from("LIBRARY libmpv-2.dll\nEXPORTS\n");
    for n in nombres.iter().filter(|n| n.starts_with("mpv_")) {
        texto.push_str(&format!("    {n}\n"));
    }
    std::fs::write(&def, texto).unwrap();
    let target = std::env::var("TARGET").unwrap();
    let mut lib = cc::windows_registry::find(&target, "lib.exe").expect("embed-win: no encontré lib.exe de MSVC");
    let ok = lib
        .arg(format!("/def:{}", def.display()))
        .arg("/machine:x64")
        .arg(format!("/out:{}", out.join("mpv.lib").display()))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "embed-win: lib.exe no pudo generar mpv.lib");
    println!("cargo:rustc-link-search=native={}", out.display());
}
