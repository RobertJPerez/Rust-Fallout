//! Fallout 4 workshop, recipe and scrap record structures. FormIDs stay unresolved.
use crate::{Error, Result, bad, census, condition};
use fallout_data::plugin::{self, Record, Subrecord};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RawFormId {
    pub subrecord: String,
    pub raw: u32,
    pub payload_offset: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Component {
    pub component: RawFormId,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CreatedObjectCount {
    pub count: u16,
    /// Mutagen's COBJ schema records a VersioningBreak when this field is absent.
    pub priority: Option<u16>,
    pub payload_offset: usize,
}

/// Data retained for future field decoding, with an exact source coordinate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpaqueSubrecord {
    pub kind: String,
    pub payload_offset: usize,
    pub bytes_hex: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Recipe {
    pub plugin: String,
    pub plugin_sha256: String,
    pub record_offset: u64,
    pub record_version: u16,
    pub record_flags: u32,
    /// Raw header bits only; no FormKey or load-order slot is assigned.
    pub form_id_raw: u32,
    pub editor_ids: Vec<OpaqueSubrecord>,
    pub created_objects: Vec<RawFormId>,
    pub workbench_keywords: Vec<RawFormId>,
    pub menu_art_objects: Vec<RawFormId>,
    pub pickup_sounds: Vec<RawFormId>,
    pub putdown_sounds: Vec<RawFormId>,
    pub components: Vec<Component>,
    pub categories: Vec<RawFormId>,
    pub created_object_counts: Vec<CreatedObjectCount>,
    /// CTDA and its CIS1/CIS2 companions are preserved but not evaluated.
    pub condition_subrecords: Vec<OpaqueSubrecord>,
    /// CTDA fixed fields decoded structurally; function parameters remain raw.
    pub decoded_conditions: Vec<condition::RawCondition>,
    /// Unknown and byte-array fields are retained without guessed semantics.
    pub opaque_subrecords: Vec<OpaqueSubrecord>,
    pub diagnostics: Vec<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_offset_for(
    name: &str,
    record_offset: u64,
    record_kind: &str,
    sub_offset: usize,
    field: &str,
) -> Error {
    let offset = usize::try_from(record_offset).unwrap_or(usize::MAX);
    bad(
        name,
        offset,
        format!("{record_kind}/{field} at decoded +0x{sub_offset:X}"),
    )
}

fn source_offset(name: &str, record_offset: u64, sub_offset: usize, field: &str) -> Error {
    source_offset_for(name, record_offset, "COBJ", sub_offset, field)
}

fn raw_id_for(
    name: &str,
    record_offset: u64,
    record_kind: &str,
    sub: Subrecord<'_>,
) -> Result<RawFormId> {
    let kind = plugin::signature(sub.kind);
    let raw = sub.data.try_into().map(u32::from_le_bytes).map_err(|_| {
        source_offset_for(name, record_offset, record_kind, sub.payload_offset, &kind)
    })?;
    Ok(RawFormId {
        subrecord: kind,
        raw,
        payload_offset: sub.payload_offset,
    })
}

fn raw_id(name: &str, record_offset: u64, sub: Subrecord<'_>) -> Result<RawFormId> {
    raw_id_for(name, record_offset, "COBJ", sub)
}

fn opaque(sub: Subrecord<'_>) -> OpaqueSubrecord {
    OpaqueSubrecord {
        kind: plugin::signature(sub.kind),
        payload_offset: sub.payload_offset,
        bytes_hex: hex(sub.data),
        sha256: census::sha256(sub.data),
    }
}

fn fixed_entries<const N: usize>(
    name: &str,
    record_offset: u64,
    record_kind: &str,
    sub: &Subrecord<'_>,
) -> Result<Vec<[u8; N]>> {
    if !sub.data.len().is_multiple_of(N) {
        return Err(source_offset_for(
            name,
            record_offset,
            record_kind,
            sub.payload_offset,
            &plugin::signature(sub.kind),
        ));
    }
    Ok(sub.data.as_chunks::<N>().0.to_vec())
}

fn add_subrecord(recipe: &mut Recipe, name: &str, sub: Subrecord<'_>) -> Result<()> {
    let record_offset = recipe.record_offset;
    match &sub.kind {
        b"EDID" => recipe.editor_ids.push(opaque(sub)),
        b"CNAM" => recipe
            .created_objects
            .push(raw_id(name, record_offset, sub)?),
        b"BNAM" => recipe
            .workbench_keywords
            .push(raw_id(name, record_offset, sub)?),
        b"ANAM" => recipe
            .menu_art_objects
            .push(raw_id(name, record_offset, sub)?),
        b"YNAM" => recipe.pickup_sounds.push(raw_id(name, record_offset, sub)?),
        b"ZNAM" => recipe
            .putdown_sounds
            .push(raw_id(name, record_offset, sub)?),
        b"FVPA" => {
            for (index, entry) in fixed_entries::<8>(name, record_offset, "COBJ", &sub)?
                .into_iter()
                .enumerate()
            {
                let payload_offset = sub.payload_offset + index * 8;
                recipe.components.push(Component {
                    component: RawFormId {
                        subrecord: "FVPA".into(),
                        raw: u32::from_le_bytes(entry[..4].try_into().expect("four bytes")),
                        payload_offset,
                    },
                    count: u32::from_le_bytes(entry[4..].try_into().expect("four bytes")),
                });
            }
        }
        b"FNAM" => {
            for (index, entry) in fixed_entries::<4>(name, record_offset, "COBJ", &sub)?
                .into_iter()
                .enumerate()
            {
                recipe.categories.push(RawFormId {
                    subrecord: "FNAM".into(),
                    raw: u32::from_le_bytes(entry),
                    payload_offset: sub.payload_offset + index * 4,
                });
            }
        }
        b"INTV" => {
            if !sub.data.len().is_multiple_of(2) {
                return Err(source_offset(
                    name,
                    record_offset,
                    sub.payload_offset,
                    "INTV payload is not a sequence of 16-bit fields",
                ));
            }
            let mut position = 0;
            while position < sub.data.len() {
                let item_offset = position;
                let count = u16::from_le_bytes(
                    sub.data[position..position + 2]
                        .try_into()
                        .expect("two remaining bytes"),
                );
                position += 2;
                let priority = if position < sub.data.len() {
                    let value = u16::from_le_bytes(
                        sub.data[position..position + 2]
                            .try_into()
                            .expect("even INTV payload has two remaining bytes"),
                    );
                    position += 2;
                    Some(value)
                } else {
                    None
                };
                recipe.created_object_counts.push(CreatedObjectCount {
                    count,
                    priority,
                    payload_offset: sub.payload_offset + item_offset,
                });
            }
        }
        b"CTDA" => {
            recipe
                .decoded_conditions
                .push(condition::parse_ctda(name, record_offset, &sub)?);
            recipe.condition_subrecords.push(opaque(sub));
        }
        b"CIS1" | b"CIS2" => recipe.condition_subrecords.push(opaque(sub)),
        _ => recipe.opaque_subrecords.push(opaque(sub)),
    }
    Ok(())
}

fn duplicate_diagnostics(recipe: &mut Recipe) {
    let counts = [
        ("EDID", recipe.editor_ids.len()),
        ("CNAM", recipe.created_objects.len()),
        ("BNAM", recipe.workbench_keywords.len()),
        ("ANAM", recipe.menu_art_objects.len()),
        ("YNAM", recipe.pickup_sounds.len()),
        ("ZNAM", recipe.putdown_sounds.len()),
    ];
    recipe
        .diagnostics
        .extend(
            counts
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .map(|(kind, count)| {
                    format!("repeated schema-singleton {kind}: {count} occurrences retained")
                }),
        );
}

pub fn parse_record(record: &Record, plugin_name: &str, plugin_sha256: &str) -> Result<Recipe> {
    if record.header.kind != *b"COBJ" {
        return Err(Error::Unsupported(
            "workshop parser accepts COBJ records only".into(),
        ));
    }
    let mut recipe = Recipe {
        plugin: plugin_name.into(),
        plugin_sha256: plugin_sha256.into(),
        record_offset: record.header.offset,
        record_version: record.header.version,
        record_flags: record.header.flags,
        form_id_raw: record.header.form_id,
        editor_ids: Vec::new(),
        created_objects: Vec::new(),
        workbench_keywords: Vec::new(),
        menu_art_objects: Vec::new(),
        pickup_sounds: Vec::new(),
        putdown_sounds: Vec::new(),
        components: Vec::new(),
        categories: Vec::new(),
        created_object_counts: Vec::new(),
        condition_subrecords: Vec::new(),
        decoded_conditions: Vec::new(),
        opaque_subrecords: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut callback_error = None;
    let walk = plugin::visit_subrecords(record, plugin_name, |sub| {
        if let Err(error) = add_subrecord(&mut recipe, plugin_name, sub) {
            callback_error = Some(error);
            return Err(fallout_data::Error::Unsupported(
                "COBJ subrecord shape does not match the admitted FO4 schema".into(),
            ));
        }
        Ok(())
    });
    if let Some(error) = callback_error {
        return Err(error);
    }
    walk?;
    duplicate_diagnostics(&mut recipe);
    Ok(recipe)
}

/// Fallout 4 CMPO fields used by workshop scrap data. Links and values remain raw.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ComponentScrapRecord {
    pub plugin: String,
    pub plugin_sha256: String,
    pub record_offset: u64,
    pub record_version: u16,
    pub record_flags: u32,
    pub form_id_raw: u32,
    pub editor_ids: Vec<OpaqueSubrecord>,
    pub auto_calc_values: Vec<u32>,
    pub crafting_sounds: Vec<RawFormId>,
    pub scrap_items: Vec<RawFormId>,
    pub scrap_scalars: Vec<RawFormId>,
    pub opaque_subrecords: Vec<OpaqueSubrecord>,
    pub diagnostics: Vec<String>,
}

/// One raw MISC CVPA component/count entry, with its source coordinate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScrapComponentEntry {
    pub component: RawFormId,
    pub count: u32,
}

/// One raw byte from MISC CDIX, whose display semantics are not interpreted.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ComponentDisplayIndex {
    pub value: u8,
    pub payload_offset: usize,
}

/// Fallout 4 MISC CVPA/CDIX structure. Component links remain raw.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MiscScrapBreakdown {
    pub plugin: String,
    pub plugin_sha256: String,
    pub record_offset: u64,
    pub record_version: u16,
    pub record_flags: u32,
    pub form_id_raw: u32,
    pub editor_ids: Vec<OpaqueSubrecord>,
    pub preview_transforms: Vec<RawFormId>,
    pub pickup_sounds: Vec<RawFormId>,
    pub putdown_sounds: Vec<RawFormId>,
    pub keywords: Vec<RawFormId>,
    pub featured_item_messages: Vec<RawFormId>,
    pub components: Vec<ScrapComponentEntry>,
    pub component_display_indices: Vec<ComponentDisplayIndex>,
    pub opaque_subrecords: Vec<OpaqueSubrecord>,
    pub diagnostics: Vec<String>,
}

