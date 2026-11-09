//! Notificaciones del sistema (nunca fallan).
//! Windows: toast de WinRT con el icono de iCopy. macOS: Centro de notificaciones, a nombre del
//! bundle `com.z1code.icopy` (lleva el icono de la app; sin bundle, p. ej. en desarrollo, puede no verse).

#[cfg(windows)]
pub fn mostrar(titulo: &str, texto: &str) {
    use tauri_winrt_notification::{IconCrop, Toast};
    let mut toast = Toast::new(Toast::POWERSHELL_APP_ID).title(titulo).text1(texto);
    if let Some(ruta) = icono() {
        toast = toast.icon(&ruta, IconCrop::Square, "icopia");
    }
    let _ = toast.show();
}

/// Icono de icopia para las notificaciones (las imágenes de un toast tienen que ser archivos).
#[cfg(windows)]
fn icono() -> Option<std::path::PathBuf> {
    const PNG: &[u8] = include_bytes!("../../assets/icopia-aviso.png");
    let ruta = std::env::temp_dir().join("icopia-aviso.png");
    let al_dia = std::fs::metadata(&ruta).map(|m| m.len() == PNG.len() as u64).unwrap_or(false);
    if al_dia || std::fs::write(&ruta, PNG).is_ok() {
        Some(ruta)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
pub fn mostrar(titulo: &str, texto: &str) {
    static APP: std::sync::Once = std::sync::Once::new();
    APP.call_once(|| {
        let _ = notify_rust::set_application("com.z1code.icopy");
    });
    let _ = notify_rust::Notification::new().summary(titulo).body(texto).show();
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn mostrar(titulo: &str, texto: &str) {
    eprintln!("{titulo}: {texto}");
}
