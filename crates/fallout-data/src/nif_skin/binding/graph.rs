use super::{Bone, Catalogue, Diagnostic, Instance, Limits, Node, Owner, Target, reserve};
use crate::{
    Error, Result, nif,
    nif_scene::{self, ObjectKind, Scene},
    nif_skin::{Data, Skin, Transform},
};
use sha2::{Digest, Sha256};

fn charge(remaining: &mut usize, count: usize, source: &str) -> Result<()> {
    *remaining = remaining.checked_sub(count).ok_or_else(|| {
        Error::Unsupported(format!("{source}: binding graph-check budget exceeded"))
    })?;
    Ok(())
}

struct Graph {
    positions: Vec<Option<usize>>,
    parents: Vec<Option<u32>>,
    reachable: Vec<bool>,
    nodes: Vec<bool>,
    enter: Vec<usize>,
    leave: Vec<usize>,
}

impl Graph {
    fn target(&self, target: Option<u32>) -> Target {
        let decoded_node = target.is_some_and(|id| self.nodes[id as usize]);
        Target {
            target,
            decoded_node,
            parent: target
                .filter(|_| decoded_node)
                .and_then(|id| self.parents[id as usize]),
            reachable_from_footer: target
                .filter(|_| decoded_node)
                .map(|id| self.reachable[id as usize]),
        }
    }
    fn contains(&self, root: &Target, target: u32) -> Option<bool> {
        root.target.filter(|_| root.decoded_node).map(|root| {
            self.enter[root as usize] <= self.enter[target as usize]
                && self.leave[target as usize] <= self.leave[root as usize]
        })
    }
}

fn forest(
    scene: &Scene,
    blocks: usize,
    storage: &mut usize,
    checks: &mut usize,
    source: &str,
) -> Result<Graph> {
    charge(
        checks,
        blocks + scene.objects.len() + scene.world_transforms.len(),
        source,
    )?;
    reserve::<Option<usize>>(storage, blocks, source)?;
    reserve::<Option<u32>>(storage, blocks, source)?;
    reserve::<bool>(storage, blocks * 2, source)?;
    reserve::<usize>(storage, blocks * 2, source)?;
    let mut graph = Graph {
        positions: vec![None; blocks],
        parents: vec![None; blocks],
        reachable: vec![false; blocks],
        nodes: vec![false; blocks],
        enter: vec![0; blocks],
        leave: vec![0; blocks],
    };
    for (position, object) in scene.objects.iter().enumerate() {
        graph.positions[object.block as usize] = Some(position);
        graph.nodes[object.block as usize] = matches!(object.kind, ObjectKind::Node { .. });
    }
    for world in &scene.world_transforms {
        graph.parents[world.block as usize] = world.parent;
        graph.reachable[world.block as usize] = world.reachable_from_footer;
    }
    reserve::<(u32, bool)>(storage, scene.objects.len() * 2, source)?;
    let mut stack = Vec::with_capacity(scene.objects.len() * 2);
    for object in scene.objects.iter().rev() {
        if graph.parents[object.block as usize].is_none() {
            stack.push((object.block, false));
        }
    }
    let mut tick = 0;
    while let Some((block, exit)) = stack.pop() {
        charge(checks, 1, source)?;
        if exit {
            graph.leave[block as usize] = tick;
        } else {
            graph.enter[block as usize] = tick;
            stack.push((block, true));
            let position = graph.positions[block as usize].expect("validated source object");
            if let ObjectKind::Node { children, .. } = &scene.objects[position].kind {
                charge(checks, children.len(), source)?;
                for child in children.iter().rev().flatten() {
                    if graph.positions[*child as usize].is_some() {
                        stack.push((*child, false));
                    }
                }
            }
        }
        tick += 1;
    }
    Ok(graph)
}

fn diagnostic(
    out: &mut Vec<Diagnostic>,
    value: Diagnostic,
    storage: &mut usize,
    source: &str,
) -> Result<()> {
    reserve::<Diagnostic>(storage, 1, source)?;
    out.push(value);
    Ok(())
}

