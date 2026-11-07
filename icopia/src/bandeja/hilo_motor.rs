//! Hilo que ejecuta rondas periódicas y obedece órdenes del menú.
use crate::auditoria;
use crate::bandeja::estado::{EstadoUi, EventoUi, Fase};
use crate::bitacora;
use crate::cerrojo;
use crate::config::Config;
use crate::descarga;
use crate::energia::SinSuspension;
use crate::fuente::FuenteIcloud;
use crate::motor::{self, Avance, FinRonda, OpcionesRonda};
use crate::planificador;
use crate::registro::Registro;
use reqwest::blocking::Client;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Orden {
    CopiarAhora,
    Pausar,
    Reanudar,
    Verificar,
    Salir,
}

pub type CargarConfig = Box<dyn Fn() -> Config + Send>;
pub type AbrirFuente = Box<dyn Fn(&Config) -> anyhow::Result<Box<dyn FuenteIcloud>> + Send>;
pub type Emitir = Box<dyn Fn(EventoUi) + Send + Sync>;

pub fn lanzar(rx: Receiver<Orden>, cancelar: Arc<AtomicBool>, cargar_config: CargarConfig, abrir_fuente: AbrirFuente, emitir: Emitir) -> JoinHandle<()> {
    std::thread::spawn(move || bucle(rx, &cancelar, &*cargar_config, &*abrir_fuente, &*emitir))
}

fn aviso(emitir: &(dyn Fn(EventoUi) + Sync), texto: impl Into<String>) {
    emitir(EventoUi::Aviso { titulo: "icopia".into(), texto: texto.into() });
}

fn abrir_registro(destino: &Path) -> anyhow::Result<Registro> {
    Registro::abrir(&destino.join(".icopia").join("registro.db"))
}

fn emitir_resumen(cfg: &Config, fase: Fase, emitir: &(dyn Fn(EventoUi) + Sync)) {
    let progreso = cfg
        .destino
        .as_deref()
        .filter(|d| d.is_dir())
        .and_then(|d| abrir_registro(d).ok())
        .and_then(|r| r.progreso().ok())
        .unwrap_or_default();
    emitir(EventoUi::Estado(EstadoUi::con_progreso(fase, &progreso)));
}

fn bucle(
    rx: Receiver<Orden>,
    cancelar: &AtomicBool,
    cargar_config: &dyn Fn() -> Config,
    abrir_fuente: &dyn Fn(&Config) -> anyhow::Result<Box<dyn FuenteIcloud>>,
    emitir: &(dyn Fn(EventoUi) + Sync),
) {
    let http = descarga::cliente();
    let mut pausado = false;
    let mut espera: Option<Duration> = Some(Duration::ZERO);
    let mut ultimo_fin: Option<FinRonda> = None;
    emitir_resumen(&cargar_config(), Fase::Inactiva, emitir);
    loop {
        let orden = match espera {
            Some(d) => match rx.recv_timeout(d) {
                Ok(o) => Some(o),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(o) => Some(o),
                Err(_) => return,
            },
        };
        let mut forzar = false;
        match orden {
            Some(Orden::Salir) => return,
            Some(Orden::Pausar) => {
                pausado = true;
                espera = None;
                emitir_resumen(&cargar_config(), Fase::Pausada, emitir);
                continue;
            }
            Some(Orden::Reanudar) | Some(Orden::CopiarAhora) => {
                pausado = false;
                forzar = true;
            }
            Some(Orden::Verificar) => {
                verificar(&cargar_config(), cancelar, emitir);
                continue;
            }
            None => {}
        }
        if pausado {
            espera = None;
            continue;
        }
        // Una orden llegada justo antes de borrar `cancelar` (p. ej. Pausar) no debe perderse:
        // tras borrarlo se atiende lo que haya en la cola, hasta que esté vacía.
        loop {
            cancelar.store(false, Ordering::SeqCst);
            let mut hubo = false;
            loop {
                match rx.try_recv() {
                    Ok(Orden::Salir) | Err(TryRecvError::Disconnected) => return,
                    Ok(Orden::Pausar) => pausado = true,
                    Ok(Orden::Reanudar) | Ok(Orden::CopiarAhora) => {
                        pausado = false;
                        forzar = true;
                    }
                    Ok(Orden::Verificar) => verificar(&cargar_config(), cancelar, emitir),
                    Err(TryRecvError::Empty) => break,
                }
                hubo = true;
            }
            if !hubo || pausado {
                break;
            }
        }
        if pausado {
            espera = None;
            emitir_resumen(&cargar_config(), Fase::Pausada, emitir);
            continue;
        }
        espera = una_ronda(&cargar_config(), &http, cancelar, abrir_fuente, emitir, forzar, &mut ultimo_fin);
    }
}

