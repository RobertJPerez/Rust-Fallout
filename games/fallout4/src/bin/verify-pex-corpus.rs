//! Incremental operand proof against the immutable extracts of a completed census.
use fallout4_prep::{Error, Result, census};
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn write(path: &Path, value: &Value) -> Result<()> {
    serde_json::to_writer_pretty(
        OpenOptions::new().write(true).create_new(true).open(path)?,
        value,
    )?;
    Ok(())
}
fn command(exe: &Path, args: &[PathBuf], output: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let status = Command::new(exe)
        .args(args)
        .stdout(Stdio::from(file))
        .status()?;
    if !status.success() {
        return Err(Error::Unsupported(format!(
            "{} failed ({status}); evidence retained at {}",
            exe.display(),
            output.display()
        )));
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(Error::Unsupported(
            "verify-pex-corpus <completed-proof> <oracle-exe> <new-local-output>".into(),
        ));
    }
    let prior = fs::canonicalize(&args[0])?;
    let complete: Value = serde_json::from_reader(File::open(prior.join("complete.json"))?)?;
    let corpus_path = prior.join("census.json");
    if complete["status"] != "matched"
        || complete["census_sha256"] != census::hash_file(&corpus_path)?
    {
        return Err(Error::Unsupported(
            "prior proof incomplete or census changed".into(),
        ));
    }
    let corpus: Value = serde_json::from_reader(File::open(corpus_path)?)?;
    let requested = Path::new(&args[2]);
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    if !parent.starts_with(fs::canonicalize("local")?) || parent.starts_with(&prior) {
        return Err(Error::Unsupported(
            "new proof must be under private local/, outside the prior proof".into(),
        ));
    }
    let output = parent.join(
        requested
            .file_name()
            .ok_or_else(|| Error::Unsupported("output name required".into()))?,
    );
    fs::create_dir(&output)?;
    let own = env::current_exe()?;
    let verify = own.parent().unwrap().join("verify-pex.exe");
    let oracle = fs::canonicalize(&args[1])?;
    let binaries = [&own, &verify, &oracle];
    let fingerprints = binaries
        .iter()
        .map(|p| census::hash_file(p))
        .collect::<Result<Vec<_>>>()?;
    write(
        &output.join("inputs.json"),
        &json!({"prior_proof":prior,"prior_completion_sha256":census::hash_file(&prior.join("complete.json"))?,"binaries":binaries,"sha256":fingerprints,"source_pins":serde_json::from_reader::<_,Value>(File::open("sources.lock.json")?)?}),
    )?;
    let mut results = Vec::new();
    let mut files = 0u64;
    let mut tokens = 0u64;
    let mut instructions = 0u64;
    let archives = corpus["archives"]
        .as_array()
        .ok_or_else(|| Error::Unsupported("missing archive list".into()))?;
    for (i, archive) in archives.iter().enumerate() {
        if archive["pex"].as_array().is_none_or(|a| a.is_empty()) {
            continue;
        }
        eprintln!("PEX definitions/operands {}", archive["name"]);
        let old = prior.join(format!("archive-{i:03}"));
        let archive_json = old.join("archive.json");
        if serde_json::from_reader::<_, Value>(File::open(&archive_json)?)? != *archive {
            return Err(Error::Unsupported(
                "saved archive report differs from frozen census".into(),
            ));
        }
        let new = output.join(format!("archive-{i:03}"));
        fs::create_dir(&new)?;
        let reference = new.join("reference.jsonl");
        let extracted = old.join("pex");
        command(&oracle, std::slice::from_ref(&extracted), &reference)?;
        let comparison = new.join("comparison.json");
        command(
            &verify,
            &[
                archive_json,
                extracted,
                reference.clone(),
                "--values".into(),
            ],
            &comparison,
        )?;
        let report: Value = serde_json::from_reader(File::open(&comparison)?)?;
        files += report["files"]
            .as_u64()
            .ok_or_else(|| Error::Unsupported("missing file count".into()))?;
        tokens += report["semantic_tokens_compared"]
            .as_u64()
            .ok_or_else(|| Error::Unsupported("missing token count".into()))?;
        instructions += report["instructions_counted"]
            .as_u64()
            .ok_or_else(|| Error::Unsupported("missing instruction count".into()))?;
        results.push(json!({"archive":archive["name"],"comparison":report,"reference_sha256":census::hash_file(&reference)?,"comparison_sha256":census::hash_file(&comparison)?}));
    }
    if complete["pex_files_compared"] != files || files == 0 {
        return Err(Error::Unsupported("PEX proof cardinality differs".into()));
    }
    for (p, h) in binaries.iter().zip(&fingerprints) {
        if census::hash_file(p)? != *h {
            return Err(Error::Unsupported("proof executable changed".into()));
        }
    }
    let result = json!({"schema":1,"status":"matched","files":files,"instructions":instructions,"semantic_tokens":tokens,"archives":results.len(),"results":results,"scope":"frozen physical PEX corpus; all definitions/operands/scalar bits/string bytes; excludes header/debug metadata, executable linking and behavior"});
    write(&output.join("complete.json"), &result)?;
    println!(
        "{}",
        json!({"status":"matched","output":output,"files":files,"instructions":instructions,"semantic_tokens":tokens})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
