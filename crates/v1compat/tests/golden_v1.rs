//! Golden contra V1 real: `tests/fixtures/v1/demo-a` fue generado ejecutando los
//! módulos de V1 (`make_v1_fixture.py`) sobre `fixture-a.mp4`; `expected-v1.json`
//! contiene lo que V1 calculó (fingerprint, unión de recortes, aplanado del
//! montaje, mapping fuente→secuencia). Aquí V2 debe coincidir sin usar V1.

use std::path::PathBuf;
use tv2_domain::commands::Command;
use tv2_domain::project::Project;
use tv2_domain::time::Ticks;
use tv2_media::ffmpeg::FfmpegTools;
use tv2_v1compat::import::{import_commands, read_v1_editorial};
use tv2_v1compat::master::V1Master;
use tv2_v1compat::montaje::V1Montaje;
use tv2_v1compat::trims::{enabled_intervals, trims_from_v1};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn expected() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(root().join("v1/demo-a/expected-v1.json")).unwrap()).unwrap()
}

#[test]
fn v2_fingerprint_equals_v1_fingerprint_for_the_same_file() {
    let tools = FfmpegTools::locate().expect("ffmpeg vendorizado");
    let media = root().join("media/fixture-a.mp4");
    let probe = tools.probe(&media).unwrap();
    let fp = tools.fingerprint(&media, &probe).unwrap();
    let exp = expected();
    assert_eq!(fp.size, exp["fingerprint"]["size"].as_u64().unwrap());
    assert_eq!(fp.hash_muestreado, exp["fingerprint"]["hash_muestreado"].as_str().unwrap(), "hash muestreado (3 bloques de 8 MiB + size)");
    assert_eq!(fp.inventario_sha256, exp["fingerprint"]["inventario_sha256"].as_str().unwrap(), "inventario ffprobe canónico (json.dumps sort_keys)");
    let master = V1Master::load(&root().join("v1/demo-a/editorial/demo-a.editorial.master.json")).unwrap();
    assert!(master.fingerprint.same_identity(&fp));
}

#[test]
fn source_master_digest_matches_v1() {
    let master = V1Master::load(&root().join("v1/demo-a/editorial/demo-a.editorial.master.json")).unwrap();
    // el generador reescribió `media.path` a una ruta relativa tras calcular el digest en V1:
    // se compara con el digest recalculado sobre la misma ruta que usó V1
    let mut raw = master.raw.clone();
    raw["media"]["path"] = expected()["info"]["path"].clone();
    let m2 = V1Master::parse(raw).unwrap();
    assert_eq!(m2.source_master_digest(), expected()["source_master_digest"].as_str().unwrap());
}

#[test]
fn trims_union_and_montage_flatten_match_v1() {
    let exp = expected();
    let asset = tv2_domain::ids::AssetId::new("a");
    let trims_raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("v1/demo-a/editorial/views/trims.json")).unwrap()).unwrap();
    let trims = trims_from_v1(&trims_raw, &asset, None).unwrap();
    let layers: Vec<&tv2_domain::layers::SemanticLayer> = trims.layers.iter().collect();
    let got: Vec<[f64; 2]> = enabled_intervals(&layers).iter().map(|r| [r.start.as_seconds_ms(), r.end.as_seconds_ms()]).collect();
    let want: Vec<[f64; 2]> =
        exp["trims_enabled_intervals"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]).collect();
    assert_eq!(got, want, "unión de recortes habilitados (el desactivado no corta, el propuesto sí)");

    let montaje_raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("v1/demo-a/editorial/views/montaje.json")).unwrap()).unwrap();
    let m = V1Montaje::parse(montaje_raw, None).unwrap();
    let pieces = m.flatten(false);
    let want = exp["montaje_flatten"].as_array().unwrap();
    assert_eq!(pieces.len(), want.len());
    for (p, w) in pieces.iter().zip(want) {
        assert_eq!(p.clip_id, w["clip_id"].as_str().unwrap());
        assert_eq!(p.track_id, w["track_id"].as_str().unwrap());
        assert_eq!(
            (p.seq_ini, p.seq_fin, p.source_ini, p.source_fin),
            (w["seq_ini"].as_f64().unwrap(), w["seq_fin"].as_f64().unwrap(), w["source_ini"].as_f64().unwrap(), w["source_fin"].as_f64().unwrap())
        );
    }
    assert_eq!(m.total_seconds(), exp["montaje_total_seconds"].as_f64().unwrap());
    let s2s: Vec<f64> = exp["source_to_seq_1_5"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert_eq!(m.source_to_seq(1.5), s2s, "el instante fuente 1,5 s aparece dos veces (clip repetido)");
}

#[test]
fn folder_import_produces_v2_project_with_same_observable_sequence() {
    let tools = FfmpegTools::locate().expect("ffmpeg vendorizado");
    let media = root().join("media/fixture-a.mp4");
    let asset = tools.import(&media, media.to_string_lossy().to_string()).unwrap();
    let mut project = Project::new("golden");
    Command::ImportAsset { asset: asset.clone() }.apply(&mut project).unwrap();
    let import = read_v1_editorial(&root().join("v1/demo-a"), &asset).unwrap();
    assert!(import.report.warnings.is_empty(), "{:?}", import.report.warnings);
    assert_eq!(import.report.layers, 2);
    assert_eq!(import.report.trims_layers, 2);
    let cmds = import_commands(&import, &project, &asset.id);
    Command::Batch { label: "v1".into(), commands: cmds }.apply(&mut project).unwrap();
    let topics = project.layer(&tv2_domain::ids::LayerId::new("layer-demo-topics")).unwrap();
    assert_eq!(topics.items.len(), 2);
    assert_eq!(topics.deleted_item_ids[0].as_str(), "item-borrado");
    assert_eq!(topics.items[0].state, tv2_domain::layers::ItemState::Accepted);
    assert_eq!(topics.items[1].parent_id.as_ref().unwrap().as_str(), "item-tema-1");
    let seq = project.active().unwrap();
    let total: Ticks = expected()["montaje_total_seconds"].as_f64().map(tv2_v1compat::secs_to_ticks).unwrap();
    assert_eq!(seq.extent(), total, "duración de la secuencia = total_seconds del montaje V1");
    // el instante fuente 1,5 s suena en dos posiciones de secuencia, como en V1
    let places = seq.source_to_seq(&asset.id, Ticks::from_millis(1500));
    let video_places: Vec<f64> = places.iter().filter(|(id, _)| !id.as_str().contains("-a")).map(|(_, t)| t.as_seconds_ms()).collect();
    let want: Vec<f64> = expected()["source_to_seq_1_5"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    // V1 mapea sobre la secuencia con huecos; aquí el montaje no tiene huecos, así que coinciden
    assert_eq!(video_places, want);
}
