# Kerf

- Commit: `5fc1db875bdc1a267000fe6d45afa84719610d5b`.
- Evidence paths: `crates/kerf-core/src/project.rs`, other `crates/kerf-core`, queue/task and MCP modules, `frontend/`.
- Relevant design: EDL/timeline in SQLite, revision history and revision diff, staged agent edits, apply/discard, stale detection, task queue, FFmpeg supervision and proposal review.
- Platforms/deps declared: Rust core with Tauri/Svelte frontend; this frontend is not eligible as V2 primary UI.
- License: PolyForm Noncommercial 1.0.0, required notice copyright Orell Bühler. No copy or commercial dependency without written legal clearance.
