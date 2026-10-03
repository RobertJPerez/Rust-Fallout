//! Source fields for exterior loading. Authored height deltas and normal bytes
//! remain unchanged; interpreting them for rendering/physics is a separate step.
pub mod heights;
mod inspect;
pub mod mesh;
mod schema;
mod surface;
pub mod textures;
pub use surface::{Surface, SurfaceLand, compare_neighbor, reconstruct_cell};

pub use inspect::{RecordEntry, TerrainReport, inspect_cell, inspect_cell_key};
pub use schema::{
    AlphaVertex, CellFields, Fields, HeightMap, LandTexture, Landscape, Layer, RawField,
    TextureSet, Worldspace, decode,
};
