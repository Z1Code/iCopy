# Cambios

Todas las versiones de iCopy. Formato: [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/);
numeración: [SemVer](https://semver.org/lang/es/).

## [Sin publicar]

### Añadido
- **iCopy para Mac (beta, Apple Silicon):** app de la barra de menús con la misma copia verificada que
  en Windows, inicio de sesión propio, avisos, «Abrir al iniciar sesión» y actualizaciones automáticas.
  Se instala con un `.dmg` firmado y notarizado por Apple. «Liberar espacio en iCloud» llega a Mac en
  la próxima versión.

### Seguridad
- Las actualizaciones llevan una firma v2 que cubre también el nombre del paquete (versión y sistema):
  ya no se puede servir un instalador antiguo auténtico como si fuera nuevo. Se rechazan paquetes de
  más de 300 MB.

### Corregido
- Si el instalador de una actualización falla, iCopy sigue abierta, reanuda la copia y avisa (antes se
  cerraba hasta el siguiente inicio de sesión). El instalador deja un log para diagnosticar.
- Rutas de las copias preparadas para macOS (en el registro siguen igual; los registros existentes valen).

## [0.2.0] - 2026-11-08

### Añadido
- Instalador para Windows (`iCopy-Setup-0.2.0.exe`): sin permisos de administrador, menú Inicio,
  arranque con Windows y desinstalador.
- Actualizaciones automáticas desde GitHub Releases, verificadas con firma ed25519; nunca durante
  un borrado en iCloud.
- Versión visible en el menú del icono, en `icopia --version` y en las propiedades del `.exe`.

### Corregido
- Ya no pide volver a iniciar sesión por un error pasajero de Apple (421) justo tras el login.
- La sesión ya no se pierde si la app se cierra a mitad de guardarla (pausa, apagado, corte de luz).

## [0.1.0] - 2026-11-07

Primera versión: copia verificada de iCloud Fotos, panel en la bandeja y asistente para liberar espacio.

[Sin publicar]: https://github.com/Z1Code/iCopy/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Z1Code/iCopy/releases/tag/v0.2.0
