mod comun;
use comun::*;
use icopia::descarga;
use icopia::fuente::FuenteFalsa;
use icopia::modelo::TipoParte;
use icopia::motor::{self, Avance, FinRonda, OpcionesRonda, ResultadoRonda};
use icopia::registro::Registro;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

const AGO: &str = "2026-08-10T19:37:18-04:00";
const SEP: &str = "2026-09-02T10:00:00-04:00";

fn ronda_con(destino: &Path, reg: &Mutex<Registro>, f: &mut FuenteFalsa, conc: usize, mes: Option<&str>) -> ResultadoRonda {
    let op = OpcionesRonda { concurrencia: conc, mes: mes.map(String::from) };
    motor::ejecutar_ronda(destino, reg, f, &descarga::cliente(), &op, &AtomicBool::new(false), &|_: Avance| {}).unwrap()
}

fn ronda(destino: &Path, reg: &Mutex<Registro>, f: &mut FuenteFalsa) -> ResultadoRonda {
    ronda_con(destino, reg, f, 2, None)
}

fn nuevo_registro() -> Mutex<Registro> {
    Mutex::new(Registro::en_memoria().unwrap())
}

#[test]
fn copia_live_photo_y_jpg_en_carpetas_por_mes() {
    let (h, m, j) = (heic(5000), mov(3000), jpeg(2000));
    let s = servidor(vec![("/h", Comportamiento::Servir(h.clone())), ("/m", Comportamiento::Servir(m.clone())), ("/j", Comportamiento::Servir(j.clone()))]);
    let mut f = FuenteFalsa {
        assets: vec![
            asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &h, s.url("/h"), "HEIC"), parte(TipoParte::Live, &m, s.url("/m"), "MOV")]),
            asset("A2", "IMG_0002", SEP, "PrimarySync", vec![parte(TipoParte::Original, &j, s.url("/j"), "JPG")]),
        ],
        ..Default::default()
    };
    let d = tempfile::tempdir().unwrap();
    let reg = nuevo_registro();
    let r = ronda(d.path(), &reg, &mut f);
    assert_eq!((r.fin, r.verificadas, r.fallidas), (FinRonda::Completa, 3, 0));
    let ago = d.path().join("2026").join("2026-08");
    assert_eq!(std::fs::read(ago.join("IMG_0001.HEIC")).unwrap(), h);
    assert_eq!(std::fs::read(ago.join("IMG_0001.MOV")).unwrap(), m);
    assert_eq!(std::fs::read(d.path().join("2026").join("2026-09").join("IMG_0002.JPG")).unwrap(), j);
    assert!(!ago.join("IMG_0001.HEIC.part").exists());
    let r2 = ronda(d.path(), &reg, &mut f);
    assert_eq!(r2.verificadas, 0);
    assert_eq!(s.total_peticiones(), 3);
}

#[test]
fn reanuda_tras_corte() {
    let h = heic(3_000_000);
    let s = servidor(vec![("/h", Comportamiento::CortarPrimeraVez { datos: h.clone(), tras: 1_000_000 })]);
    let mut f = FuenteFalsa { assets: vec![asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &h, s.url("/h"), "HEIC")])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let reg = nuevo_registro();
    assert_eq!(ronda(d.path(), &reg, &mut f).fallidas, 1);
    reg.lock().unwrap().reintentar_ya().unwrap();
    assert_eq!(ronda(d.path(), &reg, &mut f).verificadas, 1);
    assert_eq!(std::fs::read(d.path().join(r"2026\2026-08\IMG_0001.HEIC")).unwrap(), h);
    assert!(s.peticiones_a("/h")[1].rango.is_some());
}

