use super::{Data, Dependency, Owner, Skin, reserve};
use crate::{Result, malformed, nif::NifIndex, nif_scene};
use std::collections::{BTreeMap, BTreeSet};

fn block_kind(index: &NifIndex, target: u32) -> &str {
    &index.block_types[index.blocks[target as usize].type_index as usize]
}

// NiNode inheritance facts from the pinned XML. Only NiNode/BSFadeNode have
// decoded scene payloads today. Recognizing a subtype is not graph evaluation.
fn node_kind(name: &str) -> bool {
    matches!(
        name,
        "NiNode"
            | "BSFadeNode"
            | "AvoidNode"
            | "BSBlastNode"
            | "BSDamageStage"
            | "BSDebrisNode"
            | "BSDistantObjectInstancedNode"
            | "BSLeafAnimNode"
            | "BSMasterParticleSystem"
            | "BSMultiBoundNode"
            | "BSOrderedNode"
            | "BSRangeNode"
            | "BSTreeNode"
            | "BSValueNode"
            | "CsNiNode"
            | "FxButton"
            | "FxRadioButton"
            | "FxWidget"
            | "JPSJigsawNode"
            | "NiBillboardNode"
            | "NiBone"
            | "NiBSAnimationNode"
            | "NiBSParticleNode"
            | "NiCollisionSwitch"
            | "NiLODNode"
            | "NiRoom"
            | "NiRoomGroup"
            | "NiSortAdjustNode"
            | "NiSwitchNode"
            | "NiWall"
            | "RootCollisionNode"
    )
}

fn dependency(
    dependencies: &mut Vec<Dependency>,
    value: Dependency,
    remaining: &mut usize,
    source: &str,
) -> Result<()> {
    reserve::<Dependency>(remaining, 1, source)?;
    dependencies.push(value);
    Ok(())
}

pub(super) fn resolve(
    skin: &mut Skin,
    index: &NifIndex,
    scene: &nif_scene::Scene,
    source: &str,
    remaining: &mut usize,
    mut index_checks: usize,
) -> Result<()> {
    let decoded = skin
        .blocks
        .iter()
        .map(|block| (block.block, block))
        .collect::<BTreeMap<_, _>>();
    let meshes = scene
        .meshes
        .iter()
        .map(|mesh| (mesh.block, mesh.vertex_count))
        .collect::<BTreeMap<_, _>>();
    let decoded_nodes = scene
        .objects
        .iter()
        .filter(|object| matches!(object.kind, nif_scene::ObjectKind::Node { .. }))
        .map(|object| object.block)
        .collect::<BTreeSet<_>>();
    let mut owned_instances = BTreeSet::new();
    let mut owned_data = BTreeSet::new();
    for object in &scene.objects {
        let nif_scene::ObjectKind::Mesh {
            skin: Some(instance),
            data,
            ..
        } = &object.kind
        else {
            continue;
        };
        let Some(block) = decoded.get(instance) else {
            return Err(malformed(
                source,
                index.blocks[object.block as usize].offset as u64,
                "geometry skin link does not target a supported skin instance",
            ));
        };
        let Data::Instance { instance: fields } = &block.data else {
            return Err(malformed(
                source,
                block.offset as u64,
                "geometry skin link targets NiSkinData",
            ));
        };
        let vertex_count = data.and_then(|target| meshes.get(&target).copied());
        reserve::<Owner>(remaining, 1, source)?;
        skin.owners.push(Owner {
            geometry: object.block,
            instance: *instance,
            geometry_data: *data,
            vertex_count,
        });
        owned_instances.insert(*instance);
        if vertex_count.is_none() {
            dependency(
                &mut skin.dependencies,
                Dependency::MissingGeometryData {
                    geometry: object.block,
                },
                remaining,
                source,
            )?;
        }
        if let Some(data) = fields.data {
            owned_data.insert(data);
            if let Some(data_block) = decoded.get(&data)
                && let Data::SkinData { bones, .. } = &data_block.data
                && let Some(vertices) = vertex_count
            {
                for bone in bones {
                    index_checks =
                        index_checks
                            .checked_sub(bone.weights.len())
                            .ok_or_else(|| {
                                crate::Error::Unsupported(format!(
                                    "{source}: skin owner index-check budget exceeded"
                                ))
                            })?;
                    if bone.weights.iter().any(|weight| weight.vertex >= vertices) {
                        return Err(malformed(
                            source,
                            data_block.offset as u64,
                            format!(
                                "skin vertex index exceeds owner geometry {} with {vertices} vertices",
                                object.block
                            ),
                        ));
                    }
                }
            }
        }
    }
    for block in &skin.blocks {
        let Data::Instance { instance } = &block.data else {
            if !owned_data.contains(&block.block) {
                dependency(
                    &mut skin.dependencies,
                    Dependency::UnownedData { data: block.block },
                    remaining,
                    source,
                )?;
            }
            continue;
        };
        let fail = |reason| malformed(source, block.offset as u64, reason);
        if !owned_instances.contains(&block.block) {
            dependency(
                &mut skin.dependencies,
                Dependency::UnownedInstance {
                    instance: block.block,
                },
                remaining,
                source,
            )?;
        }
        if let Some(data) = instance.data {
            if block_kind(index, data) != "NiSkinData" {
                return Err(fail("skin data link has wrong target kind"));
            }
            let Data::SkinData { bones, .. } = &decoded[&data].data else {
                return Err(fail("linked skin data was not decoded"));
            };
            if bones.len() != instance.bones.len() {
                return Err(fail("skin instance/data bone-count mismatch"));
            }
        } else {
            dependency(
                &mut skin.dependencies,
                Dependency::MissingData {
                    instance: block.block,
                },
                remaining,
                source,
            )?;
        }
        if let Some(partition) = instance.partition {
            if block_kind(index, partition) != "NiSkinPartition" {
                return Err(fail("skin partition link has wrong target kind"));
            }
            dependency(
                &mut skin.dependencies,
                Dependency::PartitionPayload {
                    instance: block.block,
                    target: partition,
                },
                remaining,
                source,
            )?;
        }
        match instance.skeleton_root {
            None => dependency(
                &mut skin.dependencies,
                Dependency::MissingSkeletonRoot {
                    instance: block.block,
                },
                remaining,
                source,
            )?,
            Some(target) => {
                if !node_kind(block_kind(index, target)) {
                    return Err(fail("skin skeleton-root link has wrong target kind"));
                }
                if !decoded_nodes.contains(&target) {
                    dependency(
                        &mut skin.dependencies,
                        Dependency::UndecodedNode {
                            instance: block.block,
                            target,
                        },
                        remaining,
                        source,
                    )?;
                }
            }
        }
        for (ordinal, target) in instance.bones.iter().enumerate() {
            match target {
                None => dependency(
                    &mut skin.dependencies,
                    Dependency::MissingBone {
                        instance: block.block,
                        ordinal,
                    },
                    remaining,
                    source,
                )?,
                Some(target) => {
                    if !node_kind(block_kind(index, *target)) {
                        return Err(fail("skin bone link has wrong target kind"));
                    }
                    if !decoded_nodes.contains(target) {
                        dependency(
                            &mut skin.dependencies,
                            Dependency::UndecodedNode {
                                instance: block.block,
                                target: *target,
                            },
                            remaining,
                            source,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
