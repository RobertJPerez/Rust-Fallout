//! Read-only requests over protected winning world/content sources.
use super::{Result, inspection_input::Order};
use fallout_data::{
    condition_operands::Signatures,
    identity::FormKey,
    loaded_scripts::{Catalogue, Limits as ScriptLimits},
    store::RecordStore,
    terrain::preparation::{Receipt as TerrainReceipt, TexturePreparation, TextureSourcePlan},
    vfs::MountIndex,
    world::{
        cells::CellGridSources,
        conversation::{DialogueSources, Limits},
        doors::DoorDestination,
        preparation::CellModelPlan,
        residency::{CellResidency, Snapshot, Stage, TerrainState, TexturePlan, TextureState},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) struct ResidencyInput {
    pub cell: FormKey,
    pub source_timeout_ms: u64,
}

/// An executable source consumer over the same leased model/texture jobs used
/// by the host. It never reports GPU, collision, behavior or dependency Ready.
pub(super) fn residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: ResidencyInput,
) -> Result<Value> {
    let deadline = source_deadline(input.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
    let plan = CellModelPlan::load(&mut store, &input.cell, assets.mounts(), Default::default())?;
    let mut report = consume_plan(install, resource_cache, deadline, plan, assets.mounts())?;
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) struct DoorInput {
    pub source: ResidencyInput,
    pub door: FormKey,
}

pub(super) struct GridInput {
    pub world: FormKey,
    pub grid: [i32; 2],
    pub source_timeout_ms: u64,
}

pub(super) fn grid_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::Models,
    )
}

pub(super) fn grid_terrain(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::TerrainTextures,
    )
}

pub(super) fn grid_cell_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::CellSources,
    )
}

#[derive(Clone, Copy)]
enum GridKind {
    Models,
    TerrainTextures,
    CellSources,
}

fn grid_report(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
    kind: GridKind,
) -> Result<Value> {
    let deadline = source_deadline(input.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = CellGridSources::load(&mut store, &input.world, Default::default())?;
    let request = sources.request(input.grid);
    let mut selected = None;
    let plan = match request {
        Ok(request) => {
            selected = Some(serde_json::to_value(&request)?);
            let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
            match kind {
                GridKind::Models => match sources.prepare_cell(
                    &mut store,
                    &request,
                    assets.mounts(),
                    Default::default(),
                ) {
                    Ok(plan) => Ok(consume_plan(
                        install,
                        resource_cache,
                        deadline,
                        plan,
                        assets.mounts(),
                    )?),
                    Err(error) => Err(error.to_string()),
                },
                GridKind::TerrainTextures => match sources.prepare_terrain(
                    &mut store,
                    &request,
                    assets.mounts(),
                    Default::default(),
                ) {
                    Ok(plan) => Ok(consume_terrain(install, resource_cache, deadline, plan)?),
                    Err(error) => Err(error.to_string()),
                },
                GridKind::CellSources => {
                    let prepared = (|| -> fallout_data::Result<_> {
                        let models = sources.prepare_cell(
                            &mut store,
                            &request,
                            assets.mounts(),
                            Default::default(),
                        )?;
                        let terrain = sources.prepare_terrain(
                            &mut store,
                            &request,
                            assets.mounts(),
                            Default::default(),
                        )?;
                        Ok((models, terrain))
                    })();
                    match prepared {
                        Ok((models, terrain)) => Ok(consume_cell_plan(
                            install,
                            resource_cache,
                            deadline,
                            models,
                            assets.mounts(),
                            Some(terrain),
                        )?),
                        Err(error) => Err(error.to_string()),
                    }
                }
            }
        }
        Err(error) => Err(error.to_string()),
    };
    let mut report = match plan {
        Ok(report) => report,
        Err(error) => json!({"schema_version":1,"profile":"nv-original",
            "source_error":error,"cell_models":null,"cell_textures":null,
            "texture_payloads":[],"residency":null,"captured_sources_available":false,
            "lookup_precedence_verified":false,"runtime_ready":false,"retail_parity_accepted":false}),
    };
    provenance(&mut report, &order, &mut store)?;
    report["cell_grid_sources"] = serde_json::to_value(sources.metadata())?;
    report["explicit_grid"] = json!(input.grid);
    report["cell_grid_request"] = json!(selected);
    report["current_cell_changed"] = json!(false);
    report["activation_applied"] = json!(false);
    match kind {
        GridKind::Models => {
            report["scope"] = json!(
                "Explicit WRLD/XCLC source CELL model/texture jobs; persistent groups remain separate, no position/grid inference or runtime activation"
            )
        }
        GridKind::TerrainTextures => {
            report["surface_prepared"] = json!(false);
            report["scope"] = json!(
                "Explicit WRLD/XCLC CELL strict LAND/world/layer external texture source jobs; surface, inheritance and runtime activation remain unadmitted"
            );
        }
        GridKind::CellSources => {
            report["terrain_scope_requested"] = json!(true);
            report["surface_prepared"] = json!(false);
            report["scope"] = json!(
                "Explicit WRLD/XCLC CELL model and both texture source batches in one residency epoch; retained leases checked across unload and final quota drain; no surface, inheritance or runtime activation"
            );
        }
    }
    Ok(report)
}

fn consume_terrain(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: TextureSourcePlan,
) -> Result<Value> {
    let source_plan = serde_json::to_value(plan.receipt())?;
    let mut preparation =
        TexturePreparation::new(plan, install, resource_cache, Default::default())?;
    let start = Instant::now();
    let result = (|| -> Result<TerrainReceipt> {
        loop {
            if preparation.poll()? {
                let ready = preparation
                    .take_ready()?
                    .ok_or("terrain source batch completed without receipt")?;
                return Ok(ready.publish_for(preparation.plan().terrain())?);
            }
            if start.elapsed() >= deadline {
                return Err(
                    "terrain source polling deadline exceeded; owned request cancelled".into(),
                );
            }
            thread::sleep(Duration::from_millis(1));
        }
    })();
    let (textures, error, available) = match result {
        Ok(receipt) => {
            let available = receipt.all_requested_texture_sources_ready
                && receipt.all_authored_texture_sources_resolved;
            (Some(serde_json::to_value(receipt)?), None, available)
        }
        Err(error) => {
            preparation.cancel();
            (None, Some(error.to_string()), false)
        }
    };
    let usage = preparation.usage();
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "terrain_source_plan":source_plan,"terrain_textures":textures,
        "terrain_job_usage":{"outstanding":usage.outstanding,"decoded_bytes":usage.decoded_bytes},"source_error":error,
        "captured_sources_available":available,"lookup_precedence_verified":false,
        "surface_prepared":false,"runtime_ready":false,"retail_parity_accepted":false}))
}

