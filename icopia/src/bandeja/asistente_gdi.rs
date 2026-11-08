//! Dibujo del asistente "Liberar espacio en iCloud" con el mismo estilo que el panel: formas
//! suavizadas a mano y texto con GDI. Lo usan la ventana y `examples/asistente_demo`.
use crate::bandeja::asistente::{Asistente, Control, Paso, Rect, Restauracion, Revision, ALTO, ANCHO, CONTENIDO_Y, M, PIE_Y};
use crate::bandeja::panel::{bytes_dec, vista_liberar, TonoLiberar};
use crate::bandeja::panel_gdi::{ancho_texto, texto, texto_envuelto, Alinear, Fuente, Lienzo, Paleta, Rgb, CLARA, OSCURA};
use crate::criterio::PRESETS_DIAS;
use crate::i18n::{mes_anio, miles, t, tf, T};

/// Glifos de "Segoe MDL2 Assets".
const CHECK: &str = "\u{E73E}";
const AVISO: &str = "\u{E7BA}";

/// Rojo del botón de liberar (con texto blanco legible en los dos temas).
const ROJO_BOTON: Rgb = [196, 43, 28];
const ROJO_LLENO: Rgb = [140, 24, 12];

struct Fuentes {
    titulo: Fuente,
    grande: Fuente,
    h: Fuente,
    semi: Fuente,
    semi_peq: Fuente,
    norm: Fuente,
    peq: Fuente,
    icono: Fuente,
    icono_peq: Fuente,
}

