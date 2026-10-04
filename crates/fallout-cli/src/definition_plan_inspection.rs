//! Join immutable winning source handles, structural plans and owning tables.
use super::{Result, command_catalogue, inspection_input::Order, protected_tree, script_profile};
use fallout_data::{
    baseline, loaded_scripts,
    obscript::{
        control_flow_bundle::Structure, definition_plan, expression_census, expression_plan,
        operand_binding,
    },
};
use fallout_runtime::{
    execution::admission,
    programs::{LookupError, PreparationJob, PreparationStatus, PreparedSources, StepBudget},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::Path,
};

pub(super) fn finding(error: impl std::borrow::Borrow<definition_plan::Error>) -> Value {
    use definition_plan::Error as E;
    match error.borrow() {
        E::MissingBody => json!({"kind":"absent_compiled_field"}),
        E::SourceMetadata(issues) => json!({"kind":"source_metadata","issues":issues}),
        E::Control(fallout_data::obscript::control_flow::Error::Structure(issue)) => {
            json!({"kind":"control_structure","issue":issue})
        }
        E::ExpressionPlan {
            instruction_offset,
            source,
        } => {
            json!({"kind":"expression_structure","instruction_scda_offset":instruction_offset,"issue":source.diagnostic()})
        }
        other => json!({"kind":"preparation_failure","reason":other.to_string()}),
    }
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    bundle_path: Option<&Path>,
    admission_path: Option<&Path>,
    cooperative_path: Option<&Path>,
    selected_path: Option<&Path>,
) -> Result<Value> {
    if [admission_path, bundle_path, cooperative_path, selected_path]
        .iter()
        .filter(|path| path.is_some())
        .count()
        > 1
    {
        return Err(
            "execution admission, cooperative/selected preparation and comparison bundle are separate source-plan requests".into(),
        );
    }
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let provenance = || {
        json!({
            "explicit_load_order": order.names,
            "load_order_sha256": order.sha256,
            "executable_source_sha256": descriptors.source_sha256,
            "index_cache": store.index_cache_report(),
        })
    };
    if let Some(path) = selected_path {
        return selected(path, &catalogue, &model, &signatures, provenance());
    }
    if let Some(path) = cooperative_path {
        return cooperative(path, &catalogue, &model, &signatures, provenance());
    }
    if let Some(path) = admission_path {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err("execution admission request byte budget exceeded".into());
        }
        let request: admission::Request = serde_json::from_slice(&bytes)?;
        let sources = PreparedSources::load(&catalogue, &model, &signatures, Default::default())?;
        let attachments = fallout_data::quest_scripts::Attachments::load(
            &mut store,
            &catalogue,
            131_072,
            |_, _| Ok(()),
        )?;
        let report = request.check(&sources, &attachments, Default::default())?;
        return Ok(json!({
            "schema_version": 1,
            "scope": "bounded_declared_source_dependencies_and_unverified_execution_capabilities",
            "request_sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
            "explicit_load_order": order.names,
            "load_order_sha256": order.sha256,
            "executable_source_sha256": descriptors.source_sha256,
            "execution_admission": report,
            "index_cache": store.index_cache_report(),
            "retail_parity_accepted": false,
        }));
    }
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("winning script bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FROBS001")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut rows = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut compiled = 0;
    let mut bundle_bytes = 8;
    let mut total_expressions = 0;
    let mut total_tokens = 0;
    let mut total_nodes = 0;
    let mut total_uses = 0;
    for (_, script) in catalogue.iter() {
        let body_index = if let Some(bytes) = script.compiled() {
            let index = compiled;
            compiled += 1;
            bundle_bytes += 4 + bytes.len();
            if compiled > 65_536 || bundle_bytes > 66 * 1024 * 1024 {
                return Err("winning source bundle budget exceeded".into());
            }
            if let Some(bundle) = &mut bundle {
                bundle.write_all(&(bytes.len() as u32).to_le_bytes())?;
                bundle.write_all(bytes)?;
            }
            Some(index)
        } else {
            None
        };
        let (prepared, issue) = match definition_plan::prepare(
            &catalogue,
            script.handle(),
            &model,
            &signatures,
            definition_plan::Limits::default(),
        ) {
            Ok(plan) => {
                total_expressions += plan.statements().len();
                total_tokens += plan.tokens();
                total_nodes += plan.nodes();
                total_uses += plan.bindings().uses.len();
                if total_expressions > 262_144
                    || total_tokens > 2_000_000
                    || total_nodes > 2_000_000
                    || total_uses > 1_000_000
                {
                    return Err("winning prepared-plan aggregate budget exceeded".into());
                }
                *counts
                    .entry("prepared_source_structure".into())
                    .or_default() += 1;
                let statements = plan.statements().iter().map(|s| {
                    let instruction = &plan.control().instructions()[s.instruction()];
                    json!({"instruction_scda_offset":instruction.bytes.start,"opcode":instruction.opcode,
                        "expression_operand_offset":s.expression_scda_offset()-instruction.operand_offset,
                        "expression_sha256":format!("{:x}",sha2::Sha256::digest(s.plan().source_bytes())),
                        "token_sha256":expression_census::token_digest(s.plan().tokens()),
                        "plan":{"shape_sha256":s.plan().shape_sha256(),"nodes":s.plan().nodes().len(),"root":s.plan().root(),
                            "maximum_stack":s.plan().maximum_stack(),"height":s.plan().height()},"issue":null})
                }).collect::<Vec<_>>();
                let control = Structure {
                    events: plan.control().events().to_vec(),
                    arms: plan.control().arms().to_vec(),
                    links: plan.control().links().to_vec(),
                    maximum_depth: plan.control().maximum_depth(),
                };
                (
                    Some(
                        json!({"instructions":plan.control().instructions().len(),"control":control,"statements":statements,
                    "binding_counts":plan.bindings().counts,"binding_sha256":operand_binding::digest(&plan.bindings().uses)}),
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
                            .ok_or("Missing source finding kind")?
                            .to_owned(),
                    )
                    .or_default() += 1;
                (None, Some(issue))
            }
        };
        rows.push(
            json!({"handle":script.handle(),"version":script.version(),"owner":script.owner(),
            "compiled_bundle_index":body_index,"prepared":prepared,"finding":issue}),
        );
    }
    let bundle_receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("created bundle path"))?;
        Some(json!({"format":"FROBS001","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Winning source versions with complete delimiter/expression plans and encoded owning-table associations; no VM execution permission",
        "explicit_load_order":order.names,"load_order_sha256":order.sha256,"source_cohort_sha256":catalogue.winning_content_sha256(),
        "sources":catalogue.sources,"source_counts":catalogue.counts,"counts":counts,"compiled_bodies":compiled,
        "prepared_expressions":total_expressions,"prepared_tokens":total_tokens,"prepared_nodes":total_nodes,"prepared_operand_uses":total_uses,
        "executable_source_sha256":descriptors.source_sha256,"operator_descriptors":descriptors.operators,
        "definitions":rows,"comparison_bundle":bundle_receipt,"index_cache":store.index_cache_report(),
        "execution_ready":false,"retail_parity_accepted":false}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedRequest {
    schema_version: u32,
    source_cohort_sha256: String,
    definitions: Vec<loaded_scripts::Handle>,
}

fn selected(
    path: &Path,
    catalogue: &loaded_scripts::Catalogue,
    model: &expression_plan::Model<'_>,
    signatures: &fallout_data::obscript::argument_census::Signatures,
    provenance: Value,
) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(path)?.take(65_536 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("selected source request byte budget exceeded".into());
    }
    let request: SelectedRequest = serde_json::from_slice(&bytes)?;
    if request.schema_version != 1 || request.definitions.len() > 128 {
        return Err("invalid selected source request version/definition allowance".into());
    }
    let sources = PreparedSources::load_selected(
        catalogue,
        model,
        signatures,
        &request.definitions,
        Default::default(),
    )?;
    if request.source_cohort_sha256 != sources.source_cohort_sha256() {
        return Err("selected source request has a different source receipt cohort".into());
    }
    let mut rows = Vec::with_capacity(request.definitions.len());
    let mut unavailable = 0;
    for handle in &request.definitions {
        let source = catalogue
            .get_handle(handle)
            .ok_or("validated source disappeared")?;
        let compiled = source.compiled().map(|bytes| {
            json!({
                "bytes": bytes.len(), "sha256": format!("{:x}", sha2::Sha256::digest(bytes)),
            })
        });
        rows.push(match sources.get(handle) {
            Ok(prepared) => json!({
                "handle": handle,
                "status": "prepared_source_structure",
                "compiled": compiled,
                "binding_sha256": prepared.binding_sha256(),
                "instructions": prepared.plan().control().instructions().len(),
                "expressions": prepared.plan().statements().len(),
                "nodes": prepared.plan().nodes(),
                "finding": null,
            }),
            Err(LookupError::Source(error)) => {
                unavailable += 1;
                json!({"handle":handle,"status":"source_finding","compiled":compiled,"finding":finding(error)})
            }
            Err(error) => return Err(error.into()),
        });
    }
    Ok(json!({
        "schema_version": 1,
        "scope": "explicit_selected_immutable_source_preparation",
        "request_sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
        "provenance": provenance,
        "selected_source": {
            "cache": {
                "counts": sources.counts(),
                "source_cohort_sha256": sources.source_cohort_sha256(),
                "decoder_sha256": sources.decoder_sha256(),
            },
            "definitions": rows,
            "unavailable_definitions": unavailable,
            "inferred_dependency_closure": false,
        },
        "execution_ready": false,
        "retail_parity_accepted": false,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CooperativeRequest {
    schema_version: u32,
    maximum_definitions_per_advance: usize,
    maximum_source_bytes_per_advance: usize,
    maximum_advances: usize,
    cancel_after_advances: Option<usize>,
}

fn cooperative(
    path: &Path,
    catalogue: &loaded_scripts::Catalogue,
    model: &expression_plan::Model<'_>,
    signatures: &fallout_data::obscript::argument_census::Signatures,
    provenance: Value,
) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16_384 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("cooperative preparation request byte budget exceeded".into());
    }
    let request: CooperativeRequest = serde_json::from_slice(&bytes)?;
    if request.schema_version != 1
        || request.maximum_advances == 0
        || request.maximum_advances > 4096
        || request.maximum_definitions_per_advance == 0
        || request.maximum_source_bytes_per_advance == 0
        || request
            .cancel_after_advances
            .is_some_and(|count| count > request.maximum_advances)
    {
        return Err("invalid cooperative preparation request/version/budgets".into());
    }
    let mut job = PreparationJob::new(catalogue, model, signatures, Default::default())?;
    let budget = StepBudget {
        maximum_definitions: request.maximum_definitions_per_advance,
        maximum_source_bytes: request.maximum_source_bytes_per_advance,
    };
    let mut advances = Vec::new();
    let mut cache = None;
    let (outcome, failure) = loop {
        if request.cancel_after_advances == Some(advances.len()) {
            drop(job);
            break ("cancelled", None);
        }
        if advances.len() == request.maximum_advances {
            drop(job);
            break ("advance_budget", None);
        }
        let progress = job.advance(budget);
        let status = progress.status;
        let stalled = status == PreparationStatus::Pending && progress.step_definitions == 0;
        advances.push(progress);
        match status {
            PreparationStatus::Complete => {
                let sources = job.finish()?;
                cache = Some(json!({
                    "counts": sources.counts(),
                    "source_cohort_sha256": sources.source_cohort_sha256(),
                    "decoder_sha256": sources.decoder_sha256(),
                }));
                break ("complete", None);
            }
            PreparationStatus::Failed => {
                let error = job.finish().err().ok_or("missing preparation failure")?;
                break ("failed", Some(error.to_string()));
            }
            PreparationStatus::Pending if stalled => {
                drop(job);
                break ("step_budget", None);
            }
            PreparationStatus::Pending => {}
        }
    };
    Ok(json!({
        "schema_version": 1,
        "scope": "cooperative_immutable_source_preparation",
        "request_sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
        "provenance": provenance,
        "cooperative_preparation": {
            "outcome": outcome,
            "failure": failure,
            "advances": advances,
            "cache": cache,
            "prepared_sources_published": cache.is_some(),
            "indivisible_unit": "one_definition_with_existing_source_limits",
            "setup": "bounded_catalogue_and_decoder_identity_before_first_advance",
            "hard_time_slice": false,
        },
        "execution_ready": false,
        "retail_parity_accepted": false,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CooperativeAdmissionRequest {
    schema_version: u32,
    source_cohort_sha256: String,
    roots: Vec<loaded_scripts::Handle>,
    maximum_roots: usize,
    maximum_definitions: usize,
    maximum_dependencies: usize,
    maximum_instructions: usize,
    maximum_operand_uses: usize,
    maximum_frontier: usize,
    maximum_variable_bytes: usize,
    maximum_lookup_source_visits: usize,
    maximum_indivisible_instructions: usize,
    maximum_definition_expansions_per_advance: usize,
    maximum_operand_visits_per_advance: usize,
    maximum_advances: usize,
    #[serde(deserialize_with = "required_optional_count")]
    cancel_after_advances: Option<usize>,
    maximum_progress_bytes: usize,
    maximum_report_bytes: usize,
}
fn required_optional_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error> {
    <Option<usize> as Deserialize>::deserialize(deserializer)
}
#[derive(serde::Serialize)]
struct CooperativeAdmissionState<'a> {
    outcome: &'static str,
    failure: Option<&'a str>,
    advances: &'a [admission::Progress],
    progress_bytes: usize,
    report_published: bool,
    indivisible_unit: &'static str,
    source_preparation: &'static str,
    hard_time_slice: bool,
}
#[derive(serde::Serialize)]
struct CooperativeAdmissionReport<'a, I: serde::Serialize> {
    schema_version: u32,
    scope: &'static str,
    request_sha256: String,
    explicit_load_order: &'a [String],
    load_order_sha256: &'a str,
    executable_source_sha256: &'a str,
    prepared_counts: &'a fallout_runtime::programs::Counts,
    execution_admission: Option<&'a admission::Report>,
    cooperative_admission: CooperativeAdmissionState<'a>,
    index_cache: I,
    retail_parity_accepted: bool,
}
struct AdmissionBytes {
    bytes: usize,
    maximum: usize,
}
impl Write for AdmissionBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|&n| n <= self.maximum)
            .ok_or_else(|| std::io::Error::other("cooperative admission byte budget exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn admission_failure(error: impl std::fmt::Display, maximum: usize) -> Result<String> {
    struct Counter {
        bytes: usize,
        maximum: usize,
    }
    impl std::fmt::Write for Counter {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.bytes = self
                .bytes
                .checked_add(text.len())
                .filter(|&n| n <= self.maximum)
                .ok_or(std::fmt::Error)?;
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, maximum };
    std::fmt::write(&mut counter, format_args!("{error}"))
        .map_err(|_| "cooperative admission failure text byte budget exceeded")?;
    Ok(error.to_string())
}
pub(super) fn inspect_cooperative_admission(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(request_path)?
        .take(65_536 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("cooperative admission request byte budget exceeded".into());
    }
    let request: CooperativeAdmissionRequest = serde_json::from_slice(&bytes)?;
    let limits = admission::Limits::default();
    let jobs = admission::JobLimits::default();
    if request.schema_version != 1
        || request.maximum_roots > limits.maximum_roots
        || request.maximum_definitions > limits.maximum_definitions
        || request.maximum_dependencies > limits.maximum_dependencies
        || request.maximum_instructions > limits.maximum_instructions
        || request.maximum_operand_uses > limits.maximum_operand_uses
        || request.maximum_frontier > jobs.maximum_frontier
        || request.maximum_variable_bytes > jobs.maximum_variable_bytes
        || request.maximum_lookup_source_visits > jobs.maximum_lookup_source_visits
        || request.maximum_indivisible_instructions > jobs.maximum_indivisible_instructions
        || request.maximum_definition_expansions_per_advance > limits.maximum_definitions
        || request.maximum_operand_visits_per_advance > limits.maximum_operand_uses
        || request.maximum_advances == 0
        || request.maximum_advances > 4096
        || request
            .cancel_after_advances
            .is_some_and(|n| n > request.maximum_advances)
        || request.maximum_progress_bytes > 2 * 1024 * 1024
        || request.maximum_report_bytes == 0
        || request.maximum_report_bytes > 8 * 1024 * 1024
    {
        return Err("unsupported cooperative admission schema/budget ceiling".into());
    }
    let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = expression_plan::Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue = loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(()))?;
    let sources = PreparedSources::load(&catalogue, &model, &signatures, Default::default())?;
    let attachments =
        fallout_data::quest_scripts::Attachments::load(&mut store, &catalogue, 131_072, |_, _| {
            Ok(())
        })?;
    if request.source_cohort_sha256 != sources.source_cohort_sha256() {
        return Err("cooperative admission request has a different source receipt cohort".into());
    }
    let mut job = admission::AdmissionJob::new(
        &sources,
        &attachments,
        &request.roots,
        admission::Limits {
            maximum_roots: request.maximum_roots,
            maximum_definitions: request.maximum_definitions,
            maximum_dependencies: request.maximum_dependencies,
            maximum_instructions: request.maximum_instructions,
            maximum_operand_uses: request.maximum_operand_uses,
        },
        admission::JobLimits {
            maximum_frontier: request.maximum_frontier,
            maximum_variable_bytes: request.maximum_variable_bytes,
            maximum_lookup_source_visits: request.maximum_lookup_source_visits,
            maximum_indivisible_instructions: request.maximum_indivisible_instructions,
        },
    )?;
    let budget = admission::StepBudget {
        maximum_definition_expansions: request.maximum_definition_expansions_per_advance,
        maximum_operand_visits: request.maximum_operand_visits_per_advance,
    };
    let mut advances = Vec::new();
    let mut progress_bytes = 0;
    let mut complete = None;
    let (outcome, failure) = loop {
        if request.cancel_after_advances == Some(advances.len()) {
            drop(job);
            break ("cancelled", None);
        }
        if advances.len() == request.maximum_advances {
            drop(job);
            break ("advance_budget", None);
        }
        let progress = job.advance(budget);
        let status = progress.status;
        let stalled = progress.step_definition_expansions == 0 && progress.step_operand_visits == 0;
        let mut counter = AdmissionBytes {
            bytes: 0,
            maximum: request.maximum_progress_bytes - progress_bytes,
        };
        serde_json::to_writer(&mut counter, &progress)
            .map_err(|_| "cooperative admission progress byte budget exceeded")?;
        progress_bytes += counter.bytes;
        advances.push(progress);
        match status {
            admission::AdmissionStatus::Complete => {
                complete = Some(job.finish()?);
                break ("complete", None);
            }
            admission::AdmissionStatus::Failed => {
                let error = job
                    .finish()
                    .err()
                    .ok_or("missing cooperative admission failure")?;
                break (
                    "failed",
                    Some(admission_failure(error, request.maximum_report_bytes)?),
                );
            }
            admission::AdmissionStatus::Pending if stalled => {
                drop(job);
                break ("step_budget", None);
            }
            admission::AdmissionStatus::Pending => {}
        }
    };
    let report = CooperativeAdmissionReport {
        schema_version: 1,
        scope: "cooperative_declared_dependency_admission_without_execution",
        request_sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
        explicit_load_order: &order.names,
        load_order_sha256: &order.sha256,
        executable_source_sha256: &descriptors.source_sha256,
        prepared_counts: sources.counts(),
        execution_admission: complete.as_ref(),
        cooperative_admission: CooperativeAdmissionState {
            outcome,
            failure: failure.as_deref(),
            advances: &advances,
            progress_bytes,
            report_published: complete.is_some(),
            indivisible_unit: "one cached definition operation scan, one operand's bounded source targets, final bounded cycle analysis",
            source_preparation: "existing bounded synchronous catalogue/cache preparation before first admission advance",
            hard_time_slice: false,
        },
        index_cache: store.index_cache_report(),
        retail_parity_accepted: false,
    };
    let mut admitted = AdmissionBytes {
        bytes: 0,
        maximum: request.maximum_report_bytes,
    };
    serde_json::to_writer_pretty(&mut admitted, &report)
        .map_err(|_| "cooperative admission report byte budget exceeded")?;
    admitted
        .write_all(b"\n")
        .map_err(|_| "cooperative admission report byte budget exceeded")?;
    Ok(serde_json::to_value(report)?)
}

use sha2::Digest;
