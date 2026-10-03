//! Join exact winning handles with independently checked structures and tables.
use super::{Result, digest, json_file, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

const QUEST: &str = "source-item-regression/native-migration-regression/item-state-regression/leveled-source-regression/base-inventory-regression/foreign-runtime-regression/quest-script-regression";
fn key(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
fn source_key(plugin: &str, offset: &Value, header: &Value) -> Result<(String, u64, u64)> {
    Ok((
        plugin.to_lowercase(),
        offset.as_u64().ok_or("Missing source record offset")?,
        header.as_u64().ok_or("Missing source unit offset")?,
    ))
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    install: &Path,
    sources: &Value,
) -> Result<Value> {
    let bundle = run.join("winning-compiled.bin");
    let path = run.join("source-plans-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("source-plans")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--comparison-bundle")
        .arg(&bundle)
        .arg("--output")
        .arg(&path);
    run_logged_status(command, &run.join("source-plans-rust.log"), 1)?;
    let rust = json_file(&path)?;
    if rust["sources"] != *sources
        || rust["comparison_bundle"]["sha256"] != digest(&bundle)?
        || rust["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Prepared source identity or capability scope differs".into());
    }
    let loaded_path = run
        .join(QUEST)
        .join("loaded-script-regression/loaded-scripts-rust.json");
    let loaded = json_file(&loaded_path)?;
    if rust["source_counts"] != loaded["counts"]
        || rust["sources"] != loaded["plugins"]
        || rust["source_cohort_sha256"] != loaded["metadata"]["winning_definitions_sha256"]
    {
        return Err("Prepared and independently checked catalogue cohorts differ".into());
    }
    let loaded_rows = loaded["scripts"]
        .as_array()
        .ok_or("Missing winning source definitions")?;
    let definitions = rust["definitions"]
        .as_array()
        .ok_or("Missing prepared source definitions")?;
    if loaded_rows.len() != definitions.len() || definitions.len() != 80_627 {
        return Err("Prepared source definition coverage differs".into());
    }
    let operand_path = run
        .join(QUEST)
        .join("operand-regression/operand-bindings-rust.json");
    let operands = json_file(&operand_path)?;
    let mut bindings = BTreeMap::new();
    for plugin in operands["plugins"]
        .as_array()
        .ok_or("Missing independently checked operand sources")?
    {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing operand source name")?;
        for unit in plugin["compiled_units"]
            .as_array()
            .ok_or("Missing operand unit rows")?
        {
            let id = source_key(
                name,
                &unit["record_file_offset"],
                &unit["header_decoded_offset"],
            )?;
            if bindings.insert(id, unit).is_some() {
                return Err("Duplicate physical source unit identity".into());
            }
        }
    }
    let controls = native(
        root,
        run,
        "control",
        root.join("local/control-flow-oracle-build/Release/control-flow-oracle.exe"),
        &bundle,
        None,
    )?;
    let expressions = native(
        root,
        run,
        "expression",
        root.join("local/expression-plan-oracle-build/Release/expression-plan-oracle.exe"),
        &bundle,
        Some(&install.join("FalloutNV.exe")),
    )?;
    let control_rows = controls["structure"]["bodies"]
        .as_array()
        .ok_or("Missing independent control bodies")?;
    let expression_rows = expressions["plans"]["bodies"]
        .as_array()
        .ok_or("Missing independent expression bodies")?;
    if control_rows.len() != 14_476
        || expression_rows.len() != control_rows.len()
        || controls["structure"]["bundle_sha256"] != digest(&bundle)?
        || expressions["plans"]["bundle_sha256"] != digest(&bundle)?
        || expressions["executable_source_sha256"] != rust["executable_source_sha256"]
        || expressions["operator_descriptors"] != rust["operator_descriptors"]
    {
        return Err("Independent winning bundle identity or model differs".into());
    }
    let mut findings = Vec::new();
    let mut cursor = 0;
    let mut prepared_count = 0;
    for (source, row) in loaded_rows.iter().zip(definitions) {
        for field in ["handle", "version", "owner"] {
            if source[field] != row[field] {
                return Err(format!("Prepared winning source differs: {field}").into());
            }
        }
        if row["version"]["compiled_sha256"].is_null() {
            if !row["compiled_bundle_index"].is_null() || !row["prepared"].is_null() {
                return Err("Absent compiled field has a prepared body".into());
            }
            let expected = if source["issues"]
                .as_array()
                .ok_or("Missing source findings")?
                .is_empty()
            {
                json!({"kind":"absent_compiled_field"})
            } else {
                json!({"kind":"source_metadata","issues":source["issues"]})
            };
            if row["finding"] != expected {
                return Err("Absent source or metadata finding differs".into());
            }
        } else {
            if row["compiled_bundle_index"] != cursor {
                return Err("Winning bundle order differs from handles".into());
            }
            let control = &control_rows[cursor];
            let expression = &expression_rows[cursor];
            for body in [control, expression] {
                if body["bytes"] != row["version"]["compiled_bytes"]
                    || body["sha256"] != row["version"]["compiled_sha256"]
                {
                    return Err("Winning source handle and independent body identity differ".into());
                }
            }
            if !control["issue"].is_null() {
                if row["finding"] != json!({"kind":"control_structure","issue":control["issue"]})
                    || !row["prepared"].is_null()
                {
                    return Err("Winning control failure is not preserved".into());
                }
            } else if let Some(issue) = expression["statements"]
                .as_array()
                .ok_or("Missing source expression rows")?
                .iter()
                .find(|s| !s["issue"].is_null())
            {
                if row["finding"]
                    != json!({"kind":"expression_structure","instruction_scda_offset":issue["instruction_scda_offset"],"issue":issue["issue"]})
                    || !row["prepared"].is_null()
                {
                    return Err("Winning expression failure is not preserved".into());
                }
            } else {
                let prepared = &row["prepared"];
                if !row["finding"].is_null()
                    || prepared.is_null()
                    || prepared["control"] != control["structure"]
                    || prepared["instructions"] != control["instructions"]
                    || prepared["statements"] != expression["statements"]
                {
                    return Err("Prepared source structure differs from independent readers".into());
                }
                let name = row["version"]["source_plugin"]
                    .as_str()
                    .ok_or("Missing winning source plugin")?;
                let id = source_key(
                    name,
                    &row["version"]["record_file_offset"],
                    &row["handle"]["key"]["header_decoded_offset"],
                )?;
                let binding = bindings
                    .get(&id)
                    .ok_or("Missing independently checked owning-table binding")?;
                if binding["compiled_sha256"] != row["version"]["compiled_sha256"]
                    || binding["metadata_sha256"] != row["version"]["metadata_sha256"]
                    || binding["binding_sha256"] != prepared["binding_sha256"]
                    || binding["counts"] != prepared["binding_counts"]
                {
                    return Err("Prepared unit uses another source operand table".into());
                }
                prepared_count += 1;
            }
            cursor += 1;
        }
        if !row["finding"].is_null() && row["finding"]["kind"] != "absent_compiled_field" {
            findings.push(
                json!({"handle":row["handle"],"version":row["version"],"finding":row["finding"]}),
            );
        }
    }
    let expected = json!({"absent_compiled_field":66_148,"control_structure":53,"expression_structure":3,
        "prepared_source_structure":14_420,"source_metadata":3});
    if rust["counts"] != expected
        || prepared_count != 14_420
        || cursor != 14_476
        || findings.len() != 59
        || rust["prepared_expressions"] != 52_038
        || rust["prepared_tokens"] != 145_136
        || rust["prepared_nodes"] != 129_626
        || rust["prepared_operand_uses"] != 156_273
    {
        return Err("Prepared winning scope or totals changed".into());
    }
    // Unique handles prove no unit was quietly replaced by a body-hash alias.
    let mut handles = BTreeMap::new();
    for row in definitions {
        if handles.insert(key(&row["handle"])?, ()).is_some() {
            return Err("Duplicate winning script handle".into());
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Winning source handles joined with independently checked delimiter/expression structures and each unit's own operand tables; no execution acceptance",
        "sources":sources,"source_cohort_sha256":rust["source_cohort_sha256"],"definitions_compared":definitions.len(),
        "compiled_bodies_compared":cursor,"counts":expected,"prepared_expressions":rust["prepared_expressions"],"prepared_tokens":rust["prepared_tokens"],
        "prepared_nodes":rust["prepared_nodes"],"prepared_operand_uses":rust["prepared_operand_uses"],
        "all_winning_handles_structures_and_owning_tables_equal":true,"source_findings":findings,
        "rust_report_sha256":digest(&path)?,"winning_bundle_sha256":digest(&bundle)?,"catalogue_report_sha256":digest(&loaded_path)?,"operand_report_sha256":digest(&operand_path)?,
        "control_native_report_sha256":digest(&run.join("winning-control-native.json"))?,"expression_native_report_sha256":digest(&run.join("winning-expression-native.json"))?,
        "executable_source_sha256":rust["executable_source_sha256"],"execution_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original handling of retained source findings","Event arguments/lifecycle, native form/type admission and live context readiness",
            "Arithmetic, branch semantics, source scheduling and observable VM effects"]}),
    )
}
fn native(
    root: &Path,
    run: &Path,
    name: &str,
    oracle: std::path::PathBuf,
    bundle: &Path,
    executable: Option<&Path>,
) -> Result<Value> {
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(bundle);
    if let Some(executable) = executable {
        command.arg(executable);
    }
    command.arg("--diagnose-structure");
    let output = run_logged_status(command, &run.join(format!("winning-{name}-native.log")), 1)?;
    let path = run.join(format!("winning-{name}-native.json"));
    write_new(&path, &output.stdout)?;
    json_file(&path)
}
