//! Read-only requests over protected winning world/content sources.
use super::{Result, inspection_input::Order};
use fallout_data::{
    condition_operands::Signatures,
    identity::FormKey,
    loaded_scripts::{Catalogue, Limits as ScriptLimits},
    store::RecordStore,
    vfs::MountIndex,
    world::{
        cells::CellGridSources,
        conversation::{DialogueSources, Limits},
        doors::DoorDestination,
        preparation::CellModelPlan,
        residency::{CellResidency, Snapshot, Stage, TexturePlan, TextureState},
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
            match sources.prepare_cell(&mut store, &request, assets.mounts(), Default::default()) {
                Ok(plan) => Ok(consume_plan(
                    install,
                    resource_cache,
                    deadline,
                    plan,
                    assets.mounts(),
                )?),
                Err(error) => Err(error.to_string()),
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
    report["scope"] = json!(
        "Explicit WRLD/XCLC source CELL model/texture jobs; persistent groups remain separate, no position/grid inference or runtime activation"
    );
    Ok(report)
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
    let model_receipt = serde_json::to_value(plan.receipt())?;
    let mut owner = CellResidency::new(install, resource_cache, Default::default())?;
    let ticket = owner.request(plan)?;
    let mut error = None;
    let mut textures = None;
    let mut texture_payloads = Vec::new();
    if let Err(failed) = poll_sources(&mut owner, false, deadline) {
        error = Some(failed.to_string());
    } else {
        match TexturePlan::load(owner.sources(&ticket)?, mounts, Default::default()) {
            Ok(plan) => {
                textures = Some(serde_json::to_value(plan.receipt())?);
                owner.request_textures(&ticket, plan)?;
                if let Err(failed) = poll_sources(&mut owner, true, deadline) {
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
                }
            }
            Err(failed) => error = Some(failed.to_string()),
        }
    }
    let snapshot = owner.snapshot();
    let available =
        error.is_none() && snapshot.complete_model_coverage && snapshot.complete_texture_coverage;
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "cell_models":model_receipt,"cell_textures":textures,"texture_payloads":texture_payloads,
        "residency":snapshot,"source_error":error,
        "captured_sources_available":available,"lookup_precedence_verified":false,
        "runtime_ready":false,"retail_parity_accepted":false,
        "scope":"Protected exact CELL/model/external-texture source jobs; no GPU/DDS/physics/behavior admission"}))
}

fn poll_sources(owner: &mut CellResidency, textures: bool, timeout: Duration) -> Result<Snapshot> {
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
        if complete {
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
        "conversation":prepared.metadata(),"loaded_fragments":fragments,
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
        fs::create_dir(root.join("Data")).unwrap();
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
        let info = record(
            b"INFO",
            0x300,
            &[
                field(b"TRDT", &[0; 24]),
                field(b"NAM1", b"authored_fixture_line\0"),
                field(b"SCHR", &schr),
                field(b"SCDA", &[0x1d, 0, 0, 0]),
            ]
            .concat(),
        );
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
            crate::Command::GridResidencySources {
                grid_x: i32::MIN,
                grid_y: i32::MAX,
                ..
            }
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
