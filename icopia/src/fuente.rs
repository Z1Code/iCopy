//! De dónde vienen las fotos: el puente real o una fuente falsa para pruebas.
use crate::modelo::AssetIcloud;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorFuente {
    SesionCaducada,
    Credenciales(String),
    Cancelado,
    Otro(String),
}

impl fmt::Display for ErrorFuente {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorFuente::SesionCaducada => write!(f, "la sesión de iCloud caducó (ejecuta `icopia login`)"),
            ErrorFuente::Credenciales(d) => write!(f, "credenciales rechazadas: {d}"),
            ErrorFuente::Cancelado => write!(f, "cancelado"),
            ErrorFuente::Otro(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for ErrorFuente {}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultadoBorrado {
    pub borrados: Vec<String>,
    /// (id, motivo)
    pub fallidos: Vec<(String, String)>,
}

pub trait FuenteIcloud {
    /// Llama a `en_asset` por cada asset. Si devuelve `false`, se detiene con `Err(Cancelado)`.
    fn listar(&mut self, en_asset: &mut dyn FnMut(AssetIcloud) -> bool) -> Result<usize, ErrorFuente>;
    /// Borra (manda a "Eliminado recientemente") los ids del último listado.
    fn borrar(&mut self, ids: &[String]) -> Result<ResultadoBorrado, ErrorFuente>;
}

/// Fuente en memoria para pruebas.
#[derive(Debug, Default, Clone)]
pub struct FuenteFalsa {
    pub assets: Vec<AssetIcloud>,
    pub borrados: Vec<String>,
    pub sesion_caducada: bool,
    pub rechazar: Vec<String>,
}

impl FuenteIcloud for FuenteFalsa {
    fn listar(&mut self, en_asset: &mut dyn FnMut(AssetIcloud) -> bool) -> Result<usize, ErrorFuente> {
        if self.sesion_caducada {
            return Err(ErrorFuente::SesionCaducada);
        }
        for a in self.assets.clone() {
            if !en_asset(a) {
                return Err(ErrorFuente::Cancelado);
            }
        }
        Ok(self.assets.len())
    }

    fn borrar(&mut self, ids: &[String]) -> Result<ResultadoBorrado, ErrorFuente> {
        if self.sesion_caducada {
            return Err(ErrorFuente::SesionCaducada);
        }
        let mut r = ResultadoBorrado::default();
        for id in ids {
            if self.rechazar.contains(id) {
                r.fallidos.push((id.clone(), "reintentar".into()));
            } else {
                self.borrados.push(id.clone());
                self.assets.retain(|a| &a.id != id);
                r.borrados.push(id.clone());
            }
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str) -> AssetIcloud {
        AssetIcloud {
            id: id.into(),
            biblioteca: "PrimarySync".into(),
            nombre_base: id.into(),
            fecha: "2026-08-10T19:37:18-04:00".into(),
            partes: vec![],
        }
    }

    #[test]
    fn falsa_lista_y_cancela() {
        let mut f = FuenteFalsa { assets: vec![asset("A"), asset("B")], ..Default::default() };
        let mut vistos = vec![];
        assert_eq!(f.listar(&mut |a| { vistos.push(a.id); true }), Ok(2));
        assert_eq!(vistos, vec!["A", "B"]);
        assert_eq!(f.listar(&mut |_| false), Err(ErrorFuente::Cancelado));
    }

    #[test]
    fn falsa_borra_y_rechaza() {
        let mut f = FuenteFalsa { assets: vec![asset("A"), asset("B")], rechazar: vec!["B".into()], ..Default::default() };
        let r = f.borrar(&["A".into(), "B".into()]).unwrap();
        assert_eq!(r.borrados, vec!["A"]);
        assert_eq!(r.fallidos, vec![("B".to_string(), "reintentar".to_string())]);
        assert_eq!(f.assets.len(), 1);
    }

    #[test]
    fn falsa_caducada() {
        let mut f = FuenteFalsa { sesion_caducada: true, ..Default::default() };
        assert_eq!(f.listar(&mut |_| true), Err(ErrorFuente::SesionCaducada));
    }
}
