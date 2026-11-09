//! App de iCopy que vive en la bandeja (Windows) o en la barra de menús (macOS).
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
