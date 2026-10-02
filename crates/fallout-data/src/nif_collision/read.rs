use super::{Body, Data, Filter, Property, Subpart, Triangle, World};
use crate::{Result, nif_scene::cursor::Reader};

pub(super) fn supports(name: &str) -> bool {
    matches!(
        name,
        "bhkCollisionObject"
            | "bhkBlendCollisionObject"
            | "bhkPCollisionObject"
            | "bhkSPCollisionObject"
            | "bhkRigidBody"
            | "bhkRigidBodyT"
            | "bhkSphereShape"
            | "bhkBoxShape"
            | "bhkCapsuleShape"
            | "bhkConvexVerticesShape"
            | "bhkTransformShape"
            | "bhkConvexTransformShape"
            | "bhkListShape"
            | "bhkMoppBvTreeShape"
            | "bhkPackedNiTriStripsShape"
            | "hkPackedNiTriStripsData"
    )
}

fn filter(r: &mut Reader<'_>) -> Result<Filter> {
    Ok(Filter {
        layer: r.u8()?,
        flags_and_parts: r.u8()?,
        group: r.u16()?,
    })
}
fn property(r: &mut Reader<'_>) -> Result<Property> {
    Ok(Property {
        data: r.u32()?,
        size: r.u32()?,
        capacity_and_flags: r.u32()?,
    })
}
fn words<const N: usize>(r: &mut Reader<'_>) -> Result<[u32; N]> {
    let mut values = [0; N];
    for value in &mut values {
        *value = r.u32()?;
    }
    Ok(values)
}
fn padding<const N: usize>(r: &mut Reader<'_>) -> Result<[u8; N]> {
    Ok(r.take(N)?.try_into().expect("fixed block span"))
}

fn body(r: &mut Reader<'_>, transform_active: bool) -> Result<Body> {
    let world = World {
        shape: r.reference()?,
        filter: filter(r)?,
        unused: r.u32()?,
        broad_phase: r.u8()?,
        padding: padding(r)?,
        property: property(r)?,
    };
    let entity_response = r.u8()?;
    let entity_unused = r.u8()?;
    let entity_callback_delay = r.u16()?;
    let unused_01 = r.u32()?;
    let filter_copy = filter(r)?;
    let unused_02 = r.u32()?;
    let collision_response = r.u8()?;
    let unused_03 = r.u8()?;
    let callback_delay = r.u16()?;
    let unused_04 = r.u32()?;
    let translation = r.vector()?;
    let rotation = r.vector()?;
    let linear_velocity = r.vector()?;
    let angular_velocity = r.vector()?;
    let mut inertia = [[0.; 3]; 3];
    let mut inertia_padding = [0; 3];
    for row in 0..3 {
        inertia[row] = r.vector()?;
        inertia_padding[row] = r.u32()?;
    }
    Ok(Body {
        transform_active,
        world,
        entity_response,
        entity_unused,
        entity_callback_delay,
        unused_01,
        filter_copy,
        unused_02,
        collision_response,
        unused_03,
        callback_delay,
        unused_04,
        translation,
        rotation,
        linear_velocity,
        angular_velocity,
        inertia,
        inertia_padding,
        center: r.vector()?,
        mass: r.float()?,
        linear_damping: r.float()?,
        angular_damping: r.float()?,
        friction: r.float()?,
        restitution: r.float()?,
        max_linear_velocity: r.float()?,
        max_angular_velocity: r.float()?,
        penetration_depth: r.float()?,
        motion_system: r.u8()?,
        deactivator: r.u8()?,
        solver_deactivation: r.u8()?,
        quality: r.u8()?,
        unused_05: words(r)?,
        constraints: r.references()?,
        flags: r.u32()?,
    })
}

