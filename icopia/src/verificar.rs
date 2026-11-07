//! Verificación: tamaño exacto + estructura completa + SHA-256 releído del disco.
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum FalloVerificacion {
    NoExiste,
    Tamano { esperado: u64, real: u64 },
    Estructura(String),
    Lectura(String),
}

impl fmt::Display for FalloVerificacion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FalloVerificacion::NoExiste => write!(f, "el archivo no existe"),
            FalloVerificacion::Tamano { esperado, real } => write!(f, "tamaño {real} bytes, se esperaban {esperado}"),
            FalloVerificacion::Estructura(m) => write!(f, "archivo incompleto o dañado: {m}"),
            FalloVerificacion::Lectura(m) => write!(f, "error de lectura: {m}"),
        }
    }
}

fn lectura(e: std::io::Error) -> FalloVerificacion {
    FalloVerificacion::Lectura(e.to_string())
}

/// Devuelve el SHA-256 en hexadecimal si el archivo pasa todas las comprobaciones.
pub fn verificar_archivo(ruta: &Path, tamano_esperado: u64, extension: &str) -> Result<String, FalloVerificacion> {
    let meta = std::fs::metadata(ruta).map_err(|_| FalloVerificacion::NoExiste)?;
    if meta.len() != tamano_esperado {
        return Err(FalloVerificacion::Tamano { esperado: tamano_esperado, real: meta.len() });
    }
    let mut f = File::open(ruta).map_err(lectura)?;
    comprobar_estructura(&mut f, meta.len(), extension)?;
    f.seek(SeekFrom::Start(0)).map_err(lectura)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(lectura)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn comprobar_estructura<R: Read + Seek>(f: &mut R, len: u64, extension: &str) -> Result<(), FalloVerificacion> {
    match extension.to_ascii_uppercase().as_str() {
        "JPG" | "JPEG" => jpeg(f, len),
        "PNG" => png(f, len),
        "HEIC" | "HEIF" | "MOV" | "MP4" | "M4V" | "AVIF" | "3GP" => isobmff(f, len),
        _ => Ok(()),
    }
}

fn leer_en<R: Read + Seek>(f: &mut R, pos: u64, n: usize) -> Result<Vec<u8>, FalloVerificacion> {
    f.seek(SeekFrom::Start(pos)).map_err(lectura)?;
    let mut v = vec![0u8; n];
    f.read_exact(&mut v).map_err(lectura)?;
    Ok(v)
}

fn jpeg<R: Read + Seek>(f: &mut R, len: u64) -> Result<(), FalloVerificacion> {
    if len < 4 || leer_en(f, 0, 2)? != [0xFF, 0xD8] {
        return Err(FalloVerificacion::Estructura("no empieza como JPEG".into()));
    }
    let cola = len.min(4096);
    let fin = leer_en(f, len - cola, cola as usize)?;
    if fin.windows(2).any(|w| w == [0xFF, 0xD9]) {
        Ok(())
    } else {
        Err(FalloVerificacion::Estructura("JPEG sin marcador de fin".into()))
    }
}

fn png<R: Read + Seek>(f: &mut R, len: u64) -> Result<(), FalloVerificacion> {
    const FIRMA: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
    if len < 20 || leer_en(f, 0, 8)? != FIRMA {
        return Err(FalloVerificacion::Estructura("no empieza como PNG".into()));
    }
    if leer_en(f, len - 12, 12)? != IEND {
        return Err(FalloVerificacion::Estructura("PNG sin bloque IEND final".into()));
    }
    Ok(())
}

fn isobmff<R: Read + Seek>(f: &mut R, len: u64) -> Result<(), FalloVerificacion> {
    let mut pos = 0u64;
    let mut primera = true;
    while pos < len {
        if len - pos < 8 {
            return Err(FalloVerificacion::Estructura(format!("caja truncada en el byte {pos}")));
        }
        let h = leer_en(f, pos, 8)?;
        let tam32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
        let tipo = &h[4..8];
        if !tipo.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
            return Err(FalloVerificacion::Estructura(format!("tipo de caja inválido en el byte {pos}")));
        }
        if primera && !matches!(tipo, b"ftyp" | b"wide" | b"free" | b"skip" | b"mdat" | b"moov" | b"pnot") {
            return Err(FalloVerificacion::Estructura("no empieza como HEIC/MOV/MP4".into()));
        }
        primera = false;
        let tam = match tam32 {
            0 => len - pos,
            1 => {
                if len - pos < 16 {
                    return Err(FalloVerificacion::Estructura(format!("caja grande truncada en el byte {pos}")));
                }
                let g = leer_en(f, pos + 8, 8)?;
                u64::from_be_bytes([g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7]])
            }
            n => n as u64,
        };
        if tam < 8 {
            return Err(FalloVerificacion::Estructura(format!("tamaño de caja inválido en el byte {pos}")));
        }
        pos = pos.checked_add(tam).ok_or_else(|| FalloVerificacion::Estructura("tamaño de caja desbordado".into()))?;
    }
    if pos != len {
        return Err(FalloVerificacion::Estructura(format!("el archivo está cortado: la última caja termina en {pos} y el archivo mide {len}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn caja(tipo: &[u8; 4], contenido: &[u8]) -> Vec<u8> {
        let mut v = ((8 + contenido.len()) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(tipo);
        v.extend_from_slice(contenido);
        v
    }

    fn heic() -> Vec<u8> {
        let mut v = caja(b"ftyp", b"heic\0\0\0\0mif1heic");
        v.extend(caja(b"mdat", &[7u8; 100]));
        v
    }

    fn est(datos: &[u8], ext: &str) -> Result<(), FalloVerificacion> {
        comprobar_estructura(&mut Cursor::new(datos), datos.len() as u64, ext)
    }

    #[test]
    fn isobmff_completo_y_cortado() {
        let v = heic();
        assert_eq!(est(&v, "HEIC"), Ok(()));
        assert!(matches!(est(&v[..v.len() - 10], "HEIC"), Err(FalloVerificacion::Estructura(_))));
        assert!(matches!(est(&v[..5], "MOV"), Err(FalloVerificacion::Estructura(_))));
    }

    #[test]
    fn isobmff_caja_de_tamano_cero_llega_al_final() {
        let mut v = caja(b"ftyp", b"qt  \0\0\0\0");
        v.extend_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(b"mdat");
        v.extend_from_slice(&[1u8; 50]);
        assert_eq!(est(&v, "MOV"), Ok(()));
    }

    #[test]
    fn jpeg_y_png() {
        let jpg = [0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9];
        assert_eq!(est(&jpg, "jpg"), Ok(()));
        assert!(est(&jpg[..5], "JPEG").is_err());
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        png.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]);
        assert_eq!(est(&png, "PNG"), Ok(()));
        assert!(est(&png[..png.len() - 1], "PNG").is_err());
    }

    #[test]
    fn otras_extensiones_solo_tamano() {
        assert_eq!(est(b"lo que sea", "DNG"), Ok(()));
    }

    #[test]
    fn verificar_archivo_devuelve_sha256() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("a.txt");
        std::fs::write(&r, b"abc").unwrap();
        assert_eq!(
            verificar_archivo(&r, 3, "TXT").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(verificar_archivo(&r, 4, "TXT"), Err(FalloVerificacion::Tamano { esperado: 4, real: 3 }));
        assert_eq!(verificar_archivo(&d.path().join("no"), 3, "TXT"), Err(FalloVerificacion::NoExiste));
    }
}
