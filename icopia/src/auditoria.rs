//! "Verificar todo": re-comprueba lo copiado y reprograma lo dañado o ausente.
use crate::registro::Registro;
use crate::verificar;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultadoVerificacion {
    pub revisadas: usize,
    /// (ruta relativa, motivo)
    pub malas: Vec<(String, String)>,
}

pub fn verificar_todo(
    destino: &Path,
    registro: &Mutex<Registro>,
    cancelar: &AtomicBool,
    avance: &dyn Fn(usize, usize),
) -> anyhow::Result<ResultadoVerificacion> {
    let partes = registro.lock().unwrap().partes_verificadas()?;
    let total = partes.len();
    let mut r = ResultadoVerificacion::default();
    for p in &partes {
        if cancelar.load(Ordering::Relaxed) {
            break;
        }
        let motivo = match verificar::verificar_archivo(&destino.join(&p.ruta), p.tamano, &p.extension) {
            Ok(sha) if sha == p.sha256 => None,
            Ok(_) => Some("el contenido cambió (SHA-256 distinto)".to_string()),
            Err(f) => Some(f.to_string()),
        };
        if let Some(m) = motivo {
            registro.lock().unwrap().marcar_para_redescarga(&p.asset_id, p.tipo, &m)?;
            r.malas.push((p.ruta.clone(), m));
        }
        r.revisadas += 1;
        avance(r.revisadas, total);
    }
    Ok(r)
}
