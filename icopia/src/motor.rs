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
    /// No se intentaron (o se cortaron) porque su enlace caducaba: siguen pendientes, sin penalizar.
    pub aplazadas: usize,
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
    ResultadoRonda { fin, listados, verificadas: 0, fallidas: 0, aplazadas: 0 }
}

/// No se empieza una descarga si su enlace caduca antes de `ahora + MARGEN + tamaño / RITMO_PRUDENTE`.
const MARGEN_CADUCIDAD: i64 = 120;
const RITMO_PRUDENTE: u64 = 2 * 1024 * 1024;
/// Un rechazo (401/403/404/410) con un listado más reciente que esto es un fallo real del asset.
pub const LISTADO_RECIENTE: i64 = 600;

/// Caducidad (unix) del enlace de iCloud: parámetro `e` de la query. `None` si no lo trae.
pub fn caducidad_url(url: &str) -> Option<i64> {
    let sin_fragmento = url.split('#').next()?;
    let (_, query) = sin_fragmento.split_once('?')?;
    query.split('&').find_map(|kv| kv.strip_prefix("e=")).and_then(|v| v.parse().ok())
}

/// El enlace caducará antes de que dé tiempo a bajar `tamano` bytes a un ritmo prudente.
pub fn caduca_pronto(url: &str, tamano: u64, ahora: i64) -> bool {
    caducidad_url(url).is_some_and(|e| e <= ahora + MARGEN_CADUCIDAD + (tamano / RITMO_PRUDENTE) as i64)
}

