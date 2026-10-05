//! Re-decode every extracted payload and compare every semantic token to Mutagen.
use fallout4_prep::{Error, Result, attachments, census, vmad};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader},
    path::Path,
};

fn check(expected: &Value, actual: &Value, raw: &[u8]) -> Result<usize> {
    let file = expected["file"]
        .as_str()
        .ok_or_else(|| Error::Unsupported("missing file".into()))?;
    for label in ["file", "sha256"] {
        if expected[label] != actual[label] {
            return Err(Error::Unsupported(format!("{file}: {label} mismatch")));
        }
    }
    if !expected["error"].is_null() || !actual["error"].is_null() {
        return Err(Error::Unsupported(format!(
            "{file}: decoder failure: {} {}",
            expected["error"], actual["error"]
        )));
    }
    if expected["sha256"] != census::sha256(raw) || expected["bytes"] != raw.len() {
        return Err(Error::Unsupported(format!(
            "{file}: payload fingerprint mismatch"
        )));
    }
    let kind: [u8; 4] = expected["origin"]["record_kind"]
        .as_str()
        .unwrap_or("")
        .as_bytes()
        .try_into()
        .map_err(|_| Error::Unsupported("invalid record kind".into()))?;
    let decoded = vmad::parse(raw, kind, file, Default::default())?;
    let tokens = attachments::tokens(&decoded);
    if expected["tokens"] != json!(tokens) {
        return Err(Error::Unsupported(format!(
            "{file}: saved Rust tokens differ from re-decoding"
        )));
    }
    if actual["tokens"] != expected["tokens"] {
        let other = actual["tokens"].as_array();
        let index = tokens
            .iter()
            .enumerate()
            .find(|(i, t)| other.and_then(|v| v.get(*i)) != Some(*t))
            .map(|(i, _)| i)
            .unwrap_or(tokens.len());
        return Err(Error::Unsupported(format!(
            "{file}: independent semantic token mismatch at {index}: Rust {:?}, reference {:?}",
            tokens.get(index),
            other.and_then(|v| v.get(index))
        )));
    }
    Ok(tokens.len())
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err(Error::Unsupported(
            "verify-vmad <scan-directory> <reference.jsonl> [new-result.json]".into(),
        ));
    }
    let dir = Path::new(&args[0]);
    let scan: Value = serde_json::from_reader(fs::File::open(dir.join("scan.json"))?)?;
    if scan["decoded_all"] != true {
        return Err(Error::Unsupported("incomplete source scan".into()));
    }
    let expected = BufReader::new(fs::File::open(dir.join("attachments.jsonl"))?).lines();
    let mut actual = BufReader::new(fs::File::open(&args[1])?).lines();
    let mut fields = 0u64;
    let mut tokens = 0usize;
    let mut seen = BTreeSet::new();
    let mut kinds = BTreeMap::<String, u64>::new();
    let mut normalizations = Vec::new();
    for line in expected {
        let expected: Value = serde_json::from_str(&line?)?;
        let actual: Value = serde_json::from_str(
            &actual
                .next()
                .ok_or_else(|| Error::Unsupported("reference report ended early".into()))??,
        )?;
        let file = expected["file"]
            .as_str()
            .ok_or_else(|| Error::Unsupported("missing filename".into()))?;
        if Path::new(file).file_name().and_then(|p| p.to_str()) != Some(file)
            || !seen.insert(file.to_owned())
        {
            return Err(Error::Unsupported(
                "unsafe or duplicate evidence filename".into(),
            ));
        }
        let path = dir.join(file);
        if fs::metadata(&path)?.len() > vmad::Limits::default().bytes as u64 {
            return Err(Error::Unsupported("VMAD byte limit".into()));
        }
        tokens += check(&expected, &actual, &fs::read(path)?)?;
        if actual["reference_normalizations"]
            .as_object()
            .is_some_and(|o| !o.is_empty())
        {
            normalizations.push(json!({"file":file,"origin":expected["origin"],"raw_to_reference_canonical":actual["reference_normalizations"]}));
        }
        fields += 1;
        *kinds
            .entry(expected["origin"]["record_kind"].as_str().unwrap().into())
            .or_default() += 1;
    }
    if actual.next().is_some() || scan["vmad_fields"] != fields || fields == 0 {
        return Err(Error::Unsupported("evidence cardinality mismatch".into()));
    }
    let inputs = scan["inputs"]
        .as_array()
        .ok_or_else(|| Error::Unsupported("missing scan input provenance".into()))?;
    for input in inputs {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| Error::Unsupported("missing input path".into()))?;
        if input["sha256"] != census::hash_file(Path::new(path))? {
            return Err(Error::Unsupported(format!("plugin source changed: {path}")));
        }
    }
    let result = json!({"verified":true,"attachments":fields,"semantic_tokens":tokens,"record_kinds":kinds,"plugin_fingerprints_rechecked":inputs.len(),"reference_id_normalizations":normalizations,"scope":"all decoded scripts, properties, nested members, raw form IDs, alias formats, fragments and phases; exact scalar bits and original string bytes","manifest_sha256":census::hash_file(&dir.join("attachments.jsonl"))?,"reference_sha256":census::hash_file(Path::new(&args[1]))?,"scan_sha256":census::hash_file(&dir.join("scan.json"))?});
    if let Some(path) = args.get(2) {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        serde_json::to_writer_pretty(file, &result)?;
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
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
    fn comparison_rejects_tampered_payload_values_and_reference_errors() {
        let raw = [6, 0, 2, 0, 0, 0];
        let tokens = attachments::tokens(
            &vmad::parse(&raw, *b"ACTI", "fixture", Default::default()).unwrap(),
        );
        let expected = json!({"file":"0.vmad","sha256":census::sha256(&raw),"bytes":raw.len(),"error":null,"origin":{"record_kind":"ACTI"},"tokens":tokens});
        assert!(check(&expected, &expected, &raw).is_ok());
        let mut bad = expected.clone();
        bad["tokens"][1] = json!(1);
        assert!(check(&expected, &bad, &raw).is_err());
        let mut bad = expected.clone();
        bad["error"] = json!("upstream failed");
        assert!(check(&expected, &bad, &raw).is_err());
        assert!(check(&expected, &expected, &[5, 0, 2, 0, 0, 0]).is_err());
        let mut bad = expected.clone();
        bad["tokens"] = json!([]);
        assert!(check(&bad, &bad, &raw).is_err());
    }
}
