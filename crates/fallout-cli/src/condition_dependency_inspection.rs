//! Load winning condition fields and their immutable dependencies. Record
//! ownership stays attached; this scan does not merge unrelated AND/OR lists.
use super::{Result, command_catalogue, inspection_input::Order};
use fallout_data::{
    condition_census,
    condition_operands::{self, Parameter, Signature, Signatures},
    identity::FormKey,
    loaded_scripts, plugin, record_metadata,
};
use fallout_runtime::{
    execution::condition as condition_query, foreign::Content, identity::ReferenceId,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

const MAXIMUM_CANDIDATES: usize = 262_144;
const MAXIMUM_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAXIMUM_CONDITIONS: usize = 1_000_000;
const MAXIMUM_RETAINED_BYTES: usize = 128 * 1024 * 1024;
const MAXIMUM_QUERY_INPUT_BYTES: usize = 16 * 1024;
const MAXIMUM_QUERY_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_QUERY_CONTRIBUTIONS: usize = 65_536;

fn bounded_input(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = fallout_data::baseline::open_source(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err("Engineering condition input byte budget exceeded".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("Engineering condition input byte budget exceeded".into());
    }
    Ok(bytes)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryInput {
    record: FormKey,
    field_decoded_offset: usize,
    explicit_subject: u64,
    snapshot: PathBuf,
}
impl QueryInput {
    fn read(path: &Path) -> Result<Self> {
        let mut input: Self =
            serde_json::from_slice(&bounded_input(path, MAXIMUM_QUERY_INPUT_BYTES)?)?;
        if !input.snapshot.is_absolute() {
            input.snapshot = path
                .parent()
                .ok_or("Condition query input has no parent")?
                .join(input.snapshot);
        }
        Ok(input)
    }
}

struct QueryContext {
    input: QueryInput,
    state: SnapshotContext,
}

struct SnapshotContext {
    subject: Option<ReferenceId>,
    world: fallout_runtime::World<'static>,
    content: Content,
    snapshot_sha256: String,
}
impl SnapshotContext {
    fn load(
        snapshot: &Path,
        explicit_subject: Option<u64>,
        store: &mut fallout_data::store::RecordStore,
    ) -> Result<Self> {
        let source_catalogue = Arc::new(loaded_scripts::Catalogue::load(
            store,
            Default::default(),
            |_, _| Ok(()),
        )?);
        let content = Content::load(store, &source_catalogue, 1_000_000)?;
        let limits = fallout_runtime::Limits {
            max_snapshot_bytes: MAXIMUM_QUERY_SNAPSHOT_BYTES,
            ..Default::default()
        };
        let bytes = bounded_input(snapshot, limits.max_snapshot_bytes)?;
        let world = fallout_runtime::World::restore(
            source_catalogue,
            fallout_runtime::snapshot::Snapshot::decode(&bytes, limits)?,
            limits,
        )?;
        let subject = explicit_subject
            .map(|id| std::num::NonZeroU64::try_from(id).map(ReferenceId))
            .transpose()?;
        let snapshot_sha256 = format!(
            "{:x}",
            Sha256::digest(world.snapshot().encode(limits.max_snapshot_bytes)?)
        );
        Ok(Self {
            subject,
            world,
            content,
            snapshot_sha256,
        })
    }

    fn verify_unchanged(&self) -> Result<()> {
        let after = self.world.snapshot().encode(MAXIMUM_QUERY_SNAPSHOT_BYTES)?;
        if format!("{:x}", Sha256::digest(after)) != self.snapshot_sha256 {
            return Err("Condition observation changed canonical state".into());
        }
        Ok(())
    }
}

impl QueryContext {
    fn load(input: QueryInput, store: &mut fallout_data::store::RecordStore) -> Result<Self> {
        let state = SnapshotContext::load(&input.snapshot, Some(input.explicit_subject), store)?;
        Ok(Self { input, state })
    }

    fn observe(
        &self,
        record: &condition_operands::PreparedRecord,
        maximum: usize,
    ) -> Result<Value> {
        let request = condition_query::Request::prepare(
            &self.state.world,
            record,
            self.input.field_decoded_offset,
        )?;
        #[derive(serde::Serialize)]
        struct Reports<'a> {
            faithful: condition_query::Observation<'a>,
            engineering: condition_query::Observation<'a>,
            canonical_snapshot_sha256: &'a str,
            canonical_state_unchanged: bool,
        }
        let reports = Reports {
            faithful: request.observe(
                &self.state.world,
                &self.state.content,
                self.state.subject,
                condition_query::Intent::Faithful,
                MAXIMUM_QUERY_CONTRIBUTIONS,
            )?,
            engineering: request.observe(
                &self.state.world,
                &self.state.content,
                self.state.subject,
                condition_query::Intent::EngineeringObservation,
                MAXIMUM_QUERY_CONTRIBUTIONS,
            )?,
            canonical_snapshot_sha256: &self.state.snapshot_sha256,
            canonical_state_unchanged: true,
        };
        self.state.verify_unchanged()?;
        let mut admission = Admission { bytes: 0, maximum };
        serde_json::to_writer(&mut admission, &reports)?;
        Ok(serde_json::to_value(reports)?)
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchInput {
    schema_version: u32,
    record: FormKey,
    field_decoded_offsets: Vec<usize>,
    explicit_subject: u64,
    snapshot: PathBuf,
    maximum_source_receipt_bytes: usize,
    maximum_site_comparisons: usize,
    maximum_contributions: usize,
}
impl BatchInput {
    fn read(path: &Path) -> Result<Self> {
        let mut input: Self =
            serde_json::from_slice(&bounded_input(path, MAXIMUM_QUERY_INPUT_BYTES)?)?;
        if input.schema_version != 1
            || input.field_decoded_offsets.is_empty()
            || input.field_decoded_offsets.len() > 4096
            || input.maximum_source_receipt_bytes > 1024 * 1024
            || input.maximum_site_comparisons > 1_048_576
            || input.maximum_contributions > MAXIMUM_QUERY_CONTRIBUTIONS
        {
            return Err("invalid condition batch request schema, selection or budget".into());
        }
        if !input.snapshot.is_absolute() {
            input.snapshot = path
                .parent()
                .ok_or("Condition batch input has no parent")?
                .join(input.snapshot);
        }
        Ok(input)
    }
}
struct BatchContext {
    input: BatchInput,
    state: SnapshotContext,
}
impl BatchContext {
    fn load(input: BatchInput, store: &mut fallout_data::store::RecordStore) -> Result<Self> {
        let state = SnapshotContext::load(&input.snapshot, Some(input.explicit_subject), store)?;
        Ok(Self { input, state })
    }
    fn observe(
        &self,
        record: &condition_operands::PreparedRecord,
        maximum: usize,
    ) -> Result<Value> {
        let requests = condition_query::Requests::prepare(
            &self.state.world,
            record,
            &self.input.field_decoded_offsets,
            condition_query::BatchLimits {
                maximum_requests: 4096,
                maximum_source_receipt_bytes: self.input.maximum_source_receipt_bytes,
                maximum_site_comparisons: self.input.maximum_site_comparisons,
            },
        )?;
        #[derive(serde::Serialize)]
        struct Reports<'a> {
            preparation: condition_query::BatchCounts,
            faithful: Vec<condition_query::Observation<'a>>,
            engineering: Vec<condition_query::Observation<'a>>,
            canonical_snapshot_sha256: &'a str,
            canonical_state_unchanged: bool,
        }
        let reports = Reports {
            preparation: requests.counts(),
            faithful: requests.observe(
                &self.state.world,
                &self.state.content,
                self.state.subject,
                condition_query::Intent::Faithful,
                0,
            )?,
            engineering: requests.observe(
                &self.state.world,
                &self.state.content,
                self.state.subject,
                condition_query::Intent::EngineeringObservation,
                self.input.maximum_contributions,
            )?,
            canonical_snapshot_sha256: &self.state.snapshot_sha256,
            canonical_state_unchanged: true,
        };
        self.state.verify_unchanged()?;
        let mut admission = Admission { bytes: 0, maximum };
        serde_json::to_writer(&mut admission, &reports)?;
        Ok(serde_json::to_value(reports)?)
    }
}

fn bounded_candidates<T>(candidates: impl Iterator<Item = T>, maximum: usize) -> Result<Vec<T>> {
    let mut admitted = Vec::new();
    for candidate in candidates {
        if admitted.len() >= maximum {
            return Err("condition candidate budget exceeded".into());
        }
        admitted.push(candidate);
    }
    Ok(admitted)
}

struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "condition retained-report byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn admit_record(
    records: &mut Vec<Value>,
    row: Value,
    retained: &mut usize,
    maximum: usize,
) -> Result<()> {
    let mut admission = Admission {
        bytes: 0,
        maximum: maximum.saturating_sub(*retained),
    };
    serde_json::to_writer(&mut admission, &row)?;
    *retained += admission.bytes;
    records.push(row);
    Ok(())
}

fn attach_owners(row: &mut Value, owners: &impl serde::Serialize, maximum: usize) -> Result<()> {
    let mut base = Admission { bytes: 0, maximum };
    serde_json::to_writer(&mut base, &*row)?;
    let overhead = b",\"source_owners\":".len();
    let remaining = maximum
        .checked_sub(base.bytes)
        .and_then(|bytes| bytes.checked_sub(overhead))
        .ok_or("condition retained-report byte budget exceeded")?;
    let mut admission = Admission {
        bytes: 0,
        maximum: remaining,
    };
    serde_json::to_writer(&mut admission, owners)?;
    // Validate complete aggregate capacity before materializing another JSON tree.
    row["source_owners"] = serde_json::to_value(owners)?;
    Ok(())
}

fn attach_owners_and_runs(
    row: &mut Value,
    owners: &impl serde::Serialize,
    runs: &impl serde::Serialize,
    maximum: usize,
) -> Result<()> {
    let mut base = Admission { bytes: 0, maximum };
    serde_json::to_writer(&mut base, &*row)?;
    let overhead = b",\"source_owners\":".len() + b",\"source_runs\":".len();
    let remaining = maximum
        .checked_sub(base.bytes)
        .and_then(|bytes| bytes.checked_sub(overhead))
        .ok_or("condition retained-report byte budget exceeded")?;
    let mut admission = Admission {
        bytes: 0,
        maximum: remaining,
    };
    serde_json::to_writer(&mut admission, owners)?;
    serde_json::to_writer(&mut admission, runs)?;
    row["source_owners"] = serde_json::to_value(owners)?;
    row["source_runs"] = serde_json::to_value(runs)?;
    Ok(())
}

enum Prepared {
    Conditions(condition_operands::PreparedRecord),
    Owners(condition_operands::PreparedOwnerRecord),
}
impl Prepared {
    fn conditions(&self) -> &condition_operands::PreparedRecord {
        match self {
            Self::Conditions(value) => value,
            Self::Owners(value) => value.conditions(),
        }
    }
    fn owners(&self) -> Option<&condition_operands::SourceOwners> {
        match self {
            Self::Conditions(_) => None,
            Self::Owners(value) => Some(value.ownership()),
        }
    }
}

fn increment(counts: &mut Value, map: &str, key: &str) {
    let count = counts[map][key].as_u64().unwrap_or(0);
    counts[map][key] = (count + 1).into();
}
fn number(counts: &mut Value, key: &str, amount: u64) {
    counts[key] = (counts[key].as_u64().expect("initialized condition count") + amount).into();
}
fn label(value: impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .ok_or("Missing enum label")?
        .to_string())
}

fn query_signatures(catalogue: &command_catalogue::Catalogue) -> Signatures {
    catalogue
        .script_commands
        .iter()
        .filter(|row| row.condition_handler_present)
        .map(|row| {
            (
                (row.id - 0x1000) as u16,
                Signature {
                    parameters: row
                        .parameters
                        .iter()
                        .map(|parameter| Parameter {
                            type_id: parameter.type_id,
                            optional_word: parameter.optional_word,
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

fn explicit_subject<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<ReferenceId>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordQuerySelection {
    record: FormKey,
    field_decoded_offset: usize,
    #[serde(deserialize_with = "explicit_subject")]
    explicit_subject: Option<ReferenceId>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecordQueryIntent {
    Faithful,
    EngineeringObservation,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordQueryInput {
    schema_version: u32,
    intent: RecordQueryIntent,
    selections: Vec<RecordQuerySelection>,
    snapshot: PathBuf,
    maximum_records: usize,
    maximum_source_bytes: usize,
    maximum_record_fields: usize,
    maximum_record_sites: usize,
    maximum_retained_source_bytes: usize,
    maximum_requests: usize,
    maximum_source_receipt_bytes: usize,
    maximum_receipt_comparisons: usize,
    maximum_site_comparisons: usize,
    maximum_query_variable_bytes: usize,
    maximum_contributions: usize,
    maximum_observation_bytes: usize,
    maximum_report_bytes: usize,
}

pub(super) fn inspect_records(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    report_path: Option<&Path>,
) -> Result<Value> {
    let mut input: RecordQueryInput =
        serde_json::from_slice(&bounded_input(request_path, MAXIMUM_QUERY_INPUT_BYTES)?)?;
    let defaults = condition_query::CrossLimits::default();
    let observation_defaults = condition_query::CrossObservationLimits::default();
    if input.schema_version != 1
        || input.selections.is_empty()
        || input.selections.len() > input.maximum_requests
        || input.maximum_records > defaults.maximum_records
        || input.maximum_source_bytes > defaults.maximum_source_bytes
        || input.maximum_record_fields > defaults.maximum_record_fields
        || input.maximum_record_sites > defaults.maximum_record_sites
        || input.maximum_retained_source_bytes > defaults.maximum_retained_source_bytes
        || input.maximum_requests > defaults.maximum_requests
        || input.maximum_source_receipt_bytes > defaults.maximum_source_receipt_bytes
        || input.maximum_receipt_comparisons > defaults.maximum_receipt_comparisons
        || input.maximum_site_comparisons > defaults.maximum_site_comparisons
        || input.maximum_query_variable_bytes > defaults.maximum_query_variable_bytes
        || input.maximum_contributions > observation_defaults.maximum_contributions
        || input.maximum_observation_bytes > observation_defaults.maximum_observation_bytes
        || input.maximum_report_bytes == 0
        || input.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("invalid condition record batch schema, selection or budget ceiling".into());
    }
    if let Some(report) = report_path {
        if report.try_exists()? {
            return Err("condition record batch report must be a fresh artifact".into());
        }
        let parent = report
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(super::protected_tree(install)?) {
            return Err("condition record batch report must be outside the installation".into());
        }
    }
    if !input.snapshot.is_absolute() {
        input.snapshot = request_path
            .parent()
            .ok_or("Condition record batch request has no parent")?
            .join(input.snapshot);
    }
    let mut unique = BTreeMap::new();
    let mut keys = Vec::new();
    let mut selections = Vec::with_capacity(input.selections.len());
    for selection in &input.selections {
        let index = if let Some(&index) = unique.get(&selection.record) {
            index
        } else {
            if keys.len() >= input.maximum_records {
                return Err("condition record count budget exceeded".into());
            }
            let index = keys.len();
            keys.push(&selection.record);
            unique.insert(&selection.record, index);
            index
        };
        selections.push(condition_query::RecordSelection {
            record_index: index,
            field_decoded_offset: selection.field_decoded_offset,
            explicit_subject: selection.explicit_subject,
        });
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let signatures = query_signatures(&descriptors);
    let state = SnapshotContext::load(&input.snapshot, None, &mut store)?;
    let mut records = Vec::with_capacity(keys.len());
    let mut decoded = 0;
    let mut fields = 0;
    let mut sites = 0;
    let mut retained = 0;
    for key in keys {
        let location = store
            .winner(key)
            .ok_or("condition record selection has no winner")?;
        if !condition_census::condition_record(store.definition(location).header.kind) {
            return Err("condition record selection kind is unsupported".into());
        }
        let record = condition_operands::prepare_record(
            &mut store,
            location,
            &signatures,
            condition_operands::RecordLimits {
                maximum_decoded_bytes: input.maximum_source_bytes - decoded,
                maximum_fields: input.maximum_record_fields - fields,
                maximum_conditions: input.maximum_record_sites - sites,
                maximum_retained_bytes: input.maximum_retained_source_bytes - retained,
            },
        )?;
        decoded += record.identity().decoded_bytes;
        fields += record.fields();
        sites += record.sites().len();
        retained += record.retained_bytes();
        records.push(record);
    }
    let records: Vec<_> = records.iter().collect();
    let requests = condition_query::CrossRequests::prepare(
        &state.world,
        &records,
        &selections,
        condition_query::CrossLimits {
            maximum_records: input.maximum_records,
            maximum_source_bytes: input.maximum_source_bytes,
            maximum_record_fields: input.maximum_record_fields,
            maximum_record_sites: input.maximum_record_sites,
            maximum_retained_source_bytes: input.maximum_retained_source_bytes,
            maximum_requests: input.maximum_requests,
            maximum_source_receipt_bytes: input.maximum_source_receipt_bytes,
            maximum_receipt_comparisons: input.maximum_receipt_comparisons,
            maximum_site_comparisons: input.maximum_site_comparisons,
            maximum_query_variable_bytes: input.maximum_query_variable_bytes,
        },
    )?;
    let intent = match input.intent {
        RecordQueryIntent::Faithful => condition_query::Intent::Faithful,
        RecordQueryIntent::EngineeringObservation => {
            condition_query::Intent::EngineeringObservation
        }
    };
    let observations = requests.observe(
        &state.world,
        &state.content,
        intent,
        condition_query::CrossObservationLimits {
            maximum_contributions: input.maximum_contributions,
            maximum_observation_bytes: input.maximum_observation_bytes,
        },
    )?;
    state.verify_unchanged()?;
    #[derive(serde::Serialize)]
    struct Report<'a> {
        #[serde(flatten)]
        metadata: Value,
        preparation: condition_query::CrossCounts,
        observations: Vec<condition_query::Observation<'a>>,
        sources: &'a [fallout_data::store::SourceReceipt],
    }
    let report = Report {
        metadata: json!({"schema_version":1,"scope":"Explicit ordered engineering condition queries across exact source records; no truth or grouping",
            "canonical_snapshot_sha256":state.snapshot_sha256,"canonical_state_unchanged":true,
            "explicit_load_order":order.names,"load_order_sha256":order.sha256,
            "executable_sha256":descriptors.source_sha256,"source_cohort_sha256":state.world.catalogue_fingerprint(),
            "index_cache":store.index_cache_report(),"campaign":state.world.campaign(),"state_revision":state.world.revision(),
            "event_acknowledged":false,"group_evaluation_verified":false,"condition_evaluation_ready":false,
            "faithful_execution_admitted":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
        preparation: requests.counts(),
        observations,
        sources: &state.world.catalogue().sources,
    };
    let mut admission = Admission {
        bytes: 0,
        maximum: input.maximum_report_bytes,
    };
    serde_json::to_writer_pretty(&mut admission, &report)?;
    admission.write_all(b"\n")?;
    Ok(serde_json::to_value(report)?)
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    include_source_owners: bool,
    include_source_runs: bool,
    engineering_query_input: Option<&Path>,
    engineering_query_batch: Option<&Path>,
) -> Result<Value> {
    let include_source_owners = include_source_owners || include_source_runs;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let sources = store.source_receipts()?;
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let signatures = query_signatures(&catalogue);
    let query = engineering_query_input
        .map(QueryInput::read)
        .transpose()?
        .map(|input| QueryContext::load(input, &mut store))
        .transpose()?;
    let batch = engineering_query_batch
        .map(BatchInput::read)
        .transpose()?
        .map(|input| BatchContext::load(input, &mut store))
        .transpose()?;
    let mut query_report = None;
    let mut batch_report = None;
    let candidates = bounded_candidates(
        store
            .winning_definitions()
            .filter(|(_, location)| {
                condition_census::condition_record(store.definition(*location).header.kind)
            })
            .map(|(_, location)| location),
        MAXIMUM_CANDIDATES,
    )?;
    let mut counts = json!({"candidate_records":candidates.len(),"deleted_candidate_records":0,"decoded_candidate_bytes":0,
        "records_with_conditions":0,"conditions":0,"source_findings":0,"unused_nonzero_words":0,"unknown_parameters":0,
        "variable_indices_need_live_state":0,"signature_statuses":{},"parameter_kinds":{},"parameter_domains":{},"subjects":{},"form_statuses":{}});
    let mut records = Vec::new();
    let mut retained = 0;
    let mut owner_counts = json!({"records":0,"mapped_records":0,"unmapped_records":0,"orphan_conditions":0,"unmapped_conditions":0,
        "sections":0,"source_lists":0,"source_findings":0,"owner_kinds":{}});
    let mut run_counts = json!({"records":0,"runs":0,"conditions_in_runs":0,"orphan_conditions":0,
        "unmapped_conditions":0,"true_tail_runs":0,"end_reasons":{}});
    for location in candidates {
        if store.definition(location).header.flags & plugin::DELETED != 0 {
            number(&mut counts, "deleted_candidate_records", 1);
            continue;
        }
        let limits = condition_operands::RecordLimits {
            maximum_decoded_bytes: (MAXIMUM_DECODED_BYTES
                - counts["decoded_candidate_bytes"]
                    .as_u64()
                    .expect("byte count") as usize)
                .min(64 * 1024 * 1024),
            maximum_conditions: MAXIMUM_CONDITIONS
                - counts["conditions"].as_u64().expect("condition count") as usize,
            maximum_retained_bytes: MAXIMUM_RETAINED_BYTES - retained,
            ..condition_operands::RecordLimits::default()
        };
        let admission = if include_source_owners {
            Prepared::Owners(condition_operands::prepare_record_with_owners(
                &mut store,
                location,
                &signatures,
                limits,
                condition_operands::OwnerLimits {
                    maximum_retained_bytes: MAXIMUM_RETAINED_BYTES - retained,
                    ..condition_operands::OwnerLimits::default()
                },
            )?)
        } else {
            Prepared::Conditions(condition_operands::prepare_record(
                &mut store,
                location,
                &signatures,
                limits,
            )?)
        };
        let prepared = admission.conditions();
        let identity = prepared.identity();
        if let Some(query) = &query
            && identity.key == query.input.record
        {
            query_report = Some(query.observe(prepared, MAXIMUM_RETAINED_BYTES - retained)?);
        }
        if let Some(batch) = &batch
            && identity.key == batch.input.record
        {
            batch_report = Some(batch.observe(prepared, MAXIMUM_RETAINED_BYTES - retained)?);
        }
        number(
            &mut counts,
            "decoded_candidate_bytes",
            identity.decoded_bytes as u64,
        );
        let mut rows = Vec::new();
        for site in prepared.sites() {
            let bound = site.binding();
            let binding = serde_json::to_value(bound)?;
            number(&mut counts, "conditions", 1);
            increment(
                &mut counts,
                "signature_statuses",
                binding["signature_status"].as_str().expect("status label"),
            );
            increment(
                &mut counts,
                "subjects",
                binding["subject"]["kind"].as_str().expect("subject label"),
            );
            for operand in binding["operands"].as_array().expect("two operands") {
                let kind = operand["value"]["kind"].as_str().expect("parameter label");
                increment(&mut counts, "parameter_kinds", kind);
                if let Some(domain) = operand["value"]["domain"].as_str() {
                    increment(&mut counts, "parameter_domains", domain);
                }
                if kind == "unused" && operand["value"]["raw_word"] != 0 {
                    number(&mut counts, "unused_nonzero_words", 1);
                }
                if kind == "unknown" {
                    number(&mut counts, "unknown_parameters", 1);
                }
                if kind == "variable_index" {
                    number(&mut counts, "variable_indices_need_live_state", 1);
                }
            }
            for dependency in bound
                .operands
                .iter()
                .filter_map(|p| p.form_dependency.as_ref())
                .chain(bound.comparison_global.iter())
                .chain(bound.subject_reference.iter())
            {
                let status = label(dependency.status)?;
                increment(&mut counts, "form_statuses", &status);
            }
            number(
                &mut counts,
                "source_findings",
                site.source_findings().len() as u64,
            );
            rows.push(serde_json::to_value(site.legacy_row())?);
        }
        if !rows.is_empty() {
            number(&mut counts, "records_with_conditions", 1);
            let mut row = json!({"key":identity.key,"source_name":identity.source_name,"record_kind":identity.record_kind,
                "record_file_offset":identity.record_file_offset,"record_flags":identity.record_flags,"decoded_bytes":identity.decoded_bytes,
                "decoded_sha256":identity.decoded_sha256,"binding_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&rows)?)),"conditions":rows});
            if let Some(owners) = admission.owners() {
                if include_source_runs {
                    let Prepared::Owners(prepared) = &admission else {
                        unreachable!("owner preparation required")
                    };
                    let runs = condition_operands::prepare_source_runs(
                        prepared,
                        condition_operands::RunLimits {
                            maximum_retained_bytes: MAXIMUM_RETAINED_BYTES - retained,
                            ..condition_operands::RunLimits::default()
                        },
                    )?;
                    attach_owners_and_runs(
                        &mut row,
                        owners,
                        &runs,
                        MAXIMUM_RETAINED_BYTES - retained,
                    )?;
                    number(&mut run_counts, "records", 1);
                    number(&mut run_counts, "runs", runs.runs().len() as u64);
                    number(
                        &mut run_counts,
                        "orphan_conditions",
                        runs.orphan_sites() as u64,
                    );
                    number(
                        &mut run_counts,
                        "unmapped_conditions",
                        runs.unmapped_sites() as u64,
                    );
                    for run in runs.runs() {
                        number(
                            &mut run_counts,
                            "conditions_in_runs",
                            (run.end_site_exclusive - run.first_site) as u64,
                        );
                        number(
                            &mut run_counts,
                            "true_tail_runs",
                            u64::from(run.tail_or_flag),
                        );
                        increment(&mut run_counts, "end_reasons", &label(run.end_reason)?);
                    }
                } else {
                    attach_owners(&mut row, owners, MAXIMUM_RETAINED_BYTES - retained)?;
                }
                number(&mut owner_counts, "records", 1);
                let mapped =
                    owners.status() == condition_operands::OwnerStatus::MappedNarrativeSource;
                number(
                    &mut owner_counts,
                    if mapped {
                        "mapped_records"
                    } else {
                        "unmapped_records"
                    },
                    1,
                );
                number(
                    &mut owner_counts,
                    "sections",
                    owners.sections().len() as u64,
                );
                number(
                    &mut owner_counts,
                    "source_lists",
                    owners.source_lists().len() as u64,
                );
                number(
                    &mut owner_counts,
                    "source_findings",
                    owners.findings().len() as u64,
                );
                for site in owners.sites() {
                    if let Some(section) = site.owner_section {
                        increment(
                            &mut owner_counts,
                            "owner_kinds",
                            &label(owners.sections()[section].kind)?,
                        );
                    } else {
                        number(
                            &mut owner_counts,
                            if mapped {
                                "orphan_conditions"
                            } else {
                                "unmapped_conditions"
                            },
                            1,
                        );
                    }
                }
            }
            admit_record(&mut records, row, &mut retained, MAXIMUM_RETAINED_BYTES)?;
        }
    }
    let mut report = json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "metadata":metadata,"sources":sources,"executable_sha256":catalogue.source_sha256,"counts":counts,"records":records,
        "index_cache":store.index_cache_report(),"target_kind_acceptance_checked":false,"live_values_resolved":false,"evaluation_ready":false,"retail_parity_accepted":false});
    if include_source_owners {
        report["schema_version"] = 2.into();
        report["source_owner_counts"] = owner_counts;
        report["group_evaluation_verified"] = false.into();
        report["default_subjects_applied"] = false.into();
    }
    if include_source_runs {
        report["schema_version"] = 3.into();
        report["source_run_counts"] = run_counts;
    }
    if query.is_some() {
        let query_report = query_report
            .ok_or("Engineering condition record has no nondeleted admitted candidate")?;
        let mut admission = Admission {
            bytes: 0,
            maximum: MAXIMUM_RETAINED_BYTES,
        };
        serde_json::to_writer(&mut admission, &report)?;
        admission.write_all(b",\"engineering_query\":")?;
        serde_json::to_writer(&mut admission, &query_report)?;
        report["schema_version"] = 4.into();
        report["engineering_query"] = query_report;
    }
    if batch.is_some() {
        let batch_report = batch_report
            .ok_or("Engineering condition batch record has no nondeleted admitted candidate")?;
        let mut admission = Admission {
            bytes: 0,
            maximum: MAXIMUM_RETAINED_BYTES,
        };
        serde_json::to_writer(&mut admission, &report)?;
        admission.write_all(b",\"engineering_batch\":")?;
        serde_json::to_writer(&mut admission, &batch_report)?;
        report["schema_version"] = 5.into();
        report["engineering_batch"] = batch_report;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_overflow_stops_before_retention_or_unbounded_iteration() {
        let visits = std::cell::Cell::new(0);
        let input = (0..usize::MAX).inspect(|_| visits.set(visits.get() + 1));
        assert!(
            bounded_candidates(input, 2)
                .unwrap_err()
                .to_string()
                .contains("candidate budget")
        );
        assert_eq!(visits.get(), 3);
        assert_eq!(bounded_candidates(0..2, 2).unwrap(), vec![0, 1]);
        assert!(bounded_candidates(0..1, 0).is_err());
    }

    #[test]
    fn aggregate_admission_counts_escaped_utf8_bytes_and_is_atomic() {
        let row = json!({"source":"é\n\"\\", "conditions":[{"raw":0xffffffff_u32}]});
        let bytes = serde_json::to_vec(&row).unwrap().len();
        let mut records = Vec::new();
        let mut retained = 0;
        assert!(
            admit_record(&mut records, row.clone(), &mut retained, bytes - 1)
                .unwrap_err()
                .to_string()
                .contains("retained-report byte budget")
        );
        assert!(records.is_empty());
        assert_eq!(retained, 0);
        admit_record(&mut records, row.clone(), &mut retained, bytes * 2 - 1).unwrap();
        assert!(admit_record(&mut records, row, &mut retained, bytes * 2 - 1).is_err());
        assert_eq!(records.len(), 1);
        assert_eq!(retained, bytes);
    }

    #[test]
    fn owner_tree_is_not_materialized_until_the_complete_record_fits() {
        let original = json!({"conditions":[{"bits":0xffffffff_u32}]});
        let owners = json!({"sections":[{"name":"é\n\"\\"}],"owner_section":null});
        let mut expected = original.clone();
        expected["source_owners"] = owners.clone();
        let exact = serde_json::to_vec(&expected).unwrap().len();
        let mut row = original.clone();
        assert!(
            attach_owners(&mut row, &owners, exact - 1)
                .unwrap_err()
                .to_string()
                .contains("retained-report byte budget")
        );
        assert_eq!(row, original);
        attach_owners(&mut row, &owners, exact).unwrap();
        assert_eq!(row, expected);
    }

    #[test]
    fn combined_owner_run_admission_checks_both_trees_before_mutating_the_row() {
        let original = json!({"conditions":[{"bits":0xffffffff_u32}]});
        let owners = json!({"name":"é\n\"\\"});
        let runs = json!({"runs":[{"tail_or_flag":true,"name":"é\n\"\\"}]});
        let mut expected = original.clone();
        expected["source_owners"] = owners.clone();
        expected["source_runs"] = runs.clone();
        let bytes = serde_json::to_vec(&expected).unwrap().len();
        let mut row = original.clone();
        assert!(attach_owners_and_runs(&mut row, &owners, &runs, bytes - 1).is_err());
        assert_eq!(row, original);
        attach_owners_and_runs(&mut row, &owners, &runs, bytes).unwrap();
        assert_eq!(row, expected);
    }
}
