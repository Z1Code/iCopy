//! Actualizaciones desde GitHub Releases. Cada paquete lleva una firma ed25519 propia (`.sig`, base64)
//! que se comprueba con la clave pública compilada en la app: sin firma válida no se instala nada.
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, USER_AGENT};
use semver::Version;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const URL_API: &str = "https://api.github.com/repos/Z1Code/iCopy/releases/latest";
const CLAVE_PUBLICA: &str = include_str!("../recursos/clave_actualizaciones.pub");

#[derive(Debug, Clone, PartialEq)]
pub struct Publicacion {
    pub version: Version,
    pub nombre: String,
    pub url_paquete: String,
    pub url_firma: String,
}

/// `ICOPIA_URL_ACTUALIZACIONES` permite probar contra un servidor local.
pub fn url_api() -> String {
    std::env::var("ICOPIA_URL_ACTUALIZACIONES").unwrap_or_else(|_| URL_API.into())
}

/// Nombre del paquete de esta plataforma.
pub fn nombre_paquete(v: &Version) -> String {
    nombre_paquete_para(std::env::consts::OS, v)
}

/// Windows: el instalador; macOS: el zip de `iCopy.app` (solo Apple Silicon).
pub fn nombre_paquete_para(so: &str, v: &Version) -> String {
    match so {
        "macos" => format!("iCopy-{v}-macos-arm64.zip"),
        _ => format!("iCopy-Setup-{v}.exe"),
    }
}

/// La publicación de `json` (respuesta de `releases/latest`) si es estable, más nueva que `actual`
/// y trae el paquete de esta plataforma y su firma.
pub fn elegir(json: &Value, actual: &Version) -> Option<Publicacion> {
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let version = Version::parse(json["tag_name"].as_str()?.trim_start_matches('v')).ok()?;
    if !version.pre.is_empty() || version <= *actual {
        return None;
    }
    let nombre = nombre_paquete(&version);
    let url = |n: &str| -> Option<String> {
        json["assets"].as_array()?.iter().find(|a| a["name"].as_str() == Some(n))?["browser_download_url"]
            .as_str()
            .map(String::from)
    };
    Some(Publicacion { url_paquete: url(&nombre)?, url_firma: url(&format!("{nombre}.sig2"))?, nombre, version })
}

/// Firma v2 (`.sig2`): cubre también el nombre del paquete, que lleva la versión y el sistema. Así no
/// se puede servir un instalador antiguo auténtico (ni el de otro sistema) como si fuera el nuevo.
/// La v1 (`.sig`, solo los bytes) se sigue publicando para que las 0.2.0 instaladas puedan actualizarse.
const CONTEXTO_V2: &[u8] = b"iCopy-paquete-v2\0";

fn mensaje_v2(nombre: &str, datos: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(CONTEXTO_V2.len() + nombre.len() + 1 + datos.len());
    m.extend_from_slice(CONTEXTO_V2);
    m.extend_from_slice(nombre.as_bytes());
    m.push(0);
    m.extend_from_slice(datos);
    m
}

pub fn firmar_paquete(semilla_b64: &str, nombre: &str, datos: &[u8]) -> Result<String> {
    firmar(semilla_b64, &mensaje_v2(nombre, datos))
}

pub fn firma_paquete_valida(datos: &[u8], nombre: &str, firma_b64: &str, clave_publica_b64: &str) -> bool {
    firma_valida(&mensaje_v2(nombre, datos), firma_b64, clave_publica_b64)
}

/// Clave pública de las actualizaciones incluida en la app (base64).
pub fn clave_publica() -> &'static str {
    CLAVE_PUBLICA
}

