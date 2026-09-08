# 09 — Integración Rust + Python

## Worker lifecycle

Rust crea un proceso con entorno mínimo, working directory de staging fuera de la UI y pipes redirigidos. El worker envía `hello(capabilities, versions, schemas)`, heartbeats, `progress`, `checkpoint`, `result` o `error`; nunca carga modelos en el proceso de UI. El supervisor aplica timeout, cancelación cooperativa, SIGTERM/CTRL_BREAK, kill forzado y backoff limitado.

Cada job tiene `job_id`, `step_id`, `input_digest`, `model_id/version`, `device`, `started_at`, `checkpoint_uri` y `output_digest`. Un crash conserva el último checkpoint y permite reanudar solo si la clave de entrada coincide.

## Transporte

NDJSON stdio es la opción inicial: cada línea UTF-8 es un objeto; stdout no admite logs. Named pipes/Unix sockets se reservan para worker persistente o múltiples clientes. gRPC/protobuf solo cuando se necesite streaming bidireccional, evolución de schema y deployment independiente; añade runtime y packaging. Cap’n Proto no aporta suficiente valor en Fase 1.

## CUDA y secretos

Whisper/CTranslate2, MMS/TorchAudio, MediaPipe, ONNX, Transformers, LALM/VLM siguen en Python. Separar procesos evita conflictos CUDA/cuDNN/OpenMP y permite descargar modelos. Scheduler reserva RAM/VRAM antes de lanzar; OOM marca capability degradada y reintenta CPU/modelo menor. API keys viven en secret store/config local protegido, nunca en logs, prompts persistidos o crash reports; redacción por campos y regex antes de persistir.
