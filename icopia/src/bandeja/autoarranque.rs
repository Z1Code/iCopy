//! "Iniciar con Windows": valor `icopia` en HKCU\...\Run.
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUTA: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NOMBRE: &str = "icopia";

pub fn activo() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUTA)
        .and_then(|k| k.get_value::<String, _>(NOMBRE))
        .is_ok()
}

pub fn fijar(activar: bool) -> std::io::Result<()> {
    let (clave, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUTA)?;
    if activar {
        let exe = std::env::current_exe()?;
        clave.set_value(NOMBRE, &format!("\"{}\"", exe.display()))
    } else {
        match clave.delete_value(NOMBRE) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}
