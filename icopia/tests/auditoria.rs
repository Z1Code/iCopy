mod comun;
use comun::*;
use icopia::auditoria;
use icopia::descarga;
use icopia::fuente::FuenteFalsa;
use icopia::modelo::TipoParte;
use icopia::motor::{self, Avance, OpcionesRonda};
use icopia::registro::Registro;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

#[test]
fn detecta_archivos_danados_y_ausentes_y_los_reprograma() {
    let (a, b, c) = (jpeg(300), jpeg(400), jpeg(500));
    let s = servidor(vec![("/a", Comportamiento::Servir(a.clone())), ("/b", Comportamiento::Servir(b.clone())), ("/c", Comportamiento::Servir(c.clone()))]);
    let f8 = "2026-08-10T19:37:18-04:00";
    let mut f = FuenteFalsa {
        assets: vec![
            asset("A1", "IMG_A", f8, "PrimarySync", vec![parte(TipoParte::Original, &a, s.url("/a"), "JPG")]),
            asset("A2", "IMG_B", f8, "PrimarySync", vec![parte(TipoParte::Original, &b, s.url("/b"), "JPG")]),
            asset("A3", "IMG_C", f8, "PrimarySync", vec![parte(TipoParte::Original, &c, s.url("/c"), "JPG")]),
        ],
        ..Default::default()
    };
    let d = tempfile::tempdir().unwrap();
    let reg = Mutex::new(Registro::en_memoria().unwrap());
    let op = OpcionesRonda { concurrencia: 2, mes: None };
    motor::ejecutar_ronda(d.path(), &reg, &mut f, &descarga::cliente(), &op, &AtomicBool::new(false), &|_: Avance| {}).unwrap();
    let carpeta = d.path().join(r"2026\2026-08");
    let mut danado = b.clone();
    danado[10] ^= 0xFF; // mismo tamaño, contenido distinto
    std::fs::write(carpeta.join("IMG_B.JPG"), danado).unwrap();
    std::fs::remove_file(carpeta.join("IMG_C.JPG")).unwrap();

    let r = auditoria::verificar_todo(d.path(), &reg, &AtomicBool::new(false), &|_, _| {}).unwrap();
    assert_eq!(r.revisadas, 3);
    let mut malas: Vec<&str> = r.malas.iter().map(|(ruta, _)| ruta.as_str()).collect();
    malas.sort();
    assert_eq!(malas, vec![r"2026\2026-08\IMG_B.JPG", r"2026\2026-08\IMG_C.JPG"]);
    assert_eq!(reg.lock().unwrap().resumen().unwrap().verificadas, 1);
    assert_eq!(reg.lock().unwrap().partes_pendientes(motor::ahora(), None).unwrap().len(), 2);
}
