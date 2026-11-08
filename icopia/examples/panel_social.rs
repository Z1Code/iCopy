//! Renderiza un único panel "Copiando…" con datos de ejemplo (ninguno real) a un PNG, para la
//! imagen de redes sociales del README. Uso:
//! `cargo run --example panel_social -- salida.png [escala] [oscuro|claro] [copiando|borrando|liberado]`
//! (`borrando` y `liberado` añaden la sección de liberar espacio en iCloud).
use icopia::bandeja::estado::{Detalle, EstadoUi, Fase, Ronda};
use icopia::bandeja::panel::{vista, vista_liberar, Medidas};
use icopia::bandeja::panel_gdi::{self, Lienzo};
use icopia::progreso_liberar::{FaseLiberar, ProgresoLiberar};

fn main() {
    let mut args = std::env::args().skip(1);
    let salida = args.next().expect("ruta del PNG");
    let escala: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let oscuro = args.next().map(|s| s != "claro").unwrap_or(true);
    let modo = args.next().unwrap_or_else(|| "copiando".into());
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
    let medidas = Medidas { bytes_por_seg: Some(45_200_000.0), elementos_por_seg: None, media_ultima_ronda: Some(41_800_000.0) };
    // Liberar espacio: 11.029 elementos (74,7 GB); borrando al 21 % o ya terminado hace 5 min.
    let liberar = ProgresoLiberar {
        fase: FaseLiberar::Borrando,
        hechos: 2_350,
        total: 11_029,
        bytes_hechos: 16_100_000_000,
        bytes_total: 74_700_000_000,
        inicio_unix: ahora - 9 * 60,
        actualizado_unix: ahora - 1,
        borrados: 2_350,
        fallidos: 0,
        dias: 30,
        por_seg: Some((11_029 - 2_350) as f64 / (14.0 * 60.0)),
        ..ProgresoLiberar::nuevo(30, ahora)
    };
    let (estado, liberar) = match modo.as_str() {
        // Mientras se borra, la copia está en pausa (y al acabar, al día).
        "borrando" => (EstadoUi { fase: Fase::Pausada, verificadas: 24_224, total: 24_224, ..estado }, Some(liberar)),
        "liberado" => (
            EstadoUi { fase: Fase::Inactiva, verificadas: 24_224, total: 24_224, ..estado },
            Some(ProgresoLiberar { fase: FaseLiberar::Terminado, hechos: 11_029, borrados: 11_029, bytes_hechos: 74_700_000_000, actualizado_unix: ahora - 5 * 60, por_seg: None, ..liberar }),
        ),
        _ => (estado, None),
    };
    let estado = if liberar.is_some() {
        EstadoUi { detalle: icopia::bandeja::estado::Detalle { bytes_verificados: 310_000_000_000, actual: None, ronda: None, proxima: Some(ahora + 3 * 3600 + 55 * 60), ..estado.detalle }, ..estado }
    } else {
        estado
    };
    let mut v = vista(&estado, medidas, ahora);
    v.liberar = liberar.as_ref().and_then(|p| vista_liberar(p, ahora));
    let (w, h) = ((panel_gdi::ANCHO as f32 * escala).round() as i32, (panel_gdi::alto(&v) as f32 * escala).round() as i32);
    let mut l = Lienzo::nuevo(w, h);
    panel_gdi::pintar(&mut l, &v, oscuro, escala, None, 0.25);
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
