//! Source operand associations over explicit engineering state, without effects.
use super::{
    Result, command_catalogue, definition_plan_inspection, inspection_input::Order, script_profile,
    script_state_inspection,
};
use fallout_data::{loaded_scripts, obscript};
use fallout_runtime::{
    event_operands, execution::native, foreign::Content, identity::ReferenceId, preparation,
    programs,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, path::Path, sync::Arc};

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
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let sources = (prepare_sources || native_capabilities)
        .then(|| {
            programs::PreparedSources::load(&catalogue, &model, &signatures, Default::default())
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
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

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
