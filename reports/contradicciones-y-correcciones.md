# Contradicciones y correcciones

## 1. Gausian

El análisis anterior repetía la descripción del README como MPL-2.0. La lectura directa del commit muestra `LICENSE` Apache-2.0 mientras el README declara core MPL-2.0 y pro features comerciales. Corrección: no copiar archivos, no asumir file-level scope y solicitar aclaración al titular.

## 2. Suite de tests V1

“129 pruebas pasando” era un dato histórico de una revisión previa, no medido en esta auditoría. Corrección: `D-04` queda sin evidencia y se exige reproducibilidad en entorno aislado.

## 3. Tamaño y rendimiento

El V1 ocupa ~29,9 GB físicamente; esto confirma que el peso dominante es runtime/modelos/caches, pero no prueba que Rust reduzca tamaño ni que la UI sea cuello de botella. Corrección: medir binario, runtime, modelos, startup, IPC, inferencia y export por separado.

## 4. “GUI nativa”

egui, Slint, GTK y GPUI pueden crear ventanas nativas, pero “sostener el editor completo” no está demostrado por README. Corrección: egui permanece provisional y condicionado a 14 prototipos; GTK gana en accesibilidad documentada, Slint en backends/licencia, GPUI en ergonomía pero con riesgo de portabilidad.

## 5. GES

GES sí ofrece Timeline/Tracks/Layers/Clips/Pipeline, pero no debe convertirse en modelo de verdad: commit y reglas de solape limitan el dominio. Corrección: adaptador owner-thread con errores explícitos.

## 6. NDJSON

NDJSON es válido para control y metadatos, no para video/tensores grandes. Corrección: payloads grandes van por archivos de trabajo o shared memory con digest; el canal conserva referencias y progreso.
