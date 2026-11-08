//! Liberar espacio: borrar de iCloud solo lo copiado, verificado y re-verificado.
use crate::criterio::Criterio;
use crate::fuente::FuenteIcloud;
use crate::progreso_liberar::{self, FaseLiberar, Seguimiento};
use crate::registro::{Candidato, Registro};
use crate::verificar;
use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const TAMANO_LOTE: usize = 50;
const CABECERA_CSV: &str = "fecha_borrado,asset_id,nombre,fecha_captura,archivos,sesion";

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
    /// Identificador de esta liberación (en el registro y en borrados.csv): sirve para restaurarla.
    pub sesion: String,
    /// Se paró a petición antes de terminar (tras el lote en curso).
    pub cancelado: bool,
    pub bytes: u64,
}

/// Pausar o cancelar un borrado en marcha desde otro hilo (se mira entre lotes).
#[derive(Debug, Default)]
pub struct ControlBorrado {
    pub pausa: AtomicBool,
    pub cancelar: AtomicBool,
}

/// Requiere un listado completo reciente (`marca_lista`): solo propone lo que sigue en iCloud.
/// Anota su avance en el registro (fase "reverificando" y, con algo que borrar, "confirmando").
pub fn preparar(destino: &Path, registro: &Mutex<Registro>, mantener_dias: u32, ahora: i64, avance: &dyn Fn(usize, usize)) -> Result<PlanLiberar> {
    preparar_criterio(destino, registro, &Criterio::ConservarDias(mantener_dias), ahora, avance)
}

