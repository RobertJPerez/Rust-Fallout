//! Checkpoint tooling stays separate from the content-inspection CLI. This runner
//! records the commands it actually executes and publishes metadata, never assets.
mod argument_evidence;
mod binding_evidence;
mod catalogue_evidence;
mod compressed_record_evidence;
mod condition_dependency_evidence;
mod condition_evidence;
mod control_flow_evidence;
mod definition_plan_evidence;
mod dialogue_evidence;
mod expression_evidence;
mod expression_plan_evidence;
mod foreign_context_evidence;
mod form_list_evidence;
mod height_evidence;
mod index_evidence;
mod inventory_evidence;
mod item_state_evidence;
mod leveled_evidence;
mod loaded_script_evidence;
mod narrative_evidence;
mod native_migration_evidence;
mod native_save_evidence;
mod operand_evidence;
mod preview_evidence;
mod query_evidence;
mod quest_script_evidence;
mod script_evidence;
mod script_state_evidence;
mod shared_runtime_evidence;
mod source_item_evidence;
mod terrain_evidence;
mod texture_evidence;
mod textured_evidence;

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
    time::{SystemTime, UNIX_EPOCH},
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
    #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u8).range(7..=42))]
    checkpoint: u8,
    /// Repeat verification into the fresh local directory, preserving published reports.
    #[arg(long)]
    no_publish: bool,
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

fn checkpoint_path(root: &Path, checkpoint: u8, name: &str) -> PathBuf {
    root.join(format!("reports/checkpoint-{checkpoint:02}-{name}.json"))
}

