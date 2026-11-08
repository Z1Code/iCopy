#![windows_subsystem = "windows"]
use anyhow::Context;
use icopia::bandeja::asistente::{Accion as AccionAsistente, Asistente, Paso, Revision};
use icopia::bandeja::asistente_ventana::VentanaAsistente;
use icopia::bandeja::estado::{EstadoUi, EventoUi, Fase};
use icopia::i18n::{t, T};
use icopia::liberar::ControlBorrado;
use icopia::bandeja::panel::Principal;
use icopia::bandeja::hover::Rect;
use icopia::bandeja::panel_ventana::{Accion, Panel};
use icopia::bandeja::hilo_motor::{self, Orden};
use icopia::bandeja::{autoarranque, avisos, icono};
use icopia::config::{self, Config};
use icopia::fuente::FuenteIcloud;
use icopia::progreso_liberar::{self, ProgresoLiberar};
use icopia::puente::Puente;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Eventos del bucle principal.
enum Ev {
    Ui(EventoUi),
    Bandeja(TrayIconEvent),
    /// Lectura fresca del registro: totales (solo con el panel abierto) y avance de "liberar espacio".
    Lectura { progreso: Option<icopia::registro::Progreso>, liberar: Option<ProgresoLiberar> },
}

/// Menú contextual del icono. Para que se cierre al hacer clic fuera, su ventana dueña tiene que
/// estar en primer plano antes de TrackPopupMenu, y después hay que enviarle WM_NULL
/// (https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu).
fn mostrar_menu(menu: &Menu, dueno: isize) {
    use tray_icon::menu::ContextMenu;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, PostMessageW, TrackPopupMenu, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_NULL};
    let hwnd = dueno as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        icopia::bandeja::panel_ventana::traer_al_frente(hwnd);
        TrackPopupMenu(menu.hpopupmenu() as _, TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
    }
}

fn centro_de(r: &tray_icon::Rect) -> (i32, i32) {
    (r.position.x as i32 + r.size.width as i32 / 2, r.position.y as i32 + r.size.height as i32 / 2)
}

/// Relee el registro (solo lectura, en su hilo): cada segundo con el panel abierto, para que los
/// totales nunca sean un dato viejo; cada 5 s con él cerrado, solo el avance de "liberar espacio"
/// (lo escribe otro proceso) para el tooltip.
fn sondear_registro(abierto: Arc<AtomicBool>, proxy: tao::event_loop::EventLoopProxy<Ev>) {
    std::thread::spawn(move || {
        let mut abierta: Option<(std::path::PathBuf, icopia::registro::Registro)> = None;
        let mut vuelta = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            vuelta += 1;
            // Panel o asistente abiertos: lectura completa cada segundo.
            let panel = abierto.load(Ordering::Relaxed);
            if !panel && vuelta % 5 != 0 {
                continue;
            }
            let Some(ruta) = Config::cargar().ok().and_then(|c| c.destino).map(|d| d.join(".icopia").join("registro.db")) else { continue };
            if abierta.as_ref().is_none_or(|(r, _)| *r != ruta) {
                abierta = ruta.exists().then(|| icopia::registro::Registro::abrir_lectura(&ruta).ok().map(|reg| (ruta, reg))).flatten();
            }
            let Some((_, reg)) = abierta.as_ref() else { continue };
            let progreso = if panel { reg.progreso().ok() } else { None };
            let liberar = progreso_liberar::leer(reg);
            if proxy.send_event(Ev::Lectura { progreso, liberar }).is_err() {
                return;
            }
        }
    });
}

/// Tooltip del icono: mientras se libera espacio, ese avance; si no, el estado de la copia.
fn texto_tooltip(estado: &EstadoUi, liberar: Option<&ProgresoLiberar>) -> String {
    let ahora = chrono::Utc::now().timestamp();
    match liberar.and_then(|p| icopia::bandeja::panel::texto_bandeja_liberar(p, ahora)) {
        Some(t) => format!("icopia — {t}"),
        None => format!("icopia — {}", estado.texto()),
    }
}

