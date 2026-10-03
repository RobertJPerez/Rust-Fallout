use super::{Fields, decode};
use crate::{
    Error, Result, cache,
    identity::{FormKey, ProfileId},
    plugin::{self, RecordHeader},
    store::{Location, RecordStore},
    world::{Dependency, dependency},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct RecordEntry {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
    pub fields: Option<Fields>,
    pub decoded_sha256: Option<String>,
    pub body_cache: Option<cache::CacheResult>,
    pub links: BTreeMap<String, Dependency>,
}

#[derive(Debug, Serialize)]
pub struct TerrainReport {
    pub schema_version: u32,
    pub cell: RecordEntry,
    pub world_chain: Vec<RecordEntry>,
    pub landscapes: Vec<RecordEntry>,
    pub index_payloads_deferred: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_cache: Option<crate::index_cache::Report>,
    pub integrity_failures: usize,
    pub link_failures: usize,
    pub runtime_ready: bool,
    pub unknown: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_dependencies: Option<super::textures::Report>,
}

fn entry(
    store: &mut RecordStore,
    key: FormKey,
    location: Location,
    cache: Option<(&Path, &Path)>,
) -> Result<RecordEntry> {
    entry_bounded(store, key, location, cache, 64 * 1024 * 1024)
}

pub(super) fn entry_bounded(
    store: &mut RecordStore,
    key: FormKey,
    location: Location,
    cache: Option<(&Path, &Path)>,
    maximum: usize,
) -> Result<RecordEntry> {
    let header = store.definition(location).header.clone();
    let source_plugin = store.source_name(location).to_owned();
    let source_sha256 = store.source_digest(location)?;
    let mut out = RecordEntry {
        key,
        source_plugin,
        source_sha256,
        header,
        fields: None,
        decoded_sha256: None,
        body_cache: None,
        links: BTreeMap::new(),
    };
    if out.header.flags & plugin::DELETED != 0 {
        return Ok(out);
    }
    let record = store.read_bounded(location, maximum)?;
    let fields = decode(&record, &out.source_plugin)?;
    out.decoded_sha256 = Some(format!("{:x}", Sha256::digest(&record.payload)));
    match &fields {
        Fields::World(world) => {
            for (name, field, kind) in [
                ("WNAM", &world.parent, *b"WRLD"),
                ("CNAM", &world.climate, *b"CLMT"),
                ("NAM2", &world.water, *b"WATR"),
                ("NAM3", &world.lod_water, *b"WATR"),
                ("INAM", &world.image_space, *b"IMGS"),
                ("XEZN", &world.encounter_zone, *b"ECZN"),
                ("ZNAM", &world.music, *b"MUSC"),
            ] {
                if let Some(field) = field {
                    out.links.insert(
                        name.into(),
                        dependency(store, location, field.value, &[kind])?,
                    );
                }
            }
        }
        Fields::Land(land) => {
            for (i, layer) in land.layers.iter().enumerate() {
                out.links.insert(
                    format!("layer[{i}].texture"),
                    dependency(store, location, layer.texture_raw, &[*b"LTEX"])?,
                );
            }
        }
        Fields::LandTexture(texture) => {
            if let Some(field) = &texture.texture_set {
                out.links.insert(
                    "TNAM".into(),
                    dependency(store, location, field.value, &[*b"TXST"])?,
                );
            }
            for (i, field) in texture.grasses.iter().enumerate() {
                out.links.insert(
                    format!("grass[{i}]"),
                    dependency(store, location, field.value, &[*b"GRAS"])?,
                );
            }
        }
        Fields::Cell(_) | Fields::TextureSet(_) => {}
    }
    if let Some((root, source_tree)) = cache {
        // The four-byte record kind tags an offline oracle input. The remaining
        // bytes are the exact strictly decoded source body, never a conversion.
        let mut bytes = record.header.kind.to_vec();
        bytes.extend(&record.payload);
        out.body_cache = Some(cache::publish(
            root,
            source_tree,
            cache::ArtifactIdentity {
                profile: ProfileId::NvOriginal,
                source_sha256: out.decoded_sha256.clone().expect("decoded body hash"),
                path_bytes: record.header.kind.to_vec(),
                transform_version: "nv-selected-terrain-body-v1".into(),
            },
            &bytes,
        )?);
    }
    out.fields = Some(fields);
    Ok(out)
}

/// Inspect one winning exterior cell and its source-local LAND membership.
/// Parent worlds are traversed iteratively to diagnose cycles without recursion.
/// Their fields remain separate; inheritance has not been measured or applied.
pub fn inspect_cell(
    store: &mut RecordStore,
    editor_id: &[u8],
    body_cache: Option<(&Path, &Path)>,
) -> Result<TerrainReport> {
    let (cell_key, _) = store.cell_by_editor_id(editor_id)?;
    inspect_cell_key(store, &cell_key, body_cache)
}

/// Exterior cells often have no EDID. Select them by their persistent origin
/// identity instead of a transient load-order high byte.
pub fn inspect_cell_key(
    store: &mut RecordStore,
    cell_key: &FormKey,
    body_cache: Option<(&Path, &Path)>,
) -> Result<TerrainReport> {
    if let Some((root, source_tree)) = body_cache {
        cache::validate_root(root, source_tree)?;
    }
    let cell_location = *store
        .winners
        .get(cell_key)
        .ok_or_else(|| Error::Resolution("selected CELL identity is missing".into()))?;
    let definition = store.definition(cell_location);
    if definition.header.kind != *b"CELL" || definition.header.flags & plugin::DELETED != 0 {
        return Err(Error::Resolution(
            "selected CELL identity is deleted or has the wrong kind".into(),
        ));
    }
    let cell_key = cell_key.clone();
    let world_raw = store
        .definition(cell_location)
        .parent
        .world
        .ok_or_else(|| Error::Unsupported("selected CELL has no worldspace group".into()))?;
    let mut next_world = dependency(store, cell_location, world_raw, &[*b"WRLD"])?;
    if next_world.status != "resolved" {
        return Err(Error::Resolution(format!(
            "exterior CELL worldspace is {}",
            next_world.status
        )));
    }
    let mut cell = entry(store, cell_key.clone(), cell_location, body_cache)?;
    let Some(Fields::Cell(fields)) = cell.fields.as_ref() else {
        unreachable!("selected CELL decoder")
    };
    if fields
        .flags
        .as_ref()
        .is_none_or(|field| field.value & 1 != 0)
        || fields.grid.is_none()
    {
        return Err(Error::Unsupported(
            "terrain inspection requires exterior CELL flags and XCLC coordinates".into(),
        ));
    }
    cell.links.insert(
        "group.world".into(),
        dependency(store, cell_location, world_raw, &[*b"WRLD"])?,
    );
    let cell_world = next_world.key.clone();
    let mut world_chain = Vec::new();
    let mut visited = BTreeSet::new();
    loop {
        if next_world.status == "null" {
            break;
        }
        if next_world.status != "resolved" {
            return Err(Error::Resolution(format!(
                "worldspace chain has {} link",
                next_world.status
            )));
        }
        let key = next_world.key.as_ref().expect("resolved world key").clone();
        if !visited.insert(key.clone()) {
            return Err(Error::Resolution("cycle in parent worldspace chain".into()));
        }
        if visited.len() > 256 {
            return Err(Error::Resolution(
                "parent worldspace chain exceeds inspection budget".into(),
            ));
        }
        let location = store.winners[&key];
        let world = entry(store, key, location, body_cache)?;
        let parent = world.links.get("WNAM").cloned();
        world_chain.push(world);
        match parent {
            Some(parent) => next_world = parent,
            None => break,
        }
    }
    let mut selected = Vec::new();
    for (key, location) in &store.winners {
        let definition = store.definition(*location);
        if definition.header.kind == *b"LAND"
            && definition
                .parent
                .cell
                .map(|raw| store.key_for(*location, raw))
                .transpose()?
                .flatten()
                .as_ref()
                == Some(&cell_key)
        {
            if definition
                .parent
                .world
                .map(|raw| store.key_for(*location, raw))
                .transpose()?
                .flatten()
                != cell_world
            {
                return Err(Error::Resolution(
                    "winning LAND world group differs from selected CELL".into(),
                ));
            }
            selected.push((key.clone(), *location));
        }
    }
    let landscapes = selected
        .into_iter()
        .map(|(key, location)| entry(store, key, location, body_cache))
        .collect::<Result<Vec<_>>>()?;
    let link_failures = std::iter::once(&cell)
        .chain(&world_chain)
        .chain(&landscapes)
        .flat_map(|record| record.links.values())
        .filter(|link| !["resolved", "null"].contains(&link.status))
        .count();
    Ok(TerrainReport {
        schema_version: 1,
        cell,
        world_chain,
        landscapes,
        index_payloads_deferred: store.deferred_payloads(),
        index_cache: store.index_cache_report().cloned(),
        integrity_failures: store.integrity_failures(),
        link_failures,
        runtime_ready: false,
        texture_dependencies: None,
        unknown: vec![
            "parent worldspace inheritance and editor-default application",
            "height-delta reconstruction, normal interpretation and measured axes/units",
            "terrain seams, layer blending, materials, water rendering and physics",
            "unhandled record fields, texture asset closure and retail archive precedence",
            "streaming, navigation, script execution and gameplay acceptance",
        ],
    })
}
