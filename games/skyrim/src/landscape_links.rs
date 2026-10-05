//! Skyrim landscape, texture, cell-context and location-parent source links.
//!
//! This is a physical-source graph. It does not choose an override, consume a
//! runtime load order, decode terrain geometry/alpha, or interpret materials.
use crate::{Error, Result, asset_lookup, bad, plugin, trace};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self as framing, Group, SelectedEvent},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Cursor, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const RECORD_LIMIT: usize = 300_000;
const SUBRECORD_LIMIT: usize = 1_000_000;
const CELL_SUBRECORD_LIMIT: usize = 2_000_000;
const CELL_REGION_LINK_LIMIT: usize = 1_000_000;
const RAW_EVIDENCE_LIMIT: usize = 32;
const XWEM_PATH_LIMIT: usize = 4_096;
const FIELD_SHAPE_TAG_LIMIT: usize = 4_096;
const FIELD_SHAPE_LENGTH_LIMIT: usize = 4_096;

const RECORD_KINDS: [&[u8; 4]; 16] = [
    b"LAND", b"LTEX", b"TXST", b"MATT", b"GRAS", b"CELL", b"WRLD", b"REGN", b"CLMT", b"WATR",
    b"IMGS", b"ECZN", b"ASPC", b"MUSC", b"LGTM", b"LCTN",
];

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub plugin_files_scanned: u64,
    pub selected_records: u64,
    pub land_records: u64,
    pub land_records_with_group_path: u64,
    pub land_records_without_group_path: u64,
    pub land_group_path_depths: BTreeMap<usize, u64>,
    pub land_group_types: BTreeMap<String, u64>,
    pub ltex_records: u64,
    pub txst_records: u64,
    pub matt_records: u64,
    pub gras_records: u64,
    pub cell_records: u64,
    pub world_records: u64,
    pub regn_records: u64,
    pub location_records: u64,
    pub location_subrecords_scanned: u64,
    pub location_parent_form_links: u64,
    pub location_parent_links_with_scanned_target_candidates: u64,
    pub location_parent_links_without_scanned_target_candidates: u64,
    pub location_parent_links_unresolved_without_profile: u64,
    pub cell_water_environment_map_fields: u64,
    pub cell_water_environment_map_paths_normalized: u64,
    pub malformed_cell_water_environment_map_fields: u64,
    pub cell_water_environment_map_references_with_loose_file: u64,
    pub cell_water_environment_map_references_with_archive_member: u64,
    pub cell_water_environment_map_references_without_candidate_match: u64,
    pub unique_cell_water_environment_map_lookup_paths: u64,
    pub archives_indexed_for_cell_water_environment_map: u64,
    pub archive_index_failures_for_cell_water_environment_map: u64,
    pub cell_context_form_links: u64,
    pub cell_context_form_links_by_tag: BTreeMap<String, u64>,
    pub cell_context_links_with_scanned_target_candidates: u64,
    pub cell_context_links_without_scanned_target_candidates: u64,
    pub cell_context_links_unresolved_without_profile: u64,
    pub cell_subrecords_scanned: u64,
    pub cell_records_with_xclr: u64,
    pub cell_records_without_xclr: u64,
    pub cell_records_with_multiple_xclr: u64,
    pub cell_xclr_fields: u64,
    pub malformed_xclr_fields: u64,
    pub xclr_payload_length_counts: BTreeMap<usize, u64>,
    pub cell_region_links: u64,
    pub cell_region_links_with_scanned_target_candidates: u64,
    pub cell_region_links_without_scanned_target_candidates: u64,
    pub cell_region_links_unresolved_without_profile: u64,
    pub cell_records_with_xclc: u64,
    pub cell_records_without_xclc: u64,
    pub cell_records_with_multiple_xclc: u64,
    pub malformed_xclc_fields: u64,
    pub xclc_payload_length_counts: BTreeMap<usize, u64>,
    pub cell_records_with_xclw: u64,
    pub cell_records_without_xclw: u64,
    pub cell_records_with_multiple_xclw: u64,
    pub malformed_xclw_fields: u64,
    pub xclw_payload_length_counts: BTreeMap<usize, u64>,
    pub xclw_value_class_counts: BTreeMap<String, u64>,
    pub cell_records_with_xcll: u64,
    pub cell_records_without_xcll: u64,
    pub cell_records_with_multiple_xcll: u64,
    pub unprofiled_xcll_fields: u64,
    pub xcll_payload_length_counts: BTreeMap<usize, u64>,
    pub cell_subrecord_shapes: BTreeMap<String, FieldShape>,
    pub land_records_with_cell_group_label: u64,
    pub land_cell_group_labels_resolved: u64,
    pub land_cell_groups_with_candidates: u64,
    pub land_cell_groups_without_candidates: u64,
    pub land_cell_candidate_count_distribution: BTreeMap<usize, u64>,
    pub land_records_with_world_group_label: u64,
    pub land_world_group_labels_resolved: u64,
    pub land_world_groups_with_candidates: u64,
    pub land_world_groups_without_candidates: u64,
    pub land_world_candidate_count_distribution: BTreeMap<usize, u64>,
    pub subrecords_scanned: u64,
    pub landscape_base_layers: u64,
    pub landscape_alpha_layers: u64,
    pub ltex_texture_set_fields: u64,
    pub ltex_material_fields: u64,
    pub ltex_grass_fields: u64,
    pub null_form_links: u64,
    pub links_resolved_to_source_keys: u64,
    pub light_links_requiring_profile: u64,
    pub invalid_master_indices: u64,
    pub links_with_scanned_target_candidates: u64,
    pub links_without_scanned_target_candidates: u64,
    pub malformed_link_payloads: u64,
    pub malformed_layer_payloads: u64,
    pub duplicate_layer_slots: u64,
    pub out_of_range_quadrants: u64,
    pub nonzero_auxiliary_layer_bytes: u64,
    pub opaque_subrecords: u64,
    pub unknown_subrecords: u64,
    pub land_field_shapes: BTreeMap<String, FieldShape>,
    /// For each LAND tag, number of physical LAND records by field occurrence count.
    pub land_record_field_occurrences: BTreeMap<String, BTreeMap<u64, u64>>,
    pub ltex_field_shapes: BTreeMap<String, FieldShape>,
    pub location_field_shapes: BTreeMap<String, FieldShape>,
}

#[derive(Debug, Default, Serialize)]
pub struct FieldShape {
    pub occurrences: u64,
    pub payload_length_counts: BTreeMap<usize, u64>,
}

#[derive(Debug, Serialize)]
struct PluginSource {
    file: String,
    bytes: u64,
    sha256: String,
    masters: Vec<String>,
    light: bool,
    header_version_bits: u32,
}

#[derive(Debug, Serialize)]
struct PluginScan {
    source: PluginSource,
    selected_records: u64,
    land_records: u64,
    ltex_records: u64,
    txst_records: u64,
    matt_records: u64,
    gras_records: u64,
    cell_records: u64,
    world_records: u64,
    regn_records: u64,
    location_records: u64,
    location_subrecords_scanned: u64,
    location_parent_form_links: u64,
    cell_subrecords_scanned: u64,
    cell_context_form_links: u64,
    cell_context_form_links_by_tag: BTreeMap<String, u64>,
    cell_water_environment_map_fields: u64,
    cell_region_links_scanned: u64,
    malformed_xclr_fields: u64,
    xclr_payload_length_counts: BTreeMap<usize, u64>,
    malformed_xclc_fields: u64,
    xclc_payload_length_counts: BTreeMap<usize, u64>,
    malformed_xclw_fields: u64,
    xclw_payload_length_counts: BTreeMap<usize, u64>,
    xclw_value_class_counts: BTreeMap<String, u64>,
    unprofiled_xcll_fields: u64,
    xcll_payload_length_counts: BTreeMap<usize, u64>,
    cell_subrecord_shapes: BTreeMap<String, FieldShape>,
    subrecords_scanned: u64,
    malformed_link_payloads: u64,
    malformed_layer_payloads: u64,
    opaque_subrecords: u64,
    unknown_subrecords: u64,
    land_field_shapes: BTreeMap<String, FieldShape>,
    #[serde(skip)]
    land_record_field_occurrences: BTreeMap<String, BTreeMap<u64, u64>>,
    ltex_field_shapes: BTreeMap<String, FieldShape>,
    location_field_shapes: BTreeMap<String, FieldShape>,
}

#[derive(Debug, Serialize)]
struct FormLink {
    tag: String,
    payload_offset_in_decoded_record: usize,
    field_payload_bytes: usize,
    field_sha256: String,
    raw_form_id: Option<u32>,
    target_kind: &'static str,
    source_key: Option<trace::SourceKey>,
    status: &'static str,
    scanned_target_record_count: usize,
}

#[derive(Debug, Serialize)]
struct LayerHeader {
    tag: String,
    payload_offset_in_decoded_record: usize,
    field_payload_bytes: usize,
    field_sha256: String,
    texture: FormLink,
    quadrant: u8,
    auxiliary_byte: u8,
    layer_index: i16,
    duplicate_slot: bool,
}

#[derive(Debug, Serialize)]
struct OpaqueField {
    tag: String,
    payload_offset_in_decoded_record: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    classification: &'static str,
}

#[derive(Debug, Serialize)]
struct CellWaterEnvironmentMapField {
    tag: String,
    payload_offset_in_decoded_record: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    terminator_offset: Option<usize>,
    nonzero_bytes_after_terminator: bool,
    normalized_data_asset_path: Option<Vec<u8>>,
    status: &'static str,
    lookup: Option<CellWaterEnvironmentMapLookup>,
}

#[derive(Debug, Serialize)]
struct CellWaterEnvironmentMapLookup {
    loose_file_status: &'static str,
    archive_matches: Vec<asset_lookup::ArchiveMatch>,
}

#[derive(Debug, Serialize)]
struct RecordRow {
    record_kind: String,
    plugin_file: String,
    form_id: u32,
    source_key: Option<trace::SourceKey>,
    record_offset: u64,
    record_flags: u32,
    /// Raw group ancestry; kinds 1/6 also provide raw world/cell FormID joins.
    group_path: Vec<Group>,
    containing_cell_raw: Option<u32>,
    containing_cell: Option<trace::SourceKey>,
    containing_cell_candidate_count: usize,
    containing_world_raw: Option<u32>,
    containing_world: Option<trace::SourceKey>,
    containing_world_candidate_count: usize,
    cell_grid_fields: Vec<CellGridField>,
    cell_water_height_fields: Vec<CellWaterHeightField>,
    cell_lighting_fields: Vec<OpaqueField>,
    cell_region_fields: Vec<CellRegionField>,
    cell_water_environment_map_fields: Vec<CellWaterEnvironmentMapField>,
    editor_id_fields: Vec<OpaqueField>,
    form_links: Vec<FormLink>,
    layers: Vec<LayerHeader>,
    opaque_subrecords: Vec<OpaqueField>,
}

