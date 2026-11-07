//! Abrir el panel al pasar el ratón por el icono de la bandeja (lógica pura, con tiempos en
//! segundos para poder probarla).
//!
//! - Tras `ABRIR` segundos con el ratón sobre el icono se abre el panel (sin quitar el foco).
//! - Abierto así, sigue abierto mientras el ratón esté sobre el icono o el panel, y se cierra
//!   `CERRAR` segundos después de salir de ambos.
//! - Un clic (en el icono o dentro del panel) lo fija: entonces solo se cierra al perder el
//!   foco, con Esc o con otro clic en el icono.

pub const ABRIR: f64 = 0.4;
pub const CERRAR: f64 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orden {
    /// Abrir sin activar la ventana.
    Abrir,
    Cerrar,
}

#[derive(Debug, Default)]
pub struct Hover {
    sobre_icono: bool,
    sobre_panel: bool,
    abierto: bool,
    fijado: bool,
    /// Desde cuándo está el ratón sobre el icono con el panel cerrado.
    entro: Option<f64>,
    /// Desde cuándo está fuera de icono y panel con el panel abierto (sin fijar).
    salio: Option<f64>,
}

impl Hover {
    pub fn icono(&mut self, encima: bool, t: f64) {
        self.sobre_icono = encima;
        self.recalcular(t);
    }

    pub fn panel(&mut self, encima: bool, t: f64) {
        self.sobre_panel = encima;
        self.recalcular(t);
    }

    /// El panel se abrió o se fijó con un clic (o ganó el foco).
    pub fn fijar(&mut self) {
        self.abierto = true;
        self.fijado = true;
        self.entro = None;
        self.salio = None;
    }

    /// El panel se cerró por cualquier motivo (foco, Esc, clic en el icono, acción).
    pub fn cerrado(&mut self) {
        self.abierto = false;
        self.fijado = false;
        self.entro = None;
        self.salio = None;
    }

    pub fn sobre_icono(&self) -> bool {
        self.sobre_icono
    }

    pub fn fijado(&self) -> bool {
        self.fijado
    }

    pub fn abierto(&self) -> bool {
        self.abierto
    }

    fn recalcular(&mut self, t: f64) {
        if self.abierto {
            self.entro = None;
            if self.sobre_icono || self.sobre_panel {
                self.salio = None;
            } else if self.salio.is_none() {
                self.salio = Some(t);
            }
        } else {
            self.salio = None;
            if !self.sobre_icono {
                self.entro = None;
            } else if self.entro.is_none() {
                self.entro = Some(t);
            }
        }
    }

    /// Se llama a menudo (cada ~50 ms mientras haga falta); dice si hay que abrir o cerrar.
    pub fn tick(&mut self, t: f64) -> Option<Orden> {
        if !self.abierto {
            if self.entro.is_some_and(|e| t - e >= ABRIR) {
                self.abierto = true;
                self.entro = None;
                self.recalcular(t);
                return Some(Orden::Abrir);
            }
        } else if !self.fijado && self.salio.is_some_and(|s| t - s >= CERRAR) {
            self.cerrado();
            return Some(Orden::Cerrar);
        }
        None
    }

    /// ¿Hay un temporizador en marcha? (para despertar el bucle a menudo)
    pub fn pendiente(&self) -> bool {
        self.entro.is_some() || self.salio.is_some()
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
        nuevo && !panel.contiene(cursor) && !icono.is_some_and(|r| r.contiene(cursor))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abre_tras_un_rato_encima_y_no_si_solo_pasa() {
        let mut h = Hover::default();
        h.icono(true, 0.0);
        assert_eq!(h.tick(0.2), None);
        h.icono(false, 0.3); // solo pasaba por ahí
        assert_eq!(h.tick(1.0), None);
        assert!(!h.pendiente());
        h.icono(true, 2.0);
        assert_eq!(h.tick(2.3), None);
        assert_eq!(h.tick(2.41), Some(Orden::Abrir));
        assert!(h.abierto());
        assert_eq!(h.tick(5.0), None, "sigue abierto mientras está sobre el icono");
    }

    #[test]
    fn sigue_abierto_al_pasar_del_icono_al_panel_y_cierra_al_salir_de_ambos() {
        let mut h = Hover::default();
        h.icono(true, 0.0);
        assert_eq!(h.tick(0.5), Some(Orden::Abrir));
        h.icono(false, 1.0);
        h.panel(true, 1.2); // cruza el hueco hasta el panel a tiempo
        assert_eq!(h.tick(2.0), None);
        h.panel(false, 3.0);
        assert_eq!(h.tick(3.5), None);
        h.icono(true, 3.55); // vuelve al icono antes de que se cierre
        assert_eq!(h.tick(4.5), None);
        h.icono(false, 5.0);
        assert_eq!(h.tick(5.59), None);
        assert_eq!(h.tick(5.61), Some(Orden::Cerrar));
        assert!(!h.abierto());
    }

    #[test]
    fn fijado_con_clic_no_se_cierra_al_salir() {
        let mut h = Hover::default();
        h.icono(true, 0.0);
        assert_eq!(h.tick(0.5), Some(Orden::Abrir));
        h.panel(true, 1.0);
        h.fijar(); // clic dentro del panel
        h.panel(false, 2.0);
        assert_eq!(h.tick(10.0), None);
        assert!(h.abierto());
        h.cerrado(); // perdió el foco
        assert!(!h.abierto());
        // Tras cerrarse, el ratón sobre el icono vuelve a abrirlo con el retraso normal.
        h.icono(true, 20.0);
        assert_eq!(h.tick(20.1), None);
        assert_eq!(h.tick(20.5), Some(Orden::Abrir));
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
    fn el_boton_ya_pulsado_al_abrir_no_cuenta() {
        let panel = Rect { x0: 0, y0: 0, x1: 10, y1: 10 };
        let mut v = VigiaClics::default();
        v.reiniciar(true);
        assert!(!v.actualizar(true, (50, 50), panel, None));
        assert!(!v.actualizar(false, (50, 50), panel, None));
        assert!(v.actualizar(true, (50, 50), panel, None));
    }
}