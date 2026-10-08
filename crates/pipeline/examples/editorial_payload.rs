//! Interchange check for an invented E5 stdlib fixture; no inference or GUI IO.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use tv2_application::{Actor, CommandEnvelope, ProjectSession};
use tv2_domain::{Asset, Command, ItemState, Project};
use tv2_v1compat::master::V1Master;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err("usage: editorial_payload MASTER_JSON ASSET_JSON REPORT_JSON".into());
    }
    let master_bytes = fs::read(&arguments[0])?;
    let original: Value = serde_json::from_slice(&master_bytes)?;
    let master = V1Master::parse(original.clone())?;
    let asset: Asset = serde_json::from_slice(&fs::read(&arguments[1])?)?;
    assert_eq!(master.fingerprint.size, fs::metadata(&asset.path)?.len());
    assert!(master.fingerprint.same_identity(&asset.fingerprint));
    assert_eq!(master.duration, asset.duration());
    let mut missing_size = original.clone();
    missing_size["media"]["fingerprint"].as_object_mut().unwrap().remove("size");
    assert!(V1Master::parse(missing_size).is_err(), "old incomplete fixture must fail");
    let mut invalid_size = original.clone();
    invalid_size["media"]["fingerprint"]["size"] = json!(-1);
    assert!(V1Master::parse(invalid_size).is_err(), "size must be u64");

    let mut project = Project::new("Invented E5 interchange fixture");
    project.assets.push(asset.clone());
    project.validate()?;
    let evidence = master.evidence(&asset.id);
    evidence.validate(&project)?;
    let layers = master.projections(&asset.id)?;
    assert_eq!(layers.len(), 2, "one word layer and one utterance layer");
    assert!(layers.iter().all(|layer| layer.locked));
    assert!(layers.iter().flat_map(|layer| &layer.items).all(|item| item.state == ItemState::Proposed && !item.edited));
    let word = layers.iter().flat_map(|layer| &layer.items).find(|item| item.item_id.as_str() == "a0-w-000001").ok_or("projected word ID missing")?;
    assert_eq!(word.extra["evidence"]["asr_prob"], json!(0.91));
    assert_eq!(word.extra["evidence"]["rms_dbfs"], json!(-12.041));

    let mut commands = vec![Command::AttachMaster { master: evidence }];
    commands.extend(layers.into_iter().map(|layer| Command::ReplaceLayer { layer }));
    let mut session = ProjectSession::new(project.clone());
    let envelope = CommandEnvelope::human(Command::Batch { label: "Invented E5 interchange".into(), commands })
        .with_actor(Actor::External { source: "e5-stdlib-fixture-only".into() })
        .with_base(session.revision());
    let prepared = session.prepare_command(envelope)?;
    assert_eq!(session.project(), &project, "preparation must not mutate even the in-memory project");
    let applied = session.commit_prepared(prepared)?;
    assert_eq!(session.project().masters.len(), 1);
    assert_eq!(session.project().layers.len(), 2);
    assert_eq!(session.project().masters[0].document, original);
    session.project().masters[0].validate(session.project())?;
    session.undo(Actor::Human)?;
    assert!(session.project().masters.is_empty());
    assert!(session.project().layers.is_empty());
    assert_eq!(session.project().assets, project.assets);
    assert_eq!(fs::read(&arguments[0])?, master_bytes, "original fixture remains immutable");

    let report = json!({
        "fixture_only": true, "inference_accepted": false, "gui_exercised": false,
        "master_sha256": hex::encode(Sha256::digest(&master_bytes)),
        "fingerprint_size": master.fingerprint.size, "source_digest": master.source_master_digest(),
        "v1_master_parse": "passed", "missing_or_negative_size_rejected": true,
        "projections": {"layers": 2, "word_evidence_preserved": true, "locked": true, "human_edits_fabricated": false},
        "master_evidence_validate": "passed", "prepare_did_not_mutate": true,
        "in_memory_commit_revision": applied.new_revision, "in_memory_undo_revision": session.revision(),
        "project_written": false, "original_fixture_unchanged": true
    });
    fs::write(&arguments[2], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
