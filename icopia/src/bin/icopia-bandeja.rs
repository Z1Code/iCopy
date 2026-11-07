#![windows_subsystem = "windows"]
use anyhow::Context;
use icopia::bandeja::estado::{EstadoUi, EventoUi, Fase};
use icopia::bandeja::panel_ventana::{Accion, Panel};
use icopia::bandeja::hilo_motor::{self, Orden};
use icopia::bandeja::{autoarranque, avisos, icono};
use icopia::config::{self, Config};
use icopia::fuente::FuenteIcloud;
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

/// Abre `icopia.exe <sub> --pausa` en una consola nueva, pausando la copia mientras tanto.
fn lanzar_consola(sub: &str, tx: &mpsc::Sender<Orden>, cancelar: &AtomicBool, ya_pausado: bool) {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(exe_junto("icopia.exe"));
    cmd.arg(sub).arg("--pausa").creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    lanzar_pausando(cmd, tx, cancelar, ya_pausado);
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
        if let Some(p) = rfd::FileDialog::new().set_title("icopia: elige dónde guardar las fotos").pick_folder() {
            cfg.destino = Some(p);
            let _ = cfg.guardar();
        }
    }

    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = Mutex::new(event_loop.create_proxy());
    let proxy_bandeja = Mutex::new(event_loop.create_proxy());
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy_bandeja.lock().unwrap().send_event(Ev::Bandeja(e));
    }));
    let (tx, rx) = mpsc::channel::<Orden>();
    let cancelar = Arc::new(AtomicBool::new(false));
    hilo_motor::lanzar(
        rx,
        cancelar.clone(),
        Box::new(|| Config::cargar().unwrap_or_default()),
        Box::new(|cfg: &Config| {
            let id = cfg.apple_id.as_deref().context("sin Apple ID")?;
            Ok(Box::new(Puente::iniciar(id, &config::dir_sesion(), &config::log_puente())?) as Box<dyn FuenteIcloud>)
        }),
        Box::new(move |e| {
            let _ = proxy.lock().unwrap().send_event(Ev::Ui(e));
        }),
    );
    if cfg.apple_id.is_none() {
        lanzar_login(&tx, &cancelar, false);
    }

    let i_estado = MenuItem::new("icopia — iniciando…", false, None);
    let i_copiar = MenuItem::new("Copiar ahora", true, None);
    let i_pausa = MenuItem::new("Pausar", true, None);
    let i_verificar = MenuItem::new("Verificar todo", true, None);
    let i_liberar = MenuItem::new("Liberar espacio en iCloud…", true, None);
    let i_abrir = MenuItem::new("Abrir carpeta de fotos", true, None);
    let i_destino = MenuItem::new("Cambiar destino…", true, None);
    let i_login = MenuItem::new("Iniciar sesión en iCloud…", true, None);
    let i_auto = CheckMenuItem::new("Iniciar con Windows", true, autoarranque::activo(), None);
    let i_salir = MenuItem::new("Salir", true, None);
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

    let mut menu_pendiente = Some(menu);
    let mut tray: Option<TrayIcon> = None;
    let mut panel: Option<Panel> = None;
    let mut estado = EstadoUi::sin_datos(Fase::Inactiva);
    let mut pausado = false;
    let mut ultimo_refresco = Instant::now();

    let (tx_pausa, cancelar_pausa) = (tx.clone(), cancelar.clone());
    let alternar_pausa = move |pausado: &mut bool, i_pausa: &MenuItem| {
        if *pausado {
            *pausado = false;
            i_pausa.set_text("Pausar");
            let _ = tx_pausa.send(Orden::Reanudar);
        } else {
            *pausado = true;
            i_pausa.set_text("Reanudar");
            let _ = tx_pausa.send(Orden::Pausar);
            cancelar_pausa.store(true, Ordering::SeqCst);
        }
    };
    let abrir_carpeta = || {
        if let Some(d) = Config::cargar().ok().and_then(|c| c.destino) {
            let _ = std::process::Command::new("explorer").arg(d).spawn();
        }
    };

    event_loop.run(move |event, destino_ev, control_flow| {
        let mut espera = Duration::from_millis(200);
        match event {
            Event::NewEvents(StartCause::Init) => {
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu_pendiente.take().expect("menú")))
                        .with_menu_on_left_click(false)
                        .with_tooltip("icopia")
                        .with_icon(icono_de(icopia::bandeja::estado::Color::Gris))
                        .build()
                        .expect("icono de bandeja"),
                );
                panel = Some(Panel::nuevo(destino_ev));
            }
            Event::UserEvent(Ev::Ui(EventoUi::Estado(e))) => {
                let t = e.texto();
                i_estado.set_text(&t);
                if let Some(tr) = &tray {
                    let _ = tr.set_tooltip(Some(format!("icopia — {t}")));
                    if e.color() != estado.color() || estado.fase == Fase::Inactiva && estado.total == 0 {
                        let _ = tr.set_icon(Some(icono_de(e.color())));
                    }
                }
                estado = e;
            }
            Event::UserEvent(Ev::Ui(EventoUi::Aviso { titulo, texto })) => avisos::mostrar(&titulo, &texto),
            Event::UserEvent(Ev::Bandeja(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. })) => {
                if let Some(p) = panel.as_mut() {
                    let centro = (rect.position.x as i32 + rect.size.width as i32 / 2, rect.position.y as i32 + rect.size.height as i32 / 2);
                    p.alternar(centro, &estado);
                }
            }
            Event::WindowEvent { window_id, event, .. } if panel.as_ref().is_some_and(|p| p.id() == window_id) => {
                let p = panel.as_mut().expect("panel");
                match p.evento(&event, &estado) {
                    Some(Accion::Pausa) => {
                        alternar_pausa(&mut pausado, &i_pausa);
                        p.refrescar(&estado);
                    }
                    Some(Accion::AbrirCarpeta) => {
                        p.ocultar();
                        abrir_carpeta();
                    }
                    None => {}
                }
                if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                    p.refrescar(&estado);
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
        *control_flow = ControlFlow::WaitUntil(Instant::now() + espera);
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = ev.id;
            if id == *i_copiar.id() {
                pausado = false;
                i_pausa.set_text("Pausar");
                let _ = tx.send(Orden::CopiarAhora);
            } else if id == *i_pausa.id() {
                alternar_pausa(&mut pausado, &i_pausa);
            } else if id == *i_verificar.id() {
                let _ = tx.send(Orden::Verificar);
            } else if id == *i_liberar.id() {
                lanzar_consola("liberar", &tx, &cancelar, pausado);
            } else if id == *i_login.id() {
                lanzar_login(&tx, &cancelar, pausado);
            } else if id == *i_abrir.id() {
                abrir_carpeta();
            } else if id == *i_destino.id() {
                if let Some(p) = rfd::FileDialog::new().set_title("icopia: nueva carpeta destino").pick_folder() {
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
