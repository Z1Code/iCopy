//! iCopy en la barra de menús de macOS. El estado de la copia se ve en el propio menú (sin el panel
//! flotante de Windows). «Liberar espacio en iCloud» y las actualizaciones llegan en próximas tareas.
use icopia::bandeja::estado::{EstadoUi, EventoUi, Fase};
use icopia::bandeja::hilo_motor::{self, Orden};
use icopia::bandeja::{autoarranque, avisos, icono};
use icopia::config::{self, Config};
use icopia::fuente::FuenteIcloud;
use icopia::i18n::{t, T};
use icopia::liberar::ControlBorrado;
use icopia::puente::Puente;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

enum Ev {
    Ui(EventoUi),
    /// La ventana de inicio de sesión se cerró (con éxito o no).
    LoginCerrado,
}

/// Icono plantilla (negro sobre transparente) a 2x del tamaño de la barra de menús (22 pt).
fn icono_barra() -> Icon {
    let n = 44;
    Icon::from_rgba(icono::plantilla(n), n, n).expect("icono válido")
}

/// Abre la ventana de inicio de sesión pausando la copia mientras está abierta.
fn lanzar_login(tx: &mpsc::Sender<Orden>, cancelar: &AtomicBool, ya_pausado: bool, proxy: tao::event_loop::EventLoopProxy<Ev>) {
    if !ya_pausado {
        // Primero la orden y luego `cancelar`: así el hilo del motor no puede perder la pausa.
        let _ = tx.send(Orden::Pausar);
        cancelar.store(true, Ordering::SeqCst);
    }
    let tx = tx.clone();
    std::thread::spawn(move || {
        let _ = std::process::Command::new(icopia::plataforma::exe_junto("icopia-login")).status();
        if !ya_pausado {
            let _ = tx.send(Orden::Reanudar);
        }
        let _ = proxy.send_event(Ev::LoginCerrado);
    });
}

/// Segunda línea del menú mientras se copia: «64 % · 182 de 285 GB».
fn detalle(e: &EstadoUi) -> Option<String> {
    let copiando = matches!(e.fase, Fase::Listando { .. } | Fase::Copiando { .. } | Fase::Verificando { .. });
    let p = e.porcentaje()?;
    copiando.then(|| {
        let gb = |b: u64| b as f64 / 1e9;
        if e.detalle.bytes_total > 0 {
            format!("{p} % · {:.0} de {:.0} GB", gb(e.detalle.bytes_verificados), gb(e.detalle.bytes_total))
        } else {
            format!("{p} %")
        }
    })
}

