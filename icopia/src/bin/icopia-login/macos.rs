//! Ventana de inicio de sesión de macOS: la página `recursos/web/login.html` en un webview (WKWebView).
//! Mismo comportamiento que la de Windows: comprueba la sesión guardada, pide Apple ID y contraseña,
//! luego el código de 6 dígitos, guarda el Apple ID y sale con 0 si la sesión quedó iniciada (1 si no).
//! La contraseña solo pasa por memoria camino del puente: nunca se registra ni se guarda.
use base64::{engine::general_purpose::STANDARD, Engine};
use icopia::config::{self, Config};
use icopia::i18n::{t, tf, T};
use icopia::login::{comprobar_sesion, EstadoSesion, FlujoLogin, Paso};
use icopia::puente::Puente;
use serde_json::{json, Value};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const HTML: &str = include_str!("../../../recursos/web/login.html");
const ICONO: &[u8] = include_bytes!("../../../assets/icopia-aviso.png");

enum Orden {
    Comprobar(String),
    Entrar { apple_id: String, password: String },
    Codigo(String),
}

enum Respuesta {
    Sesion(EstadoSesion),
    Paso(Paso),
    Cerrar,
}

enum Ev {
    Respuesta(Respuesta),
    Pagina(Value),
}

/// Hace las llamadas (bloqueantes) al puente fuera del hilo de la ventana. Igual que en Windows.
fn trabajador(rx: Receiver<Orden>, proxy: EventLoopProxy<Ev>) {
    let enviar = |r| {
        let _ = proxy.send_event(Ev::Respuesta(r));
    };
    let mut flujo: Option<FlujoLogin<Puente>> = None;
    let mut apple_id = String::new();
    for orden in rx {
        let mut paso = match orden {
            Orden::Comprobar(id) => {
                let r = match Puente::iniciar(&id, &config::dir_sesion(), &config::log_puente()) {
                    Ok(mut p) => comprobar_sesion(&mut p),
                    Err(e) => EstadoSesion::Desconocida(format!("{e:#}")),
                };
                enviar(Respuesta::Sesion(r));
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
        enviar(Respuesta::Paso(paso));
        if hecho {
            std::thread::sleep(Duration::from_millis(1500));
            enviar(Respuesta::Cerrar);
        }
    }
}

/// Lo que se le dice a la página: paso y textos ya traducidos.
#[derive(Default)]
struct Pantalla {
    paso: &'static str,
    titulo: String,
    sub: String,
    info: String,
    error: String,
    apple_id: Option<String>,
    ocupado: bool,
}

impl Pantalla {
    fn comprobando(id: &str) -> Pantalla {
        Pantalla { paso: "comprobando", titulo: t(T::LogTitulo).into(), info: tf(T::LogComprobandoSub, &[("id", &id)]), ..Default::default() }
    }
    fn credenciales(id: &str) -> Pantalla {
        Pantalla { paso: "credenciales", titulo: t(T::LogTitulo).into(), sub: t(T::LogSubCred).into(), apple_id: Some(id.into()), ..Default::default() }
    }
    fn codigo() -> Pantalla {
        Pantalla { paso: "codigo", titulo: t(T::LogCodigo).into(), sub: t(T::LogSubCodigo).into(), ..Default::default() }
    }
    fn ya(id: &str) -> Pantalla {
        Pantalla { paso: "ya", titulo: t(T::LogYa).into(), info: tf(T::LogYaSub, &[("id", &id)]), ..Default::default() }
    }
    fn listo() -> Pantalla {
        Pantalla { paso: "listo", titulo: t(T::LogYa).into(), info: t(T::LogHecho).into(), ..Default::default() }
    }
    fn json(&self) -> String {
        json!({
            "paso": self.paso, "titulo": self.titulo, "sub": self.sub, "info": self.info,
            "error": self.error, "apple_id": self.apple_id, "ocupado": self.ocupado,
        })
        .to_string()
    }
}

/// Textos de etiquetas y botones e icono, antes de que cargue la página.
fn script_inicial() -> String {
    let textos: serde_json::Map<String, Value> = [
        ("LogAppleId", T::LogAppleId),
        ("LogContrasena", T::LogContrasena),
        ("LogCodigo", T::LogCodigo),
        ("LogCancelar", T::LogCancelar),
        ("LogEntrar", T::LogEntrar),
        ("LogVerificar", T::LogVerificar),
        ("LogCambiar", T::LogCambiar),
        ("LogCerrar", T::LogCerrar),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), Value::String(t(v).to_string())))
    .collect();
    format!(
        "window.icopy = {{ textos: {}, icono: \"data:image/png;base64,{}\" }};",
        Value::Object(textos),
        STANDARD.encode(ICONO)
    )
}

