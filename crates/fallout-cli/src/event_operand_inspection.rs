//! Source operand associations, plus an explicit opt-in engineering local copy.
use super::{
    Result, command_catalogue, definition_plan_inspection, inspection_input::Order, script_profile,
    script_state_inspection,
};
use fallout_data::{loaded_scripts, obscript, quest_scripts};
use fallout_runtime::{
    event_operands,
    execution::{attachment_boot, copy_probe, local_copy, native, pending_batch},
    foreign::Content,
    identity::{ReferenceId, Value as RuntimeValue},
    preparation, programs,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedQuestBootRequest {
    schema_version: u32,
    quest: fallout_data::identity::FormKey,
    initialization: attachment_boot::Request,
}

pub(super) fn boot_saved_quest(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
) -> Result<Value> {
    let request: SavedQuestBootRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "quest boot request byte budget exceeded",
    )?)?;
    if request.schema_version != 1 {
        return Err("unsupported quest boot request schema".into());
    }
    let parent = result_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    if parent.starts_with(super::protected_tree(install)?) {
        return Err("quest boot result must be outside the installation".into());
    }
    if result_path.try_exists()? {
        return Err("quest boot result must be a fresh artifact".into());
    }
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = obscript::expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        Default::default(),
        |_, _| Ok(()),
    )?);
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let attachments =
        quest_scripts::Attachments::load(&mut store, &catalogue, 1_000_000, |_, _| Ok(()))?;
    let handles: Vec<_> = attachments
        .get(&request.quest)
        .and_then(|attachment| attachment.script.as_ref())
        .cloned()
        .into_iter()
        .collect();
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &handles,
        Default::default(),
    )?;
    let plan = attachment_boot::prepare(
        &sources,
        &attachments,
        &content,
        &request.quest,
        &request.initialization,
        Default::default(),
    )?;
    let limits = fallout_runtime::Limits::default();
    let input_bytes = read_bounded_named(
        snapshot_path,
        limits.max_snapshot_bytes,
        "quest boot snapshot byte budget exceeded",
    )?;
    let snapshot = fallout_runtime::snapshot::Snapshot::decode(&input_bytes, limits)?;
    let result = plan.apply(snapshot, limits)?;
    let bytes = result.snapshot.encode(limits.max_snapshot_bytes)?;
    let cold = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&bytes, limits)?,
        limits,
    )?;
    let owner = fallout_runtime::identity::Owner::Quest {
        key: request.quest.clone(),
    };
    if cold.snapshot() != result.snapshot
        || cold.owner_instance(&owner) != Some(result.instance)
        || cold.instance(cold.handle(result.instance)?)?.definition() != plan.definition()
    {
        return Err("quest boot cold restoration differs from the private result".into());
    }
    let report = json!({"schema_version":1,"scope":"Explicit engineering creation of one source-attached quest script owner; no retail activation",
        "quest":request.quest,"instance":result.instance,"definition":plan.definition(),
        "quest_attachment":plan.attachment(),"script_source":plan.script_version(),
        "source_receipts":attachments.source_receipts(),"source_cohort_sha256":plan.source_cohort_sha256(),
        "initialization_counts":plan.counts(),"prepared_sources":{"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
        "campaign":request.initialization.campaign,"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),
        "snapshot_artifact":{"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":result.snapshot.schema_version},
        "canonical_restore_verified":true,"private_result":true,"event_enqueued":false,
        "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),
        "quest_activation_verified":false,"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    // Validate the complete result and bounded report before creating an output.
    let mut admitted = BoundedJson {
        bytes: Vec::new(),
        maximum: 8 * 1024 * 1024,
    };
    serde_json::to_writer_pretty(&mut admitted, &report)?;
    admitted.write_all(b"\n")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(result_path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(report)
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedIntent {
    Engineering,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedCopyRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    activation: std::num::NonZeroU64,
    intent: SavedIntent,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedBatchRequest {
    schema_version: u32,
    intent: SavedIntent,
    events: Vec<pending_batch::Request>,
    maximum_source_instructions: usize,
    maximum_statement_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}

pub(super) fn copy_saved_batch(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedBatchRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved batch request byte budget exceeded",
    )?)?;
    let defaults = pending_batch::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    if request.schema_version != 1
        || request.events.is_empty()
        || request.events.len() > defaults.maximum_events
        || request.maximum_source_instructions > defaults.maximum_source_instructions
        || request.maximum_statement_bytes > defaults.maximum_statement_bytes
        || request.maximum_trace_source_bytes > defaults.trace_projection.maximum_source_bytes
        || request.maximum_trace_rows > defaults.trace_projection.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.trace_projection.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.trace_projection.maximum_binding_uses
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved batch schema/count/budget ceiling".into());
    }
    let protected = super::protected_tree(install)?;
    let parent = result_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    if parent.starts_with(&protected) {
        return Err("saved batch result must be outside the installation".into());
    }
    if result_path.try_exists()? {
        return Err("saved batch result must be a fresh artifact".into());
    }
    // Both outputs are fresh artifacts. Atomic create_new also refuses a racing
    // file/link, so the report cannot replace an input through a path alias.
    if let Some(report) = report_path {
        if report.try_exists()? {
            return Err("saved batch report must be a fresh artifact".into());
        }
        let resolved = {
            report
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?
                .join(
                    report
                        .file_name()
                        .ok_or("saved batch report requires a filename")?,
                )
        };
        if resolved
            .parent()
            .is_some_and(|parent| parent.starts_with(&protected))
        {
            return Err("saved batch report must be outside the installation".into());
        }
        let result_resolved = parent.join(
            result_path
                .file_name()
                .ok_or("saved batch result requires a filename")?,
        );
        if resolved == result_resolved
            || (cfg!(windows)
                && resolved
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&result_resolved.to_string_lossy()))
        {
            return Err("saved batch report must differ from the result snapshot".into());
        }
    }
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = obscript::expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        Default::default(),
        |_, _| Ok(()),
    )?);
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let input_bytes = read_bounded_named(
        snapshot_path,
        world_limits.max_snapshot_bytes,
        "saved batch snapshot byte budget exceeded",
    )?;
    let snapshot = fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?;
    let world = fallout_runtime::World::restore(Arc::clone(&catalogue), snapshot, world_limits)?;
    let campaign = world.campaign();
    let before_revision = world.revision();
    // Only the existing prefix's distinct exact definitions are prepared. The
    // complete source cohort remains the identity of the selected cache.
    let mut selected = BTreeMap::new();
    for pending in world.pending_events().take(request.events.len()) {
        let handle = world
            .instance(world.handle(pending.instance)?)?
            .definition()
            .clone();
        selected.insert(handle.key.clone(), handle);
    }
    let handles: Vec<_> = selected.into_values().collect();
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &handles,
        Default::default(),
    )?;
    let intent = match request.intent {
        SavedIntent::Engineering => local_copy::Intent::Engineering,
    };
    let outcome = pending_batch::consume(
        world,
        &sources,
        &content,
        &request.events,
        intent,
        pending_batch::Limits {
            maximum_events: defaults.maximum_events,
            maximum_source_instructions: request.maximum_source_instructions,
            maximum_statement_bytes: request.maximum_statement_bytes,
            trace_projection: preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_trace_source_bytes,
                maximum_rows: request.maximum_trace_rows,
                maximum_variable_bytes: request.maximum_trace_variable_bytes,
                maximum_binding_uses: request.maximum_trace_binding_uses,
            },
        },
    )?;
    let mut report = json!({"schema_version":1,"scope":"explicit engineering existing journal prefix","campaign":campaign,
        "before_revision":before_revision,"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),
        "explicit_events":request.events,"intent":intent,"result_snapshot":null,
        "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
        "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),
        "faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    let result_bytes = match outcome {
        pending_batch::Outcome::Unsupported {
            event_index,
            reason,
            detail,
        } => {
            report["snapshot_batch"] = json!({"status":"unsupported","event_index":event_index,"reason":reason,"detail":detail});
            report["after_revision"] = json!(before_revision);
            report["private_result_discarded"] = json!(true);
            None
        }
        pending_batch::Outcome::EngineeringCommitted { result } => {
            let bytes = result
                .snapshot
                .encode(request.maximum_result_snapshot_bytes)?;
            let cold = fallout_runtime::World::restore(
                Arc::clone(&catalogue),
                fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
                world_limits,
            )?;
            if cold.snapshot() != result.snapshot {
                return Err("saved batch result failed canonical restore verification".into());
            }
            report["after_revision"] = json!(result.snapshot.state_revision);
            report["snapshot_batch"] = json!({"status":"engineering_committed","committed":result.committed,"counts":result.counts});
            report["result_snapshot"] = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":result.snapshot.schema_version,"decode_restore_equal":true,"remaining_head":result.snapshot.pending_events.first()});
            report["private_result_discarded"] = json!(false);
            Some(bytes)
        }
    };
    // Admit the exact pretty report plus newline before any result file exists.
    let mut admitted = BoundedJson {
        bytes: Vec::new(),
        maximum: request.maximum_report_bytes,
    };
    serde_json::to_writer_pretty(&mut admitted, &report)
        .map_err(|_| "saved batch report byte budget exceeded")?;
    admitted
        .write_all(b"\n")
        .map_err(|_| "saved batch report byte budget exceeded")?;
    if let Some(bytes) = result_bytes {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(result_path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    Ok(report)
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    read_bounded_named(path, maximum, "saved copy input byte budget exceeded")
}

fn read_bounded_named(path: &Path, maximum: usize, capacity: &'static str) -> Result<Vec<u8>> {
    let file = fallout_data::baseline::open_source(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err(capacity.into());
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(capacity.into());
    }
    Ok(bytes)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedNativeIntent {
    Faithful,
    EngineeringObservation,
}

fn explicit_optional_reference<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<ReferenceId>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNativeSelection {
    occurrence: usize,
    intent: SavedNativeIntent,
    #[serde(deserialize_with = "explicit_optional_reference")]
    supplied_subject: Option<ReferenceId>,
    #[serde(deserialize_with = "explicit_optional_reference")]
    explicit_player: Option<ReferenceId>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNativeRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    calls: Vec<SavedNativeSelection>,
    maximum_contributions: usize,
    maximum_report_bytes: usize,
}

#[derive(serde::Serialize)]
struct SavedNativeRow<'a> {
    occurrence: usize,
    observation: native::Observation<'a>,
}

pub(super) fn observe_saved_native(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
) -> Result<Value> {
    let request: SavedNativeRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        64 * 1024,
        "saved native request byte budget exceeded",
    )?)?;
    if request.schema_version != 1
        || request.calls.is_empty()
        || request.calls.len() > 128
        || request.maximum_contributions > 65_536
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("invalid saved native request schema, selection or budget".into());
    }
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
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let limits = fallout_runtime::Limits::default();
    let input_bytes = read_bounded_named(
        snapshot_path,
        limits.max_snapshot_bytes,
        "saved native snapshot byte budget exceeded",
    )?;
    let snapshot = fallout_runtime::snapshot::Snapshot::decode(&input_bytes, limits)?;
    let world = fallout_runtime::World::restore(Arc::clone(&catalogue), snapshot, limits)?;
    let before = world.snapshot();
    let pending = world
        .pending_events()
        .find(|event| event.sequence == request.sequence.get())
        .ok_or("Saved native sequence is not present in the pending journal")?;
    let definition = world
        .instance(world.handle(pending.instance)?)?
        .definition()
        .clone();
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[definition],
        Default::default(),
    )?;
    let calls = world.prepare_native_calls_with_sources(
        request.sequence.get(),
        &sources,
        Default::default(),
    )?;
    // Validate the complete ordered selection before any host query. Repeated
    // and nonmonotonic physical occurrences retain their requested order.
    for selection in &request.calls {
        calls
            .calls()
            .get(selection.occurrence)
            .ok_or(native::Error::MissingCall(selection.occurrence))?;
    }
    let mut report = json!({"schema_version":1,"scope":"Read-only physical native observations from an explicitly supplied current snapshot",
        "campaign":world.campaign(),"state_revision":world.revision(),"pending_sequence":request.sequence,
        "physical_call_count":calls.calls().len(),"observations":[],
        "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),
            "decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
        "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),
        "input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),
        "canonical_state_unchanged":true,"event_acknowledged":false,
        "faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    let mut base = BoundedJson {
        bytes: Vec::new(),
        maximum: request.maximum_report_bytes,
    };
    serde_json::to_writer(&mut base, &report)?;
    let mut retained = base.bytes.len();
    let mut contributions = 0_usize;
    let mut rows = Vec::with_capacity(request.calls.len());
    for selection in request.calls {
        let intent = match selection.intent {
            SavedNativeIntent::Faithful => native::Intent::Faithful,
            SavedNativeIntent::EngineeringObservation => native::Intent::EngineeringObservation,
        };
        let observation = calls.observe(
            selection.occurrence,
            &content,
            native::Inputs {
                supplied_subject: selection.supplied_subject,
                player: selection.explicit_player,
            },
            intent,
            request.maximum_contributions.saturating_sub(contributions),
        )?;
        if let native::Outcome::EngineeringObservation { trace } = &observation.outcome {
            contributions = contributions
                .checked_add(trace.query.contributions.len())
                .filter(|&count| count <= request.maximum_contributions)
                .ok_or("saved native aggregate contribution budget exceeded")?;
        }
        let separator = usize::from(!rows.is_empty());
        let mut writer = BoundedJson {
            bytes: Vec::new(),
            maximum: request
                .maximum_report_bytes
                .saturating_sub(retained)
                .saturating_sub(separator),
        };
        serde_json::to_writer(
            &mut writer,
            &SavedNativeRow {
                occurrence: selection.occurrence,
                observation,
            },
        )?;
        retained += writer.bytes.len() + separator;
        // Allocate an owned JSON row only after bounded serialization admits it.
        rows.push(serde_json::from_slice::<Value>(&writer.bytes)?);
    }
    if world.snapshot() != before {
        return Err("saved native observation changed canonical state or journal".into());
    }
    report["observations"] = json!(rows);
    // Match the actual emitter's pretty report and trailing newline, including
    // metadata, repeated context, separators and indentation in the one cap.
    let mut final_report = BoundedJson {
        bytes: Vec::new(),
        maximum: request.maximum_report_bytes,
    };
    serde_json::to_writer_pretty(&mut final_report, &report)?;
    final_report.write_all(b"\n")?;
    Ok(report)
}

