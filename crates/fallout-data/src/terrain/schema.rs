use crate::{
    Result, malformed,
    plugin::{self, Record, Subrecord},
    world::{SourceField, terminated},
};
use serde::Serialize;
use std::mem::size_of;

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct RawField {
    pub kind: String,
    pub decoded_offset: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Default)]
pub struct Worldspace {
    pub editor_id: Option<SourceField<Vec<u8>>>,
    pub full_name: Option<SourceField<Vec<u8>>>,
    pub flags: Option<SourceField<u8>>,
    pub parent: Option<SourceField<u32>>,
    pub parent_flags: Option<SourceField<u16>>,
    pub climate: Option<SourceField<u32>>,
    pub water: Option<SourceField<u32>>,
    pub lod_water: Option<SourceField<u32>>,
    pub lod_water_height_bits: Option<SourceField<u32>>,
    pub default_height_bits: Option<SourceField<[u32; 2]>>,
    pub image_space: Option<SourceField<u32>>,
    pub encounter_zone: Option<SourceField<u32>>,
    pub music: Option<SourceField<u32>>,
    pub unhandled: Vec<RawField>,
}

#[derive(Debug, Serialize, Default)]
pub struct CellFields {
    pub editor_id: Option<SourceField<Vec<u8>>>,
    pub full_name: Option<SourceField<Vec<u8>>>,
    pub flags: Option<SourceField<u8>>,
    pub grid: Option<SourceField<[i32; 2]>>,
    pub quadrant_flags: Option<SourceField<u32>>,
    pub unhandled: Vec<RawField>,
}

#[derive(Debug, Serialize)]
pub struct HeightMap {
    pub offset_bits: u32,
    pub deltas: Vec<i8>,
    pub unused: [u8; 3],
}

#[derive(Debug, Serialize)]
pub struct AlphaVertex {
    pub position: u16,
    pub unused: [u8; 2],
    pub opacity_bits: u32,
}

#[derive(Debug, Serialize)]
pub struct Layer {
    pub kind: String,
    pub decoded_offset: usize,
    pub texture_raw: u32,
    pub quadrant: u8,
    pub unused: u8,
    pub layer: i16,
    pub alpha: Option<SourceField<Vec<AlphaVertex>>>,
}

#[derive(Debug, Serialize, Default)]
pub struct Landscape {
    pub flags: Option<SourceField<u32>>,
    pub normals: Option<SourceField<Vec<[u8; 3]>>>,
    pub heights: Option<SourceField<HeightMap>>,
    pub colors: Option<SourceField<Vec<[u8; 3]>>>,
    pub layers: Vec<Layer>,
    pub unhandled: Vec<RawField>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Fields {
    World(Worldspace),
    Cell(CellFields),
    Land(Landscape),
}

fn size(sub: &Subrecord<'_>, expected: usize, record: &Record, name: &str) -> Result<()> {
    if sub.data.len() != expected {
        return Err(malformed(
            name,
            record.header.offset,
            format!(
                "{} at decoded +0x{:X}: expected {expected} bytes, got {}",
                plugin::signature(sub.kind),
                sub.payload_offset,
                sub.data.len()
            ),
        ));
    }
    Ok(())
}

fn one<T>(
    slot: &mut Option<SourceField<T>>,
    sub: &Subrecord<'_>,
    value: T,
    record: &Record,
    name: &str,
) -> Result<()> {
    if slot.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            format!("duplicate {}", plugin::signature(sub.kind)),
        ));
    }
    *slot = Some(SourceField {
        decoded_offset: sub.payload_offset,
        value,
    });
    Ok(())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("field length checked"))
}

fn float_bits(bytes: &[u8], at: usize, record: &Record, name: &str) -> Result<u32> {
    let bits = u32_at(bytes, at);
    if !f32::from_bits(bits).is_finite() {
        return Err(malformed(
            name,
            record.header.offset,
            "non-finite terrain field",
        ));
    }
    Ok(bits)
}

fn raw(sub: &Subrecord<'_>) -> RawField {
    RawField {
        kind: plugin::signature(sub.kind),
        decoded_offset: sub.payload_offset,
        bytes: sub.data.to_vec(),
    }
}

