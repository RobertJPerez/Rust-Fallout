//! Native persistence engineering evidence. Container comparison is independent;
//! original gameplay/save compatibility is not inferred from a successful write.
use super::{
    Result, digest, json_file, run_logged, run_logged_status, script_state_evidence, write_new,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

pub(super) struct Oracles<'a> {
    pub container: &'a Path,
    pub schemas: &'a Path,
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let schema_directory = run.join("schema-regression");
    fs::create_dir(&schema_directory)?;
    let schemas =
        script_state_evidence::run(root, &schema_directory, cli, oracles.schemas, install)?;
    let order = root.join("profiles/nv-inspection-order.json");
    let repository = run.join("native-repository");
    let report_path = run.join("native-save-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("native-save-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order)
        .arg("--new-repository")
        .arg(&repository)
        .arg("--output")
        .arg(&report_path);
    run_logged(command, &run.join("native-save-rust.log"))?;
    let report = json_file(&report_path)?;
    for key in [
        "worker_capture_isolated",
        "current_round_trip_equal",
        "strict_truncation_rejected",
        "previous_round_trip_equal",
    ] {
        if report[key] != true {
            return Err(format!("Native save probe failed: {key}").into());
        }
    }
    if report["recovery"]["current_repaired"] != false
        || report["recovery"]["current_failure"].is_null()
        || report["repair"]["current_repaired"] != true
    {
        return Err("Native recovery receipt is incomplete".into());
    }
    let cold_path = run.join("native-cold-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("native-load-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order)
        .arg("--repository")
        .arg(&repository)
        .arg("--output")
        .arg(&cold_path);
    run_logged(command, &run.join("native-cold-rust.log"))?;
    let cold = json_file(&cold_path)?;
    if cold["canonical_snapshot_sha256"] != report["canonical_snapshot_sha256"]
        || cold["receipt"]["metadata"] != report["final_write"]["metadata"]
        || cold["instances"] != report["instances"]
        || cold["pending_events"] != report["pending_events"]
        || cold["source_bound_restore"] != true
    {
        return Err("Cold native process restoration differs".into());
    }
    let mut containers = Vec::new();
    for (name, key) in [
        ("golden-current", "current"),
        ("golden-previous", "previous"),
    ] {
        let file = repository.join(format!("{name}.frsv"));
        let native_path = run.join(format!("{name}-native.json"));
        let mut command = Command::new(oracles.container);
        command.current_dir(root).arg(&file);
        let output = run_logged(command, &run.join(format!("{name}-native.log")))?;
        write_new(&native_path, &output.stdout)?;
        let native = json_file(&native_path)?;
        if native["metadata"] != report[key] || native["retail_save_compatibility"] != false {
            return Err("Independent native container metadata differs".into());
        }
        containers.push(json!({"name":name,"file_sha256":digest(&file)?,"native_report_sha256":digest(&native_path)?,"metadata":native["metadata"]}));
    }
    let malformed = malformed(
        root,
        run,
        cli,
        oracles.container,
        &fs::read(repository.join("golden-current.frsv"))?,
    )?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Engine-native script/reference/event persistence; engineering inputs, not original live-state capture or .fos compatibility",
        "oracle_binary_sha256":digest(oracles.container)?,"schema_oracle_binary_sha256":digest(oracles.schemas)?,
        "rust_report_sha256":digest(&report_path)?,"cold_report_sha256":digest(&cold_path)?,"instances":report["instances"],"pending_events":report["pending_events"],
        "worker_capture_isolated":true,"cold_process_state_equal":true,"explicit_recovery_equal":true,"independent_containers":containers,
        "malformed_container_cases_rejected":malformed,"snapshot_migration":"Version 1 to 2 requires explicit campaign identity; unit-tested; no automatic migration or retail-save import",
        "process_interruption":"Workspace native save tests kill a real child at five write/sync/publication stages and verify old/new complete slots and released locks",
        "fresh_schema_regression":schemas,"recovery":report["recovery"],"repair":report["repair"],
        "persistence_guarantees":"File sync and same-directory replacement; Windows NTFS process interruption tested; Windows directory/power-loss durability not verified",
        "retail_save_compatibility":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Full mutable player/inventory/actor/quest/world state is not implemented","Original live initialization and execution behavior remain unmeasured","Native filesystem tests do not prove power-loss durability or remote filesystem semantics","Retail .fos, NVSE cosaves, third-party DLL state and campaign compatibility remain separate tracks"]}),
    )
}