pub(super) fn copy_saved(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
) -> Result<Value> {
    let request: SavedCopyRequest =
        serde_json::from_slice(&read_bounded(request_path, 16 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("unsupported saved copy request schema".into());
    }
    let intent = match request.intent {
        SavedIntent::Engineering => local_copy::Intent::Engineering,
    };
    let parent = result_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    if parent.starts_with(super::protected_tree(install)?) {
        return Err("saved copy result must be outside the installation".into());
    }
    if result_path.try_exists()? {
        return Err("saved copy result must be a fresh artifact".into());
    }
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
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let limits = fallout_runtime::Limits::default();
    let input_bytes = read_bounded(snapshot_path, limits.max_snapshot_bytes)?;
    let snapshot = fallout_runtime::snapshot::Snapshot::decode(&input_bytes, limits)?;
    let mut world = fallout_runtime::World::restore(Arc::clone(&catalogue), snapshot, limits)?;
    let before = world.snapshot();
    let pending = world
        .pending_events()
        .next()
        .filter(|event| event.sequence == request.sequence.get())
        .ok_or("Saved copy must name the existing pending journal head")?;
    let definition = world
        .instance(world.handle(pending.instance)?)?
        .definition()
        .clone();
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[definition],
        Default::default(),
    )?;
    let outcome = copy_probe::commit_pending(
        &mut world,
        &sources,
        &content,
        request.sequence.get(),
        request.activation,
        intent,
        Default::default(),
    )?;
    let committed = matches!(
        outcome,
        copy_probe::PendingOutcome::EngineeringCommitted { .. }
    );
    let mut artifact = Value::Null;
    if committed {
        let after = world.snapshot();
        let bytes = after.encode(limits.max_snapshot_bytes)?;
        let restored = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, limits)?,
            limits,
        )?;
        if restored.snapshot() != after {
            return Err("saved copy result failed canonical restore verification".into());
        }
        // The only output open is after successful canonical commit. A racing
        // existing file/symlink still refuses; no input/result is replaced.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(result_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        artifact = json!({"path":result_path,"bytes":bytes.len(),
            "sha256":format!("{:x}",Sha256::digest(&bytes)),
            "schema_version":after.schema_version,"decode_restore_equal":true});
    } else if world.snapshot() != before {
        return Err("unsupported saved copy changed canonical state".into());
    }
    Ok(
        json!({"schema_version":1,"snapshot_copy":outcome,"result_snapshot":artifact,
        "input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),
        "campaign":world.campaign(),"before_revision":before.state_revision,"after_revision":world.revision(),
        "explicit_activation":request.activation,"intent":intent,
        "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),
            "decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
        "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),
        "canonical_state_unchanged":!committed,"event_acknowledged":committed,
        "faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NumberInput {
    index: u32,
    bits: u64,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyRequest {
    sequence: u64,
    initial_numbers: Vec<NumberInput>,
}
impl CopyRequest {
    fn read(path: &Path) -> Result<Self> {
        const MAXIMUM_BYTES: u64 = 64 * 1024;
        let file = fallout_data::baseline::open_source(path)?;
        if file.metadata()?.len() > MAXIMUM_BYTES {
            return Err("Engineering local-copy input byte budget exceeded".into());
        }
        let mut bytes = Vec::new();
        file.take(MAXIMUM_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAXIMUM_BYTES {
            return Err("Engineering local-copy input byte budget exceeded".into());
        }
        Self::decode(&bytes)
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        let request: Self = serde_json::from_slice(bytes)?;
        if request.initial_numbers.len() > 64 {
            return Err("Engineering local-copy initializer count budget exceeded".into());
        }
        Ok(request)
    }
}

fn engineering_copy(
    world: &mut fallout_runtime::World<'_>,
    sources: &programs::PreparedSources<'_>,
    content: &Content,
    request: CopyRequest,
) -> Result<Value> {
    let pending = world
        .pending_events()
        .next()
        .filter(|event| event.sequence == request.sequence)
        .ok_or("Engineering local copy must name the first pending event")?;
    let handle = world.handle(pending.instance)?;
    let inputs: Vec<_> = request
        .initial_numbers
        .into_iter()
        .map(|input| (input.index, RuntimeValue::Number { bits: input.bits }))
        .collect();
    // Explicit fixture initialization uses runtime's normal typed validation;
    // these inputs are never inferred from source or retail defaults.
    if !inputs.is_empty() {
        world.assign(handle, &inputs)?;
    }
    let initialized = world.snapshot();
    let stage = world.stage_source_local_copy_with_sources(
        request.sequence,
        sources,
        content,
        local_copy::Intent::Engineering,
        Default::default(),
    )?;
    if world.snapshot() != initialized {
        return Err("Local-copy staging changed canonical state".into());
    }
    match stage {
        local_copy::Preparation::Unsupported { reason, detail } => Ok(json!({
            "status":"unsupported", "reason":reason, "detail":detail,
            "explicit_initial_numbers":inputs,"staging_changed_state":false,
            "event_acknowledged":false,"original_behavior_verified":false
        })),
        local_copy::Preparation::Staged(stage) => {
            let mut expected = initialized.clone();
            let instance = expected
                .instances
                .iter_mut()
                .find(|instance| instance.id == stage.changes().instance())
                .ok_or("Staged local-copy instance missing")?;
            instance
                .locals
                .iter_mut()
                .find(|local| local.index == stage.trace().destination_index)
                .ok_or("Staged local-copy destination missing")?
                .value = stage.trace().copied_value.clone();
            expected.state_revision = expected
                .state_revision
                .checked_add(1)
                .ok_or("Revision exhausted")?;
            expected.pending_events.remove(0);
            let committed = (*stage).commit(world)?;
            let after = world.snapshot();
            if after != expected {
                return Err(
                    "Local-copy commit differs from the single staged assignment/head change"
                        .into(),
                );
            }
            let limits = fallout_runtime::Limits::default();
            let before_bytes = initialized.encode(limits.max_snapshot_bytes)?;
            let after_bytes = after.encode(limits.max_snapshot_bytes)?;
            let restored = fallout_runtime::World::restore(
                world.catalogue(),
                fallout_runtime::snapshot::Snapshot::decode(&after_bytes, limits)?,
                limits,
            )?;
            if restored.snapshot() != after || restored.instance(handle).is_ok() {
                return Err("Local-copy restore differs or accepted old transient handle".into());
            }
            Ok(
                json!({"status":"engineering_committed", "committed":committed,
                "explicit_initial_numbers":inputs,"staging_changed_state":false,"only_staged_changes":true,
                "initialized_snapshot_sha256":format!("{:x}",Sha256::digest(before_bytes)),
                "committed_snapshot_sha256":format!("{:x}",Sha256::digest(after_bytes)),
                "same_process_decode_restore_equal":true,"old_handle_rejected":true,
                "event_acknowledged":true,"original_behavior_verified":false}),
            )
        }
    }
}

struct BoundedJson {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other(
                "native observation report-byte budget exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn finding(error: event_operands::Error) -> Value {
    match error {
        event_operands::Error::Preparation(preparation::Error::Source(error)) => {
            definition_plan_inspection::finding(error)
        }
        event_operands::Error::Preparation(preparation::Error::CachedSource(
            programs::LookupError::Source(error),
        )) => definition_plan_inspection::finding(error.as_ref()),
        other => json!({"kind":"event_operand_probe","reason":other.to_string()}),
    }
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    player_id: Option<u64>,
    prepare_sources: bool,
    native_capabilities: bool,
    engineering_local_copy: Option<&Path>,
) -> Result<Value> {
    let copy_request = engineering_local_copy.map(CopyRequest::read).transpose()?;
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
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let sources = (prepare_sources || native_capabilities || copy_request.is_some())
        .then(|| {
            programs::PreparedSources::load(&catalogue, &model, &signatures, Default::default())
        })
        .transpose()?;
    let seed = script_state_inspection::engineering_world(&catalogue)?;
    let before = seed.world.snapshot();
    let mut world: fallout_runtime::World<'static> = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        before.clone(),
        fallout_runtime::Limits::default(),
    )?;
    drop(seed);
    let player = player_id
        .map(|id| -> Result<_> {
            let reference = ReferenceId(id.try_into()?);
            world.reference_origin(reference)?;
            Ok(reference)
        })
        .transpose()?;
    let mut rows = Vec::new();
    let mut source_counts = BTreeMap::<String, usize>::new();
    let mut operand_counts = BTreeMap::<String, usize>::new();
    let mut attempted_source_bytes = 0;
    let mut operand_uses = 0;
    let mut retained_report_bytes = 0;
    let mut probes = 0;
    let mut unresolved_operands = 0;
    let mut native_calls = 0;
    let mut native_argument_bytes = 0;
    let mut native_unsupported = 0;
    let mut native_counts = BTreeMap::<String, usize>::new();
    for pending in world.pending_events() {
        if rows.len() >= 65_536 {
            return Err("event operand row budget exceeded".into());
        }
        let instance = world.instance(world.handle(pending.instance)?)?;
        let source_bytes = catalogue
            .get_handle(instance.definition())
            .ok_or("Pending instance source has changed")?
            .compiled()
            .map_or(0, <[u8]>::len);
        // This historical field counts source bytes per pending event. Prepared
        // mode reports its actual once-per-definition work separately below.
        if source_bytes > (66_usize * 1024 * 1024).saturating_sub(attempted_source_bytes) {
            return Err("attempted operand source-byte budget exceeded".into());
        }
        attempted_source_bytes += source_bytes;
        let result = match &sources {
            Some(sources) => world.probe_event_operands_with_sources(
                pending.sequence,
                sources,
                &content,
                player,
                Default::default(),
            ),
            None => world.probe_event_operands(
                pending.sequence,
                &model,
                &signatures,
                &content,
                player,
                event_operands::Limits::default(),
            ),
        };
        let (probe, issue) = match result {
            Ok(probe) => {
                if probe.operands.len() > 2_000_000_usize.saturating_sub(operand_uses) {
                    return Err("event operand-use budget exceeded".into());
                }
                operand_uses += probe.operands.len();
                for operand in &probe.operands {
                    let status = match &operand.outcome {
                        event_operands::Outcome::Resolved { access, .. } => match access {
                            event_operands::Access::Read => "resolved_read",
                            event_operands::Access::Destination => "resolved_destination",
                            event_operands::Access::Reference => "resolved_reference",
                        },
                        event_operands::Outcome::Unresolved { code, .. } => {
                            unresolved_operands += 1;
                            code.as_str()
                        }
                    };
                    *operand_counts.entry(status.into()).or_default() += 1;
                }
                probes += 1;
                *source_counts
                    .entry("prepared_operand_probe".into())
                    .or_default() += 1;
                (Some(serde_json::to_value(probe)?), None)
            }
            Err(error) => {
                let issue = finding(error);
                *source_counts
                    .entry(
                        issue["kind"]
                            .as_str()
                            .ok_or("Missing probe finding kind")?
                            .into(),
                    )
                    .or_default() += 1;
                (None, Some(issue))
            }
        };
        let mut row = json!({"pending":pending,"definition":instance.definition(),"probe":probe,"finding":issue});
        if native_capabilities {
            let base_row_bytes = serde_json::to_vec(&row)?.len();
            let mut observation_bytes = 0;
            let source = sources
                .as_ref()
                .ok_or("Native capability inspection needs prepared sources")?;
            match world.prepare_native_calls_with_sources(
                pending.sequence,
                source,
                native::Limits {
                    maximum_calls: native::Limits::default()
                        .maximum_calls
                        .min(2_000_000_usize.saturating_sub(native_calls)),
                    maximum_argument_bytes: (16_usize * 1024 * 1024)
                        .saturating_sub(native_argument_bytes),
                    ..Default::default()
                },
            ) {
                Ok(calls) => {
                    let mut observations = Vec::new();
                    for (index, call) in calls.calls().iter().enumerate() {
                        native_calls += 1;
                        native_argument_bytes += call.raw_arguments.len();
                        // Inspection inventories physical calls, including calls
                        // in branches. It never chooses an execution path.
                        let observed = calls.observe(
                            index,
                            &content,
                            native::Inputs {
                                supplied_subject: None,
                                player,
                            },
                            native::Intent::Faithful,
                            0,
                        )?;
                        if let native::Outcome::Unsupported { reason, .. } = &observed.outcome {
                            native_unsupported += 1;
                            let code = serde_json::to_value(reason)?;
                            *native_counts
                                .entry(code.as_str().ok_or("Missing native reason")?.into())
                                .or_default() += 1;
                        }
                        // Bound serialized work before constructing an owned
                        // JSON observation, including repeated context payloads.
                        let mut writer = BoundedJson {
                            bytes: Vec::new(),
                            maximum: (128_usize * 1024 * 1024)
                                .saturating_sub(retained_report_bytes)
                                .saturating_sub(base_row_bytes)
                                .saturating_sub(observation_bytes),
                        };
                        serde_json::to_writer(&mut writer, &observed)?;
                        observation_bytes += writer.bytes.len();
                        observations.push(serde_json::from_slice::<Value>(&writer.bytes)?);
                    }
                    row["native_capabilities"] =
                        json!({"observations":observations,"finding":null});
                }
                Err(native::Error::Preparation(error)) => {
                    row["native_capabilities"] = json!({"observations":null,"finding":finding(event_operands::Error::Preparation(error))});
                }
                Err(error) => return Err(error.into()),
            }
        }
        let bytes = serde_json::to_vec(&row)?.len();
        if bytes > (128_usize * 1024 * 1024).saturating_sub(retained_report_bytes) {
            return Err("retained operand report-byte budget exceeded".into());
        }
        retained_report_bytes += bytes;
        rows.push(row);
    }
    if world.snapshot() != before {
        return Err("Operand probing changed state or consumed its pending journal".into());
    }
    let snapshot = before.encode(fallout_runtime::Limits::default().max_snapshot_bytes)?;
    let copy_report = copy_request
        .map(|request| {
            engineering_copy(
                &mut world,
                sources
                    .as_ref()
                    .ok_or("Engineering copy needs prepared sources")?,
                &content,
                request,
            )
        })
        .transpose()?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact source/live storage associations over explicit engineering pending events; no native argument or caller readiness",
        "sources":catalogue.sources,"catalogue_sha256":world.catalogue_fingerprint(),
        "executable_source_sha256":descriptors.source_sha256,"context_content":content.report(),"explicit_player":player,
        "pending_events_checked":rows.len(),"prepared_probes":probes,"operand_uses":operand_uses,"unresolved_operands":unresolved_operands,
        "attempted_source_bytes":attempted_source_bytes,"retained_report_bytes":retained_report_bytes,
        "source_counts":source_counts,"operand_counts":operand_counts,"events":rows,
        "snapshot_sha256":format!("{:x}",Sha256::digest(snapshot)),"canonical_state_unchanged":true,
        "index_cache":store.index_cache_report(),"native_readiness_accepted":false,"bytecode_executed":false,
        "retail_parity_accepted":false,"accepted_scenarios":[]});
    if let Some(sources) = sources {
        report["schema_version"] = json!(2);
        report["prepared_sources"] = json!({
            "source_cohort_sha256": sources.source_cohort_sha256(),
            "decoder_sha256": sources.decoder_sha256(),
            "counts": sources.counts(),
            "scope": "Immutable source admission only; live operand outcomes are resolved for each pending event"
        });
    }
    if native_capabilities {
        report["schema_version"] = json!(3);
        report["native_intent"] = json!(native::Intent::Faithful);
        report["native_calls"] = json!(native_calls);
        report["native_argument_bytes"] = json!(native_argument_bytes);
        report["native_unsupported"] = json!(native_unsupported);
        report["native_counts"] = json!(native_counts);
        report["native_scope"] = json!(
            "Physical source-call capability inventory, including branch-contained calls; no native execution path or retail semantic support"
        );
    }
    if let Some(copy_report) = copy_report {
        let copy_bytes = serde_json::to_vec(&copy_report)?.len();
        if copy_bytes > (128_usize * 1024 * 1024).saturating_sub(retained_report_bytes) {
            return Err("Engineering local-copy report-byte budget exceeded".into());
        }
        report["schema_version"] = json!(4);
        report["engineering_report_bytes"] = json!(copy_bytes);
        report["canonical_state_unchanged"] = json!(world.snapshot() == before);
        report["engineering_local_copy"] = copy_report;
        report["engineering_scope"] = json!(
            "Explicit host initialization and one source-bound own-local Number bit copy with canonical staged head acknowledgment; original bytecode conversion/scheduling unverified"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_request_requires_explicit_numeric_bits_and_known_fields() {
        let request = CopyRequest::decode(
            br#"{"sequence":1,"initial_numbers":[{"index":1,"bits":18446744073709551615}]}"#,
        )
        .unwrap();
        assert_eq!(request.initial_numbers[0].bits, u64::MAX);
        for bytes in [
            br#"{"sequence":1}"#.as_slice(),
            br#"{"sequence":1,"initial_numbers":[],"acknowledge":false}"#,
            br#"{"sequence":1,"initial_numbers":[{"index":1,"bits":-1}]}"#,
            br#"{"sequence":1,"initial_numbers":[{"index":1,"value":0}]}"#,
        ] {
            assert!(CopyRequest::decode(bytes).is_err());
        }
    }

    #[test]
    fn copy_request_initializer_count_is_bounded() {
        for count in [64, 65] {
            let bytes = serde_json::to_vec(
                &json!({"sequence":1,"initial_numbers":vec![json!({"index":1,"bits":0});count]}),
            )
            .unwrap();
            let result = CopyRequest::decode(&bytes);
            if count == 64 {
                assert!(result.is_ok());
            } else {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("initializer count budget exceeded")
                );
            }
        }
    }

    #[test]
    fn native_observation_json_admission_counts_utf8_and_escaped_bytes() {
        let value = json!({"arguments":["é", "\\", "\u{0}"]});
        let expected = "{\"arguments\":[\"é\",\"\\\\\",\"\\u0000\"]}".as_bytes();
        let mut exact = BoundedJson {
            bytes: Vec::new(),
            maximum: expected.len(),
        };
        serde_json::to_writer(&mut exact, &value).unwrap();
        assert_eq!(exact.bytes, expected);
        let mut short = BoundedJson {
            bytes: Vec::new(),
            maximum: expected.len() - 1,
        };
        let error = serde_json::to_writer(&mut short, &value).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native observation report-byte budget exceeded")
        );
        assert!(short.bytes.len() < expected.len());
    }
}
