# Continuación 10 — candidatos, inversa, reconciliación, jobs e historial

Base `6997455`, main limpia; V1 `bb7012c`, lectura de marcas.py/app_paths.py/editorial_projects.py/editorial_pipeline.py/editorial_topics.py/editorial_layers.py/editorial_montaje.py y contratos. No AGENTS.md aplicable encontrado en ancestros ni checkout. No ejecución/import de Python V1 ni escrituras sobre V1/datos personales.

**Checkpoint de implementación parcial, no finalización del encargo. E2 abierta/E3 parcial/E4 pendiente/E5 no iniciada.** Alcance exacto en STATUS, matriz y decisiones D-0047–D-0051. La aceptación física/multimedia/revisión general sigue aplazada.

## Controles finales alpha.7

- `./scripts/cargo.ps1 test -p tv2-application --lib --locked '--' --nocapture`: **45 tests ejecutados/pasan**, compilación 39.01 s, tests 0.52 s. `unit-continuacion-10.log`. Incluye tests de jobs/lock/huérfano, conflictos/elección/protección/tombstones, codec/legacy/digests y los controles existentes de store/history/recuperación.
- `history/2`: test sintético de 20 entradas con 190000 caracteres de evidencia constante: **199436 bytes frente a 7622632** del formato anterior. No son mediciones de RAM/latencia/release ni corpus real. Arrays que cambian longitud todavía se reemplazan enteros y hay clones en memoria.
- Clippy `--workspace --all-targets --locked '--' -D warnings`: exit 0, 9.93 s; `clippy-continuacion-10.log`. Compila también los tests bloqueados; no los ejecuta.
- Check `--workspace --locked`: exit 0, 7.10 s. Fmt `--all --check`: exit 0, sin salida. Build release `--release --locked -p transcriptor`: exit 0, **2m40s**. Logs `check-continuacion-10.log`, `fmt-continuacion-10.log`, `build-continuacion-10.log`; metadatos de tamaño/hash/tiempo total del wrapper en `build-alpha7.json`. No se lanzó el ejecutable.

## Bloqueo observado (no reintentado)

El primer control de esta sesión, todavía con nombre de paquete alpha.6, fue `./scripts/cargo.ps1 test -p tv2-application -p tv2-v1compat --lib --locked`. Ejecutó y aprobó 40 tests application. Después Windows impidió ejecutar `target/debug/deps/tv2_v1compat-166b66073370ca40.exe` con:

> could not execute process … (never executed)
> Una directiva de Control de aplicaciones bloqueó este archivo. (os error 4551)

Esto es un extracto de la salida de la sesión, no un nuevo intento ni un log reproducido. A partir de ahí solo se ejecutaron tests application. V1compat, dominio y desktop están **compilados, no ejecutados**. No se cambiaron rutas de binarios, políticas de Windows ni runtime para eludirlo.

## Comportamiento preparado y límites

- Autor: `author_candidates` descubre por identidad; pruebas sintéticas escritas para empate/revisión superior/digest cambiado/error/quarentena/tombstones/path traversal. Selector GUI y preparación worker compilan. Import automático conserva protección External; adopción explícita Human no puede resucitar borrados. No se inspeccionaron stores personales durante el desarrollo.
- Montaje: nuevo test de inversión recorta/repite/reordena/elimina piezas con audio enlazado y comprueba mapping esperado, IDs de archivo, unknowns, revision/next_id y reimportación. También comprueba rechazo de audio independiente. Prueba escrita/compilada, **no ejecutada** por el bloqueo anterior. El constructor verifica su resultado con flatten durante una exportación real futura, sin sustituir esa aceptación.
- Reconciliación: tests independientes por campo, todos los conflictos y claves JSON escapadas; sin opción para cada conflicto no aplica. Revalidación, lock/commit preparado y undo mantienen las mismas puertas del núcleo. Watcher de directorio V2 compilado, sin ensayo real. No hay watcher/reconcile de cada contrato V1.
- Export: registro genérico tipado y frontend conectado a encolar/lanzar/cancelar/reabrir/reanudar. Snapshots congelados, fingerprint al reanudar y recibos con intentos. Los tests del registro simulan pérdida del lease con drop; no se mató ningún proceso. No se ejecutó FFmpeg ni recuperación multimedia real. Un destino publicado sin recibo final requiere inspección; se conserva y el retry no lo sobreescribe.
- Historial: codec /2 se usa en history.json, autosave y commit intent; /1 sigue legible. Se conservan pila completa, eventos y recibos existentes. El test de recuperación de todas las fronteras y reapertura pasa con el nuevo codec. No elimina 200 entradas, 64/128 MiB, clones de proyectos/proyecciones ni el coste de calcular diffs. Archivado general de journal/recibos de sesión y migraciones de otros contratos pendientes.

## Próximo trabajo real

Completar derivación/mapping/manifests/requests/proposals/passes y export/import de carpeta V1 integral, exclusivamente en destinos propios V2. La lectura inicial de editorial_projects.py y productores de requests/manifests está hecha; no hay implementación nueva de esos contratos en este checkpoint. Continuar después con watcher V1, inventario dinámico completo, archivado/migraciones/eficiencia y E4 (MCP real, capacidades, permisos, contexto/paginación, propuestas y cliente conectado). El bloqueo de tests V1 no impide escribir esas implementaciones y no debe usarse como excusa para aplazarlas a E4.

Fallos intermedios corregidos antes de los controles finales: inferencia de Option<usize> en candidatos, préstamo parcial de job, avisos Clippy collapsible_if. No se ocultó el bloqueo Windows ni se sumaron tests compilados a los ejecutados.
