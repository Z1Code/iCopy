//! Una ronda: listar iCloud → registrar → copiar y verificar lo pendiente.
use crate::bitacora;
use crate::descarga::{self, ErrorDescarga};
use crate::disco;
use crate::fuente::{ErrorFuente, FuenteIcloud};
use crate::layout;
use crate::registro::{Registro, TrabajoParte};
use crate::verificar::{self, FalloVerificacion};
use reqwest::blocking::Client;
use std::collections::VecDeque;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinRonda {
    Completa,
    Relistar,
    SesionCaducada,
    SinEspacio,
    DestinoAusente,
    Cancelada,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoRonda {
    pub fin: FinRonda,
    pub listados: usize,
    pub verificadas: usize,
    pub fallidas: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Avance {
    Listando { vistos: usize },
    Copiando { hechas: usize, total: usize, actual: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpcionesRonda {
    pub concurrencia: usize,
    pub mes: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Listado {
    Completo(usize),
    Interrumpido(FinRonda, usize),
}

#[derive(Debug)]
pub enum ErrorParte {
    Descarga(ErrorDescarga),
    Verificacion(FalloVerificacion),
    Ruta(String),
    Disco(String),
    Registro(String),
    Cancelada,
}

impl fmt::Display for ErrorParte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorParte::Descarga(e) => write!(f, "{e}"),
            ErrorParte::Verificacion(e) => write!(f, "verificación: {e}"),
            ErrorParte::Ruta(m) => write!(f, "ruta: {m}"),
            ErrorParte::Disco(m) => write!(f, "disco: {m}"),
            ErrorParte::Registro(m) => write!(f, "registro: {m}"),
            ErrorParte::Cancelada => write!(f, "cancelada"),
        }
    }
}

pub fn ahora() -> i64 {
    chrono::Utc::now().timestamp()
}

fn resultado(fin: FinRonda, listados: usize) -> ResultadoRonda {
    ResultadoRonda { fin, listados, verificadas: 0, fallidas: 0 }
}

/// Lista iCloud y lo vuelca al registro. Solo guarda la marca si el listado terminó bien.
pub fn listar_en_registro(
    registro: &Mutex<Registro>,
    fuente: &mut dyn FuenteIcloud,
    cancelar: &AtomicBool,
    avance: &(dyn Fn(Avance) + Sync),
) -> anyhow::Result<Listado> {
    let marca = ahora();
    let mut error_registro: Option<anyhow::Error> = None;
    let mut vistos = 0usize;
    let r = fuente.listar(&mut |asset| {
        if cancelar.load(Ordering::Relaxed) {
            return false;
        }
        if let Err(e) = registro.lock().unwrap().upsert_asset(&asset, marca) {
            error_registro = Some(e);
            return false;
        }
        vistos += 1;
        if vistos % 25 == 0 {
            avance(Avance::Listando { vistos });
        }
        true
    });
    if let Some(e) = error_registro {
        return Err(e);
    }
    match r {
        Ok(_) => {
            registro.lock().unwrap().guardar_marca_lista(marca)?;
            avance(Avance::Listando { vistos });
            Ok(Listado::Completo(vistos))
        }
        Err(ErrorFuente::SesionCaducada) => Ok(Listado::Interrumpido(FinRonda::SesionCaducada, vistos)),
        Err(ErrorFuente::Cancelado) => Ok(Listado::Interrumpido(FinRonda::Cancelada, vistos)),
        Err(e) => Err(anyhow::anyhow!("listando iCloud: {e}")),
    }
}

pub fn ejecutar_ronda(
    destino: &Path,
    registro: &Mutex<Registro>,
    fuente: &mut dyn FuenteIcloud,
    http: &Client,
    op: &OpcionesRonda,
    cancelar: &AtomicBool,
    avance: &(dyn Fn(Avance) + Sync),
) -> anyhow::Result<ResultadoRonda> {
    if !destino.is_dir() {
        return Ok(resultado(FinRonda::DestinoAusente, 0));
    }
    let dir_datos = destino.join(".icopia");
    let listados = match listar_en_registro(registro, fuente, cancelar, avance)? {
        Listado::Completo(n) => n,
        Listado::Interrumpido(fin, n) => return Ok(resultado(fin, n)),
    };

    let trabajos = registro.lock().unwrap().partes_pendientes(ahora(), op.mes.as_deref())?;
    let total = trabajos.len();
    let cola = Mutex::new(VecDeque::from(trabajos));
    let hechas = AtomicUsize::new(0);
    let verificadas = AtomicUsize::new(0);
    let fallidas = AtomicUsize::new(0);
    let relistar = AtomicBool::new(false);
    let sin_espacio = AtomicBool::new(false);

    std::thread::scope(|s| {
        for _ in 0..op.concurrencia.max(1) {
            s.spawn(|| loop {
                // Un enlace caducado solo afecta a su parte: se termina la cola y luego se pide relistar.
                if cancelar.load(Ordering::Relaxed) || sin_espacio.load(Ordering::Relaxed) {
                    break;
                }
                let Some(t) = cola.lock().unwrap().pop_front() else { break };
                if !disco::cabe(destino, t.tamano) {
                    sin_espacio.store(true, Ordering::Relaxed);
                    break;
                }
                let actual = format!("{} ({})", t.nombre_base, t.tipo.como_str());
                avance(Avance::Copiando { hechas: hechas.load(Ordering::Relaxed), total, actual: actual.clone() });
                match procesar_parte(destino, registro, http, &t, cancelar) {
                    Ok(sha) => {
                        if let Err(e) = registro.lock().unwrap().marcar_verificada(&t.asset_id, t.tipo, &sha) {
                            bitacora::escribir(&dir_datos, &format!("ERROR registro {}: {e:#}", t.asset_id));
                        }
                        verificadas.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(ErrorParte::Cancelada) => break,
                    Err(e) => {
                        if matches!(e, ErrorParte::Descarga(ErrorDescarga::UrlCaducada(_))) {
                            relistar.store(true, Ordering::Relaxed);
                        }
                        let msg = e.to_string();
                        bitacora::escribir(&dir_datos, &format!("FALLO {} {} ({}): {msg}", t.nombre_base, t.tipo.como_str(), t.asset_id));
                        let _ = registro.lock().unwrap().registrar_fallo(&t.asset_id, t.tipo, &msg, ahora());
                        fallidas.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let n = hechas.fetch_add(1, Ordering::Relaxed) + 1;
                avance(Avance::Copiando { hechas: n, total, actual });
            });
        }
    });

    let fin = if cancelar.load(Ordering::Relaxed) {
        FinRonda::Cancelada
    } else if sin_espacio.load(Ordering::Relaxed) {
        FinRonda::SinEspacio
    } else if relistar.load(Ordering::Relaxed) {
        FinRonda::Relistar
    } else {
        FinRonda::Completa
    };
    Ok(ResultadoRonda {
        fin,
        listados,
        verificadas: verificadas.load(Ordering::Relaxed),
        fallidas: fallidas.load(Ordering::Relaxed),
    })
}

fn procesar_parte(destino: &Path, registro: &Mutex<Registro>, http: &Client, t: &TrabajoParte, cancelar: &AtomicBool) -> Result<String, ErrorParte> {
    let rel = match &t.ruta {
        Some(r) => r.clone(),
        None => {
            let mut reg = registro.lock().unwrap();
            let rel = {
                let consulta = &*reg;
                layout::elegir_ruta(&t.nombre_base, &t.asset_id, &t.fecha, t.tipo, &t.extension, &mut |clave| {
                    let en_disco = destino.join(clave);
                    consulta.ruta_ocupada(clave).unwrap_or(true) || en_disco.exists() || descarga::ruta_part(&en_disco).exists()
                })
                .map_err(|e| ErrorParte::Ruta(e.to_string()))?
            };
            reg.asignar_ruta(&t.asset_id, t.tipo, &rel).map_err(|e| ErrorParte::Ruta(e.to_string()))?;
            rel
        }
    };
    let final_ = destino.join(&rel);
    if let Some(p) = final_.parent() {
        std::fs::create_dir_all(p).map_err(|e| ErrorParte::Disco(e.to_string()))?;
    }
    // Recuperación tras un corte entre "renombrar" y "anotar como verificado": solo se adopta
    // el archivo final si es exactamente el `.part` que se verificó (mismo SHA-256 anotado).
    if let Some(esperado) = &t.sha_pendiente {
        if final_.exists() {
            if let Ok(sha) = verificar::verificar_archivo(&final_, t.tamano, &t.extension) {
                if &sha == esperado {
                    return Ok(sha);
                }
            }
        }
    }
    let part = descarga::ruta_part(&final_);
    let identidad = format!("{}:{}", t.tamano, t.checksum);
    descarga::descargar(http, &t.url, &part, t.tamano, &identidad, cancelar).map_err(|e| match e {
        ErrorDescarga::Cancelada => ErrorParte::Cancelada,
        otro => ErrorParte::Descarga(otro),
    })?;
    match verificar::verificar_archivo(&part, t.tamano, &t.extension) {
        Ok(sha) => {
            registro
                .lock()
                .unwrap()
                .anotar_sha_pendiente(&t.asset_id, t.tipo, &sha)
                .map_err(|e| ErrorParte::Registro(e.to_string()))?;
            std::fs::rename(&part, &final_).map_err(|e| ErrorParte::Disco(e.to_string()))?;
            let _ = std::fs::remove_file(descarga::ruta_meta(&part));
            Ok(sha)
        }
        Err(f) => {
            descarga::borrar_part(&part);
            Err(ErrorParte::Verificacion(f))
        }
    }
}