/// Como `preparar`, con cualquier criterio (días a conservar o años de captura).
pub fn preparar_criterio(destino: &Path, registro: &Mutex<Registro>, criterio: &Criterio, ahora: i64, avance: &dyn Fn(usize, usize)) -> Result<PlanLiberar> {
    let mut seg = Seguimiento::continuar(registro, Some(criterio.dias()));
    let r = preparar_con(destino, registro, criterio, ahora, avance, &mut seg);
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
    criterio: &Criterio,
    ahora: i64,
    avance: &dyn Fn(usize, usize),
    seg: &mut Seguimiento,
) -> Result<PlanLiberar> {
    let c = {
        let reg = registro.lock().unwrap();
        let marca = reg.marca_lista()?.ok_or_else(|| anyhow!("No hay un listado completo de iCloud; no se puede liberar"))?;
        reg.candidatos_liberar_en(marca, criterio.limite(ahora), criterio.anios())?
    };
    let candidatos = c.candidatos;
    let total = candidatos.len();
    let bytes_total: u64 = candidatos.iter().map(bytes_de).sum();
    seg.fase(FaseLiberar::Reverificando, total as u64, bytes_total);
    let mut plan = PlanLiberar { listos: vec![], excluidos: vec![], no_verificados: c.no_verificados, no_liberables: c.no_liberables, bytes: 0, atipicos: 0 };
    let mut revisados = 0u64;
    for (i, c) in candidatos.into_iter().enumerate() {
        revisados += bytes_de(&c);
        match motivo_exclusion(destino, &c) {
            None => {
                plan.bytes += bytes_de(&c);
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

fn bytes_de(c: &Candidato) -> u64 {
    c.partes.iter().map(|p| p.tamano).sum()
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

/// Lista de lo que se va a borrar (para revisarla antes de confirmar): `.icopia\plan_liberar.csv`.
pub fn escribir_plan_csv(destino: &Path, plan: &PlanLiberar) -> Result<std::path::PathBuf> {
    let ruta = destino.join(".icopia").join("plan_liberar.csv");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&ruta)?);
    writeln!(f, "asset_id,nombre,fecha_captura,bytes,archivos")?;
    for c in &plan.listos {
        let archivos: Vec<&str> = c.partes.iter().map(|p| p.ruta.as_str()).collect();
        writeln!(f, "{},{},{},{},{}", campo(&c.asset_id), campo(&c.nombre_base), campo(&c.fecha), bytes_de(c), campo(&archivos.join(" | ")))?;
    }
    f.flush()?;
    Ok(ruta)
}

/// Borra por lotes y anota el avance en el registro (fase "borrando" y, al final, "terminado";
/// si iCloud falla, "fallido" con lo hecho hasta entonces).
pub fn ejecutar(destino: &Path, registro: &Mutex<Registro>, fuente: &mut dyn FuenteIcloud, listos: &[Candidato], avance: &dyn Fn(usize, usize)) -> Result<ResumenBorrado> {
    ejecutar_con(destino, registro, fuente, listos, avance, &ControlBorrado::default())
}

/// Como `ejecutar`, pero se puede pausar o cancelar entre lotes con `control`.
pub fn ejecutar_con(
    destino: &Path,
    registro: &Mutex<Registro>,
    fuente: &mut dyn FuenteIcloud,
    listos: &[Candidato],
    avance: &dyn Fn(usize, usize),
    control: &ControlBorrado,
) -> Result<ResumenBorrado> {
    let mut seg = Seguimiento::continuar(registro, None);
    let sesion = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    seg.sesion(&sesion);
    seg.fase(FaseLiberar::Borrando, listos.len() as u64, listos.iter().map(bytes_de).sum());
    let csv = destino.join(".icopia").join("borrados.csv");
    let mut resumen = ResumenBorrado { borrados: 0, fallidos: vec![], errores: vec![], sesion: sesion.clone(), cancelado: false, bytes: 0 };
    if let Err(e) = preparar_csv(&csv) {
        resumen.errores.push((String::new(), format!("no se pudo preparar {}: {e:#}", csv.display())));
    }
    let mut hechos = 0;
    for lote in listos.chunks(TAMANO_LOTE) {
        if esperar_si_pausa(control, &mut seg) {
            resumen.cancelado = true;
            break;
        }
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
            if let Err(e) = anotar_csv(&csv, &cuando, &sesion, c) {
                resumen.errores.push((id.clone(), format!("no se pudo anotar en {}: {e:#}", csv.display())));
            }
            if let Err(e) = registro.lock().unwrap().marcar_borrado_en(id, &cuando, Some(&sesion)) {
                resumen.errores.push((id.clone(), format!("no se pudo anotar en el registro: {e:#}")));
            }
            resumen.borrados += 1;
            resumen.bytes += bytes_de(c);
        }
        resumen.fallidos.extend(r.fallidos);
        hechos += lote.len();
        seg.contar(resumen.borrados as u64, resumen.fallidos.len() as u64);
        seg.avance(hechos as u64, resumen.bytes);
        avance(hechos, listos.len());
    }
    seg.terminar(resumen.cancelado);
    Ok(resumen)
}

/// En pausa espera (renovando el avance para que no parezca interrumpido). `true` = cancelar.
fn esperar_si_pausa(control: &ControlBorrado, seg: &mut Seguimiento) -> bool {
    if control.pausa.load(Ordering::SeqCst) && !control.cancelar.load(Ordering::SeqCst) {
        seg.pausa(true);
        let mut latido = Instant::now();
        while control.pausa.load(Ordering::SeqCst) && !control.cancelar.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
            if latido.elapsed() >= Duration::from_secs(20) {
                latido = Instant::now();
                seg.pausa(true);
            }
        }
        seg.pausa(false);
    }
    control.cancelar.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResumenRestaurar {
    pub restaurados: Vec<String>,
    /// (id, motivo)
    pub fallidos: Vec<(String, String)>,
}

/// Saca de "Eliminado recientemente" lo borrado en una liberación (`sesion`) y lo vuelve a dar por
/// presente en iCloud. Queda anotado en `.icopia\restaurados.csv`.
pub fn restaurar_sesion(destino: &Path, registro: &Mutex<Registro>, fuente: &mut dyn FuenteIcloud, sesion: &str) -> Result<ResumenRestaurar> {
    let ids = registro.lock().unwrap().ids_de_sesion(sesion)?;
    restaurar(destino, registro, fuente, &ids)
}

pub fn restaurar(destino: &Path, registro: &Mutex<Registro>, fuente: &mut dyn FuenteIcloud, ids: &[String]) -> Result<ResumenRestaurar> {
    let csv = destino.join(".icopia").join("restaurados.csv");
    let mut r = ResumenRestaurar::default();
    for lote in ids.chunks(TAMANO_LOTE * 4) {
        let hecho = fuente.restaurar(lote).map_err(|e| anyhow!("restaurando en iCloud: {e}"))?;
        let cuando = chrono::Local::now().to_rfc3339();
        for id in &hecho.borrados {
            registro.lock().unwrap().marcar_restaurado(id)?;
            let nuevo = !csv.exists();
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&csv)?;
            if nuevo {
                writeln!(f, "fecha_restaurado,asset_id")?;
            }
            writeln!(f, "{},{}", campo(&cuando), campo(id))?;
        }
        r.restaurados.extend(hecho.borrados);
        r.fallidos.extend(hecho.fallidos);
    }
    // Lo restaurado vuelve a la fototeca: el resumen del panel ya no vale.
    if !r.restaurados.is_empty() {
        let _ = registro.lock().unwrap().borrar_meta(progreso_liberar::CLAVE);
    }
    Ok(r)
}

fn campo(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Crea borrados.csv con su cabecera o, si es de una versión anterior (sin columna `sesion`),
/// le añade la columna a la cabecera (las filas antiguas quedan sin sesión).
fn preparar_csv(csv: &Path) -> Result<()> {
    if let Some(p) = csv.parent() {
        std::fs::create_dir_all(p)?;
    }
    if !csv.exists() {
        std::fs::write(csv, format!("{CABECERA_CSV}\n"))?;
        return Ok(());
    }
    let texto = std::fs::read_to_string(csv)?;
    let (primera, resto) = texto.split_once('\n').unwrap_or((texto.as_str(), ""));
    if primera.trim_end().ends_with(",sesion") {
        return Ok(());
    }
    let tmp = csv.with_extension("csv.tmp");
    std::fs::write(&tmp, format!("{},sesion\n{resto}", primera.trim_end()))?;
    std::fs::rename(&tmp, csv)?;
    Ok(())
}

fn anotar_csv(csv: &Path, cuando: &str, sesion: &str, c: &Candidato) -> Result<()> {
    if !csv.exists() {
        preparar_csv(csv)?;
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(csv)?;
    let archivos: Vec<&str> = c.partes.iter().map(|p| p.ruta.as_str()).collect();
    writeln!(f, "{},{},{},{},{},{}", campo(cuando), campo(&c.asset_id), campo(&c.nombre_base), campo(&c.fecha), campo(&archivos.join(" | ")), campo(sesion))?;
    f.sync_all()?;
    Ok(())
}
