//! Shared bounded inputs for inspections that need an explicit plugin order.
use super::{Result, protected_tree};
use fallout_data::{baseline, plugin, store::RecordStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

pub(super) struct Order {
    pub names: Vec<String>,
    pub sha256: String,
    _source: File,
}
impl Order {
    pub fn read(path: &Path) -> Result<Self> {
        let mut source = baseline::open_source(path)?;
        let mut bytes = Vec::new();
        (&mut source).take(65_537).read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err("inspection load order exceeds 64 KiB".into());
        }
        let names: Vec<String> = serde_json::from_slice(&bytes)?;
        if names.is_empty() || names.len() > 254 {
            return Err("inspection load order requires 1..=254 plugins".into());
        }
        Ok(Self {
            names,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            _source: source,
        })
    }
    pub fn store(&self, install: &Path, cache: Option<&Path>) -> Result<RecordStore> {
        Ok(if let Some(root) = cache {
            RecordStore::open_nv_headers_cached(
                &install.join("Data"),
                &self.names,
                plugin::Limits::default(),
                root,
            )?
        } else {
            RecordStore::open_nv_headers(
                &install.join("Data"),
                &self.names,
                plugin::Limits::default(),
            )?
        })
    }
}

pub(super) struct RecordBundle {
    file: BufWriter<File>,
    path: PathBuf,
    format: String,
    bytes: u64,
}
impl RecordBundle {
    pub fn create(path: &Path, install: &Path, magic: &[u8; 8]) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(protected_tree(install)?) {
            return Err("inspection bundle must be outside the installation".into());
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(magic)?;
        Ok(Self {
            file: BufWriter::new(file),
            path: path.into(),
            format: std::str::from_utf8(magic)?.into(),
            bytes: 8,
        })
    }
    pub fn write(
        &mut self,
        source_index: usize,
        record: &plugin::Record,
    ) -> fallout_data::Result<()> {
        if source_index >= 254 {
            return Err(fallout_data::Error::Unsupported(
                "inspection source index exceeds profile budget".into(),
            ));
        }
        self.bytes += 25 + record.payload.len() as u64;
        if self.bytes > 256 * 1024 * 1024 {
            return Err(fallout_data::Error::Unsupported(
                "inspection bundle exceeds 256 MiB".into(),
            ));
        }
        let write = |error: std::io::Error| fallout_data::Error::Resolution(error.to_string());
        self.file.write_all(&[source_index as u8]).map_err(write)?;
        self.file.write_all(&record.header.kind).map_err(write)?;
        self.file
            .write_all(&record.header.form_id.to_le_bytes())
            .map_err(write)?;
        self.file
            .write_all(&record.header.flags.to_le_bytes())
            .map_err(write)?;
        self.file
            .write_all(&record.header.offset.to_le_bytes())
            .map_err(write)?;
        self.file
            .write_all(&(record.payload.len() as u32).to_le_bytes())
            .map_err(write)?;
        self.file.write_all(&record.payload).map_err(write)?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<Value> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        drop(self.file);
        let (bytes, sha256) = baseline::digest_file(&self.path)?;
        Ok(json!({"format":self.format,"bytes":bytes,"sha256":sha256}))
    }
}