#[derive(Debug, Serialize)]
struct CellGridField {
    payload_offset_in_decoded_record: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    shape_status: &'static str,
    x: Option<i32>,
    y: Option<i32>,
    land_flags: Option<u8>,
    reserved_bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct CellWaterHeightField {
    payload_offset_in_decoded_record: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    shape_status: &'static str,
    raw_bits: Option<u32>,
    raw_bits_hex: Option<String>,
    ieee754_class: Option<&'static str>,
    finite_value: Option<f32>,
}

#[derive(Debug, Serialize)]
struct CellRegionField {
    payload_offset_in_decoded_record: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    links: Vec<FormLink>,
    trailing_bytes: Option<OpaqueField>,
}

fn field_hash(data: &[u8]) -> Result<String> {
    Ok(digest_reader(&mut Cursor::new(data))?.1)
}

fn opaque_field(
    tag: &[u8; 4],
    offset: usize,
    data: &[u8],
    classification: &'static str,
) -> Result<OpaqueField> {
    let retained = data.len().min(RAW_EVIDENCE_LIMIT);
    Ok(OpaqueField {
        tag: framing::signature(*tag),
        payload_offset_in_decoded_record: offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
        classification,
    })
}

fn cell_water_environment_map_field(
    offset: usize,
    data: &[u8],
) -> Result<CellWaterEnvironmentMapField> {
    let terminator_offset = data.iter().position(|byte| *byte == 0);
    let raw_path = terminator_offset.map_or(data, |at| &data[..at]);
    let nonzero_bytes_after_terminator =
        terminator_offset.is_some_and(|at| data[at + 1..].iter().any(|byte| *byte != 0));
    let retained = data.len().min(XWEM_PATH_LIMIT);
    let (normalized_data_asset_path, status) = if data.len() > XWEM_PATH_LIMIT {
        (None, "field-over-4096-bytes")
    } else if terminator_offset.is_none() {
        (None, "missing-terminator")
    } else if raw_path.is_empty() {
        (None, "empty-path")
    } else if raw_path.len() < 5
        || !raw_path[..5].eq_ignore_ascii_case(b"Data\\")
            && !raw_path[..5].eq_ignore_ascii_case(b"Data/")
    {
        (None, "missing-data-root-prefix")
    } else {
        match AssetPath::new(&raw_path[5..]) {
            Ok(path) if nonzero_bytes_after_terminator => (
                Some(path.bytes().to_vec()),
                "normalized-asset-path-with-nonzero-trailing-bytes",
            ),
            Ok(path) => (Some(path.bytes().to_vec()), "normalized-asset-path"),
            Err(_) => (None, "invalid-data-asset-path"),
        }
    };
    Ok(CellWaterEnvironmentMapField {
        tag: "XWEM".into(),
        payload_offset_in_decoded_record: offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
        terminator_offset,
        nonzero_bytes_after_terminator,
        normalized_data_asset_path,
        status,
        lookup: None,
    })
}

fn cell_grid_field(offset: usize, data: &[u8]) -> Result<CellGridField> {
    let retained = data.len().min(RAW_EVIDENCE_LIMIT);
    let (shape_status, x, y, land_flags, reserved_bytes) = match data.len() {
        8 => (
            "coordinates-only",
            Some(i32::from_le_bytes(
                data[0..4].try_into().expect("four-byte X coordinate"),
            )),
            Some(i32::from_le_bytes(
                data[4..8].try_into().expect("four-byte Y coordinate"),
            )),
            None,
            Vec::new(),
        ),
        12 => (
            "coordinates-and-land-flags",
            Some(i32::from_le_bytes(
                data[0..4].try_into().expect("four-byte X coordinate"),
            )),
            Some(i32::from_le_bytes(
                data[4..8].try_into().expect("four-byte Y coordinate"),
            )),
            Some(data[8]),
            data[9..12].to_vec(),
        ),
        _ => ("unsupported-payload-length", None, None, None, Vec::new()),
    };
    Ok(CellGridField {
        payload_offset_in_decoded_record: offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
        shape_status,
        x,
        y,
        land_flags,
        reserved_bytes,
    })
}

fn cell_water_height_field(offset: usize, data: &[u8]) -> Result<CellWaterHeightField> {
    let retained = data.len().min(RAW_EVIDENCE_LIMIT);
    let (shape_status, raw_bits, raw_bits_hex, ieee754_class, finite_value) =
        if let Ok(bytes) = <[u8; 4]>::try_from(data) {
            let raw_bits = u32::from_le_bytes(bytes);
            let value = f32::from_bits(raw_bits);
            let ieee754_class = if value.is_nan() {
                "nan"
            } else if value == f32::INFINITY {
                "positive-infinity"
            } else if value == f32::NEG_INFINITY {
                "negative-infinity"
            } else {
                "finite"
            };
            (
                "ieee754-single-precision",
                Some(raw_bits),
                Some(format!("0x{raw_bits:08X}")),
                Some(ieee754_class),
                value.is_finite().then_some(value),
            )
        } else {
            ("unsupported-payload-length", None, None, None, None)
        };
    Ok(CellWaterHeightField {
        payload_offset_in_decoded_record: offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
        shape_status,
        raw_bits,
        raw_bits_hex,
        ieee754_class,
        finite_value,
    })
}

fn cell_lighting_field(offset: usize, data: &[u8]) -> Result<OpaqueField> {
    let classification = if matches!(data.len(), 64 | 92) {
        "known-corpus-size-unparsed"
    } else {
        "unprofiled-size-unparsed"
    };
    opaque_field(b"XCLL", offset, data, classification)
}

fn cell_region_field(
    offset: usize,
    data: &[u8],
    file: &str,
    masters: &[String],
) -> Result<CellRegionField> {
    let complete_bytes = data.len() / 4 * 4;
    let links = data[..complete_bytes]
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            form_link(b"XCLR", offset + index * 4, bytes, "REGN", file, masters)
                .map(|link| link.expect("XCLR chunk is four bytes"))
        })
        .collect::<Result<Vec<_>>>()?;
    let trailing = &data[complete_bytes..];
    let trailing_bytes = if trailing.is_empty() {
        None
    } else {
        Some(opaque_field(
            b"XCLR",
            offset + complete_bytes,
            trailing,
            "incomplete-cell-region-form-id",
        )?)
    };
    let retained = data.len().min(RAW_EVIDENCE_LIMIT);
    Ok(CellRegionField {
        payload_offset_in_decoded_record: offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
        links,
        trailing_bytes,
    })
}

fn form_link(
    tag: &[u8; 4],
    offset: usize,
    data: &[u8],
    target_kind: &'static str,
    file: &str,
    masters: &[String],
) -> Result<Option<FormLink>> {
    if data.len() != 4 {
        return Ok(None);
    }
    let raw = u32::from_le_bytes(data.try_into().expect("four-byte FormID field"));
    let (source_key, status) = if raw == 0 {
        (None, "null-form-id")
    } else if raw >> 24 == 0xFE {
        (None, "light-reference-needs-active-profile")
    } else if (raw >> 24) as usize > masters.len() {
        (None, "master-index-out-of-range")
    } else {
        (
            trace::source_key(file, masters, raw)?,
            "source-key-resolved",
        )
    };
    Ok(Some(FormLink {
        tag: framing::signature(*tag),
        payload_offset_in_decoded_record: offset,
        field_payload_bytes: data.len(),
        field_sha256: field_hash(data)?,
        raw_form_id: Some(raw),
        target_kind,
        source_key,
        status,
        scanned_target_record_count: 0,
    }))
}

fn cell_context_target(kind: &[u8; 4]) -> Option<&'static str> {
    match kind {
        b"XCCM" => Some("REGN"),
        b"XLCN" => Some("LCTN"),
        b"XCWT" => Some("WATR"),
        b"XCIM" => Some("IMGS"),
        b"XEZN" => Some("ECZN"),
        b"XCAS" => Some("ASPC"),
        b"XCMO" => Some("MUSC"),
        b"LTMP" => Some("LGTM"),
        _ => None,
    }
}

fn known_location_field(kind: &[u8; 4]) -> bool {
    matches!(
        kind,
        b"ACPR"
            | b"LCPR"
            | b"RCPR"
            | b"ACUN"
            | b"LCUN"
            | b"RCUN"
            | b"ACSR"
            | b"LCSR"
            | b"RCSR"
            | b"ACEC"
            | b"LCEC"
            | b"RCEC"
            | b"ACID"
            | b"LCID"
            | b"ACEP"
            | b"LCEP"
            | b"EDID"
            | b"FULL"
            | b"KSIZ"
            | b"KWDA"
            | b"NAM1"
            | b"FNAM"
            | b"MNAM"
            | b"RNAM"
            | b"NAM0"
            | b"CNAM"
    )
}

