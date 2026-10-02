//! Authored NV collision data, in its original Havok coordinates and units.
//! Decoding a shape is not a physics conversion or a claim about Havok behavior.
mod graph;
mod read;

pub use crate::nif_scene::Limits;
use crate::{Error, Result, nif, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Filter {
    pub layer: u8,
    /// Keep the part-number bits together with the source flags.
    pub flags_and_parts: u8,
    pub group: u16,
}

#[derive(Debug, Serialize)]
pub struct Property {
    pub data: u32,
    pub size: u32,
    pub capacity_and_flags: u32,
}

#[derive(Debug, Serialize)]
pub struct World {
    pub shape: Option<u32>,
    pub filter: Filter,
    pub unused: u32,
    pub broad_phase: u8,
    pub padding: [u8; 3],
    pub property: Property,
}

#[derive(Debug, Serialize)]
pub struct Body {
    pub transform_active: bool,
    pub world: World,
    pub entity_response: u8,
    pub entity_unused: u8,
    pub entity_callback_delay: u16,
    pub unused_01: u32,
    pub filter_copy: Filter,
    pub unused_02: u32,
    pub collision_response: u8,
    pub unused_03: u8,
    pub callback_delay: u16,
    pub unused_04: u32,
    pub translation: [f32; 4],
    /// Stored x, y, z, w. Leave normalization and body activation to an adapter.
    pub rotation: [f32; 4],
    pub linear_velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
    pub inertia: [[f32; 3]; 3],
    /// Alignment words are not numerical matrix components; retain their bits.
    pub inertia_padding: [u32; 3],
    pub center: [f32; 4],
    pub mass: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub friction: f32,
    pub restitution: f32,
    pub max_linear_velocity: f32,
    pub max_angular_velocity: f32,
    pub penetration_depth: f32,
    pub motion_system: u8,
    pub deactivator: u8,
    pub solver_deactivation: u8,
    pub quality: u8,
    pub unused_05: [u32; 3],
    pub constraints: Vec<Option<u32>>,
    pub flags: u32,
}

#[derive(Debug, Serialize)]
pub struct Triangle {
    pub indices: [u16; 3],
    pub welding: u16,
}

#[derive(Debug, Serialize)]
pub struct Subpart {
    pub filter: Filter,
    pub vertices: u32,
    pub material: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    CollisionObject {
        target: Option<u32>,
        flags: u16,
        body: Option<u32>,
        blend_gains: Option<[f32; 2]>,
    },
    RigidBody {
        body: Box<Body>,
    },
    Sphere {
        material: u32,
        radius: f32,
    },
    Box {
        material: u32,
        radius: f32,
        padding: [u8; 8],
        half_extents: [f32; 3],
        unused_w: u32,
    },
    Capsule {
        material: u32,
        radius: f32,
        padding: [u8; 8],
        first: [f32; 3],
        first_radius: f32,
        second: [f32; 3],
        second_radius: f32,
    },
    ConvexVertices {
        material: u32,
        radius: f32,
        vertex_property: Property,
        normal_property: Property,
        vertices: Vec<[f32; 4]>,
        planes: Vec<[f32; 4]>,
    },
    Transform {
        shape: Option<u32>,
        material: u32,
        radius: f32,
        padding: [u8; 8],
        matrix: [[f32; 4]; 4],
        convex_only: bool,
    },
    List {
        shapes: Vec<Option<u32>>,
        material: u32,
        shape_property: Property,
        filter_property: Property,
        filters: Vec<Filter>,
    },
    Mopp {
        shape: Option<u32>,
        unused: [u32; 3],
        scale: f32,
        offset: [f32; 4],
        code: Vec<u8>,
    },
    PackedShape {
        user_data: u32,
        unused_01: u32,
        radius: f32,
        unused_02: u32,
        scale: [f32; 4],
        radius_copy: f32,
        scale_copy: [f32; 4],
        data: Option<u32>,
    },
    PackedData {
        triangles: Vec<Triangle>,
        vertex_count: u32,
        compressed: bool,
        vertices: Vec<[f32; 3]>,
        compressed_words: Vec<[u16; 3]>,
        subparts: Vec<Subpart>,
    },
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: String,
    pub source_offset: usize,
    pub source_bytes: usize,
    pub source_sha256: String,
    pub data: Data,
}

#[derive(Debug, Serialize)]
pub struct UnsupportedLink {
    pub parent: u32,
    pub target: u32,
    pub role: &'static str,
    pub block_type: String,
    pub type_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct Collision {
    pub blocks: Vec<Block>,
    pub unsupported_blocks: BTreeMap<String, Vec<u32>>,
    pub unsupported_links: Vec<UnsupportedLink>,
    pub shape_order: Vec<u32>,
    /// Retained collection capacity and owned payload storage, with conservative
    /// BTreeMap node overhead. Allocator bookkeeping and temporary traversal differ.
    pub retained_bytes: usize,
    pub units: &'static str,
    pub physics_ready: bool,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Collision)> {
    decode_with_limits(bytes, source, Limits::default())
}

pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Collision)> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Unsupported(
            "NIF collision input byte budget exceeded".into(),
        ));
    }
    let index = nif::inspect(bytes, source)?;
    if index.blocks.len() > limits.blocks {
        return Err(Error::Unsupported(
            "NIF collision block count budget exceeded".into(),
        ));
    }
    // inspect already restricts the tuple to the twelve observed NV streams.
    // They share this pre-Skyrim collision layout, including stream 14.
    let mut collision = Collision {
        blocks: vec![],
        unsupported_blocks: BTreeMap::new(),
        unsupported_links: vec![],
        shape_order: vec![],
        retained_bytes: 0,
        units: "original Havok units; no rendering-unit, axis or physics conversion applied",
        physics_ready: false,
    };
    let mut array_bytes_left = limits.array_bytes;
    for (id, block) in index.blocks.iter().enumerate() {
        let name = index.block_types[block.type_index as usize].as_str();
        if !read::supports(name) {
            if name.starts_with("bhk") || name.starts_with("hk") {
                collision
                    .unsupported_blocks
                    .entry(name.into())
                    .or_default()
                    .push(id as u32);
            }
            continue;
        }
        let data = &bytes[block.offset..block.offset + block.bytes];
        let mut reader = Reader {
            data,
            base: block.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut array_bytes_left,
        };
        let parsed = read::read(&mut reader, name)?;
        reader.finish()?;
        collision.blocks.push(Block {
            block: id as u32,
            block_type: name.into(),
            source_offset: block.offset,
            source_bytes: block.bytes,
            source_sha256: format!("{:x}", Sha256::digest(data)),
            data: parsed,
        });
    }
    graph::validate(&mut collision, &index, source)?;
    collision.retained_bytes = retained_bytes(&collision);
    Ok((index, collision))
}

