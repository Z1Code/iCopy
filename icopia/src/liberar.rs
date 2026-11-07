//! Liberar espacio: borrar de iCloud solo lo copiado, verificado y re-verificado.
use crate::fuente::FuenteIcloud;
use crate::registro::{Candidato, Registro};
use crate::verificar;
use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

pub const TAMANO_LOTE: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanLiberar {
    pub listos: Vec<Candidato>,
    pub excluidos: Vec<(Candidato, String)>,
    pub no_verificados: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumenBorrado {
    pub borrados: usize,
    pub fallidos: Vec<(String, String)>,
}

/// Requiere un listado completo reciente (`marca_lista`): solo propone lo que sigue en iCloud.
pub fn preparar(destino: &Path, registro: &Mutex<Registro>, mantener_dias: u32, ahora: i64, avance: &dyn Fn(usize, usize)) -> Result<PlanLiberar> {
    let (candidatos, no_verificados) = {
        let reg = registro.lock().unwrap();
        let marca = reg.marca_lista()?.ok_or_else(|| anyhow!("No hay un listado completo de iCloud; no se puede liberar"))?;
        reg.candidatos_liberar(marca, ahora - i64::from(mantener_dias) * 86_400)?
    };
    let total = candidatos.len();
    let mut plan = PlanLiberar { listos: vec![], excluidos: vec![], no_verificados, bytes: 0 };
    for (i, c) in candidatos.into_iter().enumerate() {
        match motivo_exclusion(destino, &c) {
            None => {
                plan.bytes += c.partes.iter().map(|p| p.tamano).sum::<u64>();
                plan.listos.push(c);
            }
            Some(m) => plan.excluidos.push((c, m)),
        }
        avance(i + 1, total);
    }
    Ok(plan)
}

fn motivo_exclusion(destino: &Path, c: &Candidato) -> Option<String> {
    for p in &c.partes {
        match verificar::verificar_archivo(&destino.join(&p.ruta), p.tamano, &p.extension) {
            Ok(sha) if sha == p.sha256 => {}
            Ok(_) => return Some(format!("{}: el contenido cambió (SHA-256 distinto)", p.ruta)),
            Err(f) => return Some(format!("{}: {f}", p.ruta)),
        }
    }
    None
}

pub fn ejecutar(destino: &Path, registro: &Mutex<Registro>, fuente: &mut dyn FuenteIcloud, listos: &[Candidato], avance: &dyn Fn(usize, usize)) -> Result<ResumenBorrado> {
    let csv = destino.join(".icopia").join("borrados.csv");
    let mut resumen = ResumenBorrado { borrados: 0, fallidos: vec![] };
    let mut hechos = 0;
    for lote in listos.chunks(TAMANO_LOTE) {
        let ids: Vec<String> = lote.iter().map(|c| c.asset_id.clone()).collect();
        let r = fuente.borrar(&ids).map_err(|e| anyhow!("borrando en iCloud: {e}"))?;
        let cuando = chrono::Local::now().to_rfc3339();
        for id in &r.borrados {
            let Some(c) = lote.iter().find(|c| &c.asset_id == id) else { continue };
            registro.lock().unwrap().marcar_borrado(id, &cuando)?;
            anotar_csv(&csv, &cuando, c)?;
            resumen.borrados += 1;
        }
        resumen.fallidos.extend(r.fallidos);
        hechos += lote.len();
        avance(hechos, listos.len());
    }
    Ok(resumen)
}

fn campo(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn anotar_csv(csv: &Path, cuando: &str, c: &Candidato) -> Result<()> {
    if let Some(p) = csv.parent() {
        std::fs::create_dir_all(p)?;
    }
    let nuevo = !csv.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(csv)?;
    if nuevo {
        writeln!(f, "fecha_borrado,asset_id,nombre,fecha_captura,archivos")?;
    }
    let archivos: Vec<&str> = c.partes.iter().map(|p| p.ruta.as_str()).collect();
    writeln!(f, "{},{},{},{},{}", campo(cuando), campo(&c.asset_id), campo(&c.nombre_base), campo(&c.fecha), campo(&archivos.join(" | ")))?;
    f.sync_all()?;
    Ok(())
}
