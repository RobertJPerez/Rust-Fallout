//! Shape ownership is a DAG: different bodies may share a shape. Attachment and
//! constraint links have different roles and do not belong in the shape traversal.
use super::{Collision, Data, UnsupportedLink};
use crate::{Result, malformed, nif::NifIndex};

#[derive(Clone, Copy)]
enum Role {
    Visual,
    Body,
    Shape,
    Convex,
    PackedData,
    Constraint,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Visual => "visual-target",
            Self::Body => "body",
            Self::Shape => "shape",
            Self::Convex => "convex-shape",
            Self::PackedData => "packed-data",
            Self::Constraint => "constraint",
        }
    }
    fn accepts(self, name: &str) -> bool {
        let convex = matches!(
            name,
            "bhkSphereShape"
                | "bhkBoxShape"
                | "bhkCapsuleShape"
                | "bhkConvexVerticesShape"
                | "bhkConvexTransformShape"
                | "bhkConvexListShape"
                | "bhkCylinderShape"
        );
        match self {
            Self::Visual => matches!(
                name,
                "NiNode"
                    | "BSFadeNode"
                    | "NiTriShape"
                    | "NiTriStrips"
                    | "NiBillboardNode"
                    | "BSLeafAnimNode"
                    | "BSOrderedNode"
                    | "BSMultiBoundNode"
                    | "NiLODNode"
            ),
            Self::Body => matches!(
                name,
                "bhkRigidBody" | "bhkRigidBodyT" | "bhkSimpleShapePhantom" | "bhkAabbPhantom"
            ),
            Self::Shape => {
                convex
                    || matches!(
                        name,
                        "bhkListShape"
                            | "bhkTransformShape"
                            | "bhkMoppBvTreeShape"
                            | "bhkPackedNiTriStripsShape"
                            | "bhkNiTriStripsShape"
                            | "bhkMultiSphereShape"
                            | "bhkPlaneShape"
                    )
            }
            Self::Convex => convex,
            Self::PackedData => name == "hkPackedNiTriStripsData",
            Self::Constraint => matches!(
                name,
                "bhkLimitedHingeConstraint"
                    | "bhkHingeConstraint"
                    | "bhkMalleableConstraint"
                    | "bhkRagdollConstraint"
                    | "bhkBreakableConstraint"
                    | "bhkStiffSpringConstraint"
                    | "bhkPrismaticConstraint"
                    | "bhkBallAndSocketConstraint"
                    | "bhkBallSocketConstraintChain"
            ),
        }
    }
}

pub(super) fn validate(collision: &mut Collision, index: &NifIndex, source: &str) -> Result<()> {
    let mut decoded = vec![false; index.blocks.len()];
    let mut children = vec![Vec::new(); index.blocks.len()];
    let mut shapes = vec![false; index.blocks.len()];
    for block in &collision.blocks {
        decoded[block.block as usize] = true;
        shapes[block.block as usize] = !matches!(
            block.data,
            Data::CollisionObject { .. } | Data::RigidBody { .. }
        );
    }
    for block in &collision.blocks {
        let parent = block.block;
        let mut check = |target: Option<u32>, role: Role, shape_edge: bool| -> Result<()> {
            let Some(target) = target else { return Ok(()) };
            let name = &index.block_types[index.blocks[target as usize].type_index as usize];
            if !role.accepts(name) {
                let known_role = [
                    Role::Visual,
                    Role::Body,
                    Role::Shape,
                    Role::PackedData,
                    Role::Constraint,
                ]
                .iter()
                .any(|role| role.accepts(name));
                if !known_role
                    && !super::read::supports(name)
                    && !matches!(
                        name.as_str(),
                        "NiNode"
                            | "BSFadeNode"
                            | "NiTriShape"
                            | "NiTriStrips"
                            | "NiTriShapeData"
                            | "NiTriStripsData"
                            | "NiMaterialProperty"
                            | "NiAlphaProperty"
                    )
                {
                    collision.unsupported_links.push(UnsupportedLink {
                        parent,
                        target,
                        role: role.name(),
                        block_type: name.clone(),
                        type_verified: false,
                    });
                    return Ok(());
                }
                return Err(malformed(
                    source,
                    block.source_offset as u64,
                    format!(
                        "collision block {parent} expects {} at block {target}, found {name}",
                        role.name()
                    ),
                ));
            }
            if !matches!(role, Role::Visual) && !decoded[target as usize] {
                collision.unsupported_links.push(UnsupportedLink {
                    parent,
                    target,
                    role: role.name(),
                    block_type: name.clone(),
                    type_verified: true,
                });
            }
            if shape_edge && decoded[target as usize] {
                children[parent as usize].push(target);
            }
            Ok(())
        };
        match &block.data {
            Data::CollisionObject { target, body, .. } => {
                check(*target, Role::Visual, false)?;
                check(*body, Role::Body, false)?;
            }
            Data::RigidBody { body } => {
                check(body.world.shape, Role::Shape, false)?;
                for &constraint in &body.constraints {
                    check(constraint, Role::Constraint, false)?;
                }
            }
            Data::Transform {
                shape, convex_only, ..
            } => check(
                *shape,
                if *convex_only {
                    Role::Convex
                } else {
                    Role::Shape
                },
                true,
            )?,
            Data::Mopp { shape, .. } => check(*shape, Role::Shape, true)?,
            Data::List { shapes, .. } => {
                for &shape in shapes {
                    check(shape, Role::Shape, true)?;
                }
            }
            Data::PackedShape { data, .. } => check(*data, Role::PackedData, true)?,
            _ => {}
        }
    }
    // Visit disconnected shapes too. Reusing a completed shape is legal; revisiting
    // a shape still on the stack is a cycle. No recursion limit depends on file data.
    let mut colors = vec![0u8; index.blocks.len()];
    let mut stack = Vec::new();
    for start in 0..index.blocks.len() {
        if !shapes[start] || colors[start] != 0 {
            continue;
        }
        colors[start] = 1;
        stack.push((start, 0));
        while let Some((id, next)) = stack.last_mut() {
            if *next == children[*id].len() {
                colors[*id] = 2;
                collision.shape_order.push(*id as u32);
                stack.pop();
                continue;
            }
            let child = children[*id][*next] as usize;
            *next += 1;
            match colors[child] {
                0 => {
                    colors[child] = 1;
                    stack.push((child, 0));
                }
                1 => {
                    return Err(malformed(
                        source,
                        index.blocks[child].offset as u64,
                        "cycle in collision shape graph",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
