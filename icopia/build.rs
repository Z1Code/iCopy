//! Incrusta en los tres .exe el icono de la app y el manifiesto (estilos visuales de
//! Common Controls v6, DPI por monitor v2 y página de códigos UTF-8).
fn main() {
    println!("cargo:rerun-if-changed=recursos/icopia.rc");
    println!("cargo:rerun-if-changed=recursos/icopia.manifest");
    println!("cargo:rerun-if-changed=assets/icopia.ico");
    embed_resource::compile("recursos/icopia.rc", embed_resource::NONE).manifest_required().unwrap();
}