fn scan_plugin(
    path: &Path,
    remaining_records: usize,
    remaining_subrecords: usize,
    remaining_cell_subrecords: usize,
    remaining_cell_region_links: usize,
) -> Result<(PluginScan, Vec<RecordRow>)> {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad("plugin", 0, "non-Unicode filename"))?;
    plugin_name(file)?;
    let metadata = plugin::inspect(path)?;
    let mut source = BufReader::new(open_source(path)?);
    let (bytes, sha256) = digest_reader(&mut source)?;
    if bytes != metadata.bytes || sha256 != metadata.sha256 {
        return Err(Error::Unsupported(format!(
            "plugin changed between inspect and landscape-link scan: {}",
            path.display()
        )));
    }
    source.seek(SeekFrom::Start(0))?;

    let mut rows = Vec::new();
    let mut active_groups = Vec::<(u64, Group)>::new();
    let mut scan = PluginScan {
        source: PluginSource {
            file: metadata.file.clone(),
            bytes,
            sha256,
            masters: metadata.masters.clone(),
            light: metadata.light,
            header_version_bits: metadata.header_version_bits,
        },
        selected_records: 0,
        land_records: 0,
        ltex_records: 0,
        txst_records: 0,
        matt_records: 0,
        gras_records: 0,
        cell_records: 0,
        world_records: 0,
        regn_records: 0,
        location_records: 0,
        location_subrecords_scanned: 0,
        location_parent_form_links: 0,
        cell_subrecords_scanned: 0,
        cell_context_form_links: 0,
        cell_context_form_links_by_tag: BTreeMap::new(),
        cell_water_environment_map_fields: 0,
        cell_region_links_scanned: 0,
        malformed_xclr_fields: 0,
        xclr_payload_length_counts: BTreeMap::new(),
        subrecords_scanned: 0,
        malformed_link_payloads: 0,
        malformed_layer_payloads: 0,
        malformed_xclc_fields: 0,
        opaque_subrecords: 0,
        unknown_subrecords: 0,
        land_field_shapes: BTreeMap::new(),
        land_record_field_occurrences: BTreeMap::new(),
        location_field_shapes: BTreeMap::new(),
        xclc_payload_length_counts: BTreeMap::new(),
        malformed_xclw_fields: 0,
        xclw_payload_length_counts: BTreeMap::new(),
        xclw_value_class_counts: BTreeMap::new(),
        unprofiled_xcll_fields: 0,
        xcll_payload_length_counts: BTreeMap::new(),
        cell_subrecord_shapes: BTreeMap::new(),
        ltex_field_shapes: BTreeMap::new(),
    };

    framing::visit_selected(
        &mut source,
        bytes,
        &metadata.file,
        framing::Limits::default(),
        |header| RECORD_KINDS.contains(&&header.kind),
        |event| {
            let record = match event {
                SelectedEvent::Group(group) => {
                    while active_groups
                        .last()
                        .is_some_and(|(end, _)| *end <= group.offset)
                    {
                        active_groups.pop();
                    }
                    active_groups.push((
                        group.offset.saturating_add(u64::from(group.size)),
                        group.clone(),
                    ));
                    return Ok(());
                }
                SelectedEvent::Deferred(_) => return Ok(()),
                SelectedEvent::Record(record) => record,
            };
            if !RECORD_KINDS.contains(&&record.header.kind) {
                return Ok(());
            }
            while active_groups
                .last()
                .is_some_and(|(end, _)| *end <= record.header.offset)
            {
                active_groups.pop();
            }
            if rows.len() >= remaining_records || rows.len() >= RECORD_LIMIT {
                return Err(shared_error("landscape selected-record budget exceeded"));
            }
            scan.selected_records += 1;
            let kind = record.header.kind;
            match &kind {
                b"LAND" => scan.land_records += 1,
                b"LTEX" => scan.ltex_records += 1,
                b"TXST" => scan.txst_records += 1,
                b"MATT" => scan.matt_records += 1,
                b"GRAS" => scan.gras_records += 1,
                b"CELL" => scan.cell_records += 1,
                b"WRLD" => scan.world_records += 1,
                b"REGN" => scan.regn_records += 1,
                b"LCTN" => scan.location_records += 1,
                b"CLMT" | b"WATR" | b"IMGS" | b"ECZN" | b"ASPC" | b"MUSC" | b"LGTM" => {}
                _ => unreachable!("selected record kind checked"),
            }

            let source_key = trace::source_record_key(
                &metadata.file,
                &metadata.masters,
                record.header.form_id,
                metadata.light,
                metadata.header_version_bits,
            )
            .map_err(shared_error)?;
            let group_path = if kind == *b"LAND" {
                active_groups
                    .iter()
                    .map(|(_, group)| group.clone())
                    .collect()
            } else {
                Vec::new()
            };
            let containing_cell_raw = trace::group_label(&group_path, 6);
            let containing_world_raw = trace::group_label(&group_path, 1);
            let containing_cell = containing_cell_raw
                .map(|raw| trace::source_key(&metadata.file, &metadata.masters, raw))
                .transpose()
                .map_err(shared_error)?
                .flatten();
            let containing_world = containing_world_raw
                .map(|raw| trace::source_key(&metadata.file, &metadata.masters, raw))
                .transpose()
                .map_err(shared_error)?
                .flatten();
            let mut row = RecordRow {
                record_kind: framing::signature(kind),
                plugin_file: metadata.file.clone(),
                form_id: record.header.form_id,
                source_key,
                record_offset: record.header.offset,
                record_flags: record.header.flags,
                group_path,
                containing_cell_raw,
                containing_cell,
                containing_cell_candidate_count: 0,
                containing_world_raw,
                containing_world,
                containing_world_candidate_count: 0,
                cell_grid_fields: Vec::new(),
                cell_water_height_fields: Vec::new(),
                cell_lighting_fields: Vec::new(),
                cell_region_fields: Vec::new(),
                cell_water_environment_map_fields: Vec::new(),
                editor_id_fields: Vec::new(),
                form_links: Vec::new(),
                layers: Vec::new(),
                opaque_subrecords: Vec::new(),
            };

            let mut land_field_occurrences = BTreeMap::<String, u64>::new();
            if kind == *b"CELL" {
                framing::visit_subrecords(record, &metadata.file, |field| {
                    scan.cell_subrecords_scanned += 1;
                    if scan.cell_subrecords_scanned as usize > remaining_cell_subrecords
                        || scan.cell_subrecords_scanned as usize > CELL_SUBRECORD_LIMIT
                    {
                        return Err(shared_error("landscape CELL subrecord budget exceeded"));
                    }
                    observe_field_shape(
                        &mut scan.cell_subrecord_shapes,
                        &field.kind,
                        field.data.len(),
                    )
                    .map_err(shared_error)?;
                    if field.kind == *b"XCLC" {
                        if !scan
                            .xclc_payload_length_counts
                            .contains_key(&field.data.len())
                            && scan.xclc_payload_length_counts.len() >= FIELD_SHAPE_LENGTH_LIMIT
                        {
                            return Err(shared_error("CELL XCLC payload-shape budget exceeded"));
                        }
                        *scan
                            .xclc_payload_length_counts
                            .entry(field.data.len())
                            .or_default() += 1;
                        let grid = cell_grid_field(field.payload_offset, field.data)
                            .map_err(shared_error)?;
                        if grid.shape_status == "unsupported-payload-length" {
                            scan.malformed_xclc_fields += 1;
                        }
                        row.cell_grid_fields.push(grid);
                    } else if field.kind == *b"XCLW" {
                        if !scan
                            .xclw_payload_length_counts
                            .contains_key(&field.data.len())
                            && scan.xclw_payload_length_counts.len() >= FIELD_SHAPE_LENGTH_LIMIT
                        {
                            return Err(shared_error("CELL XCLW payload-shape budget exceeded"));
                        }
                        *scan
                            .xclw_payload_length_counts
                            .entry(field.data.len())
                            .or_default() += 1;
                        let water_height =
                            cell_water_height_field(field.payload_offset, field.data)
                                .map_err(shared_error)?;
                        if water_height.shape_status == "unsupported-payload-length" {
                            scan.malformed_xclw_fields += 1;
                        }
                        if let Some(class) = water_height.ieee754_class {
                            *scan
                                .xclw_value_class_counts
                                .entry(class.into())
                                .or_default() += 1;
                        }
                        row.cell_water_height_fields.push(water_height);
                    } else if field.kind == *b"XCLL" {
                        if !scan
                            .xcll_payload_length_counts
                            .contains_key(&field.data.len())
                            && scan.xcll_payload_length_counts.len() >= FIELD_SHAPE_LENGTH_LIMIT
                        {
                            return Err(shared_error("CELL XCLL payload-shape budget exceeded"));
                        }
                        *scan
                            .xcll_payload_length_counts
                            .entry(field.data.len())
                            .or_default() += 1;
                        let lighting = cell_lighting_field(field.payload_offset, field.data)
                            .map_err(shared_error)?;
                        if lighting.classification == "unprofiled-size-unparsed" {
                            scan.unprofiled_xcll_fields += 1;
                        }
                        row.cell_lighting_fields.push(lighting);
                    } else if field.kind == *b"XCLR" {
                        if !scan
                            .xclr_payload_length_counts
                            .contains_key(&field.data.len())
                            && scan.xclr_payload_length_counts.len() >= FIELD_SHAPE_LENGTH_LIMIT
                        {
                            return Err(shared_error("CELL XCLR payload-shape budget exceeded"));
                        }
                        *scan
                            .xclr_payload_length_counts
                            .entry(field.data.len())
                            .or_default() += 1;
                        let entry_count = field.data.len() / 4;
                        let next_count = (scan.cell_region_links_scanned as usize)
                            .checked_add(entry_count)
                            .ok_or_else(|| shared_error("CELL XCLR entry count overflow"))?;
                        if next_count > remaining_cell_region_links
                            || next_count > CELL_REGION_LINK_LIMIT
                        {
                            return Err(shared_error("CELL XCLR FormID entry budget exceeded"));
                        }
                        let region_field = cell_region_field(
                            field.payload_offset,
                            field.data,
                            &metadata.file,
                            &metadata.masters,
                        )
                        .map_err(shared_error)?;
                        scan.cell_region_links_scanned += region_field.links.len() as u64;
                        if region_field.trailing_bytes.is_some() {
                            scan.malformed_xclr_fields += 1;
                        }
                        row.cell_region_fields.push(region_field);
                    } else if field.kind == *b"XWEM" {
                        scan.cell_water_environment_map_fields += 1;
                        row.cell_water_environment_map_fields.push(
                            cell_water_environment_map_field(field.payload_offset, field.data)
                                .map_err(shared_error)?,
                        );
                    } else if let Some(target_kind) = cell_context_target(&field.kind) {
                        if let Some(link) = form_link(
                            &field.kind,
                            field.payload_offset,
                            field.data,
                            target_kind,
                            &metadata.file,
                            &metadata.masters,
                        )
                        .map_err(shared_error)?
                        {
                            scan.cell_context_form_links += 1;
                            *scan
                                .cell_context_form_links_by_tag
                                .entry(framing::signature(field.kind))
                                .or_default() += 1;
                            row.form_links.push(link);
                        } else {
                            scan.malformed_link_payloads += 1;
                            row.opaque_subrecords.push(
                                opaque_field(
                                    &field.kind,
                                    field.payload_offset,
                                    field.data,
                                    "malformed-cell-context-form-id",
                                )
                                .map_err(shared_error)?,
                            );
                        }
                    }
                    Ok(())
                })?;
            } else if kind == *b"LAND" || kind == *b"LTEX" {
                framing::visit_subrecords(record, &metadata.file, |field| {
                    scan.subrecords_scanned += 1;
                    if scan.subrecords_scanned as usize > remaining_subrecords
                        || scan.subrecords_scanned as usize > SUBRECORD_LIMIT
                    {
                        return Err(shared_error("landscape subrecord budget exceeded"));
                    }
                    let field_shapes = if kind == *b"LAND" {
                        &mut scan.land_field_shapes
                    } else {
                        &mut scan.ltex_field_shapes
                    };
                    observe_field_shape(field_shapes, &field.kind, field.data.len())
                        .map_err(shared_error)?;
                    if kind == *b"LAND" {
                        *land_field_occurrences
                            .entry(framing::signature(field.kind))
                            .or_default() += 1;
                    }
                    match (&kind, &field.kind) {
                        (b"LTEX", b"EDID") => row.editor_id_fields.push(
                            opaque_field(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                "editor-id",
                            )
                            .map_err(shared_error)?,
                        ),
                        (b"LTEX", b"TNAM") => {
                            if let Some(link) = form_link(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                "TXST",
                                &metadata.file,
                                &metadata.masters,
                            )
                            .map_err(shared_error)?
                            {
                                row.form_links.push(link);
                            } else {
                                scan.malformed_link_payloads += 1;
                                row.opaque_subrecords.push(
                                    opaque_field(
                                        &field.kind,
                                        field.payload_offset,
                                        field.data,
                                        "malformed-texture-set-form-id",
                                    )
                                    .map_err(shared_error)?,
                                );
                            }
                        }
                        (b"LTEX", b"MNAM") => {
                            if let Some(link) = form_link(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                "MATT",
                                &metadata.file,
                                &metadata.masters,
                            )
                            .map_err(shared_error)?
                            {
                                row.form_links.push(link);
                            } else {
                                scan.malformed_link_payloads += 1;
                                row.opaque_subrecords.push(
                                    opaque_field(
                                        &field.kind,
                                        field.payload_offset,
                                        field.data,
                                        "malformed-material-form-id",
                                    )
                                    .map_err(shared_error)?,
                                );
                            }
                        }
                        (b"LTEX", b"GNAM") => {
                            if let Some(link) = form_link(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                "GRAS",
                                &metadata.file,
                                &metadata.masters,
                            )
                            .map_err(shared_error)?
                            {
                                row.form_links.push(link);
                            } else {
                                scan.malformed_link_payloads += 1;
                                row.opaque_subrecords.push(
                                    opaque_field(
                                        &field.kind,
                                        field.payload_offset,
                                        field.data,
                                        "malformed-grass-form-id",
                                    )
                                    .map_err(shared_error)?,
                                );
                            }
                        }
                        (b"LAND", b"BTXT") | (b"LAND", b"ATXT") => {
                            if field.data.len() != 8 {
                                scan.malformed_layer_payloads += 1;
                                row.opaque_subrecords.push(
                                    opaque_field(
                                        &field.kind,
                                        field.payload_offset,
                                        field.data,
                                        "malformed-landscape-layer-header",
                                    )
                                    .map_err(shared_error)?,
                                );
                                return Ok(());
                            }
                            let link = form_link(
                                &field.kind,
                                field.payload_offset,
                                &field.data[..4],
                                "LTEX",
                                &metadata.file,
                                &metadata.masters,
                            )
                            .map_err(shared_error)?
                            .expect("landscape layer FormID is four bytes");
                            row.layers.push(LayerHeader {
                                tag: framing::signature(field.kind),
                                payload_offset_in_decoded_record: field.payload_offset,
                                field_payload_bytes: field.data.len(),
                                field_sha256: field_hash(field.data).map_err(shared_error)?,
                                texture: link,
                                quadrant: field.data[4],
                                auxiliary_byte: field.data[5],
                                layer_index: i16::from_le_bytes([field.data[6], field.data[7]]),
                                duplicate_slot: false,
                            });
                        }
                        (b"LAND", b"EDID") => row.editor_id_fields.push(
                            opaque_field(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                "editor-id",
                            )
                            .map_err(shared_error)?,
                        ),
                        _ => {
                            scan.opaque_subrecords += 1;
                            let classification = if matches!(
                                &field.kind,
                                b"VTXT"
                                    | b"VHGT"
                                    | b"VNML"
                                    | b"BTPC"
                                    | b"LTEX"
                                    | b"HNAM"
                                    | b"SNAM"
                                    | b"GNAM"
                                    | b"TNAM"
                                    | b"MNAM"
                            ) {
                                "known-but-out-of-scope"
                            } else {
                                scan.unknown_subrecords += 1;
                                "unknown"
                            };
                            row.opaque_subrecords.push(
                                opaque_field(
                                    &field.kind,
                                    field.payload_offset,
                                    field.data,
                                    classification,
                                )
                                .map_err(shared_error)?,
                            );
                        }
                    }
                    Ok(())
                })?;
            } else if kind == *b"LCTN" {
                framing::visit_subrecords(record, &metadata.file, |field| {
                    scan.subrecords_scanned += 1;
                    scan.location_subrecords_scanned += 1;
                    if scan.subrecords_scanned as usize > remaining_subrecords
                        || scan.subrecords_scanned as usize > SUBRECORD_LIMIT
                    {
                        return Err(shared_error("landscape/location subrecord budget exceeded"));
                    }
                    observe_field_shape(
                        &mut scan.location_field_shapes,
                        &field.kind,
                        field.data.len(),
                    )
                    .map_err(shared_error)?;
                    if field.kind == *b"PNAM" {
                        if let Some(link) = form_link(
                            &field.kind,
                            field.payload_offset,
                            field.data,
                            "LCTN",
                            &metadata.file,
                            &metadata.masters,
                        )
                        .map_err(shared_error)?
                        {
                            scan.location_parent_form_links += 1;
                            row.form_links.push(link);
                        } else {
                            scan.malformed_link_payloads += 1;
                            row.opaque_subrecords.push(
                                opaque_field(
                                    &field.kind,
                                    field.payload_offset,
                                    field.data,
                                    "malformed-location-parent-form-id",
                                )
                                .map_err(shared_error)?,
                            );
                        }
                    } else {
                        scan.opaque_subrecords += 1;
                        let classification = if known_location_field(&field.kind) {
                            "known-location-field-not-decoded"
                        } else {
                            scan.unknown_subrecords += 1;
                            "unrecognized-location-subrecord"
                        };
                        row.opaque_subrecords.push(
                            opaque_field(
                                &field.kind,
                                field.payload_offset,
                                field.data,
                                classification,
                            )
                            .map_err(shared_error)?,
                        );
                    }
                    Ok(())
                })?;
            }

            if kind == *b"LAND" {
                for (tag, occurrences) in land_field_occurrences {
                    *scan
                        .land_record_field_occurrences
                        .entry(tag)
                        .or_default()
                        .entry(occurrences)
                        .or_default() += 1;
                }
                let mut slots = BTreeSet::new();
                for layer in &mut row.layers {
                    let key = (layer.tag.clone(), layer.quadrant, layer.layer_index);
                    if !slots.insert(key) {
                        layer.duplicate_slot = true;
                    }
                }
            }
            rows.push(row);
            Ok(())
        },
    )?;
    Ok((scan, rows))
}

