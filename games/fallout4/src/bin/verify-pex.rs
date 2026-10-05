//! Offline comparison of independently decoded structures and native signatures.
use fallout4_prep::{Error, Result, census, pex, pex_evidence};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};

fn compare(rust: &Value, reference: &Value) -> Result<()> {
    for key in [
        "strings",
        "objects",
        "structs",
        "variables",
        "properties",
        "states",
        "functions",
        "instructions",
        "opcode_counts",
        "native_declarations",
    ] {
        let mut actual = rust
            .get(key)
            .ok_or_else(|| Error::Unsupported(format!("missing Rust field {key}")))?
            .clone();
        if key == "native_declarations" {
            for n in actual
                .as_array_mut()
                .ok_or_else(|| Error::Unsupported("native list missing".into()))?
            {
                n.as_object_mut()
                    .ok_or_else(|| Error::Unsupported("native row missing".into()))?
                    .remove("byte_offset");
            }
        }
        if reference.get(key) != Some(&actual) {
            return Err(Error::Unsupported(format!(
                "independent PEX mismatch in {key}"
            )));
        }
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 && !(args.len() == 4 && args[3] == "--values") {
        return Err(Error::Unsupported(
            "usage: verify-pex <archive-report.json> <extracted-dir> <reference.jsonl> [--values]"
                .into(),
        ));
    }
    let report: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    if report["pex_failures"]
        .as_array()
        .is_none_or(|a| !a.is_empty())
    {
        return Err(Error::Unsupported("Rust PEX census incomplete".into()));
    }
    let extracted = Path::new(&args[1]);
    let manifest: Vec<(String, String, String)> =
        serde_json::from_slice(&fs::read(extracted.join("manifest.json"))?)?;
    let mut refs = BTreeMap::new();
    for line in fs::read_to_string(&args[2])?.lines() {
        let row: Value = serde_json::from_str(line)?;
        let name = row["file"]
            .as_str()
            .ok_or_else(|| Error::Unsupported("reference file missing".into()))?
            .to_owned();
        if refs.insert(name, row).is_some() {
            return Err(Error::Unsupported("duplicate reference file".into()));
        }
    }
    let rows = report["pex"]
        .as_array()
        .ok_or_else(|| Error::Unsupported("missing census rows".into()))?;
    if rows.len() != manifest.len() || rows.len() != refs.len() {
        return Err(Error::Unsupported("PEX denominators disagree".into()));
    }
    let mut by_path = BTreeMap::new();
    for row in rows {
        let path = row["path"]
            .as_str()
            .ok_or_else(|| Error::Unsupported("missing census path".into()))?;
        if by_path.insert(path, row).is_some() {
            return Err(Error::Unsupported(format!("ambiguous census path {path}")));
        }
    }
    let mut used = BTreeSet::new();
    let mut instructions = 0u64;
    let mut natives = 0usize;
    let mut semantic_tokens = 0usize;
    for (file, name, hash) in &manifest {
        if !used.insert(file) {
            return Err(Error::Unsupported("duplicate manifest file".into()));
        }
        if Path::new(file).components().count() != 1 || !file.ends_with(".pex") {
            return Err(Error::Unsupported("invalid manifest file path".into()));
        }
        if census::hash_file(&extracted.join(file))? != *hash {
            return Err(Error::Unsupported(format!(
                "extracted file hash changed: {file}"
            )));
        }
        let row = by_path
            .get(name.as_str())
            .ok_or_else(|| Error::Unsupported(format!("missing census path {name}")))?;
        if row["sha256"].as_str() != Some(hash.as_str()) {
            return Err(Error::Unsupported(format!("census hash mismatch: {name}")));
        }
        let reference = refs
            .get(file)
            .ok_or_else(|| Error::Unsupported(format!("missing reference {file}")))?;
        compare(row, reference)?;
        if args.len() == 4 {
            let bytes = fs::read(extracted.join(file))?;
            let decoded = pex::parse(&bytes, name, Default::default())?;
            let tokens = pex_evidence::tokens(&decoded);
            if reference["value_tokens"] != serde_json::json!(tokens) {
                let other = reference["value_tokens"].as_array();
                let index = tokens
                    .iter()
                    .enumerate()
                    .find(|(i, t)| other.and_then(|o| o.get(*i)) != Some(*t))
                    .map(|(i, _)| i)
                    .unwrap_or(tokens.len());
                return Err(Error::Unsupported(format!(
                    "{name}: definition/operand mismatch at token {index}: Rust {:?}, reference {:?}",
                    tokens.get(index),
                    other.and_then(|o| o.get(index))
                )));
            }
            semantic_tokens += tokens.len();
        }
        instructions += row["instructions"]
            .as_u64()
            .ok_or_else(|| Error::Unsupported("invalid instruction count".into()))?;
        natives += row["native_declarations"]
            .as_array()
            .ok_or_else(|| Error::Unsupported("invalid native declarations".into()))?
            .len();
    }
    println!(
        "{}",
        serde_json::json!({"status":"matched","files":manifest.len(),"instructions_counted":instructions,"native_declarations_compared":natives,"semantic_tokens_compared":semantic_tokens,"definitions_and_operands_compared":args.len()==4,"scope":if args.len()==4 { "per-file structure, all class/member/property/state/function definitions, string table, initial values and instruction operands; excludes debug metadata/header and execution" } else { "per-file structural counts, opcode histograms, native class/state/function signatures; no execution or operand-value oracle" }})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_tampered_or_missing_reference_evidence() {
        let good = serde_json::json!({"strings":1,"objects":1,"structs":0,"variables":0,"properties":0,"states":1,"functions":1,"instructions":1,"opcode_counts":{"26":1},"native_declarations":[]});
        assert!(compare(&good, &good).is_ok());
        for key in [
            "strings",
            "objects",
            "structs",
            "variables",
            "properties",
            "states",
            "functions",
            "instructions",
            "opcode_counts",
            "native_declarations",
        ] {
            let mut bad = good.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(compare(&good, &bad).is_err());
        }
        let mut bad = good.clone();
        bad["opcode_counts"] = serde_json::json!({"25":1});
        assert!(compare(&good, &bad).is_err());
    }
}
