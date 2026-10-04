//! Source operand associations, plus an explicit opt-in engineering local copy.
use super::{
    Result, command_catalogue, definition_plan_inspection, inspection_input::Order, script_profile,
    script_state_inspection,
};
use fallout_data::{loaded_scripts, obscript, quest_scripts, script_reference_attachment};
use fallout_runtime::{
    event_operands,
    execution::{
        attachment_boot, copy_probe, event_request, foreign_copy, literal_assignment, local_copy,
        native, native_assignment, native_plan, pending_batch, reference_attachment_boot,
        reference_copy,
    },
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

#[derive(serde::Deserialize)]
#[serde(remote = "script_reference_attachment::Limits", deny_unknown_fields)]
struct ReferenceSourceLimits {
    maximum_sources: usize,
    maximum_source_bytes: u64,
    maximum_header_visits: usize,
    maximum_catalogue_scripts: usize,
    maximum_variable_bytes: usize,
    maximum_record_bytes: usize,
    maximum_read_bytes: usize,
    maximum_field_visits: usize,
}
#[derive(serde::Deserialize)]
#[serde(remote = "attachment_boot::Limits", deny_unknown_fields)]
struct ReferenceInitializationLimits {
    maximum_initializers: usize,
    maximum_context_arguments: usize,
    maximum_variable_bytes: usize,
    maximum_source_receipt_bytes: usize,
    maximum_declarations: usize,
}
fn explicit_optional_reference_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<fallout_runtime::identity::ReferenceValue>, D::Error> {
    <Option<fallout_runtime::identity::ReferenceValue> as serde::Deserialize>::deserialize(d)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceBootContext {
    #[serde(deserialize_with = "explicit_optional_reference")]
    calling_reference: Option<ReferenceId>,
    #[serde(deserialize_with = "explicit_optional_reference")]
    containing_reference: Option<ReferenceId>,
    #[serde(deserialize_with = "explicit_optional_reference_value")]
    target: Option<fallout_runtime::identity::ReferenceValue>,
    arguments: Vec<fallout_runtime::identity::ReferenceValue>,
}
impl ReferenceBootContext {
    fn into_context(self) -> fallout_runtime::events::Context {
        fallout_runtime::events::Context {
            calling_reference: self.calling_reference,
            containing_reference: self.containing_reference,
            target: self.target,
            arguments: self.arguments,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedEventRequest {
    schema_version: u32,
    instance: fallout_runtime::identity::InstanceId,
    owner: fallout_runtime::identity::Owner,
    definition: loaded_scripts::Handle,
    begin_scda_offset: u32,
    event_id: u16,
    intent: SavedForeignIntent,
    context: ReferenceBootContext,
    maximum_source_bytes: usize,
    maximum_source_instructions: usize,
    maximum_context_arguments: usize,
    maximum_variable_bytes: usize,
    maximum_source_receipts: usize,
    maximum_trace_bytes: usize,
    maximum_prepared_instructions: usize,
    maximum_prepared_operand_uses: usize,
    maximum_prepared_tokens: usize,
    maximum_prepared_record_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SavedEventOutcome<'a> {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    EngineeringEnqueued {
        sequence: u64,
        trace: &'a event_request::Trace<'a>,
        queued_event: &'a fallout_runtime::events::Pending,
    },
}
#[derive(serde::Serialize)]
struct SavedEventReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    snapshot_event_request: SavedEventOutcome<'a>,
}
pub(super) fn enqueue_saved_event(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedEventRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved event request byte budget exceeded",
    )?)?;
    let defaults = event_request::Limits::default();
    let p = programs::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_bytes > defaults.maximum_source_bytes
        || request.maximum_source_instructions > defaults.maximum_source_instructions
        || request.maximum_context_arguments > defaults.maximum_context_arguments
        || request.maximum_variable_bytes > defaults.maximum_variable_bytes
        || request.maximum_source_receipts > defaults.maximum_source_receipts
        || request.maximum_trace_bytes > defaults.maximum_trace_bytes
        || request.maximum_prepared_instructions > p.maximum_instructions
        || request.maximum_prepared_operand_uses > p.maximum_uses
        || request.maximum_prepared_tokens > p.maximum_tokens
        || request.maximum_prepared_record_bytes > p.maximum_attempted_record_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved event request schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(install, result_path, report_path, "saved event request")?;
    let context = request.context.into_context();
    if context.arguments.len() > request.maximum_context_arguments {
        return Err("saved event context argument budget exceeded".into());
    }
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = obscript::expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    if order.names.len() > request.maximum_source_receipts {
        return Err("saved event source receipt budget exceeded".into());
    }
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
        "saved event snapshot byte budget exceeded",
    )?;
    let world = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?,
        world_limits,
    )?;
    let instance = world.instance(world.handle(request.instance)?)?;
    if instance.owner() != &request.owner || instance.definition() != &request.definition {
        return Err(
            "saved event explicit owner or definition differs from current instance".into(),
        );
    }
    let script = catalogue
        .get_handle(&request.definition)
        .ok_or("saved event source definition is missing")?;
    if script.compiled().map_or(0, |bytes| bytes.len()) > request.maximum_source_bytes {
        return Err("saved event source byte budget exceeded".into());
    }
    if request
        .definition
        .key
        .record
        .origin_plugin
        .len()
        .checked_add(request.definition.version_sha256.len())
        .is_none_or(|bytes| bytes > request.maximum_variable_bytes)
    {
        return Err("saved event variable byte budget exceeded before source preparation".into());
    }
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        std::slice::from_ref(&request.definition),
        programs::Limits {
            maximum_attempted_record_bytes: request.maximum_prepared_record_bytes,
            maximum_attempted_bytes: request.maximum_source_bytes,
            maximum_instructions: request.maximum_prepared_instructions,
            maximum_uses: request.maximum_prepared_operand_uses,
            maximum_expressions: request.maximum_prepared_tokens.min(p.maximum_expressions),
            maximum_tokens: request.maximum_prepared_tokens,
            maximum_nodes: request.maximum_prepared_tokens,
            ..p
        },
    )?;
    let campaign = world.campaign();
    let before_revision = world.revision();
    let clocks = world.clocks();
    let existing_pending = world.pending_events().len();
    let prepared = event_request::prepare(
        &world,
        &sources,
        &content,
        event_request::Selection {
            instance: request.instance,
            expected_owner: &request.owner,
            definition: &request.definition,
            begin_scda_offset: request.begin_scda_offset,
            event_id: request.event_id,
            intent: match request.intent {
                SavedForeignIntent::Engineering => local_copy::Intent::Engineering,
                SavedForeignIntent::Faithful => local_copy::Intent::Faithful,
            },
        },
        &context,
        event_request::Limits {
            maximum_source_bytes: request.maximum_source_bytes,
            maximum_source_instructions: request.maximum_source_instructions,
            maximum_context_arguments: request.maximum_context_arguments,
            maximum_variable_bytes: request.maximum_variable_bytes,
            maximum_source_receipts: request.maximum_source_receipts,
            maximum_trace_bytes: request.maximum_trace_bytes,
        },
    )?;
    let (queued, unsupported) = match &prepared {
        event_request::Preparation::Unsupported { reason, detail } => {
            (None, Some((*reason, *detail)))
        }
        event_request::Preparation::Ready(operation) => {
            // The admitted World is read-only; apply consumes its complete
            // current snapshot into a separately restored private result.
            let snapshot = world.snapshot();
            drop(world);
            (Some(operation.apply(snapshot, world_limits)?), None)
        }
    };
    let mut artifact = Value::Null;
    let result_bytes = if let Some(result) = &queued {
        let bytes = result
            .snapshot
            .encode(request.maximum_result_snapshot_bytes)?;
        let cold = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
            world_limits,
        )?;
        if cold.snapshot() != result.snapshot
            || cold.instance(cold.handle(request.instance)?)?.owner() != &request.owner
            || cold.instance(cold.handle(request.instance)?)?.definition() != &request.definition
            || cold.clocks() != clocks
            || cold.pending_events().len() != existing_pending + 1
        {
            return Err("saved event complete cold result differs".into());
        }
        artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":result.snapshot.schema_version,"decode_restore_equal":true,"after_revision":result.snapshot.state_revision,"pending_events":result.snapshot.pending_events.len()});
        Some(bytes)
    } else {
        None
    };
    let outcome = match (&prepared, &queued, &unsupported) {
        (event_request::Preparation::Ready(operation), Some(result), _) => {
            SavedEventOutcome::EngineeringEnqueued {
                sequence: result.sequence,
                trace: operation.trace(),
                queued_event: result
                    .snapshot
                    .pending_events
                    .last()
                    .ok_or("saved event result lost its appended event")?,
            }
        }
        (_, _, Some((reason, detail))) => SavedEventOutcome::Unsupported {
            reason: *reason,
            detail,
        },
        _ => unreachable!("complete event request preparation"),
    };
    let report = SavedEventReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering enqueue of one exact source block on an existing owner","campaign":campaign,"instance":request.instance,"owner":request.owner,"before_revision":before_revision,"clocks":clocks,"existing_pending_events":existing_pending,"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,"prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},"executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"private_result_discarded":queued.is_none(),"event_executed":false,"event_acknowledged":false,"clocks_advanced":false,"original_dispatch_verified":false,"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        snapshot_event_request: outcome,
    };
    let report =
        admit_saved_copy_report(&report, request.maximum_report_bytes, "saved event request")?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceBootInitialization {
    campaign: fallout_runtime::identity::CampaignId,
    context: ReferenceBootContext,
    initializers: Vec<copy_probe::Initializer>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedReferenceBootRequest {
    schema_version: u32,
    reference: ReferenceId,
    expected_authored_key: fallout_data::identity::FormKey,
    intent: SavedForeignIntent,
    initialization: ReferenceBootInitialization,
    #[serde(with = "ReferenceSourceLimits")]
    source_limits: script_reference_attachment::Limits,
    #[serde(with = "ReferenceInitializationLimits")]
    initialization_limits: attachment_boot::Limits,
    maximum_source_instructions: usize,
    maximum_source_operand_uses: usize,
    maximum_source_tokens: usize,
    maximum_trace_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}
#[derive(serde::Serialize)]
struct ReferenceBootTrace<'a> {
    reference: ReferenceId,
    expected_authored_key: &'a fallout_data::identity::FormKey,
    definition: &'a loaded_scripts::Handle,
    script_source: &'a loaded_scripts::Version,
    attachment: &'a script_reference_attachment::Proof,
    source_receipts: &'a [fallout_data::store::SourceReceipt],
    source_counts: script_reference_attachment::Counts,
    initialization: &'a attachment_boot::Request,
    initialization_counts: Option<attachment_boot::Counts>,
    source_cohort_sha256: &'a str,
    decoder_sha256: &'a str,
}
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ReferenceBootOutcome {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    EngineeringBooted {
        instance: fallout_runtime::identity::InstanceId,
    },
}
#[derive(serde::Serialize)]
struct ReferenceBootReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    trace: &'a ReferenceBootTrace<'a>,
    reference_boot: ReferenceBootOutcome,
}
pub(super) fn boot_saved_reference(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedReferenceBootRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "reference boot request byte budget exceeded",
    )?)?;
    let s = request.source_limits;
    let d = script_reference_attachment::Limits::default();
    let i = request.initialization_limits;
    let j = attachment_boot::Limits::default();
    let p = programs::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    if request.schema_version != 1
        || s.maximum_sources > d.maximum_sources
        || s.maximum_source_bytes > d.maximum_source_bytes
        || s.maximum_header_visits > d.maximum_header_visits
        || s.maximum_catalogue_scripts > d.maximum_catalogue_scripts
        || s.maximum_variable_bytes > d.maximum_variable_bytes
        || s.maximum_record_bytes > d.maximum_record_bytes
        || s.maximum_read_bytes > d.maximum_read_bytes
        || s.maximum_field_visits > d.maximum_field_visits
        || i.maximum_initializers > j.maximum_initializers
        || i.maximum_context_arguments > j.maximum_context_arguments
        || i.maximum_variable_bytes > j.maximum_variable_bytes
        || i.maximum_source_receipt_bytes > j.maximum_source_receipt_bytes
        || i.maximum_declarations > j.maximum_declarations
        || request.maximum_source_instructions > p.maximum_instructions
        || request.maximum_source_operand_uses > p.maximum_uses
        || request.maximum_source_tokens > p.maximum_tokens
        || request.maximum_trace_bytes > 2 * 1024 * 1024
        || request.maximum_trace_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
        || request.maximum_report_bytes == 0
    {
        return Err("unsupported reference boot schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(install, result_path, report_path, "reference boot")?;
    let initialization = attachment_boot::Request {
        campaign: request.initialization.campaign,
        context: request.initialization.context.into_context(),
        initializers: request.initialization.initializers,
    };
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = obscript::expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    if order.names.len() > s.maximum_sources {
        return Err("reference boot source count budget exceeded".into());
    }
    let mut store = order.store(install, cache)?;
    script_reference_attachment::preflight(&store, &request.expected_authored_key, s)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits {
            max_candidate_records: s.maximum_header_visits.min(1_000_000),
            max_candidate_read_bytes: s.maximum_read_bytes,
            max_candidate_record_bytes: s.maximum_record_bytes,
            max_scripts: s.maximum_catalogue_scripts,
            max_retained_bytes: s.maximum_read_bytes,
            max_variables: s.maximum_field_visits,
            max_references: s.maximum_field_visits,
        },
        |_, _| Ok(()),
    )?);
    let attachment = script_reference_attachment::request(
        &mut store,
        &catalogue,
        &request.expected_authored_key,
        s,
    )?;
    let content = Content::load(
        &mut store,
        &catalogue,
        s.maximum_header_visits.min(1_000_000),
    )?;
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[attachment.definition().clone()],
        programs::Limits {
            maximum_attempted_record_bytes: s.maximum_read_bytes,
            maximum_attempted_bytes: s.maximum_read_bytes,
            maximum_instructions: request.maximum_source_instructions,
            maximum_expressions: request.maximum_source_tokens.min(p.maximum_expressions),
            maximum_tokens: request.maximum_source_tokens,
            maximum_nodes: request.maximum_source_tokens,
            maximum_uses: request.maximum_source_operand_uses,
            ..p
        },
    )?;
    let prepared = reference_attachment_boot::prepare(
        &sources,
        &attachment,
        &content,
        reference_attachment_boot::Selection {
            reference: request.reference,
            expected_authored_key: &request.expected_authored_key,
            intent: match request.intent {
                SavedForeignIntent::Engineering => local_copy::Intent::Engineering,
                SavedForeignIntent::Faithful => local_copy::Intent::Faithful,
            },
        },
        &initialization,
        i,
    )?;
    let trace = ReferenceBootTrace {
        reference: request.reference,
        expected_authored_key: &request.expected_authored_key,
        definition: attachment.definition(),
        script_source: attachment.script_version(),
        attachment: attachment.proof(),
        source_receipts: attachment.source_receipts(),
        source_counts: attachment.counts(),
        initialization: &initialization,
        initialization_counts: match &prepared {
            reference_attachment_boot::Preparation::Ready(plan) => Some(plan.counts()),
            _ => None,
        },
        source_cohort_sha256: sources.source_cohort_sha256(),
        decoder_sha256: sources.decoder_sha256(),
    };
    let mut trace_bytes = BoundedJson {
        bytes: Vec::new(),
        maximum: request.maximum_trace_bytes,
    };
    serde_json::to_writer(&mut trace_bytes, &trace)
        .map_err(|_| "reference boot trace byte budget exceeded")?;
    let trace_size = trace_bytes.bytes.len();
    drop(trace_bytes);
    let input_bytes = read_bounded_named(
        snapshot_path,
        world_limits.max_snapshot_bytes,
        "reference boot snapshot byte budget exceeded",
    )?;
    let input = fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?;
    let before_revision = input.state_revision;
    let (outcome, result_bytes, artifact) = match prepared {
        reference_attachment_boot::Preparation::Unsupported { reason, detail } => {
            // Faithful still admits only a valid strict current snapshot.
            let world =
                fallout_runtime::World::restore(Arc::clone(&catalogue), input, world_limits)?;
            sources.validate_world(&world)?;
            (
                ReferenceBootOutcome::Unsupported { reason, detail },
                None,
                Value::Null,
            )
        }
        reference_attachment_boot::Preparation::Ready(plan) => {
            let result = plan.apply(input, world_limits)?;
            let bytes = result
                .snapshot
                .encode(request.maximum_result_snapshot_bytes)?;
            let cold = fallout_runtime::World::restore(
                Arc::clone(&catalogue),
                fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
                world_limits,
            )?;
            let owner = fallout_runtime::identity::Owner::Placed {
                reference: request.reference,
            };
            if cold.snapshot() != result.snapshot
                || cold.owner_instance(&owner) != Some(result.instance)
                || cold.instance(cold.handle(result.instance)?)?.definition() != plan.definition()
                || cold.reference_origin(request.reference)? != Some(&request.expected_authored_key)
                || cold.authored_reference(&request.expected_authored_key)
                    != Some(request.reference)
            {
                return Err("reference boot complete cold result differs".into());
            }
            let artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":result.snapshot.schema_version,"decode_restore_equal":true,"after_revision":result.snapshot.state_revision});
            (
                ReferenceBootOutcome::EngineeringBooted {
                    instance: result.instance,
                },
                Some(bytes),
                artifact,
            )
        }
    };
    let report = ReferenceBootReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering boot of one existing source-attached reference owner","campaign":initialization.campaign,"before_revision":before_revision,"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,"trace_bytes":trace_size,"prepared_sources":{"counts":sources.counts()},"executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"event_enqueued":false,"reference_created":false,"original_activation_verified":false,"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        trace: &trace,
        reference_boot: outcome,
    };
    let report = admit_saved_copy_report(&report, request.maximum_report_bytes, "reference boot")?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedForeignIntent {
    Faithful,
    Engineering,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedForeignCopyRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    owner: fallout_runtime::identity::Owner,
    intent: SavedForeignIntent,
    #[serde(deserialize_with = "explicit_optional_reference")]
    explicit_player: Option<ReferenceId>,
    maximum_source_instructions: usize,
    maximum_operand_uses: usize,
    maximum_statement_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_metadata_rows: usize,
    maximum_probe_variable_bytes: usize,
    maximum_trace_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}