/// Export selected physical record identities and full-master link candidates.
/// Light-plugin FE links remain raw and unresolved without an explicit profile.
pub fn export_many(
    data_dir: &Path,
    plugin_paths: &[PathBuf],
    writer: &mut impl Write,
) -> Result<Summary> {
    if plugin_paths.is_empty() {
        return Err(Error::Unsupported(
            "at least one plugin source is required".into(),
        ));
    }
    let mut scans = Vec::new();
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    let mut remaining_records = RECORD_LIMIT;
    let mut remaining_subrecords = SUBRECORD_LIMIT;
    let mut remaining_cell_subrecords = CELL_SUBRECORD_LIMIT;
    let mut remaining_cell_region_links = CELL_REGION_LINK_LIMIT;
    for input in plugin_paths {
        let metadata = std::fs::symlink_metadata(input)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::Unsupported(format!(
                "plugin input must be a regular non-symlink file: {}",
                input.display()
            )));
        }
        let path = input.canonicalize()?;
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Unsupported("plugin filename is not Unicode".into()))?;
        let normalized_name = plugin_name(file)?;
        if !seen.insert(normalized_name.clone()) {
            return Err(Error::Unsupported(format!(
                "duplicate plugin source name in landscape-link scan: {normalized_name}"
            )));
        }
        let (scan, mut source_rows) = scan_plugin(
            &path,
            remaining_records,
            remaining_subrecords,
            remaining_cell_subrecords,
            remaining_cell_region_links,
        )?;
        remaining_records = remaining_records.saturating_sub(scan.selected_records as usize);
        remaining_subrecords =
            remaining_subrecords.saturating_sub(scan.subrecords_scanned as usize);
        remaining_cell_subrecords =
            remaining_cell_subrecords.saturating_sub(scan.cell_subrecords_scanned as usize);
        remaining_cell_region_links =
            remaining_cell_region_links.saturating_sub(scan.cell_region_links_scanned as usize);
        scans.push(scan);
        rows.append(&mut source_rows);
    }

    let mut requested = BTreeSet::new();
    for row in &rows {
        for field in &row.cell_water_environment_map_fields {
            if let Some(path) = &field.normalized_data_asset_path {
                requested.insert(AssetPath::new(path)?);
            }
        }
    }
    let lookup = asset_lookup::inspect(data_dir, &requested)?;

    let mut candidates = BTreeMap::<(String, trace::SourceKey), usize>::new();
    for row in &rows {
        if let Some(key) = &row.source_key {
            *candidates
                .entry((row.record_kind.clone(), key.clone()))
                .or_default() += 1;
        }
    }

    let mut summary = summarize(&scans);
    summary.unique_cell_water_environment_map_lookup_paths = requested.len() as u64;
    summary.archives_indexed_for_cell_water_environment_map = lookup.summary.archives_indexed;
    summary.archive_index_failures_for_cell_water_environment_map =
        lookup.summary.archive_index_failures;
    for row in &mut rows {
        for field in &mut row.cell_water_environment_map_fields {
            let Some(path) = &field.normalized_data_asset_path else {
                summary.malformed_cell_water_environment_map_fields += 1;
                continue;
            };
            summary.cell_water_environment_map_paths_normalized += 1;
            if field.nonzero_bytes_after_terminator {
                summary.malformed_cell_water_environment_map_fields += 1;
            }
            let loose_file_status = asset_lookup::loose_file_status(data_dir, path);
            let archive_matches = lookup.matches.get(path).cloned().unwrap_or_default();
            if loose_file_status == "present" {
                summary.cell_water_environment_map_references_with_loose_file += 1;
            }
            if !archive_matches.is_empty() {
                summary.cell_water_environment_map_references_with_archive_member += 1;
            }
            if loose_file_status != "present" && archive_matches.is_empty() {
                summary.cell_water_environment_map_references_without_candidate_match += 1;
            }
            field.lookup = Some(CellWaterEnvironmentMapLookup {
                loose_file_status,
                archive_matches,
            });
        }
        if row.record_kind == "CELL" {
            if row.cell_region_fields.is_empty() {
                summary.cell_records_without_xclr += 1;
            } else {
                summary.cell_records_with_xclr += 1;
                if row.cell_region_fields.len() > 1 {
                    summary.cell_records_with_multiple_xclr += 1;
                }
            }
            if row.cell_grid_fields.is_empty() {
                summary.cell_records_without_xclc += 1;
            } else {
                summary.cell_records_with_xclc += 1;
                if row.cell_grid_fields.len() > 1 {
                    summary.cell_records_with_multiple_xclc += 1;
                }
            }
            if row.cell_water_height_fields.is_empty() {
                summary.cell_records_without_xclw += 1;
            } else {
                summary.cell_records_with_xclw += 1;
                if row.cell_water_height_fields.len() > 1 {
                    summary.cell_records_with_multiple_xclw += 1;
                }
            }
            if row.cell_lighting_fields.is_empty() {
                summary.cell_records_without_xcll += 1;
            } else {
                summary.cell_records_with_xcll += 1;
                if row.cell_lighting_fields.len() > 1 {
                    summary.cell_records_with_multiple_xcll += 1;
                }
            }
        }
        if row.record_kind == "LAND" {
            if row.containing_cell_raw.is_some() {
                summary.land_records_with_cell_group_label += 1;
            }
            if let Some(key) = &row.containing_cell {
                summary.land_cell_group_labels_resolved += 1;
                row.containing_cell_candidate_count = candidates
                    .get(&("CELL".into(), key.clone()))
                    .copied()
                    .unwrap_or_default();
                *summary
                    .land_cell_candidate_count_distribution
                    .entry(row.containing_cell_candidate_count)
                    .or_default() += 1;
                if row.containing_cell_candidate_count == 0 {
                    summary.land_cell_groups_without_candidates += 1;
                } else {
                    summary.land_cell_groups_with_candidates += 1;
                }
            }
            if row.containing_world_raw.is_some() {
                summary.land_records_with_world_group_label += 1;
            }
            if let Some(key) = &row.containing_world {
                summary.land_world_group_labels_resolved += 1;
                row.containing_world_candidate_count = candidates
                    .get(&("WRLD".into(), key.clone()))
                    .copied()
                    .unwrap_or_default();
                *summary
                    .land_world_candidate_count_distribution
                    .entry(row.containing_world_candidate_count)
                    .or_default() += 1;
                if row.containing_world_candidate_count == 0 {
                    summary.land_world_groups_without_candidates += 1;
                } else {
                    summary.land_world_groups_with_candidates += 1;
                }
            }
            if row.group_path.is_empty() {
                summary.land_records_without_group_path += 1;
            } else {
                summary.land_records_with_group_path += 1;
                *summary
                    .land_group_path_depths
                    .entry(row.group_path.len())
                    .or_default() += 1;
                for group in &row.group_path {
                    *summary
                        .land_group_types
                        .entry(group.kind.to_string())
                        .or_default() += 1;
                }
            }
        }
        for link in &mut row.form_links {
            link_into_summary(link, &candidates, &mut summary);
            if row.record_kind == "CELL" {
                match link.status {
                    "scanned-target-candidates" => {
                        summary.cell_context_links_with_scanned_target_candidates += 1;
                    }
                    "source-key-resolved-no-scanned-target" => {
                        summary.cell_context_links_without_scanned_target_candidates += 1;
                    }
                    "light-reference-needs-active-profile" => {
                        summary.cell_context_links_unresolved_without_profile += 1;
                    }
                    _ => {}
                }
            } else if row.record_kind == "LCTN" && link.tag == "PNAM" {
                match link.status {
                    "scanned-target-candidates" => {
                        summary.location_parent_links_with_scanned_target_candidates += 1;
                    }
                    "source-key-resolved-no-scanned-target" => {
                        summary.location_parent_links_without_scanned_target_candidates += 1;
                    }
                    "light-reference-needs-active-profile" => {
                        summary.location_parent_links_unresolved_without_profile += 1;
                    }
                    _ => {}
                }
            }
            if row.record_kind == "LTEX" && link.tag == "TNAM" {
                summary.ltex_texture_set_fields += 1;
            } else if row.record_kind == "LTEX" && link.tag == "MNAM" {
                summary.ltex_material_fields += 1;
            } else if row.record_kind == "LTEX" && link.tag == "GNAM" {
                summary.ltex_grass_fields += 1;
            }
        }
        for region_field in &mut row.cell_region_fields {
            for link in &mut region_field.links {
                link_into_summary(link, &candidates, &mut summary);
                summary.cell_region_links += 1;
                match link.status {
                    "scanned-target-candidates" => {
                        summary.cell_region_links_with_scanned_target_candidates += 1;
                    }
                    "source-key-resolved-no-scanned-target" => {
                        summary.cell_region_links_without_scanned_target_candidates += 1;
                    }
                    "light-reference-needs-active-profile" => {
                        summary.cell_region_links_unresolved_without_profile += 1;
                    }
                    _ => {}
                }
            }
        }
        for layer in &mut row.layers {
            link_into_summary(&mut layer.texture, &candidates, &mut summary);
            if layer.tag == "BTXT" {
                summary.landscape_base_layers += 1;
            } else if layer.tag == "ATXT" {
                summary.landscape_alpha_layers += 1;
            }
            if layer.quadrant >= 4 {
                summary.out_of_range_quadrants += 1;
            }
            if layer.auxiliary_byte != 0 {
                summary.nonzero_auxiliary_layer_bytes += 1;
            }
        }
        if row.record_kind == "LAND" {
            for layer in &row.layers {
                if layer.duplicate_slot {
                    summary.duplicate_layer_slots += 1;
                }
            }
        }
    }

    line(
        writer,
        &serde_json::json!({
            "type": "source",
            "schema_version": 1,
            "data_directory": data_dir,
            "plugins": scans.iter().map(|scan| &scan.source).collect::<Vec<_>>(),
            "plugin_scans": scans,
            "record_scope": ["LAND", "LTEX", "TXST", "MATT", "GRAS", "CELL", "WRLD", "REGN", "CLMT", "WATR", "IMGS", "ECZN", "ASPC", "MUSC", "LGTM", "LCTN"],
            "identity": "physical record rows; ordinary FormIDs resolve through declared masters; physical light-plugin record identities use validated HEDR local IDs; FE references need an explicit active profile",
            "link_scope": "LAND BTXT/ATXT -> LTEX; LTEX TNAM -> TXST; LTEX MNAM -> MATT; LTEX GNAM -> GRAS; CELL XCLR four-byte FormID entries -> REGN; CELL XCCM -> REGN; CELL XLCN -> LCTN; LCTN PNAM -> LCTN; CELL XCWT -> WATR; CELL XCIM -> IMGS; CELL XEZN -> ECZN; CELL XCAS -> ASPC; CELL XCMO -> MUSC; CELL LTMP -> LGTM; LAND kind-6/kind-1 group labels -> CELL/WRLD physical candidates through declared masters; every physical row is emitted once and no override is selected",
            "group_context": "LAND rows retain raw ancestor Group frames; kind-6/kind-1 labels are retained as cell/world FormIDs and matched to physical CELL/WRLD candidates where resolvable; CELL XCLC signed grid coordinates and XCLW raw water-height bits are retained without coordinate conversion or runtime interpretation",
            "subrecord_shapes": "per-tag occurrence and payload-length counts for every visited CELL, LAND, LTEX and LCTN field, plus per-LAND-record occurrence-count histograms for LAND tags; sizes and counts are evidence, not a decoder",
            "limits": {"selected_records": RECORD_LIMIT, "visited_land_ltex_location_subrecords": SUBRECORD_LIMIT, "visited_cell_subrecords": CELL_SUBRECORD_LIMIT, "cell_region_form_id_entries": CELL_REGION_LINK_LIMIT, "retained_raw_field_bytes": RAW_EVIDENCE_LIMIT},
            "cell_xclc": "CELL XCLC recognizes 8-byte signed X/Y coordinates or 12-byte X/Y plus flags and three reserved bytes; all raw bytes and hashes are retained, other lengths are visible as unsupported shapes",
            "cell_xclw": "CELL XCLW recognizes a four-byte IEEE 754 single-precision value; raw bits, bytes, and hash are retained, non-finite classes omit a JSON number, and other lengths remain unsupported",
            "cell_xcll": "CELL XCLL fields retain full hashes, exact lengths, and bounded raw bytes; 64- and 92-byte corpus sizes are identified but no lighting members are decoded",
            "cell_xclr": "CELL XCLR retains field hash and bounded bytes, resolves complete four-byte FormID entries to physical REGN candidates through declared masters, and preserves incomplete trailing bytes; no region runtime behavior is interpreted",
            "cell_context_form_links": "Skyrim CELL XCCM (Sky/Weather from Region) links to REGN and XLCN (Location) links to LCTN; XCWT, XCIM, XEZN, XCAS, XCMO and LTMP link to WATR, IMGS, ECZN, ASPC, MUSC and LGTM; recognized links accept only four-byte FormIDs and malformed sizes remain raw findings",
            "location_parent_form_links": "Skyrim LCTN PNAM is a four-byte parent-location FormLink to LCTN; source keys resolve through declared masters and every physical candidate is retained without override selection; other LCTN fields stay bounded raw evidence",
            "cell_xwem": "Skyrim CELL XWEM is treated as a bounded NUL-terminated Data-rooted asset path; an ASCII-case-insensitive Data\\ or Data/ prefix is stripped, the remaining path uses shared AssetPath normalization, and loose files plus names-only BSA matches are recorded without decompression or precedence selection; exact field hash and bounded raw bytes remain attached, and malformed terminators, prefixes, paths or nonzero trailing bytes stay visible",
            "cell_xwem_lookup": "requested paths are matched against loose files and immediate BSA name indexes through the shared lookup; no archive payload is read, decompressed or selected",
            "opaque": "LAND geometry, vertex normals, vertex heights, alpha weights, LTEX material payload, TXST bodies, MATT bodies, GRAS bodies, CELL fields other than XCLC/XCLW/XCLL/XCLR and the listed four-byte context links, XCLL lighting members, REGN/CLMT/WATR/IMGS/ECZN/ASPC/MUSC/LGTM bodies, LCTN fields other than PNAM, WRLD bodies, and unknown subrecords are not decoded"
        }),
    )?;
    for archive in &lookup.archives {
        line(
            writer,
            &serde_json::json!({"type":"archive-index", "archive":archive}),
        )?;
    }
    for row in rows {
        line(
            writer,
            &serde_json::json!({"type":"landscape-record", "record":row}),
        )?;
    }
    line(
        writer,
        &serde_json::json!({
            "type": "complete",
            "summary": summary,
            "scope": "bounded Skyrim landscape/texture links, CELL XCLR region references, selected CELL context FormID joins, LCTN PNAM parent-location links, XWEM loose/BSA name lookup, raw cell/world group-label joins, and structural CELL XCLC/XCLW/XCLL evidence from supplied physical plugin files; no load-order winner, archive payload/decompression, terrain geometry, texture pixels, material behavior, or runtime acceptance"
        }),
    )?;
    Ok(summary)
}

