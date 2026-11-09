//! Renderiza el asistente "Liberar espacio en iCloud" con datos de ejemplo (ninguno real) a un
//! PNG, para revisarlo sin la bandeja. Uso:
//! `cargo run --example asistente_demo -- salida.png [escala] [oscuro|claro] [paso] [es|en]`
//! con paso = elegir | anios | revisando | revisado | confirmar | manteniendo | enmarcha | esperando | resultado.

// Dibuja con GDI: solo existe en Windows.
#[cfg(windows)]
mod demo {
use icopia::bandeja::asistente::{Accion, Asistente, Control, EventoLiberar, Paso, ResumenFinal, ResumenPlan};
use icopia::bandeja::asistente_gdi;
use icopia::bandeja::panel_gdi::Lienzo;
use icopia::criterio::FilaInventario;
use icopia::i18n::{con_idioma, Idioma};
use icopia::progreso_liberar::{FaseLiberar, ProgresoLiberar};

fn inventario(ahora: i64) -> Vec<FilaInventario> {
    // Unos 12 años de fotos, con más cada año; algunas compartidas, sin copiar o recientes.
    let mut v = Vec::new();
    let mut semilla = 7u64;
    let mut azar = || {
        semilla = semilla.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (semilla >> 33) as f64 / (1u64 << 31) as f64
    };
    for anio in 2015..=2026 {
        let n = 300 + (anio - 2015) * 160;
        for i in 0..n {
            let fecha = chrono::NaiveDate::from_ymd_opt(anio, 1, 1).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() + (i as i64 * 365 * 86_400) / n as i64;
            if fecha > ahora - 86_400 {
                continue;
            }
            let r = azar();
            v.push(FilaInventario {
                anio,
                fecha_unix: fecha,
                agregado_unix: (r < 0.004).then_some(ahora - 5 * 86_400),
                personal: r > 0.03,
                liberable: !(0.03..0.035).contains(&r),
                completo: !(0.035..0.05).contains(&r) || anio < 2020,
                bytes: (2_500_000.0 + azar() * 9_000_000.0) as u64,
                atipico: (0.05..0.051).contains(&r),
            });
        }
    }
    v
}

pub fn main() {
    let mut args = std::env::args().skip(1);
    let salida = args.next().expect("ruta del PNG");
    let escala: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let oscuro = args.next().map(|s| s != "claro").unwrap_or(true);
    let paso = args.next().unwrap_or_else(|| "elegir".into());
    let idioma = args.next().and_then(|s| Idioma::desde_codigo(&s)).unwrap_or(Idioma::Es);
    let ahora = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap().and_hms_opt(18, 0, 0).unwrap().and_utc().timestamp();
    con_idioma(idioma, || {
        let mut a = Asistente::nuevo(inventario(ahora), ahora);
        let plan = ResumenPlan {
            elementos: 11_029,
            bytes: 74_700_000_000,
            desde: Some(ahora - 11 * 365 * 86_400),
            hasta: Some(ahora - 32 * 86_400),
            reverificados: 11_031,
            excluidos_reverificacion: 2,
            no_verificados: 214,
            no_liberables: 31,
            atipicos: 12,
            lista: Some("plan_liberar.csv".into()),
        };
        let progreso = |fase, hechos, total, bytes_hechos, bytes_total| ProgresoLiberar {
            fase,
            hechos,
            total,
            bytes_hechos,
            bytes_total,
            actualizado_unix: ahora - 1,
            borrados: if fase == FaseLiberar::Borrando { hechos } else { 0 },
            por_seg: Some((total - hechos) as f64 / (14.0 * 60.0)),
            ..ProgresoLiberar::nuevo(30, ahora - 600)
        };
        let mut t = 0.0;
        match paso.as_str() {
            "elegir" => {
                a.activar(Control::Preset(3));
                a.foco = None;
            }
            "anios" => {
                a.activar(Control::ModoAnios);
                for anio in [2015, 2016, 2017, 2018] {
                    a.activar(Control::Anio(anio));
                }
                a.hover = Some(Control::Anio(2019));
                a.foco = None;
            }
            "revisando" => {
                a.activar(Control::Primario);
                a.progreso = Some(progreso(FaseLiberar::Reverificando, 4_120, 11_031, 27_900_000_000, 74_710_000_000));
                a.foco = None;
            }
            "revisado" | "confirmar" | "manteniendo" | "enmarcha" | "esperando" | "resultado" => {
                a.activar(Control::Primario);
                a.evento(EventoLiberar::Plan(plan));
                a.foco = None;
                if paso != "revisado" {
                    a.activar(Control::Primario);
                    a.foco = None;
                }
                if paso != "revisado" && paso != "confirmar" {
                    a.activar(Control::Entendido);
                    a.pulsar_mantener(0.0);
                    t = 1.2;
                    a.foco = Some(Control::Mantener);
                }
                if paso == "enmarcha" || paso == "esperando" || paso == "resultado" {
                    assert_eq!(a.tick(2.0), Some(Accion::Ejecutar));
                    a.progreso = Some(progreso(FaseLiberar::Borrando, 2_350, 11_029, 16_100_000_000, 74_700_000_000));
                    a.foco = None;
                }
                if paso == "esperando" {
                    // Apple frenó los borrados: se espera y se reanuda solo.
                    let mut p = progreso(FaseLiberar::Borrando, 9_800, 11_029, 66_300_000_000, 74_700_000_000);
                    p.espera_hasta = Some(ahora + 4 * 60);
                    p.por_seg = None;
                    a.progreso = Some(p);
                }
                if paso == "resultado" {
                    a.evento(EventoLiberar::Terminado(ResumenFinal {
                        borrados: 11_027,
                        bytes: 74_690_000_000,
                        fallidos: 2,
                        cancelado: false,
                        sesion: "20261107-180000".into(),
                        csv: Some("borrados.csv".into()),
                    }));
                    a.foco = None;
                    a.hover = Some(Control::Restaurar);
                }
            }
            otro => panic!("paso desconocido: {otro}"),
        }
        assert!(Paso::TODOS.contains(&a.paso));
        let (w, h) = asistente_gdi::tamano(escala);
        let mut l = Lienzo::nuevo(w, h);
        asistente_gdi::pintar(&mut l, &a, oscuro, escala, t, ahora);
        let px = l.pixeles();
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (d, s) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<4>().0) {
            *d = [s[2], s[1], s[0], 255];
        }
        let archivo = std::fs::File::create(&salida).unwrap();
        let mut enc = png::Encoder::new(archivo, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    });
}

}

fn main() {
    #[cfg(windows)]
    demo::main();
    #[cfg(not(windows))]
    eprintln!("Este ejemplo dibuja con GDI: solo funciona en Windows.");
}