fn verify_kind(record: &Record, expected: &[u8; 4], description: &str) -> Result<()> {
    if record.header.kind != *expected {
        return Err(Error::Unsupported(format!(
            "{description} parser received {} record",
            plugin::signature(record.header.kind)
        )));
    }
    Ok(())
}

fn visit_record_subrecords(
    record: &Record,
    plugin_name: &str,
    context: &str,
    mut consume: impl FnMut(Subrecord<'_>) -> Result<()>,
) -> Result<()> {
    let mut callback_error = None;
    let walk = plugin::visit_subrecords(record, plugin_name, |sub| {
        if let Err(error) = consume(sub) {
            callback_error = Some(error);
            return Err(fallout_data::Error::Unsupported(format!(
                "{context} subrecord shape does not match the admitted FO4 schema"
            )));
        }
        Ok(())
    });
    if let Some(error) = callback_error {
        return Err(error);
    }
    walk?;
    Ok(())
}

fn duplicate_field_diagnostics(diagnostics: &mut Vec<String>, fields: &[(&str, usize)]) {
    diagnostics.extend(
        fields
            .iter()
            .filter(|(_, count)| *count > 1)
            .map(|(kind, count)| {
                format!("repeated schema-singleton {kind}: {count} occurrences retained")
            }),
    );
}

pub fn parse_component_scrap_record(
    record: &Record,
    plugin_name: &str,
    plugin_sha256: &str,
) -> Result<ComponentScrapRecord> {
    verify_kind(record, b"CMPO", "component scrap")?;
    let mut parsed = ComponentScrapRecord {
        plugin: plugin_name.into(),
        plugin_sha256: plugin_sha256.into(),
        record_offset: record.header.offset,
        record_version: record.header.version,
        record_flags: record.header.flags,
        form_id_raw: record.header.form_id,
        editor_ids: Vec::new(),
        auto_calc_values: Vec::new(),
        crafting_sounds: Vec::new(),
        scrap_items: Vec::new(),
        scrap_scalars: Vec::new(),
        opaque_subrecords: Vec::new(),
        diagnostics: Vec::new(),
    };
    visit_record_subrecords(record, plugin_name, "CMPO", |sub| {
        match &sub.kind {
            b"EDID" => parsed.editor_ids.push(opaque(sub)),
            b"DATA" => {
                let bytes: [u8; 4] = sub.data.try_into().map_err(|_| {
                    source_offset_for(
                        plugin_name,
                        record.header.offset,
                        "CMPO",
                        sub.payload_offset,
                        "DATA",
                    )
                })?;
                parsed.auto_calc_values.push(u32::from_le_bytes(bytes));
            }
            b"CUSD" => parsed.crafting_sounds.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "CMPO",
                sub,
            )?),
            b"MNAM" => {
                parsed
                    .scrap_items
                    .push(raw_id_for(plugin_name, record.header.offset, "CMPO", sub)?)
            }
            b"GNAM" => parsed.scrap_scalars.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "CMPO",
                sub,
            )?),
            _ => parsed.opaque_subrecords.push(opaque(sub)),
        }
        Ok(())
    })?;
    duplicate_field_diagnostics(
        &mut parsed.diagnostics,
        &[
            ("EDID", parsed.editor_ids.len()),
            ("DATA", parsed.auto_calc_values.len()),
            ("CUSD", parsed.crafting_sounds.len()),
            ("MNAM", parsed.scrap_items.len()),
            ("GNAM", parsed.scrap_scalars.len()),
        ],
    );
    Ok(parsed)
}

