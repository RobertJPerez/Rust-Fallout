// SPDX-License-Identifier: GPL-3.0-or-later
// Independent esplugin driver. No parser code is copied into the runtime.
use esplugin::{GameId, ParseOptions, Plugin};
use serde_json::json;
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: plugin-oracle DATA_DIRECTORY")?,
    );
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("esm") || v.eq_ignore_ascii_case("esp"))
        {
            files.push(entry.path());
        }
    }
    files.sort();
    let mut reports = Vec::new();
    for path in files {
        let mut plugin = Plugin::new(GameId::FalloutNV, &path);
        plugin.parse_file(ParseOptions::whole_plugin())?;
        reports.push(json!({"name":plugin.filename(),"parsed_record_ids":plugin.overlap_size(&[&plugin])?,
            "declared_records_and_groups":plugin.record_and_group_count(),"masters":plugin.masters()?,
            "header_version":plugin.header_version(),"override_records":plugin.count_override_records()?,
            "compressed_payloads_validated":false}));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
