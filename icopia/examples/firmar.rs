//! Firma un archivo para las actualizaciones. Uso: firmar <archivo>
//! La clave privada (base64) sale de la variable de entorno `ICOPY_CLAVE_FIRMA`. Escribe
//! `<archivo>.sig` y comprueba que la clave pública incluida en la app lo acepta: así nunca se
//! publica algo que las instalaciones rechazarían.
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
    let firma = icopia::actualizar::firmar(&privada, &datos).expect("firmar");
    let publica = include_str!("../recursos/clave_actualizaciones.pub");
    if !icopia::actualizar::firma_valida(&datos, &firma, publica) {
        eprintln!("La clave privada no corresponde a recursos/clave_actualizaciones.pub: no se escribe la firma");
        std::process::exit(1);
    }
    std::fs::write(format!("{ruta}.sig"), &firma).expect("escribir la firma");
    println!("Firmado: {ruta}.sig");
}
