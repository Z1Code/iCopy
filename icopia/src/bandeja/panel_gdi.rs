//! Dibujo del panel de progreso en un mapa de bits en memoria: formas suavizadas pintadas a
//! mano y texto con GDI (ClearType, Segoe UI). Lo usa la bandeja y `examples/panel_demo`.
use crate::bandeja::icono::{self, APP};
use crate::bandeja::panel::{Indicador, Vista};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi as gdi;

/// Tamaño del panel en píxeles a 96 ppp.
pub const ANCHO: i32 = 360;
pub const ALTO: i32 = 372;
const P: i32 = 16;
const PIE: i32 = 308; // empieza el pie con los botones

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boton {
    Pausa,
    Carpeta,
}

/// Rectángulo (x, y, ancho, alto) de un botón, en píxeles a 96 ppp.
pub fn rect_boton(b: Boton) -> (i32, i32, i32, i32) {
    let w = (ANCHO - 2 * P - 8) / 2;
    let y = PIE + (ALTO - PIE - 32) / 2;
    match b {
        Boton::Pausa => (P, y, w, 32),
        Boton::Carpeta => (P + w + 8, y, w, 32),
    }
}

/// Botón bajo un punto en píxeles a 96 ppp.
pub fn boton_en(x: f64, y: f64) -> Option<Boton> {
    [Boton::Pausa, Boton::Carpeta].into_iter().find(|&b| {
        let (bx, by, bw, bh) = rect_boton(b);
        x >= bx as f64 && x < (bx + bw) as f64 && y >= by as f64 && y < (by + bh) as f64
    })
}

/// ¿Usa Windows el tema oscuro para las aplicaciones?
pub fn tema_oscuro() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(false)
}

type Rgb = [u8; 3];

struct Paleta {
    fondo: Rgb,
    pie: Rgb,
    linea: Rgb,
    tarjeta: Rgb,
    borde_tarjeta: Rgb,
    texto: Rgb,
    secundario: Rgb,
    pista: Rgb,
    acento: Rgb,
    texto_acento: Rgb,
    /// Relleno del botón principal: azul profundo para que el texto blanco tenga contraste (≥ 4,5:1).
    boton_acento: Rgb,
    boton_acento_hover: Rgb,
    boton: Rgb,
    boton_hover: Rgb,
    borde_boton: Rgb,
    verde: Rgb,
    gris: Rgb,
    naranja: Rgb,
    rojo: Rgb,
}

const CLARA: Paleta = Paleta {
    fondo: [243, 243, 243],
    pie: [238, 238, 238],
    linea: [224, 224, 224],
    tarjeta: [251, 251, 251],
    borde_tarjeta: [229, 229, 229],
    texto: [27, 27, 27],
    secundario: [96, 96, 96],
    pista: [214, 214, 214],
    acento: [0, 95, 184],
    texto_acento: [255, 255, 255],
    boton_acento: [0, 95, 184],
    boton_acento_hover: [0, 84, 163],
    boton: [251, 251, 251],
    boton_hover: [246, 246, 246],
    borde_boton: [216, 216, 216],
    verde: [15, 123, 15],
    gris: [138, 138, 138],
    naranja: [202, 92, 0],
    rojo: [196, 43, 28],
};

const OSCURA: Paleta = Paleta {
    fondo: [36, 36, 36],
    pie: [29, 29, 29],
    linea: [56, 56, 56],
    tarjeta: [45, 45, 45],
    borde_tarjeta: [56, 56, 56],
    texto: [255, 255, 255],
    secundario: [200, 200, 200],
    pista: [72, 72, 72],
    acento: [96, 205, 255],
    texto_acento: [255, 255, 255],
    boton_acento: [0, 95, 184],
    boton_acento_hover: [0, 108, 207],
    boton: [55, 55, 55],
    boton_hover: [62, 62, 62],
    borde_boton: [70, 70, 70],
    verde: [108, 203, 95],
    gris: [150, 150, 150],
    naranja: [252, 160, 70],
    rojo: [255, 153, 164],
};

fn colorref(c: Rgb) -> u32 {
    c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
}

/// Mapa de bits en memoria (BGRA, de arriba abajo) con su DC.
pub struct Lienzo {
    dc: gdi::HDC,
    bmp: gdi::HBITMAP,
    viejo: gdi::HGDIOBJ,
    bits: *mut u8,
    pub ancho: i32,
    pub alto: i32,
}