#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SavedCopyOutcome<'a, T> {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'a str,
    },
    EngineeringCommitted {
        committed: &'a T,
    },
}
type SavedForeignOutcome<'a> = SavedCopyOutcome<'a, foreign_copy::Committed>;
#[derive(serde::Serialize)]
struct SavedForeignReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    snapshot_foreign_copy: SavedForeignOutcome<'a>,
}

fn admit_saved_copy_outputs(
    install: &Path,
    result: &Path,
    report: Option<&Path>,
    scope: &str,
) -> Result<()> {
    let protected = super::protected_tree(install)?;
    let fresh = |path: &Path| -> Result<std::path::PathBuf> {
        if path.try_exists()? {
            return Err(format!("{scope} output must be a fresh artifact").into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(&protected) {
            return Err(format!("{scope} output must be outside the installation").into());
        }
        Ok(parent.join(
            path.file_name()
                .ok_or_else(|| format!("{scope} output requires a filename"))?,
        ))
    };
    let result = fresh(result)?;
    if let Some(report) = report {
        let report = fresh(report)?;
        if report == result
            || (cfg!(windows)
                && report
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&result.to_string_lossy()))
        {
            return Err(format!("{scope} report must differ from result snapshot").into());
        }
    }
    Ok(())
}
fn admit_saved_copy_report(
    report: &impl serde::Serialize,
    maximum: usize,
    scope: &str,
) -> Result<Value> {
    let mut admitted = BoundedJson {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer_pretty(&mut admitted, report)
        .map_err(|_| format!("{scope} report byte budget exceeded"))?;
    admitted
        .write_all(b"\n")
        .map_err(|_| format!("{scope} report byte budget exceeded"))?;
    Ok(serde_json::to_value(report)?)
}
fn write_saved_copy_result(path: &Path, bytes: Option<Vec<u8>>) -> Result<()> {
    if let Some(bytes) = bytes {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    Ok(())
}

pub(super) fn copy_saved_foreign(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedForeignCopyRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved foreign copy request byte budget exceeded",
    )?)?;
    let defaults = foreign_copy::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_instructions > defaults.maximum_event_instructions
        || request.maximum_operand_uses > defaults.maximum_operand_uses
        || request.maximum_statement_bytes > defaults.maximum_statement_bytes
        || request.maximum_trace_source_bytes > defaults.observation.maximum_source_bytes
        || request.maximum_trace_rows > defaults.observation.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.observation.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.observation.maximum_binding_uses
        || request.maximum_metadata_rows > defaults.maximum_metadata_rows
        || request.maximum_probe_variable_bytes > defaults.maximum_probe_variable_bytes
        || request.maximum_trace_bytes > defaults.maximum_trace_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved foreign copy schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(install, result_path, report_path, "saved foreign copy")?;
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
        "saved foreign copy snapshot byte budget exceeded",
    )?;
    let mut world = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?,
        world_limits,
    )?;
    let pending = world
        .pending_events()
        .next()
        .filter(|pending| pending.sequence == request.sequence.get())
        .ok_or("saved foreign copy must name the existing journal head")?;
    let instance = world.instance(world.handle(pending.instance)?)?;
    if instance.owner() != &request.owner {
        return Err("saved foreign copy explicit owner differs from journal head".into());
    }
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[instance.definition().clone()],
        Default::default(),
    )?;
    let before_revision = world.revision();
    let outcome = foreign_copy::stage(
        &world,
        &sources,
        &content,
        foreign_copy::Selection {
            sequence: request.sequence.get(),
            explicit_player: request.explicit_player,
            intent: match request.intent {
                SavedForeignIntent::Faithful => local_copy::Intent::Faithful,
                SavedForeignIntent::Engineering => local_copy::Intent::Engineering,
            },
        },
        foreign_copy::Limits {
            maximum_event_instructions: request.maximum_source_instructions,
            maximum_operand_uses: request.maximum_operand_uses,
            maximum_statement_bytes: request.maximum_statement_bytes,
            observation: preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_trace_source_bytes,
                maximum_rows: request.maximum_trace_rows,
                maximum_variable_bytes: request.maximum_trace_variable_bytes,
                maximum_binding_uses: request.maximum_trace_binding_uses,
            },
            maximum_metadata_rows: request.maximum_metadata_rows,
            maximum_probe_variable_bytes: request.maximum_probe_variable_bytes,
            maximum_trace_bytes: request.maximum_trace_bytes,
        },
    )?;
    let (committed, unsupported) = match outcome {
        foreign_copy::Preparation::Unsupported { reason, detail } => (None, Some((reason, detail))),
        foreign_copy::Preparation::Staged(proposal) => (Some(proposal.commit(&mut world)?), None),
    };
    let mut artifact = Value::Null;
    let result_bytes = if committed.is_some() {
        let snapshot = world.snapshot();
        let bytes = snapshot.encode(request.maximum_result_snapshot_bytes)?;
        let cold = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
            world_limits,
        )?;
        if cold.snapshot() != snapshot {
            return Err("saved foreign copy complete cold result differs".into());
        }
        artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":snapshot.schema_version,"decode_restore_equal":true,"remaining_head":snapshot.pending_events.first()});
        Some(bytes)
    } else {
        None
    };
    let outcome = match (&committed, &unsupported) {
        (Some(committed), _) => SavedForeignOutcome::EngineeringCommitted { committed },
        (_, Some((reason, detail))) => SavedForeignOutcome::Unsupported {
            reason: *reason,
            detail,
        },
        _ => unreachable!("complete preparation outcome"),
    };
    let report = SavedForeignReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering current foreign numeric read into one own numeric slot", "campaign":world.campaign(),"owner":request.owner,"explicit_player":request.explicit_player,
            "before_revision":before_revision,"after_revision":world.revision(),"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,
            "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
            "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"private_result_discarded":committed.is_none(),"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        snapshot_foreign_copy: outcome,
    };
    // Borrow the complete trace until the exact pretty report plus newline fits.
    // No output exists for a strict identity/work/output capacity failure.
    let report =
        admit_saved_copy_report(&report, request.maximum_report_bytes, "saved foreign copy")?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedReferenceCopyRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    owner: fallout_runtime::identity::Owner,
    intent: SavedForeignIntent,
    maximum_source_instructions: usize,
    maximum_operand_uses: usize,
    maximum_statement_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_probe_variable_bytes: usize,
    maximum_trace_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}
