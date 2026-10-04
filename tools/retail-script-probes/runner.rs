//! Rust capture-import runner. Retail launch/observation belongs to the separate
//! coordinator capture transport; this executable never calls a retail handler.
use crate::{Result, command_catalogue, inspection_input::Order, script_profile};
use fallout_data::{loaded_scripts::Catalogue, obscript::expression_plan::Model};
use fallout_runtime::{
    execution::{copy_probe, trace},
    foreign::Content,
    programs::PreparedSources,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

#[path = "fixtures.rs"]
pub(crate) mod fixtures;

pub(crate) struct Inputs<'a> {
    pub install: &'a Path,
    pub load_order: &'a Path,
    pub cache: Option<&'a Path>,
    pub manifest: &'a Path,
    pub profile_receipt: &'a Path,
    pub original: Option<&'a Path>,
    pub replacement: Option<&'a Path>,
    pub replacement_copy: Option<&'a Path>,
}

fn bounded_bytes(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    // Metadata is only an early refusal. The bounded stream also catches files
    // that grow or report a misleading length, before JSON allocation.
    let file = File::open(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err("semantic trace input byte budget exceeded".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("semantic trace input byte budget exceeded".into());
    }
    Ok(bytes)
}
fn read<T: DeserializeOwned>(path: &Path) -> Result<(T, String)> {
    let bytes = bounded_bytes(path, 4 * 1024 * 1024)?;
    Ok((
        serde_json::from_slice(&bytes)?,
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}
pub(crate) fn inspect(inputs: Inputs<'_>) -> Result<Value> {
    if inputs.replacement.is_some() && inputs.replacement_copy.is_some() {
        return Err("choose imported replacement or actual engineering copy".into());
    }
    let (manifest, manifest_sha256): (trace::Manifest, _) = read(inputs.manifest)?;
    let original = inputs.original.map(read::<trace::Capture>).transpose()?;
    let replacement = inputs.replacement.map(read::<trace::Capture>).transpose()?;
    let copy_request = inputs
        .replacement_copy
        .map(read::<copy_probe::Request>)
        .transpose()?;
    let receipt = bounded_bytes(inputs.profile_receipt, 1024 * 1024)?;
    let profile_sha256 = format!("{:x}", Sha256::digest(receipt));
    if profile_sha256 != manifest.identity.profile_receipt_sha256 {
        return Err("semantic trace profile receipt identity differs".into());
    }
    let descriptors = command_catalogue::inspect(&inputs.install.join("FalloutNV.exe"))?;
    // The descriptor loader already verifies the supported executable profile.
    // The manifest independently binds the exact executable file bytes.
    let executable = bounded_bytes(&inputs.install.join("FalloutNV.exe"), 64 * 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(executable)) != manifest.identity.executable_sha256 {
        return Err("semantic trace executable identity differs".into());
    }
    let operators = script_profile::operators(&descriptors)?;
    let model = Model::vanilla(&operators)?;
    let signatures = script_profile::signatures(&descriptors);
    let order = Order::read(inputs.load_order)?;
    let mut store = order.store(inputs.install, inputs.cache)?;
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(()))?;
    let sources = PreparedSources::load(&catalogue, &model, &signatures, Default::default())?;
    let copy_observation = if let Some((request, digest)) = &copy_request {
        let content = Content::load(&mut store, &catalogue, 1_000_000)?;
        let producer = File::open(std::env::current_exe()?)?;
        let mut limited = producer.take(256 * 1024 * 1024 + 1);
        let (bytes, producer_sha256) = fallout_data::baseline::digest_reader(&mut limited)?;
        if bytes > 256 * 1024 * 1024 {
            return Err("copy producer executable byte budget exceeded".into());
        }
        Some(copy_probe::observe(
            &sources,
            &content,
            &manifest,
            request,
            &producer_sha256,
            digest,
            Default::default(),
        )?)
    } else {
        None
    };
    let actual_replacement = replacement
        .as_ref()
        .map(|(capture, _)| capture)
        .or_else(|| {
            copy_observation
                .as_ref()
                .map(|observation| &observation.capture)
        });
    let comparison = trace::compare(
        &sources,
        &manifest,
        original.as_ref().map(|(capture, _)| capture),
        actual_replacement,
        Default::default(),
    )?;
    Ok(json!({
        "schema_version": 1,
        "scope": "imported_semantic_observations_against_prepared_source",
        "manifest_sha256": manifest_sha256,
        "profile_receipt_sha256": profile_sha256,
        "original_capture_sha256": original.as_ref().map(|(_, hash)| hash),
        "replacement_capture_sha256": replacement.as_ref().map(|(_, hash)| hash),
        "copy_request_sha256": copy_request.as_ref().map(|(_, hash)| hash),
        "replacement_observation": copy_observation,
        "identity": manifest.identity,
        "purpose": manifest.purpose,
        "comparison": comparison,
        "retail_execution_performed": false,
        "capture_transport_authenticated": false,
        "faithful_execution_admitted": false,
    }))
}
