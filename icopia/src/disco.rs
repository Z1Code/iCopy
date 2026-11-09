//! Espacio libre en el disco destino.
use std::path::Path;

/// Margen que siempre se deja libre: 1 GiB.
pub const MARGEN: u64 = 1 << 30;

#[cfg(windows)]
pub fn espacio_libre(ruta: &Path) -> std::io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let ancho: Vec<u16> = ruta.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut libre: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(ancho.as_ptr(), &mut libre, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(libre)
}

#[cfg(unix)]
pub fn espacio_libre(ruta: &Path) -> std::io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(ruta.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Bloques disponibles para usuarios normales × tamaño de fragmento.
    #[allow(clippy::unnecessary_cast)]
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

pub fn cabe(ruta: &Path, bytes: u64) -> bool {
    espacio_libre(ruta).map(|l| l >= bytes.saturating_add(MARGEN)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hay_espacio_en_temp() {
        let t = std::env::temp_dir();
        assert!(super::espacio_libre(&t).unwrap() > 0);
        assert!(super::cabe(&t, 1));
        assert!(!super::cabe(&t, u64::MAX / 2));
    }
}
