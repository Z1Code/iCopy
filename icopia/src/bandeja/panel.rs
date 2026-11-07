//! Lógica del panel de progreso de la bandeja (sin dibujo): textos, indicador, velocidad y
//! tiempo restante.
use crate::bandeja::estado::{miles, EstadoUi, Fase};
use std::collections::VecDeque;

/// Indicador de la cabecera del panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicador {
    EnVivo,
    AlDia,
    EnEspera,
    EnPausa,
    InicieSesion,
    Atencion,
    Error,
}

impl Indicador {
    pub fn texto(self) -> &'static str {
        match self {
            Indicador::EnVivo => "En vivo",
            Indicador::AlDia => "Al día",
            Indicador::EnEspera => "En espera",
            Indicador::EnPausa => "En pausa",
            Indicador::InicieSesion => "Inicia sesión",
            Indicador::Atencion => "Atención",
            Indicador::Error => "Error",
        }
    }

    /// El punto late solo mientras hay trabajo en marcha.
    pub fn late(self) -> bool {
        self == Indicador::EnVivo
    }
}

pub fn indicador(e: &EstadoUi) -> Indicador {
    match &e.fase {
        Fase::Listando { .. } | Fase::Copiando { .. } | Fase::Verificando { .. } => Indicador::EnVivo,
        Fase::Inactiva if e.total > 0 && e.verificadas < e.total => Indicador::EnEspera,
        Fase::Inactiva => Indicador::AlDia,
        Fase::Pausada => Indicador::EnPausa,
        Fase::SesionCaducada => Indicador::InicieSesion,
        Fase::SinEspacio | Fase::DestinoAusente | Fase::SinDestino => Indicador::Atencion,
        Fase::Error(_) => Indicador::Error,
    }
}

/// Velocidad media de descarga sobre una ventana móvil de muestras (segundos, bytes acumulados).
#[derive(Debug, Default)]
pub struct Velocimetro {
    muestras: VecDeque<(f64, u64)>,
}

impl Velocimetro {
    const VENTANA: f64 = 8.0;

    pub fn registrar(&mut self, t: f64, bytes: u64) {
        if let Some(&(_, b)) = self.muestras.back() {
            if bytes < b {
                self.muestras.clear();
            }
        }
        self.muestras.push_back((t, bytes));
        while self.muestras.len() > 2 && t - self.muestras[1].0 >= Self::VENTANA {
            self.muestras.pop_front();
        }
    }

    /// Bytes por segundo; `None` hasta tener al menos un segundo de muestras.
    pub fn bytes_por_seg(&self) -> Option<f64> {
        let (&(t0, b0), &(t1, b1)) = (self.muestras.front()?, self.muestras.back()?);
        (t1 - t0 >= 1.0).then(|| (b1 - b0) as f64 / (t1 - t0))
    }
}

/// Número con coma decimal: un decimal por debajo de 10, entero a partir de ahí.
fn num(x: f64) -> String {
    if x < 10.0 && x.fract().abs() > 0.05 {
        format!("{x:.1}").replace('.', ",")
    } else {
        miles(x.round() as u64)
    }
}

fn unidad(b: u64) -> (f64, &'static str) {
    const K: f64 = 1000.0;
    match b as f64 {
        x if x >= K * K * K * K => (K * K * K * K, "TB"),
        x if x >= K * K * K => (K * K * K, "GB"),
        x if x >= K * K => (K * K, "MB"),
        x if x >= K => (K, "kB"),
        _ => (1.0, "B"),
    }
}

/// "108 de 210 GB": ambas cifras en la unidad de la mayor.
pub fn par_bytes(a: u64, b: u64) -> String {
    let (u, nombre) = unidad(a.max(b));
    format!("{} de {} {nombre}", num(a as f64 / u), num(b as f64 / u))
}

/// "12,4 MB/s"
pub fn velocidad(bps: f64) -> String {
    let (u, nombre) = unidad(bps.max(0.0) as u64);
    let x = bps.max(0.0) / u;
    let cifra = if x < 100.0 { format!("{x:.1}").replace('.', ",") } else { miles(x.round() as u64) };
    format!("{cifra} {nombre}/s")
}

