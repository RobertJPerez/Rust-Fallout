//! Private admitted single-CELL source cohort shared by owned consumers.
//! Record framing and source identity remain with the existing Store/decoder.
use super::corridor::{CorridorError, InputLimits, InputUsage, Result, add, debit};
use fallout_data::{
    identity::{FormKey, plugin_name},
    navigation::{self, SourceMesh},
    plugin,
    store::RecordStore,
};
pub(super) struct SourceCohort {
    pub(super) cell: FormKey,
    pub(super) meshes: Vec<SourceMesh>,
    pub(super) usage: InputUsage,
}
impl SourceCohort {
    pub(super) fn load(
        store: &mut RecordStore,
        cell: &FormKey,
        limits: InputLimits,
        reserve_graph: bool,
    ) -> Result<Self> {
        let max = InputLimits::default();
        for (n, c) in [
            (limits.index_visits, max.index_visits),
            (limits.meshes, max.meshes),
            (limits.records.record_bytes, max.records.record_bytes),
            (limits.records.fields, max.records.fields),
            (limits.records.elements, max.records.elements),
            (limits.retained_bytes, max.retained_bytes),
            (limits.graph.nodes, max.graph.nodes),
            (limits.graph.edges, max.graph.edges),
            (limits.graph.door_links, max.graph.door_links),
        ] {
            if n > c {
                return Err(CorridorError::Invalid("input ceiling"));
            }
        }
        // Admit caller identity copies before normalization allocates a string.
        let mut reserved = cell
            .origin_plugin
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(4096))
            .ok_or(CorridorError::Budget("cell identity reservation"))?;
        if reserved > limits.retained_bytes {
            return Err(CorridorError::Budget("cell identity reservation"));
        }
        if limits.source_bytes > max.source_bytes
            || plugin_name(&cell.origin_plugin)? != cell.origin_plugin
        {
            return Err(CorridorError::Invalid("exact source identity/byte ceiling"));
        }
        let mut visits = limits.index_visits;
        let mut source_bytes = 0u64;
        let mut longest = cell.origin_plugin.len();
        for index in store.indices() {
            debit(&mut visits, 1, "source index visits")?;
            source_bytes = source_bytes
                .checked_add(index.census.source_bytes)
                .ok_or(CorridorError::Budget("source bytes"))?;
            if source_bytes > limits.source_bytes {
                return Err(CorridorError::Budget("source bytes"));
            }
            for name in std::iter::once(&index.census.name).chain(&index.census.masters) {
                debit(&mut visits, 1, "source name visits")?;
                longest = longest.max(name.len());
                add(&mut reserved, 1024 + 4 * name.len())?;
            }
        }
        let mut selected = Vec::new();
        let mut winner_visits = 0usize;
        for (_, at) in store.winning_definitions() {
            debit(&mut visits, 1, "source winner visits")?;
            winner_visits += 1;
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
            if selected.len() >= limits.meshes {
                return Err(CorridorError::Budget("source meshes"));
            }
            add(&mut reserved, 4096 + 8 * longest)?;
            if reserved > limits.retained_bytes {
                return Err(CorridorError::Budget("source identity reservation"));
            }
            selected.push(at);
        }
        // The existing single-cell loader performs the second winner scan.
        debit(&mut visits, winner_visits, "source winner visits")?;
        let mut remaining = limits.records;
        let mut nodes = 0usize;
        let mut edges = 0usize;
        let mut doors = 0usize;
        for at in selected {
            let record = store.read_bounded(at, remaining.record_bytes)?;
            let mesh = navigation::decode_mesh(&record, store.source_name(at), remaining)?;
            debit(
                &mut remaining.record_bytes,
                mesh.decoded_bytes,
                "source read bytes",
            )?;
            debit(&mut remaining.fields, mesh.fields.len(), "source fields")?;
            debit(
                &mut remaining.elements,
                mesh.vertices.len()
                    + mesh.triangles.len()
                    + mesh.edge_links.len()
                    + mesh.cover_triangles.len()
                    + mesh.door_links.len(),
                "source elements",
            )?;
            nodes += mesh.triangles.len();
            doors += mesh.door_links.len();
            for tri in &mesh.triangles {
                for i in 0..3 {
                    if tri.edges[i] != -1 || tri.flags & (1 << i) != 0 {
                        edges += 1;
                    }
                }
            }
            // Existing decoder bounds transient source arrays/fields. Admit all
            // retained source capacities and resolved names before final load.
            add(
                &mut reserved,
                mesh.decoded_bytes
                    .checked_mul(2)
                    .ok_or(CorridorError::Budget("source retained bytes"))?,
            )?;
            add(
                &mut reserved,
                mesh.fields.capacity() * 128
                    + mesh.vertices.capacity() * 12
                    + mesh.triangles.capacity() * 16
                    + mesh.edge_links.capacity() * 16
                    + mesh.cover_triangles.capacity() * 2
                    + mesh.door_links.capacity() * 8,
            )?;
            add(
                &mut reserved,
                (mesh.edge_links.len() + mesh.door_links.len()) * (128 + 2 * longest),
            )?;
            if reserved > limits.retained_bytes {
                return Err(CorridorError::Budget("source retained reservation"));
            }
        }
        if reserve_graph {
            if nodes > limits.graph.nodes
                || edges > limits.graph.edges
                || doors > limits.graph.door_links
            {
                return Err(CorridorError::Budget("source graph admission"));
            }
            add(
                &mut reserved,
                nodes * (768 + 4 * longest)
                    + edges * (2048 + 6 * longest)
                    + doors * (512 + longest)
                    + 64 * (edges + 1),
            )?;
            if reserved > limits.retained_bytes {
                return Err(CorridorError::Budget("source graph reservation"));
            }
        }
        if !reserve_graph && reserved > limits.retained_bytes {
            return Err(CorridorError::Budget("source retained reservation"));
        }
        let meshes = navigation::load_cell(store, cell, limits.meshes, remaining)?;
        for mesh in &meshes {
            debit(
                &mut remaining.record_bytes,
                mesh.mesh.decoded_bytes,
                "source read bytes",
            )?;
            debit(
                &mut remaining.fields,
                mesh.mesh.fields.len(),
                "source fields",
            )?;
            debit(
                &mut remaining.elements,
                mesh.mesh.vertices.len()
                    + mesh.mesh.triangles.len()
                    + mesh.mesh.edge_links.len()
                    + mesh.mesh.cover_triangles.len()
                    + mesh.mesh.door_links.len(),
                "source elements",
            )?;
        }
        let usage = InputUsage {
            source_bytes,
            index_visits: limits.index_visits - visits,
            meshes: meshes.len(),
            record_bytes: limits.records.record_bytes - remaining.record_bytes,
            fields: limits.records.fields - remaining.fields,
            elements: limits.records.elements - remaining.elements,
            retained_reservation_bytes: reserved,
        };
        Ok(Self {
            cell: cell.clone(),
            meshes,
            usage,
        })
    }
}
