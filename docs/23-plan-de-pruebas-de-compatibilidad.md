# 23 — Plan de pruebas de compatibilidad

1. Congelar fixtures reales V1 sin medios sensibles: master, trims, layers, topics, chunks, montage, manifests, config, keymap y exports.
2. Importar en Rust y comparar JSON normalizado, IDs, rangos, fingerprints y digests donde la semántica lo permita.
3. Exportar desde Rust y reabrir en V1 en modo solo lectura; verificar que no se pierden items humanos, tombstones, offsets o derivation.
4. Editar JSON externamente entre load/apply; exigir `external_changed`, diff y decisión explícita.
5. Interrumpir cada escritura/proceso: archivo parcial, worker kill, disk full, FFmpeg kill; exigir recuperación sin corrupción.
6. Probar VFR/B-frames, OBS offsets, multipista, audio/video gaps y 3h VOD con frame hashes/drift.
7. Probar AI responses viejas, pass incorrecto, digest incorrecto, ids repetidos, JSON malformado y reintento de idempotency key.
8. Verificar Windows/Linux, paths Unicode/espacios, 100/150/200% DPI, CPU-only y GPU NVIDIA.

Gate: cero pérdida de IDs/rangos humanos, ningún original modificado, diff explicable, import/export determinista y todos los fallos dejan código/error estable.
