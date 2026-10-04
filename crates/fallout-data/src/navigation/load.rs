use super::{Limits, NavMesh, decode_mesh};
use crate::{
    Error, Result,
    identity::FormKey,
    plugin,
    store::{Location, RecordStore},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SourceMesh {
    pub key: FormKey,
    pub cell: Option<FormKey>,
    pub source_plugin: String,
    pub source_sha256: String,
    pub mesh: NavMesh,
    /// Raw NVEX references resolved in the owning physical plugin's master table.
    /// Missing winners are deliberately not changed into invented neighbors.
    pub external_targets: Vec<Option<FormKey>>,
    pub door_targets: Vec<Option<FormKey>>,
}
fn load_one(
    store: &mut RecordStore,
    key: FormKey,
    at: Location,
    limits: Limits,
) -> Result<SourceMesh> {
    let record = store.read_bounded(at, limits.record_bytes)?;
    let source_plugin = store.source_name(at).to_owned();
    let mesh = decode_mesh(&record, &source_plugin, limits)?;
    let cell = store.key_for(at, mesh.cell_raw.value)?;
    let external_targets = mesh
        .edge_links
        .iter()
        .map(|edge| store.key_for(at, edge.navmesh_raw))
        .collect::<Result<Vec<_>>>()?;
    let door_targets = mesh
        .door_links
        .iter()
        .map(|door| store.key_for(at, door.door_raw))
        .collect::<Result<Vec<_>>>()?;
    Ok(SourceMesh {
        key,
        cell,
        source_plugin,
        source_sha256: store.source_digest(at)?,
        mesh,
        external_targets,
        door_targets,
    })
}

/// Selected-cell source input through the existing winning index and strict
/// read-on-demand store. DATA cell identity must agree with retained GRUP ancestry.
pub fn load_cell(
    store: &mut RecordStore,
    cell: &FormKey,
    maximum_meshes: usize,
    limits: Limits,
) -> Result<Vec<SourceMesh>> {
    let cell_at = store
        .winner(cell)
        .ok_or_else(|| Error::Resolution("navigation CELL winner is missing".into()))?;
    let header = &store.definition(cell_at).header;
    if header.kind != *b"CELL" || header.flags & plugin::DELETED != 0 {
        return Err(Error::Resolution(
            "navigation target must be a live CELL".into(),
        ));
    }
    let mut selected = Vec::new();
    for (key, at) in store.winning_definitions() {
        let def = store.definition(at);
        if def.header.kind != *b"NAVM" || def.header.flags & plugin::DELETED != 0 {
            continue;
        }
        if def
            .parent
            .cell
            .map(|raw| store.key_for(at, raw))
            .transpose()?
            .flatten()
            .as_ref()
            != Some(cell)
        {
            continue;
        }
        if selected.len() >= maximum_meshes {
            return Err(Error::Unsupported("navigation selected mesh budget".into()));
        }
        selected.push((key.clone(), at));
    }
    let mut result = Vec::new();
    let mut elements = limits.elements;
    let mut bytes = limits.record_bytes;
    let mut fields = limits.fields;
    for (key, at) in selected {
        let source = load_one(
            store,
            key,
            at,
            Limits {
                record_bytes: bytes,
                elements,
                fields,
            },
        )?;
        if source.cell.as_ref() != Some(cell) {
            return Err(Error::Resolution(
                "NAVM DATA cell differs from source GRUP ancestry".into(),
            ));
        }
        let count = source.mesh.vertices.len()
            + source.mesh.triangles.len()
            + source.mesh.edge_links.len()
            + source.mesh.cover_triangles.len()
            + source.mesh.door_links.len();
        elements = elements
            .checked_sub(count)
            .ok_or_else(|| Error::Unsupported("navigation aggregate element budget".into()))?;
        bytes = bytes
            .checked_sub(source.mesh.decoded_bytes)
            .ok_or_else(|| Error::Unsupported("navigation aggregate record byte budget".into()))?;
        fields = fields
            .checked_sub(source.mesh.fields.len())
            .ok_or_else(|| Error::Unsupported("navigation aggregate field budget".into()))?;
        result.push(source);
    }
    Ok(result)
}
