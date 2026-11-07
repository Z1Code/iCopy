//! Icono redondo de 32×32 del color del estado, con un anillo blanco ("lente").
use crate::bandeja::estado::Color;

pub fn rgba(color: Color) -> (Vec<u8>, u32, u32) {
    const N: u32 = 32;
    let (r, g, b) = match color {
        Color::Gris => (120u8, 120u8, 120u8),
        Color::Azul => (30, 120, 230),
        Color::Naranja => (240, 150, 20),
        Color::Rojo => (220, 40, 40),
    };
    let c = (N as f32 - 1.0) / 2.0;
    let mut v = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = if d <= 13.0 { 255 } else if d <= 14.0 { ((14.0 - d) * 255.0) as u8 } else { 0 };
            let (pr, pg, pb) = if (5.0..=7.0).contains(&d) { (255, 255, 255) } else { (r, g, b) };
            v.extend_from_slice(&[pr, pg, pb, a]);
        }
    }
    (v, N, N)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tamano_y_pixeles() {
        let (v, w, h) = rgba(Color::Azul);
        assert_eq!((w, h, v.len()), (32, 32, 32 * 32 * 4));
        assert_eq!(v[3], 0, "esquina transparente");
        let i = ((4 * 32 + 15) * 4) as usize; // (x=15, y=4): dentro del círculo, fuera del anillo
        assert_eq!(&v[i..i + 4], &[30, 120, 230, 255]);
    }
}