pub fn parse_misc_scrap_breakdown(
    record: &Record,
    plugin_name: &str,
    plugin_sha256: &str,
) -> Result<MiscScrapBreakdown> {
    verify_kind(record, b"MISC", "misc scrap breakdown")?;
    let mut parsed = MiscScrapBreakdown {
        plugin: plugin_name.into(),
        plugin_sha256: plugin_sha256.into(),
        record_offset: record.header.offset,
        record_version: record.header.version,
        record_flags: record.header.flags,
        form_id_raw: record.header.form_id,
        editor_ids: Vec::new(),
        preview_transforms: Vec::new(),
        pickup_sounds: Vec::new(),
        putdown_sounds: Vec::new(),
        keywords: Vec::new(),
        featured_item_messages: Vec::new(),
        components: Vec::new(),
        component_display_indices: Vec::new(),
        opaque_subrecords: Vec::new(),
        diagnostics: Vec::new(),
    };
    visit_record_subrecords(record, plugin_name, "MISC", |sub| {
        match &sub.kind {
            b"EDID" => parsed.editor_ids.push(opaque(sub)),
            b"PTRN" => parsed.preview_transforms.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "MISC",
                sub,
            )?),
            b"YNAM" => parsed.pickup_sounds.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "MISC",
                sub,
            )?),
            b"ZNAM" => parsed.putdown_sounds.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "MISC",
                sub,
            )?),
            b"KWDA" => {
                for (index, entry) in
                    fixed_entries::<4>(plugin_name, record.header.offset, "MISC", &sub)?
                        .into_iter()
                        .enumerate()
                {
                    parsed.keywords.push(RawFormId {
                        subrecord: "KWDA".into(),
                        raw: u32::from_le_bytes(entry),
                        payload_offset: sub.payload_offset + index * 4,
                    });
                }
            }
            b"FIMD" => parsed.featured_item_messages.push(raw_id_for(
                plugin_name,
                record.header.offset,
                "MISC",
                sub,
            )?),
            b"CVPA" => {
                if !sub.data.len().is_multiple_of(8) {
                    return Err(source_offset_for(
                        plugin_name,
                        record.header.offset,
                        "MISC",
                        sub.payload_offset,
                        "CVPA payload is not a sequence of eight-byte component/count entries",
                    ));
                }
                for (index, entry) in sub.data.as_chunks::<8>().0.iter().enumerate() {
                    let payload_offset = sub.payload_offset + index * 8;
                    parsed.components.push(ScrapComponentEntry {
                        component: RawFormId {
                            subrecord: "CVPA".into(),
                            raw: u32::from_le_bytes(entry[..4].try_into().expect("four bytes")),
                            payload_offset,
                        },
                        count: u32::from_le_bytes(entry[4..].try_into().expect("four bytes")),
                    });
                }
            }
            b"CDIX" => {
                parsed
                    .component_display_indices
                    .extend(sub.data.iter().enumerate().map(|(index, value)| {
                        ComponentDisplayIndex {
                            value: *value,
                            payload_offset: sub.payload_offset + index,
                        }
                    }))
            }
            _ => parsed.opaque_subrecords.push(opaque(sub)),
        }
        Ok(())
    })?;
    duplicate_field_diagnostics(
        &mut parsed.diagnostics,
        &[("EDID", parsed.editor_ids.len())],
    );
    Ok(parsed)
}

