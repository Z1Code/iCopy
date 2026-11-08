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

/// Nombre del paquete de esta plataforma (macOS llegará en v0.4.0).
pub fn nombre_paquete(v: &Version) -> String {
    format!("iCopy-Setup-{v}.exe")
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
    Some(Publicacion { url_paquete: url(&nombre)?, url_firma: url(&format!("{nombre}.sig"))?, nombre, version })
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
    descargar_con_clave(http, p, dir, CLAVE_PUBLICA)
}

pub fn descargar_con_clave(http: &Client, p: &Publicacion, dir: &Path, clave_publica_b64: &str) -> Result<PathBuf> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let pedir = |url: &str| http.get(url).header(USER_AGENT, format!("iCopy/{VERSION}")).send()?.error_for_status();
    let firma = pedir(&p.url_firma)?.text()?;
    let datos = pedir(&p.url_paquete)?.bytes()?;
    if !firma_valida(&datos, &firma, clave_publica_b64) {
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

/// Lanza el instalador en silencio; quien llama debe cerrar la app justo después
/// (el instalador cierra lo que quede, reemplaza y vuelve a abrir iCopy).
pub fn aplicar(paquete: &Path) -> Result<()> {
    if !cfg!(windows) {
        bail!("actualizar aún no está disponible en este sistema");
    }
    std::process::Command::new(paquete)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .spawn()
        .with_context(|| format!("no pude lanzar {}", paquete.display()))?;
    Ok(())
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

    fn release(tag: &str, assets: &[&str]) -> Value {
        json!({
            "tag_name": tag, "draft": false, "prerelease": false,
            "assets": assets.iter().map(|n| json!({"name": n, "browser_download_url": format!("https://x/{n}")})).collect::<Vec<_>>()
        })
    }

    #[test]
    fn elige_solo_si_es_mas_nueva_y_trae_paquete_y_firma() {
        let r = release("v0.3.0", &["iCopy-Setup-0.3.0.exe", "iCopy-Setup-0.3.0.exe.sig"]);
        let p = elegir(&r, &v("0.2.0")).unwrap();
        assert_eq!(p.version, v("0.3.0"));
        assert_eq!(p.url_paquete, "https://x/iCopy-Setup-0.3.0.exe");
        assert_eq!(p.url_firma, "https://x/iCopy-Setup-0.3.0.exe.sig");
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
