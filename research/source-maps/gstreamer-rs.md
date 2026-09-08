# GStreamer Rust bindings / GES

- Commit: `b13e8e067896cf28959ac00aa63056f783c2a160c`.
- Evidence paths: `gstreamer-editing-services/`, `gstreamer/`, `gstreamer-video/`, `gstreamer-play/`, examples `examples/src/bin/ges.rs`.
- Relevant design: Rust bindings, GES Timeline/Track/Layer/Clip/Asset/Pipeline APIs, examples, GLib/GStreamer message bus and pipeline integration.
- Upstream fact: current project structure places GES in the GStreamer monorepo; the official docs say GES is cross-platform including Windows and LGPL, and bindings warn the API is not thread-safe.
- Decision: dependency, pinned runtime/plugin matrix, one owner thread for GES.