fn link_into_summary(
    link: &mut FormLink,
    candidates: &BTreeMap<(String, trace::SourceKey), usize>,
    summary: &mut Summary,
) {
    let original_status = link.status;
    match original_status {
        "null-form-id" => summary.null_form_links += 1,
        "light-reference-needs-active-profile" => summary.light_links_requiring_profile += 1,
        "master-index-out-of-range" => summary.invalid_master_indices += 1,
        "source-key-resolved" => summary.links_resolved_to_source_keys += 1,
        _ => {}
    }
    if let Some(key) = &link.source_key {
        link.scanned_target_record_count = candidates
            .get(&(link.target_kind.into(), key.clone()))
            .copied()
            .unwrap_or_default();
        if link.scanned_target_record_count == 0 {
            summary.links_without_scanned_target_candidates += 1;
            link.status = "source-key-resolved-no-scanned-target";
        } else {
            summary.links_with_scanned_target_candidates += 1;
            link.status = "scanned-target-candidates";
        }
    }
}

fn summarize(scans: &[PluginScan]) -> Summary {
    let mut summary = Summary {
        plugin_files_scanned: scans.len() as u64,
        selected_records: scans.iter().map(|scan| scan.selected_records).sum(),
        land_records: scans.iter().map(|scan| scan.land_records).sum(),
        ltex_records: scans.iter().map(|scan| scan.ltex_records).sum(),
        txst_records: scans.iter().map(|scan| scan.txst_records).sum(),
        matt_records: scans.iter().map(|scan| scan.matt_records).sum(),
        gras_records: scans.iter().map(|scan| scan.gras_records).sum(),
        cell_records: scans.iter().map(|scan| scan.cell_records).sum(),
        world_records: scans.iter().map(|scan| scan.world_records).sum(),
        regn_records: scans.iter().map(|scan| scan.regn_records).sum(),
        location_records: scans.iter().map(|scan| scan.location_records).sum(),
        location_subrecords_scanned: scans
            .iter()
            .map(|scan| scan.location_subrecords_scanned)
            .sum(),
        location_parent_form_links: scans
            .iter()
            .map(|scan| scan.location_parent_form_links)
            .sum(),
        cell_water_environment_map_fields: scans
            .iter()
            .map(|scan| scan.cell_water_environment_map_fields)
            .sum(),
        cell_context_form_links: scans.iter().map(|scan| scan.cell_context_form_links).sum(),
        cell_subrecords_scanned: scans.iter().map(|scan| scan.cell_subrecords_scanned).sum(),
        cell_xclr_fields: scans
            .iter()
            .map(|scan| scan.xclr_payload_length_counts.values().sum::<u64>())
            .sum(),
        malformed_xclr_fields: scans.iter().map(|scan| scan.malformed_xclr_fields).sum(),
        malformed_xclc_fields: scans.iter().map(|scan| scan.malformed_xclc_fields).sum(),
        malformed_xclw_fields: scans.iter().map(|scan| scan.malformed_xclw_fields).sum(),
        unprofiled_xcll_fields: scans.iter().map(|scan| scan.unprofiled_xcll_fields).sum(),
        subrecords_scanned: scans.iter().map(|scan| scan.subrecords_scanned).sum(),
        malformed_link_payloads: scans.iter().map(|scan| scan.malformed_link_payloads).sum(),
        malformed_layer_payloads: scans.iter().map(|scan| scan.malformed_layer_payloads).sum(),
        opaque_subrecords: scans.iter().map(|scan| scan.opaque_subrecords).sum(),
        unknown_subrecords: scans.iter().map(|scan| scan.unknown_subrecords).sum(),
        ..Summary::default()
    };
    for scan in scans {
        for (tag, count) in &scan.cell_context_form_links_by_tag {
            *summary
                .cell_context_form_links_by_tag
                .entry(tag.clone())
                .or_default() += count;
        }
        merge_field_shapes(&mut summary.land_field_shapes, &scan.land_field_shapes);
        for (tag, occurrence_counts) in &scan.land_record_field_occurrences {
            let destination = summary
                .land_record_field_occurrences
                .entry(tag.clone())
                .or_default();
            for (occurrences, records) in occurrence_counts {
                *destination.entry(*occurrences).or_default() += records;
            }
        }
        merge_field_shapes(&mut summary.ltex_field_shapes, &scan.ltex_field_shapes);
        merge_field_shapes(
            &mut summary.location_field_shapes,
            &scan.location_field_shapes,
        );
        merge_field_shapes(
            &mut summary.cell_subrecord_shapes,
            &scan.cell_subrecord_shapes,
        );
        for (length, count) in &scan.xclc_payload_length_counts {
            *summary
                .xclc_payload_length_counts
                .entry(*length)
                .or_default() += count;
        }
        for (length, count) in &scan.xclw_payload_length_counts {
            *summary
                .xclw_payload_length_counts
                .entry(*length)
                .or_default() += count;
        }
        for (class, count) in &scan.xclw_value_class_counts {
            *summary
                .xclw_value_class_counts
                .entry(class.clone())
                .or_default() += count;
        }
        for (length, count) in &scan.xcll_payload_length_counts {
            *summary
                .xcll_payload_length_counts
                .entry(*length)
                .or_default() += count;
        }
        for (length, count) in &scan.xclr_payload_length_counts {
            *summary
                .xclr_payload_length_counts
                .entry(*length)
                .or_default() += count;
        }
    }
    for occurrence_counts in summary.land_record_field_occurrences.values_mut() {
        let records_with_field: u64 = occurrence_counts.values().sum();
        occurrence_counts.insert(0, summary.land_records.saturating_sub(records_with_field));
    }
    summary
}

