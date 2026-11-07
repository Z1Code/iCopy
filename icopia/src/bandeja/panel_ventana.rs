//! Ventana del panel de progreso: sin bordes, esquinas redondeadas, encima de la bandeja,
//! junto al reloj (como los paneles de Wi-Fi o volumen de Windows 11).
use crate::bandeja::estado::EstadoUi;
use crate::bandeja::panel::{vista, Velocimetro};
use crate::bandeja::panel_gdi::{self, Boton, Lienzo};
use crate::descarga::BYTES_RECIBIDOS;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tao::dpi::PhysicalSize;
use tao::event::{ElementState, MouseButton, WindowEvent};
use tao::event_loop::EventLoopWindowTarget;
use tao::keyboard::Key;
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder, WindowId};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi as gdi;

/// Lo que el usuario pidió desde el panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accion {
    Pausa,
    AbrirCarpeta,
}

pub struct Panel {
    ventana: Window,
    lienzo: Option<Lienzo>,
    escala: f32,
    oscuro: bool,
    hover: Option<Boton>,
    oculto_en: Option<Instant>,
    velocimetro: Velocimetro,
    ultima_muestra: Option<Instant>,
    t0: Instant,
}

impl Panel {
    pub fn nuevo<T>(destino: &EventLoopWindowTarget<T>) -> Panel {
        let ventana = WindowBuilder::new()
            .with_title("icopia")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_always_on_top(true)
            .with_skip_taskbar(true)
            .with_undecorated_shadow(true)
            .with_inner_size(PhysicalSize::new(panel_gdi::ANCHO as u32, panel_gdi::ALTO as u32))
            .build(destino)
            .expect("ventana del panel");
        unsafe {
            use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
            let redondo = DWMWCP_ROUND;
            DwmSetWindowAttribute(ventana.hwnd() as HWND, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &redondo as *const _ as _, 4);
        }
        Panel {
            ventana,
            lienzo: None,
            escala: 1.0,
            oscuro: false,
            hover: None,
            oculto_en: None,
            velocimetro: Velocimetro::default(),
            ultima_muestra: None,
            t0: Instant::now(),
        }
    }

    pub fn id(&self) -> WindowId {
        self.ventana.id()
    }

    pub fn visible(&self) -> bool {
        self.ventana.is_visible()
    }

    /// Muestrea el contador de bytes descargados (para la velocidad), como mucho dos veces por segundo.
    pub fn muestrear(&mut self) {
        let ahora = Instant::now();
        if self.ultima_muestra.is_some_and(|t| ahora.duration_since(t) < Duration::from_millis(500)) {
            return;
        }
        self.ultima_muestra = Some(ahora);
        self.velocimetro.registrar(ahora.duration_since(self.t0).as_secs_f64(), BYTES_RECIBIDOS.load(Ordering::Relaxed));
    }

    /// Clic en el icono de la bandeja: abre o cierra el panel. `ancla` es un punto del icono.
    pub fn alternar(&mut self, ancla: (i32, i32), estado: &EstadoUi) {
        if self.visible() {
            self.ocultar();
        } else if self.oculto_en.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            // Ese mismo clic acaba de quitarle el foco al panel y cerrarlo: no se reabre.
            self.oculto_en = None;
        } else {
            self.mostrar(ancla, estado);
        }
    }

    pub fn mostrar(&mut self, ancla: (i32, i32), estado: &EstadoUi) {
        unsafe {
            use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
            let monitor = gdi::MonitorFromPoint(POINT { x: ancla.0, y: ancla.1 }, gdi::MONITOR_DEFAULTTONEAREST);
            let mut info: gdi::MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<gdi::MONITORINFO>() as u32;
            gdi::GetMonitorInfoW(monitor, &mut info);
            let (mut dx, mut dy) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let escala = dx as f32 / 96.0;
            let (w, h) = ((panel_gdi::ANCHO as f32 * escala).round() as i32, (panel_gdi::ALTO as f32 * escala).round() as i32);
            let margen = (12.0 * escala).round() as i32;
            let (m, t) = (info.rcMonitor, info.rcWork);
            // La barra de tareas puede estar abajo (lo normal), arriba o a un lado.
            let x = if t.left > m.left { t.left + margen } else { t.right - w - margen };
            let y = if t.top > m.top { t.top + margen } else { t.bottom - h - margen };
            // Directo con Win32: tao no aplica bien el tamaño a una ventana oculta sin bordes.
            use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE};
            SetWindowPos(self.ventana.hwnd() as HWND, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
            self.escala = escala;
        }
        self.oscuro = panel_gdi::tema_oscuro();
        self.hover = None;
        self.pintar(estado);
        self.ventana.set_visible(true);
        self.ventana.set_focus();
        self.volcar();
    }

    pub fn ocultar(&mut self) {
        if self.visible() {
            self.ventana.set_visible(false);
        }
        self.oculto_en = Some(Instant::now());
    }

    /// Cada cuánto hay que repintar mientras está abierto (el punto "En vivo" late).
    pub fn intervalo(&self, estado: &EstadoUi) -> Duration {
        if crate::bandeja::panel::indicador(estado).late() {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        }
    }

    /// Repinta el contenido y lo lleva a la pantalla.
    pub fn refrescar(&mut self, estado: &EstadoUi) {
        if self.visible() {
            self.pintar(estado);
            self.volcar();
        }
    }

    fn pintar(&mut self, estado: &EstadoUi) {
        let (w, h) = unsafe {
            let mut r = std::mem::zeroed();
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(self.ventana.hwnd() as HWND, &mut r);
            self.escala = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(self.ventana.hwnd() as HWND).max(96) as f32 / 96.0;
            (r.right, r.bottom)
        };
        if self.lienzo.as_ref().is_none_or(|l| l.ancho != w || l.alto != h) {
            self.lienzo = Some(Lienzo::nuevo(w, h));
        }
        let pulso = (self.t0.elapsed().as_secs_f32() / 1.6).fract();
        let v = vista(estado, self.velocimetro.bytes_por_seg(), chrono::Utc::now().timestamp());
        let l = self.lienzo.as_mut().expect("lienzo");
        panel_gdi::pintar(l, &v, self.oscuro, self.escala, self.hover, pulso);
    }

    fn volcar(&self) {
        let Some(l) = &self.lienzo else { return };
        unsafe {
            let hwnd = self.ventana.hwnd() as HWND;
            let dc = gdi::GetDC(hwnd);
            gdi::BitBlt(dc, 0, 0, l.ancho, l.alto, l.dc(), 0, 0, gdi::SRCCOPY);
            gdi::ReleaseDC(hwnd, dc);
        }
    }

    /// Atiende un evento de su ventana. Devuelve la acción pedida, si la hay.
    pub fn evento(&mut self, e: &WindowEvent, estado: &EstadoUi) -> Option<Accion> {
        match e {
            WindowEvent::Focused(false) => self.ocultar(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.logical_key == Key::Escape => self.ocultar(),
            WindowEvent::CursorMoved { position, .. } => {
                let b = panel_gdi::boton_en(position.x / self.escala as f64, position.y / self.escala as f64);
                if b != self.hover {
                    self.hover = b;
                    self.refrescar(estado);
                }
            }
            WindowEvent::CursorLeft { .. } if self.hover.is_some() => {
                self.hover = None;
                self.refrescar(estado);
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                return self.hover.map(|b| match b {
                    Boton::Pausa => Accion::Pausa,
                    Boton::Carpeta => Accion::AbrirCarpeta,
                });
            }
            _ => {}
        }
        None
    }
}
