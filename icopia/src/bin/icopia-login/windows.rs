//! Ventana pequeña para iniciar sesión en iCloud (lo mismo que `icopia login`, sin consola).
//! Sale con código 0 si la sesión quedó iniciada; 1 si se cierra o cancela.
use icopia::config::{self, Config};
use icopia::i18n::{t, tf, T};
use icopia::login::{comprobar_sesion, normalizar_codigo, EstadoSesion, FlujoLogin, Paso};
use icopia::puente::Puente;
use native_windows_gui as nwg;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi as gdi;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

enum Orden {
    /// Comprueba la sesión guardada de este Apple ID.
    Comprobar(String),
    Entrar { apple_id: String, password: String },
    Codigo(String),
}

enum Respuesta {
    Sesion(EstadoSesion),
    Paso(Paso),
    Cerrar,
}

/// Hace las llamadas (bloqueantes) al puente fuera del hilo de la ventana.
fn trabajador(rx: Receiver<Orden>, tx: Sender<Respuesta>, aviso: nwg::NoticeSender) {
    let mut flujo: Option<FlujoLogin<Puente>> = None;
    let mut apple_id = String::new();
    for orden in rx {
        let mut paso = match orden {
            Orden::Comprobar(id) => {
                let r = match Puente::iniciar(&id, &config::dir_sesion(), &config::log_puente()) {
                    Ok(mut p) => comprobar_sesion(&mut p),
                    Err(e) => EstadoSesion::Desconocida(format!("{e:#}")),
                };
                let _ = tx.send(Respuesta::Sesion(r));
                aviso.notice();
                continue;
            }
            Orden::Entrar { apple_id: id, password } => {
                flujo = None; // cierra el puente anterior: el Apple ID pudo cambiar
                apple_id = id;
                match Puente::iniciar(&apple_id, &config::dir_sesion(), &config::log_puente()) {
                    Ok(p) => {
                        let mut f = FlujoLogin::new(p);
                        let paso = f.entrar(&password);
                        flujo = Some(f);
                        paso
                    }
                    Err(e) => Paso::Reintentar(format!("{e:#}")),
                }
            }
            Orden::Codigo(c) => match flujo.as_mut() {
                Some(f) => f.verificar(&c),
                None => Paso::Fallo(t(T::LogEmpezar).into()),
            },
        };
        if paso == Paso::Hecho {
            let guardado = Config::cargar().and_then(|mut cfg| {
                cfg.apple_id = Some(apple_id.clone());
                cfg.guardar()
            });
            if let Err(e) = guardado {
                paso = Paso::Fallo(tf(T::LogNoGuardada, &[("e", &format!("{e:#}"))]));
            }
        }
        if matches!(paso, Paso::Hecho | Paso::Fallo(_)) {
            flujo = None;
        }
        let hecho = paso == Paso::Hecho;
        let _ = tx.send(Respuesta::Paso(paso));
        aviso.notice();
        if hecho {
            std::thread::sleep(Duration::from_millis(1500));
            let _ = tx.send(Respuesta::Cerrar);
            aviso.notice();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Etapa {
    /// Comprobando si la sesión guardada sigue valiendo.
    Comprobando,
    /// La sesión guardada vale: no hace falta la contraseña.
    YaIniciada,
    #[default]
    Credenciales,
    Codigo,
}

impl Etapa {
    /// Las etapas sin campos usan una ventana más baja.
    fn compacta(self) -> bool {
        matches!(self, Etapa::Comprobando | Etapa::YaIniciada)
    }
}

/// Color de la línea de estado.
#[derive(Clone, Copy, Default)]
enum Tono {
    #[default]
    Info,
    Error,
    Exito,
}


// Medidas en píxeles a 96 ppp (rejilla de 8 px); se escalan con el DPI del monitor.
const ANCHO: i32 = 400;
const MARGEN: i32 = 24;
const ICONO: i32 = 40;
const BARRA: i32 = 288; // empieza la barra inferior de botones
const BARRA_COMPACTA: i32 = 128;
const ALTO_BARRA: i32 = 64;
const BOTON: (i32, i32) = (112, 32);

const BLANCO: u32 = rgb(255, 255, 255);
const GRIS_BARRA: u32 = rgb(243, 243, 243);
const LINEA: u32 = rgb(229, 229, 229);
const TEXTO: u32 = rgb(26, 26, 26);
const TEXTO_SUAVE: u32 = rgb(96, 96, 96);
const ROJO: u32 = rgb(196, 43, 28);
const VERDE: u32 = rgb(15, 123, 15);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

fn hwnd(h: &nwg::ControlHandle) -> HWND {
    h.hwnd().expect("control con ventana") as HWND
}

/// Texto estático de Win32. A diferencia de `nwg::Label`, parte en varias líneas el texto
/// que no cabe y deja que la ventana elija sus colores (`WM_CTLCOLORSTATIC`).
struct Texto(HWND);

impl Default for Texto {
    fn default() -> Self {
        Texto(std::ptr::null_mut())
    }
}

impl Texto {
    fn nuevo(padre: HWND, texto: &str) -> Texto {
        let clase: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let texto: Vec<u16> = texto.encode_utf16().chain([0]).collect();
        const SS_NOPREFIX: u32 = 0x80;
        let h = unsafe {
            wm::CreateWindowExW(
                0,
                clase.as_ptr(),
                texto.as_ptr(),
                wm::WS_CHILD | wm::WS_VISIBLE | SS_NOPREFIX,
                0,
                0,
                0,
                0,
                padre,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        Texto(h)
    }

    fn set_text(&self, texto: &str) {
        let texto: Vec<u16> = texto.encode_utf16().chain([0]).collect();
        unsafe { wm::SetWindowTextW(self.0, texto.as_ptr()) };
    }

    fn set_visible(&self, si: bool) {
        unsafe { wm::ShowWindow(self.0, if si { wm::SW_SHOW } else { wm::SW_HIDE }) };
    }

    fn set_font(&self, fuente: &nwg::Font) {
        unsafe { wm::SendMessageW(self.0, wm::WM_SETFONT, fuente.handle as usize, 1) };
    }
}

#[derive(Default)]
struct Ui {
    ventana: nwg::Window,
    titulo: Texto,
    subtitulo: Texto,
    l_id: Texto,
    t_id: nwg::TextInput,
    l_pw: Texto,
    t_pw: nwg::TextInput,
    l_codigo: Texto,
    t_codigo: nwg::TextInput,
    estado: Texto,
    b_cancelar: nwg::Button,
    b_entrar: nwg::Button,
    b_verificar: nwg::Button,
    b_cambiar: nwg::Button,
    b_cerrar: nwg::Button,
    aviso: nwg::Notice,
    // Estado visual
    etapa: Cell<Etapa>,
    dpi: Cell<u32>,
    tono: Cell<Tono>,
    fuentes: RefCell<Option<(nwg::Font, nwg::Font)>>,
    icono: Cell<isize>, // HICON de la cabecera
}

impl Ui {
    fn mostrar_etapa(&self, e: Etapa) {
        let cambia_alto = e.compacta() != self.etapa.get().compacta();
        let (cred, cod) = (e == Etapa::Credenciales, e == Etapa::Codigo);
        let (titulo, subtitulo) = match e {
            Etapa::Comprobando => (t(T::LogComprobando).to_string(), tf(T::LogComprobandoSub, &[("id", &self.t_id.text().trim())])),
            Etapa::YaIniciada => (t(T::LogYa).to_string(), tf(T::LogYaSub, &[("id", &self.t_id.text().trim())])),
            Etapa::Credenciales => (t(T::LogTitulo).to_string(), t(T::LogSubCred).to_string()),
            Etapa::Codigo => (t(T::LogTitulo).to_string(), t(T::LogSubCodigo).to_string()),
        };
        self.titulo.set_text(&titulo);
        self.subtitulo.set_text(&subtitulo);
        for c in [&self.l_id, &self.l_pw] {
            c.set_visible(cred);
        }
        for c in [&self.t_id, &self.t_pw] {
            c.set_visible(cred);
        }
        self.b_entrar.set_visible(cred);
        self.l_codigo.set_visible(cod);
        self.t_codigo.set_visible(cod);
        self.b_verificar.set_visible(cod);
        self.b_cancelar.set_visible(e != Etapa::YaIniciada);
        self.b_cambiar.set_visible(e == Etapa::YaIniciada);
        self.b_cerrar.set_visible(e == Etapa::YaIniciada);
        self.estado.set_visible(!e.compacta());
        self.etapa.set(e);
        self.colocar(self.estado.0, self.sitio_estado());
        self.colocar(self.subtitulo.0, self.sitio_subtitulo());
        self.colocar_botones();
        if cambia_alto {
            self.dimensionar_y_centrar();
        }
        unsafe { gdi::InvalidateRect(hwnd(&self.ventana.handle), std::ptr::null(), 1) };
    }

    fn barra(&self) -> i32 {
        if self.etapa.get().compacta() {
            BARRA_COMPACTA
        } else {
            BARRA
        }
    }

    /// Botones de la barra inferior: el principal a la derecha y el secundario a su izquierda.
    fn colocar_botones(&self) {
        let (bw, bh) = BOTON;
        let by = self.barra() + (ALTO_BARRA - bh) / 2;
        let primario_x = ANCHO - MARGEN - bw;
        let ancho_cambiar = 144;
        for (b, sitio) in [
            (&self.b_cancelar, (primario_x - 8 - bw, by, bw, bh)),
            (&self.b_entrar, (primario_x, by, bw, bh)),
            (&self.b_verificar, (primario_x, by, bw, bh)),
            (&self.b_cerrar, (primario_x, by, bw, bh)),
            (&self.b_cambiar, (primario_x - 8 - ancho_cambiar, by, ancho_cambiar, bh)),
        ] {
            self.colocar(hwnd(&b.handle), sitio);
        }
    }

    fn habilitar(&self, si: bool) {
        for t in [&self.t_id, &self.t_pw, &self.t_codigo] {
            t.set_enabled(si);
        }
        self.b_entrar.set_enabled(si);
        self.b_verificar.set_enabled(si);
    }

    fn avisar(&self, tono: Tono, texto: &str) {
        self.tono.set(tono);
        self.estado.set_text(texto);
    }

    fn esc(&self, v: i32) -> i32 {
        (v * self.dpi.get() as i32 + 48) / 96
    }

    /// El subtítulo admite tres líneas en las etapas sin campos (el Apple ID puede ser largo).
    fn sitio_subtitulo(&self) -> (i32, i32, i32, i32) {
        let x = MARGEN + ICONO + 16;
        (x, 52, ANCHO - MARGEN - x, if self.etapa.get().compacta() { 56 } else { 36 })
    }

    /// La línea de estado va justo debajo del último campo de la etapa.
    fn sitio_estado(&self) -> (i32, i32, i32, i32) {
        let y = if self.etapa.get() == Etapa::Credenciales { 226 } else { 168 };
        (MARGEN, y, ANCHO - 2 * MARGEN, 52)
    }

    /// Mueve un control a una posición dada en píxeles a 96 ppp.
    fn colocar(&self, h: HWND, (x, y, w, alto): (i32, i32, i32, i32)) {
        let (x, y, w, alto) = (self.esc(x), self.esc(y), self.esc(w), self.esc(alto));
        unsafe { wm::SetWindowPos(h, std::ptr::null_mut(), x, y, w, alto, wm::SWP_NOZORDER | wm::SWP_NOACTIVATE) };
    }

    /// Coloca controles, fuentes e iconos para el DPI actual.
    fn maquetar(&self, dpi: u32) {
        self.dpi.set(dpi);
        let normal_px = self.esc(13) as u32;
        let titulo_px = self.esc(20) as u32;
        let mut normal = nwg::Font::default();
        let mut titulo = nwg::Font::default();
        let _ = nwg::Font::builder().family("Segoe UI").size_absolute(normal_px).build(&mut normal);
        let _ = nwg::Font::builder().family("Segoe UI Semibold").size_absolute(titulo_px).weight(600).build(&mut titulo);
        self.titulo.set_font(&titulo);
        for l in [&self.subtitulo, &self.l_id, &self.l_pw, &self.l_codigo, &self.estado] {
            l.set_font(&normal);
        }
        for t in [&self.t_id, &self.t_pw, &self.t_codigo] {
            t.set_font(Some(&normal));
        }
        for b in [&self.b_cancelar, &self.b_entrar, &self.b_verificar, &self.b_cambiar, &self.b_cerrar] {
            b.set_font(Some(&normal));
        }
        *self.fuentes.borrow_mut() = Some((normal, titulo));

        let texto_x = MARGEN + ICONO + 16;
        let ancho_campo = ANCHO - 2 * MARGEN;
        let sitios: [(HWND, (i32, i32, i32, i32)); 9] = [
            (self.titulo.0, (texto_x, 22, ANCHO - MARGEN - texto_x, 28)),
            (self.subtitulo.0, self.sitio_subtitulo()),
            (self.l_id.0, (MARGEN, 104, ancho_campo, 20)),
            (hwnd(&self.t_id.handle), (MARGEN, 126, ancho_campo, 28)),
            (self.l_pw.0, (MARGEN, 164, ancho_campo, 20)),
            (hwnd(&self.t_pw.handle), (MARGEN, 186, ancho_campo, 28)),
            (self.l_codigo.0, (MARGEN, 104, ancho_campo, 20)),
            (hwnd(&self.t_codigo.handle), (MARGEN, 126, 160, 28)),
            (self.estado.0, self.sitio_estado()),
        ];
        for (h, sitio) in sitios {
            self.colocar(h, sitio);
        }
        self.colocar_botones();
        // Un poco de aire a los lados del texto en las cajas.
        let m = self.esc(4) as isize;
        for t in [&self.t_id, &self.t_pw, &self.t_codigo] {
            unsafe { wm::SendMessageW(hwnd(&t.handle), 0x00D3 /* EM_SETMARGINS */, 3, (m | m << 16) as _) };
        }

        // Iconos de la barra de título, Alt-Tab y cabecera, al tamaño exacto de este DPI.
        let v = hwnd(&self.ventana.handle);
        let cargar = |lado: i32| unsafe {
            wm::LoadImageW(GetModuleHandleW(std::ptr::null()), 1 as _, wm::IMAGE_ICON, lado, lado, 0)
        };
        unsafe {
            for (tipo, metrica) in [(wm::ICON_SMALL, wm::SM_CXSMICON), (wm::ICON_BIG, wm::SM_CXICON)] {
                let nuevo = cargar(GetSystemMetricsForDpi(metrica, dpi));
                let viejo = wm::SendMessageW(v, wm::WM_SETICON, tipo as usize, nuevo as isize);
                if viejo != 0 {
                    wm::DestroyIcon(viejo as _);
                }
            }
            let viejo = self.icono.replace(cargar(self.esc(ICONO)) as isize);
            if viejo != 0 {
                wm::DestroyIcon(viejo as _);
            }
            gdi::InvalidateRect(v, std::ptr::null(), 1);
        }
    }

    /// Fija el tamaño del área cliente para el DPI actual y centra la ventana en su monitor.
    fn dimensionar_y_centrar(&self) {
        let v = hwnd(&self.ventana.handle);
        unsafe {
            let estilo = wm::GetWindowLongW(v, wm::GWL_STYLE) as u32;
            let estilo_ex = wm::GetWindowLongW(v, wm::GWL_EXSTYLE) as u32;
            let mut r = RECT { left: 0, top: 0, right: self.esc(ANCHO), bottom: self.esc(self.barra() + ALTO_BARRA) };
            AdjustWindowRectExForDpi(&mut r, estilo, 0, estilo_ex, self.dpi.get());
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let mut info: gdi::MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<gdi::MONITORINFO>() as u32;
            gdi::GetMonitorInfoW(gdi::MonitorFromWindow(v, gdi::MONITOR_DEFAULTTOPRIMARY), &mut info);
            let t = info.rcWork;
            let x = t.left + (t.right - t.left - w) / 2;
            let y = t.top + (t.bottom - t.top - h) / 2;
            wm::SetWindowPos(v, std::ptr::null_mut(), x, y, w, h, wm::SWP_NOZORDER | wm::SWP_NOACTIVATE);
        }
    }

    /// Fondo blanco, barra inferior gris con su línea divisoria y el icono de la cabecera.
    fn pintar_fondo(&self, dc: gdi::HDC) {
        let v = hwnd(&self.ventana.handle);
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            wm::GetClientRect(v, &mut r);
            let barra = self.esc(self.barra());
            let rellenar = |rect: RECT, color: u32| {
                let pincel = gdi::CreateSolidBrush(color);
                gdi::FillRect(dc, &rect, pincel);
                gdi::DeleteObject(pincel);
            };
            rellenar(RECT { bottom: barra, ..r }, BLANCO);
            rellenar(RECT { top: barra, ..r }, GRIS_BARRA);
            rellenar(RECT { top: barra, bottom: barra + self.esc(1).max(1), ..r }, LINEA);
            let lado = self.esc(ICONO);
            let p = self.esc(MARGEN);
            wm::DrawIconEx(dc, p, p, self.icono.get() as _, lado, lado, 0, std::ptr::null_mut(), wm::DI_NORMAL);
        }
    }

    /// Colores del texto de las etiquetas (todas sobre fondo blanco).
    fn color_etiqueta(&self, etiqueta: HWND) -> Option<u32> {
        let es = |l: &Texto| l.0 == etiqueta;
        if es(&self.estado) {
            Some(match self.tono.get() {
                Tono::Info => TEXTO_SUAVE,
                Tono::Error => ROJO,
                Tono::Exito => VERDE,
            })
        } else if es(&self.subtitulo) {
            Some(TEXTO_SUAVE)
        } else if es(&self.titulo) || es(&self.l_id) || es(&self.l_pw) || es(&self.l_codigo) {
            Some(TEXTO)
        } else {
            None
        }
    }
}

fn construir(ui: &mut Ui, apple_id: &str) -> Result<(), nwg::NwgError> {
    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW)
        .size((ANCHO, BARRA + ALTO_BARRA))
        .title(t(T::LogVentana))
        .build(&mut ui.ventana)?;
    let v = &ui.ventana;
    let padre = hwnd(&v.handle);
    ui.titulo = Texto::nuevo(padre, t(T::LogTitulo));
    ui.subtitulo = Texto::nuevo(padre, t(T::LogSubCred));
    ui.l_id = Texto::nuevo(padre, t(T::LogAppleId));
    nwg::TextInput::builder().text(apple_id).parent(v).build(&mut ui.t_id)?;
    ui.l_pw = Texto::nuevo(padre, t(T::LogContrasena));
    nwg::TextInput::builder().password(Some('●')).parent(v).build(&mut ui.t_pw)?;
    ui.l_codigo = Texto::nuevo(padre, t(T::LogCodigo));
    nwg::TextInput::builder().limit(7).parent(v).build(&mut ui.t_codigo)?;
    ui.estado = Texto::nuevo(padre, "");
    nwg::Button::builder().text(t(T::LogCancelar)).parent(v).build(&mut ui.b_cancelar)?;
    nwg::Button::builder().text(t(T::LogEntrar)).parent(v).build(&mut ui.b_entrar)?;
    nwg::Button::builder().text(t(T::LogVerificar)).parent(v).build(&mut ui.b_verificar)?;
    nwg::Button::builder().text(t(T::LogCambiar)).parent(v).build(&mut ui.b_cambiar)?;
    nwg::Button::builder().text(t(T::LogCerrar)).parent(v).build(&mut ui.b_cerrar)?;
    nwg::Notice::builder().parent(v).build(&mut ui.aviso)?;
    // Los botones de acción se dibujan como botón predeterminado (Intro).
    for b in [&ui.b_entrar, &ui.b_verificar, &ui.b_cerrar] {
        unsafe { wm::SendMessageW(hwnd(&b.handle), wm::BM_SETSTYLE, wm::BS_DEFPUSHBUTTON as usize, 1) };
    }
    Ok(())
}

/// Solo una ventana de inicio de sesión a la vez: si ya hay una, se trae al frente.
fn instancia_unica() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let sufijo = std::env::var("ICOPIA_DIR").map(|d| format!("-{:x}", d.bytes().fold(0u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64)))).unwrap_or_default();
    let nombre: Vec<u16> = format!("Local\\icopia-login{sufijo}\0").encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, nombre.as_ptr());
        if !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        let titulo: Vec<u16> = t(T::LogVentana).encode_utf16().chain([0]).collect();
        let otra = wm::FindWindowW(std::ptr::null(), titulo.as_ptr());
        if !otra.is_null() {
            wm::ShowWindow(otra, wm::SW_RESTORE);
            wm::SetForegroundWindow(otra);
        }
        false
    }
}