fn string(
    slot: &mut Option<SourceField<Vec<u8>>>,
    sub: &Subrecord<'_>,
    record: &Record,
    name: &str,
) -> Result<()> {
    one(
        slot,
        sub,
        terminated(sub.data, name, record.header.offset)?.to_vec(),
        record,
        name,
    )
}

/// Layouts are selected FNV definitions from xEdit 9fb016884bec138ea6c7b872cec831537d464c3e.
/// Float bit patterns, padding and unknown fields are retained for exact comparisons.
/// No editor defaults, parent-world inheritance or coordinate conversion is applied.
pub fn decode(record: &Record, name: &str) -> Result<Fields> {
    if record.integrity_issue.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted terrain payload",
        ));
    }
    if record.payload.len() > MAX_BODY_BYTES {
        return Err(malformed(
            name,
            record.header.offset,
            "terrain input budget exceeded",
        ));
    }
    // Charge payload elements plus conservative per-field storage before building
    // vectors. Many tiny fields must not turn a bounded body into huge metadata.
    // This estimate excludes allocator overhead and is not a peak-memory promise.
    let field_storage = 64 + 2 * size_of::<Layer>().max(size_of::<RawField>());
    let mut retained = size_of::<Fields>();
    plugin::visit_subrecords(record, name, |sub| {
        retained = retained
            .checked_add(sub.data.len())
            .and_then(|n| n.checked_add(field_storage))
            .ok_or_else(|| {
                malformed(
                    name,
                    record.header.offset,
                    "terrain storage estimate overflow",
                )
            })?;
        if retained > MAX_RETAINED_BYTES {
            return Err(malformed(
                name,
                record.header.offset,
                "terrain retained-field budget exceeded",
            ));
        }
        Ok(())
    })?;
    match &record.header.kind {
        b"WRLD" => world(record, name).map(Fields::World),
        b"CELL" => cell(record, name).map(Fields::Cell),
        b"LAND" => land(record, name).map(Fields::Land),
        _ => Err(malformed(
            name,
            record.header.offset,
            "expected WRLD, CELL or LAND",
        )),
    }
}

fn world(record: &Record, name: &str) -> Result<Worldspace> {
    let mut out = Worldspace::default();
    plugin::visit_subrecords(record, name, |sub| {
        let link = match &sub.kind {
            b"WNAM" => Some(&mut out.parent),
            b"CNAM" => Some(&mut out.climate),
            b"NAM2" => Some(&mut out.water),
            b"NAM3" => Some(&mut out.lod_water),
            b"INAM" => Some(&mut out.image_space),
            b"XEZN" => Some(&mut out.encounter_zone),
            b"ZNAM" => Some(&mut out.music),
            _ => None,
        };
        if let Some(slot) = link {
            size(&sub, 4, record, name)?;
            return one(slot, &sub, u32_at(sub.data, 0), record, name);
        }
        match &sub.kind {
            b"EDID" => string(&mut out.editor_id, &sub, record, name)?,
            b"FULL" => string(&mut out.full_name, &sub, record, name)?,
            b"DATA" => {
                size(&sub, 1, record, name)?;
                one(&mut out.flags, &sub, sub.data[0], record, name)?;
            }
            b"PNAM" => {
                size(&sub, 2, record, name)?;
                one(
                    &mut out.parent_flags,
                    &sub,
                    u16::from_le_bytes(sub.data.try_into().expect("two bytes")),
                    record,
                    name,
                )?;
            }
            b"NAM4" => {
                size(&sub, 4, record, name)?;
                one(
                    &mut out.lod_water_height_bits,
                    &sub,
                    float_bits(sub.data, 0, record, name)?,
                    record,
                    name,
                )?;
            }
            b"DNAM" => {
                size(&sub, 8, record, name)?;
                one(
                    &mut out.default_height_bits,
                    &sub,
                    [
                        float_bits(sub.data, 0, record, name)?,
                        float_bits(sub.data, 4, record, name)?,
                    ],
                    record,
                    name,
                )?;
            }
            _ => out.unhandled.push(raw(&sub)),
        }
        Ok(())
    })?;
    Ok(out)
}

