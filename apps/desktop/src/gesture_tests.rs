//! Pruebas de gestos de ratón sobre la aplicación real con `egui_kittest`
//! (sin ventana): arrastre con previsualización y una única transacción,
//! cancelación con Escape sin residuo, trim de borde y scrub en la regla.

use crate::app::TranscriptorApp;
use crate::ui_timeline::{Gesture, HANDLE_W, RULER_H};
use egui_kittest::Harness;
use std::path::PathBuf;
use tv2_domain::time::Ticks;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media").join(name)
}

fn harness() -> Harness<'static, TranscriptorApp> {
    let (sink, rx) = crossbeam_channel::unbounded();
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_max_steps(64).build_eframe(move |cc| TranscriptorApp::new(cc, sink, rx));
    h.run_steps(2);
    h
}

fn steps(h: &mut Harness<'static, TranscriptorApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn wait_export_started(h: &mut Harness<'static, TranscriptorApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.state().export.running.is_none() && std::time::Instant::now() < deadline {
        steps(h, 1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.state().export.running.is_some(), "durable enqueue did not launch: {:?}", h.state().export.last_result);
}

#[test]
#[ignore = "benchmark release explícito, no prueba de presentación GPU"]
fn dense_ui_benchmark() {
    for count in [10_000usize, 100_000] {
        let mut h = harness();
        prepare(&mut h);
        let mut project = h.state().project().clone();
        project.revision += 1;
        let base = project.sequences[0].clips.clone();
        project.sequences[0].clips = (0..count)
            .map(|i| {
                let mut clip = base[i % base.len()].clone();
                clip.id = tv2_domain::ids::ClipId::new(format!("clip-bench-{i}"));
                clip.link_group = Some(format!("link-bench-{}", i / 2));
                clip.position = Ticks::from_seconds((i / 2) as i64);
                clip.source = tv2_domain::time::TimeRange::new(Ticks::ZERO, Ticks::from_seconds(1));
                clip
            })
            .collect();
        h.state_mut().session = tv2_application::ProjectSession::new(project);
        h.state_mut().timeline_view.px_per_s = 60.0;
        let cold = std::time::Instant::now();
        steps(&mut h, 1);
        let cold_ms = cold.elapsed().as_secs_f64() * 1000.0;
        steps(&mut h, 20);
        let mut samples = Vec::new();
        for frame in 0..200 {
            h.state_mut().timeline_view.scroll_t = Ticks::from_seconds(frame * 3);
            let start = std::time::Instant::now();
            h.step();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "BENCH clips={count} frames={} cold_ms={cold_ms:.3} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3} visited={}",
            samples.len(),
            samples[100],
            samples[190],
            samples[198],
            samples[199],
            h.state().timeline_view.index.last_visited
        );
        assert!(samples[190] <= 16.7, "presupuesto original CPU UI16,7ms superado");
    }
}

#[test]
fn shutdown_joins_active_export_and_caches_and_discards_queue() {
    let mut h = harness();
    prepare(&mut h);
    let dir = tempfile::tempdir().unwrap();
    h.state_mut().export.destination = dir.path().join("active.mp4").display().to_string();
    let preset = tv2_media::export::presets().into_iter().find(|p| p.id == "h264-2160p").unwrap();
    h.state_mut().start_export(preset.clone());
    wait_export_started(&mut h);
    h.state_mut().export.destination = dir.path().join("pending.mp4").display().to_string();
    h.state_mut().start_export(preset);
    assert!(h.state().export.running.is_some());
    assert_eq!(h.state().export.pending.len(), 1);
    let start = std::time::Instant::now();
    h.state_mut().shutdown_workers();
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert!(h.state().export.running.is_none());
    assert!(h.state().media_view.is_none());
    assert!(h.state().player.is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "sin salidas ni staging tras cerrar");
}

#[test]
fn export_selection_compacts_ranges_and_preserves_repeated_editorial_occurrences() {
    use tv2_domain::{
        Command,
        layers::{LayerKind, SemanticItem, SemanticLayer},
        time::TimeRange,
    };
    let mut h = harness();
    let (_, clip) = prepare(&mut h);
    let asset = h.state().project().assets[0].id.clone();
    h.state_mut().playhead = Ticks::from_seconds(15);
    h.state_mut().insert_asset_at_playhead(asset.clone(), None);
    let second = h.state().sequence().unwrap().clips.iter().find(|c| c.position == Ticks::from_seconds(15)).unwrap().id.clone();
    // Mantener un job real activo permite inspeccionar los snapshots pendientes.
    let dir = tempfile::tempdir().unwrap();
    h.state_mut().export.destination = dir.path().join("active.mp4").display().to_string();
    let preset = tv2_media::export::presets().remove(0);
    h.state_mut().start_export(preset.clone());
    wait_export_started(&mut h);
    h.state_mut().selection.clips = vec![clip, second];
    h.state_mut().export.range_mode = 2;
    h.state_mut().export.destination = dir.path().join("selection.mp4").display().to_string();
    h.state_mut().start_export(preset.clone());
    assert_eq!(h.state().export.pending[0].timeline.duration, Ticks::from_seconds(24));
    let layer = SemanticLayer::new(asset.clone(), LayerKind::Blocks, "Bloques");
    let item = SemanticItem::new(TimeRange::new(Ticks::from_seconds(1), Ticks::from_seconds(2)), "Tramo repetido");
    h.state_mut().exec(Command::CreateLayer {
        asset_id: asset,
        kind: LayerKind::Blocks,
        name: "Bloques".into(),
        color: None,
        layer_id: Some(layer.layer_id.clone()),
    });
    h.state_mut().exec(Command::AddItem { layer_id: layer.layer_id.clone(), item: item.clone() });
    h.state_mut().selection.items = vec![(layer.layer_id.clone(), item.item_id.clone())];
    h.state_mut().export.range_mode = 3;
    h.state_mut().export.destination = dir.path().join("blocks.mp4").display().to_string();
    h.state_mut().start_export(preset);
    let snapshot = &h.state().export.pending[1].timeline;
    assert_eq!(snapshot.duration, Ticks::from_seconds(2));
    for second in [0, 1] {
        let t = Ticks::from_seconds(second);
        assert_eq!(snapshot.top_video_at(t).unwrap().source_at(t), Ticks::from_seconds(1));
        assert_eq!(snapshot.piece_at(t).unwrap().audio.len(), 1, "A/V no duplica los rangos");
    }
    h.state_mut().cancel_queued_export(1);
    h.state_mut().cancel_queued_export(0);
    if let Some(job) = &h.state().export.running {
        job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = job.rx.recv_timeout(std::time::Duration::from_secs(15));
    }
}

#[test]
fn markers_snap_drag_and_undo_insert_keeps_linked_audio() {
    let mut h = harness();
    let (track, clip) = prepare(&mut h);
    h.state_mut().playhead = Ticks::from_seconds(3);
    h.state_mut().dispatch("sequence.marker_add");
    h.state_mut().markers_open = false;
    let marker = h.state().sequence().unwrap().markers[0].clone();
    assert_eq!(marker.range.start, Ticks::from_seconds(3));
    h.state_mut().playhead = Ticks::ZERO;
    steps(&mut h, 2);
    let start = clip_center(&h, &track, Ticks::from_seconds(2));
    let target = start + egui::vec2(h.state().timeline_view.px_per_s * 3.0 + 4.0, 0.0);
    h.drag_at(start);
    steps(&mut h, 1);
    h.hover_at(target);
    steps(&mut h, 3);
    h.drop_at(target);
    steps(&mut h, 2);
    assert_eq!(h.state().sequence().unwrap().clip(&clip).unwrap().position, marker.range.start);
    h.state_mut().dispatch("edit.undo");
    // Dos pares A/V, insertar el segundo en el principio desplaza el primero.
    let asset = h.state().project().assets[0].id.clone();
    h.state_mut().playhead = Ticks::from_seconds(12);
    h.state_mut().insert_asset_at_playhead(asset, None);
    let second = h.state().sequence().unwrap().clips.iter().find(|c| c.position == Ticks::from_seconds(12)).unwrap().id.clone();
    h.state_mut().selection.clips = vec![second];
    let moved = h.state().linked_selection();
    assert_eq!(moved.len(), 2);
    h.state_mut().playhead = Ticks::ZERO;
    let rev = h.state().session.revision();
    h.state_mut().dispatch("sequence.insert_selection");
    assert_eq!(h.state().session.revision(), rev + 1);
    for id in &moved {
        assert_eq!(h.state().sequence().unwrap().clip(id).unwrap().position, Ticks::ZERO);
    }
    assert_eq!(h.state().sequence().unwrap().clip(&clip).unwrap().position, Ticks::from_seconds(12));
    h.state_mut().dispatch("edit.undo");
    assert_eq!(h.state().sequence().unwrap().clip(&clip).unwrap().position, Ticks::ZERO);
    for id in &moved {
        assert_eq!(h.state().sequence().unwrap().clip(id).unwrap().position, Ticks::from_seconds(12));
    }
}

#[test]
fn autoscroll_move_keeps_pointer_mapping_and_commits_once() {
    let mut h = harness();
    let (track, _) = prepare(&mut h);
    h.state_mut().timeline_view.px_per_s = 300.0;
    h.state_mut().ui.snapping = false;
    steps(&mut h, 2);
    let start = clip_center(&h, &track, Ticks::from_seconds(1));
    let target = egui::pos2(h.state().timeline_view.last_rect.right() - 3.0, start.y);
    let rev = h.state().session.revision();
    h.drag_at(start);
    steps(&mut h, 1);
    h.hover_at(target);
    steps(&mut h, 12);
    let scroll = h.state().timeline_view.scroll_t;
    assert!(scroll > Ticks::ZERO);
    let delta1 = match h.state().timeline_view.gesture {
        Gesture::MoveClips { delta_t, .. } => delta_t,
        ref g => panic!("{g:?}"),
    };
    steps(&mut h, 12);
    let delta2 = match h.state().timeline_view.gesture {
        Gesture::MoveClips { delta_t, .. } => delta_t,
        ref g => panic!("{g:?}"),
    };
    assert!(delta2 > delta1, "puntero quieto junto al borde sigue moviendo en tiempo fuente");
    assert_eq!(h.state().session.revision(), rev);
    h.drop_at(target);
    steps(&mut h, 2);
    assert_eq!(h.state().session.revision(), rev + 1);
}

#[test]
fn visible_media_cache_draws_in_source_and_sequence_without_editing_project() {
    let mut h = harness();
    prepare(&mut h);
    let rev = h.state().session.revision();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        steps(&mut h, 1);
        let media = h.state().media_view.as_ref().unwrap();
        if media.wave_columns > 100 && media.thumb_tiles > 5 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "cachés visibles no listas");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(h.state().session.revision(), rev);
    h.state_mut().dispatch("view.mode_montage");
    steps(&mut h, 3);
    assert!(h.state().media_view.as_ref().unwrap().thumb_tiles > 5);
    assert_eq!(h.state().session.revision(), rev);
}

