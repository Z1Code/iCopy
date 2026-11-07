mod comun;
use comun::*;
use icopia::descarga;
use icopia::fuente::FuenteFalsa;
use icopia::liberar;
use icopia::modelo::TipoParte;
use icopia::motor::{self, Avance, OpcionesRonda};
use icopia::registro::Registro;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

struct Escenario {
    _s: Servidor,
    dir: tempfile::TempDir,
    reg: Mutex<Registro>,
    fuente: FuenteFalsa,
}

/// V1 y V2 viejas y personales; C1 vieja de Fototeca Compartida; P1 vieja que no se pudo copiar (403);
/// N1 de ayer.
fn escenario() -> Escenario {
    let (v1, v2, c1, n1) = (jpeg(100), jpeg(200), jpeg(300), jpeg(400));
    let s = servidor(vec![
        ("/v1", Comportamiento::Servir(v1.clone())),
        ("/v2", Comportamiento::Servir(v2.clone())),
        ("/c1", Comportamiento::Servir(c1.clone())),
        ("/p1", Comportamiento::Estado(403)),
        ("/n1", Comportamiento::Servir(n1.clone())),
    ]);
    let ayer = (chrono::Local::now() - chrono::Duration::days(1)).fixed_offset().to_rfc3339();
    let fuente = FuenteFalsa {
        assets: vec![
            asset("V1", "IMG_V1", "2025-01-10T10:00:00-05:00", "PrimarySync", vec![parte(TipoParte::Original, &v1, s.url("/v1"), "JPG")]),
            asset("V2", "IMG_V2", "2025-01-11T10:00:00-05:00", "PrimarySync", vec![parte(TipoParte::Original, &v2, s.url("/v2"), "JPG")]),
            asset("C1", "IMG_C1", "2025-01-12T10:00:00-05:00", "SharedSync-AB", vec![parte(TipoParte::Original, &c1, s.url("/c1"), "JPG")]),
            asset("P1", "IMG_P1", "2025-03-01T10:00:00-05:00", "PrimarySync", vec![parte(TipoParte::Original, &jpeg(10), s.url("/p1"), "JPG")]),
            asset("N1", "IMG_N1", &ayer, "PrimarySync", vec![parte(TipoParte::Original, &n1, s.url("/n1"), "JPG")]),
        ],
        ..Default::default()
    };
    let mut e = Escenario { _s: s, dir: tempfile::tempdir().unwrap(), reg: Mutex::new(Registro::en_memoria().unwrap()), fuente };
    let op = OpcionesRonda { concurrencia: 1, mes: None };
    motor::ejecutar_ronda(e.dir.path(), &e.reg, &mut e.fuente, &descarga::cliente(), &op, &AtomicBool::new(false), &|_: Avance| {}).unwrap();
    e
}

#[test]
fn prepara_solo_lo_seguro_y_borra() {
    let mut e = escenario();
    // V2 se altera en disco después de verificarse
    let ruta_v2 = e.dir.path().join(r"2025\2025-01\IMG_V2.JPG");
    let mut v2 = std::fs::read(&ruta_v2).unwrap();
    v2[5] ^= 0xFF;
    std::fs::write(&ruta_v2, v2).unwrap();

    let plan = liberar::preparar(e.dir.path(), &e.reg, 30, motor::ahora(), &|_, _| {}).unwrap();
    assert_eq!(plan.listos.iter().map(|c| c.asset_id.as_str()).collect::<Vec<_>>(), vec!["V1"]);
    assert_eq!(plan.excluidos.iter().map(|(c, _)| c.asset_id.as_str()).collect::<Vec<_>>(), vec!["V2"]);
    assert_eq!(plan.no_verificados, 1); // P1
    assert_eq!(plan.bytes, 104);

    let r = liberar::ejecutar(e.dir.path(), &e.reg, &mut e.fuente, &plan.listos, &|_, _| {}).unwrap();
    assert_eq!(r.borrados, 1);
    assert_eq!(e.fuente.borrados, vec!["V1"]);
    let csv = std::fs::read_to_string(e.dir.path().join(r".icopia\borrados.csv")).unwrap();
    assert!(csv.starts_with("fecha_borrado,asset_id,nombre,fecha_captura,archivos"));
    assert!(csv.contains("\"V1\""));
    assert!(e.dir.path().join(r"2025\2025-01\IMG_V1.JPG").exists(), "nunca se borra del PC");

    let otra = liberar::preparar(e.dir.path(), &e.reg, 30, motor::ahora(), &|_, _| {}).unwrap();
    assert!(otra.listos.is_empty());
}

#[test]
fn los_rechazados_no_se_marcan() {
    let mut e = escenario();
    e.fuente.rechazar = vec!["V1".into()];
    let plan = liberar::preparar(e.dir.path(), &e.reg, 30, motor::ahora(), &|_, _| {}).unwrap();
    // Aquí V2 no se altera: listos = [V1, V2]; V1 es rechazada por iCloud y V2 se borra.
    let r = liberar::ejecutar(e.dir.path(), &e.reg, &mut e.fuente, &plan.listos, &|_, _| {}).unwrap();
    assert_eq!((r.borrados, r.fallidos.len()), (1, 1));
    let de_nuevo = liberar::preparar(e.dir.path(), &e.reg, 30, motor::ahora(), &|_, _| {}).unwrap();
    assert!(de_nuevo.listos.iter().any(|c| c.asset_id == "V1"));
}

#[test]
fn sin_listado_no_se_puede_liberar() {
    let d = tempfile::tempdir().unwrap();
    let reg = Mutex::new(Registro::en_memoria().unwrap());
    assert!(liberar::preparar(d.path(), &reg, 30, motor::ahora(), &|_, _| {}).is_err());
}

#[test]
fn assets_con_datos_no_copiables_no_se_proponen() {
    let v1 = jpeg(100);
    let s = servidor(vec![("/v1", Comportamiento::Servir(v1.clone()))]);
    let mut a = asset("V1", "IMG_V1", "2025-01-10T10:00:00-05:00", "PrimarySync", vec![parte(TipoParte::Original, &v1, s.url("/v1"), "JPG")]);
    a.liberable = false;
    let mut fuente = FuenteFalsa { assets: vec![a], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let reg = Mutex::new(Registro::en_memoria().unwrap());
    let op = OpcionesRonda { concurrencia: 1, mes: None };
    let r = motor::ejecutar_ronda(d.path(), &reg, &mut fuente, &descarga::cliente(), &op, &AtomicBool::new(false), &|_: Avance| {}).unwrap();
    assert_eq!(r.verificadas, 1, "se copia igualmente");
    let plan = liberar::preparar(d.path(), &reg, 30, motor::ahora(), &|_, _| {}).unwrap();
    assert!(plan.listos.is_empty());
    assert_eq!((plan.no_verificados, plan.no_liberables), (0, 1));
}
