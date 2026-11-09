//! Arrancar iCopy al iniciar sesión.
//! Windows: valor `icopia` en HKCU\...\Run. macOS: LaunchAgent `com.z1code.icopy` en
//! `~/Library/LaunchAgents` (sin KeepAlive: si la persona sale de la app, no se reabre).

#[cfg(windows)]
mod imp {
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
}

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};

    pub const ETIQUETA: &str = "com.z1code.icopy";

    fn ruta() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join("Library").join("LaunchAgents").join(format!("{ETIQUETA}.plist")))
    }

    fn escapar(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    /// Contenido del LaunchAgent que abre `exe` al iniciar sesión.
    pub fn plist(exe: &Path) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{ETIQUETA}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
            escapar(&exe.display().to_string())
        )
    }

    pub fn activo() -> bool {
        ruta().is_some_and(|r| r.exists())
    }

    pub fn fijar(activar: bool) -> std::io::Result<()> {
        let r = ruta().ok_or_else(|| std::io::Error::other("sin carpeta personal"))?;
        if activar {
            if let Some(p) = r.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&r, plist(&std::env::current_exe()?))
        } else {
            match std::fs::remove_file(&r) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                otro => otro,
            }
        }
    }
}

pub use imp::{activo, fijar};

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn plist_valido_con_la_ruta() {
        let exe = std::path::Path::new("/Applications/iCopy & Co.app/Contents/MacOS/icopia-bandeja");
        let texto = super::imp::plist(exe);
        assert!(texto.contains("/Applications/iCopy &amp; Co.app/Contents/MacOS/icopia-bandeja"));
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.plist");
        std::fs::write(&f, texto).unwrap();
        let ok = std::process::Command::new("plutil").arg("-lint").arg(&f).status().unwrap();
        assert!(ok.success(), "plutil -lint");
    }
}