#[derive(serde::Serialize)]
struct SavedReferenceReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    snapshot_reference_copy: SavedCopyOutcome<'a, reference_copy::Committed>,
}
pub(super) fn copy_saved_reference(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedReferenceCopyRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved reference copy request byte budget exceeded",
    )?)?;
    let defaults = reference_copy::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_instructions > defaults.maximum_event_instructions
        || request.maximum_operand_uses > defaults.maximum_operand_uses
        || request.maximum_statement_bytes > defaults.maximum_statement_bytes
        || request.maximum_trace_source_bytes > defaults.observation.maximum_source_bytes
        || request.maximum_trace_rows > defaults.observation.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.observation.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.observation.maximum_binding_uses
        || request.maximum_probe_variable_bytes > defaults.maximum_probe_variable_bytes
        || request.maximum_trace_bytes > defaults.maximum_trace_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved reference copy schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(install, result_path, report_path, "saved reference copy")?;
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
        "saved reference copy snapshot byte budget exceeded",
    )?;
    let mut world = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?,
        world_limits,
    )?;
    let pending = world
        .pending_events()
        .next()
        .filter(|p| p.sequence == request.sequence.get())
        .ok_or("saved reference copy must name the existing journal head")?;
    let instance = world.instance(world.handle(pending.instance)?)?;
    if instance.owner() != &request.owner {
        return Err("saved reference copy explicit owner differs from journal head".into());
    }
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[instance.definition().clone()],
        Default::default(),
    )?;
    let before_revision = world.revision();
    let outcome = reference_copy::stage(
        &world,
        &sources,
        &content,
        reference_copy::Selection {
            sequence: request.sequence.get(),
            intent: match request.intent {
                SavedForeignIntent::Faithful => local_copy::Intent::Faithful,
                SavedForeignIntent::Engineering => local_copy::Intent::Engineering,
            },
        },
        reference_copy::Limits {
            maximum_event_instructions: request.maximum_source_instructions,
            maximum_operand_uses: request.maximum_operand_uses,
            maximum_statement_bytes: request.maximum_statement_bytes,
            observation: preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_trace_source_bytes,
                maximum_rows: request.maximum_trace_rows,
                maximum_variable_bytes: request.maximum_trace_variable_bytes,
                maximum_binding_uses: request.maximum_trace_binding_uses,
            },
            maximum_probe_variable_bytes: request.maximum_probe_variable_bytes,
            maximum_trace_bytes: request.maximum_trace_bytes,
        },
    )?;
    let (committed, unsupported) = match outcome {
        reference_copy::Preparation::Unsupported { reason, detail } => {
            (None, Some((reason, detail)))
        }
        reference_copy::Preparation::Staged(proposal) => (Some(proposal.commit(&mut world)?), None),
    };
    let mut artifact = Value::Null;
    let result_bytes = if committed.is_some() {
        let snapshot = world.snapshot();
        let bytes = snapshot.encode(request.maximum_result_snapshot_bytes)?;
        let cold = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
            world_limits,
        )?;
        if cold.snapshot() != snapshot {
            return Err("saved reference copy complete cold result differs".into());
        }
        artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":snapshot.schema_version,"decode_restore_equal":true,"remaining_head":snapshot.pending_events.first()});
        Some(bytes)
    } else {
        None
    };
    let outcome = match (&committed, &unsupported) {
        (Some(committed), _) => SavedCopyOutcome::EngineeringCommitted { committed },
        (_, Some((reason, detail))) => SavedCopyOutcome::Unsupported {
            reason: *reason,
            detail,
        },
        _ => unreachable!("complete preparation outcome"),
    };
    let report = SavedReferenceReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering own typed reference identity copy","campaign":world.campaign(),"owner":request.owner,"before_revision":before_revision,"after_revision":world.revision(),"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,
            "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},"executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"private_result_discarded":committed.is_none(),"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        snapshot_reference_copy: outcome,
    };
    let report = admit_saved_copy_report(
        &report,
        request.maximum_report_bytes,
        "saved reference copy",
    )?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
}

