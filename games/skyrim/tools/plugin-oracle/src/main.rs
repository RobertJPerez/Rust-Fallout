//! Offline structural comparison with the existing project's pinned esplugin oracle.
//! esplugin is GPL-3.0; this separate executable is not linked into skyrim-prep.
use esplugin::{GameId, ParseOptions, Plugin};
use serde_json::{Value, json};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: skyrim-plugin-oracle CENSUS_JSON")?,
    );
    let report: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let data = PathBuf::from(report["data"].as_str().ok_or("missing data path")?);
    let mut results = Vec::new();
    let mut passed = true;
    for item in report["plugins"].as_array().ok_or("missing plugins")? {
        let name = item["file"].as_str().ok_or("missing plugin name")?;
        if name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("unsafe plugin filename".into());
        }
        let mut parser = Plugin::new(GameId::SkyrimSE, &data.join(name));
        parser.parse_file(ParseOptions::whole_plugin())?;
        let records = parser.overlap_size(&[&parser])? as u64;
        let version = parser
            .header_version()
            .ok_or("missing header version")?
            .to_bits();
        let masters = parser.masters()?;
        let light = parser.is_light_plugin();
        let record_match = item["records"].as_u64() == Some(records + 1);
        let version_match = item["header_version_bits"].as_u64() == Some(u64::from(version));
        let masters_match = item["masters"] == json!(masters);
        let light_match = item["light"].as_bool() == Some(light);
        let matches = record_match && version_match && masters_match && light_match;
        passed &= matches;
        results.push(json!({"file": name, "records_excluding_header": records,
            "header_version_bits": version, "masters": masters, "light": light,
            "record_match": record_match, "version_match": version_match,
            "masters_match": masters_match, "light_match": light_match, "matches": matches}));
    }
    if results.is_empty() {
        return Err("no plugins to compare".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1, "oracle": "esplugin", "revision": "b33bd81aae02dd5cc797769705b093e407cd4a66",
            "passed": passed, "comparisons": results,
            "scope": "Plugin physical record counts, HEDR version, MAST list and light status only; not individual FormID values, compressed payload/VMAD semantics or gameplay"
        }))?
    );
    if !passed {
        std::process::exit(2);
    }
    Ok(())
}
