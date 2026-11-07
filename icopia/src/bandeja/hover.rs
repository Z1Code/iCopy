//! Abrir el panel al pasar el ratón por el icono de la bandeja (lógica pura, con tiempos en
//! segundos para poder probarla).
//!
//! - Tras `ABRIR` segundos con el ratón sobre el icono se abre el panel (sin quitar el foco).
//! - Abierto así (sin fijar), se cierra `CERRAR` segundos después de que el ratón salga del
//!   panel. Estar sobre el icono solo lo mantiene abierto si el icono avisó hace poco (menos de
//!   `ICONO_RECIENTE`): en el área de iconos ocultos de Windows 11 su posición y sus avisos de
//!   salida no son fiables, así que ante la duda se cierra.
//! - Seguridad: sin fijar, se cierra si el ratón lleva `SEGURIDAD` segundos sin pasar por el
//!   panel, pase lo que pase.
//! - Un clic (en el icono o dentro) lo fija: entonces solo se cierra con un clic fuera, al
//!   perder el foco, con Esc o con otro clic en el icono.

pub const ABRIR: f64 = 0.4;
pub const CERRAR: f64 = 0.6;
pub const ICONO_RECIENTE: f64 = 0.3;
pub const SEGURIDAD: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orden {
    /// Abrir sin activar la ventana.
    Abrir,
    Cerrar,
}

/// Dónde está el cursor ahora (lo mide la bandeja en cada vuelta, ~50 ms).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    /// Dentro del rectángulo del panel.
    pub en_panel: bool,
    /// Dentro del rectángulo del icono, y ese rectángulo parece válido (no vacío, en pantalla).
    pub en_icono: bool,
}

#[derive(Debug, Default)]
pub struct Hover {
    abierto: bool,
    fijado: bool,
    /// Desde cuándo está el ratón sobre el icono con el panel cerrado.
    entro: Option<f64>,
    /// Último aviso del icono (entrar/mover).
    aviso_icono: Option<f64>,
    /// Desde cuándo está fuera (con el panel abierto sin fijar).
    salio: Option<f64>,
    /// Última vez que el ratón estuvo sobre el panel (o cuándo se abrió).
    visto_en_panel: f64,
}

impl Hover {
    /// El icono avisa de que el ratón entró o se movió sobre él.
    pub fn aviso_icono(&mut self, t: f64) {
        self.aviso_icono = Some(t);
        if !self.abierto && self.entro.is_none() {
            self.entro = Some(t);
        }
    }

    /// El icono avisa de que el ratón salió (no siempre llega).
    pub fn salio_icono(&mut self) {
        self.entro = None;
        self.aviso_icono = None;
    }

    /// El panel se abrió o se fijó con un clic (o ganó el foco).
    pub fn fijar(&mut self) {
        self.abierto = true;
        self.fijado = true;
        self.entro = None;
        self.salio = None;
    }

    /// El panel se cerró por cualquier motivo (foco, Esc, clic, acción).
    pub fn cerrado(&mut self) {
        self.abierto = false;
        self.fijado = false;
        self.entro = None;
        self.salio = None;
    }

    pub fn fijado(&self) -> bool {
        self.fijado
    }

    pub fn abierto(&self) -> bool {
        self.abierto
    }

    /// Se llama a menudo (cada ~50 ms mientras haga falta); dice si hay que abrir o cerrar.
    pub fn tick(&mut self, t: f64, c: Cursor) -> Option<Orden> {
        if !self.abierto {
            if !c.en_icono {
                // Sin el cursor sobre un icono válido no se abre (y se olvida la entrada).
                self.entro = None;
                return None;
            }
            if self.entro.is_some_and(|e| t - e >= ABRIR) {
                self.abierto = true;
                self.entro = None;
                self.salio = None;
                self.visto_en_panel = t;
                return Some(Orden::Abrir);
            }
            return None;
        }
        if self.fijado {
            return None;
        }
        if c.en_panel {
            self.visto_en_panel = t;
        }
        let icono_fiable = c.en_icono && self.aviso_icono.is_some_and(|a| t - a <= ICONO_RECIENTE);
        if c.en_panel || icono_fiable {
            self.salio = None;
        } else if self.salio.is_none() {
            self.salio = Some(t);
        }
        if self.salio.is_some_and(|s| t - s >= CERRAR) || t - self.visto_en_panel >= SEGURIDAD {
            self.cerrado();
            return Some(Orden::Cerrar);
        }
        None
    }

