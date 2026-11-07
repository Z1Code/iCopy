//! Notificaciones de Windows (nunca fallan).
use std::path::PathBuf;
use tauri_winrt_notification::{IconCrop, Toast};

/// Icono de icopia para las notificaciones (las imágenes de un toast tienen que ser archivos).
fn icono() -> Option<PathBuf> {
    const PNG: &[u8] = include_bytes!("../../assets/icopia-aviso.png");
    let ruta = std::env::temp_dir().join("icopia-aviso.png");
    let al_dia = std::fs::metadata(&ruta).map(|m| m.len() == PNG.len() as u64).unwrap_or(false);
    if al_dia || std::fs::write(&ruta, PNG).is_ok() {
        Some(ruta)
    } else {
        None
    }
}

pub fn mostrar(titulo: &str, texto: &str) {
    let mut toast = Toast::new(Toast::POWERSHELL_APP_ID).title(titulo).text1(texto);
    if let Some(ruta) = icono() {
        toast = toast.icon(&ruta, IconCrop::Square, "icopia");
    }
    let _ = toast.show();
}