impl Lienzo {
    pub fn nuevo(ancho: i32, alto: i32) -> Lienzo {
        unsafe {
            let dc = gdi::CreateCompatibleDC(std::ptr::null_mut());
            let mut info: gdi::BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<gdi::BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = ancho;
            info.bmiHeader.biHeight = -alto;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut bits = std::ptr::null_mut();
            let bmp = gdi::CreateDIBSection(dc, &info, gdi::DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let viejo = gdi::SelectObject(dc, bmp);
            Lienzo { dc, bmp, viejo, bits: bits as *mut u8, ancho, alto }
        }
    }

    pub fn dc(&self) -> gdi::HDC {
        self.dc
    }

    /// Píxeles BGRA.
    pub fn pixeles(&self) -> &[u8] {
        unsafe {
            gdi::GdiFlush();
            std::slice::from_raw_parts(self.bits, (self.ancho * self.alto * 4) as usize)
        }
    }

    fn pixeles_mut(&mut self) -> &mut [u8] {
        unsafe {
            gdi::GdiFlush();
            std::slice::from_raw_parts_mut(self.bits, (self.ancho * self.alto * 4) as usize)
        }
    }

    /// Rectángulo redondeado suavizado (coordenadas en píxeles físicos).
    fn caja(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgb, alfa: f32) {
        let (ancho, alto) = (self.ancho, self.alto);
        let px = self.pixeles_mut();
        let r = r.min(w / 2.0).min(h / 2.0);
        let (cx, cy, hx, hy) = (x + w / 2.0, y + h / 2.0, w / 2.0 - r, h / 2.0 - r);
        let x0 = (x.floor() as i32).max(0);
        let x1 = ((x + w).ceil() as i32).min(ancho);
        let y0 = (y.floor() as i32).max(0);
        let y1 = ((y + h).ceil() as i32).min(alto);
        for py in y0..y1 {
            for pxl in x0..x1 {
                let qx = (pxl as f32 + 0.5 - cx).abs() - hx;
                let qy = (py as f32 + 0.5 - cy).abs() - hy;
                let fuera = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let d = fuera + qx.max(qy).min(0.0) - r;
                // Para formas rectas, cobertura por píxel exacta en los bordes.
                let cob = (0.5 - d).clamp(0.0, 1.0)
                    * ((pxl as f32 + 1.0).min(x + w) - (pxl as f32).max(x)).clamp(0.0, 1.0)
                    * ((py as f32 + 1.0).min(y + h) - (py as f32).max(y)).clamp(0.0, 1.0);
                let a = cob * alfa;
                if a <= 0.0 {
                    continue;
                }
                let i = ((py * ancho + pxl) * 4) as usize;
                for (k, canal) in [2usize, 1, 0].into_iter().enumerate() {
                    px[i + k] = (px[i + k] as f32 * (1.0 - a) + c[canal] as f32 * a).round() as u8;
                }
                px[i + 3] = 255;
            }
        }
    }

    /// Imagen RGBA sin premultiplicar.
    fn imagen(&mut self, rgba: &[u8], n: i32, x: i32, y: i32) {
        let ancho = self.ancho;
        let px = self.pixeles_mut();
        for j in 0..n {
            for i in 0..n {
                let s = ((j * n + i) * 4) as usize;
                let a = rgba[s + 3] as f32 / 255.0;
                let d = (((y + j) * ancho + x + i) * 4) as usize;
                for (k, canal) in [2usize, 1, 0].into_iter().enumerate() {
                    px[d + k] = (px[d + k] as f32 * (1.0 - a) + rgba[s + canal] as f32 * a).round() as u8;
                }
            }
        }
    }
}

impl Drop for Lienzo {
    fn drop(&mut self) {
        unsafe {
            gdi::SelectObject(self.dc, self.viejo);
            gdi::DeleteObject(self.bmp);
            gdi::DeleteDC(self.dc);
        }
    }
}

struct Fuente(gdi::HFONT);

impl Fuente {
    fn nueva(em: f32, semibold: bool) -> Fuente {
        let nombre: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        let peso = if semibold { 600 } else { 400 };
        Fuente(unsafe {
            gdi::CreateFontW(-(em.round() as i32), 0, 0, 0, peso, 0, 0, 0, 1, 0, 0, gdi::CLEARTYPE_QUALITY as u32, 0, nombre.as_ptr())
        })
    }
}

impl Drop for Fuente {
    fn drop(&mut self) {
        unsafe { gdi::DeleteObject(self.0) };
    }
}

#[derive(Clone, Copy)]
enum Alinear {
    Izq,
    Der,
    Centro,
}

fn texto(dc: gdi::HDC, f: &Fuente, s: &str, c: Rgb, r: (f32, f32, f32, f32), al: Alinear) {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut rc = RECT { left: r.0.round() as i32, top: r.1.round() as i32, right: (r.0 + r.2).round() as i32, bottom: (r.1 + r.3).round() as i32 };
    let al = match al {
        Alinear::Izq => gdi::DT_LEFT,
        Alinear::Der => gdi::DT_RIGHT,
        Alinear::Centro => gdi::DT_CENTER,
    };
    unsafe {
        let viejo = gdi::SelectObject(dc, f.0);
        gdi::SetTextColor(dc, colorref(c));
        gdi::SetBkMode(dc, gdi::TRANSPARENT as i32);
        gdi::DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut rc, al | gdi::DT_SINGLELINE | gdi::DT_VCENTER | gdi::DT_END_ELLIPSIS | gdi::DT_NOPREFIX);
        gdi::SelectObject(dc, viejo);
    }
}

