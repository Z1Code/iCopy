#!/bin/bash
# Empaqueta iCopy para macOS (Apple Silicon) en dist/:
#   iCopy.app, iCopy-<versión>-macos-arm64.zip (para el actualizador) y iCopy-<versión>-macos-arm64.dmg.
#
# Uso: ./empaquetar-mac.sh [--firmar] [--notarizar]
#   sin opciones   firma ad-hoc (desarrollo; macOS avisará al abrirla descargada de internet).
#   --firmar       firma con «Developer ID Application». La identidad sale de:
#                    ICOPY_MAC_P12 + ICOPY_MAC_P12_PASS  → llavero temporal (CI, o local por SSH), o
#                    ICOPY_MAC_IDENTIDAD                 → una identidad ya disponible en los llaveros.
#   --notarizar    (implica --firmar) notariza y grapa con la API de App Store Connect:
#                    ICOPY_NOTARIO_P8 (ruta del .p8), ICOPY_NOTARIO_KEY_ID, ICOPY_NOTARIO_ISSUER.
#
# El llavero temporal se añade a la lista de búsqueda del usuario solo mientras dura el script (lo
# necesita PyInstaller); la lista original se restaura SIEMPRE al salir y se comprueba.
set -euo pipefail
raiz="$(cd "$(dirname "$0")" && pwd)"
cd "$raiz"

firmar=0
notarizar=0
for a in "$@"; do
    case "$a" in
        --firmar) firmar=1 ;;
        --notarizar) firmar=1; notarizar=1 ;;
        *) echo "Opción desconocida: $a" >&2; exit 2 ;;
    esac
done

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' icopia/Cargo.toml | head -1)"
dist="$raiz/dist"
app="$dist/iCopy.app"
base="iCopy-$version-macos-arm64"
tmp="$(mktemp -d)"
llavero=""
lista_original=()

