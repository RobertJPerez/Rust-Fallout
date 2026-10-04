//! Exact actor effect declarations and base-effect source requests, never effects.
use super::{Catalogue, Definition, associations};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_depth: usize,
    pub max_selected_records: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_visits: usize,
    pub max_fields: usize,
    pub max_bindings: usize,
    pub max_groups: usize,
    pub max_raw_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_depth: 2,
            max_selected_records: 4096,
            max_record_bytes: 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_field_visits: 200_000,
            max_fields: 65_536,
            max_bindings: 4096,
            max_groups: 4096,
            max_raw_bytes: 1024 * 1024,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    SpellMetadata {
        effect_type: u32,
        cost_unused: u32,
        level_unused: u32,
        flags: u8,
        unused: [u8; 3],
    },
    EnchantmentMetadata {
        effect_type: u32,
        unused_words: [u32; 2],
        flags: u8,
        unused: [u8; 3],
    },
    EffectData {
        magnitude: u32,
        area: u32,
        duration: u32,
        effect_type: u32,
        actor_value: i32,
        known_effect_type: bool,
    },
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub sha256: String,
    pub value: Value,
}
#[derive(Debug, Serialize)]
pub struct SourceRequest {
    pub key: FormKey,
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub header: plugin::RecordHeader,
}
#[derive(Debug, Serialize)]
pub struct Group {
    pub efid_field_index: usize,
    pub efit_field_indices: Vec<usize>,
    pub condition_field_indices: Vec<usize>,
    pub unknown_field_indices: Vec<usize>,
    pub binding: Option<inventory::Binding>,
    pub schema_kind_allowed: Option<bool>,
    pub base_effect: Option<SourceRequest>,
    pub binding_admitted: bool,
    pub data_admitted: bool,
    pub issues: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Declaration {
    pub source: SourceRequest,
    pub decoded_record_sha256: String,
    pub record_version_supported: bool,
    pub fields: Vec<Field>,
    pub metadata_field_indices: Vec<usize>,
    pub groups: Vec<Group>,
    pub issues: Vec<&'static str>,
}
/// A report cannot be deserialized into this source authority.
#[derive(Debug, Serialize)]
pub struct Manifest<'a> {
    sources: &'a [SourceReceipt],
    winning_content_sha256: &'a str,
    actor: &'a Definition<'a>,
    configuration_fields: Vec<&'a inventory::Field>,
    actor_effect_template_flag: Option<bool>,
    association: &'a associations::Association,
    actor_field: &'a super::fields::Field,
    actor_raw_bytes: Vec<u8>,
    singleton_repeated: bool,
    declaration_binding_available: bool,
    declaration: Option<Declaration>,
    selected_records: usize,
    source_depth: usize,
    field_visits: usize,
    retained_fields: usize,
    decoded_bytes: usize,
    raw_bytes: usize,
    bindings: usize,
    issues: Vec<&'static str>,
    active_effects_created: bool,
    execution_supported: bool,
    scope: &'static str,
}
impl Manifest<'_> {
    pub fn declaration(&self) -> Option<&Declaration> {
        self.declaration.as_ref()
    }
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor effect input {label} budget exceeded"
        )))
    }
}
fn add(value: &mut usize, amount: usize, maximum: usize, label: &str) -> Result<()> {
    *value = value
        .checked_add(amount)
        .ok_or_else(|| Error::Unsupported(format!("actor effect input {label} overflow")))?;
    budget(*value <= maximum, label)
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("admitted effect layout"))
}
fn source_request(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    key: &FormKey,
) -> Result<SourceRequest> {
    let at = store
        .winner(key)
        .ok_or_else(|| Error::Resolution("actor effect input winner unavailable".into()))?;
    let source = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor effect input source unavailable".into()))?;
    Ok(SourceRequest {
        key: key.clone(),
        source_name: source.source_name.clone(),
        source_bytes: source.source_bytes,
        source_sha256: source.source_sha256.clone(),
        header: store.definition(at).header.clone(),
    })
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor effect input projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Select one physical association occurrence. Source reads use existing strict APIs.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    root: &FormKey,
    actor_field_index: usize,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    let winners = record_metadata::inspect(store)?.winning_definitions_sha256;
    for (sources, digest) in [
        (actors.sources(), actors.winning_content_sha256()),
        (
            associations.sources(),
            associations.winning_content_sha256(),
        ),
    ] {
        budget(sources.len() <= limits.max_sources, "source")?;
        if !same_sources(sources, &receipts) || digest != winners {
            return Err(Error::Resolution(
                "actor effect input source cohorts differ".into(),
            ));
        }
    }
    let actor = actors.get(root).filter(|a| !a.deleted).ok_or_else(|| {
        Error::Resolution("actor effect input root unavailable or deleted".into())
    })?;
    let retained = actor
        .record()
        .ok_or_else(|| Error::Resolution("actor effect input retained actor unavailable".into()))?;
    let source = source_request(store, &receipts, root)?;
    if source.header != retained.header
        || source.source_name != actor.source.plugin
        || source.source_sha256 != actor.source.sha256
        || source.header.offset != actor.source.record_file_offset
        || source.header.flags != actor.source.record_flags
    {
        return Err(Error::Resolution(
            "actor effect input retained actor differs".into(),
        ));
    }
    let joined = associations
        .get(root)
        .ok_or_else(|| Error::Resolution("actor effect input associations unavailable".into()))?;
    let mut visits = 0;
    add(
        &mut visits,
        joined.associations.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        joined.associations.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        actor.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        actor.inventory_definition().fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    budget(actor.fields.len() <= limits.max_fields, "field")?;
    budget(
        retained.payload.len() <= limits.max_decoded_bytes,
        "decoded byte",
    )?;
    budget(limits.max_selected_records >= 1, "selected record")?;
    let association = joined
        .associations
        .iter()
        .find(|a| a.field_index == actor_field_index)
        .filter(|a| {
            matches!(
                a.role,
                associations::Role::ActorEffect | associations::Role::UnarmedEffect
            )
        })
        .ok_or_else(|| {
            Error::Resolution("chosen actor field is not an effect association".into())
        })?;
    let actor_field = actor
        .fields
        .get(actor_field_index)
        .ok_or_else(|| Error::Resolution("actor effect input physical field unavailable".into()))?;
    let expected_kind = if association.role == associations::Role::ActorEffect {
        *b"SPLO"
    } else {
        *b"EITM"
    };
    if actor_field.kind != expected_kind {
        return Err(Error::Resolution(
            "actor effect input association kind differs".into(),
        ));
    }
    let configurations: Vec<_> = actor
        .inventory_definition()
        .fields
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect();
    let template_flag = match configurations.as_slice() {
        [f] => match f.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags & 8 != 0),
            _ => None,
        },
        _ => None,
    };
    let singleton_repeated = association.role == associations::Role::UnarmedEffect
        && joined
            .associations
            .iter()
            .filter(|a| a.role == association.role)
            .count()
            > 1;
    let mut result = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields: configurations,
        actor_effect_template_flag: template_flag,
        association,
        actor_field,
        actor_raw_bytes: Vec::new(),
        singleton_repeated,
        declaration_binding_available: template_flag == Some(false)
            && !singleton_repeated
            && association.binding.status == inventory::Status::Defined
            && association.schema_kind_allowed == Some(true),
        declaration: None,
        selected_records: 1,
        source_depth: 0,
        field_visits: visits,
        retained_fields: actor.fields.len(),
        decoded_bytes: retained.payload.len(),
        raw_bytes: 0,
        bindings: 0,
        issues: Vec::new(),
        active_effects_created: false,
        execution_supported: false,
        scope: "Explicit physical actor SPLO/EITM occurrence and ordered SPEL/ENCH EFID/EFIT declarations with MGEF winning source-header requests; exact unsigned/signed/raw flags and opaque conditions, no template inheritance, editor rewrite, active effects, stacking, timing, magnitude conversion or execution",
    };
    add(
        &mut result.retained_fields,
        result.configuration_fields.len(),
        limits.max_fields,
        "field",
    )?;
    if template_flag.is_none() {
        result.issues.push("unique_configuration_unavailable");
    }
    if template_flag == Some(true) {
        result
            .issues
            .push("actor_effect_template_inheritance_unsupported");
    }
    if singleton_repeated {
        result.issues.push("repeated_unarmed_effect_association");
    }
    match association.binding.status {
        inventory::Status::Null => result.issues.push("null_effect_association"),
        inventory::Status::Missing => result.issues.push("missing_effect_association_target"),
        inventory::Status::Deleted => result.issues.push("deleted_effect_association_target"),
        inventory::Status::Defined => (),
    }
    if association.schema_kind_allowed == Some(false) {
        result.issues.push("effect_association_kind_not_allowed");
    }
    let mut seen = 0;
    plugin::visit_subrecords(retained, &actor.source.plugin, |field| {
        let index = seen;
        seen += 1;
        let cached = actor
            .fields
            .get(index)
            .ok_or_else(|| Error::Resolution("actor effect input physical field missing".into()))?;
        if cached.kind != field.kind
            || cached.decoded_offset as usize != field.payload_offset
            || cached.bytes != field.data.len()
        {
            return Err(Error::Resolution(
                "actor effect input physical field differs".into(),
            ));
        }
        if index == actor_field_index {
            add(
                &mut result.raw_bytes,
                field.data.len(),
                limits.max_raw_bytes,
                "raw byte",
            )?;
            if field.data.len() != 4 || word(field.data, 0) != association.binding.raw_form {
                return Err(Error::Resolution(
                    "actor effect input physical binding differs".into(),
                ));
            }
            result.actor_raw_bytes = field.data.to_vec();
        }
        Ok(())
    })?;
    if seen != actor.fields.len() {
        return Err(Error::Resolution(
            "actor effect input physical actor count differs".into(),
        ));
    }
    if association.binding.status == inventory::Status::Defined
        && association.schema_kind_allowed == Some(true)
    {
        let key = association
            .binding
            .key
            .as_ref()
            .ok_or_else(|| Error::Resolution("actor effect input bound key missing".into()))?;
        budget(limits.max_depth >= 1, "depth")?;
        result.source_depth = 1;
        add(
            &mut result.selected_records,
            1,
            limits.max_selected_records,
            "selected record",
        )?;
        let at = store
            .winner(key)
            .ok_or_else(|| Error::Resolution("actor effect input declaration missing".into()))?;
        let source = source_request(store, &receipts, key)?;
        let bound =
            association.binding.target.as_ref().ok_or_else(|| {
                Error::Resolution("actor effect input bound target missing".into())
            })?;
        if bound.kind != source.header.kind
            || bound.source_plugin != source.source_name
            || bound.record_file_offset != source.header.offset
            || bound.record_flags != source.header.flags
        {
            return Err(Error::Resolution(
                "actor effect input declaration identity differs".into(),
            ));
        }
        let record = store.read_bounded(
            at,
            limits.max_record_bytes.min(
                limits
                    .max_decoded_bytes
                    .saturating_sub(result.decoded_bytes),
            ),
        )?;
        if record.integrity_issue.is_some() {
            return Err(Error::Resolution(
                "actor effect declaration checksum differs".into(),
            ));
        }
        add(
            &mut result.decoded_bytes,
            record.payload.len(),
            limits.max_decoded_bytes,
            "decoded byte",
        )?;
        // This source profile is independently exercised with version15 bodies.
        // Other versions retain all bytes but cannot admit typed declarations.
        let supported = record.header.version == 15;
        let metadata_kind = if record.header.kind == *b"SPEL" {
            *b"SPIT"
        } else {
            *b"ENIT"
        };
        let mut declaration = Declaration {
            source,
            decoded_record_sha256: format!("{:x}", Sha256::digest(&record.payload)),
            record_version_supported: supported,
            fields: Vec::new(),
            metadata_field_indices: Vec::new(),
            groups: Vec::new(),
            issues: Vec::new(),
        };
        if !supported {
            declaration.issues.push("unsupported_effect_record_version");
        }
        let mut binding_counts = inventory::Counts::default();
        plugin::visit_subrecords(&record, &declaration.source.source_name, |field| {
            add(
                &mut result.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            add(&mut result.retained_fields, 1, limits.max_fields, "field")?;
            add(
                &mut result.raw_bytes,
                field.data.len(),
                limits.max_raw_bytes,
                "raw byte",
            )?;
            let index = declaration.fields.len();
            let offset = u32::try_from(field.payload_offset)
                .map_err(|_| Error::Unsupported("actor effect field offset exceeds u32".into()))?;
            let value = if supported && field.kind == metadata_kind && field.data.len() == 16 {
                declaration.metadata_field_indices.push(index);
                if metadata_kind == *b"SPIT" {
                    Value::SpellMetadata {
                        effect_type: word(field.data, 0),
                        cost_unused: word(field.data, 4),
                        level_unused: word(field.data, 8),
                        flags: field.data[12],
                        unused: field.data[13..16].try_into().expect("three unused bytes"),
                    }
                } else {
                    Value::EnchantmentMetadata {
                        effect_type: word(field.data, 0),
                        unused_words: [word(field.data, 4), word(field.data, 8)],
                        flags: field.data[12],
                        unused: field.data[13..16].try_into().expect("three unused bytes"),
                    }
                }
            } else if supported && field.kind == *b"EFIT" && field.data.len() == 20 {
                Value::EffectData {
                    magnitude: word(field.data, 0),
                    area: word(field.data, 4),
                    duration: word(field.data, 8),
                    effect_type: word(field.data, 12),
                    actor_value: word(field.data, 16) as i32,
                    known_effect_type: word(field.data, 12) <= 2,
                }
            } else {
                Value::Opaque
            };
            if field.kind == metadata_kind && !(supported && field.data.len() == 16) {
                declaration
                    .issues
                    .push("unsupported_effect_metadata_layout");
            }
            if field.kind == *b"EFID" {
                budget(declaration.groups.len() < limits.max_groups, "effect group")?;
                let mut group = Group {
                    efid_field_index: index,
                    efit_field_indices: Vec::new(),
                    condition_field_indices: Vec::new(),
                    unknown_field_indices: Vec::new(),
                    binding: None,
                    schema_kind_allowed: None,
                    base_effect: None,
                    binding_admitted: false,
                    data_admitted: false,
                    issues: Vec::new(),
                };
                if supported && field.data.len() == 4 {
                    add(&mut result.bindings, 1, limits.max_bindings, "binding")?;
                    let binding =
                        inventory::binding(store, at, word(field.data, 0), &mut binding_counts)?;
                    group.schema_kind_allowed = binding.target.as_ref().map(|t| t.kind == *b"MGEF");
                    match binding.status {
                        inventory::Status::Null => group.issues.push("null_base_effect"),
                        inventory::Status::Missing => group.issues.push("missing_base_effect"),
                        inventory::Status::Deleted => group.issues.push("deleted_base_effect"),
                        inventory::Status::Defined => (),
                    }
                    if group.schema_kind_allowed == Some(false) {
                        group.issues.push("base_effect_kind_not_allowed");
                    }
                    if let Some(key) = &binding.key
                        && store.winner(key).is_some()
                    {
                        budget(limits.max_depth >= 2, "depth")?;
                        result.source_depth = 2;
                        add(
                            &mut result.selected_records,
                            1,
                            limits.max_selected_records,
                            "selected record",
                        )?;
                        group.base_effect = Some(source_request(store, &receipts, key)?);
                    }
                    group.binding_admitted = result.declaration_binding_available
                        && binding.status == inventory::Status::Defined
                        && group.schema_kind_allowed == Some(true);
                    group.binding = Some(binding);
                } else {
                    group
                        .issues
                        .push("unsupported_base_effect_link_layout_or_version");
                }
                declaration.groups.push(group);
            } else if matches!(&field.kind, b"EFIT" | b"CTDA") {
                if let Some(group) = declaration.groups.last_mut() {
                    if field.kind == *b"EFIT" {
                        if !group.condition_field_indices.is_empty() {
                            group.issues.push("effect_data_after_conditions");
                        }
                        group.efit_field_indices.push(index);
                    } else {
                        if group.efit_field_indices.len() != 1 {
                            group.issues.push("condition_without_unique_effect_data");
                        }
                        group.condition_field_indices.push(index);
                    }
                } else {
                    declaration
                        .issues
                        .push("effect_field_without_explicit_efid_anchor");
                }
            } else if let Some(group) = declaration.groups.last_mut() {
                group.unknown_field_indices.push(index);
            }
            declaration.fields.push(Field {
                kind: field.kind,
                decoded_offset: offset,
                raw_bytes: field.data.to_vec(),
                sha256: format!("{:x}", Sha256::digest(field.data)),
                value,
            });
            Ok(())
        })?;
        if declaration.metadata_field_indices.len() != 1 {
            declaration
                .issues
                .push("unique_effect_metadata_unavailable");
        }
        if declaration.groups.is_empty() {
            declaration.issues.push("missing_explicit_effect_groups");
        }
        for group in &mut declaration.groups {
            add(
                &mut result.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            if group.efit_field_indices.len() != 1 {
                group.issues.push("unique_effect_data_unavailable");
            }
            if !group.unknown_field_indices.is_empty() {
                group.issues.push("unknown_effect_group_members");
            }
            if !group.condition_field_indices.is_empty() {
                group.issues.push("opaque_condition_requests_not_evaluated");
            }
            let data_known = match group.efit_field_indices.as_slice() {
                [index] => matches!(
                    declaration.fields[*index].value,
                    Value::EffectData {
                        known_effect_type: true,
                        ..
                    }
                ),
                _ => false,
            };
            if group.efit_field_indices.len() == 1 && !data_known {
                group.issues.push("unsupported_effect_data_layout_or_type");
            }
            group.data_admitted = supported
                && declaration.metadata_field_indices.len() == 1
                && declaration.issues.is_empty()
                && group.unknown_field_indices.is_empty()
                && data_known
                && !group.issues.iter().any(|i| {
                    matches!(
                        *i,
                        "effect_data_after_conditions" | "condition_without_unique_effect_data"
                    )
                });
        }
        result.declaration = Some(declaration);
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|e| Error::Unsupported(format!("actor effect input projection: {e}")))?;
    Ok(result)
}
