//! Static quest attachments and foreign declaration associations, never values.
use super::{
    Result, command_catalogue,
    inspection_input::{Order, RecordBundle},
    script_profile,
};
use fallout_data::{loaded_scripts, quest_scripts, record_metadata};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    script_bundle: Option<&Path>,
    quest_bundle: Option<&Path>,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let mut scripts = script_bundle
        .map(|path| RecordBundle::create(path, install, b"FRCAT001"))
        .transpose()?;
    let mut quests = quest_bundle
        .map(|path| RecordBundle::create(path, install, b"FRQUEST1"))
        .transpose()?;
    eprintln!("Loading script definitions and authored quest attachments");
    let catalogue = loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |source, record| {
            if let Some(bundle) = &mut scripts {
                bundle.write(source, record)?;
            }
            Ok(())
        },
    )?;
    let attachments =
        quest_scripts::Attachments::load(&mut store, &catalogue, 65_536, |source, record| {
            if let Some(bundle) = &mut quests {
                bundle.write(source, record)?;
            }
            Ok(())
        })?;
    let executable = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&executable)?;
    let signatures = script_profile::signatures(&executable);
    let mut units = Vec::new();
    let mut foreign = Vec::new();
    let mut statuses = BTreeMap::<String, u64>::new();
    let mut uses = 0_u64;
    let mut missing = 0_u64;
    let mut decode_issues = 0_u64;
    for (_, script) in catalogue.iter() {
        let Some(binding) = script.bind_operands(&operators, &signatures, 262_144)? else {
            continue;
        };
        uses += binding.counts.uses;
        missing += binding.counts.missing_bindings;
        decode_issues += binding.decode_issues.len() as u64;
        if uses > 2_000_000 {
            return Err("loaded operand inspection exceeds two million uses".into());
        }
        let first = foreign.len();
        for operand in &binding.uses {
            let Some(context) = operand.context_reference else {
                continue;
            };
            if operand.status != 4 {
                continue;
            }
            if foreign.len() >= 1_000_000 {
                return Err("foreign declaration inspection exceeds one million uses".into());
            }
            let lookup = quest_scripts::declaration(
                &catalogue,
                &attachments,
                script.handle(),
                context,
                operand.index,
            );
            let status = serde_json::to_value(lookup.status)?
                .as_str()
                .ok_or("Invalid foreign status")?
                .to_string();
            *statuses.entry(status).or_default() += 1;
            foreign.push(
                json!({"scda_offset":operand.scda_offset,"role":operand.role,"lookup":lookup}),
            );
        }
        units.push(json!({"handle":script.handle(),"binding_sha256":fallout_data::obscript::operand_binding::digest(&binding.uses),
            "counts":binding.counts,"decode_issues":binding.decode_issues,"foreign_uses":foreign.len()-first}));
    }
    let quest_rows = attachments.iter().map(|(_, row)| row).collect::<Vec<_>>();
    let scripts_receipt = scripts.map(RecordBundle::finish).transpose()?;
    let quests_receipt = quests.map(RecordBundle::finish).transpose()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "plugins":catalogue.sources,"metadata":metadata,"catalogue_counts":catalogue.counts,"quest_counts":attachments.counts,"quests":quest_rows,
        "compiled_units":units,"foreign_operands":foreign,"operand_counts":{"uses":uses,"missing_bindings":missing,"decode_issues":decode_issues,"foreign_uses":foreign.len(),"declaration_statuses":statuses},
        "executable_sha256":executable.source_sha256,"script_comparison_bundle":scripts_receipt,"quest_comparison_bundle":quests_receipt,
        "index_cache":store.index_cache_report(),"index_payloads_deferred":store.deferred_payloads(),
        "static_declarations_only":true,"live_event_lists_loaded":false,"live_values_resolved":false,"execution_ready":false,"retail_parity_accepted":false}),
    )
}