pub fn main() {
    if !instancia_unica() {
        std::process::exit(1);
    }
    let apple_id = Config::cargar().ok().and_then(|c| c.apple_id).unwrap_or_default();
    nwg::init().expect("no pude iniciar la interfaz");
    let mut ui = Ui::default();
    construir(&mut ui, &apple_id).expect("no pude crear la ventana");
    let ui = Rc::new(ui);

    // Pintado propio, colores del texto y cambios de DPI (al mover la ventana de monitor).
    let ui_raw = ui.clone();
    let crudo = nwg::bind_raw_event_handler(&ui.ventana.handle, 0x1_0000, move |_h, msg, w, l| unsafe {
        match msg {
            wm::WM_ERASEBKGND => {
                ui_raw.pintar_fondo(w as gdi::HDC);
                Some(1)
            }
            wm::WM_CTLCOLORSTATIC => {
                let color = ui_raw.color_etiqueta(l as HWND)?;
                gdi::SetTextColor(w as gdi::HDC, color);
                gdi::SetBkColor(w as gdi::HDC, BLANCO);
                Some(gdi::GetStockObject(gdi::WHITE_BRUSH) as isize)
            }
            wm::WM_DPICHANGED => {
                ui_raw.maquetar((w & 0xFFFF) as u32);
                let r = &*(l as *const RECT);
                wm::SetWindowPos(
                    hwnd(&ui_raw.ventana.handle),
                    std::ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    wm::SWP_NOZORDER | wm::SWP_NOACTIVATE,
                );
                Some(0)
            }
            _ => None,
        }
    })
    .expect("manejador de la ventana");

    let (tx_orden, rx_orden) = mpsc::channel::<Orden>();
    let (tx_resp, rx_resp) = mpsc::channel::<Respuesta>();
    let aviso = ui.aviso.sender();
    std::thread::spawn(move || trabajador(rx_orden, tx_resp, aviso));

    // Con un Apple ID guardado, primero se mira si la sesión sigue valiendo.
    let inicial = if apple_id.is_empty() { Etapa::Credenciales } else { Etapa::Comprobando };
    ui.etapa.set(inicial);
    ui.maquetar(unsafe { GetDpiForWindow(hwnd(&ui.ventana.handle)) });
    ui.dimensionar_y_centrar();
    ui.mostrar_etapa(inicial);
    ui.ventana.set_visible(true);
    if inicial == Etapa::Comprobando {
        ui.b_cancelar.set_focus();
        let _ = tx_orden.send(Orden::Comprobar(apple_id.clone()));
    } else {
        ui.t_id.set_focus();
    }

    let exito = Rc::new(Cell::new(false));
    let ocupado = Rc::new(Cell::new(inicial == Etapa::Comprobando));

    let enviar = {
        let (ui, ocupado) = (ui.clone(), ocupado.clone());
        move || {
            if ocupado.get() {
                return;
            }
            let orden = match ui.etapa.get() {
                Etapa::Comprobando | Etapa::YaIniciada => return,
                Etapa::Credenciales => {
                    let apple_id = ui.t_id.text().trim().to_string();
                    let password = ui.t_pw.text();
                    if apple_id.is_empty() {
                        ui.avisar(Tono::Error, t(T::LogFaltaId));
                        ui.t_id.set_focus();
                        return;
                    }
                    if password.is_empty() {
                        ui.avisar(Tono::Error, t(T::LogFaltaPw));
                        ui.t_pw.set_focus();
                        return;
                    }
                    ui.avisar(Tono::Info, t(T::LogConectando));
                    Orden::Entrar { apple_id, password }
                }
                Etapa::Codigo => {
                    let Some(c) = normalizar_codigo(&ui.t_codigo.text()) else {
                        ui.avisar(Tono::Error, t(T::LogSeisDigitos));
                        ui.t_codigo.set_focus();
                        return;
                    };
                    ui.avisar(Tono::Info, t(T::LogVerificando));
                    Orden::Codigo(c)
                }
            };
            ocupado.set(true);
            ui.habilitar(false);
            let _ = tx_orden.send(orden);
        }
    };

    let recibir = {
        let (ui, ocupado, exito) = (ui.clone(), ocupado.clone(), exito.clone());
        move || {
            while let Ok(r) = rx_resp.try_recv() {
                let paso = match r {
                    Respuesta::Cerrar => {
                        nwg::stop_thread_dispatch();
                        return;
                    }
                    Respuesta::Sesion(s) => {
                        if ui.etapa.get() != Etapa::Comprobando {
                            continue; // el usuario ya eligió otra cosa
                        }
                        ocupado.set(false);
                        match s {
                            EstadoSesion::Valida => {
                                exito.set(true);
                                ui.mostrar_etapa(Etapa::YaIniciada);
                                ui.b_cerrar.set_focus();
                            }
                            EstadoSesion::Caducada => {
                                ui.mostrar_etapa(Etapa::Credenciales);
                                ui.avisar(Tono::Info, t(T::LogCaducada));
                                ui.t_pw.set_focus();
                            }
                            EstadoSesion::Desconocida(m) => {
                                ui.mostrar_etapa(Etapa::Credenciales);
                                ui.avisar(Tono::Error, &tf(T::LogNoComprobada, &[("m", &m)]));
                                ui.t_pw.set_focus();
                            }
                        }
                        continue;
                    }
                    Respuesta::Paso(p) => p,
                };
                ocupado.set(false);
                let mut etapa = ui.etapa.get();
                match paso {
                    Paso::Hecho => {
                        exito.set(true);
                        ocupado.set(true); // ya no se aceptan más órdenes
                        ui.avisar(Tono::Exito, t(T::LogHecho));
                        continue;
                    }
                    Paso::PideCodigo => {
                        etapa = Etapa::Codigo;
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.avisar(Tono::Info, t(T::LogCodigoEnviado));
                    }
                    Paso::Reintentar(m) => {
                        ui.avisar(Tono::Error, &m);
                        match etapa {
                            Etapa::Codigo => ui.t_codigo.set_text(""),
                            _ => ui.t_pw.set_text(""),
                        }
                    }
                    Paso::Fallo(m) => {
                        etapa = Etapa::Credenciales;
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.avisar(Tono::Error, &m);
                    }
                }
                ui.mostrar_etapa(etapa);
                ui.habilitar(true);
                match etapa {
                    Etapa::Codigo => ui.t_codigo.set_focus(),
                    _ if ui.t_id.text().trim().is_empty() => ui.t_id.set_focus(),
                    _ => ui.t_pw.set_focus(),
                }
            }
        }
    };

    let cambiar_cuenta = {
        let (ui, exito) = (ui.clone(), exito.clone());
        move || {
            exito.set(false);
            ui.mostrar_etapa(Etapa::Credenciales);
            ui.avisar(Tono::Info, "");
            ui.t_id.set_focus();
            unsafe { wm::SendMessageW(hwnd(&ui.t_id.handle), 0x00B1 /* EM_SETSEL */, 0, -1) };
        }
    };

    let ui_ev = ui.clone();
    let manejador = nwg::full_bind_event_handler(&ui.ventana.handle, move |evento, _datos, handle| {
        use nwg::Event as E;
        let ya = ui_ev.etapa.get() == Etapa::YaIniciada;
        match evento {
            E::OnWindowClose if handle == ui_ev.ventana.handle => nwg::stop_thread_dispatch(),
            E::OnKeyEsc => nwg::stop_thread_dispatch(),
            E::OnKeyEnter if handle == ui_ev.b_cancelar.handle || ya && handle != ui_ev.b_cambiar.handle => nwg::stop_thread_dispatch(),
            E::OnKeyEnter if handle == ui_ev.b_cambiar.handle => cambiar_cuenta(),
            E::OnKeyEnter => enviar(),
            E::OnButtonClick if handle == ui_ev.b_cancelar.handle || handle == ui_ev.b_cerrar.handle => nwg::stop_thread_dispatch(),
            E::OnButtonClick if handle == ui_ev.b_cambiar.handle => cambiar_cuenta(),
            E::OnButtonClick if handle == ui_ev.b_entrar.handle || handle == ui_ev.b_verificar.handle => enviar(),
            E::OnNotice if handle == ui_ev.aviso.handle => recibir(),
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&manejador);
    let _ = nwg::unbind_raw_event_handler(&crudo);
    std::process::exit(if exito.get() { 0 } else { 1 });
}