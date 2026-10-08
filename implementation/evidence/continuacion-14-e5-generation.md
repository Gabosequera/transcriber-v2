# Generación posterior a etapas — integración en curso

Este incremento no acredita ASR real: PyAV continúa bloqueado. Las pruebas de modelos usan voz y texto inventados, con esa procedencia conservada en los recibos. La aplicación humana349df permanece abierta; no incluye estos cambios posteriores.

## Contrato implementado

`crates/pipeline/src/generation.rs` consume el parent ASR final y mapas tipados de alineación, arousal y risas por audio_index. Cada etapa debe pertenecer al mismo parent exacto; arousal debe citar la alineación incluida. Se revalidan recibos, payload_digest, fuente/modelos/código y artefactos antes/después. Los jobs anteriores no se reescriben. El finalizador stdlib tiene Python3.12.13 explícito y código/hash de worker/ensamblador/derivación fijados.

`finalize_worker.py` limita NDJSON a1MiB, JSON de entrada a128MiB, guarda checkpoints por todas las entradas y responde a cancelación. Los artefactos solo salen del job propio. `finalize.py` combina palabras/timestamps/señales y recalcula intensidad/heurísticas/conversación con PCM del reloj fuente. Conserva el master ASR completo y extensiones; las risas no suman por segunda vez offsets a PCM ya normalizado. MMS sobre chunks preexistentes se rechaza hasta implementar rechunk explícito.

La incorporación comparte `ArtifactPreparation`/prepare/commit con ASR: fuente y base verificadas, actor External, idempotencia, master protegido y decisiones humanas. No se aplica automáticamente ni se rebindea contra otra revisión. La validación Rust también contrasta texto/IDs/probabilidades/extensiones/tiempos y eventos contra sus etapas, incluso si se recalcula el SHA del master modificado.

## Evidencia hasta este punto

- `e5-generation-command-boundary.log`:12 contratos anteriores PASS tras factorizar preparación.
- `e5-generation-contracts.log`:14 PASS, incluyendo falsificación de palabras/extensiones/intervenciones/original.
- `e5-generation-record-contracts.log`:15 PASS; payload modificado rechazado antes del claim/spawn, sin registrar intento ni cambiar Queued.
- `e5-finalize-boundary.log`:8 pruebas Python de archivos/hash/cancelación/límites/publicación PASS. Cambios posteriores del cache/runtime pendientes de repetición integrada.
- `e5-generation-check-initial.log`, `e5-generation-example-check.log`, `e5-generation-build.log`:compilación correcta del host.
- Clippy inicial/segundo conservaron errores durante construcción del módulo/ejemplo arousal, corregidos después por su responsable. No son PASS integrados.

## Primera integración ejecutada

`e5-generation-host-01.log` y [resultado](e5-generation-host-result.json):PASS17.258s, job `job-cec557ef8f92`, parent ASR inventado `job-57fe5b97cf1c`, alineación/arousal/risas reales de la misma ascendencia. Se preparó sin mutar, incorporó por comando común, guardó/reabrió y deshizo/rehizo el proyecto sintético. Alterar una palabra y recalcular el SHA del recibo se rechazó; se restauraron los bytes propios y revalidó el resultado original. No se ejecutó ASR ni GUI.

La revisión previa descubrió y corrigió dos defectos: confundir fingerprint.inventario_sha256 con el SHA completo de fuente, y permitir que SourceFileLoader ejecutase un `.pyc` no verificado aunque el `.py` tuviese el SHA esperado. El finalizador ejecuta los bytes de fuente verificados mediante compile/exec e inyecta el módulo de derivación igualmente verificado. NDJSON completo sin salto final se rechaza en EOF. `e5-finalize-bytecode-boundary.log`:11 PASS; `e2/continuacion-14/finalize-tests-verified-import.log`:15 pruebas puras PASS. Repros anteriores conservados por el revisor.

`e5-generation-workspace-clippy.log` y `e5-generation-workspace-fmt.log`:PASS sobre ese incremento. La revisión posterior encontró PCM válido más corto que la duración del vídeo; se ajusta el ensamblador para aceptarlo sin inventar padding y exigir palabras dentro del audio. Repetición de finalización pendiente de ese cambio, sin repetir modelos.

Pendiente: selección/DAG/GUI, pruebas adicionales de resume/cancel de generación y generación editorial LLM. Un parent inventado y el éxito de etapas por separado o juntas no cierran E5.

## Segunda integración y verificación exacta

`e5-generation-host-02.log` y `e5-generation-host-02-result.json`: PASS41.658s, job `job-f39dc0617d18`, master SHA256 `c85281412efc9cc209f5a60ff94e00db123790956cc63374d2b45cca1b0184e5`. Conserva las mismas etapas reales y parent ASR inventado; no repite inferencia. Preparación/commit/Save/reopen/undo/redo correctos. Se rechazaron cinco alteraciones con SHA recalculado: texto, intensidad normalizada, pausas, IDs de conversación y módulos completados. Cada artefacto propio se restauró y el original se revalidó.

La revisión añadió `verify` de solo lectura: rederiva el JSON completo desde las entradas verificadas, sin confiar en checkpoints ni publicar archivos, y contrasta tipos y valores exactos. Conserva extensiones de objetos sin reasociar notas a arrays regenerados; el original completo sigue archivado. Acepta audio más corto que el vídeo con palabras dentro del PCM. Evidencia de la corrección: `continuacion-14-e5-review.md`, 19 pruebas puras,17 de protocolo/archivos y3 contratos Rust específicos PASS. La duración del host02 incluye incorporación, persistencia, ataques y repetidas validaciones, no solo ensamblado. Integración GUI aún sin build nativo nuevo.