pub(super) fn door_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: DoorInput,
) -> Result<Value> {
    let deadline = source_deadline(input.source.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = DoorDestination::load(
        &mut store,
        &input.source.cell,
        &input.door,
        Default::default(),
    )?;
    let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
    let mut report = match sources.prepare_cell(&mut store, assets.mounts(), Default::default()) {
        Ok(plan) => consume_plan(install, resource_cache, deadline, plan, assets.mounts())?,
        Err(error) => json!({"schema_version":1,"profile":"nv-original",
            "source_error":error.to_string(),"cell_models":null,"cell_textures":null,
            "texture_payloads":[],"residency":null,"captured_sources_available":false,
            "lookup_precedence_verified":false,"runtime_ready":false,"retail_parity_accepted":false}),
    };
    provenance(&mut report, &order, &mut store)?;
    report["door_destination"] = serde_json::to_value(sources.metadata())?;
    report["door_source_graph"] = serde_json::to_value(sources.graph())?;
    report["destination_applied"] = json!(false);
    report["current_cell_changed"] = json!(false);
    report["scope"] = json!(
        "Source-selected XTEL destination CELL model/texture jobs; no movement, activation, GPU, physics or behavior admission"
    );
    Ok(report)
}

fn source_deadline(milliseconds: u64) -> Result<Duration> {
    if !(1..=120_000).contains(&milliseconds) {
        return Err("source polling timeout must be 1..=120000 milliseconds".into());
    }
    Ok(Duration::from_millis(milliseconds))
}

fn provenance(report: &mut Value, order: &Order, store: &mut RecordStore) -> Result<()> {
    report["explicit_load_order"] = json!(order.names);
    report["load_order_sha256"] = json!(order.sha256);
    report["plugins"] = serde_json::to_value(store.source_receipts()?)?;
    Ok(())
}

fn consume_plan(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: CellModelPlan,
    mounts: &MountIndex,
) -> Result<Value> {
    consume_cell_plan(install, resource_cache, deadline, plan, mounts, None)
}

fn consume_cell_plan(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: CellModelPlan,
    mounts: &MountIndex,
    terrain: Option<TextureSourcePlan>,
) -> Result<Value> {
    let model_receipt = serde_json::to_value(plan.receipt())?;
    let terrain_requested = terrain.is_some();
    let terrain_receipt = terrain
        .as_ref()
        .map(|plan| serde_json::to_value(plan.receipt()))
        .transpose()?;
    let mut owner = CellResidency::new(install, resource_cache, Default::default())?;
    let ticket = owner.request(plan)?;
    let mut error = None;
    let mut textures = None;
    let mut texture_payloads = Vec::new();
    let mut terrain_payloads = Vec::new();
    if let Err(failed) = poll_sources(&mut owner, false, deadline) {
        error = Some(failed.to_string());
    } else {
        match TexturePlan::load(owner.sources(&ticket)?, mounts, Default::default()) {
            Ok(plan) => {
                textures = Some(serde_json::to_value(plan.receipt())?);
                owner.request_textures(&ticket, plan)?;
                if let Some(plan) = terrain {
                    owner.request_terrain(&ticket, plan)?;
                }
                if let Err(failed) =
                    poll_cell_sources(&mut owner, true, terrain_requested, deadline)
                {
                    error = Some(failed.to_string());
                } else {
                    // Consume the retained lease, not another archive/cache read.
                    let sources = owner.texture_sources(&ticket)?;
                    for index in 0..sources.receipt()?.requests.len() {
                        let bytes = sources.texture(index)?;
                        if bytes.len() != sources.receipt()?.requests[index].decoded_bytes {
                            return Err(
                                "resident texture extent differs from sealed source request".into(),
                            );
                        }
                        texture_payloads.push(json!({"request":index,"bytes":bytes.len(),
                            "sha256":format!("{:x}", Sha256::digest(bytes))}));
                    }
                    if terrain_requested {
                        let sources = owner.terrain_sources(&ticket)?;
                        for (index, request) in sources.receipt()?.requests.iter().enumerate() {
                            let bytes = sources.texture(index)?;
                            if bytes.len() != request.decoded_bytes {
                                return Err("resident terrain texture extent differs from sealed source request".into());
                            }
                            terrain_payloads.push(json!({"request":index,"bytes":bytes.len(),
                                "sha256":format!("{:x}", Sha256::digest(bytes)),
                                "cell_identity":sources.ticket().identity(),"generation":sources.ticket().generation()}));
                        }
                    }
                }
            }
            Err(failed) => error = Some(failed.to_string()),
        }
    }
    let snapshot = owner.snapshot();
    let available = error.is_none()
        && snapshot.complete_model_coverage
        && snapshot.complete_texture_coverage
        && (!terrain_requested || snapshot.complete_terrain_coverage);
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "cell_models":model_receipt,"cell_textures":textures,"texture_payloads":texture_payloads,
        "residency":snapshot,"source_error":error,
        "captured_sources_available":available,"lookup_precedence_verified":false,
        "runtime_ready":false,"retail_parity_accepted":false,
        "scope":"Protected exact CELL/model/external-texture source jobs; no GPU/DDS/physics/behavior admission"});
    if terrain_requested {
        report["cell_terrain"] = json!(terrain_receipt);
        report["terrain_payloads"] = json!(terrain_payloads);
        // Exercise the real lifetime with leases held across unload. Payloads
        // are consumed above; no source read or staging can follow revocation.
        let models = owner.sources(&ticket).ok();
        let textures = owner.texture_sources(&ticket).ok();
        let terrain = owner.terrain_sources(&ticket).ok();
        let terrain_ticket_matches = terrain.as_ref().is_some_and(|sources| {
            sources.ticket().identity() == ticket.identity()
                && sources.ticket().generation() == ticket.generation()
                && sources.ticket().root() == ticket.root()
        });
        owner.unload()?;
        report["residency_after_unload"] = serde_json::to_value(owner.poll()?)?;
        report["retained_source_lifetime"] = json!({
            "terrain_ticket_matches_cell":terrain_ticket_matches,
            "old_ticket_rejected":ticket.check().is_err(),
            "old_owner_access_rejected":owner.terrain_sources(&ticket).is_err(),
            "borrowed_model_access_rejected":models.as_ref().map(|s| s.plan().is_err()),
            "borrowed_texture_access_rejected":textures.as_ref().map(|s| s.receipt().is_err()),
            "borrowed_terrain_access_rejected":terrain.as_ref().map(|s| s.receipt().is_err()),
            "terrain_lease_retained":terrain.is_some()});
        drop(terrain);
        drop(textures);
        drop(models);
        let start = Instant::now();
        loop {
            let drained = owner.poll()?;
            if drained.stage == Stage::Unrequested {
                report["residency_after_release"] = serde_json::to_value(drained)?;
                break;
            }
            if start.elapsed() >= deadline {
                return Err(
                    "unloaded CELL source reservations did not drain within deadline".into(),
                );
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(report)
}

fn poll_sources(owner: &mut CellResidency, textures: bool, timeout: Duration) -> Result<Snapshot> {
    poll_cell_sources(owner, textures, false, timeout)
}
fn poll_cell_sources(
    owner: &mut CellResidency,
    textures: bool,
    terrain: bool,
    timeout: Duration,
) -> Result<Snapshot> {
    let start = Instant::now();
    loop {
        let snapshot = owner.poll()?;
        let complete = if textures {
            matches!(
                snapshot.texture_state,
                TextureState::Decoded | TextureState::Unsupported
            )
        } else {
            snapshot.stage == Stage::Decoded
        };
        let terrain_complete = !terrain
            || matches!(
                snapshot.terrain_state,
                TerrainState::Decoded | TerrainState::Unsupported
            );
        if complete && terrain_complete {
            return Ok(snapshot);
        }
        if start.elapsed() >= timeout {
            owner.unload()?;
            return Err("source polling deadline exceeded; owned cell request cancelled".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

pub(super) struct Input {
    pub topic: FormKey,
    pub info: FormKey,
    pub speaker: Option<FormKey>,
    pub bind_result_fragments: bool,
}
pub(super) fn conversation(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    input: Input,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let sources = DialogueSources::build(&mut store, Limits::default())?;
    let request = sources.request(input.topic, input.info, input.speaker)?;
    // This is an exact source inspection, not a native function metadata capture.
    // Unknown function signatures remain explicit in each existing CTDA binding.
    let prepared = sources.prepare(&mut store, &request, &Signatures::new(), Limits::default())?;
    let mut subtitle_payloads = Vec::new();
    for (response_index, response) in prepared.metadata().responses.iter().enumerate() {
        let mut occurrence = 0;
        for &field_index in &response.fields {
            let field = &prepared.metadata().info_fields[field_index];
            if field.kind != *b"NAM1" {
                continue;
            }
            // Indexed access over retained bytes avoids repeated nth scans when
            // one response has many distinct NAM1 occurrences.
            let bytes = prepared
                .info_bytes(field_index)
                .ok_or("prepared subtitle field has no retained source span")?;
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            if sha256 != field.sha256 {
                return Err("retained subtitle bytes differ from prepared source field".into());
            }
            subtitle_payloads.push(json!({"response":response_index,
                "source_response_number":response.number,"occurrence":occurrence,
                "info_field":field_index,"bytes":bytes.len(),"sha256":sha256}));
            occurrence += 1;
        }
    }
    let fragments = if input.bind_result_fragments {
        let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(()))?;
        Some(prepared.metadata().fragments.iter().map(|fragment| {
            let loaded = fragment.resolve(&catalogue)?;
            Ok(json!({"handle":loaded.handle(),"version":loaded.version(),"owner":loaded.owner(),
                "issues":loaded.issues(),"execution_admitted":false}))
        }).collect::<fallout_data::Result<Vec<_>>>()?)
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,
        "load_order_sha256":order.sha256,"plugins":store.source_receipts()?,
        "membership_metadata_bytes":sources.retained_bytes(),"retained_conversation_bytes":prepared.retained_bytes(),
        "conversation":prepared.metadata(),"subtitle_payloads":subtitle_payloads,"loaded_fragments":fragments,
        "condition_signatures_supplied":false,"runtime_ready":false,"retail_parity_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Private ignored fixtures, kept for review rather than adding a dependency
    // or deleting an unrelated temporary tree during a shared team run.
    fn directory() -> std::path::PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/world-conversation-cli-fixtures");
        fs::create_dir_all(&root).unwrap();
        let mut ordinal = 0;
        loop {
            let path = root.join(format!("{}-{ordinal}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ordinal += 1,
                Err(error) => panic!("private conversation fixture: {error}"),
            }
        }
    }

    fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
    }
    fn record(kind: &[u8; 4], form: u32, body: &[u8]) -> Vec<u8> {
        [
            kind.as_slice(),
            &(body.len() as u32).to_le_bytes(),
            &[0; 4],
            &form.to_le_bytes(),
            &[0; 8],
            body,
        ]
        .concat()
    }
    fn fixture(root: &Path) {
        fixture_subtitles(root, &[b"authored_fixture_line\0"], None);
    }
    fn fixture_subtitles(root: &Path, subtitles: &[&[u8]], orphan: Option<&[u8]>) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
        let mut body = Vec::new();
        if let Some(orphan) = orphan {
            body.extend(field(b"NAM1", orphan));
        }
        body.extend(field(b"TRDT", &[0; 24]));
        for subtitle in subtitles {
            body.extend(field(b"NAM1", subtitle));
        }
        body.extend(field(b"SCHR", &schr));
        body.extend(field(b"SCDA", &[0x1d, 0, 0, 0]));
        let info = record(b"INFO", 0x300, &body);
        let group = [
            b"GRUP".as_slice(),
            &(info.len() as u32 + 24).to_le_bytes(),
            &0x100_u32.to_le_bytes(),
            &7_i32.to_le_bytes(),
            &[0; 8],
            &info,
        ]
        .concat();
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"DIAL", 0x100, &field(b"FULL", b"authored_fixture_topic\0")),
                group,
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
    }
    fn input(info: &str) -> Input {
        Input {
            topic: crate::parse_cell_key("Base.esm:100").unwrap(),
            info: crate::parse_cell_key(info).unwrap(),
            speaker: None,
            bind_result_fragments: true,
        }
    }
    #[test]
    fn cli_consumer_prepares_exact_sources_and_binds_existing_loaded_unit() {
        let directory = directory();
        fixture(&directory);
        let report = conversation(
            &directory,
            &directory.join("order.json"),
            None,
            input("Base.esm:300"),
        )
        .unwrap();
        assert_eq!(report["conversation"]["info"]["key"]["local_id"], 0x300);
        assert_eq!(
            report["subtitle_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored_fixture_line\0"))
        );
        assert_eq!(
            report["conversation"]["responses"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            report["loaded_fragments"][0]["handle"]["key"]["record"]["local_id"],
            0x300
        );
        assert_eq!(report["loaded_fragments"][0]["execution_admitted"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(report["condition_signatures_supplied"], false);
        assert_eq!(report["retail_parity_accepted"], false);
        assert!(!report.to_string().contains("authored_fixture"));
    }
    #[test]
    fn cli_subtitle_consumer_keeps_repeated_non_utf8_occurrences_and_orphans_distinct() {
        let directory = directory();
        let subtitles: [&[u8]; 3] = [b"same\0", &[0xff, 0x80, 0], b"same\0"];
        fixture_subtitles(&directory, &subtitles, Some(b"orphan\0"));
        let report = conversation(
            &directory,
            &directory.join("order.json"),
            None,
            input("Base.esm:300"),
        )
        .unwrap();
        let payloads = report["subtitle_payloads"].as_array().unwrap();
        assert_eq!(payloads.len(), 3);
        for (occurrence, (payload, bytes)) in payloads.iter().zip(subtitles).enumerate() {
            assert_eq!(payload["response"], 0);
            assert_eq!(payload["source_response_number"], 0);
            assert_eq!(payload["occurrence"], occurrence);
            assert_eq!(payload["info_field"], occurrence + 2);
            assert_eq!(payload["bytes"], bytes.len());
            assert_eq!(payload["sha256"], format!("{:x}", Sha256::digest(bytes)));
            assert!(payload.get("text").is_none());
        }
        assert_eq!(
            report["conversation"]["info_fields"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"orphan\0"))
        );
        assert_eq!(
            report["conversation"]["info_fields"][0]["owner_section"],
            serde_json::Value::Null
        );
        assert_eq!(report["loaded_fragments"].as_array().unwrap().len(), 1);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(report["conversation"]["voice_filename_verified"], false);
        assert_eq!(report["conversation"]["condition_truth_verified"], false);
    }
    #[test]
    fn cli_consumer_refuses_info_outside_requested_winning_topic() {
        let directory = directory();
        fixture(&directory);
        assert!(
            conversation(
                &directory,
                &directory.join("order.json"),
                None,
                input("Base.esm:100")
            )
            .is_err()
        );
    }

    fn archive(root: &Path, label: &str, folder: &[u8], name: &[u8], payload: &[u8]) {
        // Independently authored one-folder/file BSA104; use the real importer.
        let table = 54 + folder.len();
        let offset = table + 16 + name.len() + 1;
        let mut bytes = vec![0; offset];
        bytes[..4].copy_from_slice(b"BSA\0");
        for (at, word) in [
            (4, 104),
            (8, 36),
            (12, 3),
            (16, 1),
            (20, 1),
            (24, folder.len() as u32 + 1),
            (28, name.len() as u32 + 1),
            (44, 1),
            (48, 52),
        ] {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[52] = folder.len() as u8 + 1;
        bytes[53..53 + folder.len()].copy_from_slice(folder);
        bytes[table..table + 8].copy_from_slice(&1u64.to_le_bytes());
        bytes[table + 8..table + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes[table + 12..table + 16].copy_from_slice(&(offset as u32).to_le_bytes());
        bytes[table + 16..offset - 1].copy_from_slice(name);
        bytes.extend(payload);
        fs::write(root.join(format!("Data/{label}.bsa")), bytes).unwrap();
    }
    fn cell_fixture(root: &Path, model_path: &[u8], texture_path: &[u8]) {
        fs::create_dir(root.join("Data")).unwrap();
        let payload = [
            1u32.to_le_bytes().as_slice(),
            &(texture_path.len() as u32).to_le_bytes(),
            texture_path,
        ]
        .concat();
        let kind = b"BSShaderTextureSet";
        let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        nif.extend(0x14020007u32.to_le_bytes());
        nif.push(1);
        for value in [11u32, 1, 34] {
            nif.extend(value.to_le_bytes());
        }
        nif.extend([0; 3]);
        nif.extend(1u16.to_le_bytes());
        nif.extend((kind.len() as u32).to_le_bytes());
        nif.extend(kind);
        nif.extend(0u16.to_le_bytes());
        nif.extend((payload.len() as u32).to_le_bytes());
        nif.extend([0; 12]);
        nif.extend(payload);
        nif.extend([0; 4]);
        archive(root, "models", b"meshes", b"m.nif", &nif);
        archive(
            root,
            "textures",
            b"textures",
            b"t.dds",
            b"authored-source-texture",
        );
        let reference = record(
            b"REFR",
            0x300,
            &[
                field(b"NAME", &0x400u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        );
        let group = |kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &0x200u32.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(
                    b"STAT",
                    0x400,
                    &field(b"MODL", &[model_path, &[0]].concat()),
                ),
                record(b"CELL", 0x200, &field(b"DATA", &[1])),
                group(6, &group(9, &reference)),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
    }
    fn residency_input() -> ResidencyInput {
        ResidencyInput {
            cell: crate::parse_cell_key("Base.esm:200").unwrap(),
            source_timeout_ms: 10_000,
        }
    }
    fn grid_fixture(root: &Path, duplicate: bool) {
        cell_fixture(root, b"m.nif", b"t.dds");
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let grid = [-18_i32, 0]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        let cell = |id| {
            record(
                b"CELL",
                id,
                &[field(b"DATA", &[0]), field(b"XCLC", &grid)].concat(),
            )
        };
        let mut persistent = record(b"CELL", 0x202, &field(b"DATA", &[0]));
        persistent[8..12].copy_from_slice(&fallout_data::plugin::PERSISTENT.to_le_bytes());
        let mut children = [
            cell(0x200),
            group(
                0x200,
                6,
                &group(
                    0x200,
                    9,
                    &record(
                        b"REFR",
                        0x300,
                        &[
                            field(b"NAME", &0x400_u32.to_le_bytes()),
                            field(b"DATA", &[0; 24]),
                        ]
                        .concat(),
                    ),
                ),
            ),
            persistent,
        ]
        .concat();
        if duplicate {
            children.extend(cell(0x201));
        }
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"STAT", 0x400, &field(b"MODL", b"m.nif\0")),
                record(b"WRLD", 0x100, &[]),
                group(0x100, 1, &children),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(
            root.join("grid-case.json"),
            serde_json::to_vec(&json!({"duplicate":duplicate})).unwrap(),
        )
        .unwrap();
    }
    fn grid_input(grid: [i32; 2]) -> GridInput {
        GridInput {
            world: crate::parse_cell_key("Base.esm:100").unwrap(),
            grid,
            source_timeout_ms: 10_000,
        }
    }
    fn terrain_grid_fixture(root: &Path, mode: &str) {
        grid_fixture(root, false);
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let texture = if mode == "default" { 0_u32 } else { 0x600 };
        let land = record(
            b"LAND",
            0x500,
            &field(
                b"BTXT",
                &[texture.to_le_bytes().as_slice(), &[0; 4]].concat(),
            ),
        );
        let mut lands = land;
        if mode == "duplicate" {
            lands.extend(record(
                b"LAND",
                0x501,
                &field(
                    b"BTXT",
                    &[texture.to_le_bytes().as_slice(), &[0; 4]].concat(),
                ),
            ));
        }
        let path = root.join("Data/Base.esm");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend(group(0x100, 1, &group(0x200, 6, &group(0x200, 9, &lands))));
        bytes.extend(record(
            b"LTEX",
            0x600,
            &field(b"TNAM", &0x700_u32.to_le_bytes()),
        ));
        bytes.extend(record(
            b"TXST",
            0x700,
            &field(
                b"TX00",
                if mode == "missing" {
                    b"missing.dds\0"
                } else {
                    b"t.dds\0"
                },
            ),
        ));
        fs::write(path, bytes).unwrap();
        if mode == "ambiguous" {
            archive(
                root,
                "other-terrain",
                b"textures",
                b"t.dds",
                b"other-source",
            );
        }
        fs::write(
            root.join("terrain-grid-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn cli_terrain_grid_consumes_exact_existing_jobs_and_private_cache_without_surface_admission() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| Sha256::digest(fs::read(directory.join(path)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-terrain-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        for reused in [false, true] {
            let report = grid_terrain(
                &directory,
                &directory.join("order.json"),
                None,
                Some(&cache),
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
            assert_eq!(report["terrain_source_plan"]["root"]["local_id"], 0x200);
            assert_eq!(
                report["terrain_source_plan"]["source_cohort_sha256"],
                report["cell_grid_sources"]["source_cohort_sha256"]
            );
            assert_eq!(
                report["terrain_textures"]["textures"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                report["terrain_textures"]["textures"][0]["sha256"],
                format!("{:x}", Sha256::digest(b"authored-source-texture"))
            );
            assert_eq!(
                report["terrain_textures"]["textures"][0]["cache"]["reused"],
                reused
            );
            assert_eq!(report["captured_sources_available"], true);
            assert_eq!(report["terrain_job_usage"]["outstanding"], 0);
            assert_eq!(report["terrain_job_usage"]["decoded_bytes"], 0);
            assert_eq!(report["surface_prepared"], false);
            assert_eq!(report["current_cell_changed"], false);
            assert_eq!(report["activation_applied"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(report["terrain_textures"]["runtime_ready"], false);
        }
        for (path, sha) in paths.iter().zip(before) {
            assert_eq!(Sha256::digest(fs::read(directory.join(path)).unwrap()), sha);
        }
    }
    #[test]
    fn cli_combined_grid_consumes_one_cell_epoch_then_proves_unload_and_final_release() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|p| Sha256::digest(fs::read(directory.join(p)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-combined-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        for _ in 0..2 {
            let report = grid_cell_residency(
                &directory,
                &directory.join("order.json"),
                None,
                Some(&cache),
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], true);
            assert_eq!(report["terrain_scope_requested"], true);
            assert_eq!(
                report["cell_terrain"]["source_cohort_sha256"],
                report["cell_models"]["source_cohort_sha256"]
            );
            assert_eq!(
                report["cell_terrain"]["root"],
                report["cell_grid_request"]["cell"]
            );
            assert_eq!(report["residency"]["terrain_state"], "Decoded");
            assert_eq!(report["residency"]["completed_models"], 1);
            assert_eq!(report["residency"]["completed_textures"], 1);
            assert_eq!(report["residency"]["completed_terrain_textures"], 1);
            assert_eq!(report["residency"]["outstanding"], 3);
            let payload = &report["terrain_payloads"][0];
            assert_eq!(
                payload["sha256"],
                format!("{:x}", Sha256::digest(b"authored-source-texture"))
            );
            assert_eq!(payload["bytes"], 23);
            assert_eq!(payload["cell_identity"], report["residency"]["identity"]);
            assert_eq!(payload["generation"], report["residency"]["generation"]);
            for field in [
                "terrain_ticket_matches_cell",
                "old_ticket_rejected",
                "old_owner_access_rejected",
                "borrowed_model_access_rejected",
                "borrowed_texture_access_rejected",
                "borrowed_terrain_access_rejected",
                "terrain_lease_retained",
            ] {
                assert_eq!(report["retained_source_lifetime"][field], true, "{field}");
            }
            assert_eq!(report["residency_after_unload"]["stage"], "Unloading");
            for field in [
                "outstanding",
                "pinned_source_bytes",
                "retained_plans",
                "plan_metadata_bytes",
                "mapped_source_bytes",
            ] {
                assert_eq!(
                    report["residency_after_unload"][field], report["residency"][field],
                    "{field}"
                );
                assert_eq!(report["residency_after_release"][field], 0, "{field}");
            }
            assert_eq!(report["residency_after_release"]["stage"], "Unrequested");
            for field in ["dependencies", "collision", "behavior"] {
                assert_eq!(report["residency"][field], "Pending");
            }
            for field in ["simulation_ready", "render_published"] {
                assert_eq!(report["residency"][field], false);
            }
            for field in [
                "surface_prepared",
                "current_cell_changed",
                "activation_applied",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[field], false);
            }
        }
        for (path, sha) in paths.iter().zip(before) {
            assert_eq!(Sha256::digest(fs::read(directory.join(path)).unwrap()), sha);
        }
    }
    #[test]
    fn cli_combined_grid_keeps_source_refusals_and_no_job_selection_failures() {
        for mode in ["missing", "ambiguous", "default", "duplicate"] {
            let directory = directory();
            terrain_grid_fixture(&directory, mode);
            let report = grid_cell_residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false, "{mode}");
            assert_eq!(report["terrain_scope_requested"], true);
            assert_eq!(report["runtime_ready"], false);
            if mode == "duplicate" {
                assert!(report["residency"].is_null());
                assert!(report["cell_terrain"].is_null());
                assert!(
                    report["source_error"]
                        .as_str()
                        .unwrap()
                        .contains("exactly one winning present LAND")
                );
            } else {
                assert_eq!(report["residency"]["terrain_state"], "Unsupported");
                assert_eq!(report["residency"]["dependencies"], "Pending");
                assert_eq!(report["residency"]["simulation_ready"], false);
                assert_eq!(report["residency_after_release"]["stage"], "Unrequested");
                assert_eq!(report["residency_after_release"]["outstanding"], 0);
            }
        }
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let report = grid_cell_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([i32::MIN, i32::MAX]),
        )
        .unwrap();
        assert!(report["residency"].is_null());
        assert!(report["cell_grid_request"].is_null());
        for timeout in [0, 120001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_cell_residency(Path::new("absent"), Path::new("absent"), None, None, input)
                    .is_err()
            );
        }
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-residency-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-18",
            "--grid-y",
            "0",
            "--include-terrain",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::GridResidencySources {
                include_terrain: true,
                grid_x: -18,
                ..
            }
        ));
    }
    #[test]
    fn cli_terrain_grid_retains_missing_ambiguous_default_and_duplicate_land_refusals() {
        for mode in ["missing", "ambiguous", "default", "duplicate"] {
            let directory = directory();
            terrain_grid_fixture(&directory, mode);
            let report = grid_terrain(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false, "{mode}");
            assert_eq!(report["surface_prepared"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
            if mode == "duplicate" {
                assert!(report["terrain_source_plan"].is_null());
                assert!(report["terrain_textures"].is_null());
                assert!(
                    report["source_error"]
                        .as_str()
                        .unwrap()
                        .contains("exactly one winning present LAND")
                );
            } else {
                assert_eq!(
                    report["terrain_textures"]["all_requested_texture_sources_ready"],
                    true
                );
                assert_eq!(
                    report["terrain_textures"]["all_authored_texture_sources_resolved"],
                    false
                );
                let sources = &report["terrain_source_plan"]["texture_sources"];
                assert!(
                    sources["failures"].as_u64().unwrap() > 0
                        || sources["unapplied_default_layers"].as_u64().unwrap() > 0
                );
            }
        }
    }
    #[test]
    fn cli_terrain_grid_missing_selection_and_bad_deadline_never_start_jobs() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let report = grid_terrain(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([i32::MAX, i32::MIN]),
        )
        .unwrap();
        assert!(report["cell_grid_request"].is_null());
        assert!(report["terrain_source_plan"].is_null());
        assert!(report["terrain_textures"].is_null());
        assert_eq!(report["captured_sources_available"], false);
        assert!(report["source_error"].as_str().unwrap().contains("no live"));
        for timeout in [0, 120_001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_terrain(Path::new("absent"), Path::new("absent"), None, None, input).is_err()
            );
        }
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-terrain-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-2147483648",
            "--grid-y",
            "2147483647",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::GridTerrainSources {
                grid_x: i32::MIN,
                grid_y: i32::MAX,
                ..
            }
        ));
    }
    #[test]
    fn cli_grid_consumer_selects_explicit_cell_and_uses_existing_resident_sources() {
        let directory = directory();
        grid_fixture(&directory, false);
        let before = Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap());
        let report = grid_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([-18, 0]),
        )
        .unwrap();
        assert_eq!(report["cell_grid_sources"]["world"]["local_id"], 0x100);
        assert_eq!(
            report["cell_grid_sources"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            report["cell_grid_sources"]["entries"][1]["role"],
            "persistent-group"
        );
        assert_eq!(report["explicit_grid"], json!([-18, 0]));
        assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
        assert_eq!(report["cell_models"]["root"]["local_id"], 0x200);
        assert_eq!(
            report["cell_models"]["source_cohort_sha256"],
            report["cell_grid_sources"]["source_cohort_sha256"]
        );
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["activation_applied"], false);
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap()),
            before
        );
    }
    #[test]
    fn cli_grid_missing_and_ambiguous_requests_preserve_directory_without_residency() {
        for (duplicate, grid, expected) in [
            (false, [i32::MAX, i32::MIN], "no live"),
            (true, [-18, 0], "ambiguous"),
        ] {
            let directory = directory();
            grid_fixture(&directory, duplicate);
            let report = grid_residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input(grid),
            )
            .unwrap();
            assert!(report["source_error"].as_str().unwrap().contains(expected));
            assert!(report["cell_grid_request"].is_null());
            assert!(report["cell_models"].is_null());
            assert!(report["residency"].is_null());
            assert_eq!(report["captured_sources_available"], false);
            assert_eq!(report["explicit_grid"], json!(grid));
            assert_eq!(report["activation_applied"], false);
            assert_eq!(report["current_cell_changed"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(
                report["cell_grid_sources"]["entries"]
                    .as_array()
                    .unwrap()
                    .len(),
                if duplicate { 3 } else { 2 }
            );
        }
    }
    #[test]
    fn cli_grid_flags_preserve_signed_values_and_refuse_out_of_domain_inputs() {
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-residency-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-2147483648",
            "--grid-y",
            "2147483647",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::World(crate::WorldCommand::GridResidencySources {
                grid_x: i32::MIN,
                grid_y: i32::MAX,
                ..
            })
        ));
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "grid-residency-sources",
                "--install",
                "fixture",
                "--load-order",
                "order.json",
                "--world",
                "Base.esm:100",
                "--grid-x",
                "2147483648",
                "--grid-y",
                "0"
            ])
            .is_err()
        );
        for timeout in [0, 120_001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_residency(Path::new("absent"), Path::new("absent"), None, None, input)
                    .is_err()
            );
        }
    }
    fn door_fixture(root: &Path, target: u32) {
        cell_fixture(root, b"m.nif", b"t.dds");
        let group = |cell: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &cell.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let reference = |id: u32, teleport: u32, x: f32| {
            let pose = [x, 2.0, 3.0, 0.0, -0.0, -0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            record(
                b"REFR",
                id,
                &[
                    field(b"NAME", &0x401_u32.to_le_bytes()),
                    field(b"DATA", &[0; 24]),
                    field(
                        b"XTEL",
                        &[
                            teleport.to_le_bytes().as_slice(),
                            &pose,
                            &7_u32.to_le_bytes(),
                        ]
                        .concat(),
                    ),
                ]
                .concat(),
            )
        };
        let path = root.join("Data/Base.esm");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend(record(b"DOOR", 0x401, &field(b"MODL", b"m.nif\0")));
        bytes.extend(group(
            0x200,
            6,
            &group(0x200, 9, &reference(0x310, target, 42.0)),
        ));
        bytes.extend(record(b"CELL", 0x201, &field(b"DATA", &[1])));
        bytes.extend(group(
            0x201,
            6,
            &group(0x201, 9, &reference(0x311, 0x310, 99.0)),
        ));
        fs::write(path, bytes).unwrap();
    }
    fn door_input() -> DoorInput {
        DoorInput {
            source: residency_input(),
            door: crate::parse_cell_key("Base.esm:310").unwrap(),
        }
    }
    #[test]
    fn cli_door_destination_prepares_exact_target_sources_without_moving_current_cell() {
        let directory = directory();
        door_fixture(&directory, 0x311);
        let source_before = Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap());
        let report = door_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            door_input(),
        )
        .unwrap();
        assert_eq!(report["door_destination"]["source_cell"]["local_id"], 0x200);
        assert_eq!(
            report["door_destination"]["destination"]["cell"]["local_id"],
            0x201
        );
        assert_eq!(report["residency"]["root"]["local_id"], 0x201);
        assert_eq!(report["cell_models"]["root"]["local_id"], 0x201);
        assert_eq!(
            report["door_destination"]["destination"]["authored_transform"]["position"][0],
            42.0
        );
        assert_eq!(report["door_destination"]["destination"]["raw_flags"], 7);
        assert_eq!(
            report["door_destination"]["destination"]["authored_transform_words"],
            json!([
                0x4228_0000_u32,
                0x4000_0000,
                0x4040_0000,
                0,
                0x8000_0000_u32,
                0xbf00_0000_u32
            ])
        );
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["destination_applied"], false);
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap()),
            source_before
        );
    }
    #[test]
    fn cli_unresolved_door_emits_link_evidence_without_creating_residency() {
        let directory = directory();
        door_fixture(&directory, 0x999);
        let report = door_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            door_input(),
        )
        .unwrap();
        assert_eq!(
            report["door_destination"]["source_destination_resolved"],
            false
        );
        assert!(
            !report["door_destination"]["issues"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(report["captured_sources_available"], false);
        assert!(report["source_error"].is_string());
        assert!(report["residency"].is_null());
        assert!(report["cell_models"].is_null());
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
    }
    #[test]
    fn cli_residency_consumes_leased_texture_bytes_and_leaves_sources_unchanged() {
        let directory = directory();
        cell_fixture(&directory, b"m.nif", b"t.dds");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|p| Sha256::digest(fs::read(directory.join(p)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        let report = residency(
            &directory,
            &directory.join("order.json"),
            None,
            Some(&cache),
            residency_input(),
        )
        .unwrap();
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(report["residency"]["outstanding"], 2);
        assert_eq!(report["residency"]["texture_state"], "Decoded");
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(fs::read_dir(cache).unwrap().count(), 4);
        for (path, hash) in paths.iter().zip(before) {
            assert_eq!(
                Sha256::digest(fs::read(directory.join(path)).unwrap()),
                hash
            );
        }
    }
    #[test]
    fn cli_residency_retains_missing_absolute_and_ambiguous_texture_refusals() {
        for (path, ambiguous) in [
            (b"missing.dds".as_slice(), false),
            (b"C:\\export\\t.dds", false),
            (b"t.dds", true),
        ] {
            let directory = directory();
            cell_fixture(&directory, b"m.nif", path);
            if ambiguous {
                archive(
                    &directory,
                    "second-texture",
                    b"textures",
                    b"t.dds",
                    b"other-source",
                );
            }
            let report = residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                residency_input(),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false);
            assert_eq!(report["residency"]["texture_state"], "Unsupported");
            assert_eq!(report["cell_textures"]["missing_or_ambiguous"], 1);
            assert_eq!(
                report["cell_textures"]["usages"][0]["raw_path"],
                json!(path)
            );
            assert_eq!(
                report["cell_textures"]["models"][0]["sha256"]
                    .as_str()
                    .unwrap()
                    .len(),
                64
            );
            assert_eq!(report["residency"]["dependencies"], "Pending");
        }
    }
    #[test]
    fn cli_residency_missing_model_and_bad_timeout_never_claim_availability() {
        let directory = directory();
        cell_fixture(&directory, b"missing.nif", b"t.dds");
        let report = residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            residency_input(),
        )
        .unwrap();
        assert_eq!(report["captured_sources_available"], false);
        assert_eq!(report["residency"]["completed_models"], 0);
        assert_eq!(report["residency"]["complete_model_coverage"], false);
        for timeout in [0, 120_001] {
            let mut input = residency_input();
            input.source_timeout_ms = timeout;
            assert!(
                residency(&directory, &directory.join("order.json"), None, None, input).is_err()
            );
        }
    }
}
