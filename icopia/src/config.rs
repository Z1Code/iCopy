//! Configuración en %APPDATA%\icopia\config.json (o ICOPIA_DIR para desarrollo).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub destino: Option<PathBuf>,
    pub apple_id: Option<String>,
    pub mantener_dias: u32,
    pub concurrencia: usize,
    pub intervalo_horas: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config { destino: None, apple_id: None, mantener_dias: 30, concurrencia: 3, intervalo_horas: 4 }
    }
}

pub fn dir_app() -> PathBuf {
    if let Ok(p) = std::env::var("ICOPIA_DIR") {
        return PathBuf::from(p);
    }
    dirs::config_dir().expect("no encuentro %APPDATA%").join("icopia")
}

pub fn ruta_config() -> PathBuf {
    dir_app().join("config.json")
}

pub fn dir_sesion() -> PathBuf {
    dir_app().join("sesion")
}

pub fn log_puente() -> PathBuf {
    dir_app().join("puente.log")
}

impl Config {
    pub fn cargar_de(ruta: &Path) -> anyhow::Result<Config> {
        if !ruta.exists() {
            return Ok(Config::default());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(ruta)?)?)
    }

    pub fn guardar_en(&self, ruta: &Path) -> anyhow::Result<()> {
        if let Some(p) = ruta.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = ruta.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, ruta)?;
        Ok(())
    }

    pub fn cargar() -> anyhow::Result<Config> {
        Self::cargar_de(&ruta_config())
    }

    pub fn guardar(&self) -> anyhow::Result<()> {
        self.guardar_en(&ruta_config())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_archivo_da_valores_por_defecto() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Config::cargar_de(&d.path().join("no.json")).unwrap(), Config::default());
    }

    #[test]
    fn guarda_y_carga() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("sub").join("config.json");
        let c = Config { destino: Some(PathBuf::from(r"F:\iPhone Fotos")), apple_id: Some("yo@x.com".into()), ..Config::default() };
        c.guardar_en(&ruta).unwrap();
        assert_eq!(Config::cargar_de(&ruta).unwrap(), c);
    }

    #[test]
    fn json_parcial_completa_con_defectos() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("c.json");
        std::fs::write(&ruta, r#"{"apple_id":"yo@x.com"}"#).unwrap();
        let c = Config::cargar_de(&ruta).unwrap();
        assert_eq!(c.apple_id.as_deref(), Some("yo@x.com"));
        assert_eq!(c.mantener_dias, 30);
        assert_eq!(c.concurrencia, 3);
    }
}
