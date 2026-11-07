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
        Fase::Inactiva if e.total == 0 || e.verificadas < e.total => Indicador::EnEspera,
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
        self.medida()
    }

    /// Empieza de cero (nueva ronda).
    pub fn reiniciar(&mut self) {
        self.muestras.clear();
    }

    fn medida(&self) -> Option<f64> {
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
    let (x, y) = (a as f64 / u, b as f64 / u);
    // Por debajo de 10, las dos cifras con un decimal para que se vea el avance.
    let f = |v: f64| if y < 10.0 && u > 1.0 { format!("{v:.1}").replace('.', ",") } else { num(v) };
    format!("{} de {} {nombre}", f(x), f(y))
}

/// "12,4 MB/s"
pub fn velocidad(bps: f64) -> String {
    let (u, nombre) = unidad(bps.max(0.0) as u64);
    let x = bps.max(0.0) / u;
    let cifra = if x < 100.0 { format!("{x:.1}").replace('.', ",") } else { miles(x.round() as u64) };
    format!("{cifra} {nombre}/s")
}

/// "~58 min", "~2 h 10 min", "~3 días", "menos de 1 min"
pub fn duracion(segundos: u64) -> String {
    let min = segundos.div_ceil(60);
    if segundos < 60 {
        "menos de 1 min".into()
    } else if min < 60 {
        format!("~{min} min")
    } else if min < 48 * 60 {
        let (h, m) = (min / 60, min % 60);
        if m == 0 {
            format!("~{h} h")
        } else {
            format!("~{h} h {m} min")
        }
    } else {
        format!("~{} días", min.div_ceil(24 * 60))
    }
}

/// "~58 min restantes"
pub fn restante(segundos: u64) -> String {
    if segundos < 60 {
        "menos de 1 min restante".into()
    } else {
        format!("{} restantes", duracion(segundos))
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

/// ¿Empieza una ronda nueva? (para reiniciar la velocidad y el tiempo restante)
pub fn es_nueva_ronda(antes: &EstadoUi, despues: &EstadoUi) -> bool {
    let listando = matches!(despues.fase, Fase::Listando { .. });
    listando && !matches!(antes.fase, Fase::Listando { .. }) || despues.trabajando() && !antes.trabajando()
}

/// Acción del botón principal del panel, según el estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Pausar,
    Reanudar,
    RevisarAhora,
    IniciarSesion,
}

impl Principal {
    pub fn texto(self) -> &'static str {
        match self {
            Principal::Pausar => "Pausar",
            Principal::Reanudar => "Reanudar",
            Principal::RevisarAhora => "Revisar ahora",
            Principal::IniciarSesion => "Iniciar sesión",
        }
    }
}

pub fn principal(e: &EstadoUi) -> Principal {
    match e.fase {
        _ if e.trabajando() => Principal::Pausar,
        Fase::Pausada => Principal::Reanudar,
        Fase::SesionCaducada => Principal::IniciarSesion,
        _ => Principal::RevisarAhora,
    }
}

/// Todo lo que muestra el panel, ya en texto.
#[derive(Debug, Clone, PartialEq)]
pub struct Vista {
    pub indicador: Indicador,
    pub titulo: String,
    /// Fracción de la barra (0–1); `None` = barra indeterminada (listando).
    pub fraccion: Option<f64>,
    /// Avance de la ronda (o, en reposo, última y próxima revisión).
    pub linea: String,
    /// Totales de toda la fototeca.
    pub linea_total: String,
    pub ahora: String,
    pub velocidad: String,
    pub listado: String,
    pub errores: String,
    pub principal: Principal,
}

fn plural(n: u64, uno: &str, varios: &str) -> String {
    format!("{} {}", miles(n), if n == 1 { uno } else { varios })
}

