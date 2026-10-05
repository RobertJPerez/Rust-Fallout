// Offline comparison only. Native DirectXTex/zlib/LZ4 remain outside production.
use ba2::{
    fo4::{Archive, ArchiveKey, FileWriteOptions},
    prelude::*,
};
use dream_archive::ByteSlice;
use fallout4_prep::archive::{Ba2, Limits};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::File,
    io::{self, Write},
    path::PathBuf,
};

struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
}
// DDS_HEADER dwDepth is unused for non-volume textures. Both writers emit
// identical data except that one fills this unused field with 0 and the other
// with 1. This is a named, reported normalization, never applied to volume DDS.
// Evidence: docs/validation.md and Microsoft's DDS_HEADER contract.
fn equivalent_non_volume_depth(a: &[u8], b: &[u8]) -> bool {
    fn word(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
    }
    if a.len() < 128 || a.len() != b.len() || &a[..4] != b"DDS " || &b[..4] != b"DDS " {
        return false;
    }
    if word(a, 4) != 124
        || word(b, 4) != 124
        || word(a, 8) & 0x800000 != 0
        || word(b, 8) & 0x800000 != 0
        || word(a, 112) & 0x200000 != 0
        || word(b, 112) & 0x200000 != 0
    {
        return false;
    }
    if &a[84..88] == b"DX10" && (a.len() < 148 || word(a, 132) != 3) {
        return false;
    }
    word(a, 24) <= 1 && word(b, 24) <= 1 && a[..24] == b[..24] && a[28..] == b[28..]
}
impl Write for Bounded {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("oracle output budget"));
        }
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: fo4-ba2-oracle <archive> [--all]")?,
    );
    let all = std::env::args().any(|s| s == "--all");
    let primary = Ba2::open(&path, Limits::default())?;
    let file = File::open(&path)?;
    let (oracle, meta) = Archive::read(&file)?;
    let options: FileWriteOptions = meta.into();
    if oracle.len() != primary.entries().len() {
        return Err("BA2 entry count disagreement".into());
    }
    let mut compared = 0;
    let mut normalized_depth_fields = 0;
    let mut hash_findings = Vec::new();
    let by_hash: BTreeMap<_, _> = oracle
        .iter()
        .map(|(k, v)| (*k.hash(), (k.name().as_bytes(), v)))
        .collect();
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    for (i, entry) in primary.entries().iter().enumerate() {
        let name = entry.name().as_bytes();
        if name.is_empty() {
            return Err("nameless member needs separate hash-only comparison".into());
        }
        let key = ArchiveKey::from(name);
        let stored = entry.hash();
        let stored_key: ba2::fo4::FileHash = ba2::fo4::Hash {
            file: stored.file,
            extension: stored.extension,
            directory: stored.directory,
        }
        .into();
        let (other_name, other) = by_hash
            .get(&stored_key)
            .ok_or_else(|| format!("reference missing stored hash at entry {i}"))?;
        if *other_name != name {
            return Err(format!("reference filename bytes differ at entry {i}").into());
        }
        let hash_disagrees = *key.hash() != stored_key;
        if hash_disagrees {
            hash_findings.push(serde_json::json!({"entry":i,"path_bytes":name,"stored_hash":[stored.file,stored.extension,stored.directory],"recomputed_hash":[key.hash().file,key.hash().extension,key.hash().directory],"status":"unresolved path-hash/encoding discrepancy; physical member compared by stored hash and exact name"}));
        }
        if all || i < 4 || i % 997 == 0 || hash_disagrees {
            let actual = primary.read(i)?;
            let mut expected = Bounded {
                bytes: Vec::new(),
                limit: Limits::default().member_bytes as usize,
            };
            other.write(&mut expected, &options)?;
            if actual != expected.bytes {
                if primary.texture(i).is_some()
                    && equivalent_non_volume_depth(&actual, &expected.bytes)
                {
                    normalized_depth_fields += 1;
                } else {
                    eprintln!(
                        "{}",
                        serde_json::json!({"member":i,"primary_length":actual.len(),"oracle_length":expected.bytes.len(),"primary_prefix":&actual[..actual.len().min(148)],"oracle_prefix":&expected.bytes[..expected.bytes.len().min(148)]})
                    );
                    return Err(format!("BA2 extracted bytes differ: entry {i}").into());
                }
            }
            compared += 1;
            bytes += actual.len() as u64;
            digest.update((i as u64).to_le_bytes());
            digest.update(Sha256::digest(&actual));
        }
    }
    println!(
        "{}",
        serde_json::json!({"status":"matched","archive":path.file_name().unwrap_or_default().to_string_lossy(),"entries_matched_by_stored_hash_and_name":oracle.len(),"path_hash_findings":hash_findings,"payloads_compared":compared,"decoded_bytes_compared":bytes,"all_payloads":all,"non_volume_dds_unused_depth_normalizations":normalized_depth_fields,"comparison_sha256":format!("{:x}",digest.finalize())})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dds_normalization_cannot_hide_volume_metadata_or_payload_changes() {
        let mut a = vec![0; 132];
        a[..4].copy_from_slice(b"DDS ");
        a[4..8].copy_from_slice(&124u32.to_le_bytes());
        let mut b = a.clone();
        b[24] = 1;
        assert!(equivalent_non_volume_depth(&a, &b));
        for at in [12usize, 16, 28, 84, 128] {
            let mut changed = b.clone();
            changed[at] ^= 1;
            assert!(!equivalent_non_volume_depth(&a, &changed));
        }
        a[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
        b[8..12].copy_from_slice(&0x800000u32.to_le_bytes());
        assert!(!equivalent_non_volume_depth(&a, &b));
    }
}
