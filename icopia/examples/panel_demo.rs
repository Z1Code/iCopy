//! Renderiza el panel de progreso de la bandeja en varios estados (claro y oscuro) a un PNG,
//! para revisarlo sin abrir la bandeja. Uso: `cargo run --example panel_demo -- salida.png [escala]`
use icopia::bandeja::estado::{Detalle, EstadoUi, Fase};
use icopia::bandeja::panel::vista;
use icopia::bandeja::panel_gdi::{self, Boton, Lienzo};

fn main() {
    let mut args = std::env::args().skip(1);
    let salida = args.next().expect("ruta del PNG");
    let escala: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let detalle = Detalle {
        bytes_verificados: 108_400_000_000,
        bytes_total: 210_000_000_000,
        con_error: 0,
        ultimo_listado: Some(1_000_000 - 5 * 60),
        actual: Some("IMG_4821.HEIC (original)".into()),
    };
    let estados = [
        (EstadoUi { fase: Fase::Copiando { hechas: 812, total: 9_400 }, verificadas: 18_200, total: 35_000, detalle: detalle.clone() }, Some(29_300_000.0), None, 0.3),
        (EstadoUi { fase: Fase::Inactiva, verificadas: 35_000, total: 35_000, detalle: Detalle { bytes_verificados: 210_000_000_000, actual: None, ..detalle.clone() } }, None, Some(Boton::Carpeta), 0.0),
        (EstadoUi { fase: Fase::Pausada, verificadas: 18_200, total: 35_000, detalle: Detalle { con_error: 3, actual: None, ..detalle.clone() } }, None, Some(Boton::Pausa), 0.0),
        (EstadoUi { fase: Fase::Listando { vistos: 12_400 }, verificadas: 0, total: 0, detalle: Detalle::default() }, None, None, 0.7),
    ];
    let (w, h) = ((panel_gdi::ANCHO as f32 * escala).round() as i32, (panel_gdi::ALTO as f32 * escala).round() as i32);
    let hueco = 24;
    let (ancho, alto) = ((w + hueco) * estados.len() as i32 + hueco, (h + hueco) * 2 + hueco);
    let mut lamina = vec![128u8; (ancho * alto * 4) as usize];
    for (fila, oscuro) in [false, true].into_iter().enumerate() {
        for (col, (e, vel, hover, pulso)) in estados.iter().enumerate() {
            let mut l = Lienzo::nuevo(w, h);
            panel_gdi::pintar(&mut l, &vista(e, *vel, 1_000_000), oscuro, escala, *hover, *pulso);
            let px = l.pixeles();
            let (ox, oy) = (hueco + col as i32 * (w + hueco), hueco + fila as i32 * (h + hueco));
            for y in 0..h {
                for x in 0..w {
                    let s = ((y * w + x) * 4) as usize;
                    let d = (((oy + y) * ancho + ox + x) * 4) as usize;
                    lamina[d..d + 4].copy_from_slice(&[px[s + 2], px[s + 1], px[s], 255]);
                }
            }
        }
    }
    let archivo = std::fs::File::create(&salida).unwrap();
    let mut enc = png::Encoder::new(archivo, ancho as u32, alto as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&lamina).unwrap();
}