/// Pinta el asistente entero. `t` (segundos) anima el botón de mantener y la barra indeterminada.
pub fn pintar(l: &mut Lienzo, a: &Asistente, oscuro: bool, escala: f32, t_seg: f64, ahora_unix: i64) {
    let p = if oscuro { &OSCURA } else { &CLARA };
    let e = |x: i32| x as f32 * escala;
    let er = |r: Rect| (e(r.x), e(r.y), e(r.w), e(r.h));
    let (w, h) = (l.ancho as f32, l.alto as f32);
    let f = Fuentes {
        titulo: Fuente::nueva(e(20), true),
        grande: Fuente::nueva(e(22), true),
        h: Fuente::nueva(e(16), true),
        semi: Fuente::nueva(e(13), true),
        semi_peq: Fuente::nueva(e(12), true),
        norm: Fuente::nueva(e(13), false),
        peq: Fuente::nueva(e(12), false),
        icono: Fuente::familia(e(14), false, "Segoe MDL2 Assets"),
        icono_peq: Fuente::familia(e(11), false, "Segoe MDL2 Assets"),
    };
    let ancho_util = (ANCHO - 2 * M) as f32 * escala;
    let x0 = e(M);

    // Fondo, separador bajo los pasos y pie.
    l.caja(0.0, 0.0, w, h, 0.0, p.fondo, 1.0);
    l.caja(0.0, e(100), w, escala.round().max(1.0), 0.0, p.linea, 1.0);
    l.caja(0.0, e(PIE_Y), w, h - e(PIE_Y), 0.0, p.pie, 1.0);
    l.caja(0.0, e(PIE_Y), w, escala.round().max(1.0), 0.0, p.linea, 1.0);

    // Pasos: círculo con número (o marca si ya pasó) y nombre, repartidos con el mismo hueco.
    let fuente_paso = |paso: Paso| if paso == a.paso { &f.semi_peq } else { &f.peq };
    let anchos: Vec<f32> = Paso::TODOS.iter().map(|&paso| e(26) + ancho_texto(l.dc(), fuente_paso(paso), paso.nombre())).collect();
    let hueco = (ancho_util - anchos.iter().sum::<f32>()) / 4.0;
    let xs: Vec<f32> = anchos.iter().scan(x0, |x, w| {
        let este = *x;
        *x += w + hueco;
        Some(este)
    }).collect();
    for (i, paso) in Paso::TODOS.iter().enumerate() {
        let cx = xs[i];
        let (fondo, borde) = if *paso < a.paso {
            (p.verde, p.verde)
        } else if *paso == a.paso {
            (p.boton_acento, p.boton_acento)
        } else {
            (p.fondo, p.borde_boton)
        };
        l.caja(cx, e(64), e(20), e(20), e(10), borde, 1.0);
        l.caja(cx + escala, e(64) + escala, e(20) - 2.0 * escala, e(20) - 2.0 * escala, e(10) - escala, fondo, 1.0);
    }

    // Formas de cada paso (antes que el texto).
    let botones = a.controles();
    // Anillo de foco (teclado): redondeado como el control, dibujado debajo de él.
    if let Some((c, r)) = botones.iter().find(|(c, _)| a.foco == Some(*c)) {
        let (x, y, ww, hh) = er(*r);
        let radio = match c {
            Control::Preset(_) => hh / 2.0,
            Control::Mantener => e(8),
            Control::Primario | Control::Secundario | Control::VerLista | Control::AbrirCarpeta | Control::VerBorrados | Control::Restaurar => e(4),
            _ => e(6),
        };
        let (g, s) = (e(2).round().max(2.0), e(2).round().max(2.0));
        l.caja(x - g - s, y - g - s, ww + 2.0 * (g + s), hh + 2.0 * (g + s), radio + g + s, p.texto, 1.0);
        l.caja(x - s, y - s, ww + 2.0 * s, hh + 2.0 * s, radio + s, p.fondo, 1.0);
    }
    let tarjeta = |l: &mut Lienzo, y: i32, alto: i32| {
        l.caja(x0, e(y), ancho_util, e(alto), e(8), p.borde_tarjeta, 1.0);
        l.caja(x0 + 1.0, e(y) + 1.0, ancho_util - 2.0, e(alto) - 2.0, e(8) - 1.0, p.tarjeta, 1.0);
    };
    let barra = |l: &mut Lienzo, y: i32, fraccion: Option<f64>, color: Rgb| {
        let alto = e(6);
        l.caja(x0, e(y), ancho_util, alto, e(3), p.pista, 1.0);
        match fraccion {
            Some(fr) if fr > 0.0 => l.caja(x0, e(y), (ancho_util * fr as f32).max(alto), alto, e(3), color, 1.0),
            Some(_) => {}
            None => {
                let tramo = ancho_util * 0.3;
                let x = x0 - tramo + (ancho_util + tramo) * (t_seg / 1.6).fract() as f32;
                let (a0, a1) = (x.max(x0), (x + tramo).min(x0 + ancho_util));
                if a1 > a0 {
                    l.caja(a0, e(y), a1 - a0, alto, e(3), color, 1.0);
                }
            }
        }
    };
    let vista_progreso = a.progreso.as_ref().and_then(|pr| vista_liberar(pr, ahora_unix));
    let color_tono = |tono: TonoLiberar| match tono {
        TonoLiberar::Activo => p.ambar,
        TonoLiberar::Hecho => p.verde,
        TonoLiberar::Problema => p.naranja,
    };
    let filas_resumen = filas_plan(a);
    match a.paso {
        Paso::Elegir => tarjeta(l, 470, 98),
        Paso::Revisar => match &a.revision {
            Revision::EnCurso => barra(l, 234, vista_progreso.as_ref().and_then(|v| v.fraccion), p.ambar),
            Revision::Lista(_) => tarjeta(l, 216, 16 + 26 * filas_resumen.len() as i32),
            Revision::Error(_) => {}
        },
        Paso::Confirmar => {
            let r = Asistente::RECT_MANTENER;
            let activo = a.habilitado(Control::Mantener);
            let (x, y, ww, hh) = er(r);
            if activo {
                l.caja(x, y, ww, hh, e(8), ROJO_BOTON, 1.0);
                let av = a.mantener.avance(t_seg) as f32;
                if av > 0.0 {
                    // Relleno con el borde izquierdo redondeado y el avance recto.
                    let ancho = (ww * av).max(e(8));
                    l.caja(x, y, ancho.min(e(16)), hh, e(8), ROJO_LLENO, 1.0);
                    if ancho > e(8) {
                        let fin = if av >= 1.0 { ww - e(8) } else { ancho - e(8) };
                        l.caja(x + e(8), y, fin, hh, if av >= 1.0 { e(8) } else { 0.0 }, ROJO_LLENO, 1.0);
                    }
                }
            } else {
                l.caja(x, y, ww, hh, e(8), p.borde_boton, 1.0);
                l.caja(x + 1.0, y + 1.0, ww - 2.0, hh - 2.0, e(8) - 1.0, p.boton, 1.0);
            }
        }
        Paso::EnMarcha => barra(l, 234, vista_progreso.as_ref().and_then(|v| v.fraccion).or(Some(0.0)).filter(|_| vista_progreso.is_some()), p.ambar),
        Paso::Resultado => tarjeta(l, 216, 76),
    }
    // Controles con forma: chips, casillas, botones.
    for (c, r) in &botones {
        let (x, y, ww, hh) = er(*r);
        let hover = a.hover == Some(*c);
        let activo = a.habilitado(*c);
        match c {
            Control::Preset(i) => {
                let elegido = a.preset == *i;
                let (fondo, borde) = if elegido { (p.boton_acento, p.boton_acento) } else { (if hover { p.boton_hover } else { p.boton }, p.borde_boton) };
                l.caja(x, y, ww, hh, hh / 2.0, borde, 1.0);
                l.caja(x + 1.0, y + 1.0, ww - 2.0, hh - 2.0, hh / 2.0 - 1.0, fondo, 1.0);
            }
            Control::ModoDias | Control::ModoAnios => {
                let elegido = (*c == Control::ModoDias) == (a.modo == crate::bandeja::asistente::Modo::Dias);
                let (cx, cy, d) = (x, y + (hh - e(18)) / 2.0, e(18));
                l.caja(cx, cy, d, d, d / 2.0, if elegido { p.boton_acento } else { p.secundario }, 1.0);
                l.caja(cx + e(2), cy + e(2), d - e(4), d - e(4), d / 2.0 - e(2), p.fondo, 1.0);
                if elegido {
                    l.caja(cx + e(5), cy + e(5), d - e(10), d - e(10), d / 2.0 - e(5), p.boton_acento, 1.0);
                }
            }
            Control::Anio(anio) => {
                let elegido = a.anios_elegidos.contains(anio);
                let fondo = if hover && activo { p.boton_hover } else { p.tarjeta };
                l.caja(x, y, ww, hh, e(6), if elegido { p.boton_acento } else { p.borde_tarjeta }, 1.0);
                l.caja(x + 1.0, y + 1.0, ww - 2.0, hh - 2.0, e(6) - 1.0, fondo, 1.0);
                casilla(l, p, x + e(10), y + (hh - e(16)) / 2.0, e(16), elegido, activo);
            }
            Control::Entendido => casilla(l, p, x, y + e(3), e(20), a.entendido, true),
            Control::Mantener => {}
            Control::Primario | Control::Secundario | Control::VerLista | Control::AbrirCarpeta | Control::VerBorrados | Control::Restaurar => {
                let acento = *c == Control::Primario && matches!(a.paso, Paso::Elegir | Paso::Resultado)
                    || *c == Control::Primario && a.paso == Paso::Revisar && matches!(&a.revision, Revision::Lista(r) if r.elementos > 0);
                let (fondo, borde) = if acento && activo {
                    (if hover { p.boton_acento_hover } else { p.boton_acento }, p.boton_acento)
                } else {
                    (if hover && activo { p.boton_hover } else { p.boton }, p.borde_boton)
                };
                l.caja(x, y, ww, hh, e(4), borde, 1.0);
                l.caja(x + 1.0, y + 1.0, ww - 2.0, hh - 2.0, e(4) - 1.0, fondo, 1.0);
            }
        }
    }

    // ---- Texto ----
    let dc = l.dc();
    unsafe { windows_sys::Win32::Graphics::Gdi::GdiFlush() };
    texto(dc, &f.titulo, t(T::AsVentana), p.texto, (x0, e(20), ancho_util, e(32)), Alinear::Izq);
    for (i, paso) in Paso::TODOS.iter().enumerate() {
        let cx = xs[i];
        let (marca, fuente, color_marca) = if *paso < a.paso {
            (CHECK.to_string(), &f.icono_peq, [255, 255, 255])
        } else {
            ((i + 1).to_string(), &f.peq, if *paso == a.paso { [255, 255, 255] } else { p.secundario })
        };
        texto(dc, fuente, &marca, color_marca, (cx, e(64), e(20), e(20)), Alinear::Centro);
        let color = if *paso == a.paso { p.texto } else { p.secundario };
        texto(dc, fuente_paso(*paso), paso.nombre(), color, (cx + e(26), e(64), anchos[i], e(20)), Alinear::Izq);
    }

    let s = a.seleccion();
    match a.paso {
        Paso::Elegir => {
            texto_envuelto(dc, &f.norm, t(T::AsElegirSub), p.secundario, (x0, e(CONTENIDO_Y), ancho_util, e(40)));
            texto(dc, &f.semi, t(T::AsModoDias), p.texto, (x0 + e(28), e(168), ancho_util, e(28)), Alinear::Izq);
            texto(dc, &f.semi, t(T::AsModoAnios), p.texto, (x0 + e(28), e(a.rect_modo_anios().y), ancho_util, e(28)), Alinear::Izq);
            if a.modo == crate::bandeja::asistente::Modo::Anios && a.anios.is_empty() || a.inventario.is_empty() {
                texto_envuelto(dc, &f.norm, t(T::AsSinInventario), p.secundario, (x0 + e(28), e(a.rect_modo_anios().y + 36), ancho_util - e(28), e(40)));
            }
            // Tarjeta de resumen.
            let (tx, tw) = (x0 + e(16), ancho_util - e(32));
            let (linea1, color1) = if s.elementos > 0 {
                (tf(T::AsSeLiberaran, &[("n", &miles(s.elementos)), ("b", &bytes_dec(s.bytes))]), p.texto)
            } else {
                (t(T::AsNadaSeleccionado).to_string(), p.secundario)
            };
            texto(dc, &f.h, &linea1, color1, (tx, e(480), tw, e(24)), Alinear::Izq);
            let mut linea2 = match (s.desde, s.hasta) {
                (Some(d), Some(h)) => tf(T::AsRango, &[("desde", &mes_anio(d)), ("hasta", &mes_anio(h))]),
                _ => String::new(),
            };
            if s.atipicos > 0 {
                if !linea2.is_empty() {
                    linea2.push_str(" · ");
                }
                linea2.push_str(&tf(T::AsAtipicos, &[("n", &miles(s.atipicos))]));
            }
            texto(dc, &f.peq, &linea2, p.secundario, (tx, e(504), tw, e(18)), Alinear::Izq);
            let excl = exclusiones(s);
            if excl.is_empty() && s.elementos > 0 {
                texto(dc, &f.icono_peq, CHECK, p.verde, (tx, e(527), e(16), e(18)), Alinear::Izq);
                texto(dc, &f.peq, t(T::AsTodoCopiado), p.secundario, (tx + e(20), e(526), tw - e(20), e(18)), Alinear::Izq);
            }
            if !excl.is_empty() {
                let etiqueta = t(T::AsNoSeTocan);
                let ancho_etq = ancho_texto(dc, &f.peq, etiqueta) + e(8);
                texto(dc, &f.peq, etiqueta, p.secundario, (tx, e(526), ancho_etq, e(18)), Alinear::Izq);
                let col = (tw - ancho_etq) / 2.0;
                for (i, x) in excl.iter().enumerate() {
                    let (fila, columna) = (i / 2, i % 2);
                    texto(dc, &f.peq, &format!("• {x}"), p.texto, (tx + ancho_etq + col * columna as f32, e(526 + 18 * fila as i32), col - e(6), e(18)), Alinear::Izq);
                }
            }
        }
        Paso::Revisar => match &a.revision {
            Revision::EnCurso => {
                texto(dc, &f.h, t(T::AsRevisandoTit), p.texto, (x0, e(124), ancho_util, e(24)), Alinear::Izq);
                texto_envuelto(dc, &f.norm, t(T::AsRevisandoSub), p.secundario, (x0, e(152), ancho_util, e(40)));
                progreso(dc, &f, p, vista_progreso.as_ref(), x0, ancho_util, e, color_tono, None);
            }
            Revision::Lista(r) => {
                let (titulo, color) = if r.elementos > 0 { (t(T::AsListoTit), p.verde) } else { (t(T::AsNadaQueLiberar), p.texto) };
                texto(dc, &f.h, titulo, color, (x0, e(124), ancho_util, e(24)), Alinear::Izq);
                if r.elementos > 0 {
                    texto(dc, &f.grande, &tf(T::AsListoLinea, &[("n", &miles(r.elementos)), ("b", &bytes_dec(r.bytes))]), p.texto, (x0, e(150), ancho_util, e(32)), Alinear::Izq);
                    if let (Some(d), Some(h)) = (r.desde, r.hasta) {
                        texto(dc, &f.norm, &tf(T::AsRango, &[("desde", &mes_anio(d)), ("hasta", &mes_anio(h))]), p.secundario, (x0, e(184), ancho_util, e(20)), Alinear::Izq);
                    }
                }
                for (i, (icono, texto_fila, tono)) in filas_resumen.iter().enumerate() {
                    let y = e(224 + 26 * i as i32);
                    let color = match tono {
                        Tono::Bien => p.verde,
                        Tono::Aviso => p.naranja,
                        Tono::Normal => p.secundario,
                    };
                    if let Some(ic) = icono {
                        texto(dc, &f.icono, ic, color, (x0 + e(16), y, e(20), e(26)), Alinear::Izq);
                    }
                    let c = if *tono == Tono::Normal { p.texto } else { color };
                    texto(dc, &f.norm, texto_fila, c, (x0 + e(42), y, ancho_util - e(58), e(26)), Alinear::Izq);
                }
            }
            Revision::Error(m) => {
                texto(dc, &f.h, &tf(T::AsRevisionFallo, &[("e", &"")]).trim_end_matches([':', ' ']), p.naranja, (x0, e(124), ancho_util, e(24)), Alinear::Izq);
                texto_envuelto(dc, &f.norm, m, p.texto, (x0, e(156), ancho_util, e(120)));
            }
        },
        Paso::Confirmar => {
            let n = match &a.revision {
                Revision::Lista(r) => r.elementos,
                _ => 0,
            };
            let b = bytes_dec(a.bytes_plan());
            texto(dc, &f.titulo, &tf(T::AsConfirmarTit, &[("b", &b)]), p.texto, (x0, e(124), ancho_util, e(30)), Alinear::Izq);
            texto_envuelto(dc, &f.norm, &tf(T::AsConfirmarTexto, &[("n", &miles(n))]), p.secundario, (x0, e(164), ancho_util, e(64)));
            texto_envuelto(dc, &f.norm, t(T::AsEntiendo), p.texto, (x0 + e(32), e(246), ancho_util - e(32), e(44)));
            let r = Asistente::RECT_MANTENER;
            let activo = a.habilitado(Control::Mantener);
            let color = if activo { [255, 255, 255] } else { p.secundario };
            texto(dc, &f.h, &tf(T::AsMantener, &[("b", &b)]), color, er(r), Alinear::Centro);
            let ayuda = if activo { t(T::AsMantenerAyuda) } else { t(T::AsMantenerMarca) };
            texto(dc, &f.peq, ayuda, p.secundario, (x0, e(r.y + r.h + 10), ancho_util, e(18)), Alinear::Centro);
        }
        Paso::EnMarcha => {
            texto(dc, &f.h, t(T::AsEnMarchaTit), p.texto, (x0, e(124), ancho_util, e(24)), Alinear::Izq);
            texto_envuelto(dc, &f.norm, t(T::AsEnMarchaSub), p.secundario, (x0, e(152), ancho_util, e(40)));
            let aviso = a.cancelando.then(|| t(T::AsCancelando));
            progreso(dc, &f, p, vista_progreso.as_ref(), x0, ancho_util, e, color_tono, aviso);
        }
        Paso::Resultado => {
            if let Some(r) = &a.resultado {
                let b = bytes_dec(r.bytes);
                let (titulo, color) = match (r.borrados, r.cancelado) {
                    (0, _) => (t(T::AsResultadoNada).to_string(), p.texto),
                    (_, true) => (tf(T::AsResultadoCancelado, &[("b", &b)]), p.texto),
                    _ => (tf(T::AsResultadoTit, &[("b", &b)]), p.verde),
                };
                texto(dc, &f.grande, &titulo, color, (x0, e(122), ancho_util, e(34)), Alinear::Izq);
                texto(dc, &f.norm, &tf(T::AsResultadoElementos, &[("n", &miles(r.borrados))]), p.texto, (x0, e(160), ancho_util, e(20)), Alinear::Izq);
                if r.fallidos > 0 {
                    texto(dc, &f.norm, &tf(T::AsResultadoFallidos, &[("n", &miles(r.fallidos))]), p.naranja, (x0, e(184), ancho_util, e(20)), Alinear::Izq);
                }
                texto(dc, &f.icono, AVISO, p.ambar, (x0 + e(16), e(232), e(20), e(20)), Alinear::Izq);
                texto_envuelto(dc, &f.norm, t(T::AsResultadoVaciar), p.texto, (x0 + e(44), e(230), ancho_util - e(60), e(60)));
                let (estado, color) = match &a.restauracion {
                    Restauracion::EnCurso => (t(T::AsRestaurando).to_string(), p.secundario),
                    Restauracion::Hecha { restaurados, fallidos: 0 } => (tf(T::AsRestaurados, &[("n", &miles(*restaurados))]), p.verde),
                    Restauracion::Hecha { restaurados, fallidos } => (tf(T::AsRestauradosFallos, &[("n", &miles(*restaurados)), ("f", &miles(*fallidos))]), p.naranja),
                    Restauracion::Error(m) => (tf(T::AsRestaurarFallo, &[("e", m)]), p.naranja),
                    _ => (String::new(), p.texto),
                };
                texto(dc, &f.norm, &estado, color, (x0, e(380), ancho_util, e(20)), Alinear::Izq);
            }
        }
    }
    // Rótulos de los controles.
    for (c, r) in &botones {
        let (x, y, ww, hh) = er(*r);
        let activo = a.habilitado(*c);
        match c {
            Control::Preset(i) => {
                let color = if a.preset == *i { [255, 255, 255] } else { p.texto };
                texto(dc, &f.norm, crate::bandeja::asistente::texto_preset(*i), color, (x, y, ww, hh), Alinear::Centro);
            }
            Control::Anio(anio) => {
                let datos = a.anios.iter().find(|x| x.anio == *anio);
                let color = if activo { p.texto } else { p.secundario };
                texto(dc, &f.semi, &anio.to_string(), color, (x + e(34), y + e(3), ww - e(40), e(18)), Alinear::Izq);
                if let Some(d) = datos {
                    let cifras = tf(T::AsAnioCifras, &[("n", &miles(d.elementos)), ("b", &bytes_dec(d.bytes))]);
                    texto(dc, &f.peq, &cifras, p.secundario, (x + e(34), y + e(20), ww - e(40), e(16)), Alinear::Izq);
                }
            }
            Control::Primario | Control::Secundario | Control::VerLista | Control::AbrirCarpeta | Control::VerBorrados | Control::Restaurar => {
                let rotulo = match c {
                    Control::Restaurar if a.restauracion == Restauracion::Confirmar => t(T::AsRestaurarSeguro).to_string(),
                    _ => a.nombre(*c),
                };
                let acento = *c == Control::Primario && matches!(a.paso, Paso::Elegir | Paso::Resultado)
                    || *c == Control::Primario && a.paso == Paso::Revisar && matches!(&a.revision, Revision::Lista(r) if r.elementos > 0);
                let color = if !activo { p.secundario } else if acento { p.texto_acento } else { p.texto };
                texto(dc, if acento { &f.semi } else { &f.norm }, &rotulo, color, (x, y, ww, hh), Alinear::Centro);
            }
            _ => {}
        }
    }
    let _ = PRESETS_DIAS;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tono {
    Bien,
    Aviso,
    Normal,
}

/// Filas del resumen de la revisión: (icono, texto, tono).
fn filas_plan(a: &Asistente) -> Vec<(Option<&'static str>, String, Tono)> {
    let Revision::Lista(r) = &a.revision else { return vec![] };
    let mut v = Vec::new();
    if r.elementos > 0 {
        v.push((Some(CHECK), tf(T::AsReverificados, &[("n", &miles(r.elementos))]), Tono::Bien));
    }
    if r.excluidos_reverificacion > 0 {
        v.push((Some(AVISO), tf(T::AsExcluidosReverif, &[("n", &miles(r.excluidos_reverificacion))]), Tono::Aviso));
    }
    let s = crate::criterio::Seleccion { no_copiados: r.no_verificados, no_liberables: r.no_liberables, ..a.seleccion().clone() };
    let excl = exclusiones(&s);
    if !excl.is_empty() {
        v.push((None, t(T::AsNoSeTocan).to_string(), Tono::Normal));
        v.extend(excl.into_iter().map(|x| (None, format!("   • {x}"), Tono::Normal)));
    }
    if r.atipicos > 0 {
        v.push((None, tf(T::AsAtipicos, &[("n", &miles(r.atipicos))]), Tono::Normal));
    }
    v
}

fn exclusiones(s: &crate::criterio::Seleccion) -> Vec<String> {
    [(s.no_copiados, T::AsExclNoCopiados), (s.compartida, T::AsExclCompartida), (s.no_liberables, T::AsExclNoLiberables), (s.recientes, T::AsExclRecientes)]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, c)| tf(c, &[("n", &miles(n))]))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn progreso(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    f: &Fuentes,
    p: &Paleta,
    v: Option<&crate::bandeja::panel::VistaLiberar>,
    x0: f32,
    ancho: f32,
    e: impl Fn(i32) -> f32,
    color_tono: impl Fn(TonoLiberar) -> Rgb,
    aviso: Option<&str>,
) {
    let Some(v) = v else {
        texto(dc, &f.norm, "…", p.secundario, (x0, e(206), ancho, e(22)), Alinear::Izq);
        return;
    };
    texto(dc, &f.semi, aviso.unwrap_or(&v.subtitulo), color_tono(v.tono), (x0, e(206), ancho, e(22)), Alinear::Izq);
    texto(dc, &f.norm, &v.linea, p.texto, (x0, e(250), ancho, e(22)), Alinear::Izq);
    texto(dc, &f.peq, &v.linea2, p.secundario, (x0, e(274), ancho, e(20)), Alinear::Izq);
}

