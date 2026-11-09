//! Lo poco que cambia entre Windows y macOS fuera de la interfaz: nombres de ejecutables, abrir
//! carpetas en el explorador del sistema e instancia única de las ventanas de la app.
use std::path::{Path, PathBuf};

/// `nombre.exe` en Windows; `nombre` en el resto.
pub fn nombre_exe(nombre: &str) -> String {
    if cfg!(windows) {
        format!("{nombre}.exe")
    } else {
        nombre.to_string()
    }
}

/// Otro ejecutable de iCopy, junto al actual (en macOS, dentro de `iCopy.app/Contents/MacOS`).
pub fn exe_junto(nombre: &str) -> PathBuf {
    std::env::current_exe().expect("ruta del ejecutable").with_file_name(nombre_exe(nombre))
}

/// Abre una carpeta o un archivo con el explorador del sistema (Explorador de Windows o Finder).
pub fn abrir(ruta: &Path) {
    let programa = if cfg!(windows) { "explorer" } else { "open" };
    let _ = std::process::Command::new(programa).arg(ruta).spawn();
}

/// Con `ICOPIA_DIR` (pruebas, otra configuración) se permite una instancia por carpeta.
fn sufijo() -> String {
    std::env::var("ICOPIA_DIR")
        .map(|d| format!("-{:x}", d.bytes().fold(0u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64))))
        .unwrap_or_default()
}

/// Mientras exista, ninguna otra instancia con el mismo nombre puede arrancar. Guárdalo vivo
/// durante todo el proceso.
pub struct InstanciaUnica {
    #[cfg(windows)]
    _mutex: isize,
    #[cfg(not(windows))]
    _archivo: std::fs::File,
}

/// `None` si ya hay otra instancia de `nombre` (p. ej. "bandeja", "login").
#[cfg(windows)]
pub fn instancia_unica(nombre: &str) -> Option<InstanciaUnica> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let ancho: Vec<u16> = format!("Local\\icopia-{nombre}{}\0", sufijo()).encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, ancho.as_ptr());
        (!h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS).then_some(InstanciaUnica { _mutex: h as isize })
    }
}

/// `None` si ya hay otra instancia de `nombre`: cerrojo de archivo en la carpeta de la app.
#[cfg(not(windows))]
pub fn instancia_unica(nombre: &str) -> Option<InstanciaUnica> {
    let dir = crate::config::dir_app();
    std::fs::create_dir_all(&dir).ok()?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(dir.join(format!("{nombre}{}.lock", sufijo())))
        .ok()?;
    f.try_lock().ok()?;
    Some(InstanciaUnica { _archivo: f })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_de_ejecutables() {
        assert_eq!(nombre_exe("icopia-login"), if cfg!(windows) { "icopia-login.exe" } else { "icopia-login" });
    }

    #[test]
    fn segunda_instancia_no_arranca() {
        let nombre = format!("prueba-{}", std::process::id());
        let _primera = instancia_unica(&nombre).expect("la primera arranca");
        assert!(instancia_unica(&nombre).is_none(), "mientras vive la primera, la segunda no");
    }
}