fn ancho_texto(dc: gdi::HDC, f: &Fuente, s: &str) -> f32 {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        let viejo = gdi::SelectObject(dc, f.0);
        gdi::DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut rc, gdi::DT_CALCRECT | gdi::DT_SINGLELINE | gdi::DT_NOPREFIX);
        gdi::SelectObject(dc, viejo);
    }
    rc.right as f32
}

/// Pinta el panel completo. `pulso` (0–1) anima el punto del indicador "En vivo".
pub fn pintar(l: &mut Lienzo, v: &Vista, oscuro: bool, escala: f32, hover: Option<Boton>, pulso: f32) {
    let p = if oscuro { &OSCURA } else { &CLARA };
    let e = |x: i32| x as f32 * escala;
    let (w, h) = (l.ancho as f32, l.alto as f32);

    // Fondo, pie y su línea.
    l.caja(0.0, 0.0, w, h, 0.0, p.fondo, 1.0);
    l.caja(0.0, e(PIE), w, h - e(PIE), 0.0, p.pie, 1.0);
    l.caja(0.0, e(PIE), w, escala.round().max(1.0), 0.0, p.linea, 1.0);

    // Cabecera: icono, nombre e indicador.
    let lado = e(20).round() as i32;
    let glifo = icono::pintar(lado as u32, APP);
    l.imagen(&glifo, lado, e(P).round() as i32, e(18).round() as i32);
    let color_ind = match v.indicador {
        Indicador::EnVivo => p.verde,
        Indicador::AlDia | Indicador::EnEspera => p.gris,
        Indicador::EnPausa | Indicador::InicieSesion | Indicador::Atencion => p.naranja,
        Indicador::Error => p.rojo,
    };
    let f_peq = Fuente::nueva(e(12), false);
    let f_norm = Fuente::nueva(e(13), false);
    let f_nombre = Fuente::nueva(e(14), true);
    let f_titulo = Fuente::nueva(e(18), true);
    let f_boton = Fuente::nueva(e(13), true);
    let etiqueta = v.indicador.texto();
    let ancho_etq = ancho_texto(l.dc, &f_peq, etiqueta);
    let punto_x = w - e(P) - ancho_etq - e(14);
    let punto_y = e(28);
    if v.indicador.late() {
        let radio = e(4) + e(6) * pulso;
        l.caja(punto_x + e(4) - radio, punto_y - radio, radio * 2.0, radio * 2.0, radio, color_ind, 0.35 * (1.0 - pulso));
    }
    l.caja(punto_x, punto_y - e(4), e(8), e(8), e(4), color_ind, 1.0);

    // Bloques de progreso (formas primero, luego todo el texto con GDI).
    let ancho_util = w - 2.0 * e(P);
    let barra_y = e(92);
    l.caja(e(P), barra_y, ancho_util, e(4), e(2), p.pista, 1.0);
    match v.fraccion {
        Some(f) if f > 0.0 => l.caja(e(P), barra_y, (ancho_util * f as f32).max(e(4)), e(4), e(2), p.acento, 1.0),
        Some(_) => {}
        None => {
            // Indeterminada: un tramo que recorre la barra.
            let tramo = ancho_util * 0.3;
            let x = e(P) - tramo + (ancho_util + tramo) * pulso;
            let (x0, x1) = (x.max(e(P)), (x + tramo).min(e(P) + ancho_util));
            if x1 > x0 {
                l.caja(x0, barra_y, x1 - x0, e(4), e(2), p.acento, 1.0);
            }
        }
    }
    let tarjeta_y = e(152);
    let fila = e(34);
    l.caja(e(P), tarjeta_y, ancho_util, fila * 4.0 + e(4), e(8), p.borde_tarjeta, 1.0);
    l.caja(e(P) + 1.0, tarjeta_y + 1.0, ancho_util - 2.0, fila * 4.0 + e(4) - 2.0, e(8) - 1.0, p.tarjeta, 1.0);
    for i in 1..4 {
        l.caja(e(P + 12), tarjeta_y + e(2) + fila * i as f32, ancho_util - e(24), escala.round().max(1.0), 0.0, p.linea, 0.8);
    }

    // Botones.
    for b in [Boton::Pausa, Boton::Carpeta] {
        let (bx, by, bw, bh) = rect_boton(b);
        let (fondo, borde) = match b {
            Boton::Pausa => (if hover == Some(b) { p.boton_acento_hover } else { p.boton_acento }, p.boton_acento),
            Boton::Carpeta => (if hover == Some(b) { p.boton_hover } else { p.boton }, p.borde_boton),
        };

        l.caja(e(bx), e(by), e(bw), e(bh), e(4), borde, 1.0);
        l.caja(e(bx) + 1.0, e(by) + 1.0, e(bw) - 2.0, e(bh) - 2.0, e(4) - 1.0, fondo, 1.0);
    }

    let dc = l.dc;
    unsafe { gdi::GdiFlush() };
    texto(dc, &f_nombre, "icopia", p.texto, (e(P + 28), e(16), e(120), e(24)), Alinear::Izq);
    texto(dc, &f_peq, etiqueta, p.secundario, (w - e(P) - ancho_etq - e(2), e(16), ancho_etq + e(2), e(24)), Alinear::Der);
    texto(dc, &f_titulo, &v.titulo, p.texto, (e(P), e(50), ancho_util, e(32)), Alinear::Izq);
    texto(dc, &f_peq, &v.linea, p.texto, (e(P), e(104), ancho_util, e(18)), Alinear::Izq);
    texto(dc, &f_peq, &v.linea_total, p.secundario, (e(P), e(124), ancho_util, e(18)), Alinear::Izq);
    let filas = [("Ahora", &v.ahora), (v.etiqueta_velocidad, &v.velocidad), ("Último listado de iCloud", &v.listado), ("Errores", &v.errores)];
    for (i, (nombre, valor)) in filas.iter().enumerate() {
        let y = tarjeta_y + e(2) + fila * i as f32;
        let x = e(P + 12);
        let ancho = ancho_util - e(24);
        texto(dc, &f_norm, nombre, p.secundario, (x, y, ancho, fila), Alinear::Izq);
        let ancho_nombre = ancho_texto(dc, &f_norm, nombre) + e(16);
        texto(dc, &f_norm, valor, p.texto, (x + ancho_nombre, y, ancho - ancho_nombre, fila), Alinear::Der);
    }
    for b in [Boton::Pausa, Boton::Carpeta] {
        let (bx, by, bw, bh) = rect_boton(b);
        let (rotulo, color) = match b {
            Boton::Pausa => (v.principal.texto(), p.texto_acento),
            Boton::Carpeta => ("Abrir carpeta", p.texto),
        };
        let fuente = if b == Boton::Pausa { &f_boton } else { &f_norm };
        texto(dc, fuente, rotulo, color, (e(bx), e(by), e(bw), e(bh)), Alinear::Centro);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminancia(c: Rgb) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    }

    fn contraste(a: Rgb, b: Rgb) -> f64 {
        let (x, y) = (luminancia(a), luminancia(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn texto_del_boton_principal_se_lee_en_ambos_temas() {
        for p in [&CLARA, &OSCURA] {
            assert!(contraste(p.texto_acento, p.boton_acento) >= 4.5);
            assert!(contraste(p.texto_acento, p.boton_acento_hover) >= 4.5);
            assert!(contraste(p.texto, p.boton) >= 4.5);
        }
    }
}