fn node_type(
    index: &nif::NifIndex,
    target: u32,
    storage: &mut usize,
    source: &str,
) -> Result<String> {
    let name = &index.block_types[index.blocks[target as usize].type_index as usize];
    reserve::<u8>(storage, name.len(), source)?;
    Ok(name.clone())
}

pub(super) fn bind(
    bytes: &[u8],
    index: &nif::NifIndex,
    skin: &Skin,
    scene: &Scene,
    source: &str,
    limits: Limits,
) -> Result<Catalogue> {
    let mut storage = limits.array_bytes;
    let mut checks = limits.graph_checks;
    let graph = forest(scene, index.blocks.len(), &mut storage, &mut checks, source)?;
    let mut catalogue = Catalogue {
        ancestry_scope: "decoded-source-forest",
        nodes: Vec::new(),
        instances: Vec::new(),
        footer_roots: Vec::new(),
        unsupported_scene_edges: Vec::new(),
        diagnostics: Vec::new(),
        retained_bytes: 0,
        runtime_ready: false,
    };
    reserve::<Option<u32>>(&mut storage, index.roots.len(), source)?;
    catalogue.footer_roots = index.roots.clone();
    for object in &scene.objects {
        let ObjectKind::Node { children, effects } = &object.kind else {
            continue;
        };
        reserve::<Node>(&mut storage, 1, source)?;
        reserve::<Option<u32>>(
            &mut storage,
            object.extra_data.len() + object.properties.len() + children.len() + effects.len(),
            source,
        )?;
        let block = &index.blocks[object.block as usize];
        let block_type = node_type(index, object.block, &mut storage, source)?;
        reserve::<u8>(&mut storage, 64, source)?;
        catalogue.nodes.push(Node {
            block: object.block,
            block_type,
            offset: block.offset,
            bytes: block.bytes,
            sha256: format!(
                "{:x}",
                Sha256::digest(&bytes[block.offset..block.offset + block.bytes])
            ),
            name: object.name,
            extra_data: object.extra_data.clone(),
            controller: object.controller,
            flags: object.flags,
            transform: Transform {
                rotation_bits: object.transform.rotation.map(|row| row.map(f32::to_bits)),
                translation_bits: object.transform.translation.map(f32::to_bits),
                scale_bits: object.transform.scale.to_bits(),
            },
            properties: object.properties.clone(),
            collision: object.collision,
            children: children.clone(),
            effects: effects.clone(),
            parent: graph.parents[object.block as usize],
            reachable_from_footer: graph.reachable[object.block as usize],
        });
    }
    reserve::<Vec<&crate::nif_skin::Owner>>(&mut storage, index.blocks.len(), source)?;
    reserve::<&crate::nif_skin::Owner>(&mut storage, skin.owners.len(), source)?;
    let mut owners = vec![Vec::new(); index.blocks.len()];
    charge(&mut checks, skin.owners.len(), source)?;
    for owner in &skin.owners {
        owners[owner.instance as usize].push(owner);
    }
    for block in &skin.blocks {
        charge(&mut checks, 1, source)?;
        let Data::Instance { instance } = &block.data else {
            continue;
        };
        reserve::<Instance>(&mut storage, 1, source)?;
        reserve::<Bone>(&mut storage, instance.bones.len(), source)?;
        reserve::<Owner>(&mut storage, owners[block.block as usize].len(), source)?;
        charge(
            &mut checks,
            1 + instance.bones.len() + owners[block.block as usize].len(),
            source,
        )?;
        let skeleton_root = graph.target(instance.skeleton_root);
        match skeleton_root.target {
            None => diagnostic(
                &mut catalogue.diagnostics,
                Diagnostic::MissingRoot {
                    instance: block.block,
                },
                &mut storage,
                source,
            )?,
            Some(target) if !skeleton_root.decoded_node => {
                let block_type = node_type(index, target, &mut storage, source)?;
                diagnostic(
                    &mut catalogue.diagnostics,
                    Diagnostic::UndecodedRoot {
                        instance: block.block,
                        target,
                        block_type,
                    },
                    &mut storage,
                    source,
                )?;
            }
            Some(target) if skeleton_root.reachable_from_footer == Some(false) => diagnostic(
                &mut catalogue.diagnostics,
                Diagnostic::RootUnreachableFooter {
                    instance: block.block,
                    target,
                },
                &mut storage,
                source,
            )?,
            _ => {}
        }
        let mut bound = Instance {
            instance: block.block,
            skeleton_root,
            bones: Vec::with_capacity(instance.bones.len()),
            owners: Vec::with_capacity(owners[block.block as usize].len()),
        };
        for (ordinal, &target) in instance.bones.iter().enumerate() {
            let node = graph.target(target);
            let contains = target
                .filter(|_| node.decoded_node)
                .and_then(|target| graph.contains(&bound.skeleton_root, target));
            match target {
                None => diagnostic(
                    &mut catalogue.diagnostics,
                    Diagnostic::MissingBone {
                        instance: block.block,
                        ordinal,
                    },
                    &mut storage,
                    source,
                )?,
                Some(target) if !node.decoded_node => {
                    let block_type = node_type(index, target, &mut storage, source)?;
                    diagnostic(
                        &mut catalogue.diagnostics,
                        Diagnostic::UndecodedBone {
                            instance: block.block,
                            ordinal,
                            target,
                            block_type,
                        },
                        &mut storage,
                        source,
                    )?;
                }
                Some(target) => {
                    if node.reachable_from_footer == Some(false) {
                        diagnostic(
                            &mut catalogue.diagnostics,
                            Diagnostic::BoneUnreachableFooter {
                                instance: block.block,
                                ordinal,
                                target,
                            },
                            &mut storage,
                            source,
                        )?;
                    }
                    if contains == Some(false) {
                        diagnostic(
                            &mut catalogue.diagnostics,
                            Diagnostic::BoneOutsideDecodedRoot {
                                instance: block.block,
                                ordinal,
                                target,
                            },
                            &mut storage,
                            source,
                        )?;
                    }
                }
            }
            bound.bones.push(Bone {
                ordinal,
                node,
                decoded_root_contains: contains,
            });
        }
        for owner in &owners[block.block as usize] {
            let reachable = graph.reachable[owner.geometry as usize];
            let contains = graph.contains(&bound.skeleton_root, owner.geometry);
            if !reachable {
                diagnostic(
                    &mut catalogue.diagnostics,
                    Diagnostic::OwnerUnreachableFooter {
                        instance: block.block,
                        geometry: owner.geometry,
                    },
                    &mut storage,
                    source,
                )?;
            }
            if contains == Some(false) {
                diagnostic(
                    &mut catalogue.diagnostics,
                    Diagnostic::OwnerOutsideDecodedRoot {
                        instance: block.block,
                        geometry: owner.geometry,
                    },
                    &mut storage,
                    source,
                )?;
            }
            bound.owners.push(Owner {
                geometry: owner.geometry,
                parent: graph.parents[owner.geometry as usize],
                reachable_from_footer: reachable,
                decoded_root_contains: contains,
            });
        }
        catalogue.instances.push(bound);
    }
    reserve::<nif_scene::UnsupportedEdge>(
        &mut storage,
        scene.unsupported_scene_edges.len(),
        source,
    )?;
    for edge in &scene.unsupported_scene_edges {
        reserve::<u8>(&mut storage, edge.block_type.len(), source)?;
    }
    catalogue.unsupported_scene_edges = scene
        .unsupported_scene_edges
        .iter()
        .map(|edge| nif_scene::UnsupportedEdge {
            parent: edge.parent,
            target: edge.target,
            block_type: edge.block_type.clone(),
        })
        .collect();
    catalogue.retained_bytes = limits.array_bytes - storage;
    Ok(catalogue)
}