fn instancia_unica() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    // Con ICOPIA_DIR (pruebas, otra configuración) se permite una instancia por carpeta.
    let sufijo = std::env::var("ICOPIA_DIR").map(|d| format!("-{:x}", d.bytes().fold(0u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64)))).unwrap_or_default();
    let nombre: Vec<u16> = format!("Local\\icopia-bandeja{sufijo}\0").encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, nombre.as_ptr());
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn icono_de(color: icopia::bandeja::estado::Color) -> Icon {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    // Tamaño de icono pequeño del sistema (16 px al 100 %, 24 al 150 %…): así se ve nítido.
    let lado = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64) as u32;
    let (rgba, w, h) = icono::rgba(color, lado);
    Icon::from_rgba(rgba, w, h).expect("icono válido")
}

/// Ejecuta `cmd` en otro hilo, pausando la copia mientras dure y reanudándola al acabar.
fn lanzar_pausando(mut cmd: std::process::Command, tx: &mpsc::Sender<Orden>, cancelar: &AtomicBool, ya_pausado: bool) {
    if !ya_pausado {
        // Primero la orden y luego `cancelar`: así el hilo del motor no puede perder la pausa.
        let _ = tx.send(Orden::Pausar);
        cancelar.store(true, Ordering::SeqCst);
    }
    let tx = tx.clone();
    std::thread::spawn(move || {
        let _ = cmd.status();
        if !ya_pausado {
            let _ = tx.send(Orden::Reanudar);
        }
    });
}

fn exe_junto(nombre: &str) -> std::path::PathBuf {
    std::env::current_exe().expect("ruta del exe").with_file_name(nombre)
}

/// Abre la ventana de inicio de sesión (`icopia-login.exe`), pausando la copia mientras tanto.
fn lanzar_login(tx: &mpsc::Sender<Orden>, cancelar: &AtomicBool, ya_pausado: bool) {
    lanzar_pausando(std::process::Command::new(exe_junto("icopia-login.exe")), tx, cancelar, ya_pausado);
}

