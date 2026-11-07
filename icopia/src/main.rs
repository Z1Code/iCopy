use anyhow::{anyhow, bail, Context, Result};
use icopia::config::{self, Config};
use icopia::fuente::{ErrorFuente, FuenteIcloud};
use icopia::modelo::BIBLIOTECA_PERSONAL;
use icopia::puente::Puente;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const AYUDA: &str = "icopia — copia tus fotos de iCloud al PC

Uso:
  icopia destino <carpeta>       Fija la carpeta donde se guardan las fotos
  icopia login                   Inicia sesión en iCloud (contraseña + código 2FA)
  icopia diagnostico             Comprueba la sesión y que las descargas funcionan
  icopia copiar [--mes AAAA-MM]  Copia y verifica lo que falte
  icopia estado                  Resumen de lo copiado
  icopia verificar               Vuelve a comprobar todo lo copiado
  icopia liberar [--dias N]      Borra de iCloud lo ya copiado y verificado (pide confirmación)
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pausa = args.iter().any(|a| a == "--pausa");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--pausa").collect();
    let r = ejecutar(&args);
    if let Err(e) = &r {
        eprintln!("\nError: {e:#}");
    }
    if pausa {
        eprint!("\nPulsa Enter para cerrar…");
        let _ = io::stdin().lock().read_line(&mut String::new());
    }
    if r.is_err() {
        std::process::exit(1);
    }
}

fn ejecutar(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("destino") => cmd_destino(args.get(1)),
        Some("login") => cmd_login(),
        Some("diagnostico") => cmd_diagnostico(),
        _ => {
            print!("{AYUDA}");
            Ok(())
        }
    }
}

#[allow(dead_code)]
fn opcion<'a>(args: &'a [String], nombre: &str) -> Option<&'a str> {
    args.iter().position(|a| a == nombre).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn preguntar(texto: &str) -> Result<String> {
    print!("{texto}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().lock().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn abrir_puente(cfg: &Config) -> Result<Puente> {
    let apple_id = cfg.apple_id.as_deref().context("No hay Apple ID: ejecuta `icopia login`")?;
    Puente::iniciar(apple_id, &config::dir_sesion(), &config::log_puente())
}

fn cmd_destino(ruta: Option<&String>) -> Result<()> {
    let ruta = PathBuf::from(ruta.context("Uso: icopia destino <carpeta>")?);
    std::fs::create_dir_all(&ruta).with_context(|| format!("no puedo crear {}", ruta.display()))?;
    let mut cfg = Config::cargar()?;
    cfg.destino = Some(ruta.clone());
    cfg.guardar()?;
    println!("Destino: {}", ruta.display());
    Ok(())
}

fn cmd_login() -> Result<()> {
    let mut cfg = Config::cargar()?;
    let defecto = cfg.apple_id.clone().unwrap_or_default();
    let escrito = preguntar(&format!("Apple ID [{defecto}]: "))?;
    let apple_id = if escrito.is_empty() { defecto } else { escrito };
    if apple_id.is_empty() {
        bail!("Hace falta el Apple ID");
    }
    let password = rpassword::prompt_password("Contraseña (no se guarda): ")?;
    let mut puente = Puente::iniciar(&apple_id, &config::dir_sesion(), &config::log_puente())?;
    let requiere_2fa = puente.login(&password).map_err(|e| anyhow!("{e}"))?;
    if requiere_2fa {
        let mut fallos = 0;
        loop {
            let codigo = preguntar("Código de 6 dígitos que apareció en tu iPhone: ")?;
            match puente.codigo_2fa(&codigo) {
                Ok(()) => break,
                Err(ErrorFuente::Credenciales(_)) if fallos < 2 => {
                    fallos += 1;
                    println!("Código incorrecto, prueba otra vez.");
                }
                Err(e) => bail!("{e}"),
            }
        }
    }
    puente.sesion().map_err(|e| anyhow!("La sesión no quedó válida: {e}"))?;
    cfg.apple_id = Some(apple_id);
    cfg.guardar()?;
    println!("Sesión iniciada. Dura unos 2 meses; icopia te avisará cuando caduque.");
    Ok(())
}

fn cmd_diagnostico() -> Result<()> {
    let cfg = Config::cargar()?;
    let mut puente = abrir_puente(&cfg)?;
    puente.sesion().map_err(|e| anyhow!("{e}"))?;
    println!("Sesión: válida");
    let (mut assets, mut partes, mut bytes, mut compartidos) = (0u64, 0u64, 0u64, 0u64);
    let mut primera: Option<(String, u64)> = None;
    let total = puente
        .listar(&mut |a| {
            assets += 1;
            if a.biblioteca != BIBLIOTECA_PERSONAL {
                compartidos += 1;
            }
            for p in &a.partes {
                partes += 1;
                bytes += p.tamano;
                if primera.is_none() {
                    primera = Some((p.url.clone(), p.tamano));
                }
            }
            if assets % 500 == 0 {
                eprint!("\r  listados {assets}…");
            }
            true
        })
        .map_err(|e| anyhow!("{e}"))?;
    println!(
        "\nEn iCloud: {total} elementos ({compartidos} de Fototeca Compartida), {partes} archivos, {:.1} GB",
        bytes as f64 / 1e9
    );
    let (url, tamano) = primera.context("La biblioteca está vacía")?;
    let fin = tamano.min(1024).saturating_sub(1);
    let resp = reqwest::blocking::Client::new().get(&url).header("Range", format!("bytes=0-{fin}")).send()?;
    let estado = resp.status().as_u16();
    let rango = resp.headers().get("content-range").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
    println!("Descarga de prueba sin cookies: HTTP {estado}, Content-Range: {rango}");
    match estado {
        206 => println!("OK: las descargas funcionan y se pueden reanudar."),
        200 => println!("AVISO: funciona, pero el servidor ignora Range (no se podrá reanudar a mitad de archivo)."),
        _ => bail!("La descarga directa falló: revisar el diseño antes de seguir."),
    }
    Ok(())
}
