//! Renderiza un único panel "Copiando…" con datos de ejemplo (ninguno real) a un PNG, para la
//! imagen de redes sociales del README. Uso:
//! `cargo run --example panel_social -- salida.png [escala] [oscuro|claro]`
use icopia::bandeja::estado::{Detalle, EstadoUi, Fase, Ronda};
use icopia::bandeja::panel::{vista, Medidas};
use icopia::bandeja::panel_gdi::{self, Lienzo};

fn main() {
    let mut args = std::env::args().skip(1);
    let salida = args.next().expect("ruta del PNG");
    let escala: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let oscuro = args.next().map(|s| s != "claro").unwrap_or(true);
    let ahora = 1_000_000;
    // Ronda de 285 GB, 64 % hecho, a 45 MB/s → ~38 min restantes.
    let estado = EstadoUi {
        fase: Fase::Copiando { hechas: 9_850, total: 15_390 },
        verificadas: 18_346,
        total: 24_224,
        detalle: Detalle {
            bytes_verificados: 207_400_000_000,
            bytes_total: 310_000_000_000,
            con_error: 0,
            ultimo_listado: Some(ahora - 4 * 60),
            actual: Some("IMG_4821.HEIC (original)".into()),
            ronda: Some(Ronda { archivos: 15_390, bytes: 285_000_000_000, bytes_base: 25_000_000_000 }),
            proxima: None,
            renovando_enlaces: false,
            listado_previsto: None,
        },
    };
    let medidas = Medidas { bytes_por_seg: Some(45_200_000.0), elementos_por_seg: None, media_ultima_ronda: None };
    let (w, h) = ((panel_gdi::ANCHO as f32 * escala).round() as i32, (panel_gdi::ALTO as f32 * escala).round() as i32);
    let mut l = Lienzo::nuevo(w, h);
    panel_gdi::pintar(&mut l, &vista(&estado, medidas, ahora), oscuro, escala, None, 0.25);
    let px = l.pixeles();
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (d, s) in rgba.chunks_exact_mut(4).zip(px.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], 255]);
    }
    let archivo = std::fs::File::create(&salida).unwrap();
    let mut enc = png::Encoder::new(archivo, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
}
