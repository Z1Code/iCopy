mod comun;

use base64::{engine::general_purpose::STANDARD, Engine};
use comun::{patron, servidor, Comportamiento};
use icopia::actualizar::{clave_publica_de, consultar, descargar_con, descargar_con_clave, firmar, firmar_paquete, Publicacion};
use semver::Version;

const NOMBRE: &str = "iCopy-Setup-9.9.9.exe";

fn clave() -> String {
    let mut s = [0u8; 32];
    getrandom::getrandom(&mut s).unwrap();
    STANDARD.encode(s)
}

fn publicacion(base: &comun::Servidor) -> Publicacion {
    Publicacion {
        version: Version::parse("9.9.9").unwrap(),
        nombre: NOMBRE.into(),
        url_paquete: base.url("/p.exe"),
        url_firma: base.url("/p.exe.sig2"),
    }
}

fn servir(datos: Vec<u8>, firma: String) -> comun::Servidor {
    servidor(vec![("/p.exe", Comportamiento::Servir(datos)), ("/p.exe.sig2", Comportamiento::Servir(firma.into_bytes()))])
}

fn vacia(carpeta: &std::path::Path) -> bool {
    std::fs::read_dir(carpeta).unwrap().count() == 0
}

#[test]
fn descarga_y_deja_el_paquete_si_la_firma_es_buena() {
    let privada = clave();
    let datos = patron(5000);
    let srv = servir(datos.clone(), firmar_paquete(&privada, NOMBRE, &datos).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let ruta = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap()).unwrap();
    assert_eq!(ruta, carpeta.join(NOMBRE));
    assert_eq!(std::fs::read(&ruta).unwrap(), datos);
    assert!(std::fs::read_dir(&carpeta).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".part")));
}

#[test]
fn firma_mala_no_deja_nada() {
    let privada = clave();
    let srv = servir(patron(5000), firmar_paquete(&privada, NOMBRE, b"otra cosa").unwrap());
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert!(vacia(&carpeta), "no queda ningún archivo instalable");
}

#[test]
fn firma_v2_de_otro_nombre_no_deja_nada() {
    // Un instalador antiguo auténtico servido como si fuera la 9.9.9: su firma es de otro nombre.
    let privada = clave();
    let datos = patron(5000);
    let srv = servir(datos.clone(), firmar_paquete(&privada, "iCopy-Setup-0.2.0.exe", &datos).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert!(vacia(&carpeta));
}

#[test]
fn firma_v1_no_basta() {
    let privada = clave();
    let datos = patron(5000);
    let srv = servir(datos.clone(), firmar(&privada, &datos).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert!(vacia(&carpeta));
}

#[test]
fn paquete_demasiado_grande_se_rechaza() {
    let privada = clave();
    let datos = patron(5000);
    let srv = servir(datos.clone(), firmar_paquete(&privada, NOMBRE, &datos).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap(), 1000);
    assert!(r.is_err());
    assert!(vacia(&carpeta));
}

#[test]
fn paquete_que_falla_no_deja_nada() {
    let privada = clave();
    let firma = firmar_paquete(&privada, NOMBRE, &patron(10)).unwrap();
    let srv = servidor(vec![("/p.exe", Comportamiento::Estado(500)), ("/p.exe.sig2", Comportamiento::Servir(firma.into_bytes()))]);
    let dir = tempfile::tempdir().unwrap();
    let carpeta = dir.path().join("act");
    let r = descargar_con_clave(&reqwest::blocking::Client::new(), &publicacion(&srv), &carpeta, &clave_publica_de(&privada).unwrap());
    assert!(r.is_err());
    assert!(vacia(&carpeta));
}

#[test]
fn consultar_lee_la_api() {
    // El paquete de este sistema (instalador en Windows, zip en macOS).
    let paquete = icopia::actualizar::nombre_paquete(&Version::parse("9.9.9").unwrap());
    let json = serde_json::json!({
        "tag_name": "v9.9.9", "draft": false, "prerelease": false,
        "assets": [
            {"name": paquete, "browser_download_url": "http://x/p.exe"},
            {"name": format!("{paquete}.sig2"), "browser_download_url": "http://x/p.exe.sig2"}
        ]
    });
    let srv = servidor(vec![("/latest", Comportamiento::Servir(json.to_string().into_bytes()))]);
    // Solo esta prueba toca la variable: las demás no consultan la API.
    std::env::set_var("ICOPIA_URL_ACTUALIZACIONES", srv.url("/latest"));
    let p = consultar(&reqwest::blocking::Client::new()).unwrap().expect("hay una versión más nueva");
    assert_eq!(p.version, Version::parse("9.9.9").unwrap());
    assert_eq!(p.url_paquete, "http://x/p.exe");
}

/// Contra GitHub de verdad (redirecciones a objects.githubusercontent.com incluidas). Manual:
/// `cargo test --test actualizar -- --ignored github_real`
#[test]
#[ignore]
fn github_real_descarga_y_verifica_v0_2_0() {
    use icopia::actualizar::{clave_publica, firma_valida};
    let http = reqwest::blocking::Client::builder().user_agent("iCopy-pruebas").build().unwrap();
    let texto = http
        .get("https://api.github.com/repos/Z1Code/iCopy/releases/tags/v0.2.0")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&texto).unwrap();
    let url = |n: &str| {
        json["assets"].as_array().unwrap().iter().find(|a| a["name"] == n).unwrap()["browser_download_url"].as_str().unwrap().to_string()
    };
    // La 0.2.0 solo tiene la firma v1 (`.sig`).
    let datos = http.get(url("iCopy-Setup-0.2.0.exe")).send().unwrap().error_for_status().unwrap().bytes().unwrap();
    let firma = http.get(url("iCopy-Setup-0.2.0.exe.sig")).send().unwrap().error_for_status().unwrap().text().unwrap();
    assert!(datos.len() > 1_000_000);
    assert!(firma_valida(&datos, &firma, clave_publica()));
}
