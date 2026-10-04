//! Source-authored FNV navigation. Record framing, compression and identity stay
//! with the existing plugin/store modules. Decoded flags do not decide movement.
mod load;
mod read;
mod selection;
use crate::{plugin, world::SourceField};
pub use load::{SourceMesh, load_cell};
pub use read::{decode_info_map, decode_mesh};
pub use selection::{CellSet, CellSetLimits, CellSetUsage, CellSource, load_cells};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub record_bytes: usize,
    pub fields: usize,
    pub elements: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            record_bytes: 64 * 1024 * 1024,
            fields: 100_000,
            elements: 1_000_000,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct RawField {
    pub kind: [u8; 4],
    pub decoded_offset: usize,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Triangle {
    pub vertices: [u16; 3],
    pub edges: [i16; 3],
    pub flags: u16,
    pub cover_flags: u16,
}
#[derive(Debug, Clone, Serialize)]
pub struct EdgeLink {
    pub link_type: u32,
    pub navmesh_raw: u32,
    pub triangle: u16,
}
#[derive(Debug, Clone, Serialize)]
pub struct DoorLink {
    pub door_raw: u32,
    pub triangle: u16,
    pub unused: [u8; 2],
}
#[derive(Debug, Serialize)]
pub struct NavMesh {
    pub header: plugin::RecordHeader,
    pub decoded_bytes: usize,
    pub decoded_sha256: String,
    pub version: SourceField<u32>,
    pub cell_raw: SourceField<u32>,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<Triangle>,
    pub edge_links: Vec<EdgeLink>,
    pub cover_triangles: Vec<u16>,
    pub door_links: Vec<DoorLink>,
    /// Every field remains in physical order; NVGD and unknown fields are not
    /// discarded or interpreted as a generated spatial index.
    pub fields: Vec<RawField>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Island {
    pub bounds: [[f32; 3]; 2],
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
}
#[derive(Debug, Clone, Serialize)]
pub struct MeshInfo {
    pub decoded_offset: usize,
    pub flags: u32,
    pub navmesh_raw: u32,
    pub location_raw: u32,
    pub grid_y: i16,
    pub grid_x: i16,
    pub approximate_location: [f32; 3],
    pub island: Option<Island>,
    pub preferred_percent: f32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Connections {
    pub decoded_offset: usize,
    pub navmesh_raw: u32,
    pub standard: Vec<u32>,
    pub preferred: Vec<u32>,
    pub doors: Vec<u32>,
}
#[derive(Debug, Serialize)]
pub struct InfoMap {
    pub header: plugin::RecordHeader,
    pub decoded_bytes: usize,
    pub decoded_sha256: String,
    pub version: SourceField<u32>,
    pub infos: Vec<MeshInfo>,
    pub connections: Vec<Connections>,
    pub fields: Vec<RawField>,
}