fn cell(record: &Record, name: &str) -> Result<CellFields> {
    let mut out = CellFields::default();
    plugin::visit_subrecords(record, name, |sub| {
        match &sub.kind {
            b"EDID" => string(&mut out.editor_id, &sub, record, name)?,
            b"FULL" => string(&mut out.full_name, &sub, record, name)?,
            b"DATA" => {
                size(&sub, 1, record, name)?;
                one(&mut out.flags, &sub, sub.data[0], record, name)?;
            }
            b"XCLC" => {
                if ![8, 12].contains(&sub.data.len()) {
                    return Err(malformed(name, record.header.offset, "invalid XCLC size"));
                }
                one(
                    &mut out.grid,
                    &sub,
                    [u32_at(sub.data, 0) as i32, u32_at(sub.data, 4) as i32],
                    record,
                    name,
                )?;
                if sub.data.len() == 12 {
                    one(
                        &mut out.quadrant_flags,
                        &sub,
                        u32_at(sub.data, 8),
                        record,
                        name,
                    )?;
                }
            }
            _ => out.unhandled.push(raw(&sub)),
        }
        Ok(())
    })?;
    Ok(out)
}

fn land(record: &Record, name: &str) -> Result<Landscape> {
    let mut out = Landscape::default();
    plugin::visit_subrecords(record, name, |sub| {
        match &sub.kind {
            b"DATA" => {
                size(&sub, 4, record, name)?;
                one(&mut out.flags, &sub, u32_at(sub.data, 0), record, name)?;
            }
            b"VNML" | b"VCLR" => {
                size(&sub, 33 * 33 * 3, record, name)?;
                let values = sub.data.as_chunks::<3>().0.to_vec();
                let slot = if sub.kind == *b"VNML" {
                    &mut out.normals
                } else {
                    &mut out.colors
                };
                one(slot, &sub, values, record, name)?;
            }
            b"VHGT" => {
                size(&sub, 4 + 33 * 33 + 3, record, name)?;
                let heights = HeightMap {
                    offset_bits: float_bits(sub.data, 0, record, name)?,
                    deltas: sub.data[4..1093].iter().map(|v| *v as i8).collect(),
                    unused: sub.data[1093..].try_into().expect("three bytes"),
                };
                one(&mut out.heights, &sub, heights, record, name)?;
            }
            b"BTXT" | b"ATXT" => {
                size(&sub, 8, record, name)?;
                if sub.data[4] > 3 {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        "invalid LAND quadrant",
                    ));
                }
                out.layers.push(Layer {
                    kind: plugin::signature(sub.kind),
                    decoded_offset: sub.payload_offset,
                    texture_raw: u32_at(sub.data, 0),
                    quadrant: sub.data[4],
                    unused: sub.data[5],
                    layer: i16::from_le_bytes([sub.data[6], sub.data[7]]),
                    alpha: None,
                });
            }
            b"VTXT" => {
                let layer = out
                    .layers
                    .last_mut()
                    .filter(|layer| layer.kind == "ATXT")
                    .ok_or_else(|| {
                        malformed(name, record.header.offset, "VTXT lacks an ATXT header")
                    })?;
                if !sub.data.len().is_multiple_of(8) {
                    return Err(malformed(name, record.header.offset, "partial VTXT entry"));
                }
                let mut vertices = Vec::with_capacity(sub.data.len() / 8);
                for bytes in sub.data.as_chunks::<8>().0 {
                    let position = u16::from_le_bytes([bytes[0], bytes[1]]);
                    if position > 288 {
                        return Err(malformed(
                            name,
                            record.header.offset,
                            "VTXT position outside 17 by 17 quadrant",
                        ));
                    }
                    vertices.push(AlphaVertex {
                        position,
                        unused: [bytes[2], bytes[3]],
                        opacity_bits: float_bits(bytes, 4, record, name)?,
                    });
                }
                one(&mut layer.alpha, &sub, vertices, record, name)?;
            }
            _ => out.unhandled.push(raw(&sub)),
        }
        Ok(())
    })?;
    Ok(out)
}
