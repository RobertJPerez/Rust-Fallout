//! Join immutable winning source handles, structural plans and owning tables.
use super::{Result, command_catalogue, inspection_input::Order, protected_tree, script_profile};
use fallout_data::{
    baseline, loaded_scripts,
    obscript::{
        control_flow_bundle::Structure, definition_plan, expression_census, expression_plan,
        operand_binding,
    },
};
use fallout_runtime::{execution::admission, programs::PreparedSources};
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
) -> Result<Value> {
    if admission_path.is_some() && bundle_path.is_some() {
        return Err(
            "execution admission and comparison bundle are separate source-plan requests".into(),
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

use sha2::Digest;
