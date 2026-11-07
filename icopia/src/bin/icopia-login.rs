#![windows_subsystem = "windows"]
//! Ventana pequeña para iniciar sesión en iCloud (lo mismo que `icopia login`, sin consola).
//! Sale con código 0 si la sesión quedó iniciada; 1 si se cierra o cancela.
use icopia::config::{self, Config};
use icopia::login::{normalizar_codigo, FlujoLogin, Paso};
use icopia::puente::Puente;
use native_windows_gui as nwg;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

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

#[derive(Clone, Copy, PartialEq)]
enum Etapa {
    Credenciales,
    Codigo,
}

#[derive(Default)]
struct Ui {
    ventana: nwg::Window,
    l_id: nwg::Label,
    t_id: nwg::TextInput,
    l_pw: nwg::Label,
    t_pw: nwg::TextInput,
    b_entrar: nwg::Button,
    l_codigo: nwg::Label,
    t_codigo: nwg::TextInput,
    b_verificar: nwg::Button,
    estado: nwg::Label,
    aviso: nwg::Notice,
}

impl Ui {
    fn mostrar_etapa(&self, e: Etapa) {
        let cred = e == Etapa::Credenciales;
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
    }

    fn habilitar(&self, si: bool) {
        for t in [&self.t_id, &self.t_pw, &self.t_codigo] {
            t.set_enabled(si);
        }
        self.b_entrar.set_enabled(si);
        self.b_verificar.set_enabled(si);
    }
}

fn construir(ui: &mut Ui, apple_id: &str) -> Result<(), nwg::NwgError> {
    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .size((380, 230))
        .center(true)
        .title("icopia: iniciar sesión en iCloud")
        .build(&mut ui.ventana)?;
    let v = &ui.ventana;
    nwg::Label::builder().text("Apple ID").position((20, 15)).size((340, 22)).parent(v).build(&mut ui.l_id)?;
    nwg::TextInput::builder().text(apple_id).position((20, 38)).size((340, 26)).parent(v).build(&mut ui.t_id)?;
    nwg::Label::builder().text("Contraseña").position((20, 72)).size((340, 22)).parent(v).build(&mut ui.l_pw)?;
    nwg::TextInput::builder().password(Some('●')).position((20, 95)).size((340, 26)).parent(v).build(&mut ui.t_pw)?;
    nwg::Button::builder().text("Entrar").position((250, 135)).size((110, 32)).parent(v).build(&mut ui.b_entrar)?;
    nwg::Label::builder()
        .text("Código de 6 dígitos que apareció en tu iPhone")
        .position((20, 15))
        .size((340, 22))
        .parent(v)
        .build(&mut ui.l_codigo)?;
    nwg::TextInput::builder().limit(7).position((20, 38)).size((140, 26)).parent(v).build(&mut ui.t_codigo)?;
    nwg::Button::builder().text("Verificar").position((250, 135)).size((110, 32)).parent(v).build(&mut ui.b_verificar)?;
    nwg::Label::builder().text("").position((20, 175)).size((340, 44)).parent(v).build(&mut ui.estado)?;
    nwg::Notice::builder().parent(v).build(&mut ui.aviso)?;
    Ok(())
}

fn main() {
    let apple_id = Config::cargar().ok().and_then(|c| c.apple_id).unwrap_or_default();
    nwg::init().expect("no pude iniciar la interfaz");
    let mut fuente = nwg::Font::default();
    if nwg::Font::builder().family("Segoe UI").size(17).build(&mut fuente).is_ok() {
        nwg::Font::set_global_default(Some(fuente));
    }
    let mut ui = Ui::default();
    construir(&mut ui, &apple_id).expect("no pude crear la ventana");
    let ui = Rc::new(ui);
    ui.mostrar_etapa(Etapa::Credenciales);
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
                        ui.estado.set_text("Escribe tu Apple ID.");
                        ui.t_id.set_focus();
                        return;
                    }
                    if password.is_empty() {
                        ui.estado.set_text("Escribe la contraseña.");
                        ui.t_pw.set_focus();
                        return;
                    }
                    ui.estado.set_text("Conectando con Apple…");
                    Orden::Entrar { apple_id, password }
                }
                Etapa::Codigo => {
                    let Some(c) = normalizar_codigo(&ui.t_codigo.text()) else {
                        ui.estado.set_text("El código son 6 dígitos.");
                        ui.t_codigo.set_focus();
                        return;
                    };
                    ui.estado.set_text("Verificando el código…");
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
                        ui.estado.set_text("Sesión iniciada. Dura unos 2 meses; icopia te avisará cuando caduque.");
                        continue;
                    }
                    Paso::PideCodigo => {
                        etapa.set(Etapa::Codigo);
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.estado.set_text("Apple ha enviado un código a tu iPhone.");
                    }
                    Paso::Reintentar(m) => {
                        ui.estado.set_text(&m);
                        match etapa.get() {
                            Etapa::Credenciales => ui.t_pw.set_text(""),
                            Etapa::Codigo => ui.t_codigo.set_text(""),
                        }
                    }
                    Paso::Fallo(m) => {
                        etapa.set(Etapa::Credenciales);
                        ui.t_pw.set_text("");
                        ui.t_codigo.set_text("");
                        ui.estado.set_text(&m);
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
            E::OnKeyEnter => enviar(),
            E::OnButtonClick if handle == ui_ev.b_entrar.handle || handle == ui_ev.b_verificar.handle => enviar(),
            E::OnNotice if handle == ui_ev.aviso.handle => recibir(),
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&manejador);
    std::process::exit(if exito.get() { 0 } else { 1 });
}