fn semilla(semilla_b64: &str) -> Result<SigningKey> {
    let bytes: [u8; 32] = STANDARD
        .decode(semilla_b64.trim())
        .context("clave privada: base64 inválido")?
        .try_into()
        .map_err(|_| anyhow!("la clave privada debe tener 32 bytes"))?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Firma (base64) de `datos` con la clave privada `semilla_b64` (32 bytes en base64).
pub fn firmar(semilla_b64: &str, datos: &[u8]) -> Result<String> {
    Ok(STANDARD.encode(semilla(semilla_b64)?.sign(datos).to_bytes()))
}

/// Clave pública (base64) que corresponde a una clave privada.
pub fn clave_publica_de(semilla_b64: &str) -> Result<String> {
    Ok(STANDARD.encode(semilla(semilla_b64)?.verifying_key().to_bytes()))
}

pub fn firma_valida(datos: &[u8], firma_b64: &str, clave_publica_b64: &str) -> bool {
    let clave = STANDARD.decode(clave_publica_b64.trim()).ok().and_then(|b| <[u8; 32]>::try_from(b).ok());
    let firma = STANDARD.decode(firma_b64.trim()).ok().and_then(|b| <[u8; 64]>::try_from(b).ok());
    let (Some(clave), Some(firma)) = (clave, firma) else { return false };
    let Ok(clave) = VerifyingKey::from_bytes(&clave) else { return false };
    clave.verify_strict(datos, &Signature::from_bytes(&firma)).is_ok()
}

/// ¿Hay una versión más nueva publicada?
pub fn consultar(http: &Client) -> Result<Option<Publicacion>> {
    let texto = http
        .get(url_api())
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, format!("iCopy/{VERSION}"))
        .send()?
        .error_for_status()?
        .text()?;
    let json: Value = serde_json::from_str(&texto).context("respuesta de GitHub inválida")?;
    Ok(elegir(&json, &Version::parse(VERSION)?))
}

/// Descarga paquete y firma a `dir`. El paquete solo queda en disco (con su nombre final) si la
/// firma es válida; si no, error y nada que instalar. Borra descargas anteriores de esa carpeta.
pub fn descargar(http: &Client, p: &Publicacion, dir: &Path) -> Result<PathBuf> {
    descargar_con(http, p, dir, CLAVE_PUBLICA, MAX_PAQUETE)
}

/// Tamaño máximo de un paquete (el instalador pesa unos 25 MB): un servidor hostil no puede agotar la memoria.
pub const MAX_PAQUETE: u64 = 300 * 1024 * 1024;

pub fn descargar_con_clave(http: &Client, p: &Publicacion, dir: &Path, clave_publica_b64: &str) -> Result<PathBuf> {
    descargar_con(http, p, dir, clave_publica_b64, MAX_PAQUETE)
}

pub fn descargar_con(http: &Client, p: &Publicacion, dir: &Path, clave_publica_b64: &str, max_bytes: u64) -> Result<PathBuf> {
    use std::io::Read;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let pedir = |url: &str| http.get(url).header(USER_AGENT, format!("iCopy/{VERSION}")).send()?.error_for_status();
    let firma = pedir(&p.url_firma)?.text()?;
    let respuesta = pedir(&p.url_paquete)?;
    if respuesta.content_length().is_some_and(|n| n > max_bytes) {
        bail!("{} es demasiado grande: no se descarga", p.nombre);
    }
    let mut datos = Vec::new();
    respuesta.take(max_bytes + 1).read_to_end(&mut datos)?;
    if datos.len() as u64 > max_bytes {
        bail!("{} es demasiado grande: no se instala", p.nombre);
    }
    if !firma_paquete_valida(&datos, &p.nombre, &firma, clave_publica_b64) {
        bail!("la firma de {} no es válida: no se instala", p.nombre);
    }
    let destino = dir.join(&p.nombre);
    let parcial = dir.join(format!("{}.part", p.nombre));
    std::fs::write(&parcial, &datos)?;
    std::fs::rename(&parcial, &destino)?;
    Ok(destino)
}

/// ¿Se puede instalar ya sin interrumpir nada importante?
#[derive(Debug, Clone, Copy, Default)]
pub struct Momento {
    pub copiando: bool,
    pub pausado: bool,
    pub liberando: bool,
    pub ventana_abierta: bool,
}

pub fn momento_tranquilo(m: Momento) -> bool {
    !m.copiando && !m.pausado && !m.liberando && !m.ventana_abierta
}

/// Lanza el instalador en silencio (con su log junto al paquete) y devuelve su proceso. Quien llama
/// NO debe cerrarse: el instalador cierra iCopy, reemplaza los archivos y la vuelve a abrir. Si el
/// instalador termina con error, la app sigue viva y puede avisar.
#[cfg(windows)]
pub fn aplicar(paquete: &Path) -> Result<std::process::Child> {
    let log = paquete.with_file_name("instalacion.log");
    std::process::Command::new(paquete)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .arg(format!("/LOG={}", log.display()))
        .spawn()
        .with_context(|| format!("no pude lanzar {}", paquete.display()))
}

/// Actualizar en macOS: no hay instalador; la app sustituye su propio bundle por el del zip (ya
/// verificado con la firma v2), comprueba que está firmado por Apple con nuestro Team ID y se
/// relanza al salir.
pub mod macos {
    use anyhow::{bail, Context, Result};
    use std::path::{Path, PathBuf};

