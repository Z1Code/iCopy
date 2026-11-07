//! Espacio libre en el disco destino.
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// Margen que siempre se deja libre: 1 GiB.
pub const MARGEN: u64 = 1 << 30;

pub fn espacio_libre(ruta: &Path) -> std::io::Result<u64> {
    let ancho: Vec<u16> = ruta.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut libre: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(ancho.as_ptr(), &mut libre, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(libre)
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