fn require_new_reports<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Result<()> {
    for path in paths {
        if path.try_exists()? {
            return Err(format!(
                "Immutable checkpoint report already exists: {}",
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

/// Check every destination before writing any report. Current aliases are updated
/// separately; an earlier checkpoint must never block or be overwritten by a new one.
fn publish_metadata(reports: Vec<(PathBuf, Value)>) -> Result<()> {
    require_new_reports(reports.iter().map(|(path, _)| path))?;
    let reports = reports
        .into_iter()
        .map(|(path, value)| {
            let mut bytes = serde_json::to_vec_pretty(&value)?;
            bytes.push(b'\n');
            Ok((path, bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    for (path, bytes) in reports {
        write_new(&path, &bytes)?;
    }
    Ok(())
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
    let publication_root = if args.no_publish { &destination } else { &root };
    require_new_reports([
        &checkpoint_path(publication_root, args.checkpoint, "source-snapshot"),
        &checkpoint_path(publication_root, args.checkpoint, "verification"),
    ])?;
    let revision = String::from_utf8(git(&root, &["rev-parse", "HEAD"])?)?
        .trim()
        .to_owned();
    let source = snapshot(&root, &revision)?;
    fs::create_dir(&destination)?;
    if args.no_publish {
        fs::create_dir(destination.join("reports"))?;
    }
    let cli_path = root.join("target/release/fallout.exe");
    let oracle_path = root.join("local/nif-oracle-build/Release/nif-oracle.exe");
    let cli_digest = digest(&cli_path)?;
    let oracle_digest = if args.checkpoint == 7 {
        Some(digest(&oracle_path)?)
    } else {
        None
    };
    let terrain_oracle = root.join("local/terrain-oracle-build/Release/terrain-oracle.exe");
    let terrain_digest = if matches!(args.checkpoint, 9..=14) {
        Some(digest(&terrain_oracle)?)
    } else {
        None
    };
    let member_oracle = root.join("target/release/archive-member-oracle.exe");
    let member_digest = if matches!(args.checkpoint, 12..=14) {
        Some(digest(&member_oracle)?)
    } else {
        None
    };
    let preview = root.join("target/debug/fallout-preview.exe");
    let preview_digest = if matches!(args.checkpoint, 11 | 14) {
        Some(digest(&preview)?)
    } else {
        None
    };
    let playtest = root.join("target/debug/fallout-playtest.exe");
    let playtest_digest = if args.checkpoint == 14 {
        Some(digest(&playtest)?)
    } else {
        None
    };
    let playtest_config = root.join("local/playtest.json");
    let playtest_config_digest = if args.checkpoint == 14 {
        Some(digest(&playtest_config)?)
    } else {
        None
    };
    let script_oracle = root.join("local/script-oracle-build/Release/script-oracle.exe");
    let script_digest = if args.checkpoint == 15 {
        Some(digest(&script_oracle)?)
    } else {
        None
    };
    let command_oracle = root.join("local/command-oracle-build/Release/command-oracle.exe");
    let command_digest = if matches!(args.checkpoint, 16 | 18..=21 | 25 | 27 | 30..=42) {
        Some(digest(&command_oracle)?)
    } else {
        None
    };
    let binding_oracle = root.join("local/binding-oracle-build/Release/binding-oracle.exe");
    let binding_digest = if matches!(args.checkpoint, 17 | 20 | 25 | 30..=42) {
        Some(digest(&binding_oracle)?)
    } else {
        None
    };
    let expression_oracle =
        root.join("local/expression-oracle-build/Release/expression-oracle.exe");
    let expression_digest = if matches!(args.checkpoint, 18..=20 | 25 | 30..=42) {
        Some(digest(&expression_oracle)?)
    } else {
        None
    };
    let argument_oracle = root.join("local/argument-oracle-build/Release/argument-oracle.exe");
    let argument_digest = if matches!(args.checkpoint, 19 | 20 | 25 | 30..=42) {
        Some(digest(&argument_oracle)?)
    } else {
        None
    };
    let operand_oracle = root.join("local/operand-oracle-build/Release/operand-oracle.exe");
    let operand_digest = if matches!(args.checkpoint, 20 | 25 | 30..=42) {
        Some(digest(&operand_oracle)?)
    } else {
        None
    };
    let condition_oracle = root.join("local/condition-oracle-build/Release/condition-oracle.exe");
    let condition_digest = if args.checkpoint == 21 {
        Some(digest(&condition_oracle)?)
    } else {
        None
    };
    let narrative_oracle = root.join("local/narrative-oracle-build/Release/narrative-oracle.exe");
    let narrative_digest = if args.checkpoint == 22 {
        Some(digest(&narrative_oracle)?)
    } else {
        None
    };
    let record_oracle = root.join("local/record-oracle-build/Release/record-oracle.exe");
    let record_digest = if matches!(args.checkpoint, 23..=25 | 30..=42) {
        Some(digest(&record_oracle)?)
    } else {
        None
    };
    let loaded_script_oracle =
        root.join("local/script-catalogue-oracle-build/Release/script-catalogue-oracle.exe");
    let loaded_script_digest = if matches!(args.checkpoint, 24 | 25 | 30..=42) {
        Some(digest(&loaded_script_oracle)?)
    } else {
        None
    };
    let quest_script_oracle =
        root.join("local/quest-script-oracle-build/Release/quest-script-oracle.exe");
    let quest_script_digest = if matches!(args.checkpoint, 25 | 30..=42) {
        Some(digest(&quest_script_oracle)?)
    } else {
        None
    };
    let zlib_oracle = root.join("local/zlib-oracle-build/Release/zlib-oracle.exe");
    let zlib_digest = if args.checkpoint == 26 {
        Some(digest(&zlib_oracle)?)
    } else {
        None
    };
    let condition_operand_oracle =
        root.join("local/condition-operand-oracle-build/Release/condition-operand-oracle.exe");
    let condition_operand_digest = if matches!(args.checkpoint, 27 | 36..=42) {
        Some(digest(&condition_operand_oracle)?)
    } else {
        None
    };
    let script_state_oracle =
        root.join("local/script-state-schema-oracle-build/Release/script-state-schema-oracle.exe");
    let script_state_digest = if matches!(args.checkpoint, 28..=42) {
        Some(digest(&script_state_oracle)?)
    } else {
        None
    };
    let native_save_oracle =
        root.join("local/native-save-oracle-build/Release/native-save-oracle.exe");
    let native_save_digest = if matches!(args.checkpoint, 29..=42) {
        Some(digest(&native_save_oracle)?)
    } else {
        None
    };
    let leveled_oracle = root.join("local/leveled-oracle-build/Release/leveled-oracle.exe");
    let leveled_digest = if matches!(args.checkpoint, 32..=42) {
        Some(digest(&leveled_oracle)?)
    } else {
        None
    };
    let inventory_oracle = root.join("local/inventory-oracle-build/Release/inventory-oracle.exe");
    let inventory_digest = if matches!(args.checkpoint, 31..=42) {
        Some(digest(&inventory_oracle)?)
    } else {
        None
    };
    let foreign_context_oracle =
        root.join("local/foreign-context-oracle-build/Release/foreign-context-oracle.exe");
    let foreign_context_digest = if matches!(args.checkpoint, 30..=42) {
        Some(digest(&foreign_context_oracle)?)
    } else {
        None
    };
    let form_list_oracle = root.join("local/form-list-oracle-build/Release/form-list-oracle.exe");
    let form_list_digest = if matches!(args.checkpoint, 38..=42) {
        Some(digest(&form_list_oracle)?)
    } else {
        None
    };
    let expression_plan_oracle =
        root.join("local/expression-plan-oracle-build/Release/expression-plan-oracle.exe");
    let expression_plan_digest = if matches!(args.checkpoint, 39..=42) {
        Some(digest(&expression_plan_oracle)?)
    } else {
        None
    };
    let control_flow_oracle =
        root.join("local/control-flow-oracle-build/Release/control-flow-oracle.exe");
    let control_flow_digest = if matches!(args.checkpoint, 40..=42) {
        Some(digest(&control_flow_oracle)?)
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
    let publication_checks = if matches!(args.checkpoint, 37..=42) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("Ran 5 tests") || !stderr.contains("OK") {
            return Err("Initial report publication checks were not executed successfully".into());
        }
        Some(5_u64)
    } else {
        None
    };
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
    let script_evidence = if args.checkpoint == 15 {
        Some(script_evidence::run(
            &root,
            &destination,
            &cli_path,
            &script_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let catalogue_evidence = if args.checkpoint == 16 {
        Some(catalogue_evidence::run(
            &root,
            &destination,
            &cli_path,
            &command_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let binding_evidence = if args.checkpoint == 17 {
        Some(binding_evidence::run(
            &root,
            &destination,
            &cli_path,
            &binding_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let expression_evidence = if args.checkpoint == 18 {
        Some(expression_evidence::run(
            &root,
            &destination,
            &cli_path,
            &expression_oracle,
            &command_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let argument_evidence = if args.checkpoint == 19 {
        Some(argument_evidence::run(
            &root,
            &destination,
            &cli_path,
            argument_evidence::Oracles {
                arguments: &argument_oracle,
                expressions: &expression_oracle,
                catalogue: &command_oracle,
            },
            &args.install,
        )?)
    } else {
        None
    };
    let operand_evidence = if args.checkpoint == 20 {
        Some(operand_evidence::run(
            &root,
            &destination,
            &cli_path,
            operand_evidence::Oracles {
                operands: &operand_oracle,
                tables: &binding_oracle,
                arguments: &argument_oracle,
                expressions: &expression_oracle,
                catalogue: &command_oracle,
            },
            &args.install,
        )?)
    } else {
        None
    };
    let item_state_evidence = if matches!(args.checkpoint, 33..=42) {
        let runner = match args.checkpoint {
            36..=42 => query_evidence::run,
            35 => source_item_evidence::run,
            34 => native_migration_evidence::run,
            _ => item_state_evidence::run,
        };
        Some(runner(
            &root,
            &destination,
            &cli_path,
            item_state_evidence::Oracles {
                content: leveled_evidence::Oracles {
                    lists: &leveled_oracle,
                    inventory: inventory_evidence::Oracles {
                        inventory: &inventory_oracle,
                        runtime: foreign_context_evidence::Oracles {
                            contexts: &foreign_context_oracle,
                            saves: native_save_evidence::Oracles {
                                container: &native_save_oracle,
                                schemas: &script_state_oracle,
                            },
                            quests: quest_script_evidence::Oracles {
                                quests: &quest_script_oracle,
                                scripts: &loaded_script_oracle,
                                records: &record_oracle,
                                operands: operand_evidence::Oracles {
                                    operands: &operand_oracle,
                                    tables: &binding_oracle,
                                    arguments: &argument_oracle,
                                    expressions: &expression_oracle,
                                    catalogue: &command_oracle,
                                },
                            },
                        },
                    },
                },
            },
            &args.install,
        )?)
    } else {
        None
    };
    let form_list_evidence = if matches!(args.checkpoint, 38..=42) {
        Some(form_list_evidence::run(
            &root,
            &destination,
            &cli_path,
            &form_list_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let expression_plan_evidence = if matches!(args.checkpoint, 39..=42) {
        Some(expression_plan_evidence::run(
            &root,
            &destination,
            &cli_path,
            &expression_plan_oracle,
            &args.install,
            &form_list_evidence
                .as_ref()
                .ok_or("Missing fresh form-list regression")?["sources"],
        )?)
    } else {
        None
    };
    let control_flow_evidence = if matches!(args.checkpoint, 40..=42) {
        Some(control_flow_evidence::run(
            &root,
            &destination,
            &cli_path,
            &control_flow_oracle,
            &args.install,
            &expression_plan_evidence
                .as_ref()
                .ok_or("Missing fresh expression-plan regression")?["sources"],
        )?)
    } else {
        None
    };
    let definition_plan_evidence = if matches!(args.checkpoint, 41 | 42) {
        Some(definition_plan_evidence::run(
            &root,
            &destination,
            &cli_path,
            &args.install,
            &control_flow_evidence
                .as_ref()
                .ok_or("Missing fresh control-flow regression")?["sources"],
        )?)
    } else {
        None
    };
    let shared_runtime_evidence = if args.checkpoint == 42 {
        Some(shared_runtime_evidence::run(
            &root,
            &destination,
            &cli_path,
            &args.install,
            &definition_plan_evidence
                .as_ref()
                .ok_or("Missing fresh source-plan regression")?["sources"],
        )?)
    } else {
        None
    };
    let leveled_evidence = if args.checkpoint == 32 {
        Some(leveled_evidence::run(
            &root,
            &destination,
            &cli_path,
            leveled_evidence::Oracles {
                lists: &leveled_oracle,
                inventory: inventory_evidence::Oracles {
                    inventory: &inventory_oracle,
                    runtime: foreign_context_evidence::Oracles {
                        contexts: &foreign_context_oracle,
                        saves: native_save_evidence::Oracles {
                            container: &native_save_oracle,
                            schemas: &script_state_oracle,
                        },
                        quests: quest_script_evidence::Oracles {
                            quests: &quest_script_oracle,
                            scripts: &loaded_script_oracle,
                            records: &record_oracle,
                            operands: operand_evidence::Oracles {
                                operands: &operand_oracle,
                                tables: &binding_oracle,
                                arguments: &argument_oracle,
                                expressions: &expression_oracle,
                                catalogue: &command_oracle,
                            },
                        },
                    },
                },
            },
            &args.install,
        )?)
    } else {
        None
    };
    let inventory_evidence = if args.checkpoint == 31 {
        Some(inventory_evidence::run(
            &root,
            &destination,
            &cli_path,
            inventory_evidence::Oracles {
                inventory: &inventory_oracle,
                runtime: foreign_context_evidence::Oracles {
                    contexts: &foreign_context_oracle,
                    saves: native_save_evidence::Oracles {
                        container: &native_save_oracle,
                        schemas: &script_state_oracle,
                    },
                    quests: quest_script_evidence::Oracles {
                        quests: &quest_script_oracle,
                        scripts: &loaded_script_oracle,
                        records: &record_oracle,
                        operands: operand_evidence::Oracles {
                            operands: &operand_oracle,
                            tables: &binding_oracle,
                            arguments: &argument_oracle,
                            expressions: &expression_oracle,
                            catalogue: &command_oracle,
                        },
                    },
                },
            },
            &args.install,
        )?)
    } else {
        None
    };
    let foreign_context_evidence = if args.checkpoint == 30 {
        Some(foreign_context_evidence::run(
            &root,
            &destination,
            &cli_path,
            foreign_context_evidence::Oracles {
                contexts: &foreign_context_oracle,
                saves: native_save_evidence::Oracles {
                    container: &native_save_oracle,
                    schemas: &script_state_oracle,
                },
                quests: quest_script_evidence::Oracles {
                    quests: &quest_script_oracle,
                    scripts: &loaded_script_oracle,
                    records: &record_oracle,
                    operands: operand_evidence::Oracles {
                        operands: &operand_oracle,
                        tables: &binding_oracle,
                        arguments: &argument_oracle,
                        expressions: &expression_oracle,
                        catalogue: &command_oracle,
                    },
                },
            },
            &args.install,
        )?)
    } else {
        None
    };
    let native_save_evidence = if args.checkpoint == 29 {
        Some(native_save_evidence::run(
            &root,
            &destination,
            &cli_path,
            native_save_evidence::Oracles {
                container: &native_save_oracle,
                schemas: &script_state_oracle,
            },
            &args.install,
        )?)
    } else {
        None
    };
    let script_state_evidence = if args.checkpoint == 28 {
        Some(script_state_evidence::run(
            &root,
            &destination,
            &cli_path,
            &script_state_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let condition_dependency_evidence = if args.checkpoint == 27 {
        Some(condition_dependency_evidence::run(
            &root,
            &destination,
            &cli_path,
            &condition_operand_oracle,
            &command_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let compressed_record_evidence = if args.checkpoint == 26 {
        Some(compressed_record_evidence::run(
            &root,
            &destination,
            &cli_path,
            &zlib_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let quest_script_evidence = if args.checkpoint == 25 {
        Some(quest_script_evidence::run(
            &root,
            &destination,
            &cli_path,
            quest_script_evidence::Oracles {
                quests: &quest_script_oracle,
                scripts: &loaded_script_oracle,
                records: &record_oracle,
                operands: operand_evidence::Oracles {
                    operands: &operand_oracle,
                    tables: &binding_oracle,
                    arguments: &argument_oracle,
                    expressions: &expression_oracle,
                    catalogue: &command_oracle,
                },
            },
            &args.install,
        )?)
    } else {
        None
    };
    let loaded_script_evidence = if args.checkpoint == 24 {
        Some(loaded_script_evidence::run(
            &root,
            &destination,
            &cli_path,
            loaded_script_evidence::Oracles {
                scripts: &loaded_script_oracle,
                records: &record_oracle,
            },
            &args.install,
        )?)
    } else {
        None
    };
    let dialogue_evidence = if args.checkpoint == 23 {
        Some(dialogue_evidence::run(
            &root,
            &destination,
            &cli_path,
            &record_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let narrative_evidence = if args.checkpoint == 22 {
        Some(narrative_evidence::run(
            &root,
            &destination,
            &cli_path,
            &narrative_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let condition_evidence = if args.checkpoint == 21 {
        Some(condition_evidence::run(
            &root,
            &destination,
            &cli_path,
            &condition_oracle,
            &command_oracle,
            &args.install,
        )?)
    } else {
        None
    };
    let mut terrain_evidence = if matches!(args.checkpoint, 9..=14) {
        Some(terrain_evidence::run(
            &root,
            &destination,
            &cli_path,
            terrain_evidence::OracleRun {
                binary: &terrain_oracle,
                sha256: terrain_digest
                    .as_deref()
                    .ok_or("Missing terrain oracle digest")?,
                heights: args.checkpoint >= 10,
                geometry: args.checkpoint >= 11,
                textures: args.checkpoint >= 12,
                blends: args.checkpoint >= 13,
            },
            &args.install,
            &cli_digest,
        )?)
    } else {
        None
    };
    if args.checkpoint == 10 {
        let additional = height_evidence::run(
            &root,
            &destination,
            &cli_path,
            &terrain_oracle,
            &args.install,
            terrain_digest
                .as_deref()
                .ok_or("Missing height oracle identity")?,
        )?;
        terrain_evidence
            .as_mut()
            .ok_or("Missing height comparisons")?["supplemental"] = additional;
    }
    if let Some(sha) = &member_digest {
        terrain_evidence
            .as_mut()
            .ok_or("Missing texture field evidence")?["texture_dependencies"] =
            texture_evidence::run(
                &root,
                &destination,
                &cli_path,
                &member_oracle,
                &args.install,
                sha,
            )?;
    }
    let gpu_evidence = if let Some(sha) = &preview_digest {
        Some(if args.checkpoint == 14 {
            textured_evidence::run(&root, &destination, &preview, &args.install, sha)?
        } else {
            preview_evidence::run(&root, &destination, &preview, &args.install, sha)?
        })
    } else {
        None
    };
    let launcher_evidence = if args.checkpoint == 14 {
        let mut command = Command::new(&playtest);
        command.current_dir(&root).arg("--check");
        let output = run_logged(command, &destination.join("playtest-ready.log"))?;
        let path = destination.join("playtest-ready.json");
        write_new(&path, &output.stdout)?;
        let receipt = json_file(&path)?;
        if receipt["ready"] != true
            || receipt["window_launched"] != false
            || receipt["terrain"] != "Goodsprings"
            || receipt["texture_repeats_per_quadrant"] != 4.
            || Path::new(
                receipt["preview"]
                    .as_str()
                    .ok_or("Missing launcher preview")?,
            )
            .canonicalize()?
                != preview.canonicalize()?
            || Path::new(
                receipt["install"]
                    .as_str()
                    .ok_or("Missing launcher installation")?,
            )
            .canonicalize()?
                != args.install.canonicalize()?
            || Path::new(
                receipt["load_order"]
                    .as_str()
                    .ok_or("Missing launcher load order")?,
            )
            .canonicalize()?
                != root
                    .join("profiles/nv-inspection-order.json")
                    .canonicalize()?
        {
            return Err("Manual test launcher configuration differs from GPU fixture".into());
        }
        Some(
            json!({"binary_sha256":playtest_digest,"configuration_sha256":playtest_config_digest,
            "readiness_report_sha256":digest(&path)?,"ready":true,"interactive_input_tested":false,
            "scope":"Rust launcher path/config validation; matching terrain/tiling/source paths; headless viewer tested separately; direct user input remains a manual check"}),
        )
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
        || preview_digest
            .as_ref()
            .is_some_and(|expected| digest(&preview).as_ref().ok() != Some(expected))
        || member_digest
            .as_ref()
            .is_some_and(|expected| digest(&member_oracle).as_ref().ok() != Some(expected))
        || playtest_digest
            .as_ref()
            .is_some_and(|expected| digest(&playtest).as_ref().ok() != Some(expected))
        || playtest_config_digest
            .as_ref()
            .is_some_and(|expected| digest(&playtest_config).as_ref().ok() != Some(expected))
        || script_digest
            .as_ref()
            .is_some_and(|expected| digest(&script_oracle).as_ref().ok() != Some(expected))
        || command_digest
            .as_ref()
            .is_some_and(|expected| digest(&command_oracle).as_ref().ok() != Some(expected))
        || binding_digest
            .as_ref()
            .is_some_and(|expected| digest(&binding_oracle).as_ref().ok() != Some(expected))
        || record_digest
            .as_ref()
            .is_some_and(|expected| digest(&record_oracle).as_ref().ok() != Some(expected))
        || loaded_script_digest
            .as_ref()
            .is_some_and(|expected| digest(&loaded_script_oracle).as_ref().ok() != Some(expected))
        || native_save_digest
            .as_ref()
            .is_some_and(|expected| digest(&native_save_oracle).as_ref().ok() != Some(expected))
        || control_flow_digest
            .as_ref()
            .is_some_and(|expected| digest(&control_flow_oracle).as_ref().ok() != Some(expected))
        || expression_plan_digest
            .as_ref()
            .is_some_and(|expected| digest(&expression_plan_oracle).as_ref().ok() != Some(expected))
        || form_list_digest
            .as_ref()
            .is_some_and(|expected| digest(&form_list_oracle).as_ref().ok() != Some(expected))
        || leveled_digest
            .as_ref()
            .is_some_and(|expected| digest(&leveled_oracle).as_ref().ok() != Some(expected))
        || inventory_digest
            .as_ref()
            .is_some_and(|expected| digest(&inventory_oracle).as_ref().ok() != Some(expected))
        || foreign_context_digest
            .as_ref()
            .is_some_and(|expected| digest(&foreign_context_oracle).as_ref().ok() != Some(expected))
        || script_state_digest
            .as_ref()
            .is_some_and(|expected| digest(&script_state_oracle).as_ref().ok() != Some(expected))
        || condition_operand_digest.as_ref().is_some_and(|expected| {
            digest(&condition_operand_oracle).as_ref().ok() != Some(expected)
        })
        || zlib_digest
            .as_ref()
            .is_some_and(|expected| digest(&zlib_oracle).as_ref().ok() != Some(expected))
        || quest_script_digest
            .as_ref()
            .is_some_and(|expected| digest(&quest_script_oracle).as_ref().ok() != Some(expected))
        || narrative_digest
            .as_ref()
            .is_some_and(|expected| digest(&narrative_oracle).as_ref().ok() != Some(expected))
        || condition_digest
            .as_ref()
            .is_some_and(|expected| digest(&condition_oracle).as_ref().ok() != Some(expected))
        || operand_digest
            .as_ref()
            .is_some_and(|expected| digest(&operand_oracle).as_ref().ok() != Some(expected))
        || argument_digest
            .as_ref()
            .is_some_and(|expected| digest(&argument_oracle).as_ref().ok() != Some(expected))
        || expression_digest
            .as_ref()
            .is_some_and(|expected| digest(&expression_oracle).as_ref().ok() != Some(expected))
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
        "schema_version":1, "verification_finished_unix_seconds_utc":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        "checkpoint":args.checkpoint, "engine_revision":revision,
        "source_snapshot_sha256":source["sha256"], "source_matches_implementation_commit":true,
        "release_cli_sha256":cli_digest, "raw_oracle_binary_sha256":oracle_digest,
        "evidence_runner_sha256":digest(&std::env::current_exe()?)?,
        "check_command":"powershell -NoProfile -ExecutionPolicy Bypass -File tools/check.ps1",
        "check_exit_code":0, "check_log_sha256":digest(&check_log)?, "tests_passed":passed,
        "format_check":"passed", "clippy_warnings_denied":"passed",
        "python_publication_tests_passed":publication_checks,
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
    if let Some(launcher) = launcher_evidence {
        verification["manual_test_launcher"] = launcher;
    }
    let mut publication = vec![(
        checkpoint_path(publication_root, args.checkpoint, "source-snapshot"),
        source.clone(),
    )];
    if let Some(mut terrain) = terrain_evidence {
        terrain["checkpoint"] = args.checkpoint.into();
        terrain["engine_revision"] = revision.clone().into();
        terrain["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Verification object missing")?
            .remove("fresh_collision_comparison");
        let (key, name) = if args.checkpoint >= 13 {
            ("terrain_blends", "terrain-blends")
        } else if args.checkpoint == 12 {
            ("terrain_textures", "terrain-textures")
        } else if args.checkpoint == 11 {
            ("terrain_geometry", "terrain-geometry")
        } else if args.checkpoint == 10 {
            ("terrain_heights", "terrain-heights")
        } else {
            ("exterior_fields", "exterior-fields")
        };
        verification[key] = terrain.clone();
        verification["terrain_oracle_binary_sha256"] = terrain_digest.into();
        verification["collision_evidence_origin_checkpoint"] = 7.into();
        verification["collision_comparison_reexecuted"] = false.into();
        verification["record_index_cache_evidence_origin_checkpoint"] = 8.into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, name),
            terrain,
        ));
        if args.checkpoint >= 12 {
            verification["archive_member_oracle_binary_sha256"] = member_digest.clone().into();
            verification["terrain_presentation_evidence_origin_checkpoint"] = 11.into();
            verification["terrain_presentation_reexecuted"] = false.into();
        }
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
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "record-index-cache"),
            index,
        ));
    } else if let Some(mut scripts) = script_evidence {
        scripts["checkpoint"] = args.checkpoint.into();
        scripts["engine_revision"] = revision.clone().into();
        scripts["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["compiled_scripts"] = scripts.clone();
        verification["script_oracle_binary_sha256"] = script_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this script checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "compiled-scripts"),
            scripts,
        ));
    } else if let Some(mut catalogue) = catalogue_evidence {
        catalogue["checkpoint"] = args.checkpoint.into();
        catalogue["engine_revision"] = revision.clone().into();
        catalogue["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["command_catalogue"] = catalogue.clone();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this metadata checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "command-catalogue"),
            catalogue,
        ));
    } else if let Some(mut expressions) = expression_evidence {
        expressions["checkpoint"] = args.checkpoint.into();
        expressions["engine_revision"] = revision.clone().into();
        expressions["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["script_expressions"] = expressions.clone();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this expression checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "script-expressions"),
            expressions,
        ));
    } else if let Some(mut arguments) = argument_evidence {
        arguments["checkpoint"] = args.checkpoint.into();
        arguments["engine_revision"] = revision.clone().into();
        arguments["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["native_arguments"] = arguments.clone();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this operand checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "native-arguments"),
            arguments,
        ));
    } else if let Some(mut operands) = operand_evidence {
        operands["checkpoint"] = args.checkpoint.into();
        operands["engine_revision"] = revision.clone().into();
        operands["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["operand_bindings"] = operands.clone();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this table-association checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "operand-bindings"),
            operands,
        ));
    } else if let Some(mut items) = item_state_evidence {
        if let Some(mut lists) = form_list_evidence {
            if lists["sources"] != items["sources"] {
                return Err("Form list/query source cohorts differ".into());
            }
            lists["fresh_shared_query_regression"] = items;
            items = lists;
            verification["form_list_oracle_binary_sha256"] = form_list_digest.clone().into();
        }
        if let Some(mut plans) = expression_plan_evidence {
            if plans["sources"] != items["sources"] {
                return Err("Plan/form-list source cohorts differ".into());
            }
            plans["fresh_form_list_regression"] = items;
            items = plans;
            verification["expression_plan_oracle_binary_sha256"] =
                expression_plan_digest.clone().into();
        }
        if let Some(mut structure) = control_flow_evidence {
            if structure["sources"] != items["sources"] {
                return Err("Control-flow/expression source cohorts differ".into());
            }
            structure["fresh_expression_plan_regression"] = items;
            items = structure;
            verification["control_flow_oracle_binary_sha256"] = control_flow_digest.clone().into();
        }
        if let Some(mut definitions) = definition_plan_evidence {
            if definitions["sources"] != items["sources"] {
                return Err("Prepared/control-flow source cohorts differ".into());
            }
            definitions["fresh_control_flow_regression"] = items;
            items = definitions;
        }
        if let Some(mut owned) = shared_runtime_evidence {
            if owned["sources"] != items["sources"] {
                return Err("Owned runtime/source-plan source cohorts differ".into());
            }
            owned["fresh_source_plan_regression"] = items;
            items = owned;
        }
        if args.checkpoint == 37 {
            items["initial_report_publication_checks"] = json!({"tests_passed":publication_checks,"scope":"Required new local staging, occupied destinations/files, protected current metadata and actual seven-file bootstrap publication; engineering fixtures only","source":"tools/test_report_publication.py","current_metadata_preserved":true});
            items["scope"] = "Initial census publication protection plus fresh source-bound query/condition/runtime regressions; no original behavior acceptance".into();
        }
        items["checkpoint"] = args.checkpoint.into();
        items["engine_revision"] = revision.clone().into();
        items["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        let item_report = match args.checkpoint {
            42 => "shared-runtime",
            41 => "source-plans",
            40 => "control-flow",
            39 => "expression-plans",
            38 => "form-lists",
            37 => "report-publication",
            36 => "primitive-queries",
            35 => "source-items",
            34 => "native-migration",
            _ => "item-state",
        };
        verification[item_report] = items.clone();
        if matches!(args.checkpoint, 36..=42) {
            verification["condition_operand_oracle_binary_sha256"] =
                condition_operand_digest.clone().into();
        }
        verification["leveled_oracle_binary_sha256"] = leveled_digest.into();
        verification["inventory_oracle_binary_sha256"] = inventory_digest.into();
        verification["foreign_context_oracle_binary_sha256"] = foreign_context_digest.into();
        verification["native_save_oracle_binary_sha256"] = native_save_digest.into();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["quest_script_oracle_binary_sha256"] = quest_script_digest.into();
        verification["loaded_script_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted here; complete foreign context, native save/schema and quest/operand/header regressions freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, item_report),
            items,
        ));
    } else if let Some(mut lists) = leveled_evidence {
        lists["checkpoint"] = args.checkpoint.into();
        lists["engine_revision"] = revision.clone().into();
        lists["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["leveled_lists"] = lists.clone();
        verification["leveled_oracle_binary_sha256"] = leveled_digest.into();
        verification["inventory_oracle_binary_sha256"] = inventory_digest.into();
        verification["foreign_context_oracle_binary_sha256"] = foreign_context_digest.into();
        verification["native_save_oracle_binary_sha256"] = native_save_digest.into();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["quest_script_oracle_binary_sha256"] = quest_script_digest.into();
        verification["loaded_script_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted here; complete foreign context, native save/schema and quest/operand/header regressions freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "leveled-lists"),
            lists,
        ));
    } else if let Some(mut inventory) = inventory_evidence {
        inventory["checkpoint"] = args.checkpoint.into();
        inventory["engine_revision"] = revision.clone().into();
        inventory["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["base_inventory"] = inventory.clone();
        verification["inventory_oracle_binary_sha256"] = inventory_digest.into();
        verification["foreign_context_oracle_binary_sha256"] = foreign_context_digest.into();
        verification["native_save_oracle_binary_sha256"] = native_save_digest.into();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["quest_script_oracle_binary_sha256"] = quest_script_digest.into();
        verification["loaded_script_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted here; complete foreign context, native save/schema and quest/operand/header regressions freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "base-inventory"),
            inventory,
        ));
    } else if let Some(mut contexts) = foreign_context_evidence {
        contexts["checkpoint"] = args.checkpoint.into();
        contexts["engine_revision"] = revision.clone().into();
        contexts["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["foreign_contexts"] = contexts.clone();
        verification["foreign_context_oracle_binary_sha256"] = foreign_context_digest.into();
        verification["native_save_oracle_binary_sha256"] = native_save_digest.into();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["quest_script_oracle_binary_sha256"] = quest_script_digest.into();
        verification["loaded_script_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted here; complete native saves, schemas, quest attachments and operand/header regressions freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "foreign-contexts"),
            contexts,
        ));
    } else if let Some(mut saves) = native_save_evidence {
        saves["checkpoint"] = args.checkpoint.into();
        saves["engine_revision"] = revision.clone().into();
        saves["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["native_saves"] = saves.clone();
        verification["native_save_oracle_binary_sha256"] = native_save_digest.into();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; neither reexecuted in this native-persistence checkpoint; full compiled-schema/state regression freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "native-saves"),
            saves,
        ));
    } else if let Some(mut state) = script_state_evidence {
        state["checkpoint"] = args.checkpoint.into();
        state["engine_revision"] = revision.clone().into();
        state["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["script_state"] = state.clone();
        verification["script_state_schema_oracle_binary_sha256"] = script_state_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; neither reexecuted in this compiled-schema and canonical-state checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "script-state"),
            state,
        ));
    } else if let Some(mut dependencies) = condition_dependency_evidence {
        dependencies["checkpoint"] = args.checkpoint.into();
        dependencies["engine_revision"] = revision.clone().into();
        dependencies["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["condition_dependencies"] = dependencies.clone();
        verification["condition_operand_oracle_binary_sha256"] = condition_operand_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; neither reexecuted in this condition-dependency checkpoint; executable descriptors freshly compared".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "condition-dependencies"),
            dependencies,
        ));
    } else if let Some(mut compressed) = compressed_record_evidence {
        compressed["checkpoint"] = args.checkpoint.into();
        compressed["engine_revision"] = revision.clone().into();
        compressed["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["compressed_records"] = compressed.clone();
        verification["zlib_oracle_binary_sha256"] = zlib_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; neither reexecuted in this independent compressed-byte extraction checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "compressed-records"),
            compressed,
        ));
    } else if let Some(mut quests) = quest_script_evidence {
        quests["checkpoint"] = args.checkpoint.into();
        quests["engine_revision"] = revision.clone().into();
        quests["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["quest_scripts"] = quests.clone();
        verification["quest_script_oracle_binary_sha256"] = quest_script_digest.into();
        verification["script_catalogue_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["operand_oracle_binary_sha256"] = operand_digest.into();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["argument_oracle_binary_sha256"] = argument_digest.into();
        verification["expression_oracle_binary_sha256"] = expression_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"]="Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted in this static quest/declaration checkpoint; complete loaded/header/cache and operand/table/native/expression regressions freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "quest-scripts"),
            quests,
        ));
    } else if let Some(mut scripts) = loaded_script_evidence {
        scripts["checkpoint"] = args.checkpoint.into();
        scripts["engine_revision"] = revision.clone().into();
        scripts["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["loaded_scripts"] = scripts.clone();
        verification["script_catalogue_oracle_binary_sha256"] = loaded_script_digest.into();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separate startup fix; not reexecuted in this loaded-script checkpoint; original header/membership/cache comparison freshly repeated".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "loaded-scripts"),
            scripts,
        ));
    } else if let Some(mut dialogue) = dialogue_evidence {
        dialogue["checkpoint"] = args.checkpoint.into();
        dialogue["engine_revision"] = revision.clone().into();
        dialogue["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["dialogue_membership"] = dialogue.clone();
        verification["record_oracle_binary_sha256"] = record_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; not reexecuted in this header/membership checkpoint; selected-cell cache regression freshly compared".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "dialogue-membership"),
            dialogue,
        ));
    } else if let Some(mut narrative) = narrative_evidence {
        narrative["checkpoint"] = args.checkpoint.into();
        narrative["engine_revision"] = revision.clone().into();
        narrative["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["narrative_structure"] = narrative.clone();
        verification["narrative_oracle_binary_sha256"] = narrative_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this quest/dialogue ownership checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "narrative-structure"),
            narrative,
        ));
    } else if let Some(mut conditions) = condition_evidence {
        conditions["checkpoint"] = args.checkpoint.into();
        conditions["engine_revision"] = revision.clone().into();
        conditions["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["condition_fields"] = conditions.clone();
        verification["condition_oracle_binary_sha256"] = condition_digest.into();
        verification["command_oracle_binary_sha256"] = command_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this condition field checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "condition-fields"),
            conditions,
        ));
    } else if let Some(mut bindings) = binding_evidence {
        bindings["checkpoint"] = args.checkpoint.into();
        bindings["engine_revision"] = revision.clone().into();
        bindings["source_snapshot_sha256"] = source["sha256"].clone();
        verification
            .as_object_mut()
            .ok_or("Missing verification object")?
            .remove("fresh_collision_comparison");
        verification["script_bindings"] = bindings.clone();
        verification["binding_oracle_binary_sha256"] = binding_digest.into();
        verification["presentation_evidence_origin_checkpoint"] = 14.into();
        verification["presentation_scope"] = "Prior checkpoint 14 GPU evidence and separately scoped startup fix; neither reexecuted in this binding checkpoint".into();
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "script-bindings"),
            bindings,
        ));
    } else {
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, "nif-collisions"),
            collisions,
        ));
    }
    if let Some(mut gpu) = gpu_evidence {
        gpu["engine_revision"] = revision.clone().into();
        gpu["source_snapshot_sha256"] = source["sha256"].clone();
        let (key, name) = if args.checkpoint == 14 {
            ("terrain_textured_preview", "terrain-textured-preview")
        } else {
            ("terrain_preview", "terrain-preview")
        };
        verification[key] = gpu.clone();
        verification["preview_binary_sha256"] = preview_digest.into();
        verification["presentation_reexecuted"] = true.into();
        verification["presentation_scope"] = if args.checkpoint==14 { "Three textured terrain inspections, vertex-color/interior regressions and 64 numeric synthetic GPU checks; no retail comparison" }
            else { "Terrain GPU smoke captures and interior assembly regression; checkpoint 06 material oracle checks were not repeated" }.into();
        if args.checkpoint == 14 {
            verification["presentation_evidence_origin_checkpoint"] = 14.into();
            verification["terrain_presentation_evidence_origin_checkpoint"] = 14.into();
            verification["terrain_presentation_reexecuted"] = true.into();
        }
        publication.push((
            checkpoint_path(publication_root, args.checkpoint, name),
            gpu,
        ));
    }
    publication.push((
        checkpoint_path(publication_root, args.checkpoint, "verification"),
        verification,
    ));
    publish_metadata(publication)?;
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

#[cfg(test)]
mod publication_tests {
    use super::*;

    fn scratch() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "fallout-publication-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("reports")).unwrap();
        root
    }

    fn cleanup(root: &Path, files: &[PathBuf]) {
        for path in files {
            assert_eq!(path.parent(), Some(root.join("reports").as_path()));
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(root.join("reports")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn existing_report_blocks_publication_before_any_new_file_is_written() {
        let root = scratch();
        let first = checkpoint_path(&root, 14, "source-snapshot");
        let occupied = checkpoint_path(&root, 14, "terrain-blends");
        fs::write(&occupied, b"keep this receipt").unwrap();
        assert!(
            publish_metadata(vec![
                (first.clone(), json!({})),
                (occupied.clone(), json!({}))
            ])
            .is_err()
        );
        assert!(!first.exists());
        assert_eq!(fs::read(&occupied).unwrap(), b"keep this receipt");
        cleanup(&root, &[occupied]);
    }

    #[test]
    fn new_checkpoint_publishes_beside_an_unchanged_current_alias() {
        let root = scratch();
        let alias = root.join("reports/terrain-blends.json");
        fs::write(&alias, b"previous checkpoint bytes").unwrap();
        let body = checkpoint_path(&root, 14, "terrain-blends");
        let verification = checkpoint_path(&root, 14, "verification");
        publish_metadata(vec![
            (body.clone(), json!({"checkpoint":14})),
            (verification.clone(), json!({"checked":true})),
        ])
        .unwrap();
        assert_eq!(fs::read(&alias).unwrap(), b"previous checkpoint bytes");
        assert_eq!(json_file(&body).unwrap(), json!({"checkpoint":14}));
        assert_eq!(json_file(&verification).unwrap(), json!({"checked":true}));
        cleanup(&root, &[alias, body, verification]);
    }
}
