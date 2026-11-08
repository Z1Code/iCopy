//! Ventana del panel de progreso: sin bordes, esquinas redondeadas, encima de la bandeja,
//! junto al reloj (como los paneles de Wi-Fi o volumen de Windows 11).
use crate::bandeja::estado::EstadoUi;
use crate::bandeja::hover::{self, Hover, Rect, VigiaClics};
use crate::bandeja::panel::{es_nueva_ronda, principal, vista, vista_liberar, Medidas, MediaRonda, Principal, Velocimetro, Vista};
use crate::bandeja::panel_gdi::{self, Boton, Lienzo};
use crate::descarga::BYTES_RECIBIDOS;
use crate::progreso_liberar::ProgresoLiberar;
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

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(tecla: i32) -> i16;
}

/// ¿Hay algún botón del ratón pulsado ahora mismo (en cualquier ventana)?
pub fn boton_raton_pulsado() -> bool {
    // VK_LBUTTON, VK_RBUTTON, VK_MBUTTON
    [0x01, 0x02, 0x04].into_iter().any(|vk| unsafe { GetAsyncKeyState(vk) } < 0)
}

/// Último clic (botón abajo) visto por el gancho de ratón mientras el panel está abierto.
/// Mirar el estado de los botones cada 50 ms no basta: un toque corto (p. ej. en un panel
/// táctil) puede caer entre dos lecturas.
static CLIC_GANCHO: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

unsafe extern "system" fn gancho_raton(codigo: i32, w: usize, l: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };
    if codigo >= 0 && matches!(w as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN) {
        let m = &*(l as *const MSLLHOOKSTRUCT);
        if let Ok(mut c) = CLIC_GANCHO.lock() {
            *c = Some((m.pt.x, m.pt.y));
        }
    }
    CallNextHookEx(std::ptr::null_mut(), codigo, w, l)
}

/// Pone hwnd en primer plano aunque Windows lo restrinja a otro proceso: se engancha un
/// momento a la cola de entrada del hilo que tiene el primer plano. Así un menú o panel recibe
/// la desactivación (y se cierra) al hacer clic fuera.
// `hwnd` es un identificador de ventana que Windows valida: uno inválido solo hace fallar la llamada.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn traer_al_frente(hwnd: HWND) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow};
    unsafe {
        let mio = GetCurrentThreadId();
        let otro = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let enganchado = otro != 0 && otro != mio && AttachThreadInput(mio, otro, 1) != 0;
        SetForegroundWindow(hwnd);
        if enganchado {
            AttachThreadInput(mio, otro, 0);
        }
    }
}

