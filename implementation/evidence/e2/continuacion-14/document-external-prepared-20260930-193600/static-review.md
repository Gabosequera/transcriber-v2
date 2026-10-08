# Revisión estática antes de aceptación nativa

2026-09-30. No se ejecutó ningún editor, modelo, suite Rust ni guion GUI. La ventana humana PID6868 y su binario349df permanecen intactos. Esta revisión no sustituye el recorrido nativo del build integrado nuevo.

El guion `tests/scripts/accept_e3_document_external_native.py` coincide con los hooks actuales `ExportV1Folder`, `WaitExternal`, `ApplyExternal`, `AwaitSignal`, `Dump`, `Save`, `Open`, `NewProject`, `ImportV1`, `Undo`, `Redo` y `Quit`. Serde usa los nombres snake_case del JSON preparado. `human_review` y `choices` omitidos significan false y mapa vacío; la fixture propone exactamente un cambio no conflictivo de `/name`. Las esperas reciben receipts de los polls comunes, no invocan scans ni commits artificiales.

La exportación crea una raíz nueva exacta y `documents::create` rechaza la segunda publicación a la misma raíz con IO. El harness conserva/compara el manifiesto completo entre ambas. La importación de carpeta lee `layers/*.json`, autor, trims y montaje; `views/layers.json` es snapshot auxiliar y no se vuelve a importar como una segunda copia de las capas. Los cinco grupos de proyección proceden del mismo master original. Las rutas de logs `*-script.log` coinciden con `ScriptRunner::load` mediante `with_extension("log")`. La fase probe obtiene la identidad del medio por código nativo y solo modifica los documentos sintéticos copiados.

Fragilidad condicional encontrada: `script_tick` espera imports/editorial/persistence/export/apply pero omite `autosave_job`. `save_project_to` y `open_project_path` rechazan una escritura autosave activa. El autosave puede empezar tras60s si el proyecto está dirty; el harness permite120s por fase y señales/exports de30s. Una fase lenta podría coincidir con autosave y hacer fallar Save/Open sin esperar. No se ha demostrado en ejecución ni se cambió la fuente durante esta revisión. Para robustecer el recorrido, el runner debería esperar el autosave real antes de despachar operaciones dependientes, manteniendo los guards del guardado y sin desactivar autosave. También debe quedar limitado por el deadline del harness.

El principal autorizó después el arreglo mínimo en `scripting.rs`: se agregó `self.autosave_job.is_some()` al busy guard común. No hay deadlock: `eframe::App::ui` llama `script_tick(ctx)` y a continuación `autosave()`; retornar del runner solo retorna `script_tick`. `autosave()` recoge el receiver y limpia el job antes de evaluar dirty o60s, incluso cuando el proyecto ya está limpio. No se desactiva ni simula autosave. Solo fuente corregida; no Cargo simultáneo ni aceptación nativa atribuida.

Comando preparado (solo tras autorizar un build integrado nuevo; sustituir ambas variables por ruta y SHA reales):

```powershell
.local/e5-editorial-venv/Scripts/python.exe tests/scripts/accept_e3_document_external_native.py --run implementation/evidence/e2/continuacion-14/document-external-prepared-20260930-193600 --exe NEW_ABSOLUTE_EXE --sha256 NEW_HASH
```

El harness rechaza el antiguo SHA349df y una fixture ya ejecutada, no reanuda ni sustituye resultados fallidos. Cada fase tiene configuración/cache/log propios, JobObject kill-on-close para su PID exacto,120s y presupuesto2GiB. No hay reproducción física, inferencia, V1 ejecutado, consentimiento humano simulado ni cierre de GUI ajena. Conflictos reales, diff stale y watcher de documento V1 vinculado siguen fuera de esta fixture. Aceptación nativa pendiente.