    /// Equipo de Apple Developer de iCopy (el Team ID es público: va en cualquier app firmada).
    pub const EQUIPO_APPLE: &str = "YYNWA2QYGF";
    const VIEJA: &str = ".iCopy-vieja.app";

    /// La carpeta `*.app` que contiene `exe` (p. ej. `/Applications/iCopy.app`), si la hay.
    pub fn bundle_de(exe: &Path) -> Option<PathBuf> {
        exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
    }

    /// Firma de Apple válida y de nuestro equipo.
    pub fn verificar_firma_apple(app: &Path) -> Result<()> {
        let ok = std::process::Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(app).status()?;
        if !ok.success() {
            bail!("la firma de Apple de la versión nueva no es válida");
        }
        let info = std::process::Command::new("codesign").arg("-dv").arg(app).output()?;
        let texto = String::from_utf8_lossy(&info.stderr);
        if !texto.lines().any(|l| l.trim() == format!("TeamIdentifier={EQUIPO_APPLE}")) {
            bail!("la versión nueva no está firmada por el equipo de iCopy");
        }
        Ok(())
    }

    /// Sustituye `actual` (un `iCopy.app`) por el que trae `zip`. La anterior queda en
    /// `.iCopy-vieja.app` (se borra al arrancar la nueva); si algo falla, `actual` sigue intacto.
    pub fn sustituir_bundle(actual: &Path, zip: &Path, verificar: &dyn Fn(&Path) -> Result<()>) -> Result<()> {
        let padre = actual.parent().context("el bundle no tiene carpeta padre")?;
        let tmp = padre.join(format!(".iCopy-nueva-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).with_context(|| format!("no se puede escribir en {}", padre.display()))?;
        let r = (|| -> Result<()> {
            let ok = std::process::Command::new("ditto").args(["-x", "-k"]).arg(zip).arg(&tmp).status()?;
            if !ok.success() {
                bail!("no se pudo descomprimir la versión nueva");
            }
            let nueva = tmp.join("iCopy.app");
            if !nueva.is_dir() {
                bail!("el paquete no contiene iCopy.app");
            }
            verificar(&nueva)?;
            let vieja = padre.join(VIEJA);
            let _ = std::fs::remove_dir_all(&vieja);
            std::fs::rename(actual, &vieja).context("no se pudo apartar la versión actual")?;
            if let Err(e) = std::fs::rename(&nueva, actual) {
                let _ = std::fs::rename(&vieja, actual); // deshacer: la actual vuelve a su sitio
                return Err(e).context("no se pudo colocar la versión nueva");
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        r
    }

    /// Al arrancar: borra la versión anterior que dejó una actualización.
    pub fn limpiar_vieja(bundle: &Path) {
        if let Some(padre) = bundle.parent() {
            let _ = std::fs::remove_dir_all(padre.join(VIEJA));
        }
    }

    /// Lanza un proceso que espera a que este termine y vuelve a abrir `bundle`.
    pub fn relanzar_al_salir(bundle: &Path) -> Result<()> {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.5; done; open \"$2\"")
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(bundle)
            .spawn()
            .context("no pude programar el reinicio")?;
        Ok(())
    }

    /// Todo junto: sustituye el bundle en el que corre la app y programa el reinicio. Si va bien,
    /// quien llama debe salir de la app enseguida.
    pub fn aplicar(zip: &Path) -> Result<()> {
        let bundle = bundle_de(&std::env::current_exe()?).context("iCopy no se está ejecutando desde iCopy.app")?;
        sustituir_bundle(&bundle, zip, &verificar_firma_apple)?;
        relanzar_al_salir(&bundle)
    }

    #[cfg(test)]
    mod pruebas {
        use super::*;

        #[test]
        fn bundle_de_un_ejecutable() {
            let exe = Path::new("/Applications/iCopy.app/Contents/MacOS/icopia-bandeja");
            assert_eq!(bundle_de(exe).as_deref(), Some(Path::new("/Applications/iCopy.app")));
            assert_eq!(bundle_de(Path::new("/x/target/release/icopia-bandeja")), None);
        }

        #[cfg(target_os = "macos")]
        fn bundle_falso(dir: &Path, version: &str) -> PathBuf {
            let app = dir.join("iCopy.app");
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            std::fs::write(app.join("Contents").join("version.txt"), version).unwrap();
            app
        }

        #[cfg(target_os = "macos")]
        fn zip_de(app: &Path, zip: &Path) {
            let ok = std::process::Command::new("ditto").args(["-c", "-k", "--keepParent"]).arg(app).arg(zip).status().unwrap();
            assert!(ok.success());
        }

        #[cfg(target_os = "macos")]
        fn version(app: &Path) -> String {
            std::fs::read_to_string(app.join("Contents").join("version.txt")).unwrap()
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn sustituye_y_guarda_la_vieja() {
            let d = tempfile::tempdir().unwrap();
            let nueva_dir = d.path().join("nueva");
            let zip = d.path().join("nueva.zip");
            zip_de(&bundle_falso(&nueva_dir, "2"), &zip);
            let instalada = d.path().join("apps");
            let actual = bundle_falso(&instalada, "1");
            sustituir_bundle(&actual, &zip, &|_| Ok(())).unwrap();
            assert_eq!(version(&actual), "2");
            assert_eq!(version(&instalada.join(VIEJA)), "1");
            assert_eq!(std::fs::read_dir(&instalada).unwrap().count(), 2, "sin temporales sueltos");
            limpiar_vieja(&actual);
            assert!(!instalada.join(VIEJA).exists());
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn verificacion_fallida_no_toca_nada() {
            let d = tempfile::tempdir().unwrap();
            let zip = d.path().join("nueva.zip");
            zip_de(&bundle_falso(&d.path().join("nueva"), "2"), &zip);
            let instalada = d.path().join("apps");
            let actual = bundle_falso(&instalada, "1");
            assert!(sustituir_bundle(&actual, &zip, &|_| bail!("firma mala")).is_err());
            assert_eq!(version(&actual), "1");
            assert_eq!(std::fs::read_dir(&instalada).unwrap().count(), 1);
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn una_app_sin_firma_de_nuestro_equipo_se_rechaza() {
            let d = tempfile::tempdir().unwrap();
            let app = bundle_falso(d.path(), "1");
            assert!(verificar_firma_apple(&app).is_err());
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn zip_sin_icopy_app_se_rechaza() {
            let d = tempfile::tempdir().unwrap();
            let otro = d.path().join("Otra.app");
            std::fs::create_dir_all(&otro).unwrap();
            let zip = d.path().join("x.zip");
            zip_de(&otro, &zip);
            let instalada = d.path().join("apps");
            let actual = bundle_falso(&instalada, "1");
            assert!(sustituir_bundle(&actual, &zip, &|_| Ok(())).is_err());
            assert_eq!(version(&actual), "1");
        }
    }
}

/// Las compilaciones de desarrollo (debug o ejecutadas desde `target\`) no se actualizan solas,
/// salvo que se apunte a un servidor de pruebas con `ICOPIA_URL_ACTUALIZACIONES`.
pub fn permitidas() -> bool {
    if std::env::var_os("ICOPIA_URL_ACTUALIZACIONES").is_some() {
        return true;
    }
    let desde_target = std::env::current_exe()
        .map(|e| e.components().any(|c| c.as_os_str().eq_ignore_ascii_case("target")))
        .unwrap_or(true);
    !cfg!(debug_assertions) && !desde_target
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use serde_json::json;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// Las pruebas escriben los nombres de Windows (`iCopy-Setup-X.exe[.sig…]`); aquí se cambian por
    /// los del sistema en el que corren, para que valgan también en macOS.
    fn de_este_sistema(n: &str) -> String {
        let Some(resto) = n.strip_prefix("iCopy-Setup-") else { return n.to_string() };
        let Some(fin) = resto.find(".exe") else { return n.to_string() };
        format!("{}{}", nombre_paquete(&v(&resto[..fin])), &resto[fin + 4..])
    }

    fn release(tag: &str, assets: &[&str]) -> Value {
        json!({
            "tag_name": tag, "draft": false, "prerelease": false,
            "assets": assets.iter().map(|n| {
                let n = de_este_sistema(n);
                json!({"name": n, "browser_download_url": format!("https://x/{n}")})
            }).collect::<Vec<_>>()
        })
    }

    #[test]
    fn nombres_de_paquete_por_sistema() {
        assert_eq!(nombre_paquete_para("windows", &v("0.3.0")), "iCopy-Setup-0.3.0.exe");
        assert_eq!(nombre_paquete_para("macos", &v("0.3.0")), "iCopy-0.3.0-macos-arm64.zip");
    }

    #[test]
    fn elige_solo_si_es_mas_nueva_y_trae_paquete_y_firma() {
        let r = release("v0.3.0", &["iCopy-Setup-0.3.0.exe", "iCopy-Setup-0.3.0.exe.sig2"]);
        let p = elegir(&r, &v("0.2.0")).unwrap();
        assert_eq!(p.version, v("0.3.0"));
        assert_eq!(p.url_paquete, format!("https://x/{}", nombre_paquete(&v("0.3.0"))));
        assert_eq!(p.url_firma, format!("https://x/{}.sig2", nombre_paquete(&v("0.3.0"))));
        assert!(elegir(&r, &v("0.3.0")).is_none(), "misma versión");
        assert!(elegir(&r, &v("0.4.0")).is_none(), "nunca baja de versión");
    }

    #[test]
    fn sin_firma_o_sin_paquete_no_hay_actualizacion() {
        assert!(elegir(&release("v0.3.0", &["iCopy-Setup-0.3.0.exe"]), &v("0.2.0")).is_none());
        assert!(elegir(&release("v0.3.0", &["iCopy-Setup-0.3.0.exe.sig"]), &v("0.2.0")).is_none());
    }

    #[test]
    fn ignora_preversiones_borradores_y_etiquetas_raras() {
        let mut r = release("v0.3.0", &["iCopy-Setup-0.3.0.exe", "iCopy-Setup-0.3.0.exe.sig"]);
        r["prerelease"] = json!(true);
        assert!(elegir(&r, &v("0.2.0")).is_none());
        r["prerelease"] = json!(false);
        r["draft"] = json!(true);
        assert!(elegir(&r, &v("0.2.0")).is_none());
        let rc = release("v0.3.0-rc.1", &["iCopy-Setup-0.3.0-rc.1.exe", "iCopy-Setup-0.3.0-rc.1.exe.sig"]);
        assert!(elegir(&rc, &v("0.2.0")).is_none());
        assert!(elegir(&release("ultima", &[]), &v("0.2.0")).is_none());
    }

    fn clave_de_prueba() -> String {
        let mut s = [0u8; 32];
        getrandom::getrandom(&mut s).unwrap();
        STANDARD.encode(s)
    }

    #[test]
    fn firma_v2_liga_nombre() {
        let privada = clave_de_prueba();
        let publica = clave_publica_de(&privada).unwrap();
        let firma = firmar_paquete(&privada, "iCopy-Setup-0.3.0.exe", b"x").unwrap();
        assert!(firma_paquete_valida(b"x", "iCopy-Setup-0.3.0.exe", &firma, &publica));
        assert!(!firma_paquete_valida(b"x", "iCopy-Setup-0.2.0.exe", &firma, &publica), "otro nombre");
        assert!(!firma_paquete_valida(b"y", "iCopy-Setup-0.3.0.exe", &firma, &publica), "otros bytes");
    }

    #[test]
    fn firma_v1_no_vale_como_v2() {
        let privada = clave_de_prueba();
        let publica = clave_publica_de(&privada).unwrap();
        let v1 = firmar(&privada, b"x").unwrap();
        assert!(!firma_paquete_valida(b"x", "iCopy-Setup-0.3.0.exe", &v1, &publica));
    }

    #[test]
    fn elegir_exige_sig2() {
        let sin = release("v0.3.0", &["iCopy-Setup-0.3.0.exe", "iCopy-Setup-0.3.0.exe.sig"]);
        assert!(elegir(&sin, &v("0.2.0")).is_none());
        let con = release("v0.3.0", &["iCopy-Setup-0.3.0.exe", "iCopy-Setup-0.3.0.exe.sig", "iCopy-Setup-0.3.0.exe.sig2"]);
        assert!(elegir(&con, &v("0.2.0")).unwrap().url_firma.ends_with(".sig2"));
    }

    #[test]
    fn firma_y_verificacion() {
        let privada = clave_de_prueba();
        let publica = clave_publica_de(&privada).unwrap();
        let firma = firmar(&privada, b"instalador").unwrap();
        assert!(firma_valida(b"instalador", &firma, &publica));
        assert!(!firma_valida(b"instalador alterado", &firma, &publica));
        assert!(!firma_valida(b"instalador", &firma, &clave_publica_de(&clave_de_prueba()).unwrap()), "otra clave");
        assert!(!firma_valida(b"instalador", "basura", &publica));
        assert!(!firma_valida(b"instalador", &firma, "PROVISIONAL"));
    }

    #[test]
    fn momento_tranquilo_solo_sin_nada_en_marcha() {
        assert!(momento_tranquilo(Momento::default()));
        for m in [
            Momento { copiando: true, ..Default::default() },
            Momento { pausado: true, ..Default::default() },
            Momento { liberando: true, ..Default::default() },
            Momento { ventana_abierta: true, ..Default::default() },
        ] {
            assert!(!momento_tranquilo(m));
        }
    }
}