fn retained_bytes(collision: &Collision) -> usize {
    use std::mem::size_of;
    let mut bytes = size_of::<Collision>()
        + collision.blocks.capacity() * size_of::<Block>()
        + collision.shape_order.capacity() * size_of::<u32>()
        + collision.unsupported_links.capacity() * size_of::<UnsupportedLink>();
    for block in &collision.blocks {
        bytes += block.block_type.capacity() + block.source_sha256.capacity();
        bytes += match &block.data {
            Data::RigidBody { body } => {
                size_of::<Body>() + body.constraints.capacity() * size_of::<Option<u32>>()
            }
            Data::ConvexVertices {
                vertices, planes, ..
            } => (vertices.capacity() + planes.capacity()) * size_of::<[f32; 4]>(),
            Data::List {
                shapes, filters, ..
            } => {
                shapes.capacity() * size_of::<Option<u32>>()
                    + filters.capacity() * size_of::<Filter>()
            }
            Data::Mopp { code, .. } => code.capacity(),
            Data::PackedData {
                triangles,
                vertices,
                compressed_words,
                subparts,
                ..
            } => {
                triangles.capacity() * size_of::<Triangle>()
                    + vertices.capacity() * size_of::<[f32; 3]>()
                    + compressed_words.capacity() * size_of::<[u16; 3]>()
                    + subparts.capacity() * size_of::<Subpart>()
            }
            _ => 0,
        };
    }
    for (name, blocks) in &collision.unsupported_blocks {
        bytes += 256 + name.capacity() + blocks.capacity() * size_of::<u32>();
    }
    bytes
        + collision
            .unsupported_links
            .iter()
            .map(|link| link.block_type.capacity())
            .sum::<usize>()
}