enum SavedLiteralIntent {
    Faithful,
    EngineeringExactIntegralDecimal,
}
impl<'de> serde::Deserialize<'de> for SavedLiteralIntent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        match <String as serde::Deserialize>::deserialize(d)?.as_str() {
            "faithful" => Ok(Self::Faithful),
            "engineering_exact_integral_decimal" => Ok(Self::EngineeringExactIntegralDecimal),
            _ => Err(serde::de::Error::custom(
                "unsupported saved literal assignment intent",
            )),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedLiteralAssignmentRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    owner: fallout_runtime::identity::Owner,
    intent: SavedLiteralIntent,
    maximum_source_instructions: usize,
    maximum_operand_uses: usize,
    maximum_statement_bytes: usize,
    maximum_literal_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_stage_variable_bytes: usize,
    maximum_trace_bytes: usize,
    maximum_prepared_instructions: usize,
    maximum_prepared_operand_uses: usize,
    maximum_prepared_tokens: usize,
    maximum_prepared_record_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}
#[derive(serde::Serialize)]
struct SavedLiteralReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    snapshot_literal_assignment: SavedCopyOutcome<'a, literal_assignment::Committed>,
}
pub(super) fn assign_saved_literal(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedLiteralAssignmentRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved literal assignment request byte budget exceeded",
    )?)?;
    let defaults = literal_assignment::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    let p = programs::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_instructions > defaults.maximum_event_instructions
        || request.maximum_operand_uses > defaults.maximum_operand_uses
        || request.maximum_statement_bytes > defaults.maximum_statement_bytes
        || request.maximum_literal_bytes > defaults.maximum_literal_bytes
        || request.maximum_trace_source_bytes > defaults.observation.maximum_source_bytes
        || request.maximum_trace_rows > defaults.observation.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.observation.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.observation.maximum_binding_uses
        || request.maximum_stage_variable_bytes > defaults.maximum_stage_variable_bytes
        || request.maximum_trace_bytes > defaults.maximum_trace_bytes
        || request.maximum_prepared_instructions > p.maximum_instructions
        || request.maximum_prepared_operand_uses > p.maximum_uses
        || request.maximum_prepared_tokens > p.maximum_tokens
        || request.maximum_prepared_record_bytes > p.maximum_attempted_record_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved literal assignment schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(
        install,
        result_path,
        report_path,
        "saved literal assignment",
    )?;
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
        "saved literal assignment snapshot byte budget exceeded",
    )?;
    let mut world = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?,
        world_limits,
    )?;
    let pending = world
        .pending_events()
        .next()
        .filter(|p| p.sequence == request.sequence.get())
        .ok_or("saved literal assignment must name the existing journal head")?;
    let instance = world.instance(world.handle(pending.instance)?)?;
    if instance.owner() != &request.owner {
        return Err("saved literal assignment explicit owner differs from journal head".into());
    }
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        std::slice::from_ref(instance.definition()),
        programs::Limits {
            maximum_instructions: request.maximum_prepared_instructions,
            maximum_uses: request.maximum_prepared_operand_uses,
            maximum_tokens: request.maximum_prepared_tokens,
            maximum_nodes: request.maximum_prepared_tokens,
            maximum_expressions: request.maximum_prepared_tokens.min(p.maximum_expressions),
            maximum_attempted_record_bytes: request.maximum_prepared_record_bytes,
            ..p
        },
    )?;
    let before_revision = world.revision();
    let outcome = literal_assignment::stage(
        &world,
        &sources,
        &content,
        literal_assignment::Selection {
            sequence: request.sequence.get(),
            intent: match request.intent {
                SavedLiteralIntent::Faithful => literal_assignment::Intent::Faithful,
                SavedLiteralIntent::EngineeringExactIntegralDecimal => {
                    literal_assignment::Intent::EngineeringExactIntegralDecimal
                }
            },
        },
        literal_assignment::Limits {
            maximum_event_instructions: request.maximum_source_instructions,
            maximum_operand_uses: request.maximum_operand_uses,
            maximum_statement_bytes: request.maximum_statement_bytes,
            maximum_literal_bytes: request.maximum_literal_bytes,
            observation: preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_trace_source_bytes,
                maximum_rows: request.maximum_trace_rows,
                maximum_variable_bytes: request.maximum_trace_variable_bytes,
                maximum_binding_uses: request.maximum_trace_binding_uses,
            },
            maximum_stage_variable_bytes: request.maximum_stage_variable_bytes,
            maximum_trace_bytes: request.maximum_trace_bytes,
        },
    )?;
    let (committed, unsupported) = match outcome {
        literal_assignment::Preparation::Unsupported { reason, detail } => {
            (None, Some((reason, detail)))
        }
        literal_assignment::Preparation::Staged(proposal) => {
            (Some(proposal.commit(&mut world)?), None)
        }
    };
    let mut artifact = Value::Null;
    let result_bytes = if committed.is_some() {
        let snapshot = world.snapshot();
        let bytes = snapshot.encode(request.maximum_result_snapshot_bytes)?;
        let cold = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
            world_limits,
        )?;
        if cold.snapshot() != snapshot {
            return Err("saved literal assignment complete cold result differs".into());
        }
        artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":snapshot.schema_version,"decode_restore_equal":true,"remaining_head":snapshot.pending_events.first()});
        Some(bytes)
    } else {
        None
    };
    let outcome = match (&committed, &unsupported) {
        (Some(committed), _) => SavedCopyOutcome::EngineeringCommitted { committed },
        (_, Some((reason, detail))) => SavedCopyOutcome::Unsupported {
            reason: *reason,
            detail,
        },
        _ => unreachable!("complete preparation outcome"),
    };
    let report = SavedLiteralReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering exact integral decimal source assignment","campaign":world.campaign(),"owner":request.owner,"before_revision":before_revision,"after_revision":world.revision(),"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,
            "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},"executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"private_result_discarded":committed.is_none(),"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        snapshot_literal_assignment: outcome,
    };
    let report = admit_saved_copy_report(
        &report,
        request.maximum_report_bytes,
        "saved literal assignment",
    )?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
}