pub fn main() {
    let Some(_instancia) = icopia::plataforma::instancia_unica("login") else {
        std::process::exit(1);
    };
    let apple_id_guardado = Config::cargar().ok().and_then(|c| c.apple_id).unwrap_or_default();

    let mut event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    // La lanza una app de barra de menús: que salga al frente y en el Dock mientras está abierta.
    event_loop.set_activation_policy(ActivationPolicy::Regular);
    let ventana = WindowBuilder::new()
        .with_title(t(T::LogVentana))
        .with_inner_size(LogicalSize::new(440.0, 560.0))
        .with_resizable(false)
        .build(&event_loop)
        .expect("ventana de inicio de sesión");

    let proxy_pagina = event_loop.create_proxy();
    let webview = WebViewBuilder::new()
        .with_html(HTML)
        .with_initialization_script(&script_inicial())
        .with_devtools(cfg!(debug_assertions))
        // Solo la página propia: nada de navegar a otros sitios.
        .with_navigation_handler(|url| url.starts_with("about:") || url.starts_with("data:"))
        .with_ipc_handler(move |req| {
            // No registrar nunca el contenido: lleva la contraseña.
            if let Ok(v) = serde_json::from_str::<Value>(req.body()) {
                let _ = proxy_pagina.send_event(Ev::Pagina(v));
            }
        })
        .build(&ventana)
        .expect("webview de inicio de sesión");

    let (tx, rx) = mpsc::channel::<Orden>();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || trabajador(rx, proxy));

    let mostrar = move |p: &Pantalla| {
        let _ = webview.evaluate_script(&format!("window.icopy && window.icopy.estado({})", p.json()));
    };
    let mut pantalla =
        if apple_id_guardado.is_empty() { Pantalla::credenciales("") } else { Pantalla::comprobando(&apple_id_guardado) };
    let mut apple_id = apple_id_guardado.clone();
    let mut exito = false;
    let enviar: Sender<Orden> = tx;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                ventana.set_focus();
                if !apple_id.is_empty() {
                    let _ = enviar.send(Orden::Comprobar(apple_id.clone()));
                }
            }
            // La página terminó de cargar cuando pide algo; el primer estado se envía al cargar.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                std::process::exit(if exito { 0 } else { 1 });
            }
            Event::UserEvent(Ev::Pagina(v)) => match v["accion"].as_str() {
                Some("lista") => mostrar(&pantalla),
                Some("cerrar") => std::process::exit(if exito { 0 } else { 1 }),
                Some("otra_cuenta") => {
                    exito = false;
                    pantalla = Pantalla::credenciales("");
                    mostrar(&pantalla);
                }
                Some("entrar") if !pantalla.ocupado => {
                    let id = v["apple_id"].as_str().unwrap_or("").trim().to_string();
                    let password = v["password"].as_str().unwrap_or("").to_string();
                    let falta = if id.is_empty() {
                        Some(T::LogFaltaId)
                    } else if password.is_empty() {
                        Some(T::LogFaltaPw)
                    } else {
                        None
                    };
                    pantalla = Pantalla::credenciales(&id);
                    match falta {
                        Some(f) => pantalla.error = t(f).into(),
                        None => {
                            apple_id = id.clone();
                            pantalla.ocupado = true;
                            pantalla.info = t(T::LogConectando).into();
                            let _ = enviar.send(Orden::Entrar { apple_id: id, password });
                        }
                    }
                    mostrar(&pantalla);
                }
                Some("codigo") if !pantalla.ocupado => {
                    pantalla = Pantalla::codigo();
                    pantalla.ocupado = true;
                    pantalla.info = t(T::LogVerificando).into();
                    let _ = enviar.send(Orden::Codigo(v["codigo"].as_str().unwrap_or("").to_string()));
                    mostrar(&pantalla);
                }
                _ => {}
            },
            Event::UserEvent(Ev::Respuesta(r)) => {
                match r {
                    Respuesta::Cerrar => std::process::exit(0),
                    Respuesta::Sesion(s) => {
                        if pantalla.paso != "comprobando" {
                            return; // la persona ya eligió otra cosa
                        }
                        pantalla = match s {
                            EstadoSesion::Valida => {
                                exito = true;
                                Pantalla::ya(&apple_id)
                            }
                            EstadoSesion::Caducada => {
                                let mut p = Pantalla::credenciales(&apple_id);
                                p.info = t(T::LogCaducada).into();
                                p
                            }
                            EstadoSesion::Desconocida(m) => {
                                let mut p = Pantalla::credenciales(&apple_id);
                                p.error = tf(T::LogNoComprobada, &[("m", &m)]);
                                p
                            }
                        };
                    }
                    Respuesta::Paso(p) => {
                        let en_codigo = pantalla.paso == "codigo";
                        pantalla = match p {
                            Paso::Hecho => {
                                exito = true;
                                Pantalla::listo()
                            }
                            Paso::PideCodigo => {
                                let mut p = Pantalla::codigo();
                                p.info = t(T::LogCodigoEnviado).into();
                                p
                            }
                            Paso::Reintentar(m) => {
                                let mut p = if en_codigo { Pantalla::codigo() } else { Pantalla::credenciales(&apple_id) };
                                p.error = m;
                                p
                            }
                            Paso::Fallo(m) => {
                                let mut p = Pantalla::credenciales(&apple_id);
                                p.error = m;
                                p
                            }
                        };
                    }
                }
                mostrar(&pantalla);
            }
            _ => {}
        }
    });
}
