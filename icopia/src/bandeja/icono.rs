//! Dibujo del icono de icopia: cuadrado redondeado con degradado y una nube blanca con una
//! flecha de descarga (y, en tamaños grandes, el disco donde cae la copia).
//! Se usa para la bandeja (teñido según el estado) y para generar `assets/icopia.ico`
//! (`cargo run --example generar_icono`).
use crate::bandeja::estado::Color;

/// Degradado del fondo: de arriba a la izquierda a abajo a la derecha.
#[derive(Clone, Copy)]
pub struct Paleta {
    pub claro: [u8; 3],
    pub oscuro: [u8; 3],
}

pub const APP: Paleta = Paleta { claro: [64, 176, 255], oscuro: [36, 92, 232] };

pub fn paleta(color: Color) -> Paleta {
    match color {
        Color::Gris => Paleta { claro: [158, 165, 176], oscuro: [104, 112, 126] },
        Color::Azul => APP,
        Color::Naranja => Paleta { claro: [255, 186, 72], oscuro: [240, 118, 0] },
        Color::Rojo => Paleta { claro: [255, 112, 104], oscuro: [206, 36, 36] },
    }
}

/// Icono de bandeja de `n`×`n` píxeles del color del estado.
pub fn rgba(color: Color, n: u32) -> (Vec<u8>, u32, u32) {
    (pintar(n, paleta(color)), n, n)
}

/// Rectángulo con esquinas redondeadas (coordenadas en la rejilla de 32 unidades).
struct Caja {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r: f32,
}

