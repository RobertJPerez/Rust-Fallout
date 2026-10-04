//! Prepare explicit journal entries against their live instances and sources.
use super::{
    Result, command_catalogue, definition_plan_inspection, inspection_input::Order, protected_tree,
    script_profile, script_state_inspection,
};
use fallout_data::{baseline, loaded_scripts, obscript, obscript_census};
use fallout_runtime::{events::Trigger, preparation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::Path,
    sync::Arc,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationRequest {
    schema_version: u32,
    sequence: u64,
    selected_locals: Vec<u32>,
    maximum_source_bytes: usize,
    maximum_rows: usize,
    maximum_variable_bytes: usize,
    maximum_binding_uses: usize,
}
fn bounded_input(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err("owned event input byte budget exceeded".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("owned event input byte budget exceeded".into());
    }
    Ok(bytes)
}
fn owned_observation(
    catalogue: Arc<loaded_scripts::Catalogue>,
    model: &obscript::expression_plan::Model<'_>,
    signatures: &obscript::argument_census::Signatures,
    request_path: &Path,
    snapshot_path: &Path,
) -> Result<Value> {
    let request: ObservationRequest =
        serde_json::from_slice(&bounded_input(request_path, 16 * 1024)?)?;
    if request.schema_version != 1
        || request.sequence == 0
        || request.selected_locals.len() > 4096
        || request.maximum_source_bytes > 1024 * 1024
        || request.maximum_rows > 65_536
        || request.maximum_variable_bytes > 1024 * 1024
        || request.maximum_binding_uses > 262_144
    {
        return Err("invalid owned event request schema, sequence, selection or budget".into());
    }
    let limits = fallout_runtime::Limits::default();
    let input = bounded_input(snapshot_path, limits.max_snapshot_bytes)?;
    let world: fallout_runtime::World<'static> = fallout_runtime::World::restore(
        catalogue,
        fallout_runtime::snapshot::Snapshot::decode(&input, limits)?,
        limits,
    )?;
    let before = world.snapshot().encode(limits.max_snapshot_bytes)?;
    let observation = {
        let frame = world.prepare_event(
            request.sequence,
            model,
            signatures,
            preparation::Limits::default(),
        )?;
        preparation::EventObservation::capture(
            &frame,
            &request.selected_locals,
            preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_source_bytes,
                maximum_rows: request.maximum_rows,
                maximum_variable_bytes: request.maximum_variable_bytes,
                maximum_binding_uses: request.maximum_binding_uses,
            },
        )?
    };
    if world.snapshot().encode(limits.max_snapshot_bytes)? != before {
        return Err("Owned event capture changed canonical state".into());
    }
    // The consumer owns only its selected projection, not a World/catalogue
    // borrow or a complete saved continuation. It reads after the actual drop.
    drop(world);
    let observed = std::thread::spawn(move || serde_json::to_value(observation))
        .join()
        .map_err(|_| "Owned event consumer thread panicked")??;
    Ok(
        json!({"schema_version":2,"scope":"Historical owned source frame and explicitly selected canonical locals; no execution permission",
        "observation":observed,"consumer":{"thread":true,"world_dropped_before_read":true},
        "canonical_state_unchanged":true,"snapshot_sha256":format!("{:x}",Sha256::digest(before)),
        "bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}

fn finding(error: preparation::Error) -> Value {
    match error {
        preparation::Error::Source(error) => definition_plan_inspection::finding(error),
        other => json!({"kind":"live_event_preparation","reason":other.to_string()}),
    }
}
pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    bundle_path: Option<&Path>,
    observation_request: Option<&Path>,
    snapshot_input: Option<&Path>,
) -> Result<Value> {
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = obscript::expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )?);
    if let Some(request) = observation_request {
        return owned_observation(
            catalogue,
            &model,
            &signatures,
            request,
            snapshot_input.ok_or("Owned event observation requires a saved snapshot")?,
        );
    }
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("event frame bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FROBS001")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let seed = script_state_inspection::engineering_world(&catalogue)?;
    let before = seed.world.snapshot();
    let world: fallout_runtime::World<'static> = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        before.clone(),
        fallout_runtime::Limits::default(),
    )?;
    drop(seed);
    let mut rows = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut frame_bytes = 8;
    let mut prepared_frames = 0;
    let mut instructions = 0;
    let mut plan_instructions = 0;
    let mut tokens = 0;
    let mut nodes = 0;
    let mut operand_uses = 0;
    let mut attempted_source_bytes = 0;
    for pending in world.pending_events() {
        if rows.len() >= 65_536 {
            return Err("pending frame count budget exceeded".into());
        }
        let instance = world.instance(world.handle(pending.instance)?)?;
        let source_bytes = catalogue
            .get_handle(instance.definition())
            .ok_or("Pending instance source has changed")?
            .compiled()
            .map_or(0, <[u8]>::len);
        // Charge the input before preparation, including attempts that leave a
        // finding. Repeated failures cannot bypass the inspection work budget.
        if source_bytes > (66_usize * 1024 * 1024).saturating_sub(attempted_source_bytes) {
            return Err("attempted event source-byte budget exceeded".into());
        }
        attempted_source_bytes += source_bytes;
        let (prepared, issue) = match world.prepare_event(
            pending.sequence,
            &model,
            &signatures,
            preparation::Limits::default(),
        ) {
            Ok(frame) => {
                let selected = frame.selected();
                let framed = frame.instructions();
                let start = framed
                    .first()
                    .ok_or("Prepared event has no begin header")?
                    .bytes
                    .start;
                let end = framed
                    .last()
                    .ok_or("Prepared event has no end header")?
                    .bytes
                    .end;
                let bytes = &frame.source().control().bytes()[start..end];
                let headers = obscript::decode(bytes, obscript::Limits::default())?;
                if headers.instructions.len() != framed.len() {
                    return Err("Event window framing differs from its source slice".into());
                }
                instructions += framed.len();
                plan_instructions += frame.source().control().instructions().len();
                tokens += frame.source().tokens();
                nodes += frame.source().nodes();
                operand_uses += frame.source().bindings().uses.len();
                frame_bytes += 4 + bytes.len();
                if instructions > 2_000_000
                    || plan_instructions > 2_000_000
                    || tokens > 2_000_000
                    || nodes > 2_000_000
                    || operand_uses > 2_000_000
                    || frame_bytes > 66 * 1024 * 1024
                {
                    return Err("prepared event aggregate budget exceeded".into());
                }
                if let Some(file) = &mut bundle {
                    file.write_all(&(bytes.len() as u32).to_le_bytes())?;
                    file.write_all(bytes)?;
                }
                let index = prepared_frames;
                prepared_frames += 1;
                *counts.entry("prepared_source_frame".into()).or_default() += 1;
                let calls = headers
                    .instructions
                    .iter()
                    .filter(|i| i.calling_reference.is_some())
                    .count();
                let events = headers
                    .instructions
                    .iter()
                    .filter(|i| i.event.is_some())
                    .count();
                (
                    Some(
                        json!({"selected":selected,"begin_scda_offset":start,"end_scda_offset":end,
                    "frame_bundle_index":index,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes)),
                    "framing_sha256":obscript_census::framing_digest(&headers),"instructions":framed.len(),
                    "reference_calls":calls,"event_blocks":events,
                    "binding_sha256":obscript::operand_binding::digest(&frame.source().bindings().uses)}),
                    ),
                    None,
                )
            }
            Err(error) => {
                let issue = finding(error);
                *counts
                    .entry(
                        issue["kind"]
                            .as_str()
                            .ok_or("Missing preparation finding kind")?
                            .to_owned(),
                    )
                    .or_default() += 1;
                (None, Some(issue))
            }
        };
        if !matches!(pending.trigger, Trigger::Block { .. }) {
            return Err("Engineering seed unexpectedly includes object-event observations".into());
        }
        rows.push(json!({"pending":pending,"definition":instance.definition(),"prepared":prepared,"finding":issue}));
    }
    if world.snapshot() != before {
        return Err("Event preparation mutated canonical state or consumed its journal".into());
    }
    let bundle_receipt = if let Some(mut file) = bundle {
        file.flush()?;
        file.get_ref().sync_all()?;
        drop(file);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("created bundle path"))?;
        Some(json!({"format":"FROBS001","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    let bytes = before.encode(fallout_runtime::Limits::default().max_snapshot_bytes)?;
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Bounded source frames over explicit engineering pending events and live instances; preparation grants no execution permission",
        "sources":catalogue.sources,"catalogue_sha256":world.catalogue_fingerprint(),"executable_source_sha256":descriptors.source_sha256,
        "pending_events_checked":rows.len(),"prepared_frames":prepared_frames,"prepared_instructions":instructions,
        "attempted_source_bytes":attempted_source_bytes,"prepared_operand_uses":operand_uses,
        "counts":counts,"events":rows,"comparison_bundle":bundle_receipt,
        "canonical_state_unchanged":true,"snapshot_sha256":format!("{:x}",Sha256::digest(bytes)),
        "index_cache":store.index_cache_report(),"bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}))
}