limpiar() {
    if [ -n "$llavero" ]; then
        if [ ${#lista_original[@]} -gt 0 ]; then
            security list-keychains -d user -s "${lista_original[@]}"
        fi
        security delete-keychain "$llavero" 2>/dev/null || true
    fi
    rm -rf "$tmp"
}
trap limpiar EXIT

# --- Identidad de firma ---
if [ "$firmar" = 1 ] && [ -n "${ICOPY_MAC_P12:-}" ]; then
    llavero="$tmp/icopy.keychain-db"
    clave_llavero="$(openssl rand -hex 16)"
    security create-keychain -p "$clave_llavero" "$llavero"
    security set-keychain-settings -lut 3600 "$llavero"
    security unlock-keychain -p "$clave_llavero" "$llavero"
    security import "$ICOPY_MAC_P12" -k "$llavero" -P "${ICOPY_MAC_P12_PASS:?falta ICOPY_MAC_P12_PASS}" -T /usr/bin/codesign >/dev/null
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$clave_llavero" "$llavero" >/dev/null
    while IFS= read -r linea; do
        linea="${linea#*\"}"
        lista_original+=("${linea%\"}")
    done < <(security list-keychains -d user)
    security list-keychains -d user -s "$llavero" "${lista_original[@]}"
    ICOPY_MAC_IDENTIDAD="$(security find-identity -v -p codesigning "$llavero" | awk '/Developer ID Application/ {print $2; exit}')"
fi
if [ "$firmar" = 1 ]; then
    : "${ICOPY_MAC_IDENTIDAD:?No hay identidad Developer ID Application (ICOPY_MAC_P12 o ICOPY_MAC_IDENTIDAD)}"
    export ICOPY_MAC_IDENTIDAD
    identidad="$ICOPY_MAC_IDENTIDAD"
    sello="--timestamp"
else
    unset ICOPY_MAC_IDENTIDAD
    identidad="-"
    sello="--timestamp=none"
fi

# --- Compilar ---
echo "== Puente"
puente/build-mac.sh
echo "== Rust"
cargo build --release --manifest-path icopia/Cargo.toml

# --- Icono (.icns): contenido de 824 px centrado en 1024, como los iconos de macOS ---
echo "== Icono"
cargo run -q --release --manifest-path icopia/Cargo.toml --example generar_icono -- --png 824 "$tmp/base.png"
sips -p 1024 1024 "$tmp/base.png" --out "$tmp/1024.png" >/dev/null
mkdir -p "$tmp/icopia.iconset"
for n in 16 32 128 256 512; do
    sips -z "$n" "$n" "$tmp/1024.png" --out "$tmp/icopia.iconset/icon_${n}x${n}.png" >/dev/null
    sips -z $((n * 2)) $((n * 2)) "$tmp/1024.png" --out "$tmp/icopia.iconset/icon_${n}x${n}@2x.png" >/dev/null
done
iconutil -c icns "$tmp/icopia.iconset" -o "$tmp/icopia.icns"

# --- Bundle ---
echo "== iCopy.app $version"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in icopia-bandeja icopia-login icopia; do
    cp "icopia/target/release/$b" "$app/Contents/MacOS/"
done
cp puente/dist/puente-icloud "$app/Contents/MacOS/"
cp "$tmp/icopia.icns" "$app/Contents/Resources/"
sed "s/__VERSION__/$version/g" instalador/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

# --- Firma, de dentro afuera (el puente ya va firmado por build-mac.sh, con sus permisos) ---
for b in icopia icopia-login icopia-bandeja; do
    codesign --force --options runtime "$sello" --sign "$identidad" "$app/Contents/MacOS/$b"
done
codesign --force --options runtime "$sello" --sign "$identidad" "$app"
codesign --verify --deep --strict --verbose=1 "$app"

notarizar_archivo() {
    echo "== Notarizando $(basename "$1")"
    local salida
    salida="$(xcrun notarytool submit "$1" --key "${ICOPY_NOTARIO_P8:?}" --key-id "${ICOPY_NOTARIO_KEY_ID:?}" \
        --issuer "${ICOPY_NOTARIO_ISSUER:?}" --wait --timeout 2h 2>&1)" || true
    echo "$salida" | grep -E "id:|status:" | head -3
    if ! echo "$salida" | grep -q "status: Accepted"; then
        local id
        id="$(echo "$salida" | awk '/ id:/ {print $2; exit}')"
        [ -n "$id" ] && xcrun notarytool log "$id" --key "$ICOPY_NOTARIO_P8" --key-id "$ICOPY_NOTARIO_KEY_ID" --issuer "$ICOPY_NOTARIO_ISSUER" >&2 || true
        echo "Notarización rechazada o sin terminar" >&2
        exit 1
    fi
}

if [ "$notarizar" = 1 ]; then
    ditto -c -k --keepParent "$app" "$tmp/notarizar.zip"
    notarizar_archivo "$tmp/notarizar.zip"
    xcrun stapler staple "$app"
    spctl --assess --type execute -vv "$app"
fi

# --- Zip para el actualizador (de la app ya grapada) ---
rm -f "$dist/$base.zip" "$dist/$base.dmg"
ditto -c -k --keepParent "$app" "$dist/$base.zip"

# --- DMG para instalar a mano: arrastrar a Aplicaciones ---
mkdir -p "$tmp/dmg"
cp -R "$app" "$tmp/dmg/"
ln -s /Applications "$tmp/dmg/Aplicaciones"
hdiutil create -volname "iCopy" -srcfolder "$tmp/dmg" -ov -format UDZO "$dist/$base.dmg" >/dev/null
if [ "$firmar" = 1 ]; then
    codesign --force "$sello" --sign "$identidad" "$dist/$base.dmg"
fi
if [ "$notarizar" = 1 ]; then
    notarizar_archivo "$dist/$base.dmg"
    xcrun stapler staple "$dist/$base.dmg"
fi

echo "Listo: $app"
echo "       $dist/$base.zip"
echo "       $dist/$base.dmg"
