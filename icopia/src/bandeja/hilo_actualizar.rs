//! Busca actualizaciones al minuto de arrancar y luego cada 24 h (o al pedirlo desde el menú);
//! descarga y verifica en segundo plano y avisa al bucle principal.
use crate::actualizar;
use crate::progreso_liberar::{ProgresoLiberar, Situacion};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum EventoActualizacion {
    /// Paquete descargado y con firma válida, listo para instalar.
    Lista { version: String, paquete: PathBuf },
    /// Respuesta a una búsqueda pedida desde el menú.
    AlDia,
    Error(String),
}

/// `buscar_ya`: cada mensaje fuerza una búsqueda (manual: se avisa también si está al día o falla).
pub fn lanzar(buscar_ya: Receiver<()>, dir: PathBuf, emitir: impl Fn(EventoActualizacion) + Send + 'static) {
    std::thread::spawn(move || {
        let http = reqwest::blocking::Client::builder().timeout(Duration::from_secs(600)).build().expect("cliente HTTP");
        let mut espera = Duration::from_secs(60);
        let mut ya_lista: Option<String> = None;
        loop {
            let manual = match buscar_ya.recv_timeout(espera) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            espera = Duration::from_secs(24 * 3600);
            match actualizar::consultar(&http) {
                Ok(Some(p)) if ya_lista.as_deref() == Some(&p.version.to_string()) => {
                    if manual {
                        emitir(EventoActualizacion::Lista { version: p.version.to_string(), paquete: dir.join(&p.nombre) });
                    }
                }
                Ok(Some(p)) => match actualizar::descargar(&http, &p, &dir) {
                    Ok(paquete) => {
                        ya_lista = Some(p.version.to_string());
                        emitir(EventoActualizacion::Lista { version: p.version.to_string(), paquete });
                    }
                    Err(e) if manual => emitir(EventoActualizacion::Error(format!("{e:#}"))),
                    Err(_) => {}
                },
                Ok(None) if manual => emitir(EventoActualizacion::AlDia),
                Err(e) if manual => emitir(EventoActualizacion::Error(format!("{e:#}"))),
                _ => {}
            }
        }
    });
}

/// ¿Hay un "liberar espacio" vivo (otro proceso o el asistente)? Cuenta también la espera de la
/// confirmación: la persona está a medias. Uno interrumpido (sin noticias) ya no cuenta.
pub fn liberando(p: Option<&ProgresoLiberar>, ahora: i64) -> bool {
    matches!(p.and_then(|p| p.situacion(ahora)), Some(Situacion::EnCurso | Situacion::Confirmando))
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::progreso_liberar::{FaseLiberar, CADUCA_SEG};

    fn progreso(fase: FaseLiberar, actualizado: i64) -> ProgresoLiberar {
        let mut p = ProgresoLiberar::nuevo(30, 1000);
        p.fase = fase;
        p.actualizado_unix = actualizado;
        p
    }

    #[test]
    fn solo_cuenta_un_liberar_vivo() {
        let ahora = 2000;
        assert!(!liberando(None, ahora));
        for fase in [FaseLiberar::Listando, FaseLiberar::Reverificando, FaseLiberar::Borrando, FaseLiberar::Confirmando] {
            assert!(liberando(Some(&progreso(fase, ahora - 5)), ahora), "{fase:?} reciente");
        }
        assert!(!liberando(Some(&progreso(FaseLiberar::Borrando, ahora - CADUCA_SEG - 1)), ahora), "interrumpido");
        assert!(!liberando(Some(&progreso(FaseLiberar::Confirmando, ahora - CADUCA_SEG - 1)), ahora), "abandonado");
        assert!(!liberando(Some(&progreso(FaseLiberar::Terminado, ahora - 5)), ahora));
        assert!(!liberando(Some(&progreso(FaseLiberar::Fallido, ahora - 5)), ahora));
    }
}
