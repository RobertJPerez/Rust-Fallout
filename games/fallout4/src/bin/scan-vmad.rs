//! Produce immutable, private VMAD evidence for the independent reader.
use fallout4_prep::{Error, Result, attachments, census};
use serde_json::json;
use std::{
    env, fs,
    io::{BufWriter, Write},
    path::Path,
};

fn run() -> Result<bool> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::Unsupported(
            "scan-vmad <install-root> <new-private-directory>".into(),
        ));
    }
    let source = fs::canonicalize(&args[0])?;
    let requested = Path::new(&args[1]);
    let parent = fs::canonicalize(
        requested
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    if parent.starts_with(&source) {
        return Err(Error::Unsupported(
            "output must be outside source installation".into(),
        ));
    }
    let destination = parent.join(
        requested
            .file_name()
            .ok_or_else(|| Error::Unsupported("output directory name required".into()))?,
    );
    fs::create_dir(&destination)?;
    let mut inputs = Vec::new();
    let mut paths = fs::read_dir(source.join("Data"))?
        .map(|p| p.map(|p| p.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|p| {
        p.is_file()
            && p.extension()
                .and_then(|p| p.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "esm" | "esp" | "esl"))
    });
    paths.sort();
    let mut counts = attachments::Counts::default();
    let mut failures = Vec::new();
    let mut index = 0;
    let mut manifest = BufWriter::new(fs::File::create(destination.join("attachments.jsonl"))?);
    for path in paths {
        eprintln!("VMAD {}", path.display());
        let before = census::hash_file(&path)?;
        attachments::visit_plugin(&path, |origin, raw, decoded| {
            let filename = format!("{index:08}.vmad");
            fs::write(destination.join(&filename), raw)?;
            let (tokens, error) = match decoded {
                Ok(a) => {
                    counts.add(&a);
                    (Some(attachments::tokens(&a)), None)
                }
                Err(e) => {
                    let error = e.to_string();
                    failures.push(json!({"origin":origin,"error":error}));
                    (None, Some(error))
                }
            };
            serde_json::to_writer(
                &mut manifest,
                &json!({"file":filename,"origin":origin,"bytes":raw.len(),"sha256":census::sha256(raw),"tokens":tokens,"error":error}),
            )?;
            manifest.write_all(b"\n")?;
            index += 1;
            Ok(())
        })?;
        let after = census::hash_file(&path)?;
        if before != after {
            return Err(Error::Unsupported(format!(
                "source changed: {}",
                path.display()
            )));
        }
        inputs.push(json!({"path":path,"sha256":before}));
    }
    manifest.flush()?;
    let complete = failures.is_empty();
    let report = json!({"schema":1,"decoded_all":complete,"vmad_fields":index,"counts":counts,"failures":failures,"inputs":inputs});
    fs::write(
        destination.join("scan.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{}",
        json!({"directory":destination,"decoded_all":complete,"vmad_fields":index,"failures":failures.len()})
    );
    Ok(complete)
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
