//! Loaded script definitions are immutable views of winning source records.
//! A handle names both the authored unit and its exact source version; it never
//! stands in for a running script instance, event list or variable value.
use crate::{
    Error, Result, content,
    identity::FormKey,
    narrative, obscript, obscript_census, plugin, script_bindings, script_units,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_candidate_records: usize,
    /// Sum of max(stored bytes, decoded bytes) for every read candidate,
    /// including records with no script units. Both representations must fit.
    pub max_candidate_read_bytes: usize,
    /// Stored and decoded body bound, tightened further by remaining read bytes.
    pub max_candidate_record_bytes: usize,
    pub max_scripts: usize,
    pub max_retained_bytes: usize,
    pub max_variables: usize,
    pub max_references: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_candidate_records: 1_000_000,
            max_candidate_read_bytes: 512 * 1024 * 1024,
            max_candidate_record_bytes: 64 * 1024 * 1024,
            max_scripts: 262_144,
            max_retained_bytes: 256 * 1024 * 1024,
            max_variables: 262_144,
            max_references: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptKey {
    pub record: FormKey,
    /// An authored SCHR marker within the decoded winning record. This remains
    /// stable when unrelated plugins reorder, not across arbitrary record edits.
    pub header_decoded_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    pub key: ScriptKey,
    pub version_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Version {
    pub source_plugin: String,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: String,
    pub metadata_sha256: String,
    pub compiled_sha256: Option<String>,
    pub compiled_bytes: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    Standalone,
    QuestLogEntry,
    DialogueBegin,
    DialogueEnd,
    UnverifiedEmbedded,
}

#[derive(Debug, Clone, Serialize)]
pub struct Owner {
    pub kind: OwnerKind,
    pub section_marker: Option<u32>,
    pub stage_marker: Option<u32>,
    pub schema_ownership_verified: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Declaration {
    pub index: u32,
    pub type_byte: u8,
    pub decoded_offset: u32,
    pub name_bytes: usize,
    pub name_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub source_plugin: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub record_kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStatus {
    DefinedForm,
    DeletedForm,
    MissingForm,
    NullForm,
    RuntimeDependency,
    DynamicVariable,
    MissingVariableDeclaration,
}

#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    pub index: u32,
    pub decoded_offset: u32,
    pub source_kind: String,
    pub value: u32,
    pub status: ReferenceStatus,
    pub form_key: Option<FormKey>,
    pub target: Option<Target>,
    pub variable_declaration_offset: Option<u32>,
    pub runtime_dependency: Option<content::RuntimeBinding>,
}

pub struct LoadedScript {
    handle: Handle,
    version: Version,
    owner: Owner,
    record: Arc<plugin::Record>,
    compiled: Option<Range<usize>>,
    declarations: Vec<Declaration>,
    first_declaration: BTreeMap<u32, usize>,
    names: Vec<Range<usize>>,
    references: Vec<Reference>,
    script_type: u16,
    flags: u16,
    issues: Vec<String>,
}
impl LoadedScript {
    pub fn handle(&self) -> &Handle {
        &self.handle
    }
    pub fn version(&self) -> &Version {
        &self.version
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }
    pub fn references(&self) -> &[Reference] {
        &self.references
    }
    pub fn issues(&self) -> &[String] {
        &self.issues
    }
    pub fn script_type(&self) -> u16 {
        self.script_type
    }
    pub fn flags(&self) -> u16 {
        self.flags
    }
    pub fn compiled(&self) -> Option<&[u8]> {
        self.compiled
            .as_ref()
            .map(|range| &self.record.payload[range.clone()])
    }
    /// Charge work that reconstructs an owning unit's metadata view. Embedded
    /// units share the payload, but preparing each one can revisit that payload.
    pub fn decoded_record_bytes(&self) -> usize {
        self.record.payload.len()
    }
    pub fn program(
        &self,
    ) -> std::result::Result<Option<obscript::Program<'_>>, obscript::DecodeError> {
        self.compiled()
            .map(|bytes| obscript::decode(bytes, obscript::Limits::default()))
            .transpose()
    }

    /// The existing operand decoder needs the authored table view. Recreate that
    /// checked borrow on demand; running instances never mutate these bytes.
    pub fn bind_operands(
        &self,
        operators: &obscript::expression::Operators,
        signatures: &obscript::argument_census::Signatures,
        maximum_uses: usize,
    ) -> Result<Option<obscript::operand_binding::Binding>> {
        let Some(program) = self
            .program()
            .map_err(|error| Error::Resolution(error.to_string()))?
        else {
            return Ok(None);
        };
        let units = script_units::decode(
            &self.record,
            &self.version.source_plugin,
            script_units::Limits::default(),
        )?;
        let unit = units
            .iter()
            .find(|unit| unit.header.offset as u32 == self.handle.key.header_decoded_offset)
            .ok_or_else(|| Error::Resolution("loaded script lost its authored unit".into()))?;
        Ok(Some(obscript::operand_binding::bind(
            unit,
            &program,
            operators,
            signatures,
            maximum_uses,
        )?))
    }
    pub fn declaration(&self, index: u32) -> Option<&Declaration> {
        self.first_declaration
            .get(&index)
            .map(|&position| &self.declarations[position])
    }
    pub fn declaration_name(&self, index: u32) -> Option<&[u8]> {
        self.first_declaration
            .get(&index)
            .map(|&position| &self.record.payload[self.names[position].clone()])
    }
    /// Encoded references are one-based. Zero never aliases entry one.
    pub fn reference(&self, index: u32) -> Option<&Reference> {
        self.references.get(index.checked_sub(1)? as usize)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub candidate_records_read: u64,
    pub deleted_candidates_skipped: u64,
    pub records_retained: u64,
    pub payload_bytes_scanned: u64,
    pub payload_bytes_retained: u64,
    pub scripts: u64,
    pub compiled_bodies: u64,
    pub compiled_bytes: u64,
    pub variables: u64,
    pub duplicate_variable_indices: u64,
    pub references: u64,
    pub scripts_with_issues: u64,
    pub source_ownership_findings: u64,
    pub record_kinds: BTreeMap<String, u64>,
    pub owners: BTreeMap<String, u64>,
    pub reference_statuses: BTreeMap<String, u64>,
}

pub struct Catalogue {
    scripts: BTreeMap<ScriptKey, LoadedScript>,
    winning_content_sha256: String,
    pub counts: Counts,
    pub sources: Vec<SourceReceipt>,
}
impl Catalogue {
    /// Snapshot consumers bind every winning content identity, including forms
    /// outside scripts. The source digests separately bind deferred body bytes.
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn get(&self, key: &ScriptKey) -> Option<&LoadedScript> {
        self.scripts.get(key)
    }
    pub fn get_handle(&self, handle: &Handle) -> Option<&LoadedScript> {
        self.get(&handle.key)
            .filter(|script| script.handle.version_sha256 == handle.version_sha256)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ScriptKey, &LoadedScript)> {
        self.scripts.iter()
    }

    pub fn record_scripts(&self, record: &FormKey) -> impl Iterator<Item = &LoadedScript> {
        self.scripts
            .range(
                ScriptKey {
                    record: record.clone(),
                    header_decoded_offset: 0,
                }..=ScriptKey {
                    record: record.clone(),
                    header_decoded_offset: u32::MAX,
                },
            )
            .map(|(_, script)| script)
    }

    pub fn load(
        store: &mut RecordStore,
        limits: Limits,
        mut observe: impl FnMut(usize, &plugin::Record) -> Result<()>,
    ) -> Result<Self> {
        let sources = store.source_receipts()?;
        let mut candidates = Vec::new();
        for (key, location) in store.winning_definitions() {
            if !obscript_census::script_record(store.definition(location).header.kind) {
                continue;
            }
            if candidates.len() >= limits.max_candidate_records {
                return Err(Error::Unsupported(
                    "loaded script candidate budget exceeded".into(),
                ));
            }
            candidates.push((key.clone(), location));
        }
        let mut catalogue = Self {
            scripts: BTreeMap::new(),
            winning_content_sha256: crate::record_metadata::inspect(store)?
                .winning_definitions_sha256,
            counts: Counts::default(),
            sources,
        };
        let mut candidate_read_bytes = 0;
        for (key, location) in candidates {
            if store.definition(location).header.flags & plugin::DELETED != 0 {
                catalogue.counts.deleted_candidates_skipped += 1;
                continue;
            }
            let maximum = limits.max_candidate_record_bytes.min(
                limits
                    .max_candidate_read_bytes
                    .saturating_sub(candidate_read_bytes),
            );
            let record = store.read_bounded(location, maximum)?;
            if record.integrity_issue.is_some() {
                return Err(crate::malformed(
                    store.source_name(location),
                    record.header.offset,
                    "loaded script source is untrusted checksum recovery",
                ));
            }
            // Both representations passed the same bound before allocation.
            // Empty candidates consume read work even though no body is retained.
            candidate_read_bytes += (record.header.stored_size as usize).max(record.payload.len());
            let record = Arc::new(record);
            catalogue.counts.candidate_records_read += 1;
            catalogue.counts.payload_bytes_scanned += record.payload.len() as u64;
            let units = script_units::decode(
                &record,
                store.source_name(location),
                script_units::Limits::default(),
            )?;
            if units.is_empty() {
                continue;
            }
            if catalogue.counts.payload_bytes_retained + record.payload.len() as u64
                > limits.max_retained_bytes as u64
            {
                return Err(Error::Unsupported(
                    "loaded script retained byte budget exceeded".into(),
                ));
            }
            catalogue.counts.records_retained += 1;
            catalogue.counts.payload_bytes_retained += record.payload.len() as u64;
            let (owners, owner_findings) =
                owners(&record, store.source_name(location), units.len())?;
            catalogue.counts.source_ownership_findings += owner_findings;
            let body_hash = format!("{:x}", Sha256::digest(&record.payload));
            for unit in &units {
                if catalogue.scripts.len() >= limits.max_scripts
                    || catalogue.counts.variables + unit.variables.len() as u64
                        > limits.max_variables as u64
                    || catalogue.counts.references + unit.references.len() as u64
                        > limits.max_references as u64
                {
                    return Err(Error::Unsupported(
                        "loaded script aggregate metadata budget exceeded".into(),
                    ));
                }
                let script = load_definition(
                    UnitSource {
                        store,
                        location,
                        key: &key,
                        record: &record,
                        source: &catalogue.sources[location.plugin],
                        body_hash: &body_hash,
                    },
                    unit,
                    owners.get(&unit.header.offset).cloned().unwrap_or(Owner {
                        kind: OwnerKind::UnverifiedEmbedded,
                        section_marker: Some(unit.header.offset as u32),
                        stage_marker: None,
                        schema_ownership_verified: false,
                    }),
                )?;
                count_script(&mut catalogue.counts, &script, record.header.kind);
                if catalogue
                    .scripts
                    .insert(script.handle.key.clone(), script)
                    .is_some()
                {
                    return Err(Error::Resolution("duplicate loaded script identity".into()));
                }
            }
            observe(location.plugin, &record)?;
        }
        Ok(catalogue)
    }
}

fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .expect("script enum")
        .as_str()
        .expect("enum string")
        .into()
}

fn version_digest(key: &ScriptKey, version: &Version) -> String {
    let mut hash = Sha256::new();
    hash.update(b"FRSCRV01");
    fn text(hash: &mut Sha256, value: &str) {
        hash.update((value.len() as u32).to_le_bytes());
        hash.update(value.as_bytes());
    }
    text(&mut hash, &enum_name(&key.record.profile));
    text(&mut hash, &key.record.origin_plugin);
    hash.update(key.record.local_id.to_le_bytes());
    hash.update(key.header_decoded_offset.to_le_bytes());
    text(&mut hash, &version.source_plugin);
    text(&mut hash, &version.source_sha256);
    hash.update(version.record_file_offset.to_le_bytes());
    hash.update(version.record_flags.to_le_bytes());
    text(&mut hash, &version.decoded_record_sha256);
    text(&mut hash, &version.metadata_sha256);
    hash.update([u8::from(version.compiled_sha256.is_some())]);
    if let Some(compiled) = &version.compiled_sha256 {
        text(&mut hash, compiled);
        hash.update((version.compiled_bytes.expect("paired compiled extent") as u64).to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn count_script(counts: &mut Counts, script: &LoadedScript, kind: [u8; 4]) {
    counts.scripts += 1;
    counts.variables += script.declarations.len() as u64;
    counts.references += script.references.len() as u64;
    counts.duplicate_variable_indices +=
        (script.declarations.len() - script.first_declaration.len()) as u64;
    counts.scripts_with_issues += u64::from(!script.issues.is_empty());
    if let Some(compiled) = &script.compiled {
        counts.compiled_bodies += 1;
        counts.compiled_bytes += compiled.len() as u64;
    }
    *counts
        .record_kinds
        .entry(plugin::signature(kind))
        .or_default() += 1;
    *counts
        .owners
        .entry(enum_name(&script.owner.kind))
        .or_default() += 1;
    for reference in &script.references {
        *counts
            .reference_statuses
            .entry(enum_name(&reference.status))
            .or_default() += 1;
    }
}

fn owners(
    record: &plugin::Record,
    name: &str,
    units: usize,
) -> Result<(BTreeMap<usize, Owner>, u64)> {
    let mut owners = BTreeMap::new();
    if record.header.kind == *b"SCPT" {
        plugin::visit_subrecords(record, name, |field| {
            if field.kind == *b"SCHR" {
                owners.insert(
                    field.payload_offset,
                    Owner {
                        kind: OwnerKind::Standalone,
                        section_marker: None,
                        stage_marker: None,
                        schema_ownership_verified: units == 1,
                    },
                );
            }
            Ok(())
        })?;
        return Ok((owners, u64::from(units != 1)));
    }
    if !matches!(&record.header.kind, b"QUST" | b"INFO") {
        return Ok((owners, 0));
    }
    let document = narrative::decode(record, name, narrative::Limits::default())?;
    for script in &document.scripts {
        let Some(section) = script.owner.map(|index| &document.sections[index]) else {
            continue;
        };
        let kind = match section.kind {
            narrative::SectionKind::LogEntry => OwnerKind::QuestLogEntry,
            narrative::SectionKind::BeginScript => OwnerKind::DialogueBegin,
            narrative::SectionKind::EndScript => OwnerKind::DialogueEnd,
            _ => OwnerKind::UnverifiedEmbedded,
        };
        let stage = if kind == OwnerKind::QuestLogEntry {
            section
                .parent
                .and_then(|index| document.sections[index].marker_offset)
        } else {
            None
        };
        owners.insert(
            script.unit.header.offset,
            Owner {
                kind,
                section_marker: section.marker_offset.map(|offset| offset as u32),
                stage_marker: stage.map(|offset| offset as u32),
                schema_ownership_verified: kind != OwnerKind::UnverifiedEmbedded
                    && section.parent.is_some(),
            },
        );
    }
    Ok((owners, document.findings.len() as u64))
}

struct UnitSource<'a> {
    store: &'a RecordStore,
    location: Location,
    key: &'a FormKey,
    record: &'a Arc<plugin::Record>,
    source: &'a SourceReceipt,
    body_hash: &'a str,
}
fn load_definition(
    input: UnitSource<'_>,
    unit: &script_units::Unit<'_>,
    owner: Owner,
) -> Result<LoadedScript> {
    let UnitSource {
        store,
        location,
        key,
        record,
        source,
        body_hash,
    } = input;
    let key = ScriptKey {
        record: key.clone(),
        header_decoded_offset: unit.header.offset as u32,
    };
    let compiled = unit
        .compiled
        .map(|field| field.offset + 6..field.offset + 6 + field.data.len());
    if let Some(bytes) = unit.compiled {
        obscript::decode(bytes.data, obscript::Limits::default())
            .map_err(|error| Error::Resolution(error.to_string()))?;
    }
    let version = Version {
        source_plugin: source.source_name.clone(),
        source_sha256: source.source_sha256.clone(),
        record_file_offset: record.header.offset,
        record_flags: record.header.flags,
        decoded_record_sha256: body_hash.into(),
        metadata_sha256: script_bindings::metadata_digest(unit),
        compiled_sha256: unit
            .compiled
            .map(|field| format!("{:x}", Sha256::digest(field.data))),
        compiled_bytes: unit.compiled.map(|field| field.data.len()),
    };
    let version_sha256 = version_digest(&key, &version);
    let mut declarations = Vec::with_capacity(unit.variables.len());
    let mut first = BTreeMap::new();
    let mut names = Vec::with_capacity(unit.variables.len());
    for variable in &unit.variables {
        first.entry(variable.index).or_insert(declarations.len());
        names.push(variable.name.offset + 6..variable.name.offset + 6 + variable.name.data.len());
        declarations.push(Declaration {
            index: variable.index,
            type_byte: variable.type_byte,
            decoded_offset: variable.declaration.offset as u32,
            name_bytes: variable.name.data.len(),
            name_sha256: format!("{:x}", Sha256::digest(variable.name.data)),
        });
    }
    let mut references = Vec::with_capacity(unit.references.len());
    for (index, entry) in unit.references.iter().enumerate() {
        let mut row = Reference {
            index: index as u32 + 1,
            decoded_offset: entry.field.offset as u32,
            source_kind: plugin::signature(entry.field.kind),
            value: 0,
            status: ReferenceStatus::NullForm,
            form_key: None,
            target: None,
            variable_declaration_offset: None,
            runtime_dependency: None,
        };
        match entry.target {
            script_units::Reference::Variable(index) => {
                row.value = index;
                row.variable_declaration_offset = unit
                    .variable(index)
                    .map(|variable| variable.declaration.offset as u32);
                row.status = if row.variable_declaration_offset.is_some() {
                    ReferenceStatus::DynamicVariable
                } else {
                    ReferenceStatus::MissingVariableDeclaration
                };
            }
            script_units::Reference::Form(raw) => {
                row.value = raw;
                row.form_key = store.key_for(location, raw)?;
                if let Some(key) = &row.form_key {
                    if let Some(location) = store.winner(key) {
                        let header = &store.definition(location).header;
                        row.status = if header.flags & plugin::DELETED != 0 {
                            ReferenceStatus::DeletedForm
                        } else {
                            ReferenceStatus::DefinedForm
                        };
                        row.target = Some(Target {
                            source_plugin: store.source_name(location).into(),
                            record_file_offset: header.offset,
                            record_flags: header.flags,
                            record_kind: plugin::signature(header.kind),
                        });
                    } else {
                        row.runtime_dependency = content::runtime_binding(key);
                        row.status = if row.runtime_dependency.is_some() {
                            ReferenceStatus::RuntimeDependency
                        } else {
                            ReferenceStatus::MissingForm
                        };
                    }
                }
            }
        }
        references.push(row);
    }
    let mut issues = Vec::new();
    if unit.declared_references() as usize != unit.references.len() {
        issues.push("SCHR reference count differs from ordered table length".into());
    }
    if unit.declared_compiled_bytes() as usize != unit.compiled.map_or(0, |field| field.data.len())
    {
        issues.push("SCHR compiled size differs from SCDA extent".into());
    }
    Ok(LoadedScript {
        handle: Handle {
            key,
            version_sha256,
        },
        version,
        owner,
        record: Arc::clone(record),
        compiled,
        declarations,
        first_declaration: first,
        names,
        references,
        script_type: unit.script_type(),
        flags: unit.flags(),
        issues,
    })
}