impl Caja {
    fn contiene(&self, x: f32, y: f32) -> bool {
        if x < self.x0 || x > self.x1 || y < self.y0 || y > self.y1 {
            return false;
        }
        let r = self.r.min((self.x1 - self.x0) / 2.0).min((self.y1 - self.y0) / 2.0);
        let cx = x.clamp(self.x0 + r, self.x1 - r);
        let cy = y.clamp(self.y0 + r, self.y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }
}

fn circulo(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Diseño en una rejilla de 32×32 unidades. Por debajo de 32 px se dibuja una versión
/// simplificada (nube más grande, sin disco) con los bordes rectos alineados a píxel.
struct Diseno {
    fondo: Caja,
    nube_base: Caja,
    nube_grande: (f32, f32, f32),
    nube_chica: (f32, f32, f32),
    /// Flecha recortada en la nube: (x0, x1, y0) del palo, (y base, semiancho, y punta) de la cabeza.
    palo: (f32, f32, f32),
    cabeza: (f32, f32, f32),
    disco: Option<Caja>,
    luz: Option<(f32, f32, f32)>,
}

impl Diseno {
    fn para(n: u32) -> Self {
        // Ajusta una coordenada al borde de píxel más cercano.
        let k = n as f32 / 32.0;
        let a = |v: f32| (v * k).round() / k;
        if n < 32 {
            Diseno {
                fondo: Caja { x0: 0.0, y0: 0.0, x1: 32.0, y1: 32.0, r: 7.0 },
                nube_base: Caja { x0: a(4.0), y0: a(15.0), x1: a(28.0), y1: a(26.0), r: 5.5 },
                nube_grande: (18.5, 14.0, 7.5),
                nube_chica: (10.5, 16.5, 5.5),
                palo: (a(14.0), a(18.0), 9.0),
                cabeza: (16.0, 6.5, 22.5),
                disco: None,
                luz: None,
            }
        } else {
            Diseno {
                fondo: Caja { x0: 1.0, y0: 1.0, x1: 31.0, y1: 31.0, r: 7.0 },
                nube_base: Caja { x0: 6.5, y0: 12.0, x1: 25.5, y1: 20.5, r: 4.25 },
                nube_grande: (18.0, 11.0, 6.25),
                nube_chica: (11.5, 13.5, 4.25),
                palo: (14.75, 17.25, 8.0),
                cabeza: (14.0, 4.5, 18.5),
                disco: Some(Caja { x0: 8.0, y0: 22.5, x1: 24.0, y1: 27.0, r: 2.25 }),
                luz: Some((20.75, 24.75, 1.0)),
            }
        }
    }

    fn en_fondo(&self, x: f32, y: f32) -> bool {
        self.fondo.contiene(x, y)
    }

    fn en_glifo(&self, x: f32, y: f32) -> bool {
        let (gx, gy, gr) = self.nube_grande;
        let (cx, cy, cr) = self.nube_chica;
        let nube = self.nube_base.contiene(x, y) || circulo(x, y, gx, gy, gr) || circulo(x, y, cx, cy, cr);
        if nube {
            let (p0, p1, py) = self.palo;
            let (hy, hw, tip) = self.cabeza;
            let en_palo = x >= p0 && x <= p1 && y >= py && y <= hy + 0.5;
            let centro = (p0 + p1) / 2.0;
            let en_cabeza = y >= hy && y <= tip && (x - centro).abs() <= hw * (tip - y) / (tip - hy);
            return !(en_palo || en_cabeza);
        }
        match (&self.disco, self.luz) {
            (Some(d), Some((lx, ly, lr))) => d.contiene(x, y) && !circulo(x, y, lx, ly, lr),
            _ => false,
        }
    }
}

/// Pinta el icono a `n`×`n` píxeles (RGBA sin premultiplicar) con supermuestreo.
pub fn pintar(n: u32, p: Paleta) -> Vec<u8> {
    let d = Diseno::para(n);
    let ss: u32 = if n <= 64 { 8 } else { 4 };
    let total = (ss * ss) as f32;
    let escala = 32.0 / n as f32;
    let mut v = Vec::with_capacity((n * n * 4) as usize);
    for py in 0..n {
        for px in 0..n {
            let (mut fondo, mut glifo) = (0u32, 0u32);
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = (px as f32 + (sx as f32 + 0.5) / ss as f32) * escala;
                    let y = (py as f32 + (sy as f32 + 0.5) / ss as f32) * escala;
                    if d.en_fondo(x, y) {
                        fondo += 1;
                        if d.en_glifo(x, y) {
                            glifo += 1;
                        }
                    }
                }
            }
            if fondo == 0 {
                v.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let cx = (px as f32 + 0.5) * escala;
            let cy = (py as f32 + 0.5) * escala;
            let t = ((cy + cx * 0.4) / (32.0 * 1.4)).clamp(0.0, 1.0);
            let blanco = glifo as f32 / fondo as f32;
            let mut rgb = [0u8; 3];
            for (i, canal) in rgb.iter_mut().enumerate() {
                let base = p.claro[i] as f32 + (p.oscuro[i] as f32 - p.claro[i] as f32) * t;
                *canal = (base + (255.0 - base) * blanco).round() as u8;
            }
            let alfa = (fondo as f32 / total * 255.0).round() as u8;
            v.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alfa]);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(v: &[u8], n: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * n + x) * 4) as usize;
        [v[i], v[i + 1], v[i + 2], v[i + 3]]
    }

    #[test]
    fn tamano_y_pixeles() {
        for n in [16, 20, 24, 32] {
            let (v, w, h) = rgba(Color::Azul, n);
            assert_eq!((w, h, v.len()), (n, n, (n * n * 4) as usize));
            assert_eq!(pixel(&v, n, 0, 0)[3], 0, "esquina transparente");
        }
        let (v, _, _) = rgba(Color::Naranja, 32);
        assert_eq!(pixel(&v, 32, 10, 15), [255, 255, 255, 255], "nube blanca");
        let fondo = pixel(&v, 32, 16, 2);
        assert_eq!(fondo[3], 255);
        assert!(fondo[0] > 200 && fondo[2] < 100, "fondo naranja: {fondo:?}");
        let flecha = pixel(&v, 32, 16, 12);
        assert!(flecha[2] < 100, "la flecha se recorta en la nube: {flecha:?}");
    }
}
