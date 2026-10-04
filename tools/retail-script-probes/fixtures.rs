//! Source-only microfixture authoring. No original execution or expected output.
use super::{bounded_bytes, read};
use crate::{Result, command_catalogue, inspection_input::Order, protected_tree, script_profile};
use fallout_data::{loaded_scripts::Catalogue, obscript::expression_plan::Model};
use fallout_runtime::{
    execution::{fixture, trace},
    programs::PreparedSources,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

fn write(path: &Path, bytes: &[u8]) -> Result<String> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn json_file(path: &Path, value: &impl Serialize) -> Result<String> {
    write(path, &serde_json::to_vec_pretty(value)?)
}

pub(crate) fn generate(
    install: &Path,
    request_path: &Path,
    profile_receipt: &Path,
    destination: &Path,
) -> Result<Value> {
    let (request, request_sha256): (fixture::Request, _) = read(request_path)?;
    let receipt = bounded_bytes(profile_receipt, 1024 * 1024)?;
    let receipt_sha256 = format!("{:x}", Sha256::digest(&receipt));
    let executable_path = install.join("FalloutNV.exe");
    let descriptors = command_catalogue::inspect(&executable_path)?;
    let executable = bounded_bytes(&executable_path, 64 * 1024 * 1024)?;
    let executable_sha256 = format!("{:x}", Sha256::digest(&executable));
    let artifact = fixture::generate(&request, 4096)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    if parent.starts_with(protected_tree(install)?) {
        return Err("probe fixture destination must be outside the installation".into());
    }
    // Existing directories are never adopted or overwritten. An interrupted
    // bundle has no final receipt and cannot be mistaken for completed output.
    fs::create_dir(destination)?;
    let destination = destination.canonicalize()?;
    let source = destination.join("authored-source-copy");
    fs::create_dir(&source)?;
    fs::create_dir(source.join("Data"))?;
    write(&source.join("FalloutNV.exe"), &executable)?; // metadata only, never launched
    write(
        &source.join("Data").join(fixture::PLUGIN_NAME),
        &artifact.plugin,
    )?;
    let order_path = destination.join("order.json");
    let order_sha256 = json_file(&order_path, &[fixture::PLUGIN_NAME])?;
    let order = Order::read(&order_path)?;
    let mut store = order.store(&source, None)?;
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(()))?;
    let operators = script_profile::operators(&descriptors)?;
    let model = Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let sources = PreparedSources::load(&catalogue, &model, &signatures, Default::default())?;
    let definition = catalogue
        .iter()
        .next()
        .ok_or("generated source definition missing")?
        .1
        .handle()
        .clone();
    let plan = sources.get(&definition)?.plan();
    let manifest = trace::Manifest {
        schema_version: 1,
        identity: trace::Identity {
            executable_sha256,
            profile_receipt_sha256: receipt_sha256,
            source_cohort_sha256: sources.source_cohort_sha256().into(),
            winning_content_sha256: plan.source_cohort_sha256().into(),
            definition,
            compiled_sha256: artifact.compiled_sha256.clone(),
            compiled_bytes: artifact.compiled.len(),
        },
        purpose: request.purpose,
        steps: vec![trace::StepInput {
            event_ordinal: 0,
            event_id: artifact.shape.event_id,
            begin_scda_offset: artifact.shape.begin_scda_offset,
            scda_offset: artifact.shape.operation_scda_offset,
            operation: request.purpose,
            caller: artifact.shape.caller.clone(),
            operands: artifact.shape.operand_bits.clone(),
            item: None,
        }],
    };
    trace::validate_manifest(&sources, &manifest, Default::default())?;
    let manifest_sha256 = json_file(&destination.join("manifest.json"), &manifest)?;
    let copy_request_sha256 = json_file(
        &destination.join("copy-request.json"),
        &artifact.copy_request,
    )?;
    write(&destination.join("profile-receipt.json"), &receipt)?;
    let report = json!({
        "schema_version":1,"scope":"authored_source_fixture_only","destination":destination,
        "request_sha256":request_sha256,"load_order_sha256":order_sha256,
        "plugin_name":fixture::PLUGIN_NAME,"plugin_bytes":artifact.plugin.len(),"plugin_sha256":artifact.plugin_sha256,
        "manifest_sha256":manifest_sha256,"copy_request_sha256":copy_request_sha256,
        "identity":manifest.identity,"trace_shape":artifact.shape,
        "existing_decoder_round_trip_verified":true,
        "original_expected_output_generated":false,"original_execution_performed":false,
        "retail_loading_verified":false,"faithful_execution_admitted":false,"gameplay_accepted":false,
        "disclosure":"Standalone source carrier, one script without masters or references. Retail load-order/activation/initialization are unverified. Copy-request explicit inputs are engineering-only. Metadata executable copied for descriptor inspection and never launched. Any different recorder profile requires new exact source binding."
    });
    json_file(&destination.join("receipt.json"), &report)?;
    Ok(report)
}
