mod comun;
use comun::*;
use icopia::bandeja::estado::{EventoUi, Fase};
use icopia::bandeja::hilo_motor::{self, Orden};
use icopia::config::Config;
use icopia::fuente::{FuenteFalsa, FuenteIcloud};
use icopia::modelo::TipoParte;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn primera_ronda_copia_y_avisa() {
    let datos = jpeg(1000);
    let s = servidor(vec![("/a", Comportamiento::Servir(datos.clone()))]);
    let destino = tempfile::tempdir().unwrap();
    let cfg = Config { destino: Some(destino.path().to_path_buf()), apple_id: Some("yo@x.com".into()), ..Config::default() };
    let a = asset("A1", "IMG_0001", "2026-08-10T19:37:18-04:00", "PrimarySync", vec![parte(TipoParte::Original, &datos, s.url("/a"), "JPG")]);

    let (tx, rx) = mpsc::channel();
    let (tx_ev, rx_ev) = mpsc::channel::<EventoUi>();
    let tx_ev = Mutex::new(tx_ev);
    let h = hilo_motor::lanzar(
        rx,
        Arc::new(AtomicBool::new(false)),
        Box::new(move || cfg.clone()),
        Box::new(move |_| Ok(Box::new(FuenteFalsa { assets: vec![a.clone()], ..Default::default() }) as Box<dyn FuenteIcloud>)),
        Box::new(move |e| {
            let _ = tx_ev.lock().unwrap().send(e);
        }),
    );

    let limite = Instant::now() + Duration::from_secs(20);
    let (mut aviso, mut al_dia) = (false, false);
    while Instant::now() < limite && !(aviso && al_dia) {
        match rx_ev.recv_timeout(Duration::from_secs(1)) {
            Ok(EventoUi::Aviso { texto, .. }) if texto.starts_with("1 archivos") => aviso = true,
            Ok(EventoUi::Estado(e)) if e.fase == Fase::Inactiva && e.verificadas == 1 && e.total == 1 => al_dia = true,
            _ => {}
        }
    }
    assert!(aviso && al_dia, "aviso={aviso} al_dia={al_dia}");
    assert!(destino.path().join(r"2026\2026-08\IMG_0001.JPG").exists());
    tx.send(Orden::Salir).unwrap();
    h.join().unwrap();
}