    /// ¿Hay que vigilar el cursor a menudo?
    pub fn pendiente(&self) -> bool {
        self.entro.is_some() || self.abierto && !self.fijado
    }
}
/// Rectángulo en píxeles de pantalla (x1/y1 excluidos).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn contiene(&self, (x, y): (i32, i32)) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// ¿Un clic en `punto` cierra el panel? Fuera del panel sí; en el icono no (ese clic alterna).
pub fn clic_fuera(punto: (i32, i32), panel: Rect, icono: Option<Rect>) -> bool {
    !panel.contiene(punto) && !icono.is_some_and(|r| r.contiene(punto))
}

/// Detecta un clic fuera del panel (para cerrarlo aunque nunca haya tenido el foco).
/// Los clics en el icono de la bandeja no cuentan: esos los atiende el propio icono (alternar).
#[derive(Debug, Default)]
pub struct VigiaClics {
    pulsado: bool,
}

impl VigiaClics {
    /// Al abrir el panel: un botón que ya estaba pulsado no cuenta como clic nuevo.
    pub fn reiniciar(&mut self, pulsado: bool) {
        self.pulsado = pulsado;
    }

    /// pulsado: algún botón del ratón está abajo ahora. Devuelve 	rue si hay que cerrar.
    pub fn actualizar(&mut self, pulsado: bool, cursor: (i32, i32), panel: Rect, icono: Option<Rect>) -> bool {
        let nuevo = pulsado && !self.pulsado;
        self.pulsado = pulsado;
        nuevo && clic_fuera(cursor, panel, icono)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const FUERA: Cursor = Cursor { en_panel: false, en_icono: false };
    const ICONO: Cursor = Cursor { en_panel: false, en_icono: true };
    const PANEL: Cursor = Cursor { en_panel: true, en_icono: false };

    #[test]
    fn abre_tras_un_rato_sobre_el_icono_y_no_si_solo_pasa() {
        let mut h = Hover::default();
        h.aviso_icono(0.0);
        assert_eq!(h.tick(0.2, ICONO), None);
        assert_eq!(h.tick(0.3, FUERA), None, "se fue antes de tiempo");
        assert_eq!(h.tick(1.0, ICONO), None, "sin aviso nuevo del icono no cuenta");
        h.aviso_icono(2.0);
        assert_eq!(h.tick(2.3, ICONO), None);
        assert_eq!(h.tick(2.41, ICONO), Some(Orden::Abrir));
        assert!(h.abierto());
    }

    #[test]
    fn no_abre_si_el_rectangulo_del_icono_no_vale() {
        // Aviso del icono pero el cursor no está en un rectángulo válido (p. ej. el desplegable
        // de iconos ocultos ya se cerró): no se abre.
        let mut h = Hover::default();
        h.aviso_icono(0.0);
        assert_eq!(h.tick(0.5, FUERA), None);
        assert_eq!(h.tick(1.0, FUERA), None);
        assert!(!h.abierto());
    }

    fn abierto_en(t: f64) -> Hover {
        let mut h = Hover::default();
        h.aviso_icono(t - 0.5);
        assert_eq!(h.tick(t, ICONO), Some(Orden::Abrir));
        h
    }

    #[test]
    fn se_cierra_al_salir_del_panel() {
        let mut h = abierto_en(1.0);
        h.aviso_icono(1.1);
        assert_eq!(h.tick(1.2, ICONO), None);
        assert_eq!(h.tick(1.4, PANEL), None);
        assert_eq!(h.tick(3.0, PANEL), None, "mientras está encima sigue abierto");
        assert_eq!(h.tick(3.1, FUERA), None);
        assert_eq!(h.tick(3.5, FUERA), None);
        assert_eq!(h.tick(3.71, FUERA), Some(Orden::Cerrar));
        assert!(!h.abierto());
    }

    #[test]
    fn icono_con_rectangulo_viejo_no_lo_mantiene_abierto() {
        // El cursor "está" en el rectángulo del icono, pero el icono no avisa desde hace rato
        // (desplegable de iconos ocultos cerrado, rectángulo caducado): se cierra igual.
        let mut h = abierto_en(1.0);
        h.aviso_icono(1.0);
        assert_eq!(h.tick(1.2, PANEL), None);
        assert_eq!(h.tick(1.5, ICONO), None);
        assert_eq!(h.tick(2.0, ICONO), None);
        assert_eq!(h.tick(2.11, ICONO), Some(Orden::Cerrar));
    }

    #[test]
    fn seguridad_cierra_aunque_el_icono_siga_avisando() {
        let mut h = abierto_en(1.0);
        let mut t = 1.0;
        let mut orden = None;
        while orden.is_none() && t < 5.0 {
            t += 0.05;
            h.aviso_icono(t);
            orden = h.tick(t, ICONO);
        }
        assert_eq!(orden, Some(Orden::Cerrar));
        assert!((t - (1.0 + SEGURIDAD)).abs() < 0.06, "cerró a los {t}");
    }

    #[test]
    fn fijado_no_se_cierra_solo_y_luego_reabre_con_retraso() {
        let mut h = abierto_en(1.0);
        h.fijar();
        assert_eq!(h.tick(10.0, FUERA), None);
        assert!(h.abierto() && !h.pendiente());
        h.cerrado();
        h.aviso_icono(20.0);
        assert_eq!(h.tick(20.1, ICONO), None);
        assert_eq!(h.tick(20.5, ICONO), Some(Orden::Abrir));
    }
    #[test]
    fn clic_fuera_cierra_y_dentro_o_en_el_icono_no() {
        let panel = Rect { x0: 100, y0: 100, x1: 400, y1: 500 };
        let icono = Some(Rect { x0: 450, y0: 520, x1: 480, y1: 550 });
        let mut v = VigiaClics::default();
        // Botón suelto: nada.
        assert!(!v.actualizar(false, (10, 10), panel, icono));
        // Pulsar dentro del panel no cierra; mantenerlo pulsado y salir tampoco (no es un clic nuevo).
        assert!(!v.actualizar(true, (200, 200), panel, icono));
        assert!(!v.actualizar(true, (10, 10), panel, icono));
        assert!(!v.actualizar(false, (10, 10), panel, icono));
        // Clic en el icono: lo atiende el icono (alterna), no esto.
        assert!(!v.actualizar(true, (460, 530), panel, icono));
        assert!(!v.actualizar(false, (460, 530), panel, icono));
        // Clic fuera de ambos: cierra, una sola vez por pulsación.
        assert!(v.actualizar(true, (10, 10), panel, icono));
        assert!(!v.actualizar(true, (10, 10), panel, icono));
        // Borde: x1/y1 quedan fuera.
        assert!(!v.actualizar(false, (400, 200), panel, icono));
        assert!(v.actualizar(true, (400, 200), panel, icono));
    }

    #[test]
    fn clic_del_gancho_fuera_dentro_y_en_el_icono() {
        let panel = Rect { x0: 100, y0: 100, x1: 400, y1: 500 };
        let icono = Some(Rect { x0: 450, y0: 520, x1: 480, y1: 550 });
        assert!(clic_fuera((10, 10), panel, icono));
        assert!(!clic_fuera((150, 150), panel, icono));
        assert!(!clic_fuera((460, 530), panel, icono));
        assert!(clic_fuera((460, 530), panel, None), "sin icono válido, todo lo de fuera cierra");
    }

    #[test]
    fn el_boton_ya_pulsado_al_abrir_no_cuenta() {
        let panel = Rect { x0: 0, y0: 0, x1: 10, y1: 10 };
        let mut v = VigiaClics::default();
        v.reiniciar(true);
        assert!(!v.actualizar(true, (50, 50), panel, None));
        assert!(!v.actualizar(false, (50, 50), panel, None));
        assert!(v.actualizar(true, (50, 50), panel, None));
    }
}