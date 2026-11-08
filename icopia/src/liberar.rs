//! Liberar espacio: borrar de iCloud solo lo copiado, verificado y re-verificado.
use crate::fuente::FuenteIcloud;
use crate::progreso_liberar::{FaseLiberar, Seguimiento};
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
    /// Con datos en iCloud que icopia aún no sabe copiar.
    pub no_liberables: u64,
    pub bytes: u64,
    /// Partes de los listos con estructura atípica, confirmadas por doble descarga.
    pub atipicos: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumenBorrado {
    pub borrados: usize,
    pub fallidos: Vec<(String, String)>,
    /// Borrados en iCloud que no se pudieron anotar del todo (CSV o registro): (id, motivo).
    pub errores: Vec<(String, String)>,
}

/// Requiere un listado completo reciente (`marca_lista`): solo propone lo que sigue en iCloud.
/// Anota su avance en el registro (fase "reverificando" y, con algo que borrar, "confirmando").
pub fn preparar(destino: &Path, registro: &Mutex<Registro>, mantener_dias: u32, ahora: i64, avance: &dyn Fn(usize, usize)) -> Result<PlanLiberar> {
    let mut seg = Seguimiento::continuar(registro, Some(mantener_dias));
    let r = preparar_con(destino, registro, mantener_dias, ahora, avance, &mut seg);
    match &r {
        Ok(plan) if plan.listos.is_empty() => seg.descartar(),
        Ok(plan) => seg.fase(FaseLiberar::Confirmando, plan.listos.len() as u64, plan.bytes),
        Err(e) => seg.fallar(&format!("{e:#}")),
    }
    r
}

fn preparar_con(
    destino: &Path,
    registro: &Mutex<Registro>,
    mantener_dias: u32,
    ahora: i64,
    avance: &dyn Fn(usize, usize),
    seg: &mut Seguimiento,
) -> Result<PlanLiberar> {
    let c = {
        let reg = registro.lock().unwrap();
        let marca = reg.marca_lista()?.ok_or_else(|| anyhow!("No hay un listado completo de iCloud; no se puede liberar"))?;
        reg.candidatos_liberar(marca, ahora - i64::from(mantener_dias) * 86_400)?
    };
    let candidatos = c.candidatos;
    let total = candidatos.len();
    let bytes_total: u64 = candidatos.iter().flat_map(|c| &c.partes).map(|p| p.tamano).sum();
    seg.fase(FaseLiberar::Reverificando, total as u64, bytes_total);
    let mut plan = PlanLiberar { listos: vec![], excluidos: vec![], no_verificados: c.no_verificados, no_liberables: c.no_liberables, bytes: 0, atipicos: 0 };
    let mut revisados = 0u64;
    for (i, c) in candidatos.into_iter().enumerate() {
        revisados += c.partes.iter().map(|p| p.tamano).sum::<u64>();
        match motivo_exclusion(destino, &c) {
            None => {
                plan.bytes += c.partes.iter().map(|p| p.tamano).sum::<u64>();
                plan.atipicos += c.partes.iter().filter(|p| p.estructura_atipica).count() as u64;
                plan.listos.push(c);
            }
            Some(m) => plan.excluidos.push((c, m)),
        }
        seg.avance(i as u64 + 1, revisados);
        avance(i + 1, total);
    }
    Ok(plan)
}

fn motivo_exclusion(destino: &Path, c: &Candidato) -> Option<String> {
    for p in &c.partes {
        match verificar::reverificar(&destino.join(&p.ruta), p.tamano, &p.extension, p.estructura_atipica) {
            Ok(sha) if sha == p.sha256 => {}
            Ok(_) => return Some(format!("{}: el contenido cambió (SHA-256 distinto)", p.ruta)),
            Err(f) => return Some(format!("{}: {f}", p.ruta)),
        }
    }
    None
}

/// Borra por lotes y anota el avance en el registro (fase "borrando" y, al final, "terminado";
/// si iCloud falla, "fallido" con lo hecho hasta entonces).
pub fn ejecutar(destino: &Path, registro: &Mutex<Registro>, fuente: &mut dyn FuenteIcloud, listos: &[Candidato], avance: &dyn Fn(usize, usize)) -> Result<ResumenBorrado> {
    let mut seg = Seguimiento::continuar(registro, None);
    let bytes_de = |c: &Candidato| c.partes.iter().map(|p| p.tamano).sum::<u64>();
    seg.fase(FaseLiberar::Borrando, listos.len() as u64, listos.iter().map(bytes_de).sum());
    let csv = destino.join(".icopia").join("borrados.csv");
    let mut resumen = ResumenBorrado { borrados: 0, fallidos: vec![], errores: vec![] };
    let mut hechos = 0;
    let mut bytes_borrados = 0u64;
    for lote in listos.chunks(TAMANO_LOTE) {
        let ids: Vec<String> = lote.iter().map(|c| c.asset_id.clone()).collect();
        let r = match fuente.borrar(&ids) {
            Ok(r) => r,
            Err(e) => {
                let e = anyhow!("borrando en iCloud: {e}");
                seg.fallar(&format!("{e:#}"));
                return Err(e);
            }
        };
        let cuando = chrono::Local::now().to_rfc3339();
        // Ya está borrado en iCloud: se anota todo lo posible y un fallo al anotar no detiene el resto.
        for id in &r.borrados {
            let Some(c) = lote.iter().find(|c| &c.asset_id == id) else { continue };
            if let Err(e) = anotar_csv(&csv, &cuando, c) {
                resumen.errores.push((id.clone(), format!("no se pudo anotar en {}: {e:#}", csv.display())));
            }
            if let Err(e) = registro.lock().unwrap().marcar_borrado(id, &cuando) {
                resumen.errores.push((id.clone(), format!("no se pudo anotar en el registro: {e:#}")));
            }
            resumen.borrados += 1;
            bytes_borrados += bytes_de(c);
        }
        resumen.fallidos.extend(r.fallidos);
        hechos += lote.len();
        seg.contar(resumen.borrados as u64, resumen.fallidos.len() as u64);
        seg.avance(hechos as u64, bytes_borrados);
        avance(hechos, listos.len());
    }
    seg.terminar();
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
