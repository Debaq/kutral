// Nombres exportados por un DLL de 64 bits (PE32+). Lo usa build.rs para
// armar el `.def` de libmpv-2.dll: el build de mpv para Windows trae la
// biblioteca de importación de MinGW (libmpv.dll.a), pero MSVC necesita un
// `.lib`, que `lib.exe /def` genera a partir de esta lista.

fn u16_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

pub fn exportados(b: &[u8]) -> Option<Vec<String>> {
    let pe = u32_at(b, 0x3c)?;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let n_secciones = u16_at(b, coff + 2)?;
    let opt = coff + 20;
    if u16_at(b, opt)? != 0x20b {
        return None; // no es PE32+ (64 bits)
    }
    let tam_opt = u16_at(b, coff + 16)?;
    // Directorio de datos 0 (exportaciones), en el encabezado opcional PE32+.
    let rva_exp = u32_at(b, opt + 112)?;
    let secciones = opt + tam_opt;
    // RVA → posición en el archivo, por la sección que la contiene.
    let a_archivo = |rva: usize| -> Option<usize> {
        (0..n_secciones).find_map(|i| {
            let s = secciones + i * 40;
            let (vtam, vdir) = (u32_at(b, s + 8)?, u32_at(b, s + 12)?);
            let (rtam, rpos) = (u32_at(b, s + 16)?, u32_at(b, s + 20)?);
            (rva >= vdir && rva < vdir + vtam.max(rtam)).then(|| rva - vdir + rpos)
        })
    };
    let exp = a_archivo(rva_exp)?;
    let n_nombres = u32_at(b, exp + 24)?;
    let nombres = a_archivo(u32_at(b, exp + 32)?)?;
    (0..n_nombres)
        .map(|i| {
            let p = a_archivo(u32_at(b, nombres + i * 4)?)?;
            let fin = b[p..].iter().position(|&c| c == 0)?;
            String::from_utf8(b[p..p + fin].to_vec()).ok()
        })
        .collect()
}
