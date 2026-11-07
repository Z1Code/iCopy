//! Descarga a <archivo>.part reanudando lo que ya hubiera (Range).
use reqwest::blocking::Client;
use reqwest::StatusCode;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorDescarga {
    /// 401/403/404/410: hay que pedir enlaces nuevos a iCloud.
    UrlCaducada(u16),
    Transitorio(String),
    Disco(String),
    Cancelada,
}

impl std::fmt::Display for ErrorDescarga {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorDescarga::UrlCaducada(c) => write!(f, "enlace de descarga caducado (HTTP {c})"),
            ErrorDescarga::Transitorio(m) => write!(f, "fallo de red: {m}"),
            ErrorDescarga::Disco(m) => write!(f, "fallo de disco: {m}"),
            ErrorDescarga::Cancelada => write!(f, "cancelada"),
        }
    }
}

fn disco(e: std::io::Error) -> ErrorDescarga {
    ErrorDescarga::Disco(e.to_string())
}

pub fn cliente() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .user_agent("icopia/0.1")
        .build()
        .expect("no pude crear el cliente HTTP")
}

pub fn ruta_part(final_: &Path) -> PathBuf {
    let mut s = final_.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// `<archivo>.part.meta`: identidad del contenido que se está bajando ("tamaño:checksum").
pub fn ruta_meta(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".meta");
    PathBuf::from(s)
}

/// Borra el `.part` y su `.meta` (lo que no exista se ignora).
pub fn borrar_part(part: &Path) {
    let _ = std::fs::remove_file(part);
    let _ = std::fs::remove_file(ruta_meta(part));
}

fn leer_identidad(part: &Path) -> Option<String> {
    let texto = std::fs::read_to_string(ruta_meta(part)).ok()?;
    texto.lines().next().map(str::to_string)
}

fn escribir_meta(part: &Path, identidad: &str) -> Result<(), ErrorDescarga> {
    let mut f = std::fs::File::create(ruta_meta(part)).map_err(disco)?;
    writeln!(f, "{identidad}").map_err(disco)?;
    f.sync_all().map_err(disco)
}

fn inicio_content_range(v: &str) -> Option<u64> {
    v.strip_prefix("bytes ")?.split('-').next()?.parse().ok()
}

/// Deja en `part` exactamente `tamano` bytes descargados de `url`, reanudando si ya había algo
/// del mismo contenido (`identidad`, anotada en `<part>.meta`); si no, empieza de cero.
pub fn descargar(cliente: &Client, url: &str, part: &Path, tamano: u64, identidad: &str, cancelar: &AtomicBool) -> Result<(), ErrorDescarga> {
    let mut tengo = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mismo_contenido = leer_identidad(part).as_deref() == Some(identidad);
    if part.exists() && (tengo > tamano || !mismo_contenido) {
        std::fs::remove_file(part).map_err(disco)?;
        tengo = 0;
    }
    if !part.exists() {
        escribir_meta(part, identidad)?;
    }
    if tengo == tamano && tamano > 0 {
        return Ok(());
    }
    let segundos = 120 + tamano.saturating_sub(tengo) / (256 * 1024);
    let mut pet = cliente.get(url).timeout(Duration::from_secs(segundos));
    if tengo > 0 {
        pet = pet.header(reqwest::header::RANGE, format!("bytes={tengo}-"));
    }
    let mut resp = pet.send().map_err(|e| ErrorDescarga::Transitorio(format!("conexión: {e}")))?;
    let anexar = match resp.status() {
        StatusCode::PARTIAL_CONTENT => {
            let inicio = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(inicio_content_range);
            if inicio != Some(tengo) {
                borrar_part(part);
                return Err(ErrorDescarga::Transitorio("el servidor devolvió un rango inesperado; se reinicia".into()));
            }
            true
        }
        StatusCode::OK => false,
        StatusCode::RANGE_NOT_SATISFIABLE => {
            borrar_part(part);
            return Err(ErrorDescarga::Transitorio("rango no satisfacible; se reinicia".into()));
        }
        s if s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN || s == StatusCode::NOT_FOUND || s == StatusCode::GONE => {
            return Err(ErrorDescarga::UrlCaducada(s.as_u16()));
        }
        s => return Err(ErrorDescarga::Transitorio(format!("HTTP {}", s.as_u16()))),
    };
    let mut archivo = if anexar {
        OpenOptions::new().append(true).open(part)
    } else {
        OpenOptions::new().create(true).write(true).truncate(true).open(part)
    }
    .map_err(disco)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancelar.load(Ordering::Relaxed) {
            let _ = archivo.sync_all();
            return Err(ErrorDescarga::Cancelada);
        }
        let n = match resp.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                let _ = archivo.sync_all();
                return Err(ErrorDescarga::Transitorio(format!("lectura: {e}")));
            }
        };
        if n == 0 {
            break;
        }
        archivo.write_all(&buf[..n]).map_err(disco)?;
    }
    archivo.sync_all().map_err(disco)?;
    let final_len = std::fs::metadata(part).map_err(disco)?.len();
    if final_len != tamano {
        return Err(ErrorDescarga::Transitorio(format!("descarga incompleta: {final_len} de {tamano} bytes")));
    }
    Ok(())
}
