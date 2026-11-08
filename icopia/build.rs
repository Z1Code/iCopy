//! Incrusta en los tres .exe el icono, el manifiesto (estilos visuales v6, DPI por monitor v2,
//! UTF-8) y la versión del Cargo.toml (Propiedades → Detalles).
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=recursos/icopia.manifest");
    println!("cargo:rerun-if-changed=assets/icopia.ico");
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ruta = |p: &str| dir.join(p).display().to_string().replace('\\', "\\\\");
    let v = |n: &str| std::env::var(format!("CARGO_PKG_VERSION_{n}")).unwrap();
    let (ma, mi, pa) = (v("MAJOR"), v("MINOR"), v("PATCH"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icono}"
1 24 "{manifiesto}"
1 VERSIONINFO
FILEVERSION {ma},{mi},{pa},0
PRODUCTVERSION {ma},{mi},{pa},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Z1Code"
      VALUE "FileDescription", "iCopy"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "iCopy"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "Licencia en LICENSE"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icono = ruta("assets/icopia.ico"),
        manifiesto = ruta("recursos/icopia.manifest"),
    );
    let salida = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icopia.rc");
    std::fs::write(&salida, rc).unwrap();
    embed_resource::compile(&salida, embed_resource::NONE).manifest_required().unwrap();
}
