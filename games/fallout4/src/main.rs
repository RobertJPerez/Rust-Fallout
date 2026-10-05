use dream_archive::ByteSlice;
use fallout4_prep::{Error, Result, archive::Ba2, attachments, census, materials, pex, profile};
use std::{
    env, fs,
    io::{self, Write},
    path::Path,
};

fn json(value: &impl serde::Serialize) -> Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn run() -> Result<bool> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.first().and_then(|a| a.to_str()) {
        Some("census") if args.len() == 2 || (args.len() == 3 && args[2] == "--full-hash") => {
            let out = census::installation(Path::new(&args[1]), args.len() == 3)?;
            json(&out)?;
            Ok(out.complete())
        }
        Some("plugin") if args.len() == 2 => {
            json(&census::plugin_census(Path::new(&args[1]))?)?;
            Ok(true)
        }
        Some("vmad") if args.len() == 2 => {
            let mut counts = attachments::Counts::default();
            let mut failures = Vec::new();
            attachments::visit_plugin(Path::new(&args[1]), |origin, _, parsed| {
                match parsed {
                    Ok(adapter) => counts.add(&adapter),
                    Err(error) => failures
                        .push(serde_json::json!({"origin":origin,"error":error.to_string()})),
                }
                Ok(())
            })?;
            json(&serde_json::json!({"counts":counts,"failures":failures}))?;
            Ok(failures.is_empty())
        }
        Some("archive") if args.len() == 2 => {
            let out = census::archive_census(Path::new(&args[1]))?;
            json(&out)?;
            Ok(out.pex_failures.is_empty())
        }
        Some("pex") if args.len() == 2 => {
            let path = Path::new(&args[1]);
            if fs::metadata(path)?.len() > pex::Limits::default().file_bytes as u64 {
                return Err(Error::Unsupported("PEX file byte budget".into()));
            }
            let bytes = fs::read(path)?;
            let name = path.display().to_string();
            json(&pex::parse(&bytes, &name, Default::default())?)?;
            Ok(true)
        }
        Some("profile") if args.len() == 3 => {
            let report = profile::observe(Path::new(&args[1]), Path::new(&args[2]))?;
            let complete = report.observation_complete;
            json(&report)?;
            Ok(complete)
        }
        Some("extract-materials") if args.len() == 3 => {
            let (manifest, completion) =
                materials::extract(Path::new(&args[1]), Path::new(&args[2]))?;
            json(&completion)?;
            Ok(manifest.bgsm + manifest.bgem > 0)
        }
        Some("extract-pex") if args.len() == 3 => {
            let source = fs::canonicalize(&args[1])?;
            // Output is a new directory. Never overwrite sources or prior evidence.
            let requested = Path::new(&args[2]);
            let parent = fs::canonicalize(
                requested
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?;
            let container = source.parent().unwrap();
            let protected = if container
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Data"))
            {
                container.parent().unwrap_or(container)
            } else {
                container
            };
            if parent.starts_with(protected) {
                return Err(Error::Unsupported(
                    "output must be outside the source installation".into(),
                ));
            }
            let name = requested
                .file_name()
                .ok_or_else(|| Error::Unsupported("output directory name required".into()))?;
            let destination = parent.join(name);
            fs::create_dir(&destination)?;
            let archive = Ba2::open(&source, Default::default())?;
            let mut manifest = Vec::new();
            for (i, e) in archive.entries().iter().enumerate() {
                if e.name().as_bytes().to_ascii_lowercase().ends_with(b".pex") {
                    let bytes = archive.read(i)?;
                    let filename = format!("{i:08}.pex");
                    fs::write(destination.join(&filename), &bytes)?;
                    manifest.push((
                        filename,
                        census::text(e.name().as_bytes()),
                        census::sha256(&bytes),
                    ));
                }
            }
            // A failed extraction leaves an incomplete directory without this marker.
            fs::write(
                destination.join("manifest.json"),
                serde_json::to_vec_pretty(&manifest)?,
            )?;
            json(&serde_json::json!({"extracted":manifest.len(),"directory":destination}))?;
            Ok(true)
        }
        Some("--help") | None => {
            println!(
                "Fallout 4 preparation (inspection only)\n  census <install-root> [--full-hash]\n  plugin <esm/esp/esl>\n  vmad <esm/esp/esl>\n  archive <ba2>\n  pex <pex>\n  profile <install-root> <explicit-profile-directory>\n  extract-pex <ba2> <new-local-directory>\n  extract-materials <install-root> <new-local-directory>\nJSON on stdout, progress on stderr. Exit 2 means incomplete observation; 1 means command error."
            );
            Ok(true)
        }
        _ => Err(Error::Unsupported("invalid command; use --help".into())),
    }
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
