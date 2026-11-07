//! Notificaciones de Windows (nunca fallan).
use tauri_winrt_notification::Toast;

pub fn mostrar(titulo: &str, texto: &str) {
    let _ = Toast::new(Toast::POWERSHELL_APP_ID).title(titulo).text1(texto).show();
}
