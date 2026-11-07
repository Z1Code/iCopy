//! Estado que muestra el icono: texto del menú/tooltip y color.
use crate::registro::Progreso;

#[derive(Debug, Clone, PartialEq)]
pub enum Fase {
    Inactiva,
    Listando { vistos: usize },
    Copiando { hechas: usize, total: usize },
    Verificando { hechas: usize, total: usize },
    Pausada,
    SesionCaducada,
    SinEspacio,
    DestinoAusente,
    SinDestino,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EstadoUi {
    pub fase: Fase,
    pub verificadas: u64,
    pub total: u64,
    pub detalle: Detalle,
}

/// Datos extra para el panel de progreso.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detalle {
    pub bytes_verificados: u64,
    pub bytes_total: u64,
    pub con_error: u64,
    /// Fin del último listado completo de iCloud (unix).
    pub ultimo_listado: Option<i64>,
    /// Archivo que se está copiando ahora.
    pub actual: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventoUi {
    Estado(EstadoUi),
    Aviso { titulo: String, texto: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Gris,
    Azul,
    Naranja,
    Rojo,
}

/// 12430 → "12.430"
pub fn miles(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

impl EstadoUi {
    pub fn sin_datos(fase: Fase) -> EstadoUi {
        EstadoUi { fase, verificadas: 0, total: 0, detalle: Detalle::default() }
    }

    pub fn con_progreso(fase: Fase, p: &Progreso) -> EstadoUi {
        EstadoUi {
            fase,
            verificadas: p.verificadas,
            total: p.partes,
            detalle: Detalle {
                bytes_verificados: p.bytes_verificados,
                bytes_total: p.bytes_total,
                con_error: p.con_error,
                ultimo_listado: p.ultimo_listado,
                actual: None,
            },
        }
    }

    /// Porcentaje copiado y verificado (por bytes si se conocen; si no, por archivos).
    pub fn porcentaje(&self) -> Option<u64> {
        let d = &self.detalle;
        if d.bytes_total > 0 {
            Some(d.bytes_verificados * 100 / d.bytes_total)
        } else if self.total > 0 {
            Some(self.verificadas * 100 / self.total)
        } else {
            None
        }
    }

    fn progreso(&self) -> String {
        if self.total == 0 {
            "nada copiado aún".into()
        } else {
            format!("{} de {} archivos ({}%)", miles(self.verificadas), miles(self.total), self.verificadas * 100 / self.total)
        }
    }

    /// Texto corto (cabe en el tooltip de Windows, 127 caracteres).
    pub fn texto(&self) -> String {
        let t = match &self.fase {
            Fase::Inactiva if self.total > 0 && self.verificadas == self.total => format!("Al día — {}", self.progreso()),
            Fase::Inactiva => format!("En espera — {}", self.progreso()),
            Fase::Listando { vistos } => format!("Revisando iCloud… ({} vistos)", miles(*vistos as u64)),
            Fase::Copiando { hechas, total } => {
                let pct = self.porcentaje().map(|p| format!(" · {p} % del total")).unwrap_or_default();
                format!("Copiando {} de {}…{pct}", miles(*hechas as u64), miles(*total as u64))
            }
            Fase::Verificando { hechas, total } => format!("Verificando {} de {}…", miles(*hechas as u64), miles(*total as u64)),
            Fase::Pausada => format!("En pausa — {}", self.progreso()),
            Fase::SesionCaducada => "Inicia sesión en iCloud (menú → Iniciar sesión)".into(),
            Fase::SinEspacio => "Sin espacio en el disco destino".into(),
            Fase::DestinoAusente => "No encuentro la carpeta destino (¿disco desconectado?)".into(),
            Fase::SinDestino => "Elige una carpeta destino (menú → Cambiar destino)".into(),
            Fase::Error(m) => format!("Error: {m}"),
        };
        t.chars().take(110).collect()
    }

    pub fn color(&self) -> Color {
        match self.fase {
            Fase::Listando { .. } | Fase::Copiando { .. } | Fase::Verificando { .. } => Color::Azul,
            Fase::Inactiva | Fase::Pausada => Color::Gris,
            Fase::SesionCaducada | Fase::SinEspacio | Fase::DestinoAusente | Fase::SinDestino => Color::Naranja,
            Fase::Error(_) => Color::Rojo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_miles() {
        assert_eq!(miles(999), "999");
        assert_eq!(miles(12430), "12.430");
        assert_eq!(miles(1_000_000), "1.000.000");
    }

    #[test]
    fn textos_y_colores() {
        let al_dia = EstadoUi { fase: Fase::Inactiva, verificadas: 10, total: 10, detalle: Detalle::default() };
        assert_eq!(al_dia.texto(), "Al día — 10 de 10 archivos (100%)");
        assert_eq!(al_dia.color(), Color::Gris);
        let mut copiando = EstadoUi::sin_datos(Fase::Copiando { hechas: 3, total: 1200 });
        assert_eq!(copiando.texto(), "Copiando 3 de 1.200…");
        copiando.detalle.bytes_total = 200;
        copiando.detalle.bytes_verificados = 103;
        assert_eq!(copiando.texto(), "Copiando 3 de 1.200… · 51 % del total");
        assert_eq!(copiando.color(), Color::Azul);
        assert_eq!(EstadoUi::sin_datos(Fase::SesionCaducada).color(), Color::Naranja);
        assert_eq!(EstadoUi::sin_datos(Fase::Error("x".into())).color(), Color::Rojo);
        let largo = EstadoUi::sin_datos(Fase::Error("x".repeat(500)));
        assert!(largo.texto().chars().count() <= 110);
    }
}