pub(super) fn read(r: &mut Reader<'_>, name: &str) -> Result<Data> {
    Ok(match name {
        "bhkCollisionObject"
        | "bhkBlendCollisionObject"
        | "bhkPCollisionObject"
        | "bhkSPCollisionObject" => Data::CollisionObject {
            target: r.reference()?,
            flags: r.u16()?,
            body: r.reference()?,
            blend_gains: if name == "bhkBlendCollisionObject" {
                Some(r.vector()?)
            } else {
                None
            },
        },
        "bhkRigidBody" | "bhkRigidBodyT" => Data::RigidBody {
            body: Box::new(body(r, name == "bhkRigidBodyT")?),
        },
        "bhkSphereShape" => Data::Sphere {
            material: r.u32()?,
            radius: r.float()?,
        },
        "bhkBoxShape" => Data::Box {
            material: r.u32()?,
            radius: r.float()?,
            padding: padding(r)?,
            half_extents: r.vector()?,
            unused_w: r.u32()?,
        },
        "bhkCapsuleShape" => Data::Capsule {
            material: r.u32()?,
            radius: r.float()?,
            padding: padding(r)?,
            first: r.vector()?,
            first_radius: r.float()?,
            second: r.vector()?,
            second_radius: r.float()?,
        },
        "bhkConvexVerticesShape" => {
            let material = r.u32()?;
            let radius = r.float()?;
            let vertex_property = property(r)?;
            let normal_property = property(r)?;
            let count = r.u32()? as usize;
            let vertices = r.vectors(count)?;
            let count = r.u32()? as usize;
            let planes = r.vectors(count)?;
            Data::ConvexVertices {
                material,
                radius,
                vertex_property,
                normal_property,
                vertices,
                planes,
            }
        }
        "bhkTransformShape" | "bhkConvexTransformShape" => Data::Transform {
            shape: r.reference()?,
            material: r.u32()?,
            radius: r.float()?,
            padding: padding(r)?,
            matrix: [r.vector()?, r.vector()?, r.vector()?, r.vector()?],
            convex_only: name == "bhkConvexTransformShape",
        },
        "bhkListShape" => {
            let shapes = r.references()?;
            let material = r.u32()?;
            let shape_property = property(r)?;
            let filter_property = property(r)?;
            let count = r.u32()? as usize;
            r.budget(count, 4)?;
            r.reserve::<Filter>(count)?;
            let filters = (0..count).map(|_| filter(r)).collect::<Result<_>>()?;
            Data::List {
                shapes,
                material,
                shape_property,
                filter_property,
                filters,
            }
        }
        "bhkMoppBvTreeShape" => {
            let shape = r.reference()?;
            let unused = words(r)?;
            let scale = r.float()?;
            let count = r.u32()? as usize;
            let offset = r.vector()?;
            r.budget(count, 1)?;
            r.reserve::<u8>(count)?;
            let code = r.take(count)?.to_vec();
            Data::Mopp {
                shape,
                unused,
                scale,
                offset,
                code,
            }
        }
        "bhkPackedNiTriStripsShape" => Data::PackedShape {
            user_data: r.u32()?,
            unused_01: r.u32()?,
            radius: r.float()?,
            unused_02: r.u32()?,
            scale: r.vector()?,
            radius_copy: r.float()?,
            scale_copy: r.vector()?,
            data: r.reference()?,
        },
        "hkPackedNiTriStripsData" => {
            let count = r.u32()? as usize;
            r.budget(count, 8)?;
            r.reserve::<Triangle>(count)?;
            let triangles = (0..count)
                .map(|_| {
                    Ok(Triangle {
                        indices: [r.u16()?, r.u16()?, r.u16()?],
                        welding: r.u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let vertex_count = r.u32()?;
            let compressed = r.boolean()?;
            let (vertices, compressed_words) = if compressed {
                let count = vertex_count as usize;
                r.budget(count, 6)?;
                r.reserve::<[u16; 3]>(count)?;
                (
                    vec![],
                    (0..count)
                        .map(|_| Ok([r.u16()?, r.u16()?, r.u16()?]))
                        .collect::<Result<_>>()?,
                )
            } else {
                (r.vectors(vertex_count as usize)?, vec![])
            };
            for triangle in &triangles {
                if triangle
                    .indices
                    .iter()
                    .any(|&i| u32::from(i) >= vertex_count)
                {
                    return Err(r.fail("packed collision triangle vertex index out of range"));
                }
            }
            let count = r.u16()? as usize;
            r.budget(count, 12)?;
            r.reserve::<Subpart>(count)?;
            let subparts = (0..count)
                .map(|_| {
                    Ok(Subpart {
                        filter: filter(r)?,
                        vertices: r.u32()?,
                        material: r.u32()?,
                    })
                })
                .collect::<Result<_>>()?;
            Data::PackedData {
                triangles,
                vertex_count,
                compressed,
                vertices,
                compressed_words,
                subparts,
            }
        }
        _ => unreachable!("only supported collision types reach the decoder"),
    })
}