fn seal_whole(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let checksum = Sha256::digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&checksum);
}
fn malformed(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    original: &[u8],
) -> Result<Vec<String>> {
    let mut cases = Vec::new();
    for length in [0, 7, 15, 63, 151, 199, 231, original.len() - 1] {
        cases.push((format!("truncated-{length}"), original[..length].to_vec()));
    }
    for (name, offset, data) in [
        ("magic", 0, b"BADMAGIC".to_vec()),
        ("container-version", 8, 2_u16.to_le_bytes().to_vec()),
        ("reserved-header", 10, 1_u16.to_le_bytes().to_vec()),
        ("chunk-count", 12, 3_u32.to_le_bytes().to_vec()),
        ("metadata-tag", 16, b"UNKN".to_vec()),
        ("metadata-version", 20, 2_u32.to_le_bytes().to_vec()),
        ("metadata-extent", 24, u64::MAX.to_le_bytes().to_vec()),
        ("state-tag", 152, b"UNKN".to_vec()),
        ("state-version", 156, 2_u32.to_le_bytes().to_vec()),
        ("state-extent", 160, u64::MAX.to_le_bytes().to_vec()),
    ] {
        let mut bad = original.to_vec();
        bad[offset..offset + data.len()].copy_from_slice(&data);
        seal_whole(&mut bad);
        cases.push((name.into(), bad));
    }
    for (name, offset, width) in [
        ("profile", 64, 4),
        ("state-schema", 68, 4),
        ("zero-generation", 72, 8),
        ("zero-campaign", 128, 16),
    ] {
        let mut bad = original.to_vec();
        bad[offset..offset + width].fill(0);
        let checksum = Sha256::digest(&bad[64..152]);
        bad[32..64].copy_from_slice(&checksum);
        seal_whole(&mut bad);
        cases.push((name.into(), bad));
    }
    let mut bad = original.to_vec();
    bad[32] ^= 1;
    seal_whole(&mut bad);
    cases.push(("metadata-checksum".into(), bad));
    let mut bad = original.to_vec();
    bad[168] ^= 1;
    seal_whole(&mut bad);
    cases.push(("state-checksum".into(), bad));
    let mut bad = original.to_vec();
    *bad.last_mut().ok_or("Missing native checksum")? ^= 1;
    cases.push(("whole-checksum".into(), bad));
    let mut bad = original[..original.len() - 32].to_vec();
    bad.extend(b"unrecognized trailing bytes");
    bad.extend([0; 32]);
    seal_whole(&mut bad);
    cases.push(("trailing-bytes".into(), bad));
    let mut rejected = Vec::new();
    for (name, bytes) in cases {
        let file = run.join(format!("bad-native-{name}.frsv"));
        write_new(&file, &bytes)?;
        let json = run.join(format!("bad-native-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("native-save-file")
            .arg("--file")
            .arg(&file)
            .arg("--output")
            .arg(&json);
        run_logged_status(command, &run.join(format!("bad-native-{name}-rust.log")), 1)?;
        if json.exists() {
            return Err("Malformed native container published Rust metadata".into());
        }
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(&file);
        let output = run_logged_status(
            command,
            &run.join(format!("bad-native-{name}-native.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Malformed native container published native metadata".into());
        }
        rejected.push(name);
    }
    let file = run.join("bad-native-file-budget.frsv");
    let oversized = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&file)?;
    oversized.set_len(
        (fallout_runtime::Limits::default().max_snapshot_bytes
            + fallout_runtime::save::format::OVERHEAD
            + 1) as u64,
    )?;
    drop(oversized);
    let json = run.join("bad-native-file-budget.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("native-save-file")
        .arg("--file")
        .arg(&file)
        .arg("--output")
        .arg(&json);
    run_logged_status(command, &run.join("bad-native-file-budget-rust.log"), 1)?;
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(&file);
    let output = run_logged_status(command, &run.join("bad-native-file-budget-native.log"), 1)?;
    if json.exists() || !output.stdout.is_empty() {
        return Err("Oversized native container published metadata".into());
    }
    rejected.push("file-budget".into());
    Ok(rejected)
}
