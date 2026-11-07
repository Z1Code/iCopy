//! Ventana del panel de progreso: sin bordes, esquinas redondeadas, encima de la bandeja,
//! junto al reloj (como los paneles de Wi-Fi o volumen de Windows 11).
use crate::bandeja::estado::EstadoUi;
use crate::bandeja::hover::{self, Hover};
use crate::bandeja::panel::{es_nueva_ronda, principal, vista, Principal, Velocimetro};
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
    /// El botón principal (Pausar, Reanudar, Revisar ahora o Iniciar sesión).
    Principal(Principal),
    AbrirCarpeta,
}

pub struct Panel {
    ventana: Window,
    lienzo: Option<Lienzo>,
    escala: f32,
    oscuro: bool,
    hover: Option<Boton>,
    /// Botón sobre el que se pulsó (el clic cuenta al soltar sobre el mismo).
    pulsado: Option<Boton>,
    /// Última posición del ratón (píxeles a 96 ppp): un clic siempre llega con el ratón dentro.
    cursor: (f64, f64),
    oculto_en: Option<Instant>,
    /// Abrir/cerrar al pasar el ratón por el icono.
    raton: Hover,
    /// Punto del icono de la bandeja (para colocar el panel).
    ancla: (i32, i32),
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
            pulsado: None,
            cursor: (-1.0, -1.0),
            oculto_en: None,
            raton: Hover::default(),
            ancla: (0, 0),
            velocimetro: Velocimetro::default(),
            ultima_muestra: None,
            t0: Instant::now(),
        }
    }

    pub fn id(&self) -> WindowId {
        self.ventana.id()
    }

    pub fn visible(&self) -> bool {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.ventana.hwnd() as HWND) != 0 }
    }

    fn segundos(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    /// El ratón entra en el icono de la bandeja (o se mueve por él) o sale de él.
    pub fn raton_en_icono(&mut self, encima: bool, ancla: Option<(i32, i32)>) {
        if let Some(a) = ancla {
            self.ancla = a;
        }
        let t = self.segundos();
        self.raton.icono(encima, t);
    }

    /// Avanza los temporizadores del paso del ratón; abre o cierra el panel si toca.
    pub fn tick_raton(&mut self, estado: &EstadoUi) {
        match self.raton.tick(self.segundos()) {
            Some(hover::Orden::Abrir) => self.mostrar(self.ancla, estado, false),
            Some(hover::Orden::Cerrar) => self.ocultar(),
            None => {}
        }
    }

    /// ¿Cree que el ratón está sobre el icono? (la bandeja comprueba cuándo sale)
    pub fn raton_sobre_icono(&self) -> bool {
        self.raton.sobre_icono()
    }

    /// ¿Hay que despertar pronto el bucle por el paso del ratón?
    pub fn raton_pendiente(&self) -> bool {
        self.raton.pendiente()
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

    /// Llega un estado nuevo del motor: al empezar una ronda, la velocidad se mide de cero.
    pub fn cambio_de_estado(&mut self, antes: &EstadoUi, despues: &EstadoUi) {
        if es_nueva_ronda(antes, despues) {
            self.velocimetro.reiniciar();
            self.ultima_muestra = None;
            self.muestrear();
        }
    }

    /// Clic en el icono de la bandeja: abre o cierra el panel. `ancla` es un punto del icono.
    pub fn alternar(&mut self, ancla: (i32, i32), estado: &EstadoUi) {
        self.ancla = ancla;
        if self.visible() && !self.raton.fijado() {
            // Abierto al pasar el ratón: el clic lo deja fijo (y con el foco).
            self.raton.fijar();
            self.ventana.set_focus();
        } else if self.visible() {
            self.ocultar();
        } else if self.oculto_en.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            // Ese mismo clic acaba de quitarle el foco al panel y cerrarlo: no se reabre.
            self.oculto_en = None;
        } else {
            self.raton.fijar();
            self.mostrar(ancla, estado, true);
        }
    }

    /// Muestra el panel junto a la bandeja; con ctivar, además le da el foco.
    pub fn mostrar(&mut self, ancla: (i32, i32), estado: &EstadoUi, activar: bool) {
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
        if activar {
            self.ventana.set_visible(true);
            self.ventana.set_focus();
        } else {
            // Al pasar el ratón no se roba el foco a lo que el usuario esté haciendo.
            use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
            unsafe { ShowWindow(self.ventana.hwnd() as HWND, SW_SHOWNOACTIVATE) };
        }
        self.volcar();
    }

    pub fn ocultar(&mut self) {
        if self.visible() {
            self.ventana.set_visible(false);
        }
        self.raton.cerrado();
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
            // Un clic dentro de un panel abierto al pasar el ratón le da el foco: queda fijo.
            WindowEvent::Focused(true) => self.raton.fijar(),
            WindowEvent::CursorEntered { .. } => {
                let t = self.segundos();
                self.raton.panel(true, t);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.logical_key == Key::Escape => self.ocultar(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x / self.escala as f64, position.y / self.escala as f64);
                let b = panel_gdi::boton_en(self.cursor.0, self.cursor.1);
                if b != self.hover {
                    self.hover = b;
                    self.refrescar(estado);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let t = self.segundos();
                self.raton.panel(false, t);
                if self.hover.take().is_some() {
                    self.refrescar(estado);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => self.pulsado = panel_gdi::boton_en(self.cursor.0, self.cursor.1),
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                let suelto = panel_gdi::boton_en(self.cursor.0, self.cursor.1);
                return self.pulsado.take().filter(|&b| Some(b) == suelto).map(|b| match b {
                    Boton::Pausa => Accion::Principal(principal(estado)),
                    Boton::Carpeta => Accion::AbrirCarpeta,
                });
            }
            _ => {}
        }
        None
    }
}
