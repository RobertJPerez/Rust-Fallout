//! Fallout 4 CTDA fixed-layout fields. No functions or conditions are evaluated.
use crate::{Result, bad, census};
use fallout_data::plugin::Subrecord;
use serde::Serialize;
mod condition_schema;

pub const CTDA_SIZE: usize = 32;
const COMPARE_OPERATOR_MASK: u8 = 0xE0;
const CONDITION_FLAGS_MASK: u8 = 0x1F;
const USE_GLOBAL_FLAG: u8 = 0x04;

/// Fixed-width CTDA fields, with parameter and FormID-like slots kept raw.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RawCondition {
    pub payload_offset: usize,
    pub payload_sha256: String,
    pub payload_bytes_hex: String,
    pub packed_flags_and_operator: u8,
    pub flag_bits: u8,
    pub compare_operator_bits: u8,
    pub comparison_value_is_global_form_id: bool,
    pub comparison_value_raw: u32,
    pub unknown1: [u8; 3],
    pub function_index: u16,
    pub unknown2: u16,
    pub parameter_one_raw: u32,
    pub parameter_two_raw: u32,
    pub function_parameter_hint: FunctionParameterHint,
    pub run_on_raw: u32,
    pub reference_raw: u32,
    pub unknown3_raw: i32,
}

/// Pinned-schema parameter metadata. The first two slots map to CTDA words; the third is schema metadata only.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FunctionParameterHint {
    pub function_name: Option<&'static str>,
    pub mapping_status: &'static str,
    pub parameter_one_type: &'static str,
    pub parameter_one_category: &'static str,
    pub parameter_two_type: &'static str,
    pub parameter_two_category: &'static str,
    pub parameter_three_type: &'static str,
    pub parameter_three_category: &'static str,
}

/// Every named Fallout 4 condition function and its pinned parameter metadata.
pub fn function_parameter_schemas() -> impl Iterator<Item = (u16, FunctionParameterHint)> {
    condition_schema::function_schema_rows()
}

fn u16_at(data: &[u8], start: usize) -> u16 {
    u16::from_le_bytes([data[start], data[start + 1]])
}

fn u32_at(data: &[u8], start: usize) -> u32 {
    u32::from_le_bytes([
        data[start],
        data[start + 1],
        data[start + 2],
        data[start + 3],
    ])
}

