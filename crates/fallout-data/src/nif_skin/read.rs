use super::{BodyPart, Bone, Data, Instance, Transform, Weight};
use crate::{Result, nif_scene::cursor::Reader};

fn bits<const N: usize>(reader: &mut Reader<'_>) -> Result<[u32; N]> {
    Ok(reader.vector::<N>()?.map(f32::to_bits))
}

fn transform(reader: &mut Reader<'_>) -> Result<Transform> {
    // NiTransform differs from NiAVObject's field order: rotation comes first.
    Ok(Transform {
        rotation_bits: [bits(reader)?, bits(reader)?, bits(reader)?],
        translation_bits: bits(reader)?,
        scale_bits: reader.float()?.to_bits(),
    })
}

pub(super) fn skin_data(reader: &mut Reader<'_>) -> Result<Data> {
    let transform = transform(reader)?;
    let count = reader.u32()? as usize;
    let has_vertex_weights = reader.u8()?;
    // The tuple admitted by nif::inspect has no old data-level partition ref.
    // Every bone has 52 transform + 16 bound + 2 raw vertex-count bytes.
    reader.budget(count, 70)?;
    reader.reserve::<Bone>(count)?;
    let mut bones = Vec::with_capacity(count);
    for _ in 0..count {
        let transform = self::transform(reader)?;
        let center_bits = bits(reader)?;
        let radius = reader.float()?;
        if radius < 0.0 {
            return Err(reader.fail("negative skin bone bounding radius"));
        }
        let declared_vertices = reader.u16()?;
        let count = if has_vertex_weights == 0 {
            0
        } else {
            usize::from(declared_vertices)
        };
        reader.budget(count, 6)?;
        reader.reserve::<Weight>(count)?;
        let weights = (0..count)
            .map(|_| {
                Ok(Weight {
                    vertex: reader.u16()?,
                    weight_bits: reader.float()?.to_bits(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        bones.push(Bone {
            transform,
            center_bits,
            radius_bits: radius.to_bits(),
            declared_vertices,
            weights,
        });
    }
    Ok(Data::SkinData {
        transform,
        has_vertex_weights,
        bones,
    })
}

pub(super) fn instance(reader: &mut Reader<'_>, dismember: bool) -> Result<Instance> {
    let data = reader.reference()?;
    // Present since 10.1.0.101, therefore in every admitted NV stream revision.
    let partition = reader.reference()?;
    let skeleton_root = reader.reference()?;
    let bones = reader.references()?;
    let body_parts = if dismember {
        let count = reader.u32()? as usize;
        reader.budget(count, 4)?;
        reader.reserve::<BodyPart>(count)?;
        Some(
            (0..count)
                .map(|_| {
                    Ok(BodyPart {
                        flags: reader.u16()?,
                        body_part: reader.u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        )
    } else {
        None
    };
    Ok(Instance {
        data,
        partition,
        skeleton_root,
        bones,
        body_parts,
    })
}
