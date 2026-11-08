//! Genera la clave de firma de las actualizaciones. Uso: generar_clave_firma <archivo-privado>
//! Escribe la clave PRIVADA (base64) en ese archivo —que no debe estar en el repo— y solo imprime
//! la PÚBLICA, que va en `recursos/clave_actualizaciones.pub`. Se niega a sobrescribir.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Write;

fn main() {
    let Some(ruta) = std::env::args().nth(1) else {
        eprintln!("Uso: generar_clave_firma <archivo-privado>");
        std::process::exit(2);
    };
    let mut semilla = [0u8; 32];
    getrandom::getrandom(&mut semilla).expect("sin aleatoriedad del sistema");
    let privada = STANDARD.encode(semilla);
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&ruta) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("No escribo {ruta}: {e} (si ya existe, no se sobrescribe)");
            std::process::exit(1);
        }
    };
    f.write_all(privada.as_bytes()).expect("escribir la clave");
    println!("{}", icopia::actualizar::clave_publica_de(&privada).unwrap());
}
