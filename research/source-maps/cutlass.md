# Cutlass

- Commit: `22437e2837340c7c57d62e438117f9a0fb4096d2`.
- Evidence paths: `crates/cutlass-models/src/{timeline.rs,media.rs,project/}`, `crates/cutlass-commands/src/{lib.rs,command.rs}`, `crates/cutlass-engine/src/action/`, `crates/cutlass-render/src/{scene.rs,render/,export.rs,media_cache.rs}`, `crates/cutlass-compositor/`, `crates/cutlass-ai/src/{wire/,validate/}`, `apps/cutlass-desktop/src/` and `ui/panels/timeline/`.
- Relevant design: shared UI/AI commands, dry-run/staging, inverse and compound undo, transform/keyframe model, GPU scene/render, AI validation DTOs, Slint desktop UI.
- Platforms/deps declared: Windows x64 native media backend; Linux UI only in README and media backend not implemented; macOS AVFoundation.
- License: MIT and Apache-2.0 files; audit bundled assets and dependencies before reuse.
