#![windows_subsystem = "windows"]
//! Ventana pequeña para iniciar sesión en iCloud (lo mismo que `icopia login`, sin consola).
//! Sale con código 0 si la sesión quedó iniciada; 1 si se cierra o cancela.
use icopia::config::{self, Config};
use icopia::login::{normalizar_codigo, FlujoLogin, Paso};
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
    Entrar { apple_id: String, password: String },
    Codigo(String),
}

enum Respuesta {
    Paso(Paso),
    Cerrar,
}

/// Hace las llamadas (bloqueantes) al puente fuera del hilo de la ventana.
fn trabajador(rx: Receiver<Orden>, tx: Sender<Respuesta>, aviso: nwg::NoticeSender) {
    let mut flujo: Option<FlujoLogin<Puente>> = None;
    let mut apple_id = String::new();
    for orden in rx {
        let mut paso = match orden {
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
                None => Paso::Fallo("Vuelve a empezar.".into()),
            },
        };
        if paso == Paso::Hecho {
            let guardado = Config::cargar().and_then(|mut cfg| {
                cfg.apple_id = Some(apple_id.clone());
                cfg.guardar()
            });
            if let Err(e) = guardado {
                paso = Paso::Fallo(format!("No pude guardar la configuración: {e:#}"));
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
    #[default]
    Credenciales,
    Codigo,
}

/// Color de la línea de estado.
#[derive(Clone, Copy, Default)]
enum Tono {
    #[default]
    Info,
    Error,
    Exito,
}

const SUB_CREDENCIALES: &str = "Usa tu Apple ID. La contraseña solo se envía a Apple; icopia no la guarda.";
const SUB_CODIGO: &str = "Escribe el código de 6 dígitos que apareció en tu iPhone.";

// Medidas en píxeles a 96 ppp (rejilla de 8 px); se escalan con el DPI del monitor.
const ANCHO: i32 = 400;
const MARGEN: i32 = 24;
const ICONO: i32 = 40;
const BARRA: i32 = 288; // empieza la barra inferior de botones
const ALTO: i32 = BARRA + 64;
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
        let cred = e == Etapa::Credenciales;
        self.subtitulo.set_text(if cred { SUB_CREDENCIALES } else { SUB_CODIGO });
        for c in [&self.l_id, &self.l_pw] {
            c.set_visible(cred);
        }
        for c in [&self.t_id, &self.t_pw] {
            c.set_visible(cred);
        }
        self.b_entrar.set_visible(cred);
        self.l_codigo.set_visible(!cred);
        self.t_codigo.set_visible(!cred);
        self.b_verificar.set_visible(!cred);
        self.etapa.set(e);
        self.colocar(self.estado.0, self.sitio_estado());
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
        for b in [&self.b_cancelar, &self.b_entrar, &self.b_verificar] {
            b.set_font(Some(&normal));
        }
        *self.fuentes.borrow_mut() = Some((normal, titulo));

        let texto_x = MARGEN + ICONO + 16;
        let ancho_campo = ANCHO - 2 * MARGEN;
        let (bw, bh) = BOTON;
        let by = BARRA + (ALTO - BARRA - bh) / 2;
        let primario_x = ANCHO - MARGEN - bw;
        let sitios: [(HWND, (i32, i32, i32, i32)); 12] = [
            (self.titulo.0, (texto_x, 22, ANCHO - MARGEN - texto_x, 28)),
            (self.subtitulo.0, (texto_x, 52, ANCHO - MARGEN - texto_x, 36)),
            (self.l_id.0, (MARGEN, 104, ancho_campo, 20)),
            (hwnd(&self.t_id.handle), (MARGEN, 126, ancho_campo, 28)),
            (self.l_pw.0, (MARGEN, 164, ancho_campo, 20)),
            (hwnd(&self.t_pw.handle), (MARGEN, 186, ancho_campo, 28)),
            (self.l_codigo.0, (MARGEN, 104, ancho_campo, 20)),
            (hwnd(&self.t_codigo.handle), (MARGEN, 126, 160, 28)),
            (self.estado.0, self.sitio_estado()),
            (hwnd(&self.b_cancelar.handle), (primario_x - 8 - bw, by, bw, bh)),
            (hwnd(&self.b_entrar.handle), (primario_x, by, bw, bh)),
            (hwnd(&self.b_verificar.handle), (primario_x, by, bw, bh)),
        ];
        for (h, sitio) in sitios {
            self.colocar(h, sitio);
        }
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
            let mut r = RECT { left: 0, top: 0, right: self.esc(ANCHO), bottom: self.esc(ALTO) };
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
            let barra = self.esc(BARRA);
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
        .size((ANCHO, ALTO))
        .title("Iniciar sesión · icopia")
        .build(&mut ui.ventana)?;
    let v = &ui.ventana;
    let padre = hwnd(&v.handle);
    ui.titulo = Texto::nuevo(padre, "Inicia sesión en iCloud");
    ui.subtitulo = Texto::nuevo(padre, SUB_CREDENCIALES);
    ui.l_id = Texto::nuevo(padre, "Apple ID");
    nwg::TextInput::builder().text(apple_id).parent(v).build(&mut ui.t_id)?;
    ui.l_pw = Texto::nuevo(padre, "Contraseña");
    nwg::TextInput::builder().password(Some('●')).parent(v).build(&mut ui.t_pw)?;
    ui.l_codigo = Texto::nuevo(padre, "Código de verificación");
    nwg::TextInput::builder().limit(7).parent(v).build(&mut ui.t_codigo)?;
    ui.estado = Texto::nuevo(padre, "");
    nwg::Button::builder().text("Cancelar").parent(v).build(&mut ui.b_cancelar)?;
    nwg::Button::builder().text("Entrar").parent(v).build(&mut ui.b_entrar)?;
    nwg::Button::builder().text("Verificar").parent(v).build(&mut ui.b_verificar)?;
    nwg::Notice::builder().parent(v).build(&mut ui.aviso)?;
    // Los botones de acción se dibujan como botón predeterminado (Intro).
    for b in [&ui.b_entrar, &ui.b_verificar] {
        unsafe { wm::SendMessageW(hwnd(&b.handle), wm::BM_SETSTYLE, wm::BS_DEFPUSHBUTTON as usize, 1) };
    }
    Ok(())
}

fn main() {
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

    ui.maquetar(unsafe { GetDpiForWindow(hwnd(&ui.ventana.handle)) });
    ui.dimensionar_y_centrar();
    ui.mostrar_etapa(Etapa::Credenciales);
    ui.ventana.set_visible(true);
    if apple_id.is_empty() {
        ui.t_id.set_focus();
    } else {
        ui.t_pw.set_focus();
    }

    let (tx_orden, rx_orden) = mpsc::channel::<Orden>();
    let (tx_resp, rx_resp) = mpsc::channel::<Respuesta>();
    let aviso = ui.aviso.sender();
    std::thread::spawn(move || trabajador(rx_orden, tx_resp, aviso));

    let exito = Rc::new(Cell::new(false));
    let etapa = Rc::new(Cell::new(Etapa::Credenciales));
    let ocupado = Rc::new(Cell::new(false));

    let enviar = {
        let (ui, etapa, ocupado) = (ui.clone(), etapa.clone(), ocupado.clone());
        move || {
            if ocupado.get() {
                return;
            }
            let orden = match etapa.get() {
                Etapa::Credenciales => {
                    let apple_id = ui.t_id.text().trim().to_string();
                    let password = ui.t_pw.text();
                    if apple_id.is_empty() {
                        ui.avisar(Tono::Error, "Escribe tu Apple ID.");
                        ui.t_id.set_focus();
                        return;
                    }
                    if password.is_empty() {
                        ui.avisar(Tono::Error, "Escribe la contraseña.");
                        ui.t_pw.set_focus();
                        return;
                    }
                    ui.avisar(Tono::Info, "Conectando con Apple…");
                    Orden::Entrar { apple_id, password }
                }
                Etapa::Codigo => {
                    let Some(c) = normalizar_codigo(&ui.t_codigo.text()) else {
                        ui.avisar(Tono::Error, "El código son 6 dígitos.");
                        ui.t_codigo.set_focus();
                        return;
                    };
                    ui.avisar(Tono::Info, "Verificando el código…");
                    Orden::Codigo(c)
                }
            };
            ocupado.set(true);
            ui.habilitar(false);
            let _ = tx_orden.send(orden);
        }
    };

    let recibir = {
        let (ui, etapa, ocupado, exito) = (ui.clone(), etapa.clone(), ocupado.clone(), exito.clone());
        move || {
            while let Ok(r) = rx_resp.try_recv() {
                let paso = match r {
                    Respuesta::Cerrar => {
                        nwg::stop_thread_dispatch();
                        return;
                    }
                    Respuesta::Paso(p) => p,
                };
                ocupado.set(false);
                match paso {
                    Paso::Hecho => {
                        exito.set(true);
                        ocupado.set(true); // ya no se aceptan más órdenes
                        ui.avisar(Tono::Exito, "Sesión iniciada. Dura unos 2 meses; icopia te avisará cuando caduque.");
                        continue;
                    }
                    Paso::PideCodigo => {
                        etapa.set(Etapa::Codigo);
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.avisar(Tono::Info, "Apple ha enviado un código a tu iPhone.");
                    }
                    Paso::Reintentar(m) => {
                        ui.avisar(Tono::Error, &m);
                        match etapa.get() {
                            Etapa::Credenciales => ui.t_pw.set_text(""),
                            Etapa::Codigo => ui.t_codigo.set_text(""),
                        }
                    }
                    Paso::Fallo(m) => {
                        etapa.set(Etapa::Credenciales);
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.avisar(Tono::Error, &m);
                    }
                }
                ui.mostrar_etapa(etapa.get());
                ui.habilitar(true);
                match etapa.get() {
                    Etapa::Credenciales if ui.t_id.text().trim().is_empty() => ui.t_id.set_focus(),
                    Etapa::Credenciales => ui.t_pw.set_focus(),
                    Etapa::Codigo => ui.t_codigo.set_focus(),
                }
            }
        }
    };

    let ui_ev = ui.clone();
    let manejador = nwg::full_bind_event_handler(&ui.ventana.handle, move |evento, _datos, handle| {
        use nwg::Event as E;
        match evento {
            E::OnWindowClose if handle == ui_ev.ventana.handle => nwg::stop_thread_dispatch(),
            E::OnKeyEsc => nwg::stop_thread_dispatch(),
            E::OnKeyEnter if handle == ui_ev.b_cancelar.handle => nwg::stop_thread_dispatch(),
            E::OnKeyEnter => enviar(),
            E::OnButtonClick if handle == ui_ev.b_cancelar.handle => nwg::stop_thread_dispatch(),
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