pub fn parse_ctda(
    plugin_name: &str,
    record_offset: u64,
    sub: &Subrecord<'_>,
) -> Result<RawCondition> {
    if sub.kind != *b"CTDA" {
        return Err(crate::Error::Unsupported(
            "condition parser accepts CTDA subrecords only".into(),
        ));
    }
    if sub.data.len() != CTDA_SIZE {
        let offset = usize::try_from(record_offset).unwrap_or(usize::MAX);
        return Err(bad(
            plugin_name,
            offset,
            format!(
                "CTDA at decoded +0x{:X} has {} bytes; the admitted fixed structure is {CTDA_SIZE}",
                sub.payload_offset,
                sub.data.len()
            ),
        ));
    }
    let packed = sub.data[0];
    let flag_bits = packed & CONDITION_FLAGS_MASK;
    Ok(RawCondition {
        payload_offset: sub.payload_offset,
        payload_sha256: census::sha256(sub.data),
        payload_bytes_hex: sub.data.iter().map(|byte| format!("{byte:02x}")).collect(),
        packed_flags_and_operator: packed,
        flag_bits,
        compare_operator_bits: (packed & COMPARE_OPERATOR_MASK) >> 5,
        comparison_value_is_global_form_id: flag_bits & USE_GLOBAL_FLAG != 0,
        comparison_value_raw: u32_at(sub.data, 4),
        unknown1: [sub.data[1], sub.data[2], sub.data[3]],
        function_index: u16_at(sub.data, 8),
        unknown2: u16_at(sub.data, 10),
        parameter_one_raw: u32_at(sub.data, 12),
        parameter_two_raw: u32_at(sub.data, 16),
        function_parameter_hint: condition_schema::function_parameter_hint(u16_at(sub.data, 8)),
        run_on_raw: u32_at(sub.data, 20),
        reference_raw: u32_at(sub.data, 24),
        unknown3_raw: i32::from_le_bytes(sub.data[28..32].try_into().expect("validated length")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout_data::plugin::Subrecord;

    fn ctda(data: &[u8], offset: usize) -> Subrecord<'_> {
        Subrecord {
            kind: *b"CTDA",
            data,
            payload_offset: offset,
        }
    }

    #[test]
    fn decodes_fixed_ctda_fields_without_resolving_values_or_evaluating_flags() {
        let mut bytes = [0u8; CTDA_SIZE];
        bytes[0] = 0xA5;
        bytes[1..4].copy_from_slice(&[0x12, 0x34, 0x56]);
        bytes[4..8].copy_from_slice(&0x7FC0_1234u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&4672u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&0xBEEFu16.to_le_bytes());
        bytes[12..16].copy_from_slice(&0xFE00_0123u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&0xFF00_0456u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&7u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&0x0100_9876u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&(-1i32).to_le_bytes());

        let sub = ctda(&bytes, 0x80);
        let parsed = parse_ctda("Workshop.esm", 0x1234, &sub).unwrap();
        assert_eq!(parsed.payload_offset, 0x80);
        assert_eq!(parsed.packed_flags_and_operator, 0xA5);
        assert_eq!(parsed.flag_bits, 0x05);
        assert_eq!(parsed.compare_operator_bits, 5);
        assert!(parsed.comparison_value_is_global_form_id);
        assert_eq!(parsed.comparison_value_raw, 0x7FC0_1234);
        assert_eq!(parsed.unknown1, [0x12, 0x34, 0x56]);
        assert_eq!(parsed.function_index, 4672);
        assert_eq!(parsed.unknown2, 0xBEEF);
        assert_eq!(parsed.parameter_one_raw, 0xFE00_0123);
        assert_eq!(parsed.parameter_two_raw, 0xFF00_0456);
        assert_eq!(parsed.run_on_raw, 7);
        assert_eq!(parsed.reference_raw, 0x0100_9876);
        assert_eq!(parsed.unknown3_raw, -1);
        assert_eq!(parsed.payload_bytes_hex.len(), CTDA_SIZE * 2);
    }

    #[test]
    fn decodes_mutagen_writer_authored_ctda_round_trip_fixture() {
        let bytes = include_bytes!("../tests/fixtures/condition/mutagen-ctda-v1.bin");
        let parsed = parse_ctda("ConditionWriterFixture.esp", 0, &ctda(bytes, 0x40)).unwrap();

        assert_eq!(
            parsed.payload_sha256,
            "8ad926f9a195cf1fc189a6190c81a93236091fb29c4d4e3d4639f3961810e4ca"
        );
        assert_eq!(parsed.packed_flags_and_operator, 0x63);
        assert_eq!(parsed.flag_bits, 0x03);
        assert_eq!(parsed.compare_operator_bits, 3);
        assert!(!parsed.comparison_value_is_global_form_id);
        assert_eq!(parsed.comparison_value_raw, 0x3FA0_0000);
        assert_eq!(parsed.unknown1, [0xA1, 0xB2, 0xC3]);
        assert_eq!(parsed.function_index, 59);
        assert_eq!(parsed.unknown2, 0x5A7C);
        assert_eq!(parsed.parameter_one_raw, 0x0000_0800);
        assert_eq!(parsed.parameter_two_raw, 0x1122_3344);
        assert_eq!(parsed.run_on_raw, 4);
        assert_eq!(parsed.reference_raw, 0x0000_0800);
        assert_eq!(parsed.unknown3_raw, -123);
        assert_eq!(
            parsed.function_parameter_hint.function_name,
            Some("GetStageDone")
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_one_category,
            "form"
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_two_type,
            "QuestStage"
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_two_category,
            "number"
        );
    }

    #[test]
    fn decodes_mutagen_writer_authored_global_comparison_ctda_fixture() {
        let bytes = include_bytes!("../tests/fixtures/condition/mutagen-global-ctda-v1.bin");
        let parsed = parse_ctda("ConditionWriterFixture.esp", 0, &ctda(bytes, 0x80)).unwrap();

        assert_eq!(
            parsed.payload_sha256,
            "8e1e560dcee289d02c91b3bf1b99f6b652790aa1a595fcf803c6b47b719c7efb"
        );
        assert_eq!(parsed.packed_flags_and_operator, 0xAD);
        assert_eq!(parsed.flag_bits, 0x0D);
        assert_eq!(parsed.compare_operator_bits, 5);
        assert!(parsed.comparison_value_is_global_form_id);
        assert_eq!(parsed.comparison_value_raw, 0x0000_0801);
        assert_eq!(parsed.unknown1, [0xD4, 0xE5, 0xF6]);
        assert_eq!(parsed.function_index, 59);
        assert_eq!(parsed.unknown2, 0xBEEF);
        assert_eq!(parsed.parameter_one_raw, 0x0000_0800);
        assert_eq!(parsed.parameter_two_raw, 9);
        assert_eq!(parsed.run_on_raw, 7);
        assert_eq!(parsed.reference_raw, 0x0000_0800);
        assert_eq!(parsed.unknown3_raw, 4567);
        assert_eq!(
            parsed.function_parameter_hint.function_name,
            Some("GetStageDone")
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_one_category,
            "form"
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_two_type,
            "QuestStage"
        );
        assert_eq!(
            parsed.function_parameter_hint.parameter_two_category,
            "number"
        );
    }

    #[test]
    fn decodes_mutagen_writer_authored_vmscript_string_parameter_ctda_fixture() {
        let bytes = include_bytes!("../tests/fixtures/condition/mutagen-vmscript-ctda-v1.bin");
        let parsed = parse_ctda("ConditionWriterFixture.esp", 0, &ctda(bytes, 0xC0)).unwrap();

        assert_eq!(
            parsed.payload_sha256,
            "e6f9e86d5cfd041341f4d1cd114b7118079fe3a70cf5baf9a408f82dd533347a"
        );
        assert_eq!(parsed.packed_flags_and_operator, 0x01);
        assert_eq!(parsed.flag_bits, 0x01);
        assert_eq!(parsed.compare_operator_bits, 0);
        assert_eq!(parsed.function_index, 660);
        assert_eq!(parsed.parameter_one_raw, 0);
        assert_eq!(parsed.parameter_two_raw, 0);
        assert_eq!(
            parsed.function_parameter_hint.function_name,
            Some("GetVMScriptVariable")
        );
        assert_eq!(parsed.function_parameter_hint.parameter_one_type, "String");
        assert_eq!(
            parsed.function_parameter_hint.parameter_one_category,
            "string"
        );
        assert_eq!(parsed.function_parameter_hint.parameter_two_type, "String");
        assert_eq!(
            parsed.function_parameter_hint.parameter_two_category,
            "string"
        );
    }

    #[test]
    fn comparison_value_encoding_bit_does_not_change_raw_value() {
        let mut bytes = [0u8; CTDA_SIZE];
        bytes[4..8].copy_from_slice(&0x3F80_0000u32.to_le_bytes());
        let sub = ctda(&bytes, 0);
        let float = parse_ctda("Workshop.esm", 0, &sub).unwrap();
        bytes[0] = USE_GLOBAL_FLAG;
        let sub = ctda(&bytes, 0);
        let global = parse_ctda("Workshop.esm", 0, &sub).unwrap();
        assert!(!float.comparison_value_is_global_form_id);
        assert!(global.comparison_value_is_global_form_id);
        assert_eq!(float.comparison_value_raw, global.comparison_value_raw);
    }

    #[test]
    fn observed_function_hints_tag_wire_categories_and_leave_defaulted_slots_unresolved() {
        let mut bytes = [0u8; CTDA_SIZE];
        bytes[8..10].copy_from_slice(&59u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x0100_1234u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&17u32.to_le_bytes());
        let sub = ctda(&bytes, 0);
        let stage_done = parse_ctda("Workshop.esm", 0, &sub).unwrap();
        assert_eq!(stage_done.parameter_one_raw, 0x0100_1234);
        assert_eq!(stage_done.parameter_two_raw, 17);
        assert_eq!(
            stage_done.function_parameter_hint.function_name,
            Some("GetStageDone")
        );
        assert_eq!(
            stage_done.function_parameter_hint.mapping_status,
            "explicit"
        );
        assert_eq!(
            stage_done.function_parameter_hint.parameter_one_category,
            "form"
        );
        assert_eq!(
            stage_done.function_parameter_hint.parameter_two_type,
            "QuestStage"
        );
        assert_eq!(
            stage_done.function_parameter_hint.parameter_two_category,
            "number"
        );

        bytes[8..10].copy_from_slice(&300u16.to_le_bytes());
        let sub = ctda(&bytes, 0);
        let interior = parse_ctda("Workshop.esm", 0, &sub).unwrap();
        assert_eq!(
            interior.function_parameter_hint.function_name,
            Some("IsInInterior")
        );
        assert_eq!(
            interior.function_parameter_hint.mapping_status,
            "function-enum-known-parameter-map-defaulted"
        );
        assert_eq!(
            interior.function_parameter_hint.parameter_one_category,
            "unresolved"
        );
        assert_eq!(interior.parameter_one_raw, 0x0100_1234);

        bytes[8..10].copy_from_slice(&9000u16.to_le_bytes());
        let sub = ctda(&bytes, 0);
        let unknown = parse_ctda("Workshop.esm", 0, &sub).unwrap();
        assert_eq!(unknown.function_parameter_hint.function_name, None);
        assert_eq!(
            unknown.function_parameter_hint.mapping_status,
            "unknown-function-index"
        );
        assert_eq!(unknown.parameter_two_raw, 17);
    }

    #[test]
    fn all_other_observed_function_ids_have_pinned_parameter_categories() {
        let expected = [
            (47, "GetItemCount", "ReferencableObject", "None", "none"),
            (58, "GetStage", "Quest", "None", "none"),
            (59, "GetStageDone", "Quest", "QuestStage", "number"),
            (67, "GetInCell", "Cell", "None", "none"),
            (71, "GetInFaction", "Faction", "None", "none"),
            (74, "GetGlobalValue", "Global", "None", "none"),
            (310, "GetInWorldspace", "Worldspace", "None", "none"),
            (359, "GetInCurrentLocation", "Location", "None", "none"),
            (448, "HasPerk", "Perk", "None", "none"),
            (543, "GetQuestCompleted", "Quest", "None", "none"),
            (562, "LocationHasKeyword", "Keyword", "None", "none"),
            (
                651,
                "GetKeywordDataForCurrentLocation",
                "Keyword",
                "None",
                "none",
            ),
            (
                691,
                "GetWorkshopObjectCount",
                "ReferencableObject",
                "None",
                "none",
            ),
            (749, "ModdedItemHasKeyword", "Keyword", "None", "none"),
        ];
        for (index, name, first_type, second_type, second_category) in expected {
            let hint = condition_schema::function_parameter_hint(index);
            assert_eq!(hint.function_name, Some(name));
            assert_eq!(hint.mapping_status, "explicit");
            assert_eq!(hint.parameter_one_type, first_type);
            assert_eq!(hint.parameter_one_category, "form");
            assert_eq!(hint.parameter_two_type, second_type);
            assert_eq!(hint.parameter_two_category, second_category);
        }
    }

    #[test]
    fn every_wrong_ctda_width_fails_with_source_context() {
        for length in (0..CTDA_SIZE).chain(CTDA_SIZE + 1..=CTDA_SIZE + 8) {
            let bytes = vec![0; length];
            let sub = ctda(&bytes, 0x50);
            let error = parse_ctda("Workshop.esm", 0x1234, &sub).unwrap_err();
            assert!(error.to_string().contains("Workshop.esm"));
            assert!(error.to_string().contains("0x1234"));
            assert!(error.to_string().contains("0x50"));
        }
    }
}