/// Se recalcula entera en cada refresco a partir del estado vivo (nada se guarda entre llamadas).
pub fn vista(e: &EstadoUi, bytes_por_seg: Option<f64>, ahora_unix: i64) -> Vista {
    let d = &e.detalle;
    let ind = indicador(e);
    let vel = bytes_por_seg.filter(|v| e.trabajando() && *v > 0.0);
    let titulo = match &e.fase {
        Fase::Listando { .. } if d.renovando_enlaces => "Renovando enlaces de iCloud…".to_string(),
        Fase::Listando { vistos: 0 } => "Revisando iCloud…".to_string(),
        Fase::Listando { vistos } => format!("Revisando iCloud… ({} vistos)", miles(*vistos as u64)),
        Fase::Copiando { total, .. } => {
            let n = d.ronda.as_ref().map(|r| r.archivos).unwrap_or(*total as u64);
            format!("Copiando {}", plural(n, "nuevo", "nuevos"))
        }
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
    let fraccion_total = if d.bytes_total > 0 { d.bytes_verificados as f64 / d.bytes_total as f64 } else { 0.0 };
    let revisiones = || {
        let mut partes = Vec::new();
        if let Some(t) = d.ultimo_listado {
            partes.push(format!("Última revisión {}", hace(ahora_unix - t)));
        }
        if let Some(p) = d.proxima.filter(|&p| p > ahora_unix) {
            partes.push(format!("{} en {}", if partes.is_empty() { "Próxima" } else { "próxima" }, duracion((p - ahora_unix) as u64)));
        }
        if partes.is_empty() {
            "Todavía no se ha revisado iCloud".to_string()
        } else {
            partes.join(" · ")
        }
    };
    let (fraccion, linea) = match &e.fase {
        Fase::Listando { .. } => (None, "Buscando fotos y vídeos nuevos".to_string()),
        Fase::Copiando { hechas, total } => match &d.ronda {
            Some(r) if r.bytes > 0 => {
                let hecho = d.bytes_verificados.saturating_sub(r.bytes_base).min(r.bytes);
                let f = hecho as f64 / r.bytes as f64;
                let mut l = format!("{} % · {}", (f * 100.0).floor() as u64, par_bytes(hecho, r.bytes));
                if let (Some(v), true) = (vel, hecho < r.bytes) {
                    l.push_str(" · ");
                    l.push_str(&restante(((r.bytes - hecho) as f64 / v) as u64));
                }
                (Some(f), l)
            }
            _ => {
                let f = if *total > 0 { *hechas as f64 / *total as f64 } else { 0.0 };
                (Some(f), format!("{} de {} archivos", miles(*hechas as u64), miles(*total as u64)))
            }
        },
        Fase::Verificando { hechas, total } => (
            Some(if *total > 0 { *hechas as f64 / *total as f64 } else { 0.0 }),
            format!("{} de {} archivos revisados", miles(*hechas as u64), miles(*total as u64)),
        ),
        Fase::Error(m) => (Some(fraccion_total), m.chars().take(90).collect()),
        _ => (Some(fraccion_total), revisiones()),
    };
    let linea_total = if e.total > 0 {
        format!("Fototeca: {} de {} archivos · {}", miles(e.verificadas), miles(e.total), par_bytes(d.bytes_verificados, d.bytes_total))
    } else {
        "Fototeca: aún vacía".into()
    };
    let ahora = match (&e.fase, &d.actual) {
        (Fase::Copiando { .. }, Some(a)) => a.clone(),
        _ => "—".into(),
    };
    Vista {
        indicador: ind,
        titulo,
        fraccion: fraccion.map(|f| f.clamp(0.0, 1.0)),
        linea,
        linea_total,
        ahora,
        velocidad: vel.map(velocidad).unwrap_or_else(|| "—".into()),
        listado: d.ultimo_listado.map(|t| hace(ahora_unix - t)).unwrap_or_else(|| "nunca".into()),
        errores: miles(d.con_error),
        principal: principal(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bandeja::estado::{Detalle, Ronda};
    use crate::registro::Progreso;

    #[test]
    fn formatos() {
        assert_eq!(par_bytes(108_400_000_000, 210_000_000_000), "108 de 210 GB");
        assert_eq!(par_bytes(1_500_000_000, 2_000_000_000), "1,5 de 2,0 GB");
        assert_eq!(par_bytes(0, 0), "0 de 0 B");
        assert_eq!(velocidad(12_400_000.0), "12,4 MB/s");
        assert_eq!(velocidad(850_000.0), "850 kB/s");
        assert_eq!(velocidad(2_000_000.0), "2,0 MB/s");
        assert_eq!(restante(30), "menos de 1 min restante");
        assert_eq!(restante(58 * 60), "~58 min restantes");
        assert_eq!(restante(2 * 3600 + 10 * 60), "~2 h 10 min restantes");
        assert_eq!(restante(3 * 86400), "~3 días restantes");
        assert_eq!(duracion(3 * 3600 + 55 * 60), "~3 h 55 min");
        assert_eq!(hace(10), "hace un momento");
        assert_eq!(hace(5 * 60 + 3), "hace 5 min");
        assert_eq!(hace(3 * 3600), "hace 3 h");
        assert_eq!(hace(86400 + 5), "hace 1 día");
        assert_eq!(hace(3 * 86400), "hace 3 días");
    }

    #[test]
    fn velocimetro_ventana_movil_y_reinicio() {
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
        // Nueva ronda: no arrastra la velocidad de la anterior.
        v.registrar(22.0, 9_000_005);
        assert!(v.bytes_por_seg().is_some());
        v.reiniciar();
        v.registrar(100.0, 9_000_005);
        assert_eq!(v.bytes_por_seg(), None);
    }

    fn progreso(verificadas: u64, partes: u64, bv: u64, bt: u64) -> Progreso {
        Progreso { partes, verificadas, con_error: 0, bytes_total: bt, bytes_verificados: bv, ultimo_listado: Some(1000) }
    }

    #[test]
    fn primera_copia_listado_copia_y_reposo() {
        const GB: u64 = 1_000_000_000;
        // Listando: barra indeterminada y contador de vistos.
        let mut e = EstadoUi::con_progreso(Fase::Listando { vistos: 1200 }, &progreso(0, 0, 0, 0));
        let v = vista(&e, None, 1000);
        assert_eq!((v.titulo.as_str(), v.fraccion, v.principal), ("Revisando iCloud… (1.200 vistos)", None, Principal::Pausar));
        assert_eq!(v.linea_total, "Fototeca: aún vacía");
        assert_eq!(v.indicador, Indicador::EnVivo);
        // Copiando: la barra sigue el trabajo de la ronda.
        e.fase = Fase::Copiando { hechas: 5, total: 210 };
        e.actualizar_progreso(&progreso(108, 210, 108 * GB, 210 * GB));
        e.detalle.ronda = Some(Ronda { archivos: 210, bytes: 210 * GB, bytes_base: 0 });
        e.detalle.actual = Some("IMG_1 (original)".into());
        let v = vista(&e, Some(29_310_000.0), 1300);
        assert_eq!(v.titulo, "Copiando 210 nuevos");
        assert_eq!(v.linea, "51 % · 108 de 210 GB · ~58 min restantes");
        assert_eq!(v.linea_total, "Fototeca: 108 de 210 archivos · 108 de 210 GB");
        assert_eq!((v.ahora.as_str(), v.velocidad.as_str(), v.listado.as_str()), ("IMG_1 (original)", "29,3 MB/s", "hace 5 min"));
        // Reposo: al día, con la última y la próxima revisión; botón "Revisar ahora".
        let mut e = EstadoUi::con_progreso(Fase::Inactiva, &progreso(210, 210, 210 * GB, 210 * GB));
        e.detalle.proxima = Some(1000 + 4 * 3600);
        let v = vista(&e, Some(5.0), 1000 + 5 * 60);
        assert_eq!((v.titulo.as_str(), v.indicador, v.velocidad.as_str()), ("Al día", Indicador::AlDia, "—"));
        assert_eq!(v.linea, "Última revisión hace 5 min · próxima en ~3 h 55 min");
        assert_eq!((v.fraccion, v.principal), (Some(1.0), Principal::RevisarAhora));
    }

    #[test]
    fn segunda_ronda_con_fotos_nuevas_muestra_su_propio_avance() {
        const MB: u64 = 1_000_000;
        // Ya estaba todo copiado (1000 archivos, 5000 MB); aparecen 20 nuevos de 4 MB.
        let antes = EstadoUi::con_progreso(Fase::Inactiva, &progreso(1000, 1000, 5000 * MB, 5000 * MB));
        let listando = EstadoUi::con_progreso(Fase::Listando { vistos: 0 }, &progreso(1000, 1000, 5000 * MB, 5000 * MB));
        assert!(es_nueva_ronda(&antes, &listando));
        let mut e = EstadoUi::con_progreso(Fase::Copiando { hechas: 5, total: 20 }, &progreso(1005, 1020, 5020 * MB, 5080 * MB));
        assert!(!es_nueva_ronda(&listando, &e));
        e.detalle.ronda = Some(Ronda { archivos: 20, bytes: 80 * MB, bytes_base: 5000 * MB });
        let v = vista(&e, None, 2000);
        assert_eq!(v.titulo, "Copiando 20 nuevos");
        // La ronda va por el 25 % (no el 99,9 % de toda la fototeca)...
        assert_eq!(v.fraccion, Some(0.25));
        assert_eq!(v.linea, "25 % · 20 de 80 MB");
        // ...y los totales crecieron con lo nuevo.
        assert_eq!(v.linea_total, "Fototeca: 1.005 de 1.020 archivos · 5,0 de 5,1 GB");
        // Terminar y volver a reposo no es empezar ronda; volver a listar sí.
        let fin = EstadoUi::con_progreso(Fase::Inactiva, &progreso(1020, 1020, 5080 * MB, 5080 * MB));
        assert!(!es_nueva_ronda(&e, &fin));
        assert!(es_nueva_ronda(&e, &listando));
    }

    #[test]
    fn indicadores_y_boton_principal() {
        let base = progreso(1, 2, 1, 2);
        let con = |f: Fase| EstadoUi::con_progreso(f, &base);
        assert_eq!(indicador(&con(Fase::Inactiva)), Indicador::EnEspera);
        assert_eq!((indicador(&con(Fase::Pausada)), principal(&con(Fase::Pausada))), (Indicador::EnPausa, Principal::Reanudar));
        assert_eq!((indicador(&con(Fase::SesionCaducada)), principal(&con(Fase::SesionCaducada))), (Indicador::InicieSesion, Principal::IniciarSesion));
        assert_eq!(indicador(&con(Fase::SinEspacio)), Indicador::Atencion);
        assert_eq!(indicador(&con(Fase::Error("x".into()))), Indicador::Error);
        assert_eq!(principal(&con(Fase::Verificando { hechas: 1, total: 2 })), Principal::Pausar);
        let sin = EstadoUi { detalle: Detalle { ultimo_listado: None, ..Detalle::default() }, ..con(Fase::Inactiva) };
        assert_eq!(vista(&sin, None, 0).linea, "Todavía no se ha revisado iCloud");
    }

    #[test]
    fn relistado_tras_caducar_enlaces_se_explica() {
        let mut e = EstadoUi::con_progreso(Fase::Listando { vistos: 300 }, &progreso(1, 2, 1, 2));
        e.detalle.renovando_enlaces = true;
        let v = vista(&e, None, 1000);
        assert_eq!((v.titulo.as_str(), v.indicador), ("Renovando enlaces de iCloud…", Indicador::EnVivo));
        assert_eq!(e.texto(), "Renovando enlaces de iCloud… (300 vistos)");
    }
}