fn una_ronda(
    cfg: &Config,
    http: &Client,
    cancelar: &AtomicBool,
    abrir_fuente: &dyn Fn(&Config) -> anyhow::Result<Box<dyn FuenteIcloud>>,
    emitir: &(dyn Fn(EventoUi) + Sync),
    forzar: bool,
    ultimo_fin: &mut Option<FinRonda>,
) -> Option<Duration> {
    let Some(destino) = cfg.destino.clone() else {
        emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::SinDestino)));
        return None;
    };
    if cfg.apple_id.is_none() {
        emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::SesionCaducada)));
        return None;
    }
    if !destino.is_dir() {
        emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::DestinoAusente)));
        return Some(Duration::from_secs(600));
    }
    let dir_datos = destino.join(".icopia");
    let _cerrojo = match cerrojo::intentar(&dir_datos) {
        Ok(Some(c)) => c,
        Ok(None) => return Some(Duration::from_secs(60)), // otro proceso (p. ej. "liberar") trabaja
        Err(e) => {
            emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::Error(e.to_string()))));
            return Some(Duration::from_secs(600));
        }
    };
    let registro = match abrir_registro(&destino) {
        Ok(r) => Mutex::new(r),
        Err(e) => {
            emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::Error(e.to_string()))));
            return Some(Duration::from_secs(600));
        }
    };
    if forzar {
        let _ = registro.lock().unwrap().reintentar_ya();
    }
    let _energia = SinSuspension::activar();
    let mut fuente = match abrir_fuente(cfg) {
        Ok(f) => f,
        Err(e) => {
            bitacora::escribir(&dir_datos, &format!("ERROR abriendo el puente: {e:#}"));
            emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::Error(e.to_string()))));
            return Some(Duration::from_secs(900));
        }
    };
    // Como mucho 4 avisos por segundo a la interfaz y una consulta al registro por segundo.
    let inicio = Instant::now() - Duration::from_secs(10);
    let ritmo = Mutex::new((inicio, inicio, registro.lock().unwrap().progreso().unwrap_or_default()));
    let avance = |a: Avance| {
        let (fase, actual, ultimo) = match a {
            Avance::Listando { vistos } => (Fase::Listando { vistos }, None, false),
            Avance::Copiando { hechas, total, actual } => (Fase::Copiando { hechas, total }, Some(actual), hechas == total),
        };
        let mut r = ritmo.lock().unwrap();
        let ahora = Instant::now();
        if !ultimo && ahora.duration_since(r.0) < Duration::from_millis(250) {
            return;
        }
        r.0 = ahora;
        if ahora.duration_since(r.1) >= Duration::from_secs(1) {
            r.1 = ahora;
            if let Ok(p) = registro.lock().unwrap().progreso() {
                r.2 = p;
            }
        }
        let mut e = EstadoUi::con_progreso(fase, &r.2);
        e.detalle.actual = actual;
        drop(r);
        emitir(EventoUi::Estado(e));
    };
    let op = OpcionesRonda { concurrencia: cfg.concurrencia, mes: None };
    let r = motor::ejecutar_ronda(&destino, &registro, fuente.as_mut(), http, &op, cancelar, &avance);
    drop(fuente);
    let reg = registro.lock().unwrap();
    let progreso = reg.progreso().unwrap_or_default();
    let prox = reg.proximo_reintento(None).ok().flatten();
    let estado = |fase: Fase| EventoUi::Estado(EstadoUi::con_progreso(fase, &progreso));
    match r {
        Err(e) => {
            bitacora::escribir(&dir_datos, &format!("ERROR en la ronda: {e:#}"));
            emitir(estado(Fase::Error(e.to_string())));
            Some(Duration::from_secs(900))
        }
        Ok(r) => {
            bitacora::escribir(&dir_datos, &format!("RONDA {:?}: {} verificadas, {} fallidas", r.fin, r.verificadas, r.fallidas));
            if r.verificadas > 0 {
                aviso(emitir, format!("{} archivos nuevos copiados y verificados", r.verificadas));
            }
            let nuevo = *ultimo_fin != Some(r.fin);
            *ultimo_fin = Some(r.fin);
            let fase = match r.fin {
                FinRonda::Completa | FinRonda::Relistar => Fase::Inactiva,
                FinRonda::Cancelada => Fase::Pausada,
                FinRonda::SesionCaducada => {
                    if nuevo {
                        aviso(emitir, "La sesión de iCloud caducó. Menú del icono → Iniciar sesión en iCloud…");
                    }
                    Fase::SesionCaducada
                }
                FinRonda::SinEspacio => {
                    if nuevo {
                        aviso(emitir, format!("No queda espacio en {}", destino.display()));
                    }
                    Fase::SinEspacio
                }
                FinRonda::DestinoAusente => Fase::DestinoAusente,
            };
            emitir(estado(fase));
            planificador::espera_tras_ronda(&r, prox, motor::ahora(), cfg.intervalo_horas)
        }
    }
}

fn verificar(cfg: &Config, cancelar: &AtomicBool, emitir: &(dyn Fn(EventoUi) + Sync)) {
    let Some(destino) = cfg.destino.clone() else { return };
    let dir_datos = destino.join(".icopia");
    let Ok(Some(_cerrojo)) = cerrojo::intentar(&dir_datos) else {
        aviso(emitir, "Hay una copia en marcha; vuelve a intentarlo luego.");
        return;
    };
    let Ok(reg) = abrir_registro(&destino) else { return };
    let reg = Mutex::new(reg);
    cancelar.store(false, Ordering::SeqCst);
    let r = auditoria::verificar_todo(&destino, &reg, cancelar, &|h, t| {
        if h % 50 == 0 || h == t {
            emitir(EventoUi::Estado(EstadoUi::sin_datos(Fase::Verificando { hechas: h, total: t })));
        }
    });
    match r {
        Ok(res) if res.malas.is_empty() => aviso(emitir, format!("Verificación completa: {} archivos en perfecto estado", res.revisadas)),
        Ok(res) => {
            for (ruta, m) in &res.malas {
                bitacora::escribir(&dir_datos, &format!("DAÑADO {ruta}: {m}"));
            }
            aviso(emitir, format!("{} archivos dañados o ausentes; se volverán a descargar", res.malas.len()));
        }
        Err(e) => aviso(emitir, format!("No se pudo verificar: {e}")),
    }
    drop(reg);
    emitir_resumen(cfg, Fase::Inactiva, emitir);
}
