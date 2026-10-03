//! Winning placed-actor inputs over the existing world placement decoder.
//! Extra declarations do not initialize actors or infer inherited encounter zones.
use super::fields::Finding;
use crate::{
    Result,
    content::ParentContext,
    identity::FormKey,
    inventory, malformed, plugin, record_metadata,
    store::{Location, RecordStore, SourceReceipt},
    world,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_bindings: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
            max_bindings: 1_000_000,
        }
    }
}
/// Exact source words obtained from the existing decoded world placement, with
/// bit-preserving f32 serialization rather than decimal-rounding comparisons.
#[derive(Debug, Serialize)]
pub struct Core {
    pub base: world::SourceField<u32>,
    pub transform_decoded_offset: usize,
    pub position_bits: [u32; 3],
    pub rotation_bits: [u32; 3],
    pub scale: Option<world::SourceField<u32>>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    EncounterZone {
        zone: inventory::Binding,
        schema_kind_allowed: Option<bool>,
    },
    MerchantContainer {
        container: inventory::Binding,
        schema_kind_allowed: Option<bool>,
    },
    LevelModifier {
        modifier: i32,
    },
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    pub value: Value,
}
#[derive(Debug, Serialize)]
pub struct Definition {
    pub key: FormKey,
    pub source: inventory::Source,
    pub header: plugin::RecordHeader,
    /// Raw parent IDs remain relative to this winning source plugin.
    pub parent: ParentContext,
    pub deleted: bool,
    pub core: Option<Core>,
    pub base: Option<inventory::Binding>,
    pub base_schema_kind_allowed: Option<bool>,
    pub fields: Vec<Field>,
    pub findings: Vec<Finding>,
    #[serde(skip)]
    record: Option<plugin::Record>,
    #[serde(skip)]
    placement: Option<world::Placement>,
}
impl Definition {
    pub fn record(&self) -> Option<&plugin::Record> {
        self.record.as_ref()
    }
    pub fn placement(&self) -> Option<&world::Placement> {
        self.placement.as_ref()
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub selected_extra_fields: usize,
    pub bindings: usize,
    pub source_findings: usize,
    pub record_kinds: BTreeMap<String, usize>,
    pub record_versions: BTreeMap<String, usize>,
    pub extra_layouts: BTreeMap<String, usize>,
    pub binding_statuses: BTreeMap<String, usize>,
}
pub struct Catalogue {
    sources: Vec<SourceReceipt>,
    winning_content_sha256: String,
    definitions: BTreeMap<FormKey, Definition>,
    counts: Counts,
}
impl Catalogue {
    pub fn sources(&self) -> &[SourceReceipt] {
        &self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition)> {
        self.definitions.iter()
    }
    pub fn load(store: &mut RecordStore, limits: Limits) -> Result<Self> {
        let mut locations = Vec::new();
        for (key, at) in store.winning_definitions() {
            if !matches!(&store.definition(at).header.kind, b"ACHR" | b"ACRE") {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "placed actor record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), at));
        }
        let mut result = Self {
            sources: store.source_receipts()?,
            winning_content_sha256: record_metadata::inspect(store)?.winning_definitions_sha256,
            definitions: BTreeMap::new(),
            counts: Counts::default(),
        };
        let mut binding_counts = inventory::Counts::default();
        for (key, at) in locations {
            let input = store.definition(at);
            let header = input.header.clone();
            let mut definition = Definition {
                key: key.clone(),
                parent: input.parent.clone(),
                source: inventory::Source {
                    plugin: store.source_name(at).into(),
                    sha256: store.source_digest(at)?,
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                deleted: header.flags & plugin::DELETED != 0,
                header,
                core: None,
                base: None,
                base_schema_kind_allowed: None,
                fields: Vec::new(),
                findings: Vec::new(),
                record: None,
                placement: None,
            };
            if definition.deleted {
                result.counts.deleted_records += 1;
            } else {
                let supported = if definition.header.kind == *b"ACHR" {
                    definition.header.version == 15
                } else {
                    matches!(definition.header.version, 9 | 11 | 15)
                };
                if !supported {
                    return Err(crate::Error::Unsupported(format!(
                        "{} placed actor record version {} at {}:0x{:X}",
                        plugin::signature(definition.header.kind),
                        definition.header.version,
                        definition.source.plugin,
                        definition.header.offset
                    )));
                }
                let maximum = limits.max_record_bytes.min(
                    limits
                        .max_decoded_bytes
                        .saturating_sub(result.counts.decoded_bytes),
                );
                let record = store.read_bounded(at, maximum)?;
                result.counts.decoded_bytes += record.payload.len();
                // Bound every physical field before the world decoder allocates
                // its optional values and unhandled-field map.
                plugin::visit_subrecords(&record, &definition.source.plugin, |field| {
                    if result.counts.fields + definition.fields.len() >= limits.max_fields {
                        return Err(crate::Error::Unsupported(
                            "placed actor field budget exceeded".into(),
                        ));
                    }
                    let offset = u32::try_from(field.payload_offset).map_err(|_| {
                        crate::Error::Unsupported("placed actor offset exceeds u32".into())
                    })?;
                    if matches!(&field.kind, b"XEZN" | b"XMRC" | b"XLCM") && field.data.len() != 4 {
                        return Err(malformed(
                            &definition.source.plugin,
                            record.header.offset,
                            format!(
                                "unsupported placed actor {} length {} at decoded +0x{offset:X}",
                                plugin::signature(field.kind),
                                field.data.len()
                            ),
                        ));
                    }
                    definition.fields.push(Field {
                        kind: field.kind,
                        decoded_offset: offset,
                        bytes: field.data.len(),
                        sha256: format!("{:x}", Sha256::digest(field.data)),
                        value: Value::Opaque,
                    });
                    Ok(())
                })?;
                let placement = world::decode_placement(&record, &definition.source.plugin)?;
                definition.core = Some(Core {
                    base: placement.base.clone(),
                    transform_decoded_offset: placement.transform.decoded_offset,
                    position_bits: placement.transform.value.position.map(f32::to_bits),
                    rotation_bits: placement.transform.value.rotation.map(f32::to_bits),
                    scale: placement.scale.as_ref().map(|scale| world::SourceField {
                        decoded_offset: scale.decoded_offset,
                        value: scale.value.to_bits(),
                    }),
                });
                if result.counts.bindings >= limits.max_bindings {
                    return Err(crate::Error::Unsupported(
                        "placed actor binding budget exceeded".into(),
                    ));
                }
                let base_offset = u32::try_from(placement.base.decoded_offset).map_err(|_| {
                    crate::Error::Unsupported("placed actor base offset exceeds u32".into())
                })?;
                let expected_base = if definition.header.kind == *b"ACHR" {
                    *b"NPC_"
                } else {
                    *b"CREA"
                };
                let (base, allowed) = bind(
                    store,
                    at,
                    placement.base.value,
                    expected_base,
                    &mut binding_counts,
                    &mut definition.findings,
                    base_offset,
                )?;
                definition.base = Some(base);
                definition.base_schema_kind_allowed = allowed;
                result.counts.bindings += 1;
                let mut seen = [0usize; 3];
                let mut field_index = 0;
                plugin::visit_subrecords(&record, &definition.source.plugin, |field| {
                    let output = &mut definition.fields[field_index];
                    field_index += 1;
                    let selected = match &field.kind {
                        b"XEZN" => Some(0),
                        b"XMRC" => Some(1),
                        b"XLCM" => Some(2),
                        _ => None,
                    };
                    let Some(index) = selected else {
                        return Ok(());
                    };
                    seen[index] += 1;
                    if seen[index] > 1 {
                        definition.findings.push(Finding {
                            field_decoded_offset: Some(output.decoded_offset),
                            code: [
                                "multiple_placed_encounter_zone_fields",
                                "multiple_placed_merchant_fields",
                                "multiple_placed_level_modifier_fields",
                            ][index],
                        });
                    }
                    let raw = u32::from_le_bytes(
                        field.data.try_into().expect("checked placed actor word"),
                    );
                    output.value = if index == 2 {
                        Value::LevelModifier {
                            modifier: raw as i32,
                        }
                    } else {
                        if result.counts.bindings >= limits.max_bindings {
                            return Err(crate::Error::Unsupported(
                                "placed actor binding budget exceeded".into(),
                            ));
                        }
                        let expected = if index == 0 { *b"ECZN" } else { *b"REFR" };
                        let (binding, allowed) = bind(
                            store,
                            at,
                            raw,
                            expected,
                            &mut binding_counts,
                            &mut definition.findings,
                            output.decoded_offset,
                        )?;
                        result.counts.bindings += 1;
                        if index == 0 {
                            Value::EncounterZone {
                                zone: binding,
                                schema_kind_allowed: allowed,
                            }
                        } else {
                            Value::MerchantContainer {
                                container: binding,
                                schema_kind_allowed: allowed,
                            }
                        }
                    };
                    result.counts.selected_extra_fields += 1;
                    *result
                        .counts
                        .extra_layouts
                        .entry(format!(
                            "{}:{}",
                            plugin::signature(field.kind),
                            field.data.len()
                        ))
                        .or_default() += 1;
                    Ok(())
                })?;
                definition
                    .findings
                    .sort_by_key(|finding| finding.field_decoded_offset.unwrap_or(u32::MAX));
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                definition.record = Some(record);
                definition.placement = Some(placement);
                *result
                    .counts
                    .record_versions
                    .entry(format!(
                        "{}:{}",
                        plugin::signature(definition.header.kind),
                        definition.header.version
                    ))
                    .or_default() += 1;
            }
            result.counts.records += 1;
            result.counts.fields += definition.fields.len();
            result.counts.source_findings += definition.findings.len();
            *result
                .counts
                .record_kinds
                .entry(plugin::signature(definition.header.kind))
                .or_default() += 1;
            result.definitions.insert(key, definition);
        }
        result.counts.binding_statuses = binding_counts.binding_statuses;
        Ok(result)
    }
}
fn bind(
    store: &RecordStore,
    at: Location,
    raw: u32,
    expected: [u8; 4],
    counts: &mut inventory::Counts,
    findings: &mut Vec<Finding>,
    offset: u32,
) -> Result<(inventory::Binding, Option<bool>)> {
    let binding = inventory::binding(store, at, raw, counts)?;
    let allowed = binding
        .target
        .as_ref()
        .map(|target| target.kind == expected);
    let code = match binding.status {
        inventory::Status::Missing => Some("placement_target_missing"),
        inventory::Status::Deleted => Some("placement_target_deleted"),
        _ if allowed == Some(false) => Some("placement_target_wrong_kind"),
        _ => None,
    };
    if let Some(code) = code {
        findings.push(Finding {
            field_decoded_offset: Some(offset),
            code,
        });
    }
    Ok((binding, allowed))
}