fn observe_field_shape(
    shapes: &mut BTreeMap<String, FieldShape>,
    tag: &[u8; 4],
    payload_bytes: usize,
) -> std::result::Result<(), String> {
    let signature = framing::signature(*tag);
    if !shapes.contains_key(&signature) && shapes.len() >= FIELD_SHAPE_TAG_LIMIT {
        return Err("landscape subrecord-shape tag budget exceeded".into());
    }
    let shape = shapes.entry(signature.clone()).or_default();
    if !shape.payload_length_counts.contains_key(&payload_bytes)
        && shape.payload_length_counts.len() >= FIELD_SHAPE_LENGTH_LIMIT
    {
        return Err(format!(
            "landscape subrecord-shape length budget exceeded for {signature}"
        ));
    }
    shape.occurrences += 1;
    *shape
        .payload_length_counts
        .entry(payload_bytes)
        .or_default() += 1;
    Ok(())
}

fn merge_field_shapes(
    destination: &mut BTreeMap<String, FieldShape>,
    source: &BTreeMap<String, FieldShape>,
) {
    for (tag, source_shape) in source {
        let destination_shape = destination.entry(tag.clone()).or_default();
        destination_shape.occurrences += source_shape.occurrences;
        for (length, count) in &source_shape.payload_length_counts {
            *destination_shape
                .payload_length_counts
                .entry(*length)
                .or_default() += count;
        }
    }
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn shared_error(error: impl std::fmt::Display) -> fallout_data::Error {
    fallout_data::Error::Resolution(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((data.len() as u16).to_le_bytes());
        value.extend(data);
        value
    }

    fn record(tag: &[u8; 4], id: u32, fields: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((fields.len() as u32).to_le_bytes());
        value.extend(0u32.to_le_bytes());
        value.extend(id.to_le_bytes());
        value.extend([0, 0, 0, 0, 44, 0, 0, 0]);
        value.extend(fields);
        value
    }

    fn group(kind: i32, label: u32, children: &[u8]) -> Vec<u8> {
        let mut value = b"GRUP".to_vec();
        value.extend((24u32 + children.len() as u32).to_le_bytes());
        value.extend(label.to_le_bytes());
        value.extend(kind.to_le_bytes());
        value.extend([0; 8]);
        value.extend(children);
        value
    }

    fn plugin_header(masters: &[&str], light: bool) -> Vec<u8> {
        let mut fields = field(
            b"HEDR",
            &[1.7f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        for master in masters {
            let mut value = master.as_bytes().to_vec();
            value.push(0);
            fields.extend(field(b"MAST", &value));
            fields.extend(field(b"DATA", &[0; 8]));
        }
        let mut bytes = record(b"TES4", 0, &fields);
        if light {
            bytes[8..12].copy_from_slice(&0x200u32.to_le_bytes());
        }
        bytes
    }

    fn parse_rows(output: &[u8]) -> Vec<serde_json::Value> {
        output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect()
    }

    #[test]
    fn xwem_path_parser_requires_a_data_root_and_c_string_terminator() {
        let normalized =
            cell_water_environment_map_field(7, b"dAtA/Textures/Cubemaps/Sample.dds\0").unwrap();
        assert_eq!(normalized.status, "normalized-asset-path");
        assert_eq!(
            normalized.normalized_data_asset_path,
            Some(b"textures/cubemaps/sample.dds".to_vec())
        );
        assert_eq!(normalized.terminator_offset, Some(33));

        let missing_root =
            cell_water_environment_map_field(0, b"Textures\\Cubemaps\\sample.dds\0").unwrap();
        assert_eq!(missing_root.status, "missing-data-root-prefix");
        assert_eq!(missing_root.normalized_data_asset_path, None);

        let traversal = cell_water_environment_map_field(0, b"Data\\..\\outside.dds\0").unwrap();
        assert_eq!(traversal.status, "invalid-data-asset-path");
        assert_eq!(traversal.normalized_data_asset_path, None);

        let trailing =
            cell_water_environment_map_field(0, b"Data\\Textures\\Cubemaps\\sample.dds\0\x01")
                .unwrap();
        assert_eq!(
            trailing.status,
            "normalized-asset-path-with-nonzero-trailing-bytes"
        );
        assert!(trailing.nonzero_bytes_after_terminator);
        assert!(trailing.normalized_data_asset_path.is_some());

        let missing_terminator =
            cell_water_environment_map_field(0, b"Data\\Textures\\sample").unwrap();
        assert_eq!(missing_terminator.status, "missing-terminator");

        let oversized = vec![b'a'; XWEM_PATH_LIMIT + 1];
        let oversized = cell_water_environment_map_field(0, &oversized).unwrap();
        assert_eq!(oversized.status, "field-over-4096-bytes");
        assert!(oversized.raw_bytes_truncated);
        assert_eq!(oversized.raw_bytes.len(), XWEM_PATH_LIMIT);
    }

    #[test]
    fn resolves_land_ltex_txst_chain_without_selecting_an_override() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("Base.esm");
        let patch = root.path().join("Patch.esp");
        let mut base_bytes = plugin_header(&[], false);
        base_bytes.extend(record(b"TXST", 0x0000_1234, &[]));
        base_bytes.extend(record(b"MATT", 0x0000_4567, &[]));
        base_bytes.extend(record(b"GRAS", 0x0000_3456, &[]));
        base_bytes.extend(record(b"REGN", 0x0000_5555, &[]));
        base_bytes.extend(record(b"REGN", 0x0000_1000, &[]));
        base_bytes.extend(record(b"WATR", 0x0000_1001, &[]));
        base_bytes.extend(record(b"IMGS", 0x0000_1002, &[]));
        base_bytes.extend(record(b"ECZN", 0x0000_1003, &[]));
        base_bytes.extend(record(b"ASPC", 0x0000_1004, &[]));
        base_bytes.extend(record(b"MUSC", 0x0000_1005, &[]));
        base_bytes.extend(record(b"LGTM", 0x0000_1006, &[]));
        let mut parent_location = field(b"PNAM", &0x0000_1009u32.to_le_bytes());
        parent_location.extend(field(b"FULL", b"Synthetic child location\0"));
        base_bytes.extend(record(b"LCTN", 0x0000_1007, &parent_location));
        base_bytes.extend(record(b"LCTN", 0x0000_1008, &field(b"PNAM", &[1, 2, 3])));
        let mut parent_fields = field(b"EDID", b"ParentLocation\0");
        parent_fields.extend(field(b"ZZZZ", &[0xCC]));
        base_bytes.extend(record(b"LCTN", 0x0000_1009, &parent_fields));
        let mut cell_coordinates = Vec::new();
        cell_coordinates.extend(field(b"XCLC", &[12, 0, 0, 0, 0xFC, 0xFF, 0xFF, 0xFF]));
        cell_coordinates.extend(field(b"XCLC", &[13, 0, 0, 0, 0xFC, 0xFF, 0xFF, 0xFF]));
        cell_coordinates.extend(field(b"XCLW", &1.0f32.to_le_bytes()));
        cell_coordinates.extend(field(b"XCLL", &[0xCC; 64]));
        cell_coordinates.extend(field(b"XCLR", &[0x55, 0x55, 0, 0, 0x66, 0x66, 0, 0]));
        cell_coordinates.extend(field(b"XCLR", &[0x55, 0x55, 0, 0]));
        base_bytes.extend(record(b"CELL", 0x0000_0060, &cell_coordinates));
        let mut cell_with_flags = Vec::new();
        cell_with_flags.extend(field(b"XCLC", &[7, 0, 0, 0, 9, 0, 0, 0, 0x0B, 1, 2, 3]));
        cell_with_flags.extend(field(b"XCLW", &0x7FC1_2345u32.to_le_bytes()));
        cell_with_flags.extend(field(b"XCLL", &[0xDD; 92]));
        cell_with_flags.extend(field(b"XCLR", &[0x55, 0x55, 0, 0]));
        base_bytes.extend(record(b"CELL", 0x0000_0061, &cell_with_flags));
        let mut malformed_cell = Vec::new();
        malformed_cell.extend(field(b"XCLC", &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        malformed_cell.extend(field(b"XCLW", &[0xAA; 3]));
        malformed_cell.extend(field(b"XCLL", &[0xEE; 65]));
        malformed_cell.extend(field(b"XCLR", &[0x55, 0x55, 0, 0, 0xAA]));
        malformed_cell.extend(field(b"XCCM", &[0x00, 0x10, 0, 0, 0xAA]));
        malformed_cell.extend(field(b"XWEM", b"missing-terminator"));
        base_bytes.extend(record(b"CELL", 0x0000_0062, &malformed_cell));
        let mut context_fields = Vec::new();
        context_fields.extend(field(b"XCCM", &0x0000_1000u32.to_le_bytes()));
        context_fields.extend(field(b"XCWT", &0x0000_1001u32.to_le_bytes()));
        context_fields.extend(field(b"XCIM", &0x0000_1002u32.to_le_bytes()));
        context_fields.extend(field(b"XEZN", &0x0000_1003u32.to_le_bytes()));
        context_fields.extend(field(b"XCAS", &0x0000_1004u32.to_le_bytes()));
        context_fields.extend(field(b"XCMO", &0x0000_1005u32.to_le_bytes()));
        context_fields.extend(field(b"LTMP", &0x0000_1006u32.to_le_bytes()));
        context_fields.extend(field(b"XLCN", &0x0000_1007u32.to_le_bytes()));
        context_fields.extend(field(b"XWEM", b"Data\\Textures\\Cubemaps\\sample.dds\0"));
        base_bytes.extend(record(b"CELL", 0x0000_0063, &context_fields));
        base_bytes.extend(record(b"WRLD", 0x0000_003C, &[]));
        let mut ltex_fields = field(b"TNAM", &0x0000_1234u32.to_le_bytes());
        ltex_fields.extend(field(b"GNAM", &0x0000_3456u32.to_le_bytes()));
        ltex_fields.extend(field(b"MNAM", &0x0000_4567u32.to_le_bytes()));
        base_bytes.extend(record(b"LTEX", 0x0000_2345, &ltex_fields));
        base_bytes.extend(record(b"LTEX", 0x0000_2345, &ltex_fields));
        fs::write(&base, base_bytes).unwrap();

        let mut patch_bytes = plugin_header(&["Base.esm"], false);
        let mut land_fields = field(b"BTXT", &[0x45, 0x23, 0, 0, 0, 0, 0, 0]);
        land_fields.extend(field(b"ATXT", &[0x45, 0x23, 0, 0, 1, 0, 7, 0]));
        land_fields.extend(field(b"VTXT", &[0; 8]));
        land_fields.extend(field(b"VTXT", &[0; 16]));
        let land = record(b"LAND", 0x0100_0001, &land_fields);
        let cell_group = group(6, 0x0000_0060, &land);
        patch_bytes.extend(group(1, 0x0000_003C, &cell_group));
        patch_bytes.extend(record(b"LAND", 0x0100_0002, &[]));
        fs::write(&patch, patch_bytes).unwrap();

        let sample_texture = root
            .path()
            .join("textures")
            .join("cubemaps")
            .join("sample.dds");
        fs::create_dir_all(sample_texture.parent().unwrap()).unwrap();
        fs::write(&sample_texture, b"synthetic texture placeholder").unwrap();

        let mut output = Vec::new();
        let summary = export_many(root.path(), &[base, patch], &mut output).unwrap();
        assert_eq!(summary.land_records, 2);
        assert_eq!(summary.land_records_with_group_path, 1);
        assert_eq!(summary.land_records_without_group_path, 1);
        assert_eq!(summary.land_group_path_depths.get(&2), Some(&1));
        assert_eq!(summary.ltex_records, 2);
        assert_eq!(summary.txst_records, 1);
        assert_eq!(summary.matt_records, 1);
        assert_eq!(summary.gras_records, 1);
        assert_eq!(summary.cell_records, 4);
        assert_eq!(summary.world_records, 1);
        assert_eq!(summary.cell_subrecords_scanned, 25);
        assert_eq!(summary.regn_records, 2);
        assert_eq!(summary.location_records, 3);
        assert_eq!(summary.location_subrecords_scanned, 5);
        assert_eq!(summary.location_parent_form_links, 1);
        assert_eq!(
            summary.location_parent_links_with_scanned_target_candidates,
            1
        );
        assert_eq!(
            summary.location_parent_links_without_scanned_target_candidates,
            0
        );
        assert_eq!(summary.location_parent_links_unresolved_without_profile, 0);
        assert_eq!(summary.location_field_shapes["PNAM"].occurrences, 2);
        assert_eq!(
            summary.location_field_shapes["PNAM"].payload_length_counts[&4],
            1
        );
        assert_eq!(
            summary.location_field_shapes["PNAM"].payload_length_counts[&3],
            1
        );
        assert_eq!(summary.location_field_shapes["EDID"].occurrences, 1);
        assert_eq!(summary.location_field_shapes["ZZZZ"].occurrences, 1);
        assert_eq!(summary.unknown_subrecords, 1);
        assert_eq!(summary.cell_water_environment_map_fields, 2);
        assert_eq!(summary.cell_water_environment_map_paths_normalized, 1);
        assert_eq!(summary.malformed_cell_water_environment_map_fields, 1);
        assert_eq!(
            summary.cell_water_environment_map_references_with_loose_file,
            1
        );
        assert_eq!(
            summary.cell_water_environment_map_references_with_archive_member,
            0
        );
        assert_eq!(
            summary.cell_water_environment_map_references_without_candidate_match,
            0
        );
        assert_eq!(summary.unique_cell_water_environment_map_lookup_paths, 1);
        assert_eq!(summary.cell_context_form_links, 8);
        assert_eq!(summary.cell_context_form_links_by_tag.len(), 8);
        for tag in [
            "XCCM", "XCWT", "XCIM", "XEZN", "XCAS", "XCMO", "LTMP", "XLCN",
        ] {
            assert_eq!(summary.cell_context_form_links_by_tag[tag], 1);
        }
        assert_eq!(summary.cell_context_links_with_scanned_target_candidates, 8);
        assert_eq!(
            summary.cell_context_links_without_scanned_target_candidates,
            0
        );
        assert_eq!(summary.cell_context_links_unresolved_without_profile, 0);
        assert_eq!(summary.malformed_link_payloads, 2);
        assert_eq!(summary.cell_records_with_xclr, 3);
        assert_eq!(summary.cell_records_without_xclr, 1);
        assert_eq!(summary.cell_records_with_multiple_xclr, 1);
        assert_eq!(summary.cell_xclr_fields, 4);
        assert_eq!(summary.malformed_xclr_fields, 1);
        assert_eq!(summary.xclr_payload_length_counts.get(&8), Some(&1));
        assert_eq!(summary.xclr_payload_length_counts.get(&4), Some(&2));
        assert_eq!(summary.xclr_payload_length_counts.get(&5), Some(&1));
        assert_eq!(summary.cell_region_links, 5);
        assert_eq!(summary.cell_region_links_with_scanned_target_candidates, 4);
        assert_eq!(
            summary.cell_region_links_without_scanned_target_candidates,
            1
        );
        assert_eq!(summary.cell_records_with_xclc, 3);
        assert_eq!(summary.cell_records_without_xclc, 1);
        assert_eq!(summary.cell_records_with_multiple_xclc, 1);
        assert_eq!(summary.malformed_xclc_fields, 1);
        assert_eq!(summary.xclc_payload_length_counts.get(&8), Some(&2));
        assert_eq!(summary.xclc_payload_length_counts.get(&12), Some(&1));
        assert_eq!(summary.xclc_payload_length_counts.get(&9), Some(&1));
        assert_eq!(summary.cell_records_with_xclw, 3);
        assert_eq!(summary.cell_records_without_xclw, 1);
        assert_eq!(summary.cell_records_with_multiple_xclw, 0);
        assert_eq!(summary.malformed_xclw_fields, 1);
        assert_eq!(summary.xclw_payload_length_counts.get(&4), Some(&2));
        assert_eq!(summary.xclw_payload_length_counts.get(&3), Some(&1));
        assert_eq!(summary.xclw_value_class_counts["finite"], 1);
        assert_eq!(summary.xclw_value_class_counts["nan"], 1);
        assert_eq!(summary.cell_records_with_xcll, 3);
        assert_eq!(summary.cell_records_without_xcll, 1);
        assert_eq!(summary.cell_records_with_multiple_xcll, 0);
        assert_eq!(summary.unprofiled_xcll_fields, 1);
        assert_eq!(summary.xcll_payload_length_counts.get(&64), Some(&1));
        assert_eq!(summary.xcll_payload_length_counts.get(&92), Some(&1));
        assert_eq!(summary.xcll_payload_length_counts.get(&65), Some(&1));
        assert_eq!(summary.cell_subrecord_shapes["XCLC"].occurrences, 4);
        assert_eq!(summary.cell_subrecord_shapes["XCLW"].occurrences, 3);
        assert_eq!(summary.cell_subrecord_shapes["XCLL"].occurrences, 3);
        assert_eq!(summary.cell_subrecord_shapes["XLCN"].occurrences, 1);
        assert_eq!(summary.cell_subrecord_shapes["XWEM"].occurrences, 2);
        assert_eq!(
            summary.cell_subrecord_shapes["XCLC"]
                .payload_length_counts
                .get(&8),
            Some(&2)
        );
        assert_eq!(summary.land_records_with_cell_group_label, 1);
        assert_eq!(summary.land_cell_group_labels_resolved, 1);
        assert_eq!(summary.land_cell_groups_with_candidates, 1);
        assert_eq!(summary.land_cell_groups_without_candidates, 0);
        assert_eq!(
            summary.land_cell_candidate_count_distribution.get(&1),
            Some(&1)
        );
        assert_eq!(summary.land_records_with_world_group_label, 1);
        assert_eq!(summary.land_world_group_labels_resolved, 1);
        assert_eq!(summary.land_world_groups_with_candidates, 1);
        assert_eq!(summary.land_world_groups_without_candidates, 0);
        assert_eq!(
            summary.land_world_candidate_count_distribution.get(&1),
            Some(&1)
        );
        assert_eq!(summary.landscape_base_layers, 1);
        assert_eq!(summary.landscape_alpha_layers, 1);
        assert_eq!(summary.links_resolved_to_source_keys, 22);
        assert_eq!(summary.links_with_scanned_target_candidates, 21);
        assert_eq!(summary.links_without_scanned_target_candidates, 1);
        assert_eq!(summary.ltex_texture_set_fields, 2);
        assert_eq!(summary.ltex_material_fields, 2);
        assert_eq!(summary.ltex_grass_fields, 2);
        assert_eq!(summary.land_field_shapes["BTXT"].occurrences, 1);
        assert_eq!(
            summary.land_field_shapes["BTXT"]
                .payload_length_counts
                .get(&8),
            Some(&1)
        );
        assert_eq!(
            summary.land_record_field_occurrences["BTXT"].get(&0),
            Some(&1)
        );
        assert_eq!(
            summary.land_record_field_occurrences["BTXT"].get(&1),
            Some(&1)
        );
        assert_eq!(summary.land_field_shapes["VTXT"].occurrences, 2);
        assert_eq!(
            summary.land_field_shapes["VTXT"]
                .payload_length_counts
                .get(&8),
            Some(&1)
        );
        assert_eq!(
            summary.land_field_shapes["VTXT"]
                .payload_length_counts
                .get(&16),
            Some(&1)
        );
        assert_eq!(
            summary.land_record_field_occurrences["VTXT"].get(&0),
            Some(&1)
        );
        assert_eq!(
            summary.land_record_field_occurrences["VTXT"].get(&2),
            Some(&1)
        );
        assert_eq!(summary.ltex_field_shapes["TNAM"].occurrences, 2);
        assert_eq!(
            summary.ltex_field_shapes["TNAM"]
                .payload_length_counts
                .get(&4),
            Some(&2)
        );

        let rows = parse_rows(&output);
        let cell_60 = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "CELL"
                    && row["record"]["form_id"] == 0x60
            })
            .unwrap();
        let xclc = &cell_60["record"]["cell_grid_fields"][0];
        assert_eq!(xclc["shape_status"], "coordinates-only");
        assert_eq!(xclc["x"], 12);
        assert_eq!(xclc["y"], -4);
        assert_eq!(xclc["payload_bytes"], 8);
        assert_eq!(xclc["raw_bytes"].as_array().unwrap().len(), 8);
        assert_eq!(
            cell_60["record"]["cell_grid_fields"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let xclw = &cell_60["record"]["cell_water_height_fields"][0];
        assert_eq!(xclw["shape_status"], "ieee754-single-precision");
        assert_eq!(xclw["raw_bits_hex"], "0x3F800000");
        assert_eq!(xclw["ieee754_class"], "finite");
        assert_eq!(xclw["finite_value"], 1.0);
        assert_eq!(xclw["sha256"], field_hash(&[0, 0, 0x80, 0x3F]).unwrap());
        let xcll = &cell_60["record"]["cell_lighting_fields"][0];
        assert_eq!(xcll["classification"], "known-corpus-size-unparsed");
        assert_eq!(xcll["payload_bytes"], 64);
        assert_eq!(
            xcll["raw_bytes"].as_array().unwrap().len(),
            RAW_EVIDENCE_LIMIT
        );
        assert_eq!(xcll["raw_bytes_truncated"], true);
        assert_eq!(xcll["sha256"], field_hash(&[0xCC; 64]).unwrap());
        let xclr = &cell_60["record"]["cell_region_fields"][0];
        assert_eq!(xclr["payload_bytes"], 8);
        assert_eq!(
            xclr["sha256"],
            field_hash(&[0x55, 0x55, 0, 0, 0x66, 0x66, 0, 0]).unwrap()
        );
        assert_eq!(xclr["links"].as_array().unwrap().len(), 2);
        assert_eq!(xclr["links"][0]["target_kind"], "REGN");
        assert_eq!(xclr["links"][0]["status"], "scanned-target-candidates");
        assert_eq!(xclr["links"][0]["scanned_target_record_count"], 1);
        assert_eq!(
            xclr["links"][1]["status"],
            "source-key-resolved-no-scanned-target"
        );
        assert_eq!(
            cell_60["record"]["cell_region_fields"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let cell_61 = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "CELL"
                    && row["record"]["form_id"] == 0x61
            })
            .unwrap();
        let xclc = &cell_61["record"]["cell_grid_fields"][0];
        assert_eq!(xclc["shape_status"], "coordinates-and-land-flags");
        assert_eq!(xclc["x"], 7);
        assert_eq!(xclc["y"], 9);
        assert_eq!(xclc["land_flags"], 0x0B);
        assert_eq!(xclc["reserved_bytes"], serde_json::json!([1, 2, 3]));
        let xclw = &cell_61["record"]["cell_water_height_fields"][0];
        assert_eq!(xclw["raw_bits_hex"], "0x7FC12345");
        assert_eq!(xclw["ieee754_class"], "nan");
        assert_eq!(xclw["finite_value"], serde_json::Value::Null);
        assert_eq!(
            xclw["raw_bytes"],
            serde_json::json!([0x45, 0x23, 0xC1, 0x7F])
        );
        assert_eq!(
            xclw["sha256"],
            field_hash(&[0x45, 0x23, 0xC1, 0x7F]).unwrap()
        );
        let xcll = &cell_61["record"]["cell_lighting_fields"][0];
        assert_eq!(xcll["classification"], "known-corpus-size-unparsed");
        assert_eq!(xcll["payload_bytes"], 92);
        assert_eq!(xcll["raw_bytes_truncated"], true);
        let malformed = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "CELL"
                    && row["record"]["form_id"] == 0x62
            })
            .unwrap();
        let xclc = &malformed["record"]["cell_grid_fields"][0];
        assert_eq!(xclc["shape_status"], "unsupported-payload-length");
        assert_eq!(xclc["x"], serde_json::Value::Null);
        assert_eq!(xclc["payload_bytes"], 9);
        assert_eq!(xclc["raw_bytes"].as_array().unwrap().len(), 9);
        assert_eq!(
            xclc["sha256"],
            field_hash(&[1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap()
        );
        let xclw = &malformed["record"]["cell_water_height_fields"][0];
        assert_eq!(xclw["shape_status"], "unsupported-payload-length");
        assert_eq!(xclw["raw_bits"], serde_json::Value::Null);
        assert_eq!(xclw["payload_bytes"], 3);
        assert_eq!(xclw["raw_bytes"], serde_json::json!([0xAA, 0xAA, 0xAA]));
        assert_eq!(xclw["raw_bytes_truncated"], false);
        let xcll = &malformed["record"]["cell_lighting_fields"][0];
        assert_eq!(xcll["classification"], "unprofiled-size-unparsed");
        assert_eq!(xcll["payload_bytes"], 65);
        assert_eq!(
            xcll["raw_bytes"].as_array().unwrap().len(),
            RAW_EVIDENCE_LIMIT
        );
        assert_eq!(xcll["raw_bytes_truncated"], true);
        let xclr = &malformed["record"]["cell_region_fields"][0];
        assert_eq!(xclr["payload_bytes"], 5);
        assert_eq!(xclr["links"].as_array().unwrap().len(), 1);
        assert_eq!(
            xclr["trailing_bytes"]["payload_offset_in_decoded_record"],
            xclr["payload_offset_in_decoded_record"].as_u64().unwrap() + 4
        );
        assert_eq!(xclr["trailing_bytes"]["payload_bytes"], 1);
        assert_eq!(
            xclr["trailing_bytes"]["raw_bytes"],
            serde_json::json!([0xAA])
        );
        assert_eq!(
            xclr["trailing_bytes"]["classification"],
            "incomplete-cell-region-form-id"
        );
        let context_cell = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "CELL"
                    && row["record"]["form_id"] == 0x63
            })
            .unwrap();
        assert_eq!(
            context_cell["record"]["form_links"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(context_cell["record"]["form_links"][0]["tag"], "XCCM");
        assert_eq!(
            context_cell["record"]["form_links"][0]["target_kind"],
            "REGN"
        );
        assert_eq!(
            context_cell["record"]["form_links"][0]["status"],
            "scanned-target-candidates"
        );
        assert_eq!(
            context_cell["record"]["form_links"][0]["scanned_target_record_count"],
            1
        );
        assert_eq!(context_cell["record"]["form_links"][7]["tag"], "XLCN");
        assert_eq!(
            context_cell["record"]["form_links"][7]["target_kind"],
            "LCTN"
        );
        assert_eq!(
            context_cell["record"]["form_links"][7]["scanned_target_record_count"],
            1
        );
        let child_location = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "LCTN"
                    && row["record"]["form_id"] == 0x1007
            })
            .unwrap();
        assert_eq!(
            child_location["record"]["form_links"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(child_location["record"]["form_links"][0]["tag"], "PNAM");
        assert_eq!(
            child_location["record"]["form_links"][0]["target_kind"],
            "LCTN"
        );
        assert_eq!(
            child_location["record"]["form_links"][0]["status"],
            "scanned-target-candidates"
        );
        assert_eq!(
            child_location["record"]["form_links"][0]["scanned_target_record_count"],
            1
        );
        assert_eq!(
            child_location["record"]["opaque_subrecords"][0]["classification"],
            "known-location-field-not-decoded"
        );
        let parent_location = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "LCTN"
                    && row["record"]["form_id"] == 0x1009
            })
            .unwrap();
        assert_eq!(
            parent_location["record"]["opaque_subrecords"][0]["classification"],
            "known-location-field-not-decoded"
        );
        assert_eq!(
            parent_location["record"]["opaque_subrecords"][1]["classification"],
            "unrecognized-location-subrecord"
        );
        let malformed_location = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "LCTN"
                    && row["record"]["form_id"] == 0x1008
            })
            .unwrap();
        assert_eq!(
            malformed_location["record"]["opaque_subrecords"][0]["classification"],
            "malformed-location-parent-form-id"
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["tag"],
            "XWEM"
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["status"],
            "normalized-asset-path"
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["raw_bytes"],
            serde_json::json!(b"Data\\Textures\\Cubemaps\\sample.dds\0".to_vec())
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["terminator_offset"],
            33
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["normalized_data_asset_path"],
            serde_json::json!(b"textures/cubemaps/sample.dds")
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["status"],
            "normalized-asset-path"
        );
        assert_eq!(
            context_cell["record"]["cell_water_environment_map_fields"][0]["lookup"]["loose_file_status"],
            "present"
        );
        let malformed_context = rows
            .iter()
            .find(|row| {
                row["type"] == "landscape-record"
                    && row["record"]["record_kind"] == "CELL"
                    && row["record"]["form_id"] == 0x62
            })
            .unwrap();
        let malformed_field = malformed_context["record"]["opaque_subrecords"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["tag"] == "XCCM")
            .unwrap();
        assert_eq!(
            malformed_field["classification"],
            "malformed-cell-context-form-id"
        );
        assert_eq!(malformed_field["payload_bytes"], 5);
        assert_eq!(
            malformed_field["raw_bytes"],
            serde_json::json!([0, 16, 0, 0, 0xAA])
        );
        let malformed_xwem =
            malformed_context["record"]["cell_water_environment_map_fields"][0].clone();
        assert_eq!(malformed_xwem["status"], "missing-terminator");
        assert_eq!(malformed_xwem["terminator_offset"], serde_json::Value::Null);
        assert_eq!(
            malformed_xwem["normalized_data_asset_path"],
            serde_json::Value::Null
        );
        let land = rows
            .iter()
            .find(|row| row["type"] == "landscape-record" && row["record"]["record_kind"] == "LAND")
            .unwrap();
        assert_eq!(land["record"]["layers"].as_array().unwrap().len(), 2);
        assert_eq!(land["record"]["group_path"].as_array().unwrap().len(), 2);
        assert_eq!(land["record"]["group_path"][0]["kind"], 1);
        assert_eq!(
            land["record"]["group_path"][0]["label"],
            serde_json::json!([60, 0, 0, 0])
        );
        assert_eq!(land["record"]["group_path"][1]["kind"], 6);
        assert_eq!(land["record"]["containing_cell_raw"], 0x60);
        assert_eq!(
            land["record"]["containing_cell"]["origin_plugin"],
            "base.esm"
        );
        assert_eq!(land["record"]["containing_cell"]["local_id"], 0x60);
        assert_eq!(land["record"]["containing_cell_candidate_count"], 1);
        assert_eq!(land["record"]["containing_world_raw"], 0x3C);
        assert_eq!(
            land["record"]["containing_world"]["origin_plugin"],
            "base.esm"
        );
        assert_eq!(land["record"]["containing_world"]["local_id"], 0x3C);
        assert_eq!(land["record"]["containing_world_candidate_count"], 1);
        assert_eq!(land["record"]["layers"][0]["field_payload_bytes"], 8);
        assert_eq!(
            land["record"]["layers"][0]["field_sha256"],
            field_hash(&[0x45, 0x23, 0, 0, 0, 0, 0, 0]).unwrap()
        );
        for layer in land["record"]["layers"].as_array().unwrap() {
            assert_eq!(layer["texture"]["status"], "scanned-target-candidates");
            assert_eq!(layer["texture"]["scanned_target_record_count"], 2);
        }
        let links = rows
            .iter()
            .filter(|row| {
                row["type"] == "landscape-record" && row["record"]["record_kind"] == "LTEX"
            })
            .flat_map(|row| row["record"]["form_links"].as_array().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(links[0]["target_kind"], "TXST");
        assert_eq!(links[0]["field_payload_bytes"], 4);
        assert_eq!(links[0]["scanned_target_record_count"], 1);
        assert_eq!(rows.last().unwrap()["type"], "complete");
    }

    #[test]
    fn preserves_malformed_and_opaque_landscape_payloads_and_light_links() {
        let root = tempfile::tempdir().unwrap();
        let light = root.path().join("Light.esl");
        let mut bytes = plugin_header(&[], true);
        let mut fields = field(b"BTXT", &[1, 2, 3]);
        fields.extend(field(b"ATXT", &[0x01, 0x08, 0, 0xFE, 4, 9, 1, 0]));
        fields.extend(field(b"VTXT", &[0xAA; 4]));
        bytes.extend(record(b"LAND", 0xFE00_0801, &fields));
        let mut ltex = field(b"TNAM", &0xFE00_0801u32.to_le_bytes());
        ltex.extend(field(b"MNAM", &[1, 2]));
        ltex.extend(field(b"ZZZZ", &[0xCC; 1_030]));
        bytes.extend(record(b"LTEX", 0xFE00_0802, &ltex));
        fs::write(&light, bytes).unwrap();

        let mut output = Vec::new();
        let summary = export_many(root.path(), &[light], &mut output).unwrap();
        assert_eq!(summary.malformed_layer_payloads, 1);
        assert_eq!(summary.malformed_link_payloads, 1);
        assert_eq!(summary.light_links_requiring_profile, 2);
        assert_eq!(summary.out_of_range_quadrants, 1);
        assert_eq!(summary.nonzero_auxiliary_layer_bytes, 1);
        assert_eq!(summary.unknown_subrecords, 1);

        let rows = parse_rows(&output);
        let land = rows
            .iter()
            .find(|row| row["type"] == "landscape-record" && row["record"]["record_kind"] == "LAND")
            .unwrap();
        assert_eq!(land["record"]["source_key"]["origin_plugin"], "light.esl");
        assert_eq!(
            land["record"]["layers"][0]["texture"]["status"],
            "light-reference-needs-active-profile"
        );
        let ltex = rows
            .iter()
            .find(|row| row["type"] == "landscape-record" && row["record"]["record_kind"] == "LTEX")
            .unwrap();
        let unknown = ltex["record"]["opaque_subrecords"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["tag"] == "ZZZZ")
            .unwrap();
        assert_eq!(
            unknown["raw_bytes"].as_array().unwrap().len(),
            RAW_EVIDENCE_LIMIT
        );
        assert_eq!(unknown["raw_bytes_truncated"], true);
    }

    #[test]
    fn a_failed_writer_does_not_report_completion() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic write failure"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Empty.esp");
        fs::write(&source, plugin_header(&[], false)).unwrap();
        assert!(export_many(root.path(), &[source], &mut BrokenWriter).is_err());
    }
}
