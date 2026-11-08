mod comun;

use base64::{engine::general_purpose::STANDARD, Engine};
use comun::{patron, servidor, Comportamiento};
use icopia::actualizar::{clave_publica_de, consultar, descargar_con_clave, firmar, Publicacion};
use semver::Version;

fn clave() -> String {
    let mut s = [0u8; 32];
    getrandom::getrandom(&mut s).unwrap();
    STANDARD.encode(s)
}

fn publicacion(base: &comun::Servidor) -> Publicacion {
    Publicacion {
        version: Version::parse("9.9.9").unwrap(),
        nombre: "iCopy-Setup-9.9.9.exe".into(),
        url_paquete: base.url("/p.exe"),
        url_firma: base.url("/p.exe.sig"),
    }
}

#[test]
fn descarga_y_deja_el_paquete_si_la_firma_es_buena() {
    let privada = clave();
    let datos = patron(5000);
    let firma = firmar(&privada, &datos).unwrap();
    let srv = servidor(vec![("/p.exe", Comportamiento::Servir(datos.clone())), ("/p.exe.sig", Comportamiento::Servir(firma.into_bytes()))]);
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let ruta = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap()).unwrap();
    assert_eq!(ruta, carpeta.join("iCopy-Setup-9.9.9.exe"));
    assert_eq!(std::fs::read(&ruta).unwrap(), datos);
    assert!(std::fs::read_dir(&carpeta).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".part")));
}

#[test]
fn firma_mala_no_deja_nada() {
    let privada = clave();
    let datos = patron(5000);
    let firma_de_otro_contenido = firmar(&privada, b"otra cosa").unwrap();
    let srv = servidor(vec![("/p.exe", Comportamiento::Servir(datos)), ("/p.exe.sig", Comportamiento::Servir(firma_de_otro_contenido.into_bytes()))]);
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert_eq!(std::fs::read_dir(&carpeta).unwrap().count(), 0, "no queda ningún archivo instalable");
}

#[test]
fn paquete_que_falla_no_deja_nada() {
    let privada = clave();
    let firma = firmar(&privada, &patron(10)).unwrap();
    let srv = servidor(vec![("/p.exe", Comportamiento::Estado(500)), ("/p.exe.sig", Comportamiento::Servir(firma.into_bytes()))]);
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert_eq!(std::fs::read_dir(&carpeta).unwrap().count(), 0);
}

#[test]
fn consultar_lee_la_api() {
    let json = serde_json::json!({
        "tag_name": "v9.9.9", "draft": false, "prerelease": false,
        "assets": [
            {"name": "iCopy-Setup-9.9.9.exe", "browser_download_url": "http://x/p.exe"},
            {"name": "iCopy-Setup-9.9.9.exe.sig", "browser_download_url": "http://x/p.exe.sig"}
        ]
    });
    let srv = servidor(vec![("/latest", Comportamiento::Servir(json.to_string().into_bytes()))]);
    // Solo esta prueba toca la variable: las demás no consultan la API.
    std::env::set_var("ICOPIA_URL_ACTUALIZACIONES", srv.url("/latest"));
    let p = consultar(&reqwest::blocking::Client::new()).unwrap().expect("hay una versión más nueva");
    assert_eq!(p.version, Version::parse("9.9.9").unwrap());
    assert_eq!(p.url_paquete, "http://x/p.exe");
}
