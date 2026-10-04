//! Exact package condition and embedded script sources, never package execution.
use super::{fields::Finding, packages};
use crate::{
    Error, Result, condition_operands,
    identity::{self, FormKey},
    inventory, loaded_scripts, malformed, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_field_visits: usize,
    pub max_conditions: usize,
    pub max_condition_bytes: usize,
    pub max_event_fields: usize,
    pub max_event_links: usize,
    pub max_scripts: usize,
    pub max_declarations: usize,
    pub max_references: usize,
    /// Compact serialized definitions; not total process heap or pretty JSON.
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
            max_field_visits: 4_000_000,
            max_conditions: 1_000_000,
            max_condition_bytes: 128 * 1024 * 1024,
            max_event_fields: 1_000_000,
            max_event_links: 1_000_000,
            max_scripts: 262_144,
            max_declarations: 262_144,
            max_references: 1_000_000,
            max_projection_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Marker {
    pub kind: [u8; 4],
    pub field_index: usize,
    pub field_decoded_offset: u32,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventValue {
    Marker,
    Link {
        binding: inventory::Binding,
        schema_kind_allowed: Option<bool>,
    },
}
#[derive(Debug, Serialize)]
pub struct EventField {
    pub field_index: usize,
    pub field_kind: [u8; 4],
    pub field_decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    /// Nearest preceding physical marker, without an execution role claim.
    pub physical_marker: Option<Marker>,
    pub value: EventValue,
}
#[derive(Serialize)]
pub struct Embedded<'a> {
    pub field_index: usize,
    pub physical_marker: Option<Marker>,
    pub handle: &'a loaded_scripts::Handle,
    pub version: &'a loaded_scripts::Version,
    pub owner: &'a loaded_scripts::Owner,
    pub script_type: u16,
    pub flags: u16,
    pub declarations: &'a [loaded_scripts::Declaration],
    pub references: &'a [loaded_scripts::Reference],
    pub issues: &'a [String],
}
#[derive(Serialize)]
pub struct Definition<'a> {
    pub key: &'a FormKey,
    pub source: &'a inventory::Source,
    pub header: &'a plugin::RecordHeader,
    pub deleted: bool,
    pub conditions: Option<condition_operands::PreparedRecord>,
    pub event_fields: Vec<EventField>,
    pub scripts: Vec<Embedded<'a>>,
    pub findings: Vec<Finding>,
    #[serde(skip)]
    package: &'a packages::Definition,
}
impl Definition<'_> {
    pub fn record(&self) -> Option<&plugin::Record> {
        self.package.record()
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub field_visits: usize,
    pub conditions: usize,
    pub condition_retained_bytes: usize,
    pub event_fields: usize,
    pub event_links: usize,
    pub scripts: usize,
    pub compiled_scripts: usize,
    pub declarations: usize,
    pub references: usize,
    pub source_findings: usize,
    pub projection_bytes: usize,
}
pub struct Catalogue<'a> {
    sources: Vec<SourceReceipt>,
    winning_content_sha256: String,
    definitions: BTreeMap<&'a FormKey, Definition<'a>>,
    counts: Counts,
}