/// Tras un rechazo del servidor: ¿se debe a que el enlace caducó (se pide otro sin penalizar)
/// o es un fallo real del asset (cuenta como intento, para no reintentar sin fin)?
/// `fiar_de_e` es falso cuando el reloj no cuadra con las caducidades (ver `ejecutar_ronda`).
pub fn rechazo_por_caducidad(url: &str, fin_listado: i64, ahora: i64, fiar_de_e: bool) -> bool {
    ahora - fin_listado > LISTADO_RECIENTE || (fiar_de_e && caducidad_url(url).is_some_and(|e| e <= ahora + MARGEN_CADUCIDAD))
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
    let fin_listado = ahora();

    let trabajos = registro.lock().unwrap().partes_pendientes(ahora(), op.mes.as_deref())?;
    // Si nada más listar todos los enlaces parecen ya caducados, relistar no arreglaría nada
    // (reloj desajustado o listado más largo que la vida de los enlaces): se ignora `e` y se intenta.
    let mut con_e = trabajos.iter().filter(|t| caducidad_url(&t.url).is_some()).peekable();
    let fiar_de_e = con_e.peek().is_none() || !con_e.all(|t| caduca_pronto(&t.url, t.tamano, fin_listado));
    if !fiar_de_e {
        bitacora::escribir(&dir_datos, "AVISO: todos los enlaces parecen caducados nada más listar (¿reloj desajustado?); se intentan igualmente");
    }
    let total = trabajos.len();
    let cola = Mutex::new(VecDeque::from(trabajos));
    let hechas = AtomicUsize::new(0);
    let verificadas = AtomicUsize::new(0);
    let fallidas = AtomicUsize::new(0);
    let aplazadas = AtomicUsize::new(0);
    let relistar = AtomicBool::new(false);
    // Un enlace caducó a media descarga: lo que queda en la cola espera al próximo listado.
    let parar = AtomicBool::new(false);
    let sin_espacio = AtomicBool::new(false);

    std::thread::scope(|s| {
        for _ in 0..op.concurrencia.max(1) {
            s.spawn(|| loop {
                if cancelar.load(Ordering::Relaxed) || sin_espacio.load(Ordering::Relaxed) || parar.load(Ordering::Relaxed) {
                    break;
                }
                let Some(t) = cola.lock().unwrap().pop_front() else { break };
                // Enlace a punto de caducar: no se descarga ni cuenta como fallo; queda pendiente
                // y se sigue con los demás (los que aún valen se aprovechan antes de relistar).
                if fiar_de_e && caduca_pronto(&t.url, t.tamano, ahora()) {
                    aplazadas.fetch_add(1, Ordering::Relaxed);
                    relistar.store(true, Ordering::Relaxed);
                    hechas.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
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
                    Err(ErrorParte::Descarga(ErrorDescarga::UrlCaducada(_))) if rechazo_por_caducidad(&t.url, fin_listado, ahora(), fiar_de_e) => {
                        aplazadas.fetch_add(1, Ordering::Relaxed);
                        relistar.store(true, Ordering::Relaxed);
                        parar.store(true, Ordering::Relaxed);
                    }
                    Err(e) => {
                        let msg = match e {
                            // Enlace recién pedido y aun así rechazado: fallo real, con otro texto que
                            // "enlace de descarga caducado" para que relistar no lo perdone.
                            ErrorParte::Descarga(ErrorDescarga::UrlCaducada(c)) => {
                                relistar.store(true, Ordering::Relaxed);
                                format!("iCloud rechazó un enlace recién pedido (HTTP {c})")
                            }
                            otro => otro.to_string(),
                        };
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

    if parar.load(Ordering::Relaxed) && !cancelar.load(Ordering::Relaxed) && !sin_espacio.load(Ordering::Relaxed) {
        aplazadas.fetch_add(cola.lock().unwrap().len(), Ordering::Relaxed);
    }
    let aplazadas = aplazadas.load(Ordering::Relaxed);
    if aplazadas > 0 {
        bitacora::escribir(&dir_datos, &format!("ENLACES CADUCAN: {aplazadas} archivos aplazados hasta el próximo listado"));
    }

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
        aplazadas,
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

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://cvws-h2.icloud-content.com/B/AbC/${f}?o=AtX&v=1&x=3&a=CAog&e=1762538400&fl=&r=d1&k=$k&ckc=com.apple.photos.cloud";

    #[test]
    fn lee_la_caducidad_del_enlace() {
        assert_eq!(caducidad_url(URL), Some(1_762_538_400));
        assert_eq!(caducidad_url("http://x/a?e=5"), Some(5));
        assert_eq!(caducidad_url("http://x/a?le=5&e=7#frag"), Some(7));
        assert_eq!(caducidad_url("http://x/a"), None);
        assert_eq!(caducidad_url("http://x/a?e="), None);
        assert_eq!(caducidad_url("http://x/a?v=1"), None);
    }

    #[test]
    fn caduca_pronto_cuenta_margen_y_tamano() {
        let e = 1_762_538_400;
        assert!(!caduca_pronto(URL, 1000, e - 121));
        assert!(caduca_pronto(URL, 1000, e - 120));
        // 200 MiB a 2 MiB/s = 100 s más de margen.
        assert!(caduca_pronto(URL, 200 * 1024 * 1024, e - 220));
        assert!(!caduca_pronto(URL, 200 * 1024 * 1024, e - 221));
        assert!(!caduca_pronto("http://x/sin-caducidad", u64::MAX / 2, 0), "sin `e` no se aplaza nunca");
    }

    #[test]
    fn rechazo_reciente_es_fallo_real_y_tardio_es_caducidad() {
        let e = 1_762_538_400;
        // Listado reciente y `e` lejos: el asset está roto de verdad.
        assert!(!rechazo_por_caducidad(URL, e - 3000, e - 2500, true));
        // `e` alcanzado (o a punto): caducidad.
        assert!(rechazo_por_caducidad(URL, e - 500, e - 60, true));
        assert!(rechazo_por_caducidad(URL, e - 500, e + 10, true));
        // Listado de hace más de 10 min: caducidad aunque no haya `e`.
        assert!(rechazo_por_caducidad("http://x/a", 1000, 1000 + 601, true));
        assert!(!rechazo_por_caducidad("http://x/a", 1000, 1000 + 600, true));
        // Sin fiarse de `e` (reloj desajustado) solo cuenta la antigüedad del listado.
        assert!(!rechazo_por_caducidad(URL, e + 5000, e + 5100, false));
        assert!(rechazo_por_caducidad(URL, e + 5000, e + 5700, false));
    }
}
