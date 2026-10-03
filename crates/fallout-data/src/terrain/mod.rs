//! Source fields for exterior loading. Authored height deltas and normal bytes
//! remain unchanged; interpreting them for rendering/physics is a separate step.
mod inspect;
mod schema;

pub use inspect::{RecordEntry, TerrainReport, inspect_cell};
pub use schema::{
    AlphaVertex, CellFields, Fields, HeightMap, Landscape, Layer, RawField, Worldspace, decode,
};