/// "~58 min restantes"
pub fn restante(segundos: u64) -> String {
    let min = segundos.div_ceil(60);
    if segundos < 60 {
        "menos de 1 min".into()
    } else if min < 60 {
        format!("~{min} min restantes")
    } else if min < 48 * 60 {
        let (h, m) = (min / 60, min % 60);
        if m == 0 {
            format!("~{h} h restantes")
        } else {
            format!("~{h} h {m} min restantes")
        }
    } else {
        format!("~{} días restantes", min.div_ceil(24 * 60))
    }
}

/// "hace 5 min"
pub fn hace(segundos: i64) -> String {
    let s = segundos.max(0);
    match s {
        0..=59 => "hace un momento".into(),
        60..=3599 => format!("hace {} min", s / 60),
        3600..=86399 => format!("hace {} h", s / 3600),
        _ if s / 86400 == 1 => "hace 1 día".into(),
        _ => format!("hace {} días", s / 86400),
    }
}

/// Todo lo que muestra el panel, ya en texto.
#[derive(Debug, Clone, PartialEq)]
pub struct Vista {
    pub indicador: Indicador,
    pub titulo: String,
    /// Fracción de la barra (0–1).
    pub fraccion: f64,
    pub linea: String,
    pub ahora: String,
    pub velocidad: String,
    pub listado: String,
    pub errores: String,
    pub pausado: bool,
}

