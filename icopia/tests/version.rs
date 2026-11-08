#[test]
fn icopia_version_muestra_la_del_cargo_toml() {
    for arg in ["--version", "-V"] {
        let salida = std::process::Command::new(env!("CARGO_BIN_EXE_icopia")).arg(arg).output().unwrap();
        assert!(salida.status.success());
        assert_eq!(String::from_utf8_lossy(&salida.stdout).trim(), format!("iCopy {}", env!("CARGO_PKG_VERSION")));
    }
}
