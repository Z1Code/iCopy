//! Cliente del proceso puente-icloud (Python): JSON por líneas sobre stdin/stdout.
use crate::fuente::{ErrorFuente, FuenteIcloud, ResultadoBorrado};
use crate::modelo::AssetIcloud;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Puente {
    hijo: Child,
    entrada: ChildStdin,
    salida: BufReader<ChildStdout>,
}

/// `ICOPIA_PUENTE` (ruta a .exe o .py; con .py usa `ICOPIA_PYTHON` o `python`)
/// o, si no, `puente-icloud.exe` junto al ejecutable actual.
pub fn comando_puente() -> anyhow::Result<Command> {
    if let Ok(ruta) = std::env::var("ICOPIA_PUENTE") {
        if ruta.to_lowercase().ends_with(".py") {
            let python = std::env::var("ICOPIA_PYTHON").unwrap_or_else(|_| "python".into());
            let mut c = Command::new(python);
            c.arg("-I").arg(ruta);
            return Ok(c);
        }
        return Ok(Command::new(ruta));
    }
    let exe = std::env::current_exe()?;
    let junto = exe.with_file_name("puente-icloud.exe");
    anyhow::ensure!(junto.exists(), "No encuentro puente-icloud.exe junto a {}", exe.display());
    Ok(Command::new(junto))
}

fn comprobar(v: &Value) -> Result<(), ErrorFuente> {
    if v["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    let error = v["error"].as_str().unwrap_or("otro").to_string();
    let detalle = v["detalle"].as_str().unwrap_or("").to_string();
    Err(match error.as_str() {
        "sesion_caducada" => ErrorFuente::SesionCaducada,
        "credenciales" | "codigo_incorrecto" => ErrorFuente::Credenciales(format!("{error} {detalle}").trim().to_string()),
        _ => ErrorFuente::Otro(if detalle.is_empty() { error } else { detalle }),
    })
}

impl Puente {
    pub fn iniciar(apple_id: &str, dir_sesion: &Path, log: &Path) -> anyhow::Result<Puente> {
        Self::iniciar_con(comando_puente()?, apple_id, dir_sesion, log)
    }

    pub fn iniciar_con(mut cmd: Command, apple_id: &str, dir_sesion: &Path, log: &Path) -> anyhow::Result<Puente> {
        std::fs::create_dir_all(dir_sesion)?;
        if let Some(p) = log.parent() {
            std::fs::create_dir_all(p)?;
        }
        let stderr = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        cmd.arg("--cookies")
            .arg(dir_sesion)
            .arg("--apple-id")
            .arg(apple_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut hijo = cmd.spawn().map_err(|e| anyhow::anyhow!("no pude lanzar el puente: {e}"))?;
        let entrada = hijo.stdin.take().expect("stdin del puente");
        let salida = BufReader::new(hijo.stdout.take().expect("stdout del puente"));
        Ok(Puente { hijo, entrada, salida })
    }

    fn enviar(&mut self, v: &Value) -> Result<(), ErrorFuente> {
        let mut linea = v.to_string();
        linea.push('\n');
        self.entrada
            .write_all(linea.as_bytes())
            .and_then(|_| self.entrada.flush())
            .map_err(|e| ErrorFuente::Otro(format!("no puedo escribir al puente: {e}")))
    }

    fn leer(&mut self) -> Result<Value, ErrorFuente> {
        let mut linea = String::new();
        let n = self
            .salida
            .read_line(&mut linea)
            .map_err(|e| ErrorFuente::Otro(format!("no puedo leer del puente: {e}")))?;
        if n == 0 {
            return Err(ErrorFuente::Otro("el puente terminó inesperadamente (ver puente.log)".into()));
        }
        serde_json::from_str(&linea).map_err(|e| ErrorFuente::Otro(format!("respuesta inválida del puente: {e}: {linea}")))
    }

    pub fn sesion(&mut self) -> Result<(), ErrorFuente> {
        self.enviar(&json!({"cmd": "sesion"}))?;
        comprobar(&self.leer()?)
    }

    /// Devuelve `true` si además hace falta el código 2FA.
    pub fn login(&mut self, password: &str) -> Result<bool, ErrorFuente> {
        self.enviar(&json!({"cmd": "login", "password": password}))?;
        let v = self.leer()?;
        comprobar(&v)?;
        Ok(v["requiere_2fa"].as_bool().unwrap_or(false))
    }

    pub fn codigo_2fa(&mut self, codigo: &str) -> Result<(), ErrorFuente> {
        self.enviar(&json!({"cmd": "codigo_2fa", "codigo": codigo}))?;
        comprobar(&self.leer()?)
    }
}

impl FuenteIcloud for Puente {
    fn listar(&mut self, en_asset: &mut dyn FnMut(AssetIcloud) -> bool) -> Result<usize, ErrorFuente> {
        self.enviar(&json!({"cmd": "listar"}))?;
        loop {
            let v = self.leer()?;
            if let Some(a) = v.get("asset") {
                let asset: AssetIcloud = serde_json::from_value(a.clone())
                    .map_err(|e| ErrorFuente::Otro(format!("asset inválido del puente: {e}")))?;
                if !en_asset(asset) {
                    let _ = self.hijo.kill();
                    return Err(ErrorFuente::Cancelado);
                }
                continue;
            }
            if v.get("aviso").is_some() {
                continue;
            }
            comprobar(&v)?;
            return Ok(v["total"].as_u64().unwrap_or(0) as usize);
        }
    }

    fn refrescar(&mut self, ids: &[String], en_asset: &mut dyn FnMut(AssetIcloud) -> bool) -> Result<Vec<String>, ErrorFuente> {
        self.enviar(&json!({"cmd": "refrescar", "ids": ids}))?;
        loop {
            let v = self.leer()?;
            if let Some(a) = v.get("asset") {
                let asset: AssetIcloud = serde_json::from_value(a.clone())
                    .map_err(|e| ErrorFuente::Otro(format!("asset inválido del puente: {e}")))?;
                if !en_asset(asset) {
                    let _ = self.hijo.kill();
                    return Err(ErrorFuente::Cancelado);
                }
                continue;
            }
            if v.get("aviso").is_some() {
                continue;
            }
            comprobar(&v)?;
            return Ok(v["faltan"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default());
        }
    }

    fn borrar(&mut self, ids: &[String]) -> Result<ResultadoBorrado, ErrorFuente> {
        self.enviar(&json!({"cmd": "borrar", "ids": ids}))?;
        let v = self.leer()?;
        comprobar(&v)?;
        let borrados = v["borrados"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let fallidos = v["fallidos"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| (x["id"].as_str().unwrap_or("").to_string(), x["error"].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ResultadoBorrado { borrados, fallidos })
    }
}

impl Drop for Puente {
    fn drop(&mut self) {
        let _ = self.hijo.kill();
        let _ = self.hijo.wait();
    }
}
