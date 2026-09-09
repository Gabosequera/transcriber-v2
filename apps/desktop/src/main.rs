//! Transcriptor V2 — ejecutable de escritorio (eframe/egui + wgpu).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod author_ui;
mod console;
mod control_ui;
mod conversation_ui;
mod durable_exports;
mod editorial_jobs;
mod editorial_review;
mod gesture_preview;
#[cfg(test)]
mod gesture_tests;
mod import_jobs;
mod item_editor;
mod keymap;
mod paths;
mod persistence_jobs;
mod resolve_jobs;
mod scripting;
mod semantic_jobs;
mod timeline_index;
mod ui_markers;
mod ui_media;
mod ui_panels;
mod ui_timeline;
mod v1_documents;
mod watcher;

use std::sync::Arc;

fn main() -> eframe::Result {
    let (console_sink, console_rx) = console::install_tracing();
    tracing::info!("Transcriptor V2 {} iniciando", env!("CARGO_PKG_VERSION"));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Transcriptor V2")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([960.0, 600.0])
        .with_app_id("transcriptor-v2");
    if let Some(icon) = app::load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native("Transcriptor V2", options, Box::new(move |cc| Ok(Box::new(app::TranscriptorApp::new(cc, console_sink, console_rx)))))
}
mod external;
