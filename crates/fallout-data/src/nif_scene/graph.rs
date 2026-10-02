use super::{ObjectKind, Scene, Transform, UnsupportedEdge};
use crate::{Result, malformed, nif::NifIndex};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct WorldTransform {
    pub block: u32,
    pub parent: Option<u32>,
    pub reachable_from_footer: bool,
    /// Affine rows: xyz linear terms, then translation. f64 limits accumulated error.
    pub matrix: [[f64; 4]; 3],
}

pub fn affine(transform: Transform) -> [[f64; 4]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column == 3 {
                f64::from(transform.translation[row])
            } else {
                f64::from(transform.rotation[row][column]) * f64::from(transform.scale)
            }
        })
    })
}

pub fn compose(parent: [[f64; 4]; 3], local: [[f64; 4]; 3]) -> [[f64; 4]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..3)
                .map(|k| parent[row][k] * local[k][column])
                .sum::<f64>()
                + if column == 3 { parent[row][3] } else { 0. }
        })
    })
}

pub(super) fn resolve(scene: &mut Scene, index: &NifIndex, source: &str) -> Result<()> {
    let fail = |block: u32, message: &str| {
        malformed(source, index.blocks[block as usize].offset as u64, message)
    };
    let block_type =
        |block: u32| index.block_types[index.blocks[block as usize].type_index as usize].as_str();
    let mut object_ids = vec![None; index.blocks.len()];
    for (position, object) in scene.objects.iter().enumerate() {
        object_ids[object.block as usize] = Some(position);
    }
    let mut parents = vec![None; scene.objects.len()];
    for (position, object) in scene.objects.iter().enumerate() {
        match &object.kind {
            ObjectKind::Node { children, .. } => {
                for &target in children.iter().flatten() {
                    if let Some(child) = object_ids[target as usize] {
                        if parents[child].replace(position).is_some() {
                            return Err(fail(
                                target,
                                "scene child has repeated or multiple parents",
                            ));
                        }
                    } else {
                        if matches!(block_type(target), "NiTriShapeData" | "NiTriStripsData") {
                            return Err(fail(object.block, "scene child points to geometry data"));
                        }
                        scene.unsupported_scene_edges.push(UnsupportedEdge {
                            parent: Some(object.block),
                            target,
                            block_type: block_type(target).into(),
                        });
                    }
                }
            }
            ObjectKind::Mesh {
                data: Some(target), ..
            } => {
                let expected = if block_type(object.block) == "NiTriStrips" {
                    "NiTriStripsData"
                } else {
                    "NiTriShapeData"
                };
                if block_type(*target) != expected {
                    return Err(fail(
                        object.block,
                        "mesh data reference has wrong block type",
                    ));
                }
            }
            _ => {}
        }
    }
    let mut reachable = vec![false; scene.objects.len()];
    for &root in index.roots.iter().flatten() {
        if let Some(position) = object_ids[root as usize] {
            if parents[position].is_some() {
                return Err(fail(root, "footer root also has a scene parent"));
            }
            reachable[position] = true;
        } else {
            scene.unsupported_scene_edges.push(UnsupportedEdge {
                parent: None,
                target: root,
                block_type: block_type(root).into(),
            });
        }
    }
    // Kahn's traversal handles deep or disconnected graphs without using the call stack.
    let mut queue: VecDeque<_> = parents
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.is_none().then_some(i))
        .collect();
    let mut matrices = vec![[[0.; 4]; 3]; scene.objects.len()];
    let mut visited = 0;
    while let Some(position) = queue.pop_front() {
        visited += 1;
        let object = &scene.objects[position];
        let local = affine(object.transform);
        let matrix = if let Some(parent) = parents[position] {
            reachable[position] = reachable[parent];
            compose(matrices[parent], local)
        } else {
            local
        };
        if !matrix.iter().flatten().all(|v| v.is_finite()) {
            return Err(fail(object.block, "scene transform overflow"));
        }
        matrices[position] = matrix;
        scene.world_transforms.push(WorldTransform {
            block: object.block,
            parent: parents[position].map(|p| scene.objects[p].block),
            reachable_from_footer: reachable[position],
            matrix,
        });
        if let ObjectKind::Node { children, .. } = &object.kind {
            for target in children.iter().flatten() {
                if let Some(child) = object_ids[*target as usize] {
                    queue.push_back(child);
                }
            }
        }
    }
    if visited != scene.objects.len() {
        return Err(malformed(
            source,
            index.payload_start as u64,
            "cycle in NIF scene children",
        ));
    }
    scene.world_transforms.sort_by_key(|t| t.block);
    Ok(())
}
