//! Piezas de la app de bandeja que no dependen del bucle de eventos.
//! Los modelos (estado, panel, asistente, hover, icono, hilos) son comunes; el dibujo GDI y las
//! ventanas Win32 solo existen en Windows (en macOS la interfaz va por la barra de menús y webview).
pub mod asistente;
#[cfg(windows)]
pub mod asistente_gdi;
#[cfg(windows)]
pub mod asistente_ventana;
pub mod autoarranque;
pub mod avisos;
pub mod estado;
pub mod hilo_actualizar;
pub mod hilo_liberar;
pub mod hilo_motor;
pub mod hover;
pub mod icono;
pub mod panel;
#[cfg(windows)]
pub mod panel_gdi;
#[cfg(windows)]
pub mod panel_ventana;