pub fn vista(e: &EstadoUi, bytes_por_seg: Option<f64>, ahora_unix: i64) -> Vista {
    let d = &e.detalle;
    let ind = indicador(e);
    let titulo = match &e.fase {
        Fase::Listando { .. } => "Revisando iCloud…".to_string(),
        Fase::Copiando { .. } => "Copiando tu fototeca".into(),
        Fase::Verificando { .. } => "Verificando tu copia".into(),
        Fase::Inactiva if ind == Indicador::AlDia && e.total > 0 => "Al día".into(),
        Fase::Inactiva if e.total == 0 => "Aún no hay nada copiado".into(),
        Fase::Inactiva => "En espera".into(),
        Fase::Pausada => "Copia en pausa".into(),
        Fase::SesionCaducada => "Tienes que iniciar sesión en iCloud".into(),
        Fase::SinEspacio => "No queda espacio en el disco".into(),
        Fase::DestinoAusente => "No encuentro la carpeta destino".into(),
        Fase::SinDestino => "Elige una carpeta destino".into(),
        Fase::Error(_) => "Algo ha fallado".into(),
    };
    let fraccion_bytes = if d.bytes_total > 0 { d.bytes_verificados as f64 / d.bytes_total as f64 } else { 0.0 };
    let en_marcha = ind == Indicador::EnVivo;
    let vel = bytes_por_seg.filter(|v| en_marcha && *v > 0.0);
    let (fraccion, linea) = match &e.fase {
        Fase::Verificando { hechas, total } => (
            if *total > 0 { *hechas as f64 / *total as f64 } else { 0.0 },
            format!("{} de {} archivos revisados", miles(*hechas as u64), miles(*total as u64)),
        ),
        Fase::Error(m) => (fraccion_bytes, m.chars().take(90).collect()),
        _ if d.bytes_total > 0 => {
            let mut l = format!("{} % · {}", (fraccion_bytes * 100.0).floor() as u64, par_bytes(d.bytes_verificados, d.bytes_total));
            let falta = d.bytes_total.saturating_sub(d.bytes_verificados);
            if let (Some(v), true) = (vel, falta > 0) {
                l.push_str(" · ");
                l.push_str(&restante((falta as f64 / v) as u64));
            }
            (fraccion_bytes, l)
        }
        Fase::Listando { vistos } => (0.0, format!("{} elementos revisados", miles(*vistos as u64))),
        _ => (0.0, "Aún no hay nada copiado".into()),
    };
    let ahora = match (&e.fase, &d.actual) {
        (Fase::Copiando { .. }, Some(a)) => a.clone(),
        _ => "—".into(),
    };
    Vista {
        indicador: ind,
        titulo,
        fraccion: fraccion.clamp(0.0, 1.0),
        linea,
        ahora,
        velocidad: vel.map(velocidad).unwrap_or_else(|| "—".into()),
        listado: d.ultimo_listado.map(|t| hace(ahora_unix - t)).unwrap_or_else(|| "nunca".into()),
        errores: miles(d.con_error),
        pausado: e.fase == Fase::Pausada,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bandeja::estado::Detalle;

    #[test]
    fn formatos() {
        assert_eq!(par_bytes(108_400_000_000, 210_000_000_000), "108 de 210 GB");
        assert_eq!(par_bytes(1_500_000_000, 2_000_000_000), "1,5 de 2 GB");
        assert_eq!(par_bytes(0, 0), "0 de 0 B");
        assert_eq!(velocidad(12_400_000.0), "12,4 MB/s");
        assert_eq!(velocidad(850_000.0), "850 kB/s");
        assert_eq!(velocidad(2_000_000.0), "2,0 MB/s");
        assert_eq!(restante(30), "menos de 1 min");
        assert_eq!(restante(58 * 60), "~58 min restantes");
        assert_eq!(restante(2 * 3600 + 10 * 60), "~2 h 10 min restantes");
        assert_eq!(restante(3 * 86400), "~3 días restantes");
        assert_eq!(hace(10), "hace un momento");
        assert_eq!(hace(5 * 60 + 3), "hace 5 min");
        assert_eq!(hace(3 * 3600), "hace 3 h");
        assert_eq!(hace(86400 + 5), "hace 1 día");
        assert_eq!(hace(3 * 86400), "hace 3 días");
    }

    #[test]
    fn velocimetro_ventana_movil() {
        let mut v = Velocimetro::default();
        v.registrar(0.0, 0);
        assert_eq!(v.bytes_por_seg(), None);
        v.registrar(1.0, 1_000_000);
        assert_eq!(v.bytes_por_seg(), Some(1_000_000.0));
        for s in 2..=20 {
            v.registrar(s as f64, 1_000_000 + (s as u64 - 1) * 3_000_000);
        }
        // Las muestras viejas (a 1 MB/s) ya salieron de la ventana.
        assert_eq!(v.bytes_por_seg(), Some(3_000_000.0));
        // Un contador que vuelve atrás reinicia la medida.
        v.registrar(21.0, 5);
        assert_eq!(v.bytes_por_seg(), None);
    }

    fn estado(fase: Fase, bv: u64, bt: u64) -> EstadoUi {
        EstadoUi {
            fase,
            verificadas: 108,
            total: 210,
            detalle: Detalle { bytes_verificados: bv, bytes_total: bt, con_error: 2, ultimo_listado: Some(1000), actual: Some("IMG_1 (original)".into()) },
        }
    }

    #[test]
    fn vista_copiando() {
        let e = estado(Fase::Copiando { hechas: 5, total: 100 }, 108_000_000_000, 210_000_000_000);
        let v = vista(&e, Some(29_310_000.0), 1000 + 300);
        assert_eq!(v.indicador, Indicador::EnVivo);
        assert!(v.indicador.late());
        assert_eq!(v.titulo, "Copiando tu fototeca");
        assert_eq!(v.linea, "51 % · 108 de 210 GB · ~58 min restantes");
        assert_eq!(v.ahora, "IMG_1 (original)");
        assert_eq!(v.velocidad, "29,3 MB/s");
        assert_eq!(v.listado, "hace 5 min");
        assert_eq!(v.errores, "2");
        assert!((v.fraccion - 108.0 / 210.0).abs() < 1e-9);
    }

    #[test]
    fn vista_parada_e_indicadores() {
        let mut e = estado(Fase::Inactiva, 210, 210);
        e.verificadas = 210;
        let v = vista(&e, Some(5.0), 1000);
        assert_eq!((v.indicador, v.titulo.as_str(), v.velocidad.as_str()), (Indicador::AlDia, "Al día", "—"));
        assert_eq!(v.linea, "100 % · 210 de 210 B");
        assert_eq!(indicador(&estado(Fase::Inactiva, 1, 2)), Indicador::EnEspera);
        let p = vista(&estado(Fase::Pausada, 1, 2), None, 1000);
        assert!(p.pausado);
        assert_eq!(p.indicador, Indicador::EnPausa);
        assert_eq!(indicador(&estado(Fase::SesionCaducada, 0, 0)), Indicador::InicieSesion);
        assert_eq!(indicador(&estado(Fase::SinEspacio, 0, 0)), Indicador::Atencion);
        assert_eq!(indicador(&estado(Fase::Error("x".into()), 0, 0)), Indicador::Error);
        let l = vista(&estado(Fase::Listando { vistos: 1200 }, 0, 0), None, 1000);
        assert_eq!((l.titulo.as_str(), l.linea.as_str(), l.ahora.as_str()), ("Revisando iCloud…", "1.200 elementos revisados", "—"));
        assert_eq!(vista(&estado(Fase::Inactiva, 0, 0), None, 1000).listado, "hace un momento");
    }
}
