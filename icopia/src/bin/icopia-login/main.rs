//! Ventana para iniciar sesión en iCloud. Sale con código 0 si la sesión quedó iniciada; 1 si se
//! cierra o cancela. Windows: controles nativos; macOS: webview.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(windows)]
    windows::main();
    #[cfg(target_os = "macos")]
    macos::main();
}
