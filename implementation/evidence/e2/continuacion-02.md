# Continuación 2 · corte 2026-09-08

Se retomó el trabajo de Fable sin reconstruir el producto. V2 sin Git; V1 HEAD c3677ba568cfc7d5947ec4695c42fffef6d79e03 limpio y solo lectura. Lectura completa de los tres prompts y continuidad. El usuario solicitó cerrar este tramo con un traspaso, que ahora sustituye el anexo antiguo en `prompts/00-CONSTRUIR-TRANSCRIPTOR-V2.md`.

## Implementación conservada

Base de Fable: caches/media, presets/capacidades/tests audio/export, ShiftClips.policy y fixtures. Integración/correcciones: cache IO en workers, validación de versiones y tamaños, temporales únicos, cancelación con slots retenidos; dibujo visible Fuente/Secuencia y texturas acotadas; árbol de intervalos por track/revisión; markers/editor/lista/regla y snap; autoscroll con compensación de origen; insert enlazado; cola con revisión congelada y cancelación real; relink Unicode; normalización temporal FFmpeg; controles envueltos; scripts y empaquetado provisional.

## Evidencia y resultados

| Artefacto | Resultado |
|---|---|
| workspace-final.log | 81 tests correctos: desktop14/app6/domain25/media21/v1compat11+4golden |
| build-final.log, clippy-final.log, fmt-final.log | Release construido; Clippy -D warnings y formato correctos |
| latest-regression.json | Rutas nuevas de E1/E2; ambos logs terminan fallos=false |
| regression-20260908-105213/e1-recorrido/verify-export.txt | 12s360frames, cortes/overlay correctos, PSNR42,4–42,9dB |
| regression-20260908-105213/e2-escenario1/verify-{720p,1080p}.txt | Visor↔export34,9–37,9dB, hueco/PiP correctos |
| format-matrix.txt | 13 presets reales inspeccionados, NVENC en este host |
| sync-export-measurement.txt, sync-export-720p.mp4 | Diez pulsos/destellos, corte fuente0,5–10,5; delta0,104ms;300frames |
| av-sync-atempo.txt | 1–4× y seek, desvío máximo25,9ms; no salida física |
| start-time-before.log, media-offset-tests.log | Fallo TS inicio≈5s reproducido, offset doble corregido; audio+250ms conservado |
| caches-*.png, markers-list.png, tests/scripts/e2-caches.log | Cache visible, markers guardados/reabiertos, script correcto; último wait5500 todavía no reejecutado |
| ui-1280-{100,150,200}.png | Zoom real de app, no DPI OS; zoom2 produjo ventana1920×1080 |

Revisión funcional: regresiones de import/edit/save/reopen/export pasan; tests de gestures/cola/markers/insert pasan. Técnica: se corrigieron temporización y cachés; suite pasa, quedan globalRAM/errores de decoder/cierreworkers. Experiencia: capturas muestran controles responsivos; smoke final encontró medio ausente. **E2 continúa ABIERTA**, no se completaron las tres revisiones de aceptación.

## Smoke del paquete: fallo abierto

ZIP `dist/Transcriptor V2 prueba ñ 20260908-105323.zip`, SHA256 `9daa9909d6778cfd118a2aa84dfaaf579ace09539f9ec182f9cbb20cc86caeeb`. Reporte package-build.json.
Prueba real extraída fuera del workspace, ruta TEMP con espacios/ñ, PATH solo Windows/System32. `package-smoke-20260908-065502/checksums.txt`:499 archivos correctos. `smoke.log`: fallos=true (cachés0). `portable.png`: medio ausente; exportó negro/silencio a pesar de ello.

Causas localizadas, no corregidas por corte solicitado:
1. builder escribe media/id.mp4, pero rutas relativas del contrato son respecto al padre de .transcriptor; debe usar Demo.transcriptor/media/id.mp4. También origen relativo en builder debe respetar contrato.
2. ExportJob permite assets ausentes. Validarlos para rango/formato y rechazar antes de staging; prueba negativa real necesaria.

No entregar este ZIP como validado. El anterior archivo truncado quedó con sufijo .failed.zip. Builder ya corrige timestamps ZIP pre1980 y usa publicación desde parcial. Licencias transitivas incompletas declaradas en PACKAGE.json (293 crates inventariados incluyendo dev/opcionales), pendientes E6.

Primera acción: corregir estos dos fallos, reconstruir, ejecutar package_smoke.ps1 y validar píxeles/audio. Después seguir pendientes E2 del STATUS. Traspaso completo, sin anexos duplicados, en el prompt principal.
