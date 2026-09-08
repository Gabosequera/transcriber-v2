# Gausian Native Editor

- Commit: `2e173a0d9ad60ad662a7b858e2141a8540567d70`
- Evidence paths: `crates/timeline/src/{graph.rs,commands.rs}`, `crates/project/src/lib.rs`, `crates/project/migrations/`, `crates/renderer/src/{lib.rs,cpu.rs,shaders/}`, `crates/media-io/src/lib.rs`, `crates/native-decoder/src/gstreamer_backend.rs`, `crates/exporters/`.
- Relevant design: typed frame/range graph, inverse command history, SQLite asset/job/proxy/transcript tables, WGPU YUV shaders and CPU fallback, GStreamer decoder selection/ring/preroll, waveform helper, FCPXML/EDL/JSON exporters.
- Platforms/deps declared: Rust stable, egui/wgpu, GStreamer, FFmpeg/ffprobe; README names Windows/Linux/macOS and platform accelerators.
- Maturity: active small editor; some CLI export paths explicitly say demo mode and plugin signature/WASM host code has TODOs.
- License: repository `LICENSE` is Apache-2.0; README says core MPL-2.0/pro commercial. Conflict unresolved; study only.
