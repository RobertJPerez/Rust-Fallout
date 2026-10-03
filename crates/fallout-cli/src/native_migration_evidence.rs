//! Explicit native migration is tested separately from original-save support.
use super::{
    Result, digest, item_state_evidence, json_file, run_logged, run_logged_status, write_new,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: item_state_evidence::Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let container = oracles.content.inventory.runtime.saves.container;
    let regression = run.join("item-state-regression");
    fs::create_dir(&regression)?;
    let items = item_state_evidence::run(root, &regression, cli, oracles, install)?;
    // This engineering schema-3 script-only state can author a schema-2 fixture
    // without discarding any initialized item state. The importer has no downgrade.
    let seed = fs::read(regression.join("leveled-source-regression/base-inventory-regression/foreign-runtime-regression/native-save-regression/native-repository/golden-current.frsv"))?;
    let mut legacy: Value = serde_json::from_slice(&seed[200..seed.len() - 32])?;
    if legacy["inventory_banks"] != json!([]) || legacy["next_item"] != 1 {
        return Err("Legacy fixture would discard item state".into());
    }
    let fields = legacy.as_object_mut().ok_or("Legacy state object")?;
    fields.remove("inventory_banks");
    fields.remove("next_item");
    fields.insert("schema_version".into(), 2.into());
    let body = serde_json::to_vec(&legacy)?;
    let source = assemble(&seed, &body);
    let old = run.join("authored-schema-2.frsv");
    write_new(&old, &source)?;
    let old_digest = digest(&old)?;
    let native_old_path = run.join("schema-2-native.json");
    let mut command = Command::new(container);
    command.current_dir(root).arg(&old).arg("--schema-2");
    let output = run_logged(command, &run.join("schema-2-native.log"))?;
    write_new(&native_old_path, &output.stdout)?;
    let native_old = json_file(&native_old_path)?;
    let destination = run.join("migrated-repository");
    let report_path = run.join("native-migration-rust.json");
    run_logged(
        import_command(root, cli, install, &old, &destination, &report_path),
        &run.join("native-migration-rust.log"),
    )?;
    let report = json_file(&report_path)?;
    if report["source_metadata"] != native_old["metadata"]
        || native_old["state_schema"] != 2
        || report["source_state_schema"] != 2
        || report["target_state_schema"] != 3
        || report["source_bound_restore"] != true
        || report["canonical_state_round_trip_equal"] != true
        || report["inventory_banks"] != 0
        || report["next_item"] != 1
        || report["retail_save_compatibility"] != false
    {
        return Err("Native migration scope/integrity differs".into());
    }
    let target = fs::read(destination.join("current.frsv"))?;
    let mut projection: Value = serde_json::from_slice(&target[200..target.len() - 32])?;
    let fields = projection.as_object_mut().ok_or("Migrated state object")?;
    fields.remove("inventory_banks");
    fields.remove("next_item");
    fields.insert("schema_version".into(), 2.into());
    if projection != legacy || digest(&old)? != old_digest {
        return Err("Migration changed old fields or original file".into());
    }
    let cold_path = run.join("native-migration-cold.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("native-load-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--repository")
        .arg(&destination)
        .arg("--output")
        .arg(&cold_path);
    run_logged(command, &run.join("native-migration-cold.log"))?;
    let cold = json_file(&cold_path)?;
    if cold["canonical_snapshot_sha256"] != report["snapshot_sha256"]
        || cold["receipt"]["metadata"] != report["new_write"]["metadata"]
    {
        return Err("Cold migrated state differs".into());
    }
    let native_new_path = run.join("schema-3-native.json");
    let mut command = Command::new(container);
    command
        .current_dir(root)
        .arg(destination.join("current.frsv"));
    let output = run_logged(command, &run.join("schema-3-native.log"))?;
    write_new(&native_new_path, &output.stdout)?;
    if json_file(&native_new_path)?["metadata"] != report["new_write"]["metadata"] {
        return Err("Migrated container independent metadata differs".into());
    }
    let mut bad_cases = Vec::new();
    let mut bad = source.clone();
    bad[68..72].copy_from_slice(&4_u32.to_le_bytes());
    reseal(&mut bad);
    bad_cases.push(("unsupported-schema", bad));
    let mut bad = source.clone();
    bad[144] ^= 1;
    reseal(&mut bad);
    bad_cases.push(("metadata-revision", bad));
    let mut bad = source.clone();
    bad[128..144].fill(0);
    reseal(&mut bad);
    bad_cases.push(("zero-campaign", bad));
    let mut bad = source.clone();
    bad[200] ^= 1;
    bad_cases.push(("state-checksum", bad));
    bad_cases.push(("truncated", source[..source.len() - 1].to_vec()));
    let mut value = legacy.clone();
    value["next_reference"] = 1.into();
    bad_cases.push((
        "allocator-reuse",
        assemble(&seed, &serde_json::to_vec(&value)?),
    ));
    let mut value = legacy.clone();
    value["unknown"] = true.into();
    bad_cases.push((
        "unknown-json-field",
        assemble(&seed, &serde_json::to_vec(&value)?),
    ));
    let mut value = legacy.clone();
    value["catalogue_sha256"] = "00".repeat(32).into();
    let mut bad = assemble(&seed, &serde_json::to_vec(&value)?);
    bad[88..120].fill(0);
    reseal(&mut bad);
    bad_cases.push(("changed-source-cohort", bad));
    let mut rejected = Vec::new();
    for (name, bytes) in bad_cases {
        let file = run.join(format!("bad-{name}.frsv"));
        write_new(&file, &bytes)?;
        let repo = run.join(format!("rejected-{name}"));
        let output = run.join(format!("bad-{name}.json"));
        let result = run_logged_status(
            import_command(root, cli, install, &file, &repo, &output),
            &run.join(format!("bad-{name}.log")),
            1,
        )?;
        if result.status.success()
            || repo.try_exists()?
            || output.try_exists()?
            || digest(&file)? != format!("{:x}", Sha256::digest(&bytes))
        {
            return Err(format!("Bad migration published or changed input: {name}").into());
        }
        rejected.push(name);
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Explicit native schema-2 to schema-3 import engineering; independent container integrity only",
        "source_state_schema":2,"target_state_schema":3,"source_metadata":report["source_metadata"],"new_metadata":report["new_write"]["metadata"],
        "all_prior_state_fields_equal":true,"old_file_unchanged":true,"cold_state_equal":true,"inventories_left_uninitialized":true,"malformed_imports_rejected_before_repository_creation":rejected,
        "instances":report["instances"],"references":report["references"],"pending_events":report["pending_events"],"snapshot_bytes":report["snapshot_bytes"],"snapshot_sha256":report["snapshot_sha256"],
        "fixture_provenance":"Authored schema-2 envelope from fresh source-bound script-only engineering state; checked that no inventory state was dropped",
        "rust_report_sha256":digest(&report_path)?,"cold_report_sha256":digest(&cold_path)?,"old_native_report_sha256":digest(&native_old_path)?,"new_native_report_sha256":digest(&native_new_path)?,
        "fresh_item_state_regression":items,"retail_save_compatibility":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Bethesda .fos import, NVSE cosaves and third-party DLL state","Original live initialization/execution and complete world persistence","Windows directory/power-loss durability"]}),
    )
}
fn import_command(
    root: &Path,
    cli: &Path,
    install: &Path,
    file: &Path,
    destination: &Path,
    output: &Path,
) -> Command {
    let mut c = Command::new(cli);
    c.current_dir(root)
        .arg("native-migrate-v2")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--file")
        .arg(file)
        .arg("--new-repository")
        .arg(destination)
        .arg("--output")
        .arg(output);
    c
}
fn assemble(seed: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = seed[..200].to_vec();
    out[68..72].copy_from_slice(&2_u32.to_le_bytes());
    out[120..128].copy_from_slice(&(body.len() as u64).to_le_bytes());
    out[160..168].copy_from_slice(&(body.len() as u64).to_le_bytes());
    out[168..200].copy_from_slice(&Sha256::digest(body));
    out.extend(body);
    out.extend([0; 32]);
    reseal(&mut out);
    out
}
fn reseal(out: &mut [u8]) {
    let meta = Sha256::digest(&out[64..152]);
    out[32..64].copy_from_slice(&meta);
    let end = out.len() - 32;
    let all = Sha256::digest(&out[..end]);
    out[end..].copy_from_slice(&all);
}