enum SavedNativeAssignmentIntent {
    Faithful,
    EngineeringExactCountToNumber,
}
impl<'de> serde::Deserialize<'de> for SavedNativeAssignmentIntent {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let name = <String as serde::Deserialize>::deserialize(deserializer)?;
        match name.as_str() {
            "faithful" => Ok(Self::Faithful),
            "engineering_exact_count_to_number" => Ok(Self::EngineeringExactCountToNumber),
            _ => Err(serde::de::Error::custom(
                "unsupported saved native assignment intent",
            )),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNativeAssignmentRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    owner: fallout_runtime::identity::Owner,
    intent: SavedNativeAssignmentIntent,
    #[serde(deserialize_with = "explicit_optional_reference")]
    supplied_subject: Option<ReferenceId>,
    #[serde(deserialize_with = "explicit_optional_reference")]
    explicit_player: Option<ReferenceId>,
    maximum_source_instructions: usize,
    maximum_calls: usize,
    maximum_argument_bytes: usize,
    maximum_operand_uses: usize,
    maximum_statement_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_query_variable_bytes: usize,
    maximum_stage_variable_bytes: usize,
    maximum_inventory_visits: usize,
    maximum_contributions: usize,
    maximum_trace_bytes: usize,
    maximum_prepared_instructions: usize,
    maximum_prepared_operand_uses: usize,
    maximum_prepared_tokens: usize,
    maximum_prepared_record_bytes: usize,
    maximum_result_snapshot_bytes: usize,
    maximum_report_bytes: usize,
}
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SavedNativeAssignmentOutcome<'a> {
    EngineeringCommitted {
        committed: &'a native_assignment::Committed,
    },
    Unsupported {
        reason: &'a native_assignment::Unsupported,
        detail: &'a str,
    },
}
#[derive(serde::Serialize)]
struct SavedNativeAssignmentReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    snapshot_native_assignment: SavedNativeAssignmentOutcome<'a>,
}
pub(super) fn assign_saved_native(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    snapshot_path: &Path,
    result_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedNativeAssignmentRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved native assignment request byte budget exceeded",
    )?)?;
    let defaults = native_assignment::Limits::default();
    let world_limits = fallout_runtime::Limits::default();
    let p = programs::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_instructions > defaults.native.maximum_event_instructions
        || request.maximum_calls > defaults.native.maximum_calls
        || request.maximum_argument_bytes > defaults.native.maximum_argument_bytes
        || request.maximum_operand_uses > defaults.maximum_operand_uses
        || request.maximum_statement_bytes > defaults.maximum_statement_bytes
        || request.maximum_trace_source_bytes > defaults.observation.maximum_source_bytes
        || request.maximum_trace_rows > defaults.observation.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.observation.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.observation.maximum_binding_uses
        || request.maximum_query_variable_bytes > defaults.maximum_query_variable_bytes
        || request.maximum_stage_variable_bytes > defaults.maximum_stage_variable_bytes
        || request.maximum_inventory_visits > defaults.maximum_inventory_visits
        || request.maximum_contributions > defaults.maximum_contributions
        || request.maximum_trace_bytes > defaults.maximum_trace_bytes
        || request.maximum_prepared_instructions > p.maximum_instructions
        || request.maximum_prepared_operand_uses > p.maximum_uses
        || request.maximum_prepared_tokens > p.maximum_tokens
        || request.maximum_prepared_record_bytes > p.maximum_attempted_record_bytes
        || request.maximum_result_snapshot_bytes == 0
        || request.maximum_result_snapshot_bytes > world_limits.max_snapshot_bytes
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved native assignment schema/budget ceiling".into());
    }
    admit_saved_copy_outputs(install, result_path, report_path, "saved native assignment")?;
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
        "saved native assignment snapshot byte budget exceeded",
    )?;
    let mut world = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&input_bytes, world_limits)?,
        world_limits,
    )?;
    let pending = world
        .pending_events()
        .next()
        .filter(|p| p.sequence == request.sequence.get())
        .ok_or("saved native assignment must name the existing journal head")?;
    let instance = world.instance(world.handle(pending.instance)?)?;
    if instance.owner() != &request.owner {
        return Err("saved native assignment explicit owner differs from journal head".into());
    }
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        std::slice::from_ref(instance.definition()),
        programs::Limits {
            maximum_instructions: request.maximum_prepared_instructions,
            maximum_uses: request.maximum_prepared_operand_uses,
            maximum_tokens: request.maximum_prepared_tokens,
            maximum_nodes: request.maximum_prepared_tokens,
            maximum_expressions: request.maximum_prepared_tokens.min(p.maximum_expressions),
            maximum_attempted_record_bytes: request.maximum_prepared_record_bytes,
            ..p
        },
    )?;
    let before_revision = world.revision();
    let outcome = native_assignment::stage(
        &world,
        &sources,
        &content,
        native_assignment::Selection {
            sequence: request.sequence.get(),
            inputs: native::Inputs {
                supplied_subject: request.supplied_subject,
                player: request.explicit_player,
            },
            intent: match request.intent {
                SavedNativeAssignmentIntent::Faithful => native_assignment::Intent::Faithful,
                SavedNativeAssignmentIntent::EngineeringExactCountToNumber => {
                    native_assignment::Intent::EngineeringExactCountToNumber
                }
            },
        },
        native_assignment::Limits {
            native: native::Limits {
                maximum_event_instructions: request.maximum_source_instructions,
                maximum_calls: request.maximum_calls,
                maximum_argument_bytes: request.maximum_argument_bytes,
            },
            maximum_operand_uses: request.maximum_operand_uses,
            maximum_statement_bytes: request.maximum_statement_bytes,
            observation: preparation::ObservationLimits {
                maximum_source_bytes: request.maximum_trace_source_bytes,
                maximum_rows: request.maximum_trace_rows,
                maximum_variable_bytes: request.maximum_trace_variable_bytes,
                maximum_binding_uses: request.maximum_trace_binding_uses,
            },
            maximum_query_variable_bytes: request.maximum_query_variable_bytes,
            maximum_stage_variable_bytes: request.maximum_stage_variable_bytes,
            maximum_inventory_visits: request.maximum_inventory_visits,
            maximum_contributions: request.maximum_contributions,
            maximum_trace_bytes: request.maximum_trace_bytes,
        },
    )?;
    let (committed, unsupported) = match outcome {
        native_assignment::Preparation::Unsupported { reason, detail } => {
            (None, Some((reason, detail)))
        }
        native_assignment::Preparation::Staged(proposal) => {
            (Some(proposal.commit(&mut world)?), None)
        }
    };
    let mut artifact = Value::Null;
    let result_bytes = if committed.is_some() {
        let snapshot = world.snapshot();
        let bytes = snapshot.encode(request.maximum_result_snapshot_bytes)?;
        let cold = fallout_runtime::World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, world_limits)?,
            world_limits,
        )?;
        if cold.snapshot() != snapshot {
            return Err("saved native assignment complete cold result differs".into());
        }
        artifact = json!({"path":result_path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema_version":snapshot.schema_version,"decode_restore_equal":true,"remaining_head":snapshot.pending_events.first()});
        Some(bytes)
    } else {
        None
    };
    let outcome = match (&committed, &unsupported) {
        (Some(committed), _) => SavedNativeAssignmentOutcome::EngineeringCommitted { committed },
        (_, Some((reason, detail))) => SavedNativeAssignmentOutcome::Unsupported { reason, detail },
        _ => unreachable!("complete preparation outcome"),
    };
    let report = SavedNativeAssignmentReport {
        metadata: json!({"schema_version":1,"scope":"Explicit engineering exact inventory count source assignment","campaign":world.campaign(),"owner":request.owner,"before_revision":before_revision,"after_revision":world.revision(),"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"result_snapshot":artifact,
            "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),"decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},"executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),"private_result_discarded":committed.is_none(),"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        snapshot_native_assignment: outcome,
    };
    let report = admit_saved_copy_report(
        &report,
        request.maximum_report_bytes,
        "saved native assignment",
    )?;
    write_saved_copy_result(result_path, result_bytes)?;
    Ok(report)
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNativePlanRequest {
    schema_version: u32,
    sequence: std::num::NonZeroU64,
    occurrence: usize,
    intent: SavedNativeIntent,
    #[serde(deserialize_with = "explicit_optional_reference")]
    supplied_subject: Option<ReferenceId>,
    #[serde(deserialize_with = "explicit_optional_reference")]
    explicit_player: Option<ReferenceId>,
    maximum_source_instructions: usize,
    maximum_calls: usize,
    maximum_argument_bytes: usize,
    maximum_trace_source_bytes: usize,
    maximum_trace_rows: usize,
    maximum_trace_variable_bytes: usize,
    maximum_trace_binding_uses: usize,
    maximum_query_variable_bytes: usize,
    maximum_contributions: usize,
    maximum_report_bytes: usize,
}

#[derive(serde::Serialize)]
struct SavedNativePlanProof<'a> {
    source: &'a preparation::EventObservation,
    call: &'a native_plan::CallProof,
    occurrence: usize,
    supplied_subject: Option<ReferenceId>,
    explicit_player: Option<ReferenceId>,
    resolved_subject: ReferenceId,
    resolved_item: &'a fallout_data::identity::FormKey,
    creation_counts: &'a native_plan::Counts,
}

#[derive(serde::Serialize)]
struct SavedNativePlanReport<'a> {
    #[serde(flatten)]
    metadata: Value,
    plan: Option<SavedNativePlanProof<'a>>,
    outcome: native::Outcome,
}

