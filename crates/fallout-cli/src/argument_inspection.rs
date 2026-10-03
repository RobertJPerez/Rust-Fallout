//! Offline native argument inspection; raw comparison bytes stay in a guarded local
//! destination. Only source metadata, counts and hashes enter the JSON report.
use super::{Result, command_catalogue, data_files, protected_tree};
use fallout_data::{
    baseline,
    obscript::{
        argument_census::{self, CommandSignature, Signatures},
        arguments::{Convention, Parameter},
        expression::{Operator, Operators},
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

pub(super) fn inspect(install: &Path, focused: bool, bundle_path: Option<&Path>) -> Result<Value> {
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = Operators::new(
        catalogue
            .operators
            .iter()
            .map(|row| Operator {
                code: row.code,
                precedence: row.precedence,
                spelling: row.spelling.as_bytes().to_vec(),
            })
            .collect(),
    )?;
    let signatures: Signatures = catalogue
        .script_commands
        .iter()
        .map(|row| {
            let convention = match row.parse_convention {
                "vanilla-default" => Convention::Default,
                "vanilla-message" => Convention::Message,
                _ => Convention::Unknown,
            };
            (
                row.id as u16,
                CommandSignature {
                    convention,
                    parameters: row
                        .parameters
                        .iter()
                        .map(|param| Parameter {
                            type_id: param.type_id,
                            optional_word: param.optional_word,
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let mut total_calls = 0_u64;
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("native argument bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FROBS001")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut bundle_bytes = 8_u64;
    let mut reports = Vec::new();
    let mut commands = BTreeMap::<u16, u64>::new();
    for path in data_files(install, &["esm", "esp"])? {
        eprintln!("Inspecting native arguments in {}", path.display());
        let report = argument_census::inspect(
            &path,
            focused,
            &operators,
            &signatures,
            argument_census::Limits::default(),
            |bytes| {
                if let Some(bundle) = &mut bundle {
                    bundle_bytes += 4 + bytes.len() as u64;
                    if bundle_bytes > 66 * 1024 * 1024 {
                        return Err(fallout_data::Error::Unsupported(
                            "native argument bundle exceeds 66 MiB".into(),
                        ));
                    }
                    let fail =
                        |error: std::io::Error| fallout_data::Error::Resolution(error.to_string());
                    bundle
                        .write_all(&(bytes.len() as u32).to_le_bytes())
                        .map_err(fail)?;
                    bundle.write_all(bytes).map_err(fail)?;
                }
                Ok(())
            },
        )?;
        total_calls += report.counts.top_level_calls + report.counts.expression_calls;
        if total_calls > 1_000_000 {
            return Err("native argument run exceeds one million calls".into());
        }
        for (id, count) in &report.counts.command_calls {
            *commands.entry(*id).or_default() += count;
        }
        reports.push(report);
    }
    let mut command_links = Vec::new();
    let mut missing_commands = Vec::new();
    for (id, count) in commands {
        if let Some(descriptor) = catalogue
            .script_commands
            .iter()
            .find(|row| row.id == u32::from(id))
        {
            command_links
                .push(json!({"command_id":id,"occurrences":count,"descriptor":descriptor}));
        } else {
            missing_commands.push(id);
        }
    }
    let bundle_receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("opened bundle"))?;
        Some(json!({"format":"FROBS001","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    let issues: usize = reports
        .iter()
        .map(|report| report.argument_issues + report.expression_issues + report.framing_issues)
        .sum();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"vanilla native operands in instruction and expression calls; exact typed bits without reference resolution, conversion or execution",
        "executable_source_sha256":catalogue.source_sha256,"operator_descriptors":catalogue.operators,
        "plugins":reports,"issues":issues,"command_links":command_links,"unresolved_command_ids":missing_commands,
        "comparison_bundle":bundle_receipt,"execution_ready":false,"retail_parity_accepted":false}),
    )
}
