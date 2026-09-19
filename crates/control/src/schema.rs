//! The same closed JSON schemas drive discovery and runtime validation.
use crate::{HostState, Permissions};
use serde_json::{Value, json};
use tv2_domain::Command;

fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":256})
}
fn ticks() -> Value {
    json!({"type":"integer","minimum":0,"maximum":i64::MAX})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn ids() -> Value {
    json!({"type":"array","items":text(),"minItems":1,"maxItems":1000,"uniqueItems":true})
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn optional(schema: Value) -> Value {
    json!({"anyOf":[schema,{"type":"null"}]})
}
fn command(kind: &str, mut fields: Value, required: &[&str]) -> Value {
    fields["type"] = json!({"const":kind});
    let mut required = required.to_vec();
    required.push("type");
    object(fields, &required)
}
fn command_schema() -> Value {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(build_command_schema).clone()
}
fn build_command_schema() -> Value {
    let range = object(json!({"start":ticks(),"end":ticks()}), &["start", "end"]);
    let policy = json!({"enum":["reject","overwrite","insert"]});
    let signed = json!({"type":"integer","minimum":-i64::MAX,"maximum":i64::MAX});
    let index = json!({"type":"integer","minimum":0,"maximum":1000000});
    let prose = json!({"type":"string","maxLength":8192});
    let ranges = json!({"type":"array","items":range,"minItems":1,"maxItems":1000});
    let gain = json!({"type":"number","minimum":-120,"maximum":24});
    let transform = object(
        json!({"x":{"type":"number","minimum":-100,"maximum":100},"y":{"type":"number","minimum":-100,"maximum":100},"scale":{"type":"number","minimum":0.001,"maximum":100},"opacity":{"type":"number","minimum":0,"maximum":1},"fit":{"enum":["fit","fill","stretch","native"]}}),
        &[],
    );
    let item = object(
        json!({"item_id":text(),"label":prose,"comment":prose,"state":{"enum":["proposed","disabled"]},"edited":{"const":false},"parent_id":optional(text()),"ranges":ranges,"origin":{"const":"ai"}}),
        &["item_id", "ranges"],
    );
    let empty = json!({"type":"array","maxItems":0});
    let sequence = object(
        json!({"id":text(),"name":text(),"frame_rate":object(json!({"num":{"type":"integer","minimum":1,"maximum":240000},"den":{"type":"integer","minimum":1,"maximum":10000}}),&["num","den"]),"width":{"type":"integer","minimum":1,"maximum":16384},"height":{"type":"integer","minimum":1,"maximum":16384},"sample_rate":{"type":"integer","minimum":8000,"maximum":192000},"tracks":empty,"clips":empty,"markers":empty}),
        &["id", "name", "frame_rate", "width", "height", "sample_rate", "tracks", "clips"],
    );
    let leaf = json!({"oneOf":[
        command("rename_project",json!({"name":text()}),&["name"]),
        command("set_skip_trims",json!({"enabled":boolean()}),&["enabled"]),
        command("remove_asset",json!({"asset_id":text()}),&["asset_id"]),
        command("set_image_duration",json!({"asset_id":text(),"duration":ticks()}),&["asset_id","duration"]),
        command("add_track",json!({"sequence_id":optional(text()),"kind":{"enum":["video","audio"]},"name":text(),"index":optional(index.clone())}),&["kind","name"]),
        command("remove_track",json!({"track_id":text()}),&["track_id"]),
        command("set_track_props",json!({"track_id":text(),"name":optional(prose.clone()),"muted":optional(boolean()),"solo":optional(boolean()),"locked":optional(boolean()),"visible":optional(boolean()),"gain_db":optional(gain.clone()),"height":optional(json!({"type":"number","minimum":16,"maximum":1000}))}),&["track_id"]),
        command("move_track",json!({"track_id":text(),"new_index":index}),&["track_id","new_index"]),
        command("add_clip",json!({"track_id":text(),"asset_id":text(),"source":range,"position":ticks(),"policy":policy,"clip_id":optional(text()),"link_group":optional(text()),"audio_stream":optional(json!({"type":"integer","minimum":0,"maximum":u32::MAX}))}),&["track_id","asset_id","source","position"]),
        command("insert_asset_linked",json!({"asset_id":text(),"position":ticks(),"video_track":optional(text()),"source":optional(range.clone())}),&["asset_id","position"]),
        command("move_clip",json!({"clip_id":text(),"position":ticks(),"track_id":optional(text()),"policy":policy}),&["clip_id","position"]),
        command("shift_clips",json!({"clip_ids":ids(),"delta":signed,"track_delta":{"type":"integer","minimum":-1000000,"maximum":1000000},"policy":policy}),&["clip_ids","delta","track_delta"]),
        command("split_clip",json!({"clip_id":text(),"at":ticks()}),&["clip_id","at"]),
        command("trim_clip",json!({"clip_id":text(),"edge":{"enum":["start","end"]},"new_time":ticks()}),&["clip_id","edge","new_time"]),
        command("remove_clips",json!({"clip_ids":ids(),"ripple":boolean()}),&["clip_ids","ripple"]),
        command("duplicate_clip",json!({"clip_id":text(),"position":ticks(),"track_id":optional(text())}),&["clip_id","position"]),
        command("set_clip_enabled",json!({"clip_ids":ids(),"enabled":boolean()}),&["clip_ids","enabled"]),
        command("set_clip_editorial",json!({"clip_ids":ids(),"state":{"enum":["proposed","disabled"]},"reason":optional(prose.clone())}),&["clip_ids","state"]),
        command("set_clip_props",json!({"clip_id":text(),"name":optional(prose.clone()),"gain_db":optional(gain),"transform":optional(transform)}),&["clip_id"]),
        command("link_clips",json!({"clip_ids":ids()}),&["clip_ids"]),
        command("unlink_clips",json!({"clip_ids":ids()}),&["clip_ids"]),
        command("add_marker",json!({"range":range,"label":text(),"color":text(),"marker_id":optional(text())}),&["range","label","color"]),
        command("remove_marker",json!({"marker_id":text()}),&["marker_id"]),
        command("set_marker",json!({"marker_id":text(),"range":optional(range.clone()),"label":optional(prose.clone()),"comment":optional(prose.clone()),"color":optional(text())}),&["marker_id"]),
        command("create_layer",json!({"asset_id":text(),"kind":{"enum":["user","topics","ai","author","blocks","trims","silence"]},"name":text(),"color":optional(text()),"layer_id":optional(text())}),&["asset_id","kind","name"]),
        command("delete_layer",json!({"layer_id":text()}),&["layer_id"]),
        command("set_layer_props",json!({"layer_id":text(),"name":optional(prose.clone()),"color":optional(text()),"visible":optional(boolean()),"locked":optional(boolean())}),&["layer_id"]),
        command("set_layer_order",json!({"layer_ids":{"type":"array","items":text(),"maxItems":1000,"uniqueItems":true}}),&["layer_ids"]),
        command("add_item",json!({"layer_id":text(),"item":item}),&["layer_id","item"]),
        command("paste_items",json!({"layer_id":text(),"items":{"type":"array","items":item,"minItems":1,"maxItems":1000},"position":ticks()}),&["layer_id","items","position"]),
        command("set_item_state",json!({"layer_id":text(),"item_ids":ids(),"state":{"enum":["proposed","disabled"]}}),&["layer_id","item_ids","state"]),
        command("set_item_props",json!({"layer_id":text(),"item_id":text(),"label":optional(prose.clone()),"comment":optional(prose),"ranges":optional(ranges.clone())}),&["layer_id","item_id"]),
        command("set_item_structure",json!({"layer_id":text(),"item_id":text(),"parent_id":optional(text()),"ranges":ranges}),&["layer_id","item_id","ranges"]),
        command("set_block_confidence",json!({"layer_id":text(),"item_id":text(),"confidence":{"type":"number","minimum":0,"maximum":1}}),&["layer_id","item_id","confidence"]),
        command("split_item",json!({"layer_id":text(),"item_id":text(),"at":ticks()}),&["layer_id","item_id","at"]),
        command("trim_item",json!({"layer_id":text(),"item_id":text(),"range_index":index,"edge":{"enum":["start","end"]},"new_time":ticks()}),&["layer_id","item_id","range_index","edge","new_time"]),
        command("shift_items",json!({"layer_id":text(),"item_ids":ids(),"delta":signed}),&["layer_id","item_ids","delta"]),
        command("cycle_author_decision",json!({"layer_id":text(),"item_ids":ids()}),&["layer_id","item_ids"]),
        command("box_edit",json!({"layer_id":text(),"range":range,"subtract":boolean()}),&["layer_id","range","subtract"]),
        command("snap_block_boundaries",json!({"layer_id":text(),"radius":ticks()}),&["layer_id","radius"]),
        command("coalesce_trims",json!({"layer_id":text(),"actor_id":optional(text())}),&["layer_id"]),
        command("move_trim_items",json!({"layer_id":text(),"target_layer_id":text(),"item_ids":ids()}),&["layer_id","target_layer_id","item_ids"]),
        command("remove_trim_lane",json!({"layer_id":text(),"move_to":optional(text())}),&["layer_id"]),
        command("delete_items",json!({"layer_id":text(),"item_ids":ids()}),&["layer_id","item_ids"]),
        command("set_active_sequence",json!({"sequence_id":text()}),&["sequence_id"])
        ,command("add_sequence",json!({"sequence":sequence,"activate":boolean()}),&["sequence","activate"])
    ]});
    let mut schema = leaf.clone();
    // Finite schema, not an unconstrained recursive $ref. Four nested batches,
    // at most 128 commands in each array and 128 total runtime command nodes.
    for _ in 0..4 {
        let batch =
            command("batch", json!({"label":text(),"commands":{"type":"array","minItems":1,"maxItems":128,"items":schema}}), &["label", "commands"]);
        schema = leaf.clone();
        schema["oneOf"].as_array_mut().unwrap().push(batch);
    }
    // History is a session operation, never a child of a project-only batch.
    for kind in ["undo", "redo"] {
        schema["oneOf"].as_array_mut().unwrap().push(command(kind, json!({}), &[]));
    }
    schema
}
pub(crate) fn parse_command(value: Value) -> Result<Command, String> {
    fn count(value: &Value, depth: usize, total: &mut usize) -> Result<(), String> {
        *total += 1;
        if *total > 128 || depth > 4 {
            return Err("Batch exceeds 128 command nodes or four nested batches".into());
        }
        if value["type"] == "batch" {
            for command in value["commands"].as_array().ok_or("Batch commands must be an array")? {
                count(command, depth + 1, total)?;
            }
        }
        Ok(())
    }
    count(&value, 0, &mut 0)?;
    validate(&command_schema(), &value)?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
pub(crate) fn command_types() -> Vec<String> {
    command_schema()["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|schema| schema["properties"]["type"]["const"].as_str().map(str::to_owned))
        .collect()
}
fn spec(name: &str, p: &Permissions, h: &HostState) -> Option<(&'static str, Value, bool)> {
    let mut properties = json!({"session_id":text(),"project_id":text()});
    let mut required = vec!["session_id", "project_id"];
    let mut read_only = true;
    let description = match name {
        "tv2_context" if p.read => {
            return Some((
                "Current project identity, revision, digest, clock, selection, transport, permissions and real limits. Start here.",
                object(json!({}), &[]),
                true,
            ));
        }
        "tv2_query" | "tv2_preview" if p.read => {
            properties["kind"] = json!({"enum":["clips","items","markers","layers","tracks","assets","sequences"]});
            properties["target_id"] = text();
            properties["text"] = text();
            properties["start_ticks"] = ticks();
            properties["end_ticks"] = ticks();
            required.push("kind");
            if name == "tv2_preview" {
                properties["proposal_id"] = text();
                required.push("proposal_id");
            }
            "Page project objects in stable storage order; optional text matches names/labels/comments case-insensitively before pagination. Time is source for items, sequence for clips/markers. Preview reads the exact prepared proposal."
        }
        "tv2_evidence" if p.read => {
            properties["asset_id"] = text();
            properties["section"] = text();
            required.extend(["asset_id", "section"]);
            properties["track_id"] = text();
            properties["text"] = text();
            properties["start_ticks"] = ticks();
            properties["end_ticks"] = ticks();
            "Page immutable source evidence. words/utterances/laughter/arousal/emotions return original records filtered by source time, track_id and case-insensitive text in scalar record fields. index lists sections; other sections return bounded metadata summaries."
        }
        "tv2_jobs" if p.read => "Page the running application's job records; no model/inference jobs exist.",
        "tv2_audit" if p.read && h.audit_store.is_some() => {
            properties["revision"] = ticks();
            properties["cursor"] = optional(text());
            properties["limit"] = json!({"type":"integer","minimum":1,"maximum":200});
            required.push("revision");
            return Some((
                "Page saved project audit metadata by verified archive cursor. Cursor binds the archive digest; restart when audit changes. Omits whole command/project snapshot bodies.",
                object(properties, &required),
                true,
            ));
        }
        "tv2_proposals" if p.read => "Page dry-run proposals, diffs, local review status and apply receipts for this control session.",
        "tv2_events" if p.read => {
            properties["after"] = ticks();
            properties["limit"] = json!({"type":"integer","minimum":1,"maximum":200});
            return Some((
                "Poll bounded session audit events by monotonic cursor. gap=true means earlier entries expired.",
                object(properties, &required),
                true,
            ));
        }
        "tv2_propose" if p.propose => {
            properties["revision"] = ticks();
            properties["digest"] = text();
            properties["idempotency_key"] = text();
            properties["command"] = command_schema();
            required.extend(["revision", "digest", "idempotency_key", "command"]);
            return Some((
                "Dry-run a closed typed editorial command. Returns diff and exact preview digest; local review is required before apply. Never replaces project state.",
                object(properties, &required),
                false,
            ));
        }
        "tv2_reprepare" if p.propose => {
            properties["proposal_id"] = text();
            properties["revision"] = ticks();
            properties["digest"] = text();
            properties["idempotency_key"] = text();
            required.extend(["proposal_id", "revision", "digest", "idempotency_key"]);
            return Some((
                "Re-run a restored or obsolete proposal against current revision/digest with a NEW idempotency key. Creates a new exact preview requiring local review or explicit local automatic command scope.",
                object(properties, &required),
                false,
            ));
        }
        "tv2_apply" if p.apply => {
            properties["proposal_id"] = text();
            properties["preview_digest"] = text();
            properties["idempotency_key"] = text();
            required.extend(["proposal_id", "preview_digest", "idempotency_key"]);
            return Some((
                "Commit a prepared command authorized by exact local review or current local automatic command scope, rejecting obsolete revision/content. Repeat the same proposal/key to retrieve its receipt; verify afterward.",
                object(properties, &required),
                false,
            ));
        }
        "tv2_verify" if p.read => {
            properties["proposal_id"] = text();
            required.push("proposal_id");
            return Some((
                "Verify current content in a background hash worker. verification=pending has null matches_preview/digests; retry after retry_after_ms until complete. Always reports the live session receipt and save status; any snapshot change invalidates cached verification.",
                object(properties, &required),
                true,
            ));
        }
        "tv2_select" if p.selection && h.supports_selection => {
            properties["selection"] = object(
                json!({"clip_ids":{"type":"array","items":text(),"maxItems":1000,"uniqueItems":true},"item_ids":{"type":"array","items":text(),"maxItems":1000,"uniqueItems":true},"layer_id":optional(text())}),
                &[],
            );
            required.push("selection");
            read_only = false;
            "Set live GUI selection after validating IDs and revision. Empty selection clears it."
        }
        "tv2_transport" if p.transport && h.supports_transport => {
            properties["operation"] = json!({"enum":["play","pause","seek"]});
            properties["position_ticks"] = ticks();
            required.push("operation");
            read_only = false;
            "Control the running GUI player. seek requires position_ticks in sequence flicks."
        }
        "tv2_cancel_job" if p.jobs && h.supports_job_cancel => {
            properties["job_id"] = text();
            required.push("job_id");
            read_only = false;
            "Ask the host to cancel a known job and return the actual host result."
        }
        "tv2_export" if p.jobs && h.supports_export => {
            read_only = false;
            "Start the application's locally configured export; no destination path or overwrite policy can be supplied remotely."
        }
        "tv2_import" if p.jobs && h.supports_import => {
            read_only = false;
            "Request the normal local media chooser. Returns a pending_local_selection ticket; no path is accepted and no imported media is claimed. Follow its job state after the user chooses or cancels."
        }
        _ => return None,
    };
    properties["revision"] = ticks();
    required.push("revision");
    if read_only {
        properties["limit"] = json!({"type":"integer","minimum":1,"maximum":200});
        properties["offset"] = ticks();
    } else {
        properties["idempotency_key"] = text();
        required.push("idempotency_key");
    }
    Some((description, object(properties, &required), read_only))
}
const NAMES: [&str; 17] = [
    "tv2_context",
    "tv2_query",
    "tv2_evidence",
    "tv2_jobs",
    "tv2_events",
    "tv2_audit",
    "tv2_propose",
    "tv2_reprepare",
    "tv2_proposals",
    "tv2_preview",
    "tv2_apply",
    "tv2_verify",
    "tv2_select",
    "tv2_transport",
    "tv2_cancel_job",
    "tv2_export",
    "tv2_import",
];
pub(crate) fn tools(p: &Permissions, h: &HostState) -> Vec<Value> {
    NAMES
        .iter()
        .filter_map(|name| {
            spec(name, p, h).map(|(description, schema, read)| {
                json!({"name":name,"description":description,"inputSchema":schema,
        "annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":true,"openWorldHint":false}})
            })
        })
        .collect()
}
pub(crate) fn validate_arguments(name: &str, a: &Value, p: &Permissions, h: &HostState) -> Result<(), String> {
    let (_, schema, _) = spec(name, p, h).ok_or("E_PERMISSION: tool unavailable for current permissions/host capabilities")?;
    validate(&schema, a)
}
fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(options) = schema.get("anyOf").or_else(|| schema.get("oneOf")).and_then(Value::as_array) {
        if options.iter().filter(|s| validate(s, value).is_ok()).count() == 1 {
            return Ok(());
        }
        return Err("Input does not match a supported schema variant".into());
    }
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err("Unsupported command type".into());
    }
    if let Some(options) = schema["enum"].as_array()
        && !options.contains(value)
    {
        return Err("Input is outside the allowed enum".into());
    }
    if let Some(kind) = schema["type"].as_str() {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "null" => value.is_null(),
            _ => false,
        };
        if !valid {
            return Err(format!("Expected {kind}"));
        }
    }
    if let Some(object) = value.as_object()
        && schema["type"] == "object"
    {
        for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(format!("Required field: {key}"));
            }
        }
        for (key, value) in object {
            let field = schema["properties"].get(key).ok_or_else(|| format!("Unknown field: {key}"))?;
            validate(field, value).map_err(|e| format!("{key}: {e}"))?;
        }
    }
    if let Some(array) = value.as_array() {
        if schema["maxItems"].as_u64().is_some_and(|n| array.len() as u64 > n)
            || schema["minItems"].as_u64().is_some_and(|n| (array.len() as u64) < n)
        {
            return Err("Array length outside limits".into());
        }
        for (index, value) in array.iter().enumerate() {
            validate(&schema["items"], value)?;
            if schema["uniqueItems"] == true && array[..index].contains(value) {
                return Err("Duplicate array entry".into());
            }
        }
    }
    if let Some(string) = value.as_str()
        && (schema["maxLength"].as_u64().is_some_and(|n| string.chars().count() as u64 > n)
            || schema["minLength"].as_u64().is_some_and(|n| (string.chars().count() as u64) < n))
    {
        return Err("String length outside limits".into());
    }
    if let Some(number) = value.as_f64()
        && (schema["minimum"].as_f64().is_some_and(|min| number < min) || schema["maximum"].as_f64().is_some_and(|max| number > max))
    {
        return Err("Number outside limits".into());
    }
    Ok(())
}
