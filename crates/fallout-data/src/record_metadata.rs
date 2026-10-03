//! Reproducible metadata proof over original header indices. Deferred body
//! integrity and record-specific override semantics are outside this digest.
use crate::{
    Result,
    content::Definition,
    identity::{self, FormKey, ProfileId},
    store::RecordStore,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct Report {
    pub definitions: u64,
    pub winning_definitions: u64,
    pub all_definitions_sha256: String,
    pub winning_definitions_sha256: String,
}

fn name(hash: &mut Sha256, bytes: &[u8]) -> Result<()> {
    let length = u16::try_from(bytes.len())
        .map_err(|_| crate::Error::Unsupported("metadata name budget exceeded".into()))?;
    hash.update(length.to_le_bytes());
    hash.update(bytes);
    Ok(())
}
fn definition(
    hash: &mut Sha256,
    key: &FormKey,
    source: &str,
    definition: &Definition,
) -> Result<()> {
    name(hash, key.origin_plugin.as_bytes())?;
    hash.update(key.local_id.to_le_bytes());
    name(hash, source.as_bytes())?;
    let header = &definition.header;
    hash.update(header.kind);
    hash.update(header.offset.to_le_bytes());
    hash.update(header.stored_size.to_le_bytes());
    hash.update(header.flags.to_le_bytes());
    hash.update(header.form_id.to_le_bytes());
    hash.update(header.revision);
    hash.update(header.version.to_le_bytes());
    hash.update(header.trailing_bytes);
    for value in [
        definition.parent.topic,
        definition.parent.world,
        definition.parent.cell,
        definition.parent.child_group.map(|value| value as u32),
    ] {
        hash.update([u8::from(value.is_some())]);
        hash.update(value.unwrap_or(0).to_le_bytes());
    }
    Ok(())
}

pub fn inspect(store: &RecordStore) -> Result<Report> {
    let mut all = Sha256::new();
    let mut winners = Sha256::new();
    let mut definitions = 0;
    let mut winning_definitions = 0;
    for index in store.indices() {
        for row in &index.records {
            let key = identity::resolve_form(
                ProfileId::NvOriginal,
                &index.census.name,
                &index.census.masters,
                row.header.form_id,
            )?
            .ok_or_else(|| crate::Error::Resolution("null indexed definition".into()))?;
            definition(&mut all, &key, &index.census.name, row)?;
            definitions += 1;
        }
    }
    for (key, location) in store.winning_definitions() {
        definition(
            &mut winners,
            key,
            store.source_name(location),
            store.definition(location),
        )?;
        winning_definitions += 1;
    }
    Ok(Report {
        definitions,
        winning_definitions,
        all_definitions_sha256: format!("{:x}", all.finalize()),
        winning_definitions_sha256: format!("{:x}", winners.finalize()),
    })
}