#[test]
fn export_queue_cancels_and_freezes_each_revision_before_concurrent_edit() {
    let mut h = harness();
    prepare(&mut h);
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("cancelado.mp4");
    let second = dir.path().join("snapshot.wav");
    let third = dir.path().join("pendiente-cancelado.wav");
    let video = tv2_media::presets().into_iter().find(|p| p.id == "h264-2160p").unwrap();
    let wav = tv2_media::presets().into_iter().find(|p| p.id == "wav-pcm").unwrap();
    h.state_mut().export.destination = first.to_string_lossy().into_owned();
    h.state_mut().start_export(video);
    wait_export_started(&mut h);
    h.state_mut().export.destination = second.to_string_lossy().into_owned();
    h.state_mut().start_export(wav.clone());
    h.state_mut().export.destination = third.to_string_lossy().into_owned();
    h.state_mut().start_export(wav);
    assert_eq!(h.state().export.pending.len(), 2);
    let frozen = h.state().session.revision();
    h.state_mut().cancel_queued_export(1);
    h.state().export.running.as_ref().unwrap().cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let ids = h.state().sequence().unwrap().clips.iter().map(|c| c.id.clone()).collect();
    h.state_mut().exec(tv2_domain::Command::RemoveClips { clip_ids: ids, ripple: false });
    assert!(h.state().sequence().unwrap().clips.is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while h.state().export.running.is_some() || !h.state().export.pending.is_empty() {
        steps(&mut h, 1);
        assert!(std::time::Instant::now() < deadline, "exportación no terminó");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!first.exists() && !third.exists());
    assert!(second.is_file());
    let result = h.state().export.history.iter().find(|(p, _, _)| p == &second).unwrap().2.as_ref().unwrap();
    assert_eq!(result.project_revision, frozen);
    assert_eq!(result.duration, Ticks::from_seconds(12));
    let probe = h.state().tools.as_ref().unwrap().probe(&second).unwrap();
    assert_eq!(probe.audio[0].codec, "pcm_s16le");
    assert_eq!(h.state().export.history.len(), 3);
}

/// Importa la fixture, la inserta y ajusta el zoom. Devuelve (id de pista de video, id del clip de video).
fn prepare(h: &mut Harness<'static, TranscriptorApp>) -> (String, tv2_domain::ids::ClipId) {
    let app = h.state_mut();
    app.import_paths(&[fixture("fixture-a.mp4")]);
    wait_imports(h);
    let app = h.state_mut();
    let asset = app.project().assets[0].id.clone();
    app.insert_asset_at_playhead(asset, None);
    app.dispatch("view.fit");
    steps(h, 3);
    let app = h.state();
    let seq = app.sequence().unwrap();
    let video_track = seq.tracks.iter().find(|t| t.kind == tv2_domain::TrackKind::Video).unwrap();
    let clip = seq.clips.iter().find(|c| c.track_id == video_track.id).unwrap();
    (video_track.id.to_string(), clip.id.clone())
}

fn clip_center(h: &Harness<'static, TranscriptorApp>, track: &str, t: Ticks) -> egui::Pos2 {
    let tv = &h.state().timeline_view;
    let (_, y, hgt) = tv.last_lanes.iter().find(|(id, _, _)| id == track).expect("carril de la pista").clone();
    egui::Pos2::new(tv.t_to_x(t, tv.last_x0), y + hgt / 2.0)
}

#[test]
fn drag_moves_linked_clips_in_one_transaction_and_snaps_to_frames() {
    let mut h = harness();
    let (track, clip) = prepare(&mut h);
    let rev0 = h.state().session.revision();
    let start = clip_center(&h, &track, Ticks::from_seconds(2));
    let px_per_s = h.state().timeline_view.px_per_s;
    h.drag_at(start);
    steps(&mut h, 2);
    // por debajo del umbral no hay gesto de arrastre
    h.hover_at(start + egui::vec2(2.0, 0.0));
    steps(&mut h, 2);
    assert!(matches!(h.state().timeline_view.gesture, Gesture::Pending { .. }), "{:?}", h.state().timeline_view.gesture);
    h.hover_at(start + egui::vec2(60.0, 0.0));
    steps(&mut h, 2);
    h.hover_at(start + egui::vec2(120.0, 0.0));
    steps(&mut h, 2);
    match &h.state().timeline_view.gesture {
        Gesture::MoveClips { clips, delta_t, valid, .. } => {
            assert_eq!(clips.len(), 2, "video + audio enlazado");
            assert!(*valid);
            assert!(delta_t.0 > 0);
        }
        g => panic!("gesto inesperado {g:?}"),
    }
    // durante la previsualización el proyecto no cambia
    assert_eq!(h.state().session.revision(), rev0);
    h.drop_at(start + egui::vec2(120.0, 0.0));
    steps(&mut h, 3);
    let app = h.state();
    assert_eq!(app.session.revision(), rev0 + 1, "una sola transacción por gesto");
    assert!(matches!(app.timeline_view.gesture, Gesture::None));
    let seq = app.sequence().unwrap();
    let moved = seq.clip(&clip).unwrap();
    let expected = Ticks::from_seconds_f64((120.0 / px_per_s) as f64).floor_to_frame(seq.frame_rate);
    assert_eq!(moved.position, expected, "posición alineada a fotograma");
    for c in &seq.clips {
        assert_eq!(c.position, expected, "los clips enlazados se mueven juntos");
    }
    assert_eq!(app.session.undo_label(), Some("Mover clips"));
}

#[test]
fn escape_cancels_drag_without_residue() {
    let mut h = harness();
    let (track, clip) = prepare(&mut h);
    let rev0 = h.state().session.revision();
    let start = clip_center(&h, &track, Ticks::from_seconds(2));
    h.drag_at(start);
    steps(&mut h, 2);
    h.hover_at(start + egui::vec2(150.0, 0.0));
    steps(&mut h, 2);
    assert!(matches!(h.state().timeline_view.gesture, Gesture::MoveClips { .. }));
    h.key_press(egui::Key::Escape);
    steps(&mut h, 2);
    assert!(matches!(h.state().timeline_view.gesture, Gesture::None));
    h.drop_at(start + egui::vec2(150.0, 0.0));
    steps(&mut h, 3);
    let app = h.state();
    assert_eq!(app.session.revision(), rev0, "cancelar no crea revisión");
    assert_eq!(app.sequence().unwrap().clip(&clip).unwrap().position, Ticks::ZERO);
}

#[test]
fn trim_end_handle_shrinks_source_range() {
    let mut h = harness();
    let (track, clip) = prepare(&mut h);
    let rev0 = h.state().session.revision();
    let end_x = {
        let tv = &h.state().timeline_view;
        tv.t_to_x(Ticks::from_seconds(12), tv.last_x0)
    };
    let mut p = clip_center(&h, &track, Ticks::ZERO);
    p.x = end_x - HANDLE_W / 2.0;
    h.drag_at(p);
    steps(&mut h, 2);
    h.hover_at(p + egui::vec2(-200.0, 0.0));
    steps(&mut h, 2);
    assert!(
        matches!(h.state().timeline_view.gesture, Gesture::TrimClip { edge: tv2_domain::ClipEdge::End, .. }),
        "{:?}",
        h.state().timeline_view.gesture
    );
    h.drop_at(p + egui::vec2(-200.0, 0.0));
    steps(&mut h, 3);
    let app = h.state();
    assert_eq!(app.session.revision(), rev0 + 1);
    let c = app.sequence().unwrap().clip(&clip).unwrap();
    assert!(c.source.end < Ticks::from_seconds(12) && c.source.end > Ticks::from_seconds(6), "{:?}", c.source);
    assert_eq!(c.position, Ticks::ZERO, "recortar el fin no mueve el inicio");
}

#[test]
fn ruler_scrub_moves_playhead_and_pauses() {
    let mut h = harness();
    let _ = prepare(&mut h);
    let (x, y) = {
        let tv = &h.state().timeline_view;
        (tv.t_to_x(Ticks::from_seconds(3), tv.last_x0), tv.last_rect.top() + RULER_H / 2.0)
    };
    h.drag_at(egui::Pos2::new(x, y));
    steps(&mut h, 2);
    assert!(matches!(h.state().timeline_view.gesture, Gesture::Scrub));
    let t = h.state().playhead;
    assert!((t - Ticks::from_seconds(3)).abs() < Ticks::from_millis(100), "{t}");
    let x2 = h.state().timeline_view.t_to_x(Ticks::from_seconds(6), h.state().timeline_view.last_x0);
    h.hover_at(egui::Pos2::new(x2, y));
    steps(&mut h, 2);
    let t2 = h.state().playhead;
    assert!((t2 - Ticks::from_seconds(6)).abs() < Ticks::from_millis(100), "{t2}");
    h.drop_at(egui::Pos2::new(x2, y));
    steps(&mut h, 3);
    assert!(matches!(h.state().timeline_view.gesture, Gesture::None));
    assert!(!h.state().player_snapshot.playing);
}

#[test]
fn keyboard_split_does_not_fire_while_typing() {
    let mut h = harness();
    let (_, _clip) = prepare(&mut h);
    let rev0 = h.state().session.revision();
    h.state_mut().seek(Ticks::from_seconds(4));
    steps(&mut h, 2);
    // con el foco en un campo de texto (renombrar), S no divide
    h.state_mut().rename_dialog = Some(("x".into(), crate::app::RenameTarget::Project));
    steps(&mut h, 3);
    h.key_press(egui::Key::S);
    steps(&mut h, 2);
    assert_eq!(h.state().session.revision(), rev0, "S no debe dividir mientras se escribe");
    h.state_mut().rename_dialog = None;
    steps(&mut h, 2);
    h.key_press(egui::Key::S);
    steps(&mut h, 2);
    assert_eq!(h.state().session.revision(), rev0 + 1, "S divide con el foco en el timeline");
    assert_eq!(h.state().sequence().unwrap().clips.len(), 4);
}

fn wait_imports(h: &mut Harness<'static, TranscriptorApp>) {
    let start = std::time::Instant::now();
    while h.state().imports.busy() {
        h.step();
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn async_import_applies_once_preserves_concurrent_edits_and_rejects_old_project() {
    let mut h = harness();
    h.state_mut().import_paths(&[fixture("fixture-a.mp4"), fixture("fixture-a.mp4")]);
    assert!(h.state().imports.running.is_some());
    assert!(h.state().project().assets.is_empty());
    h.state_mut().exec(tv2_domain::Command::RenameProject { name: "Edición durante import".into() });
    wait_imports(&mut h);
    assert_eq!(h.state().project().assets.len(), 1);
    assert_eq!(h.state().project().name, "Edición durante import");
    h.state_mut().import_paths(&[fixture("fixture-b.mp4")]);
    h.state_mut().new_project();
    wait_imports(&mut h);
    assert!(h.state().project().assets.is_empty());
}