#[test]
fn url_caducada_pide_relistar() {
    let h = heic(100);
    let (a, b) = (jpeg(100), jpeg(200));
    let s = servidor(vec![("/h", Comportamiento::Estado(403)), ("/a", Comportamiento::Servir(a.clone())), ("/b", Comportamiento::Servir(b.clone()))]);
    let mut f = FuenteFalsa {
        assets: vec![
            asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &h, s.url("/h"), "HEIC")]),
            asset("A2", "IMG_0002", AGO, "PrimarySync", vec![parte(TipoParte::Original, &a, s.url("/a"), "JPG")]),
            asset("A3", "IMG_0003", SEP, "PrimarySync", vec![parte(TipoParte::Original, &b, s.url("/b"), "JPG")]),
        ],
        ..Default::default()
    };
    let d = tempfile::tempdir().unwrap();
    let r = ronda_con(d.path(), &nuevo_registro(), &mut f, 1, None);
    assert_eq!((r.fin, r.fallidas, r.verificadas), (FinRonda::Relistar, 1, 2), "el resto de la ronda sigue copiándose");
}

#[test]
fn contenido_corrupto_no_se_acepta() {
    let mut malo = jpeg(1000);
    let n = malo.len();
    malo[n - 1] = 0;
    malo[n - 2] = 0;
    let s = servidor(vec![("/j", Comportamiento::Servir(malo.clone()))]);
    let mut f = FuenteFalsa { assets: vec![asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &malo, s.url("/j"), "JPG")])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let r = ronda(d.path(), &nuevo_registro(), &mut f);
    assert_eq!(r.fallidas, 1);
    let carpeta = d.path().join(r"2026\2026-08");
    assert!(!carpeta.join("IMG_0001.JPG").exists());
    assert!(!carpeta.join("IMG_0001.JPG.part").exists());
}

#[test]
fn colision_de_nombres_no_pisa() {
    let (a, b) = (jpeg(100), jpeg(200));
    let s = servidor(vec![("/a", Comportamiento::Servir(a.clone())), ("/b", Comportamiento::Servir(b.clone()))]);
    let mut f = FuenteFalsa {
        assets: vec![
            asset("A1xxxx", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &a, s.url("/a"), "JPG")]),
            asset("Bq9zzz", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &b, s.url("/b"), "JPG")]),
        ],
        ..Default::default()
    };
    let d = tempfile::tempdir().unwrap();
    assert_eq!(ronda_con(d.path(), &nuevo_registro(), &mut f, 1, None).verificadas, 2);
    let carpeta = d.path().join(r"2026\2026-08");
    assert_eq!(std::fs::read(carpeta.join("IMG_0001.JPG")).unwrap(), a);
    assert_eq!(std::fs::read(carpeta.join("IMG_0001 (bq9zzz).JPG")).unwrap(), b);
}

#[test]
fn sesion_caducada() {
    let mut f = FuenteFalsa { sesion_caducada: true, ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    assert_eq!(ronda(d.path(), &nuevo_registro(), &mut f).fin, FinRonda::SesionCaducada);
}

#[test]
fn destino_ausente() {
    let mut f = FuenteFalsa::default();
    let d = tempfile::tempdir().unwrap();
    assert_eq!(ronda(&d.path().join("no-existe"), &nuevo_registro(), &mut f).fin, FinRonda::DestinoAusente);
}

#[test]
fn archivo_final_existente_se_adopta_sin_descargar() {
    let j = jpeg(500);
    let s = servidor(vec![("/j", Comportamiento::Servir(j.clone()))]);
    let mut f = FuenteFalsa { assets: vec![asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &j, s.url("/j"), "JPG")])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let reg = nuevo_registro();
    ronda(d.path(), &reg, &mut f);
    // Simula un corte entre renombrar el .part verificado y anotarlo como verificado.
    let sha = reg.lock().unwrap().partes_verificadas().unwrap()[0].sha256.clone();
    reg.lock().unwrap().marcar_para_redescarga("A1", TipoParte::Original, "prueba").unwrap();
    reg.lock().unwrap().anotar_sha_pendiente("A1", TipoParte::Original, &sha).unwrap();
    assert_eq!(ronda(d.path(), &reg, &mut f).verificadas, 1);
    assert_eq!(s.total_peticiones(), 1);
}