fn main() {
    if !instancia_unica() {
        return;
    }
    let mut cfg = Config::cargar().unwrap_or_default();
    if cfg.destino.is_none() {
        if let Some(p) = rfd::FileDialog::new().set_title(t(T::DlgDestino)).pick_folder() {
            cfg.destino = Some(p);
            let _ = cfg.guardar();
        }
    }

    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = Mutex::new(event_loop.create_proxy());
    let proxy_bandeja = Mutex::new(event_loop.create_proxy());
    let panel_abierto = Arc::new(AtomicBool::new(false));
    sondear_registro(panel_abierto.clone(), event_loop.create_proxy());
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy_bandeja.lock().unwrap().send_event(Ev::Bandeja(e));
    }));
    let (tx, rx) = mpsc::channel::<Orden>();
    let cancelar = Arc::new(AtomicBool::new(false));
    // Pausar o cancelar un borrado en marcha (el motor está ocupado: no pasa por la cola).
    let control = Arc::new(ControlBorrado::default());
    hilo_motor::lanzar_con(
        rx,
        cancelar.clone(),
        control.clone(),
        Box::new(|| Config::cargar().unwrap_or_default()),
        Box::new(|cfg: &Config| {
            let id = cfg.apple_id.as_deref().context("sin Apple ID")?;
            Ok(Box::new(Puente::iniciar(id, &config::dir_sesion(), &config::log_puente())?) as Box<dyn FuenteIcloud>)
        }),
        Box::new(move |e| {
            let _ = proxy.lock().unwrap().send_event(Ev::Ui(e));
        }),
    );
    // Sin Apple ID se pide iniciar sesión ya. Con uno, la primera ronda del motor (en su hilo)
    // comprueba la sesión y, si caducó, se abre la ventana desde el bucle (una vez por caducidad).
    let mut login_abierto = cfg.apple_id.is_none();
    if login_abierto {
        lanzar_login(&tx, &cancelar, false);
    }

    let i_estado = MenuItem::new(t(T::MenuIniciando), false, None);
    let i_copiar = MenuItem::new(t(T::MenuCopiar), true, None);
    let i_pausa = MenuItem::new(t(T::MenuPausar), true, None);
    let i_verificar = MenuItem::new(t(T::MenuVerificar), true, None);
    let i_liberar = MenuItem::new(t(T::MenuLiberar), true, None);
    let i_abrir = MenuItem::new(t(T::MenuAbrir), true, None);
    let i_destino = MenuItem::new(t(T::MenuDestino), true, None);
    let i_login = MenuItem::new(t(T::MenuSesion), true, None);
    let i_auto = CheckMenuItem::new(t(T::MenuAuto), true, autoarranque::activo(), None);
    let i_salir = MenuItem::new(t(T::MenuSalir), true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &i_estado,
        &PredefinedMenuItem::separator(),
        &i_copiar,
        &i_pausa,
        &i_verificar,
        &i_liberar,
        &PredefinedMenuItem::separator(),
        &i_abrir,
        &i_destino,
        &i_login,
        &i_auto,
        &PredefinedMenuItem::separator(),
        &i_salir,
    ])
    .expect("menú");

    // El menú lo mostramos nosotros (no tray-icon) para poder cerrarlo bien al hacer clic fuera.
    let mut dueno_menu: Option<tao::window::Window> = None;
    let mut tray: Option<TrayIcon> = None;
    let mut panel: Option<Panel> = None;
    let mut estado = EstadoUi::sin_datos(Fase::Inactiva);
    let mut pausado = false;
    let mut ultimo_refresco = Instant::now();
    let mut tooltip = String::from("icopia");
    let mut liberar: Option<ProgresoLiberar> = None;
    let mut asistente: Option<VentanaAsistente> = None;
    let mut ultimo_asistente = Instant::now();
    // Abre (o trae al frente) el asistente de liberar espacio con el inventario actual del registro.
    let abrir_asistente = |asistente: &mut Option<VentanaAsistente>, destino_ev: &tao::event_loop::EventLoopWindowTarget<Ev>, liberar: &Option<ProgresoLiberar>| {
        let inventario = || {
            Config::cargar()
                .ok()
                .and_then(|c| c.destino)
                .map(|d| d.join(".icopia").join("registro.db"))
                .filter(|r| r.exists())
                .and_then(|r| icopia::registro::Registro::abrir_lectura(&r).ok())
                .and_then(|r| r.inventario_liberar().ok())
                .unwrap_or_default()
        };
        let ahora = chrono::Utc::now().timestamp();
        match asistente {
            Some(a) => {
                // Un borrado en marcha (o una revisión) se sigue viendo; si no, se empieza de nuevo.
                let en_curso = a.modelo.ocupado() || a.modelo.paso == Paso::Revisar && a.modelo.revision == Revision::EnCurso;
                if !a.visible() && !en_curso {
                    a.modelo.reiniciar(inventario(), ahora);
                }
            }
            None => *asistente = Some(VentanaAsistente::nueva(destino_ev, Asistente::nuevo(inventario(), ahora))),
        }
        let a = asistente.as_mut().expect("asistente");
        a.modelo.progreso = liberar.clone();
        a.mostrar();
    };
    let (tx_as, cancelar_as, control_as) = (tx.clone(), cancelar.clone(), control.clone());
    let abrir_carpeta_as = abrir_carpeta_fn;
    // Lo que pide el asistente.
    let atender = move |acc: AccionAsistente, a: &mut VentanaAsistente| match acc {
        AccionAsistente::Revisar(c) => {
            // Primero se corta la ronda en curso (si la hay); el motor lo vuelve a poner a cero.
            cancelar_as.store(true, Ordering::SeqCst);
            let _ = tx_as.send(Orden::LiberarRevisar(c));
        }
        AccionAsistente::CancelarRevision => cancelar_as.store(true, Ordering::SeqCst),
        AccionAsistente::Descartar => {
            let _ = tx_as.send(Orden::LiberarDescartar);
        }
        AccionAsistente::Ejecutar => {
            control_as.pausa.store(false, Ordering::SeqCst);
            control_as.cancelar.store(false, Ordering::SeqCst);
            let _ = tx_as.send(Orden::LiberarEjecutar);
        }
        AccionAsistente::Pausar(si) => control_as.pausa.store(si, Ordering::SeqCst),
        AccionAsistente::CancelarBorrado => control_as.cancelar.store(true, Ordering::SeqCst),
        AccionAsistente::AbrirArchivo(ruta) => {
            let _ = std::process::Command::new("explorer").arg(ruta).spawn();
        }
        AccionAsistente::AbrirCarpeta => abrir_carpeta_as(),
        AccionAsistente::Restaurar(sesion) => {
            let _ = tx_as.send(Orden::LiberarRestaurar(sesion));
        }
        AccionAsistente::Cerrar => {
            // Sin borrar nada todavía: se cancela la revisión o se olvida el plan.
            match (&a.modelo.paso, &a.modelo.revision) {
                (Paso::Revisar, Revision::EnCurso) => cancelar_as.store(true, Ordering::SeqCst),
                (Paso::Revisar | Paso::Confirmar, Revision::Lista(_)) => {
                    let _ = tx_as.send(Orden::LiberarDescartar);
                }
                _ => {}
            }
            if !a.modelo.ocupado() {
                a.modelo.paso = Paso::Elegir;
            }
            a.ocultar();
        }
    };
    let mut tooltip_puesto: Option<String> = Some(tooltip.clone());

    let (tx_pausa, cancelar_pausa) = (tx.clone(), cancelar.clone());
    let alternar_pausa = move |pausado: &mut bool, i_pausa: &MenuItem| {
        if *pausado {
            *pausado = false;
            i_pausa.set_text(t(T::MenuPausar));
            let _ = tx_pausa.send(Orden::Reanudar);
        } else {
            *pausado = true;
            i_pausa.set_text(t(T::MenuReanudar));
            let _ = tx_pausa.send(Orden::Pausar);
            cancelar_pausa.store(true, Ordering::SeqCst);
        }
    };
    fn abrir_carpeta_fn() {
        if let Some(d) = Config::cargar().ok().and_then(|c| c.destino) {
            let _ = std::process::Command::new("explorer").arg(d).spawn();
        }
    }
    let abrir_carpeta = abrir_carpeta_fn;

    event_loop.run(move |event, destino_ev, control_flow| {
        let mut espera = Duration::from_millis(200);
        match event {
            Event::NewEvents(StartCause::Init) => {
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu_on_left_click(false)
                        .with_tooltip("icopia")
                        .with_icon(icono_de(icopia::bandeja::estado::Color::Gris))
                        .build()
                        .expect("icono de bandeja"),
                );
                panel = Some(Panel::nuevo(destino_ev));
                let dueno = tao::window::WindowBuilder::new()
                    .with_visible(false)
                    .with_decorations(false)
                    .with_title("icopia-menu")
                    .build(destino_ev)
                    .expect("ventana del menú");
                {
                    use tao::platform::windows::WindowExtWindows;
                    use tray_icon::menu::ContextMenu;
                    // Los WM_COMMAND del menú llegan a esta ventana y muda los convierte en MenuEvent.
                    unsafe { menu.attach_menu_subclass_for_hwnd(dueno.hwnd()) };
                }
                dueno_menu = Some(dueno);
            }
            Event::UserEvent(Ev::Ui(EventoUi::Estado(e))) => {
                let t = e.texto();
                i_estado.set_text(&t);
                if let Some(tr) = &tray {
                    if e.color() != estado.color() || estado.fase == Fase::Inactiva && estado.total == 0 {
                        let _ = tr.set_icon(Some(icono_de(e.color())));
                    }
                }
                match e.fase {
                    Fase::SesionCaducada if !login_abierto => {
                        login_abierto = true;
                        lanzar_login(&tx, &cancelar, pausado);
                    }
                    // Solo listando o copiando hay prueba de que la sesión vuelve a valer.
                    Fase::Listando { .. } | Fase::Copiando { .. } => login_abierto = false,
                    _ => {}
                }
                if let Some(p) = panel.as_mut() {
                    p.cambio_de_estado(&estado, &e);
                }
                estado = e;
                tooltip = texto_tooltip(&estado, liberar.as_ref());
            }
            Event::UserEvent(Ev::Lectura { progreso, liberar: l }) => {
                if let Some(p) = progreso {
                    estado.actualizar_progreso(&p);
                }
                liberar = l;
                tooltip = texto_tooltip(&estado, liberar.as_ref());
                if let Some(a) = asistente.as_mut() {
                    a.modelo.progreso = liberar.clone();
                }
                if let Some(p) = panel.as_mut() {
                    p.poner_liberar(liberar.clone());
                }
            }
            Event::UserEvent(Ev::Ui(EventoUi::Aviso { titulo, texto })) => avisos::mostrar(&titulo, &texto),
            Event::UserEvent(Ev::Ui(EventoUi::Liberar(e))) => {
                if let Some(a) = asistente.as_mut() {
                    a.modelo.evento(e);
                    a.refrescar();
                }
            }
            Event::UserEvent(Ev::Bandeja(TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. })) => {
                if let Some(p) = panel.as_mut() {
                    p.aviso_icono(centro_de(&rect));
                }
            }
            Event::UserEvent(Ev::Bandeja(TrayIconEvent::Leave { .. })) => {
                if let Some(p) = panel.as_mut() {
                    p.salio_icono();
                }
            }
            Event::UserEvent(Ev::Bandeja(TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Up, .. })) => {
                if let Some(p) = panel.as_mut() {
                    p.ocultar();
                }
                if let Some(d) = &dueno_menu {
                    use tao::platform::windows::WindowExtWindows;
                    mostrar_menu(&menu, d.hwnd());
                }
            }
            Event::UserEvent(Ev::Bandeja(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. })) => {
                if let Some(p) = panel.as_mut() {
                    p.alternar(centro_de(&rect), &estado);
                }
            }
            Event::WindowEvent { window_id, event, .. } if panel.as_ref().is_some_and(|p| p.id() == window_id) => {
                let p = panel.as_mut().expect("panel");
                match p.evento(&event, &estado) {
                    Some(Accion::Principal(accion @ (Principal::Pausar | Principal::Reanudar))) => {
                        if pausado == (accion == Principal::Reanudar) {
                            alternar_pausa(&mut pausado, &i_pausa);
                        } else if accion == Principal::Reanudar {
                            let _ = tx.send(Orden::Reanudar);
                        }
                        p.refrescar(&estado);
                    }
                    Some(Accion::Principal(Principal::RevisarAhora)) => {
                        pausado = false;
                        i_pausa.set_text(t(T::MenuPausar));
                        let _ = tx.send(Orden::CopiarAhora);
                    }
                    Some(Accion::Principal(Principal::IniciarSesion)) => {
                        p.ocultar();
                        lanzar_login(&tx, &cancelar, pausado);
                    }
                    Some(Accion::AbrirCarpeta) => {
                        p.ocultar();
                        abrir_carpeta();
                    }
                    Some(Accion::Liberar) => {
                        p.ocultar();
                        abrir_asistente(&mut asistente, destino_ev, &liberar);
                    }
                    None => {}
                }
                if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                    p.refrescar(&estado);
                }
            }
            Event::WindowEvent { window_id, event, .. } if asistente.as_ref().is_some_and(|a| a.id() == window_id) => {
                let a = asistente.as_mut().expect("asistente");
                if let Some(acc) = a.evento(&event) {
                    atender(acc, a);
                }
            }
            Event::RedrawRequested(id) if asistente.as_ref().is_some_and(|a| a.id() == id) => {
                if let Some(a) = asistente.as_mut() {
                    a.refrescar();
                }
            }
            Event::RedrawRequested(id) if panel.as_ref().is_some_and(|p| p.id() == id) => {
                if let Some(p) = panel.as_mut() {
                    p.refrescar(&estado);
                }
            }
            _ => {}
        }
        if let Some(p) = panel.as_mut() {
            p.vigilar_raton(&estado, tray.as_ref().and_then(|t| t.rect()).map(|r| Rect {
                x0: r.position.x as i32,
                y0: r.position.y as i32,
                x1: r.position.x as i32 + r.size.width as i32,
                y1: r.position.y as i32 + r.size.height as i32,
            }));
            if p.raton_pendiente() {
                espera = espera.min(Duration::from_millis(50));
            }
            // Con el panel abierto no se muestra el tooltip (se solaparían).
            let quiero = if p.visible() { None } else { Some(tooltip.clone()) };
            if quiero != tooltip_puesto {
                if let Some(tr) = &tray {
                    let _ = tr.set_tooltip(quiero.as_deref());
                }
                tooltip_puesto = quiero;
            }
            panel_abierto.store(p.visible() || asistente.as_ref().is_some_and(|a| a.visible()), Ordering::Relaxed);
            p.muestrear();
            if p.visible() {
                let cada = p.intervalo(&estado);
                if ultimo_refresco.elapsed() >= cada {
                    ultimo_refresco = Instant::now();
                    p.refrescar(&estado);
                }
                espera = espera.min(cada);
            }
        }
        if let Some(a) = asistente.as_mut().filter(|a| a.visible()) {
            if let Some(acc) = a.tick() {
                atender(acc, a);
            }
            let cada = a.intervalo();
            if ultimo_asistente.elapsed() >= cada {
                ultimo_asistente = Instant::now();
                a.refrescar();
            }
            espera = espera.min(cada);
        }
        *control_flow = ControlFlow::WaitUntil(Instant::now() + espera);
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = ev.id;
            if id == *i_copiar.id() {
                pausado = false;
                i_pausa.set_text(t(T::MenuPausar));
                let _ = tx.send(Orden::CopiarAhora);
            } else if id == *i_pausa.id() {
                alternar_pausa(&mut pausado, &i_pausa);
            } else if id == *i_verificar.id() {
                let _ = tx.send(Orden::Verificar);
            } else if id == *i_liberar.id() {
                if let Some(p) = panel.as_mut() {
                    p.ocultar();
                }
                abrir_asistente(&mut asistente, destino_ev, &liberar);
            } else if id == *i_login.id() {
                lanzar_login(&tx, &cancelar, pausado);
            } else if id == *i_abrir.id() {
                abrir_carpeta();
            } else if id == *i_destino.id() {
                if let Some(p) = rfd::FileDialog::new().set_title(t(T::DlgNuevoDestino)).pick_folder() {
                    let mut c = Config::cargar().unwrap_or_default();
                    c.destino = Some(p);
                    let _ = c.guardar();
                    cancelar.store(true, Ordering::SeqCst);
                    let _ = tx.send(Orden::CopiarAhora);
                }
            } else if id == *i_auto.id() {
                let _ = autoarranque::fijar(i_auto.is_checked());
            } else if id == *i_salir.id() {
                cancelar.store(true, Ordering::SeqCst);
                let _ = tx.send(Orden::Salir);
                tray.take();
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}
