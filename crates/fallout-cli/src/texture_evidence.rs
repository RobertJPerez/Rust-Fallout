//! Bind texture dependencies, committed cache bytes and a separate ba2 member
//! reader. Raw image payloads remain in the ignored local evidence directory.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_json, write_new};
use fallout_data::cache;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

pub fn run(
    root: &Path,
    directory: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
    oracle_sha: &str,
) -> Result<Value> {
    let mut selected = BTreeMap::new();
    let mut archive_digests = BTreeMap::new();
    let mut seen_cache = BTreeSet::new();
    let mut record_keys = BTreeSet::new();
    let mut layers = 0;
    let mut default_layers = 0;
    let mut usages = 0;
    let mut records = 0;
    let mut datasets = Vec::new();
    let mut first = None;
    for cell in super::terrain_evidence::CELLS {
        let cached = json_file(&directory.join(cell).join("cached.json"))?;
        let compared = json_file(&directory.join(cell).join("compared.json"))?;
        let closure = &compared["texture_dependencies"];
        if closure["failures"] != 0 || closure["runtime_ready"] != false {
            return Err("Texture fixture closure or acceptance failed".into());
        }
        let entries = closure["records"]
            .as_array()
            .ok_or("Missing texture records")?;
        records += entries.len();
        for record in entries {
            record_keys.insert(
                record["body_cache"]["key"]
                    .as_str()
                    .ok_or("Missing texture body key")?
                    .to_owned(),
            );
        }
        let bindings = closure["bindings"]
            .as_array()
            .ok_or("Missing texture bindings")?;
        if bindings
            .iter()
            .any(|b| b["status"] != "records-resolved" && b["status"] != "null-default-unapplied")
        {
            return Err("A terrain layer lacks resolved records".into());
        }
        layers += bindings.len();
        default_layers += closure["unapplied_default_layers"]
            .as_u64()
            .ok_or("Missing NULL default-layer count")? as usize;
        usages += closure["path_usages"]
            .as_array()
            .ok_or("Missing texture path usages")?
            .len();
        let assets = closure["assets"]
            .as_array()
            .ok_or("Missing texture assets")?;
        for (i, asset) in assets.iter().enumerate() {
            if !asset["error"].is_null() || asset["candidates"].as_array().map(Vec::len) != Some(1)
            {
                return Err("An asset is unresolved or ambiguous".into());
            }
            let receipt = &asset["cache"];
            let key = receipt["key"].as_str().ok_or("Missing image cache key")?;
            let cold = &cached["texture_dependencies"]["assets"][i];
            if cold["cache"]["key"] != key
                || cold["cache"]["reused"] != seen_cache.contains(key)
                || receipt["reused"] != true
            {
                return Err("Cold/warm texture cache receipts differ".into());
            }
            seen_cache.insert(key.to_owned());
            let manifest: cache::Manifest = serde_json::from_value(receipt["manifest"].clone())?;
            let (_, bytes) = cache::read_verified(
                &directory.join("texture-cache"),
                install,
                &manifest.identity,
                256 * 1024 * 1024,
            )?
            .ok_or("Missing texture commit marker")?;
            if manifest.bytes != bytes.len() as u64
                || asset["decoded_bytes"] != manifest.bytes
                || asset["sha256"] != manifest.sha256
                || asset["archive_sha256"] != manifest.identity.source_sha256
            {
                return Err("Texture cache and source receipts differ".into());
            }
            let source = &asset["candidates"][0];
            let archive = source["container"]
                .as_str()
                .ok_or("Missing asset archive")?;
            if !archive_digests.contains_key(archive) {
                archive_digests.insert(archive.to_owned(), digest(Path::new(archive))?);
            }
            if archive_digests[archive] != manifest.identity.source_sha256 {
                return Err("Texture source archive changed".into());
            }
            let path: Vec<u8> = serde_json::from_value(source["original_path"].clone())?;
            let identity = (archive.to_owned(), path.clone());
            let expected = json!({"archive":archive,"path_bytes":path,"archive_sha256":manifest.identity.source_sha256,
                "decoded_bytes":manifest.bytes,"sha256":manifest.sha256});
            if let Some(previous) = selected.insert(identity, expected.clone())
                && previous != expected
            {
                return Err("Shared texture source receipt differs between cells".into());
            }
            if first.is_none() {
                first = Some((key.to_owned(), manifest));
            }
        }
        datasets.push(json!({"cell_editor_id":cell,"winning_texture_records":entries.len(),"layer_bindings":bindings.len(),
            "authored_path_fields":closure["path_usages"].as_array().ok_or("Missing paths")?.len(),"unique_assets":assets.len(),
            "decoded_asset_bytes":closure["decoded_asset_bytes"],"unapplied_default_layers":closure["unapplied_default_layers"],"failures":0,"cold_warm_receipts_verified":true}));
    }
    if selected.is_empty() {
        return Err("Empty asset comparison cannot certify texture closure".into());
    }
    let requests: Vec<_> = selected
        .values()
        .map(|v| json!({"archive":v["archive"],"path_bytes":v["path_bytes"]}))
        .collect();
    let requests_path = directory.join("texture-member-requests.json");
    write_json(&requests_path, &requests)?;
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(&requests_path);
    let output = run_logged(command, &directory.join("texture-member-oracle.log"))?;
    let oracle_path = directory.join("texture-member-oracle.json");
    write_new(&oracle_path, &output.stdout)?;
    let result = json_file(&oracle_path)?;
    if result["oracle_binary_sha256"] != oracle_sha {
        return Err("Member oracle binary identity differs".into());
    }
    let mut actual = BTreeMap::new();
    for row in result["files"]
        .as_array()
        .ok_or("Missing member oracle results")?
    {
        let identity = (
            row["archive"]
                .as_str()
                .ok_or("Missing oracle archive")?
                .to_owned(),
            serde_json::from_value::<Vec<u8>>(row["path_bytes"].clone())?,
        );
        if actual.insert(identity, row.clone()).is_some() {
            return Err("Duplicate member oracle result".into());
        }
    }
    if actual != selected {
        return Err("Independent texture archive-byte comparison failed".into());
    }

    // Damage a copied cache entry, preserving the verified cache and retail data.
    let corrupt = directory.join("texture-cache-corrupt");
    fs::create_dir(&corrupt)?;
    let (key, manifest) = first.ok_or("No cache entry for negative fixture")?;
    let cache_root = directory.join("texture-cache");
    let mut bytes = fs::read(cache_root.join(format!("{key}.blob")))?;
    let byte = bytes.last_mut().ok_or("Empty texture fixture")?;
    *byte ^= 1;
    write_new(&corrupt.join(format!("{key}.blob")), &bytes)?;
    write_new(
        &corrupt.join(format!("{key}.json")),
        &fs::read(cache_root.join(format!("{key}.json")))?,
    )?;
    if cache::read_verified(&corrupt, install, &manifest.identity, 256 * 1024 * 1024).is_ok() {
        return Err("Damaged copied texture cache was accepted".into());
    }
    let rejection = directory.join("texture-cache-rejected.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("terrain")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .args([
            "--editor-id",
            "Goodsprings",
            "--inspect-textures",
            "--index-cache",
        ])
        .arg(directory.join("index-cache"))
        .arg("--texture-cache")
        .arg(&corrupt)
        .arg("--output")
        .arg(&rejection);
    run_logged_status(command, &directory.join("texture-cache-rejected.log"), 1)?;
    let rejected = json_file(&rejection)?;
    if rejected["texture_dependencies"]["failures"]
        .as_u64()
        .unwrap_or(0)
        == 0
    {
        return Err("CLI did not reject a corrupt texture cache".into());
    }
    let public_assets: Vec<_> = selected.values().map(|v| json!({"archive":Path::new(v["archive"].as_str().unwrap()).file_name().unwrap().to_string_lossy(),
        "path_bytes":v["path_bytes"],"archive_sha256":v["archive_sha256"],"decoded_bytes":v["decoded_bytes"],"sha256":v["sha256"]})).collect();
    Ok(
        json!({"texture_record_appearances":records,"unique_texture_record_bodies":record_keys.len(),"layer_bindings":layers,
        "unapplied_default_layers":default_layers,"authored_path_field_appearances":usages,"unique_assets_compared":selected.len(),"unique_asset_bytes_compared":result["decoded_bytes"],
        "member_oracle_binary_sha256":oracle_sha,"member_oracle_report_sha256":digest(&oracle_path)?,"all_asset_bytes_equal":true,
        "cold_warm_cache_receipts_verified":true,"corrupt_copied_cache_rejected":true,"corrupt_cache_cli_exit_code":1,
        "corrupt_cache_report_sha256":digest(&rejection)?,"datasets":datasets,"assets":public_assets,
        "scope":"Strict winning LTEX/TXST fields and unique authored archive members; ba2 independently reads the selected bytes; cache receipts verified and copied corruption rejected; no DDS pixel, shader, blend or retail mount-policy acceptance"}),
    )
}