#[test]
fn archivo_final_con_sha_pendiente_distinto_no_se_adopta() {
    let j = jpeg(500);
    let s = servidor(vec![("/j", Comportamiento::Servir(j.clone()))]);
    let mut f = FuenteFalsa { assets: vec![asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &j, s.url("/j"), "JPG")])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let reg = nuevo_registro();
    ronda(d.path(), &reg, &mut f);
    reg.lock().unwrap().marcar_para_redescarga("A1", TipoParte::Original, "prueba").unwrap();
    reg.lock().unwrap().anotar_sha_pendiente("A1", TipoParte::Original, "otro").unwrap();
    assert_eq!(ronda(d.path(), &reg, &mut f).verificadas, 1);
    assert_eq!(s.total_peticiones(), 2);
}

#[test]
fn filtro_por_mes() {
    let (a, b) = (jpeg(100), jpeg(200));
    let s = servidor(vec![("/a", Comportamiento::Servir(a.clone())), ("/b", Comportamiento::Servir(b.clone()))]);
    let mut f = FuenteFalsa {
        assets: vec![
            asset("A1", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &a, s.url("/a"), "JPG")]),
            asset("A2", "IMG_0002", SEP, "PrimarySync", vec![parte(TipoParte::Original, &b, s.url("/b"), "JPG")]),
        ],
        ..Default::default()
    };
    let d = tempfile::tempdir().unwrap();
    assert_eq!(ronda_con(d.path(), &nuevo_registro(), &mut f, 2, Some("2026-08")).verificadas, 1);
    assert_eq!(s.peticiones_a("/b").len(), 0);
}

#[test]
fn un_part_ajeno_ocupa_el_nombre() {
    let j = jpeg(100);
    let s = servidor(vec![("/j", Comportamiento::Servir(j.clone()))]);
    let mut f = FuenteFalsa { assets: vec![asset("A1xxxx", "IMG_0001", AGO, "PrimarySync", vec![parte(TipoParte::Original, &j, s.url("/j"), "JPG")])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let carpeta = d.path().join(r"2026\2026-08");
    std::fs::create_dir_all(&carpeta).unwrap();
    std::fs::write(carpeta.join("IMG_0001.JPG.part"), b"de otro").unwrap();
    assert_eq!(ronda(d.path(), &nuevo_registro(), &mut f).verificadas, 1);
    assert_eq!(std::fs::read(carpeta.join("IMG_0001 (a1xxxx).JPG")).unwrap(), j);
    assert_eq!(std::fs::read(carpeta.join("IMG_0001.JPG.part")).unwrap(), b"de otro");
}

#[test]
fn edicion_nueva_no_reanuda_el_part_viejo() {
    let viejo = heic(3_000_000);
    let mut nuevo = viejo.clone();
    let n = nuevo.len();
    nuevo[n - 1] ^= 0xFF; // mismo tamaño, otro contenido
    let s = servidor(vec![
        ("/v", Comportamiento::CortarPrimeraVez { datos: viejo.clone(), tras: 1_000_000 }),
        ("/n", Comportamiento::Servir(nuevo.clone())),
    ]);
    let mut p = parte(TipoParte::Original, &viejo, s.url("/v"), "HEIC");
    let mut f = FuenteFalsa { assets: vec![asset("A1", "IMG_0001", AGO, "PrimarySync", vec![p.clone()])], ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let reg = nuevo_registro();
    assert_eq!(ronda(d.path(), &reg, &mut f).fallidas, 1);
    p.url = s.url("/n");
    p.checksum = "otro".into();
    f.assets[0].partes = vec![p];
    reg.lock().unwrap().reintentar_ya().unwrap();
    assert_eq!(ronda(d.path(), &reg, &mut f).verificadas, 1);
    assert_eq!(s.peticiones_a("/n")[0].rango, None);
    let final_ = d.path().join(r"2026\2026-08\IMG_0001.HEIC");
    assert_eq!(std::fs::read(&final_).unwrap(), nuevo);
    assert!(!descarga::ruta_meta(&descarga::ruta_part(&final_)).exists());
}
