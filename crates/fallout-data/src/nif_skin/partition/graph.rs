use super::{Catalogue, Dependency};
use crate::{
    Error, Result, malformed,
    nif_skin::{Data, Instance, Owner, Skin, reserve},
};
use std::collections::BTreeMap;

fn charge(remaining: &mut usize, count: usize, source: &str) -> Result<()> {
    *remaining = remaining.checked_sub(count).ok_or_else(|| {
        Error::Unsupported(format!("{source}: partition index-check budget exceeded"))
    })?;
    Ok(())
}

fn dependency(
    catalogue: &mut Vec<Dependency>,
    value: Dependency,
    remaining: &mut usize,
    source: &str,
) -> Result<()> {
    reserve::<Dependency>(remaining, 1, source)?;
    catalogue.push(value);
    Ok(())
}

pub(super) fn resolve(
    catalogue: &mut Catalogue,
    skin: &Skin,
    source: &str,
    remaining: &mut usize,
    mut checks: usize,
) -> Result<()> {
    let mut instances: BTreeMap<u32, Vec<(u32, &Instance)>> = BTreeMap::new();
    let mut owners: BTreeMap<u32, Vec<&Owner>> = BTreeMap::new();
    for block in &skin.blocks {
        if let Data::Instance { instance } = &block.data
            && let Some(partition) = instance.partition
        {
            instances
                .entry(partition)
                .or_default()
                .push((block.block, instance));
        }
    }
    for owner in &skin.owners {
        owners.entry(owner.instance).or_default().push(owner);
    }
    for block in &catalogue.blocks {
        let fail = |reason| malformed(source, block.offset as u64, reason);
        let linked = instances.get(&block.block);
        if linked.is_none() {
            dependency(
                &mut catalogue.dependencies,
                Dependency::UnownedPartition {
                    partition: block.block,
                },
                remaining,
                source,
            )?;
        }
        for (ordinal, partition) in block.partitions.iter().enumerate() {
            let mut absent = 0;
            if partition.num_vertices != 0 {
                if partition.has_vertex_map == 0 {
                    absent |= 1;
                }
                if partition.has_vertex_weights == 0 {
                    absent |= 2;
                }
                if partition.has_bone_indices == 0 {
                    absent |= 8;
                }
            }
            if (partition.num_triangles != 0
                || partition.strip_lengths.iter().any(|&length| length != 0))
                && partition.has_faces == 0
            {
                absent |= 4;
            }
            if absent != 0 {
                dependency(
                    &mut catalogue.dependencies,
                    Dependency::AbsentArrays {
                        partition: block.block,
                        ordinal,
                        fields: absent,
                    },
                    remaining,
                    source,
                )?;
            }
        }
        for &(instance_id, instance) in linked.into_iter().flatten() {
            if let Some(body_parts) = &instance.body_parts
                && body_parts.len() != block.partitions.len()
            {
                dependency(
                    &mut catalogue.dependencies,
                    Dependency::BodyPartCountMismatch {
                        partition: block.block,
                        instance: instance_id,
                        body_parts: body_parts.len(),
                        partitions: block.partitions.len(),
                    },
                    remaining,
                    source,
                )?;
            }
            let geometry = owners.get(&instance_id);
            if geometry.is_none() {
                dependency(
                    &mut catalogue.dependencies,
                    Dependency::MissingOwner {
                        partition: block.block,
                        instance: instance_id,
                    },
                    remaining,
                    source,
                )?;
            }
            for owner in geometry.into_iter().flatten() {
                if owner.vertex_count.is_none() {
                    dependency(
                        &mut catalogue.dependencies,
                        Dependency::UnknownGeometry {
                            partition: block.block,
                            geometry: owner.geometry,
                        },
                        remaining,
                        source,
                    )?;
                }
            }
            for partition in &block.partitions {
                // Include one unit for the relation even when source arrays are
                // empty, bounding adversarial many-owner/empty-partition products.
                charge(&mut checks, partition.bone_palette.len() + 1, source)?;
                if partition
                    .bone_palette
                    .iter()
                    .any(|&index| usize::from(index) >= instance.bones.len())
                {
                    return Err(fail(
                        "partition bone palette index outside linked instance bone order",
                    ));
                }
                for owner in geometry.into_iter().flatten() {
                    charge(&mut checks, partition.vertex_map.len() + 1, source)?;
                    if let Some(vertices) = owner.vertex_count
                        && partition.vertex_map.iter().any(|&index| index >= vertices)
                    {
                        return Err(fail(
                            "partition vertex map index outside linked geometry vertices",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
