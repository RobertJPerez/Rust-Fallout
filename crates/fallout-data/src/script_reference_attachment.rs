//! A sealed, fresh REFR -> CONT -> standalone SCPT source chain. This proves
//! authored attachment only, never retail activation or a current event list.
use crate::{
    identity::{FormKey, ProfileId, plugin_name},
    loaded_scripts::{Catalogue, Handle, LoadedScript, OwnerKind, Version},
    plugin::{self, Record, RecordHeader},
    record_metadata,
    store::{Location, RecordStore, SourceReceipt},
    world,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_sources: usize,
    pub maximum_source_bytes: u64,
    /// All indexed definitions plus winners visited by the existing digest.
    pub maximum_header_visits: usize,
    pub maximum_catalogue_scripts: usize,
    pub maximum_variable_bytes: usize,
    pub maximum_record_bytes: usize,
    pub maximum_read_bytes: usize,
    /// Includes both preflight and the existing placement decoder pass.
    pub maximum_field_visits: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_sources: 256,
            maximum_source_bytes: 16 * 1024 * 1024 * 1024,
            maximum_header_visits: 8_000_000,
            maximum_catalogue_scripts: 262_144,
            maximum_variable_bytes: 1024 * 1024,
            maximum_record_bytes: 8 * 1024 * 1024,
            maximum_read_bytes: 24 * 1024 * 1024,
            maximum_field_visits: 65_536,
        }
    }
}
#[derive(Debug, Default, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub sources: usize,
    pub source_bytes: u64,
    pub header_visits: usize,
    pub catalogue_scripts: usize,
    pub variable_bytes: usize,
    pub records: usize,
    pub read_bytes: usize,
    pub field_visits: usize,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("placed script source budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("placed script source chain is unavailable: {0}")]
    Unavailable(&'static str),
    #[error(transparent)]
    Data(#[from] crate::Error),
}
#[derive(Debug, Serialize)]
pub struct RecordProof {
    pub key: FormKey,
    pub header: RecordHeader,
    pub source_plugin: String,
    pub source_sha256: String,
    pub decoded_record_sha256: String,
}
#[derive(Debug, Serialize)]
pub struct LinkProof {
    pub decoded_offset: usize,
    pub raw_form: u32,
    pub key: FormKey,
}
#[derive(Debug, Serialize)]
pub struct Proof {
    pub placement: RecordProof,
    pub name: LinkProof,
    pub base: RecordProof,
    pub scri: LinkProof,
    pub script: RecordProof,
}
/// No public constructor or deserializer. Only the bounded fresh source reader
/// can establish this authority; public proof fields are borrowed diagnostics.
pub struct Request<'a> {
    catalogue: &'a Catalogue,
    script: &'a LoadedScript,
    receipts: Vec<SourceReceipt>,
    proof: Proof,
    counts: Counts,
}
impl Request<'_> {
    pub fn catalogue(&self) -> &Catalogue {
        self.catalogue
    }
    pub fn source_receipts(&self) -> &[SourceReceipt] {
        &self.receipts
    }
    pub fn proof(&self) -> &Proof {
        &self.proof
    }
    pub fn definition(&self) -> &Handle {
        self.script.handle()
    }
    pub fn script_version(&self) -> &Version {
        self.script.version()
    }
    pub fn counts(&self) -> Counts {
        self.counts
    }
}
fn charge(used: &mut usize, amount: usize, limit: usize, kind: &'static str) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&n| n <= limit)
        .ok_or(Error::Capacity(kind))?;
    Ok(())
}
fn variable(counts: &mut Counts, size: usize, limits: Limits) -> Result<(), Error> {
    charge(
        &mut counts.variable_bytes,
        size,
        limits.maximum_variable_bytes,
        "variable bytes",
    )
}
fn location(store: &RecordStore, key: &FormKey, kind: [u8; 4]) -> Result<Location, Error> {
    let location = store
        .winner(key)
        .ok_or(Error::Unavailable("missing winning record"))?;
    let header = &store.definition(location).header;
    if header.flags & plugin::DELETED != 0 {
        return Err(Error::Unavailable("deleted winning record"));
    }
    if header.kind != kind {
        return Err(Error::Unavailable("unsupported winning record kind"));
    }
    Ok(location)
}
fn read(
    store: &mut RecordStore,
    at: Location,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Record, Error> {
    let remaining = limits
        .maximum_read_bytes
        .checked_sub(counts.read_bytes)
        .ok_or(Error::Capacity("read bytes"))?;
    let record = store.read_bounded(at, remaining.min(limits.maximum_record_bytes))?;
    let bytes = record.payload.len().max(record.header.stored_size as usize);
    charge(
        &mut counts.read_bytes,
        bytes,
        limits.maximum_read_bytes,
        "read bytes",
    )?;
    counts.records += 1;
    if record.integrity_issue.is_some() {
        return Err(Error::Unavailable("untrusted decoded record"));
    }
    Ok(record)
}
fn fields(
    record: &Record,
    name: &str,
    counts: &mut Counts,
    limits: Limits,
    passes: usize,
) -> Result<(), Error> {
    // The existing visitor remains the sole framing parser. Stop on the first
    // excess field before the placement decoder allocates unknown-field names.
    let mut excess = None;
    let result = plugin::visit_subrecords(record, name, |_| {
        if charge(
            &mut counts.field_visits,
            passes,
            limits.maximum_field_visits,
            "field visits",
        )
        .is_err()
        {
            excess = Some("field visits");
            return Err(crate::Error::Unsupported(
                "placed field visit budget exceeded".into(),
            ));
        }
        if record.header.kind == *b"REFR" && variable(counts, 4, limits).is_err() {
            excess = Some("variable bytes");
            return Err(crate::Error::Unsupported(
                "placed field name byte budget exceeded".into(),
            ));
        }
        Ok(())
    });
    if let Some(kind) = excess {
        return Err(Error::Capacity(kind));
    }
    result?;
    Ok(())
}
fn record_proof(
    store: &RecordStore,
    at: Location,
    key: &FormKey,
    record: &Record,
    receipts: &BTreeMap<&str, &SourceReceipt>,
    counts: &mut Counts,
    limits: Limits,
) -> Result<RecordProof, Error> {
    let name = store.source_name(at);
    let receipt = receipts
        .get(name)
        .ok_or(Error::Unavailable("source receipt missing"))?;
    for size in [
        key.origin_plugin.len(),
        name.len(),
        receipt.source_sha256.len(),
        64,
    ] {
        variable(counts, size, limits)?;
    }
    Ok(RecordProof {
        key: key.clone(),
        header: record.header.clone(),
        source_plugin: name.into(),
        source_sha256: receipt.source_sha256.clone(),
        decoded_record_sha256: format!("{:x}", Sha256::digest(&record.payload)),
    })
}
fn preflight_store(
    store: &RecordStore,
    placed: &FormKey,
    limits: Limits,
) -> Result<(Counts, usize), Error> {
    let mut counts = Counts {
        sources: store.indices().len(),
        ..Default::default()
    };
    if counts.sources > limits.maximum_sources {
        return Err(Error::Capacity("sources"));
    }
    // Admit all names and complete hash work before source_receipts clones any
    // strings. Masters also bound allocation in the existing form resolver.
    let mut maximum_name = placed.origin_plugin.len();
    variable(&mut counts, placed.origin_plugin.len(), limits)?;
    for index in store.indices() {
        counts.source_bytes = counts
            .source_bytes
            .checked_add(index.census.source_bytes)
            .filter(|&n| n <= limits.maximum_source_bytes)
            .ok_or(Error::Capacity("source bytes"))?;
        for name in std::iter::once(&index.census.name).chain(&index.census.masters) {
            maximum_name = maximum_name.max(name.len());
            variable(&mut counts, name.len(), limits)?;
        }
        variable(&mut counts, 64, limits)?;
        charge(
            &mut counts.header_visits,
            index.records.len(),
            limits.maximum_header_visits,
            "header visits",
        )?;
    }
    for (key, _) in store.winning_definitions() {
        charge(
            &mut counts.header_visits,
            1,
            limits.maximum_header_visits,
            "header visits",
        )?;
        // Borrowed names are admitted before the digest's canonical-name checks.
        if key.origin_plugin.len() > limits.maximum_variable_bytes {
            return Err(Error::Capacity("variable bytes"));
        }
    }
    if placed.profile != ProfileId::NvOriginal
        || placed.local_id > 0x00ff_ffff
        || plugin_name(&placed.origin_plugin)?.as_str() != placed.origin_plugin
    {
        return Err(Error::Unavailable("noncanonical placed identity"));
    }
    Ok((counts, maximum_name))
}
/// Diagnostic admission for consumers about to use the existing catalogue
/// loader. This is not a sealed attachment or executable authority.
pub fn preflight(store: &RecordStore, placed: &FormKey, limits: Limits) -> Result<Counts, Error> {
    preflight_store(store, placed, limits).map(|(counts, _)| counts)
}
pub fn request<'a>(
    store: &mut RecordStore,
    catalogue: &'a Catalogue,
    placed: &FormKey,
    limits: Limits,
) -> Result<Request<'a>, Error> {
    let (mut counts, maximum_name) = preflight_store(store, placed, limits)?;
    if counts.sources != catalogue.sources.len()
        || store
            .indices()
            .iter()
            .zip(&catalogue.sources)
            .any(|(index, expected)| {
                index.census.name != expected.source_name
                    || index.census.source_bytes != expected.source_bytes
            })
    {
        return Err(Error::Unavailable("ordered source cohort differs"));
    }
    let receipts = store.source_receipts()?;
    if receipts.iter().zip(&catalogue.sources).any(|(a, b)| {
        a.source_name != b.source_name
            || a.source_bytes != b.source_bytes
            || a.source_sha256 != b.source_sha256
    }) {
        return Err(Error::Unavailable("ordered source cohort differs"));
    }
    let receipt_index: BTreeMap<_, _> = receipts
        .iter()
        .map(|r| (r.source_name.as_str(), r))
        .collect();
    if receipt_index.len() != receipts.len() {
        return Err(Error::Unavailable("duplicate source receipt"));
    }
    for (_, script) in catalogue.iter() {
        charge(
            &mut counts.catalogue_scripts,
            1,
            limits.maximum_catalogue_scripts,
            "catalogue scripts",
        )?;
        let version = script.version();
        let receipt = receipt_index
            .get(version.source_plugin.as_str())
            .ok_or(Error::Unavailable("retained script source missing"))?;
        if version.source_sha256 != receipt.source_sha256 {
            return Err(Error::Unavailable("retained script source digest differs"));
        }
    }
    if record_metadata::inspect(store)?.winning_definitions_sha256
        != catalogue.winning_content_sha256()
    {
        return Err(Error::Unavailable("winning header cohort differs"));
    }
    let placement_at = location(store, placed, *b"REFR")?;
    let placement_record = read(store, placement_at, &mut counts, limits)?;
    fields(
        &placement_record,
        store.source_name(placement_at),
        &mut counts,
        limits,
        2,
    )?;
    let placement = world::decode_placement(&placement_record, store.source_name(placement_at))?;
    // key_for and indexed record_scripts clone canonical names. Reserve before
    // either adapter; the reservation is a conservative logical byte count.
    variable(&mut counts, maximum_name, limits)?;
    let base_key = store
        .key_for(placement_at, placement.base.value)?
        .ok_or(Error::Unavailable("null NAME base"))?;
    let base_at = location(store, &base_key, *b"CONT")?;
    let base_record = read(store, base_at, &mut counts, limits)?;
    fields(
        &base_record,
        store.source_name(base_at),
        &mut counts,
        limits,
        2,
    )?;
    let mut scri = None;
    let mut bad = None;
    plugin::visit_subrecords(&base_record, store.source_name(base_at), |sub| {
        if sub.kind == *b"SCRI" {
            if scri.is_some() {
                bad = Some("multiple SCRI fields");
            }
            if sub.data.len() != 4 {
                bad = Some("unsupported SCRI field width");
            } else {
                scri = Some((
                    sub.payload_offset,
                    u32::from_le_bytes(sub.data.try_into().expect("four bytes")),
                ));
            }
        }
        Ok(())
    })?;
    if let Some(reason) = bad {
        return Err(Error::Unavailable(reason));
    }
    let (scri_offset, raw_script) = scri.ok_or(Error::Unavailable("missing SCRI field"))?;
    variable(&mut counts, maximum_name, limits)?;
    let script_key = store
        .key_for(base_at, raw_script)?
        .ok_or(Error::Unavailable("null SCRI target"))?;
    let script_at = location(store, &script_key, *b"SCPT")?;
    variable(
        &mut counts,
        script_key
            .origin_plugin
            .len()
            .checked_mul(2)
            .ok_or(Error::Capacity("variable bytes"))?,
        limits,
    )?;
    let mut units = catalogue.record_scripts(&script_key);
    let script = units
        .next()
        .ok_or(Error::Unavailable("missing loaded script unit"))?;
    if units.next().is_some()
        || script.owner().kind != OwnerKind::Standalone
        || !script.owner().schema_ownership_verified
        || !script.issues().is_empty()
    {
        return Err(Error::Unavailable(
            "script is not one verified standalone unit",
        ));
    }
    drop(units);
    let script_record = read(store, script_at, &mut counts, limits)?;
    fields(
        &script_record,
        store.source_name(script_at),
        &mut counts,
        limits,
        1,
    )?;
    let placement_proof = record_proof(
        store,
        placement_at,
        placed,
        &placement_record,
        &receipt_index,
        &mut counts,
        limits,
    )?;
    let base_proof = record_proof(
        store,
        base_at,
        &base_key,
        &base_record,
        &receipt_index,
        &mut counts,
        limits,
    )?;
    let script_proof = record_proof(
        store,
        script_at,
        &script_key,
        &script_record,
        &receipt_index,
        &mut counts,
        limits,
    )?;
    let version = script.version();
    if version.source_plugin != script_proof.source_plugin
        || version.source_sha256 != script_proof.source_sha256
        || version.record_file_offset != script_record.header.offset
        || version.record_flags != script_record.header.flags
        || version.decoded_record_sha256 != script_proof.decoded_record_sha256
    {
        return Err(Error::Unavailable(
            "selected script version differs from fresh record",
        ));
    }
    Ok(Request {
        catalogue,
        script,
        receipts,
        counts,
        proof: Proof {
            placement: placement_proof,
            name: LinkProof {
                decoded_offset: placement.base.decoded_offset,
                raw_form: placement.base.value,
                key: base_key,
            },
            base: base_proof,
            scri: LinkProof {
                decoded_offset: scri_offset,
                raw_form: raw_script,
                key: script_key,
            },
            script: script_proof,
        },
    })
}