/// Lo que el usuario pidió desde el panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accion {
    /// El botón principal (Pausar, Reanudar, Revisar ahora o Iniciar sesión).
    Principal(Principal),
    AbrirCarpeta,
    /// Abrir el asistente "Liberar espacio en iCloud".
    Liberar,
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
    /// Clics fuera del panel (lo cierran aunque nunca haya tenido el foco).
    vigia: VigiaClics,
    /// Punto del icono de la bandeja (para colocar el panel).
    ancla: (i32, i32),
    velocimetro: Velocimetro,
    /// Ritmo del listado (elementos vistos por segundo).
    ritmo_listado: Velocimetro,
    media_ronda: MediaRonda,
    ultima_muestra: Option<Instant>,
    t0: Instant,
    /// Gancho de ratón de bajo nivel (solo mientras el panel está abierto).
    gancho: Option<isize>,
    /// Último avance de "liberar espacio" leído del registro.
    liberar: Option<ProgresoLiberar>,
    /// Alto actual del panel a 96 ppp (crece con la sección de liberar) y botones del pie.
    pie: panel_gdi::Pie,
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
            vigia: VigiaClics::default(),
            ancla: (0, 0),
            velocimetro: Velocimetro::default(),
            ritmo_listado: Velocimetro::default(),
            media_ronda: MediaRonda::default(),
            ultima_muestra: None,
            t0: Instant::now(),
            gancho: None,
            liberar: None,
            pie: panel_gdi::Pie::default(),
        }
    }

    /// Nuevo avance de "liberar espacio" (o `None`): se verá en el próximo repintado.
    pub fn poner_liberar(&mut self, p: Option<ProgresoLiberar>) {
        self.liberar = p;
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

    /// El icono de la bandeja avisa de que el ratón entró o se movió sobre él.
    pub fn aviso_icono(&mut self, ancla: (i32, i32)) {
        self.ancla = ancla;
        let t = self.segundos();
        self.raton.aviso_icono(t);
    }

    /// El icono avisa de que el ratón salió (no siempre llega).
    pub fn salio_icono(&mut self) {
        self.raton.salio_icono();
    }

    /// Vigila el ratón (cada ~50 ms): abre o cierra el panel por el paso del ratón y lo cierra
    /// con un clic fuera. `icono` es el rectángulo que Windows da para el icono de la bandeja.
    pub fn vigilar_raton(&mut self, estado: &EstadoUi, icono: Option<Rect>) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) };
        let cursor = (p.x, p.y);
        // Un rectángulo vacío, enorme o fuera de la pantalla (icono en el desplegable de iconos
        // ocultos, ya cerrado) no vale: ante la duda se trata como si el ratón no estuviera ahí.
        let icono = icono.filter(|r| {
            let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
            let pantalla = unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
                let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
                Rect { x0: x, y0: y, x1: x + GetSystemMetrics(SM_CXVIRTUALSCREEN), y1: y + GetSystemMetrics(SM_CYVIRTUALSCREEN) }
            };
            w > 0 && h > 0 && w <= 200 && h <= 200 && pantalla.contiene((r.x0, r.y0)) && pantalla.contiene((r.x1 - 1, r.y1 - 1))
        });
        let visible = self.visible();
        let panel = visible.then(|| {
            let mut r = unsafe { std::mem::zeroed() };
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(self.ventana.hwnd() as HWND, &mut r) };
            Rect { x0: r.left, y0: r.top, x1: r.right, y1: r.bottom }
        });
        let estado_cursor = hover::Cursor {
            en_panel: panel.is_some_and(|r| r.contiene(cursor)),
            en_icono: icono.is_some_and(|r| r.contiene(cursor)),
        };
        let pulsado = boton_raton_pulsado();
        let clic = CLIC_GANCHO.lock().ok().and_then(|mut c| c.take());
        let fuera = panel.is_some_and(|r| {
            let por_gancho = clic.is_some_and(|p| hover::clic_fuera(p, r, icono));
            let por_estado = self.vigia.actualizar(pulsado, cursor, r, icono);
            por_gancho || por_estado
        });
        let orden = self.raton.tick(self.segundos(), estado_cursor);
        if fuera {
            self.ocultar();
            return;
        }
        match orden {
            Some(hover::Orden::Abrir) => self.mostrar(self.ancla, estado, false),
            Some(hover::Orden::Cerrar) => self.ocultar(),
            None => {}
        }
    }

    /// Con el panel abierto: gancho de ratón (clics fuera) y un temporizador propio de 50 ms en
    /// su ventana, que despierta el bucle pase lo que pase para vigilar el ratón.
    fn vigilancia(&mut self, activa: bool) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL};
        const TEMPORIZADOR: usize = 0x1C0;
        let hwnd = self.ventana.hwnd() as HWND;
        unsafe {
            if activa {
                if let Ok(mut c) = CLIC_GANCHO.lock() {
                    *c = None;
                }
                if self.gancho.is_none() {
                    let h = SetWindowsHookExW(WH_MOUSE_LL, Some(gancho_raton), windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()), 0);
                    self.gancho = (!h.is_null()).then_some(h as isize);
                }
                SetTimer(hwnd, TEMPORIZADOR, 50, None);
            } else {
                if let Some(h) = self.gancho.take() {
                    UnhookWindowsHookEx(h as _);
                }
                KillTimer(hwnd, TEMPORIZADOR);
            }
        }
    }

    /// ¿Hay que vigilar el ratón a menudo?
    pub fn raton_pendiente(&self) -> bool {
        self.raton.pendiente() || self.visible()
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
        let t = self.segundos();
        let bytes = BYTES_RECIBIDOS.load(Ordering::Relaxed);
        if es_nueva_ronda(antes, despues) {
            self.velocimetro.reiniciar();
            self.ritmo_listado.reiniciar();
            self.media_ronda.empezar(t, bytes);
            self.ultima_muestra = None;
            self.muestrear();
        } else if antes.trabajando() && !despues.trabajando() {
            self.media_ronda.terminar(t, bytes);
        }
        if let crate::bandeja::estado::Fase::Listando { vistos } = despues.fase {
            self.ritmo_listado.registrar(t, vistos as u64);
        }
    }

    /// Clic en el icono de la bandeja: abre o cierra el panel. `ancla` es un punto del icono.
    pub fn alternar(&mut self, ancla: (i32, i32), estado: &EstadoUi) {
        self.ancla = ancla;
        if self.visible() && !self.raton.fijado() {
            // Abierto al pasar el ratón: el clic lo deja fijo (y con el foco).
            self.raton.fijar();
            traer_al_frente(self.ventana.hwnd() as HWND);
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

    /// Coloca el panel junto a la bandeja con el alto actual (crece hacia arriba si la barra está abajo).
    fn colocar(&mut self) {
        let ancla = self.ancla;
        unsafe {
            use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
            let monitor = gdi::MonitorFromPoint(POINT { x: ancla.0, y: ancla.1 }, gdi::MONITOR_DEFAULTTONEAREST);
            let mut info: gdi::MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<gdi::MONITORINFO>() as u32;
            gdi::GetMonitorInfoW(monitor, &mut info);
            let (mut dx, mut dy) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let escala = dx as f32 / 96.0;
            let (w, h) = ((panel_gdi::ANCHO as f32 * escala).round() as i32, (self.pie.alto as f32 * escala).round() as i32);
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
    }

    /// Muestra el panel junto a la bandeja; con `activar`, además le da el foco.
    pub fn mostrar(&mut self, ancla: (i32, i32), estado: &EstadoUi, activar: bool) {
        self.ancla = ancla;
        self.pie = panel_gdi::pie(&self.vista_actual(estado));
        self.colocar();
        self.oscuro = panel_gdi::tema_oscuro();
        self.hover = None;
        self.pintar(estado);
        self.vigia.reiniciar(boton_raton_pulsado());
        self.vigilancia(true);
        // Mostrar y ocultar siempre con Win32 directamente (nunca con tao): tao lleva su propia
        // cuenta de si la ventana es visible y, si se mezclan, `set_visible(false)` no hace nada
        // y el panel se queda abierto.
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOW, SW_SHOWNOACTIVATE};
        let hwnd = self.ventana.hwnd() as HWND;
        if activar {
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            traer_al_frente(hwnd);
        } else {
            // Al pasar el ratón no se roba el foco a lo que el usuario esté haciendo.
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        }
        self.volcar();
    }

    pub fn ocultar(&mut self) {
        if self.visible() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
            unsafe { ShowWindow(self.ventana.hwnd() as HWND, SW_HIDE) };
        }
        self.vigilancia(false);
        self.raton.cerrado();
        self.oculto_en = Some(Instant::now());
    }

    /// Cada cuánto hay que repintar mientras está abierto (el punto "En vivo" late; la barra
    /// indeterminada de liberar se mueve).
    pub fn intervalo(&self, estado: &EstadoUi) -> Duration {
        let anima_liberar = self.liberar.as_ref().and_then(|p| vista_liberar(p, chrono::Utc::now().timestamp())).is_some_and(|v| v.fraccion.is_none());
        if crate::bandeja::panel::indicador(estado).late() || anima_liberar {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        }
    }

    /// Repinta el contenido y lo lleva a la pantalla.
    pub fn refrescar(&mut self, estado: &EstadoUi) {
        if self.visible() {
            let pie = panel_gdi::pie(&self.vista_actual(estado));
            if pie != self.pie {
                if pie.alto != self.pie.alto {
                    self.pie = pie;
                    self.colocar();
                }
                self.pie = pie;
                self.hover = None;
            }
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
        let v = self.vista_actual(estado);
        let l = self.lienzo.as_mut().expect("lienzo");
        panel_gdi::pintar(l, &v, self.oscuro, self.escala, self.hover, pulso);
    }

    fn vista_actual(&self, estado: &EstadoUi) -> Vista {
        let medidas = Medidas {
            bytes_por_seg: self.velocimetro.bytes_por_seg(),
            elementos_por_seg: self.ritmo_listado.bytes_por_seg(),
            media_ultima_ronda: self.media_ronda.media(),
        };
        let ahora = chrono::Utc::now().timestamp();
        let mut v = vista(estado, medidas, ahora);
        v.liberar = self.liberar.as_ref().and_then(|p| vista_liberar(p, ahora));
        // Mientras se libera espacio, el botón sobra (ya hay uno en marcha).
        v.boton_liberar &= v.liberar.as_ref().is_none_or(|l| l.tono != crate::bandeja::panel::TonoLiberar::Activo);
        v
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

            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.logical_key == Key::Escape => self.ocultar(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x / self.escala as f64, position.y / self.escala as f64);
                let b = panel_gdi::boton_en(self.cursor.0, self.cursor.1, self.pie);
                if b != self.hover {
                    self.hover = b;
                    self.refrescar(estado);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.hover.take().is_some() {
                    self.refrescar(estado);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => self.pulsado = panel_gdi::boton_en(self.cursor.0, self.cursor.1, self.pie),
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                let suelto = panel_gdi::boton_en(self.cursor.0, self.cursor.1, self.pie);
                return self.pulsado.take().filter(|&b| Some(b) == suelto).map(|b| match b {
                    Boton::Pausa => Accion::Principal(principal(estado)),
                    Boton::Carpeta => Accion::AbrirCarpeta,
                    Boton::Liberar => Accion::Liberar,
                });
            }
            _ => {}
        }
        None
    }
}
