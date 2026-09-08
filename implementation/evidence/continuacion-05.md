# Continuación 5 — migración de equipo e implementación E3/E4

Encargo vigente del usuario: completar implementación hasta E4 y parar antes de E5. Testing real y revisión general quedan para otra iteración; compilación y pruebas unitarias dirigidas son controles de implementación, no aceptación física. Preparar commit, push, prerelease y traspaso al interrumpir por contexto. No marcar requisitos pendientes como verificados.

Host actual: checkout `C:/Users/gabri/Todo/transcriber-v2`, base Git `42a5226`, árbol inicialmente limpio. V1 hermano `../transcriber`, solo lectura. Las rutas `G:/TODO` de evidence son históricas y se conservan. Guiones ejecutables pasan a plantillas `${WORKSPACE}` / `${OUTPUT}` materializadas en destinos nuevos.

Decisiones del incremento:

- Resolver FFmpeg de desarrollo mediante variable explícita, distribución local o PATH. No cambiar versiones del lock para adaptar una ruta.
- Activar el entorno MSVC instalado mediante el descubridor de Visual Studio: Rust no encuentra automáticamente `link.exe` de VS 18 en este host.
- Endurecer sesión antes del transporte externo: protocolo/digest, identidad del reintento y continuidad de undo/redo. Rechazar una clave reutilizada con otro contenido.
- Terminar personalización de atajos desde GUI con validación de conflictos y escritura atómica; aplicar en memoria solo después de persistir.

## Cambios completados en este incremento

Atajos editables/capturables, desasignación/defaults y conflictos; undo/redo multistep; validación protocolo/proyecto/digest/reintento; diff de propiedades/secuencias/capas borradas; CAS y lock de SO al guardar; rutas estables al Guardar como; autosave recuperable explícitamente con undo. Scripts portables y wrapper MSVC, instalación de toolchain/SDK/CLI. No implementación MCP ni cierre E3/E4.

## Controles de implementación

- Rust 1.98.1 y dependencias del lock preservadas; versión propia elevada a 2.0.0-alpha.2.
- `./scripts/cargo.ps1 clippy --workspace --all-targets --locked '--' -D warnings`: OK, incluye compilación de todos los targets y tests. Log `clippy-continuacion-05.log`.
- `./scripts/cargo.ps1 test -p tv2-application --locked`: **13 pasan**, sin medios, GUI ni servicios. Log `application-continuacion-05.log`.
- `./scripts/cargo.ps1 fmt --all --check`: OK.
- AST Python y expansión de todos los JSON de tests con destino temporal Unicode/espacios: OK; `paths-continuacion-05.txt`. Ningún generador de medios ejecutado.
- `./scripts/cargo.ps1 test -p transcriptor keymap::tests --locked`: compilación terminada (7m17s), **tests no ejecutados**. Windows Control de aplicaciones bloqueó el binario con os error 4551; ver `keymap-continuacion-05.log`. No se desactivó ni eludió la política del sistema.

Primeros intentos: faltaba link.exe en entorno, después Windows SDK/kernel32.lib; se instalaron SDK y wrapper. El primer check de código encontró un String movido antes de reutilizarlo en UI, corregido mediante préstamo. Clippy detectó inicialización/borrow y se corrigieron; el último log Clippy es el resultado aceptado. No atribuir éxito al log de check fallido conservado.

## Límites y siguiente trabajo

La validación real, suite multimedia, revisión general y paquete binario se aplazan por petición del usuario. No hay nuevo ejecutable release validado. Nueva release de **fuentes**; source archives GitHub. E3 continúa parcial y E4 pendiente; STATUS enumera los siguientes incrementos sin presentarlos como hechos.

Persistencia aún tiene ventana project/journal, posible append parcial y sin watcher/reconcile/migraciones/multidocumento. Locks coordinan instancias V2, no escritores externos que ignoren el lock. Recuperación no hace todavía validación integral de invariantes ni autosave para un proyecto sin carpeta. Idempotencia está acotada pero no persiste entre aperturas. Atajos necesitan interacción física/IME. Los medios generados de nuevo no mantienen necesariamente fingerprint de goldens históricos; regenerar con V1 aislado explícitamente.

Se conserva la evidencia del checkpoint anterior sin rebautizar rutas. No se modificó el checkout V1. Corte preventivo por contexto; objetivo global hasta E4 no completado.

## Publicación y parada

Código/contexto: commit `31d18437788fcd1da0155b478b538d1753d1b93d` subido a main. Release `v2.0.0-alpha.2` publicada, no draft, prerelease, tag correcto y sin assets binarios; registro en `publication-alpha2.json`. El commit siguiente solo registra el resultado remoto. V1 observado limpio al finalizar. Parada de esta iteración por contexto; continuar implementación E3/E4 desde STATUS, sin reiniciar E0/E1 ni iniciar E5.
