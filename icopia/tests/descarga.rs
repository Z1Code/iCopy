mod comun;
use comun::*;
use icopia::descarga::{self, ErrorDescarga};
use std::sync::atomic::AtomicBool;

fn bajar(url: &str, part: &std::path::Path, tamano: u64) -> Result<(), ErrorDescarga> {
    descarga::descargar(&descarga::cliente(), url, part, tamano, &AtomicBool::new(false))
}

#[test]
fn descarga_completa() {
    let datos = patron(300_000);
    let s = servidor(vec![("/a", Comportamiento::Servir(datos.clone()))]);
    let d = tempfile::tempdir().unwrap();
    let part = d.path().join("a.part");
    bajar(&s.url("/a"), &part, datos.len() as u64).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), datos);
}

#[test]
fn reanuda_tras_corte() {
    let datos = patron(3_000_000);
    let s = servidor(vec![("/a", Comportamiento::CortarPrimeraVez { datos: datos.clone(), tras: 1_000_000 })]);
    let d = tempfile::tempdir().unwrap();
    let part = d.path().join("a.part");
    assert!(matches!(bajar(&s.url("/a"), &part, datos.len() as u64), Err(ErrorDescarga::Transitorio(_))));
    let parcial = std::fs::metadata(&part).unwrap().len();
    assert!(parcial > 0 && parcial < datos.len() as u64, "parcial = {parcial}");
    bajar(&s.url("/a"), &part, datos.len() as u64).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), datos);
    assert_eq!(s.peticiones_a("/a")[1].rango.as_deref(), Some(format!("bytes={parcial}-").as_str()));
}

#[test]
fn url_caducada() {
    let s = servidor(vec![("/a", Comportamiento::Estado(403))]);
    let d = tempfile::tempdir().unwrap();
    assert_eq!(bajar(&s.url("/a"), &d.path().join("a.part"), 10), Err(ErrorDescarga::UrlCaducada(403)));
}

#[test]
fn error_de_servidor_es_transitorio() {
    let s = servidor(vec![("/a", Comportamiento::Estado(503))]);
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(bajar(&s.url("/a"), &d.path().join("a.part"), 10), Err(ErrorDescarga::Transitorio(_))));
}

#[test]
fn part_mas_grande_que_lo_esperado_se_descarta() {
    let datos = patron(1000);
    let s = servidor(vec![("/a", Comportamiento::Servir(datos.clone()))]);
    let d = tempfile::tempdir().unwrap();
    let part = d.path().join("a.part");
    std::fs::write(&part, patron(1500)).unwrap();
    bajar(&s.url("/a"), &part, 1000).unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), datos);
    assert_eq!(s.peticiones_a("/a")[0].rango, None);
}

#[test]
fn ya_completa_no_pide_nada() {
    let s = servidor(vec![]);
    let d = tempfile::tempdir().unwrap();
    let part = d.path().join("a.part");
    std::fs::write(&part, patron(10)).unwrap();
    bajar(&s.url("/a"), &part, 10).unwrap();
    assert_eq!(s.total_peticiones(), 0);
}

#[test]
fn cancelada() {
    let datos = patron(1000);
    let s = servidor(vec![("/a", Comportamiento::Servir(datos))]);
    let d = tempfile::tempdir().unwrap();
    let r = descarga::descargar(&descarga::cliente(), &s.url("/a"), &d.path().join("a.part"), 1000, &AtomicBool::new(true));
    assert_eq!(r, Err(ErrorDescarga::Cancelada));
}

#[test]
fn ruta_part_anade_sufijo() {
    let p = descarga::ruta_part(std::path::Path::new(r"C:\x\IMG_1.HEIC"));
    assert_eq!(p, std::path::PathBuf::from(r"C:\x\IMG_1.HEIC.part"));
}
