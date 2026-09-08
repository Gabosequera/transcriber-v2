# OpenCut

- Commit: `fee06cc8765f4205acc7f288e7399b9dfe87672c`.
- Evidence paths: `rust/src/editor/{timeline_document.rs,timeline_clip.rs,timeline_interactions.rs,editing.rs,preview_timeline.rs,clip_render_plan.rs,export_gstreamer.rs}` and `README.md`.
- Relevant design: folder-based JSON timelines, source media in place, multitrack clip model, blade/trim/move, selection rectangle, snapping guides, multiresolution waveforms, undo/redo snapshots, GStreamer preview/GES export.
- Platforms/deps declared: GPUI, GStreamer/GES plus base/good/bad/ugly/libav, FFmpeg dev libs. README calls it experimental and documents current editor behavior; no Windows validation performed here.
- License: no permissive license was relied upon; no copy permitted until explicit terms are verified.
