//! Ventana del asistente "Liberar espacio en iCloud": normal (con barra de título), centrada,
//! tema claro u oscuro, con ratón y teclado. El trabajo lo hace el hilo del motor.
use crate::bandeja::asistente::{Accion, Asistente, Control, Paso, Revision, ALTO, ANCHO};
use crate::bandeja::asistente_gdi;
use crate::bandeja::panel_gdi::{self, Lienzo};
use crate::i18n::{t, T};
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{ElementState, MouseButton, WindowEvent};
use tao::event_loop::EventLoopWindowTarget;
use tao::keyboard::KeyCode;
use tao::platform::windows::WindowExtWindows;
use tao::window::{Window, WindowBuilder, WindowId};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi as gdi;

pub struct VentanaAsistente {
    ventana: Window,
    lienzo: Option<Lienzo>,
    escala: f32,
    oscuro: bool,
    pub modelo: Asistente,
    t0: Instant,
    cursor: (f64, f64),
    mayus: bool,
    /// Tecla (Espacio o Intro) que mantiene pulsado el botón de liberar.
    tecla_mantener: Option<KeyCode>,
}

fn hwnd(v: &Window) -> HWND {
    v.hwnd() as HWND
}

impl VentanaAsistente {
    pub fn nueva<E>(destino: &EventLoopWindowTarget<E>, modelo: Asistente) -> VentanaAsistente {
        let ventana = WindowBuilder::new()
            .with_title(t(T::AsVentana))
            .with_resizable(false)
            .with_maximizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(ANCHO as f64, ALTO as f64))
            .build(destino)
            .expect("ventana del asistente");
        VentanaAsistente { ventana, lienzo: None, escala: 1.0, oscuro: false, modelo, t0: Instant::now(), cursor: (-1.0, -1.0), mayus: false, tecla_mantener: None }
    }

    pub fn id(&self) -> WindowId {
        self.ventana.id()
    }

    pub fn visible(&self) -> bool {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd(&self.ventana)) != 0 }
    }

    fn segundos(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    /// Barra de título oscura si Windows usa el tema oscuro.
    fn aplicar_tema(&mut self) {
        self.oscuro = panel_gdi::tema_oscuro();
        unsafe {
            use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
            const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
            let si: i32 = self.oscuro as i32;
            DwmSetWindowAttribute(hwnd(&self.ventana), DWMWA_USE_IMMERSIVE_DARK_MODE, &si as *const _ as _, 4);
        }
    }

    /// Muestra la ventana centrada en el monitor donde está el ratón, con el foco.
    pub fn mostrar(&mut self) {
        self.aplicar_tema();
        self.actualizar_titulo();
        unsafe {
            use windows_sys::Win32::Foundation::{POINT, RECT};
            use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForMonitor, MDT_EFFECTIVE_DPI};
            use windows_sys::Win32::UI::WindowsAndMessaging as wm;
            let v = hwnd(&self.ventana);
            if !self.visible() {
                let mut p = POINT { x: 0, y: 0 };
                wm::GetCursorPos(&mut p);
                let monitor = gdi::MonitorFromPoint(p, gdi::MONITOR_DEFAULTTOPRIMARY);
                let mut info: gdi::MONITORINFO = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<gdi::MONITORINFO>() as u32;
                gdi::GetMonitorInfoW(monitor, &mut info);
                let (mut dx, mut dy) = (96u32, 96u32);
                GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                let escala = dx as f32 / 96.0;
                let mut r = RECT { left: 0, top: 0, right: (ANCHO as f32 * escala).round() as i32, bottom: (ALTO as f32 * escala).round() as i32 };
                let estilo = wm::GetWindowLongW(v, wm::GWL_STYLE) as u32;
                let estilo_ex = wm::GetWindowLongW(v, wm::GWL_EXSTYLE) as u32;
                AdjustWindowRectExForDpi(&mut r, estilo, 0, estilo_ex, dx);
                let (w, h) = (r.right - r.left, r.bottom - r.top);
                let tr = info.rcWork;
                let x = tr.left + (tr.right - tr.left - w) / 2;
                let y = tr.top + (tr.bottom - tr.top - h) / 2;
                wm::SetWindowPos(v, std::ptr::null_mut(), x, y, w, h, wm::SWP_NOZORDER);
            }
            wm::ShowWindow(v, wm::SW_SHOWNORMAL);
            crate::bandeja::panel_ventana::traer_al_frente(v);
        }
        self.refrescar();
    }

    pub fn ocultar(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        unsafe { ShowWindow(hwnd(&self.ventana), SW_HIDE) };
        self.modelo.soltar_mantener();
        self.tecla_mantener = None;
    }

    /// El título dice el paso (lo leen los lectores de pantalla al cambiar de paso).
    fn actualizar_titulo(&self) {
        self.ventana.set_title(&format!("{} — {}", t(T::AsVentana), self.modelo.paso.nombre()));
    }

    /// Cada cuánto repintar: rápido mientras algo se mueve.
    pub fn intervalo(&self) -> Duration {
        let animando = self.modelo.mantener.pulsado()
            || self.modelo.paso == Paso::Revisar && self.modelo.revision == Revision::EnCurso
            || self.modelo.paso == Paso::EnMarcha;
        if animando {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(500)
        }
    }

    /// Avanza el tiempo (el botón de mantener). Devuelve una acción al completarse.
    pub fn tick(&mut self) -> Option<Accion> {
        let paso = self.modelo.paso;
        let r = self.modelo.tick(self.segundos());
        if self.modelo.paso != paso {
            self.actualizar_titulo();
        }
        r
    }

    pub fn refrescar(&mut self) {
        if !self.visible() {
            return;
        }
        let v = hwnd(&self.ventana);
        let (w, h) = unsafe {
            let mut r = std::mem::zeroed();
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(v, &mut r);
            self.escala = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(v).max(96) as f32 / 96.0;
            (r.right, r.bottom)
        };
        if w <= 0 || h <= 0 {
            return;
        }
        if self.lienzo.as_ref().is_none_or(|l| l.ancho != w || l.alto != h) {
            self.lienzo = Some(Lienzo::nuevo(w, h));
        }
        let l = self.lienzo.as_mut().expect("lienzo");
        asistente_gdi::pintar(l, &self.modelo, self.oscuro, self.escala, self.t0.elapsed().as_secs_f64(), chrono::Utc::now().timestamp());
        unsafe {
            let dc = gdi::GetDC(v);
            gdi::BitBlt(dc, 0, 0, l.ancho, l.alto, l.dc(), 0, 0, gdi::SRCCOPY);
            gdi::ReleaseDC(v, dc);
        }
    }

    fn activar(&mut self, c: Control) -> Option<Accion> {
        let paso = self.modelo.paso;
        let r = self.modelo.activar(c);
        if self.modelo.paso != paso {
            self.actualizar_titulo();
        }
        r
    }

    /// Atiende un evento de su ventana. Devuelve lo que hay que pedir a la bandeja.
    pub fn evento(&mut self, e: &WindowEvent) -> Option<Accion> {
        let accion = match e {
            WindowEvent::CloseRequested => Some(Accion::Cerrar),
            WindowEvent::Focused(false) => {
                self.modelo.soltar_mantener();
                self.tecla_mantener = None;
                None
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mayus = m.shift_key();
                None
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x / self.escala as f64, position.y / self.escala as f64);
                let h = self.modelo.control_en(self.cursor.0, self.cursor.1).filter(|&c| self.modelo.habilitado(c));
                if h != self.modelo.hover {
                    self.modelo.hover = h;
                }
                None
            }
            WindowEvent::CursorLeft { .. } => {
                self.modelo.hover = None;
                if self.tecla_mantener.is_none() {
                    self.modelo.soltar_mantener();
                }
                None
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let c = self.modelo.control_en(self.cursor.0, self.cursor.1);
                self.modelo.pulsado = c;
                if c == Some(Control::Mantener) {
                    self.modelo.foco = c;
                    let t = self.segundos();
                    self.modelo.pulsar_mantener(t);
                }
                None
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                let suelto = self.modelo.control_en(self.cursor.0, self.cursor.1);
                match self.modelo.pulsado.take() {
                    Some(Control::Mantener) => {
                        self.modelo.soltar_mantener();
                        None
                    }
                    Some(c) if Some(c) == suelto => self.activar(c),
                    _ => None,
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let tecla = event.physical_key;
                match (event.state, tecla) {
                    (ElementState::Released, k) if Some(k) == self.tecla_mantener => {
                        self.tecla_mantener = None;
                        self.modelo.soltar_mantener();
                        None
                    }
                    (ElementState::Released, _) => None,
                    (_, KeyCode::Tab) => {
                        self.modelo.mover_foco(self.mayus);
                        None
                    }
                    (_, KeyCode::ArrowLeft) => {
                        self.modelo.flecha(-1, 0);
                        None
                    }
                    (_, KeyCode::ArrowRight) => {
                        self.modelo.flecha(1, 0);
                        None
                    }
                    (_, KeyCode::ArrowUp) => {
                        self.modelo.flecha(0, -1);
                        None
                    }
                    (_, KeyCode::ArrowDown) => {
                        self.modelo.flecha(0, 1);
                        None
                    }
                    (_, KeyCode::Escape) if !event.repeat => Some(Accion::Cerrar),
                    (_, KeyCode::Space | KeyCode::Enter | KeyCode::NumpadEnter) if self.modelo.foco == Some(Control::Mantener) => {
                        if !event.repeat {
                            self.tecla_mantener = Some(tecla);
                            let t = self.segundos();
                            self.modelo.pulsar_mantener(t);
                        }
                        None
                    }
                    (_, KeyCode::Space | KeyCode::Enter | KeyCode::NumpadEnter) if !event.repeat => {
                        // Intro sin foco: el botón principal del pie.
                        let c = self.modelo.foco.or((tecla != KeyCode::Space).then_some(Control::Primario));
                        c.and_then(|c| self.activar(c))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        self.refrescar();
        accion
    }
}
