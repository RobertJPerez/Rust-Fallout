use super::Partition;
use crate::{Error, Result, nif_scene::cursor::Reader};

fn flag(reader: &mut Reader<'_>) -> Result<u8> {
    let value = reader.u8()?;
    if value > 1 {
        return Err(Error::Unsupported(format!(
            "{} at 0x{:X}: noncanonical partition presence byte {value}",
            reader.source,
            reader.base + reader.position - 1
        )));
    }
    Ok(value)
}

fn u16s(reader: &mut Reader<'_>, count: usize) -> Result<Vec<u16>> {
    reader.budget(count, 2)?;
    reader.reserve::<u16>(count)?;
    (0..count).map(|_| reader.u16()).collect()
}

fn width(reader: &Reader<'_>, vertices: u16, width: u16, present: u8) -> Result<usize> {
    if present == 0 {
        return Ok(0);
    }
    if vertices != 0 && width != 4 {
        return Err(Error::Unsupported(format!(
            "{} at 0x{:X}: nonempty partition arrays with width {width}; independently verified native branch requires four",
            reader.source,
            reader.base + reader.position
        )));
    }
    usize::from(vertices)
        .checked_mul(usize::from(width))
        .ok_or_else(|| reader.fail("partition array product overflow"))
}

pub(super) fn partitions(reader: &mut Reader<'_>) -> Result<Vec<Partition>> {
    let count = reader.u32()? as usize;
    // Five ushort counts plus four presence bytes, even when all arrays are empty.
    reader.budget(count, 14)?;
    reader.reserve::<Partition>(count)?;
    let mut partitions = Vec::with_capacity(count);
    for _ in 0..count {
        let num_vertices = reader.u16()?;
        let num_triangles = reader.u16()?;
        let num_bones = reader.u16()?;
        let num_strips = reader.u16()?;
        let weights_per_vertex = reader.u16()?;
        let bone_palette = u16s(reader, usize::from(num_bones))?;
        let has_vertex_map = flag(reader)?;
        let vertex_map = u16s(
            reader,
            if has_vertex_map == 0 {
                0
            } else {
                usize::from(num_vertices)
            },
        )?;
        let has_vertex_weights = flag(reader)?;
        let weights = width(reader, num_vertices, weights_per_vertex, has_vertex_weights)?;
        reader.budget(weights, 4)?;
        reader.reserve::<u32>(weights)?;
        let weight_bits = (0..weights)
            .map(|_| reader.float().map(f32::to_bits))
            .collect::<Result<Vec<_>>>()?;
        let strip_lengths = u16s(reader, usize::from(num_strips))?;
        let has_faces = flag(reader)?;
        let mut strips = Vec::new();
        let mut triangles = Vec::new();
        if has_faces != 0 {
            if num_strips != 0 {
                let indices = strip_lengths
                    .iter()
                    .try_fold(0usize, |sum, &length| sum.checked_add(usize::from(length)))
                    .ok_or_else(|| reader.fail("partition strip count overflow"))?;
                reader.budget(indices, 2)?;
                reader.reserve::<Vec<u16>>(strip_lengths.len())?;
                for &length in &strip_lengths {
                    strips.push(reader.indices(usize::from(length), num_vertices)?);
                }
            } else {
                reader.budget(usize::from(num_triangles), 6)?;
                reader.reserve::<[u16; 3]>(usize::from(num_triangles))?;
                for _ in 0..num_triangles {
                    let triangle = [reader.u16()?, reader.u16()?, reader.u16()?];
                    if triangle.iter().any(|&index| index >= num_vertices) {
                        return Err(reader.fail("partition triangle index outside local vertices"));
                    }
                    triangles.push(triangle);
                }
            }
        }
        let has_bone_indices = flag(reader)?;
        let indices = width(reader, num_vertices, weights_per_vertex, has_bone_indices)?;
        reader.budget(indices, 1)?;
        reader.reserve::<u8>(indices)?;
        let bone_indices = (0..indices)
            .map(|_| {
                let value = reader.u8()?;
                if u16::from(value) >= num_bones {
                    return Err(reader.fail("partition byte bone index outside palette"));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        partitions.push(Partition {
            num_vertices,
            num_triangles,
            num_bones,
            num_strips,
            weights_per_vertex,
            bone_palette,
            has_vertex_map,
            vertex_map,
            has_vertex_weights,
            weight_bits,
            strip_lengths,
            has_faces,
            strips,
            triangles,
            has_bone_indices,
            bone_indices,
        });
    }
    Ok(partitions)
}
