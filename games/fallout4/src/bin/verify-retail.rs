//! Offline, sequential proof runner. Outputs and extracted data remain local.
use fallout4_prep::{Error, Result, census};
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn run_command(exe: &Path, args: &[PathBuf], stdout: &Path) -> Result<()> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(stdout)?;
    let status = Command::new(exe)
        .args(args)
        .stdout(Stdio::from(output))
        .status()?;
    if !status.success() {
        return Err(Error::Unsupported(format!(
            "{} failed ({status}); retained report {}",
            exe.display(),
            stdout.display()
        )));
    }
    Ok(())
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err(Error::Unsupported("usage: verify-retail <install> <new-output-directory> <pex-oracle-exe> <ba2-oracle-exe>".into()));
    }
    let install = fs::canonicalize(&args[0])?;
    let requested = Path::new(&args[1]);
    let parent = fs::canonicalize(
        requested
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    if parent.starts_with(&install) {
        return Err(Error::Unsupported(
            "proof output must be outside installation".into(),
        ));
    }
    let name = requested
        .file_name()
        .ok_or_else(|| Error::Unsupported("new output directory required".into()))?;
    let output = parent.join(name);
    fs::create_dir(&output)?;
    let own = env::current_exe()?;
    let bin = own.parent().unwrap();
    let cli = bin.join("fallout4-prep.exe");
    let compare = bin.join("verify-pex.exe");
    let pex_oracle = fs::canonicalize(&args[2])?;
    let ba2_oracle = fs::canonicalize(&args[3])?;
    let executables = [&own, &cli, &compare, &pex_oracle, &ba2_oracle];
    let hashes = executables
        .iter()
        .map(|p| census::hash_file(p))
        .collect::<Result<Vec<_>>>()?;
    write_json(
        &output.join("inputs.json"),
        &json!({"executables":executables,"sha256":hashes,"source_pins":serde_json::from_reader::<_,Value>(File::open("sources.lock.json")?)?}),
    )?;
    let corpus_path = output.join("census.json");
    run_command(
        &cli,
        &["census".into(), install.clone(), "--full-hash".into()],
        &corpus_path,
    )?;
    let corpus: Value = serde_json::from_reader(File::open(&corpus_path)?)?;
    let archives = corpus["archives"]
        .as_array()
        .ok_or_else(|| Error::Unsupported("missing archive census".into()))?;
    let mut results = Vec::new();
    let mut pex_files = 0usize;
    let mut compared_bytes = 0u64;
    let mut compared_payloads = 0u64;
    for (i, archive) in archives.iter().enumerate() {
        let archive_name = archive["name"]
            .as_str()
            .ok_or_else(|| Error::Unsupported("missing archive name".into()))?;
        if Path::new(archive_name).components().count() != 1 {
            return Err(Error::Unsupported("non-local archive name".into()));
        }
        eprintln!("Comparing {archive_name}");
        let source = install.join("Data").join(archive_name);
        let folder = output.join(format!("archive-{i:03}"));
        fs::create_dir(&folder)?;
        let archive_json = folder.join("archive.json");
        write_json(&archive_json, archive)?;
        let ba2_report = folder.join("ba2-comparison.json");
        let mut ba2_args = vec![source.clone()];
        // Small script archive is compared exhaustively; large texture/mesh/audio
        // archives use a deterministic sample, with every member's lookup checked.
        if archive_name.eq_ignore_ascii_case("Fallout4 - Misc.ba2") {
            ba2_args.push("--all".into());
        }
        run_command(&ba2_oracle, &ba2_args, &ba2_report)?;
        let ba2_result: Value = serde_json::from_reader(File::open(&ba2_report)?)?;
        compared_bytes += ba2_result["decoded_bytes_compared"].as_u64().unwrap_or(0);
        compared_payloads += ba2_result["payloads_compared"].as_u64().unwrap_or(0);
        let count = archive["pex"].as_array().map_or(0, Vec::len);
        let mut pex_result = Value::Null;
        if count > 0 {
            let extracted = folder.join("pex");
            run_command(
                &cli,
                &["extract-pex".into(), source, extracted.clone()],
                &folder.join("extraction.json"),
            )?;
            let reference = folder.join("reference.jsonl");
            run_command(&pex_oracle, std::slice::from_ref(&extracted), &reference)?;
            let comparison = folder.join("pex-comparison.json");
            run_command(&compare, &[archive_json, extracted, reference], &comparison)?;
            pex_result = serde_json::from_reader(File::open(comparison)?)?;
            pex_files += count;
        }
        results.push(json!({"archive":archive_name,"ba2":ba2_result,"pex":pex_result}));
    }
    for (p, hash) in executables.iter().zip(&hashes) {
        if census::hash_file(p)? != *hash {
            return Err(Error::Unsupported(
                "proof executable changed during run".into(),
            ));
        }
    }
    let summary = json!({"schema":1,"status":"matched","archive_count":archives.len(),"pex_files_compared":pex_files,
        "payloads_compared":compared_payloads,"decoded_bytes_compared":compared_bytes,"census_sha256":census::hash_file(&corpus_path)?,"results":results,
        "limits":["physical archive corpus, not active load order","BA2 payloads sampled except base Misc archive","PEX structure and native signatures compared; no operand-value oracle or execution","plugin framing scanned through shared reader; independent FO4 semantic record oracle remains open"]});
    // Completion marker is written only after every subprocess and comparison.
    write_json(&output.join("complete.json"), &summary)?;
    println!(
        "{}",
        json!({"status":"matched","output":output,"archives":archives.len(),"pex_files_compared":pex_files})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
