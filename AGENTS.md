# AGENTS.md — instrucciones para agentes de IA

Guía para agentes de programación (Claude Code, Codex, Copilot, Cursor…) que trabajen en este repo.
El código, los comentarios, los mensajes de la interfaz y los de commit están **en español**: mantenlo así.

## Qué es

iCopy (crate y binarios `icopia`): app de bandeja de Windows que copia todos los originales de iCloud
Fotos a una carpeta local, los verifica y, solo con confirmación explícita, libera espacio en iCloud.
Diseño completo: `docs/superpowers/specs/2026-11-07-icopia-design.md`. Visión general: `README.md`.

## Estructura

```
icopia/                 crate Rust (lib + 3 binarios)
  src/main.rs           CLI `icopia` (destino, login, diagnostico, copiar, estado, verificar, liberar, restaurar)
  src/bin/icopia-bandeja.rs  app de bandeja (menú, panel, avisos)
  src/bin/icopia-login.rs    ventana de inicio de sesión (Apple ID, contraseña, 2FA)
  src/motor.rs          rondas: listar, descargar, verificar, renovar enlaces caducados
  src/descarga.rs       HTTP (reqwest), .part, reanudación por Range
  src/verificar.rs      tamaño + estructura de contenedor + SHA-256
  src/registro.rs       SQLite (WAL) en <destino>\.icopia\registro.db
  src/liberar.rs        plan de borrado, borrado por lotes (ritmo/esperas ante Apple), restaurar
  src/criterio.rs       qué se libera: conservar N días (mín. 30) o años de captura
  src/progreso_liberar.rs  avance del borrado en el registro (meta `liberar_progreso`) para la bandeja
  src/i18n.rs           textos en español e inglés (todas las claves en los dos idiomas)
  src/puente.rs         cliente del puente Python (JSON por líneas, stdin/stdout)
  src/bandeja/          estado, panel (vista + dibujo GDI), asistente de liberar (modelo, dibujo,
                        ventana; el trabajo lo hace hilo_liberar en el hilo del motor), icono, avisos
  examples/             panel_demo, panel_social, asistente_demo, generar_icono (renders sin datos reales)
  tests/                integración con iCloud falso (tests/datos/puente_falso.py + HTTP local)
puente/                 puente Python sobre pyicloud_ipd (icloudpd, versión fija)
  puente_icloud.py, tests/, build.ps1 (PyInstaller → dist/puente-icloud.exe)
docs/social/            imágenes del README y su generador (generar.py)
instalar.ps1            compila e instala en %LOCALAPPDATA%\Programs\icopia
```

## Comandos

```powershell
# Preparar el puente (una vez)
py -3.11 -m venv puente\.venv
puente\.venv\Scripts\pip install -r puente\requirements.txt

cargo build --manifest-path icopia\Cargo.toml
cargo test  --manifest-path icopia\Cargo.toml
cargo clippy --manifest-path icopia\Cargo.toml --all-targets
puente\.venv\Scripts\python -m pytest puente\tests
cargo run --manifest-path icopia\Cargo.toml --example panel_demo -- panel.png 1.5   # ver el panel sin bandeja
```

- Las pruebas no necesitan cuenta de Apple: usan un puente simulado y un servidor HTTP local.
- Para probar la bandeja sin tocar la instalación real, usa una carpeta temporal en `ICOPIA_DIR`
  (sustituye a `%APPDATA%\icopia`) y un puente falso.
- **No ejecutes `instalar.ps1`** salvo que la persona lo pida: mata los procesos instalados y los reemplaza.

## Reglas de seguridad (obligatorias)

1. **Nunca subas al repo** Apple IDs, correos, contraseñas, códigos 2FA, cookies o archivos de sesión,
   `config.json`, `registro.db` (ni `*.db-wal`/`*.db-shm`), logs, `borrados.csv` ni fotos de nadie.
   En pruebas usa datos inventados (`yo@ejemplo.com`).
2. **Nunca ejecutes `icopia liberar`/`icopia restaurar`** ni "Liberar espacio en iCloud…" (ni los comandos `borrar`/`restaurar` del puente)
   contra una cuenta real sin confirmación humana explícita en ese momento. Borra fotos de iCloud.
3. No toques la configuración, la sesión ni la carpeta destino reales de la persona salvo que lo pida.
4. No debilites las garantías: verificación antes de dar por bueno, re-verificación antes de borrar,
   confirmación explícita (`BORRAR` o casilla + mantener pulsado), conservar al menos 30 días, nunca la Fototeca Compartida, nunca borrar del PC.
   Cualquier cambio en `liberar.rs`, `criterio.rs`, `verificar.rs` o el borrado del puente necesita pruebas.
5. Textos nuevos de la interfaz: en `i18n.rs`, en español **y** en inglés (la prueba lo exige). Para ver
   el asistente sin bandeja: `cargo run --example asistente_demo -- salida.png 1.5 oscuro anios en`.

## Dónde va el Apple ID de la persona

**No va en el repo ni en ningún archivo del código.** Se introduce en tiempo de ejecución:

- en la ventana de inicio de sesión (`icopia-login.exe`, o menú *Iniciar sesión en iCloud…*), o
- con `icopia login` en la terminal.

Se guarda en `%APPDATA%\icopia\config.json` (campo `apple_id`, junto a `destino`, `mantener_dias`,
`concurrencia`, `intervalo_horas`). La sesión de iCloud (cookies) queda en `%APPDATA%\icopia\sesion\`.
La contraseña **no se guarda** nunca.

Si estás instalando o configurando iCopy para alguien: **pregúntale su Apple ID** y deja que **escriba
él mismo la contraseña y el código de 6 dígitos** que aparece en su iPhone. No pidas ni manejes la
contraseña ni el código, no los pongas en scripts, variables de entorno ni en el historial de la terminal.
Si su cuenta tiene Protección avanzada de datos, debe activar en el iPhone "Acceder a datos de iCloud en la web".

## Commits

- Mensajes en español, cortos y en imperativo o descriptivos (mira `git log`).
- Usa **tu propia identidad de git**. Si no quieres exponer tu correo, usa el *noreply* de GitHub
  (`<id>+<usuario>@users.noreply.github.com`). Nunca hagas commits con la identidad de otra persona.
- Antes de cada commit, revisa el diff buscando datos personales (correos, rutas con nombres, Apple IDs).
