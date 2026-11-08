//! Firma un paquete para las actualizaciones. Uso: firmar <archivo>
//! La clave privada (base64) sale de la variable de entorno `ICOPY_CLAVE_FIRMA`. Escribe dos firmas:
//! `<archivo>.sig` (v1, solo los bytes), la que comprueban las 0.2.0 ya instaladas, y
//! `<archivo>.sig2` (v2, bytes + nombre del archivo), la que exigen las versiones nuevas.
//! Antes de escribirlas comprueba que la clave pública incluida en la app las acepta: así nunca se
//! publica algo que las instalaciones rechazarían.
use icopia::actualizar::{clave_publica, firma_paquete_valida, firma_valida, firmar, firmar_paquete};
use std::path::Path;

fn main() {
    let Some(ruta) = std::env::args().nth(1) else {
        eprintln!("Uso: firmar <archivo>");
        std::process::exit(2);
    };
    let Ok(privada) = std::env::var("ICOPY_CLAVE_FIRMA") else {
        eprintln!("Falta la variable ICOPY_CLAVE_FIRMA (clave privada en base64)");
        std::process::exit(1);
    };
    let datos = std::fs::read(&ruta).unwrap_or_else(|e| panic!("no puedo leer {ruta}: {e}"));
    let nombre = Path::new(&ruta).file_name().and_then(|n| n.to_str()).expect("nombre de archivo válido");
    let v1 = firmar(&privada, &datos).expect("firmar v1");
    let v2 = firmar_paquete(&privada, nombre, &datos).expect("firmar v2");
    if !firma_valida(&datos, &v1, clave_publica()) || !firma_paquete_valida(&datos, nombre, &v2, clave_publica()) {
        eprintln!("La clave privada no corresponde a recursos/clave_actualizaciones.pub: no se escribe ninguna firma");
        std::process::exit(1);
    }
    std::fs::write(format!("{ruta}.sig"), &v1).expect("escribir .sig");
    std::fs::write(format!("{ruta}.sig2"), &v2).expect("escribir .sig2");
    println!("Firmado: {ruta}.sig y {ruta}.sig2");
}
