//! Read only explicitly requested members with ba2, independently of the runtime
//! archive backend. Output contains hashes and sizes, never extracted payloads.
use ba2::{
    prelude::*,
    tes4::{Archive, ArchiveKey, DirectoryKey, FileCompressionOptions},
};
use fallout_data::baseline;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Request {
    archive: PathBuf,
    path_bytes: Vec<u8>,
}

fn keys(raw: &[u8]) -> Result<(&[u8], &[u8])> {
    if raw.len() > 4096 || raw.is_empty() || raw.iter().any(|v| *v < 32 || *v == b':') {
        return Err("unsafe oracle member path".into());
    }
    for part in raw.split(|v| matches!(v, b'/' | b'\\')) {
        if part.is_empty() || part == b"." || part == b".." {
            return Err("invalid oracle path component".into());
        }
    }
    let at = raw
        .iter()
        .rposition(|v| matches!(v, b'/' | b'\\'))
        .ok_or("member path needs a directory")?;
    Ok((&raw[..at], &raw[at + 1..]))
}

fn main() -> Result<()> {
    let input = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: archive-member-oracle REQUESTS.json")?,
    );
    let mut raw = Vec::new();
    baseline::open_source(&input)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 1024 * 1024 {
        return Err("oracle request byte budget exceeded".into());
    }
    let requests: Vec<Request> = serde_json::from_slice(&raw)?;
    if requests.is_empty() || requests.len() > 256 {
        return Err("oracle request count budget exceeded".into());
    }
    let mut grouped: BTreeMap<PathBuf, BTreeSet<Vec<u8>>> = BTreeMap::new();
    for request in requests {
        keys(&request.path_bytes)?;
        if !grouped
            .entry(request.archive)
            .or_default()
            .insert(request.path_bytes)
        {
            return Err("duplicate oracle request".into());
        }
    }
    let mut files = Vec::new();
    let mut decoded = 0usize;
    for (path, members) in grouped {
        let mut guard = baseline::open_source(&path)?;
        let (_, archive_sha) = baseline::digest_reader(&mut guard)?;
        guard.seek(SeekFrom::Start(0))?;
        let (archive, meta) = Archive::read(&guard)?;
        let options: FileCompressionOptions = meta.into();
        for member in members {
            let (folder, name) = keys(&member)?;
            let file = archive
                .get(&ArchiveKey::from(folder))
                .and_then(|dir| dir.get(&DirectoryKey::from(name)))
                .ok_or("ba2 member missing")?;
            let size = file.decompressed_len().unwrap_or(file.len());
            if size > (256 * 1024 * 1024usize).saturating_sub(decoded) {
                return Err("oracle decoded-byte budget exceeded".into());
            }
            let mut bytes = Vec::new();
            file.write(&mut bytes, &options)?;
            if bytes.len() != size {
                return Err("oracle decoded extent differs".into());
            }
            decoded += bytes.len();
            files.push(
                json!({"archive":path,"archive_sha256":archive_sha,"path_bytes":member,
                "decoded_bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}),
            );
        }
    }
    println!(
        "{}",
        json!({"oracle_binary_sha256":baseline::digest_file(&std::env::current_exe()?)?.1,
        "files":files,"decoded_bytes":decoded,"scope":"Selected archive-member bytes read by ba2; plugin resolution, mount precedence, image pixels and shader behavior are not compared"})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn member_keys_preserve_legacy_bytes_and_refuse_path_escape() {
        assert_eq!(
            keys(b"Textures\\Land\\\xe9.DDS").unwrap(),
            (b"Textures\\Land".as_slice(), b"\xe9.DDS".as_slice())
        );
        for raw in [
            b"/a.dds".as_slice(),
            b"../a.dds",
            b"x//a.dds",
            b"x/../a.dds",
            b"C:\\a.dds",
            b"x/a\0.dds",
        ] {
            assert!(keys(raw).is_err());
        }
    }
}