pub fn main() {
    let Some(_instancia) = icopia::plataforma::instancia_unica("bandeja") else {
        return;
    };
    let mut cfg = Config::cargar().unwrap_or_default();
    if cfg.destino.is_none() {
        if let Some(p) = rfd::FileDialog::new().set_title(t(T::DlgDestino)).pick_folder() {
            cfg.destino = Some(p);
            let _ = cfg.guardar();
        }
    }

    let mut event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    // App de barra de menús: sin icono en el Dock ni menú de aplicación.
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = Mutex::new(event_loop.create_proxy());
    let proxy_login = event_loop.create_proxy();

    let (tx, rx) = mpsc::channel::<Orden>();
    let cancelar = Arc::new(AtomicBool::new(false));
    hilo_motor::lanzar_con(
        rx,
        cancelar.clone(),
        Arc::new(ControlBorrado::default()),
        Box::new(|| Config::cargar().unwrap_or_default()),
        Box::new(|cfg: &Config| {
            let id = cfg.apple_id.as_deref().ok_or_else(|| anyhow::anyhow!("sin Apple ID"))?;
            Ok(Box::new(Puente::iniciar(id, &config::dir_sesion(), &config::log_puente())?) as Box<dyn FuenteIcloud>)
        }),
        Box::new(move |e| {
            let _ = proxy.lock().unwrap().send_event(Ev::Ui(e));
        }),
    );

    let i_version = MenuItem::new(format!("iCopy {}", env!("CARGO_PKG_VERSION")), false, None);
    let i_estado = MenuItem::new(t(T::MenuIniciando), false, None);
    let i_detalle = MenuItem::new("", false, None);
    let i_copiar = MenuItem::new(t(T::MenuCopiar), true, None);
    let i_pausa = MenuItem::new(t(T::MenuPausar), true, None);
    let i_verificar = MenuItem::new(t(T::MenuVerificar), true, None);
    let i_abrir = MenuItem::new(t(T::MenuAbrir), true, None);
    let i_destino = MenuItem::new(t(T::MenuDestino), true, None);
    let i_login = MenuItem::new(t(T::MenuSesion), true, None);
    let i_auto = CheckMenuItem::new(t(T::MenuAutoMac), true, autoarranque::activo(), None);
    let i_salir = MenuItem::new(t(T::MenuSalir), true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &i_version,
        &PredefinedMenuItem::separator(),
        &i_estado,
        &i_detalle,
        &PredefinedMenuItem::separator(),
        &i_copiar,
        &i_pausa,
        &i_verificar,
        &PredefinedMenuItem::separator(),
        &i_abrir,
        &i_destino,
        &i_login,
        &i_auto,
        &PredefinedMenuItem::separator(),
        &i_salir,
    ])
    .expect("menú");

    let mut tray: Option<TrayIcon> = None;
    let mut estado = EstadoUi::sin_datos(Fase::Inactiva);
    let mut pausado = false;
    // Sin Apple ID se pide iniciar sesión ya; con uno, si la sesión caducó lo dice el motor.
    let mut login_abierto = cfg.apple_id.is_none();
    if login_abierto {
        lanzar_login(&tx, &cancelar, false, proxy_login.clone());
    }
    let mut ultimo_tooltip = Instant::now();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200));
        match event {
            Event::NewEvents(StartCause::Init) => {
                // En macOS el icono se crea con el bucle ya en marcha.
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_menu_on_left_click(true)
                        .with_icon(icono_barra())
                        .with_icon_as_template(true)
                        .with_tooltip("iCopy")
                        .build()
                        .expect("icono de la barra de menús"),
                );
            }
            Event::UserEvent(Ev::Ui(EventoUi::Estado(e))) => {
                i_estado.set_text(e.texto());
                let d = detalle(&e);
                i_detalle.set_text(d.clone().unwrap_or_default());
                i_detalle.set_enabled(false);
                match e.fase {
                    Fase::SesionCaducada if !login_abierto => {
                        login_abierto = true;
                        lanzar_login(&tx, &cancelar, pausado, proxy_login.clone());
                    }
                    // Solo listando o copiando hay prueba de que la sesión vuelve a valer.
                    Fase::Listando { .. } | Fase::Copiando { .. } => login_abierto = false,
                    _ => {}
                }
                estado = e;
                if ultimo_tooltip.elapsed() >= Duration::from_secs(1) {
                    ultimo_tooltip = Instant::now();
                    if let Some(tr) = &tray {
                        let _ = tr.set_tooltip(Some(format!("iCopy — {}", estado.texto())));
                    }
                }
            }
            Event::UserEvent(Ev::Ui(EventoUi::Aviso { titulo, texto })) => avisos::mostrar(&titulo, &texto),
            Event::UserEvent(Ev::LoginCerrado) => {
                // Si la sesión sigue caducada, la próxima ronda lo dirá; no se reabre en bucle.
            }
            _ => {}
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = ev.id;
            if id == *i_copiar.id() {
                pausado = false;
                i_pausa.set_text(t(T::MenuPausar));
                let _ = tx.send(Orden::CopiarAhora);
            } else if id == *i_pausa.id() {
                pausado = !pausado;
                if pausado {
                    i_pausa.set_text(t(T::MenuReanudar));
                    let _ = tx.send(Orden::Pausar);
                    cancelar.store(true, Ordering::SeqCst);
                } else {
                    i_pausa.set_text(t(T::MenuPausar));
                    let _ = tx.send(Orden::Reanudar);
                }
            } else if id == *i_verificar.id() {
                let _ = tx.send(Orden::Verificar);
            } else if id == *i_abrir.id() {
                if let Some(d) = Config::cargar().ok().and_then(|c| c.destino) {
                    icopia::plataforma::abrir(&d);
                }
            } else if id == *i_destino.id() {
                if let Some(p) = rfd::FileDialog::new().set_title(t(T::DlgNuevoDestino)).pick_folder() {
                    let mut c = Config::cargar().unwrap_or_default();
                    c.destino = Some(p);
                    let _ = c.guardar();
                    cancelar.store(true, Ordering::SeqCst);
                    let _ = tx.send(Orden::CopiarAhora);
                }
            } else if id == *i_login.id() {
                lanzar_login(&tx, &cancelar, pausado, proxy_login.clone());
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
