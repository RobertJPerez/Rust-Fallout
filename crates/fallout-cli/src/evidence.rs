//! Checkpoint tooling stays separate from the content-inspection CLI. This runner
//! records the commands it actually executes and publishes metadata, never assets.
mod index_evidence;
mod terrain_evidence;

use clap::Parser;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(about = "Bind content-inspection checkpoint evidence to committed source")]
struct Args {
    #[arg(long, default_value = ".")]
    repository: PathBuf,
    /// Must be a new directory immediately under the repository's local directory.
    #[arg(long)]
    run_directory: PathBuf,
    #[arg(long)]
    install: PathBuf,
    #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u8).range(7..=9))]
    checkpoint: u8,
}

#[derive(Serialize)]
struct SourceFile {
    path: String,
    sha256: String,
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn json_file(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_new(path, &bytes)
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(format!("git {args:?} failed").into());
    }
    Ok(output.stdout)
}

fn snapshot(root: &Path, revision: &str) -> Result<Value> {
    let names = git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let mut names: Vec<_> = std::str::from_utf8(&names)?
        .split('\0')
        .filter(|name| {
            let path = Path::new(name);
            matches!(
                path.extension().and_then(|v| v.to_str()),
                Some("rs" | "wgsl" | "py" | "ps1" | "toml" | "cpp" | "hpp")
            ) || matches!(
                path.file_name().and_then(|v| v.to_str()),
                Some("Cargo.lock" | "CMakeLists.txt" | "sources.lock.json")
            )
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    let mut files = Vec::new();
    for name in names {
        let sha256 = digest(&root.join(&name))?;
        let committed = git(root, &["show", &format!("{revision}:{name}")])?;
        if sha256 != format!("{:x}", Sha256::digest(&committed)) {
            return Err(format!("Source differs from implementation commit: {name}").into());
        }
        files.push(SourceFile { path: name, sha256 });
    }
    let identity = format!("{:x}", Sha256::digest(serde_json::to_vec(&files)?));
    Ok(json!({
        "schema_version":1, "revision":revision,
        "working_tree_dirty":!git(root, &["status", "--porcelain"])?.is_empty(),
        "scope":"Rust/C++ source, WGSL, headers, tooling, manifests, source pins and Cargo locks; every file matches the implementation commit",
        "digest_recipe":"SHA256 of compact UTF-8 JSON files array; object keys path then sha256",
        "sha256":identity, "files":files
    }))
}

fn run_logged(command: Command, log: &Path) -> Result<Output> {
    run_logged_status(command, log, 0)
}

fn run_logged_status(mut command: Command, log: &Path, expected: i32) -> Result<Output> {
    eprintln!("Running {command:?}");
    let output = command.output()?;
    let mut bytes = output.stdout.clone();
    bytes.extend(&output.stderr);
    write_new(log, &bytes)?;
    if output.status.code() != Some(expected) {
        return Err(format!("Command failed; see {}", log.display()).into());
    }
    Ok(output)
}

fn collision_summary(
    report_path: &Path,
    oracle_path: &Path,
    cli: &str,
    oracle: &str,
) -> Result<Value> {
    let report = json_file(report_path)?;
    if report["failures"] != 0
        || report["oracle_binary_sha256"] != oracle
        || report["oracle_report_sha256"] != digest(oracle_path)?
        || report["physics_ready"] != false
    {
        return Err("Collision report identity or success checks failed".into());
    }
    let mut streams = BTreeMap::<u64, usize>::new();
    let mut unsupported = BTreeMap::<String, usize>::new();
    let mut files = Vec::new();
    for row in report["files"]
        .as_array()
        .ok_or("Missing collision file list")?
    {
        if row["comparison"] != "all_equal" || !row["error"].is_null() {
            return Err("A collision comparison failed".into());
        }
        let path = row["input"].as_str().ok_or("Missing model path")?;
        if digest(Path::new(path))? != row["sha256"] {
            return Err("A model changed after inspection".into());
        }
        let stream = row["tuple"][2].as_u64().ok_or("Missing stream revision")?;
        *streams.entry(stream).or_default() += 1;
        let collision = &row["collision"];
        let blocks = collision["blocks"]
            .as_array()
            .ok_or("Missing decoded blocks")?;
        for (name, ids) in collision["unsupported_blocks"]
            .as_object()
            .ok_or("Missing unsupported block table")?
        {
            *unsupported.entry(name.clone()).or_default() += ids
                .as_array()
                .ok_or("Invalid unsupported block list")?
                .len();
        }
        files.push(json!({
            "sha256":row["sha256"], "decoded_bytes":row["decoded_bytes"],
            "tuple":row["tuple"], "decoded_collision_blocks":blocks.len(),
            "unsupported_links":collision["unsupported_links"].as_array().ok_or("Missing unsupported links")?.len(),
            "comparison":"all_equal"
        }));
    }
    if files.is_empty() {
        return Err("Empty collision comparison cannot certify a dataset".into());
    }
    Ok(json!({
        "files":files, "file_count":files.len(), "stream_counts":streams,
        "block_counts":report["block_counts"], "unsupported_block_counts":unsupported,
        "packed_vertices":report["packed_vertices"], "packed_triangles":report["packed_triangles"],
        "convex_vertices":report["convex_vertices"], "mopp_bytes":report["mopp_bytes"],
        "comparison":report["comparison"], "all_equal":true, "physics_ready":false,
        "release_cli_sha256":cli, "raw_oracle_binary_sha256":oracle,
        "rust_report_sha256":digest(report_path)?, "oracle_report_sha256":digest(oracle_path)?
    }))
}

fn run(args: Args) -> Result<()> {
    let root = args.repository.canonicalize()?;
    let run_parent = args
        .run_directory
        .parent()
        .ok_or("Run directory has no parent")?
        .canonicalize()?;
    if run_parent != root.join("local").canonicalize()? {
        return Err(
            "Run directory must be immediately under this repository's local directory".into(),
        );
    }
    let destination = run_parent.join(
        args.run_directory
            .file_name()
            .ok_or("Missing run directory name")?,
    );
    let revision = String::from_utf8(git(&root, &["rev-parse", "HEAD"])?)?
        .trim()
        .to_owned();
    let source = snapshot(&root, &revision)?;
    fs::create_dir(&destination)?;
    let cli_path = root.join("target/release/fallout.exe");
    let oracle_path = root.join("local/nif-oracle-build/Release/nif-oracle.exe");
    let cli_digest = digest(&cli_path)?;
    let oracle_digest = if args.checkpoint == 7 {
        Some(digest(&oracle_path)?)
    } else {
        None
    };
    let terrain_oracle = root.join("local/terrain-oracle-build/Release/terrain-oracle.exe");
    let terrain_digest = if args.checkpoint == 9 {
        Some(digest(&terrain_oracle)?)
    } else {
        None
    };
    let check_log = destination.join("workspace-check.log");
    let mut check = Command::new("powershell");
    check.current_dir(&root).args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "tools/check.ps1",
    ]);
    let output = run_logged(check, &check_log)?;
    let stdout = String::from_utf8(output.stdout)?;
    let passed: usize = stdout
        .lines()
        .filter_map(|line| {
            line.strip_prefix("test result: ok. ")?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()
        })
        .sum();
    if passed == 0 {
        return Err("Workspace check did not run tests".into());
    }
    let mut datasets = BTreeMap::new();
    let collision_inputs = [
        ("house", "local/docmitchell-models"),
        ("older_stream_samples", "local/nif-variant-models"),
    ];
    let collision_inputs = if args.checkpoint == 7 {
        collision_inputs.as_slice()
    } else {
        &[]
    };
    for &(name, directory) in collision_inputs {
        let oracle_report = destination.join(format!("{name}-oracle.json"));
        let mut oracle_command = Command::new(&oracle_path);
        oracle_command
            .current_dir(&root)
            .arg(root.join(directory))
            .arg("--collision");
        eprintln!("Running {oracle_command:?}");
        let output = oracle_command.output()?;
        write_new(&oracle_report, &output.stdout)?;
        write_new(
            &destination.join(format!("{name}-oracle.log")),
            &output.stderr,
        )?;
        if !output.status.success() {
            return Err(format!("Independent oracle failed for {name}").into());
        }
        let rust_report = destination.join(format!("{name}-rust.json"));
        let mut rust_command = Command::new(&cli_path);
        rust_command
            .current_dir(&root)
            .arg("nif-collision")
            .arg(root.join(directory))
            .arg("--oracle-report")
            .arg(&oracle_report)
            .arg("--output")
            .arg(&rust_report);
        run_logged(rust_command, &destination.join(format!("{name}-rust.log")))?;
        datasets.insert(
            name,
            collision_summary(
                &rust_report,
                &oracle_report,
                &cli_digest,
                oracle_digest.as_deref().ok_or("Missing oracle digest")?,
            )?,
        );
    }
    let index_evidence = if args.checkpoint == 8 {
        Some(index_evidence::run(
            &root,
            &destination,
            &cli_path,
            &args.install,
            &cli_digest,
        )?)
    } else {
        None
    };
    let terrain_evidence = if args.checkpoint == 9 {
        Some(terrain_evidence::run(
            &root,
            &destination,
            &cli_path,
            &terrain_oracle,
            &args.install,
            &cli_digest,
            terrain_digest
                .as_deref()
                .ok_or("Missing terrain oracle digest")?,
        )?)
    } else {
        None
    };
    let baseline_path = destination.join("baseline.json");
    let mut baseline_command = Command::new(&cli_path);
    baseline_command
        .current_dir(&root)
        .arg("baseline")
        .arg("--install")
        .arg(args.install)
        .arg("--output")
        .arg(&baseline_path);
    run_logged(baseline_command, &destination.join("baseline.log"))?;
    let before = json_file(&root.join("local/baseline.json"))?;
    let after = json_file(&baseline_path)?;
    if before["files"] != after["files"]
        || before["content_fingerprint"] != after["content_fingerprint"]
    {
        return Err("Installation differs from original baseline".into());
    }
    let installation_files = after["files"].as_array().ok_or("Missing baseline files")?;
    let installation_bytes: u64 = installation_files
        .iter()
        .map(|row| row["bytes"].as_u64().ok_or("Missing baseline file size"))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .sum();
    let current_revision = String::from_utf8(git(&root, &["rev-parse", "HEAD"])?)?;
    if current_revision.trim() != revision
        || snapshot(&root, &revision)? != source
        || digest(&cli_path)? != cli_digest
        || oracle_digest
            .as_ref()
            .is_some_and(|expected| digest(&oracle_path).as_ref().ok() != Some(expected))
        || terrain_digest
            .as_ref()
            .is_some_and(|expected| digest(&terrain_oracle).as_ref().ok() != Some(expected))
    {
        return Err("Source or executable changed during verification".into());
    }
    let collisions = json!({
        "schema_version":1, "checkpoint":7, "engine_revision":revision,
        "source_snapshot_sha256":source["sha256"], "datasets":datasets,
        "oracle":"raw nifly factories cca0a770094bb962fb28ea1fec5ea903e68fda8e; no PrepareData",
        "physics_ready":false,
        "known_gaps":[
            "Coordinates, units, body activation and motion behavior are not measured against retail",
            "Constraint and blend-controller payloads remain unsupported",
            "MOPP bytecode is opaque; no interpreter or physics backend is implemented",
            "Compressed packed vertices retain source words only; pinned nifly cannot certify this branch",
            "bhkPCollisionObject has synthetic coverage only; this is not a whole-corpus collision scan"
        ]
    });
    let verification = json!({
        "schema_version":1, "date":"2026-10-02", "checkpoint":args.checkpoint, "engine_revision":revision,
        "source_snapshot_sha256":source["sha256"], "source_matches_implementation_commit":true,
        "release_cli_sha256":cli_digest, "raw_oracle_binary_sha256":oracle_digest,
        "evidence_runner_sha256":digest(&std::env::current_exe()?)?,
        "check_command":"powershell -NoProfile -ExecutionPolicy Bypass -File tools/check.ps1",
        "check_exit_code":0, "check_log_sha256":digest(&check_log)?, "tests_passed":passed,
        "format_check":"passed", "clippy_warnings_denied":"passed",
        "original_installation_matches_baseline":true,
        "installation_files_checked":installation_files.len(), "installation_bytes_checked":installation_bytes,
        "content_fingerprint":after["content_fingerprint"], "baseline_report_sha256":digest(&baseline_path)?,
        "fresh_collision_comparison":collisions["datasets"],
        "presentation_evidence_origin_checkpoint":6, "presentation_reexecuted":false,
        "prior_verification":format!("reports/checkpoint-{:02}-verification.json", args.checkpoint - 1),
        "local_evidence_directory":destination.strip_prefix(&root)?.to_string_lossy(),
        "gameplay_acceptance":"not implemented; no accepted scenarios"
    });
    // Immutable checkpoint files are the publication boundary. Update current
    // aliases separately after this runner succeeds, retaining earlier receipts.
    let mut verification = verification;
    write_json(
        &root.join(format!(
            "reports/checkpoint-{:02}-source-snapshot.json",
            args.checkpoint
        )),
        &source,
    )?;
    if let Some(mut terrain) = terrain_evidence {
        terrain["engine_revision"] = revision.clone().into();
        terrain["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Verification object missing")?
            .remove("fresh_collision_comparison");
        verification["exterior_fields"] = terrain.clone();
        verification["terrain_oracle_binary_sha256"] = terrain_digest.into();
        verification["collision_evidence_origin_checkpoint"] = 7.into();
        verification["collision_comparison_reexecuted"] = false.into();
        verification["record_index_cache_evidence_origin_checkpoint"] = 8.into();
        write_json(&root.join("reports/exterior-fields.json"), &terrain)?;
    } else if let Some(mut index) = index_evidence {
        index["engine_revision"] = revision.clone().into();
        index["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Verification object missing")?
            .remove("fresh_collision_comparison");
        verification["record_index_cache"] = index.clone();
        verification["collision_evidence_origin_checkpoint"] = 7.into();
        verification["collision_comparison_reexecuted"] = false.into();
        write_json(&root.join("reports/record-index-cache.json"), &index)?;
    } else {
        write_json(&root.join("reports/nif-collisions.json"), &collisions)?;
    }
    write_json(
        &root.join(format!(
            "reports/checkpoint-{:02}-verification.json",
            args.checkpoint
        )),
        &verification,
    )?;
    eprintln!(
        "Checkpoint {} verified at {revision}: {passed} tests; metadata published",
        args.checkpoint
    );
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