/// Fallout 4 GLOB fields with discriminator and value bytes retained exactly.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GlobalRawValueRecord {
    pub plugin: String,
    pub plugin_sha256: String,
    pub record_offset: u64,
    pub record_version: u16,
    pub record_flags: u32,
    pub form_id_raw: u32,
    pub editor_ids: Vec<OpaqueSubrecord>,
    pub type_char_subrecords: Vec<OpaqueSubrecord>,
    pub value_subrecords: Vec<OpaqueSubrecord>,
    pub opaque_subrecords: Vec<OpaqueSubrecord>,
    pub diagnostics: Vec<String>,
}

pub fn parse_global_raw_value_record(
    record: &Record,
    plugin_name: &str,
    plugin_sha256: &str,
) -> Result<GlobalRawValueRecord> {
    verify_kind(record, b"GLOB", "global raw value")?;
    let mut parsed = GlobalRawValueRecord {
        plugin: plugin_name.into(),
        plugin_sha256: plugin_sha256.into(),
        record_offset: record.header.offset,
        record_version: record.header.version,
        record_flags: record.header.flags,
        form_id_raw: record.header.form_id,
        editor_ids: Vec::new(),
        type_char_subrecords: Vec::new(),
        value_subrecords: Vec::new(),
        opaque_subrecords: Vec::new(),
        diagnostics: Vec::new(),
    };
    visit_record_subrecords(record, plugin_name, "GLOB", |sub| {
        match &sub.kind {
            b"EDID" => parsed.editor_ids.push(opaque(sub)),
            b"FNAM" => {
                if sub.data.len() != 1 {
                    return Err(source_offset_for(
                        plugin_name,
                        record.header.offset,
                        "GLOB",
                        sub.payload_offset,
                        "FNAM type-character field must contain one byte",
                    ));
                }
                parsed.type_char_subrecords.push(opaque(sub));
            }
            b"FLTV" => parsed.value_subrecords.push(opaque(sub)),
            _ => parsed.opaque_subrecords.push(opaque(sub)),
        }
        Ok(())
    })?;
    duplicate_field_diagnostics(
        &mut parsed.diagnostics,
        &[
            ("EDID", parsed.editor_ids.len()),
            ("FNAM", parsed.type_char_subrecords.len()),
            ("FLTV", parsed.value_subrecords.len()),
        ],
    );
    if parsed.type_char_subrecords.len() == 1 {
        let kind = u8::from_str_radix(&parsed.type_char_subrecords[0].bytes_hex, 16).ok();
        match kind {
            Some(b'f' | b'l' | b's' | b'b') => {
                for value in &parsed.value_subrecords {
                    if value.bytes_hex.len() != 8 {
                        return Err(source_offset_for(
                            plugin_name,
                            record.header.offset,
                            "GLOB",
                            value.payload_offset,
                            "FLTV value for a known Fallout 4 GLOB type must contain four bytes",
                        ));
                    }
                }
            }
            Some(raw) => parsed.diagnostics.push(format!(
                "unknown GLOB type-character byte 0x{raw:02X}; FLTV values remain opaque"
            )),
            None => parsed
                .diagnostics
                .push("GLOB type-character evidence is empty; FLTV values remain opaque".into()),
        }
    }
    Ok(parsed)
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub recipes: u64,
    pub record_versions: BTreeMap<u16, u64>,
    pub components: u64,
    pub categories: u64,
    pub created_object_counts: u64,
    pub raw_condition_subrecords: u64,
    pub decoded_ctda_records: u64,
    pub opaque_subrecords: u64,
    pub recipes_with_workbench_keyword: u64,
    pub recipes_with_created_object: u64,
    pub diagnostics: u64,
    pub uninterpreted_subrecord_kinds: BTreeMap<String, u64>,
}
impl Counts {
    pub fn add(&mut self, recipe: &Recipe) {
        self.recipes += 1;
        *self
            .record_versions
            .entry(recipe.record_version)
            .or_default() += 1;
        self.components += recipe.components.len() as u64;
        self.categories += recipe.categories.len() as u64;
        self.created_object_counts += recipe.created_object_counts.len() as u64;
        self.raw_condition_subrecords += recipe.condition_subrecords.len() as u64;
        self.decoded_ctda_records += recipe.decoded_conditions.len() as u64;
        self.opaque_subrecords += recipe.opaque_subrecords.len() as u64;
        self.recipes_with_workbench_keyword += u64::from(!recipe.workbench_keywords.is_empty());
        self.recipes_with_created_object += u64::from(!recipe.created_objects.is_empty());
        self.diagnostics += recipe.diagnostics.len() as u64;
        for subrecord in &recipe.opaque_subrecords {
            *self
                .uninterpreted_subrecord_kinds
                .entry(subrecord.kind.clone())
                .or_default() += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout_data::plugin::{self, Event, RecordHeader};
    use std::io::Cursor;

    fn sub(kind: &[u8; 4], data: &[u8], payload: &mut Vec<u8>) {
        payload.extend_from_slice(kind);
        payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
        payload.extend_from_slice(data);
    }

    fn record(payload: Vec<u8>) -> Record {
        record_kind(*b"COBJ", payload)
    }

    fn record_kind(kind: [u8; 4], payload: Vec<u8>) -> Record {
        Record {
            header: RecordHeader {
                kind,
                offset: 0x1234,
                stored_size: payload.len() as u32,
                flags: 0,
                form_id: 0x0100_1234,
                revision: [0; 4],
                version: 131,
                trailing_bytes: [0; 2],
            },
            payload,
            integrity_issue: None,
        }
    }

    #[test]
    fn mutagen_writer_condition_and_string_companions_remain_ordered_and_raw() {
        let bytes = include_bytes!("../tests/fixtures/condition/mutagen-workshop-v1.esp");
        let mut reader = Cursor::new(bytes.as_slice());
        let mut recipes = Vec::new();
        plugin::visit(
            &mut reader,
            bytes.len() as u64,
            "ConditionWriterFixture.esp",
            plugin::Limits::default(),
            |event| {
                let Event::Record(record) = event else {
                    return Ok(());
                };
                if record.header.kind == *b"COBJ" {
                    recipes.push(
                        parse_record(
                            record,
                            "ConditionWriterFixture.esp",
                            "mutagen-condition-writer-fixture",
                        )
                        .unwrap(),
                    );
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(recipes.len(), 1);
        let recipe = &recipes[0];
        let kinds: Vec<_> = recipe
            .condition_subrecords
            .iter()
            .map(|subrecord| subrecord.kind.as_str())
            .collect();
        assert_eq!(kinds, ["CTDA", "CTDA", "CTDA", "CIS1", "CIS2"]);
        assert_eq!(recipe.decoded_conditions.len(), 3);
        assert_eq!(recipe.decoded_conditions[0].function_index, 59);
        assert_eq!(recipe.decoded_conditions[1].function_index, 59);
        assert_eq!(recipe.decoded_conditions[2].function_index, 660);
        assert_eq!(recipe.decoded_conditions[2].parameter_one_raw, 0);
        assert_eq!(recipe.decoded_conditions[2].parameter_two_raw, 0);
        assert_eq!(
            recipe.decoded_conditions[2]
                .function_parameter_hint
                .parameter_one_category,
            "string"
        );
        assert_eq!(
            recipe.decoded_conditions[2]
                .function_parameter_hint
                .parameter_two_category,
            "string"
        );
        assert_eq!(
            recipe.condition_subrecords[3].bytes_hex,
            "4669787475726553637269707400"
        );
        assert_eq!(
            recipe.condition_subrecords[4].bytes_hex,
            "466978747572655661726961626c6500"
        );
        assert!(recipe.diagnostics.is_empty());
    }

    #[test]
    fn decodes_recipe_lists_and_preserves_raw_condition_bytes() {
        let mut bytes = Vec::new();
        sub(b"CNAM", &0x0100_1111u32.to_le_bytes(), &mut bytes);
        sub(b"BNAM", &0x0100_2222u32.to_le_bytes(), &mut bytes);
        let mut components = Vec::new();
        components.extend_from_slice(&0x0100_3333u32.to_le_bytes());
        components.extend_from_slice(&5u32.to_le_bytes());
        components.extend_from_slice(&0x0100_4444u32.to_le_bytes());
        components.extend_from_slice(&2u32.to_le_bytes());
        sub(b"FVPA", &components, &mut bytes);
        let mut categories = Vec::new();
        categories.extend_from_slice(&0x0100_5555u32.to_le_bytes());
        categories.extend_from_slice(&0x0100_6666u32.to_le_bytes());
        sub(b"FNAM", &categories, &mut bytes);
        let mut counts = Vec::new();
        counts.extend_from_slice(&3u16.to_le_bytes());
        counts.extend_from_slice(&7u16.to_le_bytes());
        sub(b"INTV", &counts, &mut bytes);
        let mut condition = [0u8; condition::CTDA_SIZE];
        condition[8..10].copy_from_slice(&0x0504u16.to_le_bytes());
        sub(b"CTDA", &condition, &mut bytes);
        sub(b"CIS1", b"GetLevel\0", &mut bytes);

        let parsed = parse_record(&record(bytes), "Workshop.esm", "abc").unwrap();
        assert_eq!(parsed.form_id_raw, 0x0100_1234);
        assert_eq!(parsed.created_objects[0].raw, 0x0100_1111);
        assert_eq!(parsed.workbench_keywords[0].raw, 0x0100_2222);
        assert_eq!(parsed.components.len(), 2);
        assert_eq!(parsed.components[0].count, 5);
        assert_eq!(parsed.categories.len(), 2);
        assert_eq!(parsed.created_object_counts[0].count, 3);
        assert_eq!(parsed.created_object_counts[0].priority, Some(7));
        assert_eq!(
            parsed.condition_subrecords[0].bytes_hex.len(),
            condition::CTDA_SIZE * 2
        );
        assert_eq!(parsed.condition_subrecords[1].kind, "CIS1");
        assert_eq!(parsed.decoded_conditions.len(), 1);
        assert_eq!(parsed.decoded_conditions[0].function_index, 0x0504);
    }

    #[test]
    fn rejects_malformed_fixed_width_recipe_fields_with_source_context() {
        let mut bytes = Vec::new();
        sub(b"FVPA", &[0; 7], &mut bytes);
        let error = parse_record(&record(bytes), "Workshop.esm", "abc").unwrap_err();
        assert!(error.to_string().contains("Workshop.esm"));
        assert!(error.to_string().contains("FVPA"));
        assert!(error.to_string().contains("0x1234"));
    }

    #[test]
    fn retains_duplicate_singletons_and_reports_them_instead_of_selecting_one() {
        let mut bytes = Vec::new();
        sub(b"CNAM", &1u32.to_le_bytes(), &mut bytes);
        sub(b"CNAM", &2u32.to_le_bytes(), &mut bytes);
        let parsed = parse_record(&record(bytes), "Workshop.esm", "abc").unwrap();
        assert_eq!(
            parsed
                .created_objects
                .iter()
                .map(|field| field.raw)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.contains("CNAM: 2 occurrences retained"))
        );
    }

    #[test]
    fn record_version_is_retained_without_selecting_a_dialect() {
        let mut record = record(Vec::new());
        record.header.version = 118;
        let parsed = parse_record(&record, "Workshop.esm", "abc").unwrap();
        assert_eq!(parsed.record_version, 118);
    }

    #[test]
    fn intv_short_structs_preserve_missing_priority_and_mixed_list_entries() {
        let mut bytes = Vec::new();
        let mut entries = Vec::new();
        entries.extend_from_slice(&5u16.to_le_bytes());
        entries.extend_from_slice(&9u16.to_le_bytes());
        entries.extend_from_slice(&4u16.to_le_bytes());
        sub(b"INTV", &entries, &mut bytes);
        let parsed = parse_record(&record(bytes), "Workshop.esm", "abc").unwrap();
        assert_eq!(parsed.created_object_counts.len(), 2);
        assert_eq!(parsed.created_object_counts[0].count, 5);
        assert_eq!(parsed.created_object_counts[0].priority, Some(9));
        assert_eq!(parsed.created_object_counts[1].count, 4);
        assert_eq!(parsed.created_object_counts[1].priority, None);
    }

    #[test]
    fn intv_odd_byte_tails_fail_with_source_context() {
        let mut bytes = Vec::new();
        sub(b"INTV", &[1, 0, 0], &mut bytes);
        let error = parse_record(&record(bytes), "Workshop.esm", "abc").unwrap_err();
        assert!(error.to_string().contains("Workshop.esm"));
        assert!(error.to_string().contains("INTV"));
    }

    #[test]
    fn decodes_component_scrap_fields_as_raw_values() {
        let mut bytes = Vec::new();
        sub(b"EDID", b"c_Bone\0", &mut bytes);
        sub(b"CUSD", &0x0100_1122u32.to_le_bytes(), &mut bytes);
        sub(b"DATA", &0x1020_3040u32.to_le_bytes(), &mut bytes);
        sub(b"MNAM", &0x0100_1234u32.to_le_bytes(), &mut bytes);
        sub(b"GNAM", &0x0100_5678u32.to_le_bytes(), &mut bytes);

        let parsed =
            parse_component_scrap_record(&record_kind(*b"CMPO", bytes), "Fallout4.esm", "abc")
                .unwrap();
        assert_eq!(parsed.form_id_raw, 0x0100_1234);
        assert_eq!(parsed.editor_ids[0].bytes_hex, "635f426f6e6500");
        assert_eq!(parsed.auto_calc_values, [0x1020_3040]);
        assert_eq!(parsed.crafting_sounds[0].raw, 0x0100_1122);
        assert_eq!(parsed.scrap_items[0].raw, 0x0100_1234);
        assert_eq!(parsed.scrap_scalars[0].raw, 0x0100_5678);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn rejects_malformed_component_scrap_fixed_fields_with_source_context() {
        for (kind, payload) in [(b"DATA", &[1, 2, 3][..]), (b"MNAM", &[1, 2, 3][..])] {
            let mut bytes = Vec::new();
            sub(kind, payload, &mut bytes);
            let error =
                parse_component_scrap_record(&record_kind(*b"CMPO", bytes), "Workshop.esm", "abc")
                    .unwrap_err();
            assert!(error.to_string().contains("Workshop.esm"));
            assert!(error.to_string().contains("CMPO"));
            assert!(error.to_string().contains("0x1234"));
        }
    }

    #[test]
    fn rejects_malformed_component_crafting_sound_form_link() {
        let mut bytes = Vec::new();
        sub(b"CUSD", &[1, 2, 3], &mut bytes);
        let error =
            parse_component_scrap_record(&record_kind(*b"CMPO", bytes), "Fallout4.esm", "abc")
                .unwrap_err();
        assert!(error.to_string().contains("Fallout4.esm"));
        assert!(error.to_string().contains("CUSD"));
    }

    #[test]
    fn decodes_misc_scrap_entries_and_keeps_display_indices_as_bytes() {
        let mut bytes = Vec::new();
        sub(b"EDID", b"c_ScrapWood\0", &mut bytes);
        sub(b"PTRN", &0x0100_0101u32.to_le_bytes(), &mut bytes);
        sub(b"YNAM", &0x0100_0202u32.to_le_bytes(), &mut bytes);
        sub(b"ZNAM", &0x0100_0303u32.to_le_bytes(), &mut bytes);
        let mut keywords = Vec::new();
        keywords.extend_from_slice(&0x0100_0404u32.to_le_bytes());
        keywords.extend_from_slice(&0x0100_0505u32.to_le_bytes());
        sub(b"KWDA", &keywords, &mut bytes);
        sub(b"FIMD", &0x0100_0606u32.to_le_bytes(), &mut bytes);
        let mut entries = Vec::new();
        entries.extend_from_slice(&0x0100_1111u32.to_le_bytes());
        entries.extend_from_slice(&3u32.to_le_bytes());
        entries.extend_from_slice(&0x0100_2222u32.to_le_bytes());
        entries.extend_from_slice(&7u32.to_le_bytes());
        sub(b"CVPA", &entries, &mut bytes);
        sub(b"CVPA", &[], &mut bytes);
        sub(b"CDIX", &[0, 2, 255], &mut bytes);

        let parsed =
            parse_misc_scrap_breakdown(&record_kind(*b"MISC", bytes), "Fallout4.esm", "abc")
                .unwrap();
        assert_eq!(parsed.components.len(), 2);
        assert_eq!(parsed.components[0].component.raw, 0x0100_1111);
        assert_eq!(parsed.components[0].count, 3);
        assert_eq!(parsed.components[1].component.raw, 0x0100_2222);
        assert_eq!(parsed.components[1].count, 7);
        assert_eq!(parsed.preview_transforms[0].raw, 0x0100_0101);
        assert_eq!(parsed.pickup_sounds[0].raw, 0x0100_0202);
        assert_eq!(parsed.putdown_sounds[0].raw, 0x0100_0303);
        assert_eq!(
            parsed
                .keywords
                .iter()
                .map(|row| row.raw)
                .collect::<Vec<_>>(),
            [0x0100_0404, 0x0100_0505,]
        );
        assert_eq!(parsed.featured_item_messages[0].raw, 0x0100_0606);
        assert_eq!(
            parsed
                .component_display_indices
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>(),
            [0, 2, 255]
        );
        assert_eq!(
            parsed
                .component_display_indices
                .iter()
                .map(|item| item.payload_offset)
                .collect::<Vec<_>>(),
            [100, 101, 102]
        );
    }

    #[test]
    fn rejects_truncated_misc_scrap_component_entry_with_source_context() {
        let mut bytes = Vec::new();
        sub(b"CVPA", &[0; 7], &mut bytes);
        let error =
            parse_misc_scrap_breakdown(&record_kind(*b"MISC", bytes), "Workshop.esm", "abc")
                .unwrap_err();
        assert!(error.to_string().contains("Workshop.esm"));
        assert!(error.to_string().contains("CVPA"));
        assert!(error.to_string().contains("0x1234"));
    }

    #[test]
    fn rejects_malformed_misc_form_link_widths_with_source_context() {
        for (kind, payload) in [
            (b"PTRN", &[1, 2, 3][..]),
            (b"KWDA", &[1, 2, 3][..]),
            (b"FIMD", &[1, 2, 3, 4, 5][..]),
        ] {
            let mut bytes = Vec::new();
            sub(kind, payload, &mut bytes);
            let error =
                parse_misc_scrap_breakdown(&record_kind(*b"MISC", bytes), "Fallout4.esm", "abc")
                    .unwrap_err();
            assert!(error.to_string().contains("Fallout4.esm"));
            assert!(error.to_string().contains("MISC"));
            assert!(error.to_string().contains("0x1234"));
        }
    }

    #[test]
    fn preserves_global_type_and_value_bytes_without_numeric_conversion() {
        let mut bytes = Vec::new();
        sub(b"EDID", b"ModScrapScalar_Full\0", &mut bytes);
        sub(b"FNAM", b"f", &mut bytes);
        sub(b"FLTV", &[0x00, 0x00, 0x80, 0x3f], &mut bytes);
        let parsed =
            parse_global_raw_value_record(&record_kind(*b"GLOB", bytes), "Fallout4.esm", "abc")
                .unwrap();
        assert_eq!(parsed.type_char_subrecords[0].bytes_hex, "66");
        assert_eq!(parsed.value_subrecords[0].bytes_hex, "0000803f");
        assert_eq!(
            parsed.value_subrecords[0].sha256,
            census::sha256(&[0, 0, 0x80, 0x3f])
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn keeps_unknown_global_types_opaque_with_a_diagnostic() {
        let mut bytes = Vec::new();
        sub(b"FNAM", &[0xFF], &mut bytes);
        sub(b"FLTV", &[1, 2, 3], &mut bytes);
        let parsed =
            parse_global_raw_value_record(&record_kind(*b"GLOB", bytes), "Workshop.esm", "abc")
                .unwrap();
        assert_eq!(parsed.type_char_subrecords[0].bytes_hex, "ff");
        assert_eq!(parsed.value_subrecords[0].bytes_hex, "010203");
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|row| row.contains("unknown GLOB type"))
        );
    }

    #[test]
    fn rejects_bad_global_type_and_known_value_widths_with_source_context() {
        let mut bad_type = Vec::new();
        sub(b"FNAM", b"float", &mut bad_type);
        let error =
            parse_global_raw_value_record(&record_kind(*b"GLOB", bad_type), "Workshop.esm", "abc")
                .unwrap_err();
        assert!(error.to_string().contains("FNAM"));
        assert!(error.to_string().contains("0x1234"));

        let mut bad_value = Vec::new();
        sub(b"FNAM", b"b", &mut bad_value);
        sub(b"FLTV", &[0, 1], &mut bad_value);
        let error =
            parse_global_raw_value_record(&record_kind(*b"GLOB", bad_value), "Workshop.esm", "abc")
                .unwrap_err();
        assert!(error.to_string().contains("FLTV"));
        assert!(error.to_string().contains("0x1234"));
    }
}