pub(super) fn observe_saved_native_plan(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    initial_path: &Path,
    current_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let request: SavedNativePlanRequest = serde_json::from_slice(&read_bounded_named(
        request_path,
        16 * 1024,
        "saved native plan request byte budget exceeded",
    )?)?;
    let defaults = native_plan::Limits::default();
    if request.schema_version != 1
        || request.maximum_source_instructions > defaults.native.maximum_event_instructions
        || request.maximum_calls > defaults.native.maximum_calls
        || request.maximum_argument_bytes > defaults.native.maximum_argument_bytes
        || request.maximum_trace_source_bytes > defaults.source_projection.maximum_source_bytes
        || request.maximum_trace_rows > defaults.source_projection.maximum_rows
        || request.maximum_trace_variable_bytes > defaults.source_projection.maximum_variable_bytes
        || request.maximum_trace_binding_uses > defaults.source_projection.maximum_binding_uses
        || request.maximum_query_variable_bytes > defaults.maximum_query_variable_bytes
        || request.maximum_contributions > 65_536
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported saved native plan schema/budget ceiling".into());
    }
    if let Some(report) = report_path {
        if report.try_exists()? {
            return Err("saved native plan report must be a fresh artifact".into());
        }
        let parent = report
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(super::protected_tree(install)?) {
            return Err("saved native plan report must be outside the installation".into());
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
    let world_limits = fallout_runtime::Limits::default();
    let initial_bytes = read_bounded_named(
        initial_path,
        world_limits.max_snapshot_bytes,
        "saved native plan initial snapshot byte budget exceeded",
    )?;
    let initial = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&initial_bytes, world_limits)?,
        world_limits,
    )?;
    let pending = initial
        .pending_events()
        .next()
        .filter(|pending| pending.sequence == request.sequence.get())
        .ok_or("Saved native plan sequence must be the existing journal head")?;
    let definition = initial
        .instance(initial.handle(pending.instance)?)?
        .definition()
        .clone();
    let sources = programs::PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[definition],
        Default::default(),
    )?;
    let limits = native_plan::Limits {
        native: native::Limits {
            maximum_event_instructions: request.maximum_source_instructions,
            maximum_calls: request.maximum_calls,
            maximum_argument_bytes: request.maximum_argument_bytes,
        },
        source_projection: preparation::ObservationLimits {
            maximum_source_bytes: request.maximum_trace_source_bytes,
            maximum_rows: request.maximum_trace_rows,
            maximum_variable_bytes: request.maximum_trace_variable_bytes,
            maximum_binding_uses: request.maximum_trace_binding_uses,
        },
        maximum_query_variable_bytes: request.maximum_query_variable_bytes,
    };
    let selection = native_plan::Selection {
        sequence: request.sequence.get(),
        occurrence: request.occurrence,
        inputs: native::Inputs {
            supplied_subject: request.supplied_subject,
            player: request.explicit_player,
        },
        intent: match request.intent {
            SavedNativeIntent::Faithful => native::Intent::Faithful,
            SavedNativeIntent::EngineeringObservation => native::Intent::EngineeringObservation,
        },
    };
    let prepared = native_plan::prepare(&initial, &sources, &content, selection, limits)?;
    // The plan is owned. The initial World is gone before reading/restoring the
    // separately supplied current snapshot; no epoch handle or count is cached.
    drop(initial);
    let current_bytes = read_bounded_named(
        current_path,
        world_limits.max_snapshot_bytes,
        "saved native plan current snapshot byte budget exceeded",
    )?;
    let current = fallout_runtime::World::restore(
        Arc::clone(&catalogue),
        fallout_runtime::snapshot::Snapshot::decode(&current_bytes, world_limits)?,
        world_limits,
    )?;
    let before = current.snapshot();
    let (plan, outcome) = match &prepared {
        native_plan::Preparation::Unsupported { reason, detail } => (
            None,
            native::Outcome::Unsupported {
                reason: *reason,
                detail: detail.clone(),
            },
        ),
        native_plan::Preparation::Ready(plan) => (
            Some(SavedNativePlanProof {
                source: plan.source(),
                call: plan.call(),
                occurrence: plan.occurrence(),
                supplied_subject: plan.supplied_subject(),
                explicit_player: plan.explicit_player(),
                resolved_subject: plan.subject(),
                resolved_item: plan.item(),
                creation_counts: plan.counts(),
            }),
            plan.observe(&current, &sources, &content, request.maximum_contributions)?,
        ),
    };
    if current.snapshot() != before {
        return Err("saved native plan observation changed canonical state or journal".into());
    }
    let report = SavedNativePlanReport {
        metadata: json!({"schema_version":1,"scope":"Owned source-native engineering query plan evaluated after dropping the initial World and strictly restoring current canonical state",
            "initial_snapshot_sha256":format!("{:x}",Sha256::digest(&initial_bytes)),
            "current_snapshot_sha256":format!("{:x}",Sha256::digest(&current_bytes)),
            "campaign":current.campaign(),"state_revision":current.revision(),
            "prepared_sources":{"source_cohort_sha256":sources.source_cohort_sha256(),
                "decoder_sha256":sources.decoder_sha256(),"counts":sources.counts()},
            "executable_source_sha256":descriptors.source_sha256,"index_cache":store.index_cache_report(),
            "initial_world_dropped":true,"strict_current_restore":true,"canonical_state_unchanged":true,
            "event_acknowledged":false,"faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        plan,
        outcome,
    };
    // Source proof and query contributions stay borrowed until the complete
    // actual pretty report plus emitter newline fits. No partial report exists.
    let mut admitted = BoundedJson {
        bytes: Vec::new(),
        maximum: request.maximum_report_bytes,
    };
    serde_json::to_writer_pretty(&mut admitted, &report)?;
    admitted.write_all(b"\n")?;
    Ok(serde_json::from_slice(&admitted.bytes)?)
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
