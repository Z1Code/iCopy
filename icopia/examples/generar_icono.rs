//! Genera `assets/icopia.ico` (16–256 px) y `assets/icopia-aviso.png` (icono de las
//! notificaciones) a partir del dibujo de `bandeja::icono`.
//! Uso: `cargo run --example generar_icono [vista_previa.png]`
use icopia::bandeja::estado::Color;
use icopia::bandeja::icono::{self, APP};
use std::path::Path;

const TAMANOS: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

/// Entrada de ICO en formato DIB de 32 bits (BGRA de abajo arriba + máscara AND).
fn dib(rgba: &[u8], n: u32) -> Vec<u8> {
    let fila_mascara = n.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, n, n * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(n * n * 4 + fila_mascara * n).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..n).rev() {
        for x in 0..n {
            let i = ((y * n + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    for y in (0..n).rev() {
        let mut fila = vec![0u8; fila_mascara as usize];
        for x in 0..n {
            if rgba[((y * n + x) * 4 + 3) as usize] < 128 {
                fila[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&fila);
    }
    out
}

fn ico() -> Vec<u8> {
    let imagenes: Vec<(u32, Vec<u8>)> = TAMANOS
        .iter()
        .map(|&n| {
            let px = icono::pintar(n, APP);
            (n, if n >= 256 { png(&px, n, n) } else { dib(&px, n) })
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(imagenes.len() as u16).to_le_bytes());
    let mut desplazamiento = 6 + 16 * imagenes.len() as u32;
    for (n, datos) in &imagenes {
        let lado = if *n >= 256 { 0 } else { *n as u8 };
        out.extend_from_slice(&[lado, lado, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(datos.len() as u32).to_le_bytes());
        out.extend_from_slice(&desplazamiento.to_le_bytes());
        desplazamiento += datos.len() as u32;
    }
    for (_, datos) in &imagenes {
        out.extend_from_slice(datos);
    }
    out
}

/// Lámina con todos los tamaños (a 1× y ampliados 4×) sobre fondo claro y oscuro,
/// más los colores de la bandeja.
fn vista_previa(ruta: &Path) {
    let (ancho, alto) = (1400u32, 760u32);
    let mut lienzo = vec![0u8; (ancho * alto * 4) as usize];
    let poner = |x: u32, y: u32, c: [u8; 4], lienzo: &mut Vec<u8>| {
        let i = ((y * ancho + x) * 4) as usize;
        let a = c[3] as f32 / 255.0;
        for k in 0..3 {
            lienzo[i + k] = (c[k] as f32 * a + lienzo[i + k] as f32 * (1.0 - a)).round() as u8;
        }
        lienzo[i + 3] = 255;
    };
    for y in 0..alto {
        let gris = if y < alto / 2 { 243 } else { 32 };
        for x in 0..ancho {
            poner(x, y, [gris, gris, gris, 255], &mut lienzo);
        }
    }
    let dibujar = |px: &[u8], n: u32, ox: u32, oy: u32, zoom: u32, lienzo: &mut Vec<u8>| {
        for y in 0..n * zoom {
            for x in 0..n * zoom {
                let i = (((y / zoom) * n + x / zoom) * 4) as usize;
                poner(ox + x, oy + y, [px[i], px[i + 1], px[i + 2], px[i + 3]], lienzo);
            }
        }
    };
    for oy in [12u32, alto / 2 + 12] {
        let mut x = 12;
        for &n in &TAMANOS[..7] {
            let px = icono::pintar(n, APP);
            dibujar(&px, n, x, oy, 1, &mut lienzo);
            dibujar(&px, n, x, oy + 80, 4, &mut lienzo);
            x += n * 4 + 12;
        }
        let px = icono::pintar(256, APP);
        dibujar(&px, 256, ancho - 268, oy + 80, 1, &mut lienzo);
        let mut x = 12;
        for c in [Color::Gris, Color::Azul, Color::Naranja, Color::Rojo] {
            for &n in &[16u32, 20, 24, 32] {
                let (px, _, _) = icono::rgba(c, n);
                dibujar(&px, n, x, oy + 340 - n, 1, &mut lienzo);
                x += n + 6;
            }
            x += 18;
        }
    }
    std::fs::write(ruta, png(&lienzo, ancho, alto)).unwrap();
}

fn main() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("icopia.ico"), ico()).unwrap();
    std::fs::write(assets.join("icopia-aviso.png"), png(&icono::pintar(96, APP), 96, 96)).unwrap();
    if let Some(ruta) = std::env::args().nth(1) {
        vista_previa(Path::new(&ruta));
    }
    println!("icono generado en {}", assets.display());
}