fn casilla(l: &mut Lienzo, p: &Paleta, x: f32, y: f32, lado: f32, marcada: bool, activa: bool) {
    let r = lado * 0.2;
    if marcada {
        l.caja(x, y, lado, lado, r, p.boton_acento, 1.0);
        // Marca: dos trazos a mano (sin depender de fuentes de símbolos).
        let g = (lado / 9.0).max(1.5);
        let pasos = 12;
        for i in 0..=pasos {
            let k = i as f32 / pasos as f32;
            let (ax, ay) = (x + lado * (0.22 + 0.18 * k), y + lado * (0.52 + 0.18 * k));
            l.caja(ax - g / 2.0, ay - g / 2.0, g, g, g / 2.0, [255, 255, 255], 1.0);
            let (bx, by) = (x + lado * (0.40 + 0.38 * k), y + lado * (0.70 - 0.42 * k));
            l.caja(bx - g / 2.0, by - g / 2.0, g, g, g / 2.0, [255, 255, 255], 1.0);
        }
    } else {
        l.caja(x, y, lado, lado, r, if activa { p.secundario } else { p.borde_boton }, 1.0);
        l.caja(x + 1.5, y + 1.5, lado - 3.0, lado - 3.0, r - 1.0, p.tarjeta, 1.0);
    }
}

/// Tamaño del lienzo en píxeles para una escala.
pub fn tamano(escala: f32) -> (i32, i32) {
    ((ANCHO as f32 * escala).round() as i32, (ALTO as f32 * escala).round() as i32)
}
