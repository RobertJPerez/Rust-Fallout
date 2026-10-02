// Offline comparison only. The runtime has one archive backend.
use ba2::{
    prelude::*,
    tes4::{Archive, ArchiveKey, DirectoryKey, FileCompressionOptions},
};
use fallout_data::{
    archive::{MAX_ASSET_BYTES, NvArchive},
    baseline::open_source,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: archive-compare FILE.bsa [--all]")?,
    );
    let all = std::env::args().any(|arg| arg == "--all");
    let primary = NvArchive::open(&path)?;
    let guard = open_source(&path)?;
    let (oracle, meta) = Archive::read(&guard)?;
    let options: FileCompressionOptions = meta.into();
    let oracle_count: usize = oracle.values().map(|dir| dir.len()).sum();
    if oracle_count != primary.backend().len() {
        return Err("archive entry count mismatch".into());
    }
    let mut compared = 0u64;
    let mut compared_bytes = 0u64;
    let mut manifest = Sha256::new();
    let mut failures = Vec::new();
    for (id, entry) in primary.backend().entries_with_ids() {
        let folder: &[u8] = entry.folder().ok_or("missing folder")?.as_ref();
        let name: &[u8] = entry.name().ok_or("missing filename")?.as_ref();
        let dk = ArchiveKey::from(folder);
        let fk = DirectoryKey::from(name);
        let file = oracle
            .get(&dk)
            .and_then(|dir| dir.get(&fk))
            .ok_or("oracle is missing a primary entry")?;
        if ba2::tes4::hash_directory(ba2::BStr::new(folder))
            .0
            .numeric()
            != entry.folder_hash().numeric()
            || ba2::tes4::hash_file(ba2::BStr::new(name)).0.numeric() != entry.file_hash().numeric()
        {
            return Err(format!("path hash mismatch at entry {}", id.index()).into());
        }
        if all || id.index() < 4 || id.index() % 997 == 0 {
            if file.decompressed_len().unwrap_or(file.len()) as u64 > MAX_ASSET_BYTES {
                return Err("oracle asset allocation budget exceeded".into());
            }
            let actual = primary.read(id);
            let mut expected = Vec::new();
            let oracle_result = file.write(&mut expected, &options);
            if actual.is_err() || oracle_result.is_err() {
                failures.push(json!({"entry":id.index(),"path_bytes":entry.path().map(|p|{let raw:&[u8]=p.as_ref();raw.to_vec()}),
                    "offset":entry.file().data_offset,"stored_size":entry.file().stored_size,
                    "primary_error":actual.err().map(|e|e.to_string()),"oracle_error":oracle_result.err().map(|e|e.to_string()),
                    "oracle_decoded_bytes":expected.len(),"oracle_output_sha256":format!("{:x}",Sha256::digest(&expected))}));
                if failures.len() > 100 {
                    return Err("too many decode failures; comparison budget exhausted".into());
                }
                continue;
            }
            let actual = actual?;
            if actual != expected {
                return Err(format!("payload mismatch at entry {}", id.index()).into());
            }
            manifest.update((id.index() as u64).to_le_bytes());
            manifest.update(Sha256::digest(&actual));
            compared += 1;
            compared_bytes += actual.len() as u64;
        }
    }
    println!(
        "{}",
        json!({"archive":path,"entries":oracle_count,"paths_and_hashes_compared":oracle_count,
        "payloads_compared":compared,"decoded_bytes_compared":compared_bytes,"all_payloads":all,
        "comparison_digest":format!("{:x}",manifest.finalize()),"failures":failures,
        "result":if failures.is_empty(){"equal"}else{"decode-failures"}})
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err("archive comparison contains decode failures".into())
    }
}