fn cohort(sources: &[SourceReceipt]) -> Result<BTreeMap<String, (u64, &str)>> {
    let mut result = BTreeMap::new();
    for source in sources {
        if result
            .insert(
                identity::plugin_name(&source.source_name)?,
                (source.source_bytes, source.source_sha256.as_str()),
            )
            .is_some()
        {
            return Err(Error::Resolution(
                "duplicate package dependency source".into(),
            ));
        }
    }
    Ok(result)
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "package dependency projection byte budget",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn projection_bytes(value: &impl Serialize, maximum: usize) -> Result<usize> {
    let mut admission = Admission { bytes: 0, maximum };
    serde_json::to_writer(&mut admission, value)
        .map_err(|error| Error::Unsupported(error.to_string()))?;
    Ok(admission.bytes)
}
fn budget(condition: bool, label: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "package dependency {label} budget"
        )))
    }
}
impl<'a> Catalogue<'a> {
    pub fn sources(&self) -> &[SourceReceipt] {
        &self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition<'a>> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition<'a>)> {
        self.definitions
            .iter()
            .map(|(key, definition)| (*key, definition))
    }
    pub fn load(
        store: &mut RecordStore,
        packages: &'a packages::Catalogue,
        scripts: &'a loaded_scripts::Catalogue,
        signatures: &condition_operands::Signatures,
        limits: Limits,
    ) -> Result<Self> {
        budget(packages.counts().records <= limits.max_records, "record")?;
        let sources = store.source_receipts()?;
        let winning_content_sha256 = record_metadata::inspect(store)?.winning_definitions_sha256;
        for (receipts, digest) in [
            (packages.sources(), packages.winning_content_sha256()),
            (scripts.sources.as_slice(), scripts.winning_content_sha256()),
        ] {
            if cohort(receipts)? != cohort(&sources)? || digest != winning_content_sha256 {
                return Err(Error::Resolution(
                    "package dependency source cohort differs".into(),
                ));
            }
        }
        let mut catalogue = Self {
            sources,
            winning_content_sha256,
            definitions: BTreeMap::new(),
            counts: Counts::default(),
        };
        for (key, package) in packages.iter() {
            let location = store
                .winner(key)
                .ok_or_else(|| Error::Resolution("package dependency winner missing".into()))?;
            if serde_json::to_value(&store.definition(location).header)
                .map_err(|error| Error::Resolution(error.to_string()))?
                != serde_json::to_value(&package.header)
                    .map_err(|error| Error::Resolution(error.to_string()))?
            {
                return Err(Error::Resolution(
                    "package dependency winning header differs".into(),
                ));
            }
            let mut definition = Definition {
                key,
                source: &package.source,
                header: &package.header,
                deleted: package.deleted,
                conditions: None,
                event_fields: Vec::new(),
                scripts: Vec::new(),
                findings: Vec::new(),
                package,
            };
            if package.deleted {
                catalogue.counts.deleted_records += 1;
            } else {
                let record = package
                    .record()
                    .ok_or_else(|| Error::Resolution("package dependency body missing".into()))?;
                if record.integrity_issue.is_some() {
                    return Err(malformed(
                        &package.source.plugin,
                        record.header.offset,
                        "tainted record cannot supply package dependency inputs",
                    ));
                }
                budget(
                    record.payload.len()
                        <= limits
                            .max_decoded_bytes
                            .saturating_sub(catalogue.counts.decoded_bytes),
                    "decoded byte",
                )?;
                budget(
                    package.fields.len()
                        <= limits.max_fields.saturating_sub(catalogue.counts.fields),
                    "field",
                )?;
                let visits = package.fields.len().checked_mul(2).ok_or_else(|| {
                    Error::Unsupported("package field visit count overflow".into())
                })?;
                budget(
                    visits
                        <= limits
                            .max_field_visits
                            .saturating_sub(catalogue.counts.field_visits),
                    "field visit",
                )?;
                let remaining_projection = limits
                    .max_projection_bytes
                    .saturating_sub(catalogue.counts.projection_bytes);
                let prepared = condition_operands::prepare_record(
                    store,
                    location,
                    signatures,
                    condition_operands::RecordLimits {
                        maximum_decoded_bytes: limits.max_record_bytes.min(
                            limits
                                .max_decoded_bytes
                                .saturating_sub(catalogue.counts.decoded_bytes),
                        ),
                        maximum_fields: limits.max_fields.saturating_sub(catalogue.counts.fields),
                        maximum_conditions: limits
                            .max_conditions
                            .saturating_sub(catalogue.counts.conditions),
                        maximum_retained_bytes: limits
                            .max_condition_bytes
                            .saturating_sub(catalogue.counts.condition_retained_bytes)
                            .min(remaining_projection),
                    },
                )?;
                if prepared.identity().key != *key
                    || prepared.identity().record_kind != "PACK"
                    || prepared.identity().source_sha256 != package.source.sha256
                    || identity::plugin_name(&prepared.identity().source_name)?
                        != identity::plugin_name(&package.source.plugin)?
                    || prepared.identity().decoded_sha256
                        != package
                            .source
                            .decoded_record_sha256
                            .as_deref()
                            .unwrap_or_default()
                    || prepared.identity().record_file_offset != package.header.offset
                    || prepared.identity().record_flags != package.header.flags
                    || prepared.identity().decoded_bytes != record.payload.len()
                    || prepared.fields() != package.fields.len()
                {
                    return Err(Error::Resolution(
                        "prepared package condition source differs".into(),
                    ));
                }
                catalogue.counts.decoded_bytes += prepared.identity().decoded_bytes;
                catalogue.counts.fields += prepared.fields();
                catalogue.counts.field_visits += visits;
                catalogue.counts.conditions += prepared.sites().len();
                catalogue.counts.condition_retained_bytes += prepared.retained_bytes();
                catalogue.counts.source_findings += prepared
                    .sites()
                    .iter()
                    .map(|site| site.source_findings().len())
                    .sum::<usize>();
                let mut view_bytes = projection_bytes(&prepared, remaining_projection)?;
                definition.conditions = Some(prepared);
                let mut marker = None;
                let mut unit_sites = Vec::new();
                let mut binding_counts = inventory::Counts::default();
                let mut field_index = 0;
                plugin::visit_subrecords(record, &package.source.plugin, |field| {
                    let index = field_index;
                    field_index += 1;
                    let offset = u32::try_from(field.payload_offset).map_err(|_| {
                        Error::Unsupported("package dependency offset exceeds u32".into())
                    })?;
                    if matches!(&field.kind, b"POBA" | b"POEA" | b"POCA" | b"INAM" | b"TNAM") {
                        budget(
                            catalogue.counts.event_fields < limits.max_event_fields,
                            "event field",
                        )?;
                    }
                    let value = match &field.kind {
                        b"POBA" | b"POEA" | b"POCA" => {
                            if !field.data.is_empty() {
                                return Err(malformed(
                                    &package.source.plugin,
                                    record.header.offset,
                                    "package event marker must contain zero bytes",
                                ));
                            }
                            marker = Some(Marker {
                                kind: field.kind,
                                field_index: index,
                                field_decoded_offset: offset,
                            });
                            Some(EventValue::Marker)
                        }
                        b"INAM" | b"TNAM" => {
                            if field.data.len() != 4 {
                                return Err(malformed(
                                    &package.source.plugin,
                                    record.header.offset,
                                    "package event link must contain four bytes",
                                ));
                            }
                            budget(
                                catalogue.counts.event_links < limits.max_event_links,
                                "event link",
                            )?;
                            let binding = inventory::binding(
                                store,
                                location,
                                u32::from_le_bytes(
                                    field.data.try_into().expect("checked event link"),
                                ),
                                &mut binding_counts,
                            )?;
                            let expected = if field.kind == *b"INAM" {
                                *b"IDLE"
                            } else {
                                *b"DIAL"
                            };
                            let allowed = binding
                                .target
                                .as_ref()
                                .map(|target| target.kind == expected);
                            if marker.is_none() {
                                definition.findings.push(Finding {
                                    field_decoded_offset: Some(offset),
                                    code: "package_event_link_without_marker",
                                });
                            }
                            if allowed == Some(false) {
                                definition.findings.push(Finding {
                                    field_decoded_offset: Some(offset),
                                    code: "package_event_link_schema_kind_mismatch",
                                });
                            }
                            catalogue.counts.event_links += 1;
                            Some(EventValue::Link {
                                binding,
                                schema_kind_allowed: allowed,
                            })
                        }
                        b"SCHR" => {
                            budget(
                                unit_sites.len()
                                    < limits.max_scripts.saturating_sub(catalogue.counts.scripts),
                                "script",
                            )?;
                            unit_sites.push((index, offset, marker));
                            None
                        }
                        _ => None,
                    };
                    if let Some(value) = value {
                        budget(
                            catalogue.counts.event_fields < limits.max_event_fields,
                            "event field",
                        )?;
                        let row = EventField {
                            field_index: index,
                            field_kind: field.kind,
                            field_decoded_offset: offset,
                            bytes: field.data.len(),
                            sha256: format!("{:x}", Sha256::digest(field.data)),
                            physical_marker: marker,
                            value,
                        };
                        view_bytes += projection_bytes(
                            &row,
                            remaining_projection.saturating_sub(view_bytes),
                        )?;
                        catalogue.counts.event_fields += 1;
                        definition.event_fields.push(row);
                    }
                    Ok(())
                })?;
                for (unit_index, script) in scripts.record_scripts(key).enumerate() {
                    let (_, offset, _) = unit_sites.get(unit_index).ok_or_else(|| {
                        Error::Resolution("package has unexpected loaded script unit".into())
                    })?;
                    let version = script.version();
                    if script.handle().key.header_decoded_offset != *offset
                        || version.decoded_record_sha256
                            != package
                                .source
                                .decoded_record_sha256
                                .as_deref()
                                .unwrap_or_default()
                        || version.source_sha256 != package.source.sha256
                        || version.record_file_offset != package.header.offset
                        || version.record_flags != package.header.flags
                        || identity::plugin_name(&version.source_plugin)?
                            != identity::plugin_name(&package.source.plugin)?
                    {
                        return Err(Error::Resolution(
                            "package embedded script source differs".into(),
                        ));
                    }
                    budget(
                        script.declarations().len()
                            <= limits
                                .max_declarations
                                .saturating_sub(catalogue.counts.declarations),
                        "declaration",
                    )?;
                    budget(
                        script.references().len()
                            <= limits
                                .max_references
                                .saturating_sub(catalogue.counts.references),
                        "reference",
                    )?;
                    let (field_index, _, physical_marker) = unit_sites[unit_index];
                    let row = Embedded {
                        field_index,
                        physical_marker,
                        handle: script.handle(),
                        version,
                        owner: script.owner(),
                        script_type: script.script_type(),
                        flags: script.flags(),
                        declarations: script.declarations(),
                        references: script.references(),
                        issues: script.issues(),
                    };
                    // Borrowed metadata is counted before retaining another view.
                    view_bytes +=
                        projection_bytes(&row, remaining_projection.saturating_sub(view_bytes))?;
                    catalogue.counts.scripts += 1;
                    catalogue.counts.compiled_scripts +=
                        usize::from(version.compiled_sha256.is_some());
                    catalogue.counts.declarations += script.declarations().len();
                    catalogue.counts.references += script.references().len();
                    catalogue.counts.source_findings += script.issues().len();
                    definition.scripts.push(row);
                }
                if definition.scripts.len() != unit_sites.len() {
                    return Err(Error::Resolution(
                        "package loaded script unit missing".into(),
                    ));
                }
            }
            catalogue.counts.source_findings += definition.findings.len();
            catalogue.counts.projection_bytes += projection_bytes(
                &definition,
                limits
                    .max_projection_bytes
                    .saturating_sub(catalogue.counts.projection_bytes),
            )?;
            catalogue.counts.records += 1;
            catalogue.definitions.insert(key, definition);
        }
        Ok(catalogue)
    }
}
