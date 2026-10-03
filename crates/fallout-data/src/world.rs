//! The first NV world schema: cell flags and placed-reference dependencies.
//! Coordinates remain in source units. No renderer or simulation is implied.
use crate::{
    Result,
    identity::FormKey,
    malformed,
    plugin::{self, Record, Subrecord},
    store::{Location, RecordStore},
    vfs::{AssetPath, AssetSource, MountIndex},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct SourceField<T> {
    pub decoded_offset: usize,
    pub value: T,
}

#[derive(Debug, Serialize)]
pub struct Cell {
    pub flags: SourceField<u8>,
    pub grid: Option<SourceField<[i32; 2]>>,
    pub full_name: Option<SourceField<Vec<u8>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

#[derive(Debug, Serialize)]
pub struct EnableParent {
    pub target_raw: u32,
    pub flags: u8,
}

#[derive(Debug, Serialize)]
pub struct Teleport {
    pub door_raw: u32,
    pub destination: Transform,
    pub flags: u32,
}

#[derive(Debug, Serialize)]
pub struct Placement {
    pub base: SourceField<u32>,
    pub transform: SourceField<Transform>,
    pub scale: Option<SourceField<f32>>,
    pub enable_parent: Option<SourceField<EnableParent>>,
    pub teleport: Option<SourceField<Teleport>>,
    pub unhandled_fields: BTreeMap<String, usize>,
}

fn size(sub: &Subrecord<'_>, expected: usize, name: &str, record: &Record) -> Result<()> {
    if sub.data.len() != expected {
        return Err(malformed(
            name,
            record.header.offset,
            format!(
                "{} at decoded +0x{:X}: expected {expected} bytes, got {}",
                plugin::signature(sub.kind),
                sub.payload_offset,
                sub.data.len()
            ),
        ));
    }
    Ok(())
}

fn single<T>(
    slot: &mut Option<SourceField<T>>,
    sub: &Subrecord<'_>,
    value: T,
    name: &str,
    record: &Record,
) -> Result<()> {
    if slot.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            format!("duplicate {} field", plugin::signature(sub.kind)),
        ));
    }
    *slot = Some(SourceField {
        decoded_offset: sub.payload_offset,
        value,
    });
    Ok(())
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("field size checked"))
}

fn transform(data: &[u8], name: &str, offset: u64) -> Result<Transform> {
    let mut components = [0.0f32; 6];
    for (n, component) in components.iter_mut().enumerate() {
        *component = f32::from_bits(u32_at(data, n * 4));
        if !component.is_finite() {
            return Err(malformed(name, offset, "non-finite placement transform"));
        }
    }
    Ok(Transform {
        position: components[..3].try_into().expect("three components"),
        rotation: components[3..].try_into().expect("three components"),
    })
}

pub fn decode_cell(record: &Record, name: &str) -> Result<Cell> {
    if record.header.kind != *b"CELL" {
        return Err(malformed(name, record.header.offset, "expected CELL"));
    }
    let (mut flags, mut grid, mut full_name) = (None, None, None);
    plugin::visit_subrecords(record, name, |sub| {
        match &sub.kind {
            b"DATA" => {
                size(&sub, 1, name, record)?;
                single(&mut flags, &sub, sub.data[0], name, record)?;
            }
            b"XCLC" => {
                if ![8, 12].contains(&sub.data.len()) {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        "unsupported cell grid size",
                    ));
                }
                single(
                    &mut grid,
                    &sub,
                    [u32_at(sub.data, 0) as i32, u32_at(sub.data, 4) as i32],
                    name,
                    record,
                )?;
            }
            b"FULL" => single(
                &mut full_name,
                &sub,
                terminated(sub.data, name, record.header.offset)?.to_vec(),
                name,
                record,
            )?,
            _ => {}
        }
        Ok(())
    })?;
    Ok(Cell {
        flags: flags
            .ok_or_else(|| malformed(name, record.header.offset, "CELL lacks DATA flags"))?,
        grid,
        full_name,
    })
}

pub fn decode_placement(record: &Record, name: &str) -> Result<Placement> {
    if ![*b"REFR", *b"ACHR", *b"ACRE"].contains(&record.header.kind) {
        return Err(malformed(
            name,
            record.header.offset,
            "unsupported placed-reference record kind",
        ));
    }
    let (mut base, mut position, mut scale, mut enable_parent, mut teleport) =
        (None, None, None, None, None);
    let mut unhandled_fields = BTreeMap::new();
    plugin::visit_subrecords(record, name, |sub| {
        match &sub.kind {
            b"NAME" => {
                size(&sub, 4, name, record)?;
                single(&mut base, &sub, u32_at(sub.data, 0), name, record)?;
            }
            b"DATA" => {
                size(&sub, 24, name, record)?;
                single(
                    &mut position,
                    &sub,
                    transform(sub.data, name, record.header.offset)?,
                    name,
                    record,
                )?;
            }
            b"XSCL" => {
                size(&sub, 4, name, record)?;
                let value = f32::from_bits(u32_at(sub.data, 0));
                if !value.is_finite() || value <= 0.0 {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        "unsupported nonpositive or non-finite scale",
                    ));
                }
                single(&mut scale, &sub, value, name, record)?;
            }
            b"XESP" => {
                size(&sub, 8, name, record)?;
                single(
                    &mut enable_parent,
                    &sub,
                    EnableParent {
                        target_raw: u32_at(sub.data, 0),
                        flags: sub.data[4],
                    },
                    name,
                    record,
                )?;
            }
            b"XTEL" => {
                size(&sub, 32, name, record)?;
                let value = Teleport {
                    door_raw: u32_at(sub.data, 0),
                    destination: transform(&sub.data[4..28], name, record.header.offset)?,
                    flags: u32_at(sub.data, 28),
                };
                single(&mut teleport, &sub, value, name, record)?;
            }
            _ => {
                *unhandled_fields
                    .entry(plugin::signature(sub.kind))
                    .or_default() += 1;
            }
        }
        Ok(())
    })?;
    Ok(Placement {
        base: base.ok_or_else(|| malformed(name, record.header.offset, "placement lacks NAME"))?,
        transform: position
            .ok_or_else(|| malformed(name, record.header.offset, "placement lacks DATA"))?,
        scale,
        enable_parent,
        teleport,
        unhandled_fields,
    })
}

pub fn terminated<'a>(bytes: &'a [u8], name: &str, offset: u64) -> Result<&'a [u8]> {
    let value = bytes
        .strip_suffix(&[0])
        .ok_or_else(|| malformed(name, offset, "string is not NUL terminated"))?;
    if value.contains(&0) {
        return Err(malformed(name, offset, "embedded NUL in string"));
    }
    Ok(value)
}

/// MODL is inventoried as a dependency. Actors, alternate weapon/armor models,
/// construction sets, and runtime model selection still need their own adapters.
pub fn model_path(record: &Record, name: &str) -> Result<Option<SourceField<Vec<u8>>>> {
    let mut path = None;
    plugin::visit_subrecords(record, name, |sub| {
        if sub.kind == *b"MODL" {
            let raw = terminated(sub.data, name, record.header.offset)?;
            if !raw.is_empty() {
                single(&mut path, &sub, raw.to_vec(), name, record)?;
            }
        }
        Ok(())
    })?;
    Ok(path)
}

#[derive(Debug, Clone, Serialize)]
pub struct Dependency {
    pub key: Option<FormKey>,
    pub status: &'static str,
    pub kind: Option<String>,
    pub expected_kinds: Vec<String>,
}

pub(crate) fn dependency(
    store: &RecordStore,
    owner: Location,
    raw: u32,
    expected: &[[u8; 4]],
) -> Result<Dependency> {
    let key = store.key_for(owner, raw)?;
    let (status, kind) = match key.as_ref() {
        None => ("null", None),
        Some(key) if key.origin_plugin == "falloutnv.esm" && key.local_id == 0x14 => (
            if expected.contains(b"PLYR") {
                "runtime-player-binding-unimplemented"
            } else {
                "wrong-record-kind"
            },
            Some("PLYR".into()),
        ),
        Some(key) => match store.winners.get(key) {
            Some(location) => {
                let record = store.definition(*location);
                (
                    if record.header.flags & plugin::DELETED != 0 {
                        "deleted"
                    } else if !expected.contains(&record.header.kind) {
                        "wrong-record-kind"
                    } else {
                        "resolved"
                    },
                    Some(plugin::signature(record.header.kind)),
                )
            }
            None => ("missing", None),
        },
    };
    Ok(Dependency {
        key,
        status,
        kind,
        expected_kinds: expected.iter().copied().map(plugin::signature).collect(),
    })
}

const REFERENCE_BASE_KINDS: &[[u8; 4]] = &[
    *b"TREE", *b"SOUN", *b"ACTI", *b"DOOR", *b"STAT", *b"FURN", *b"CONT", *b"ARMO", *b"AMMO",
    *b"LVLN", *b"LVLC", *b"MISC", *b"WEAP", *b"BOOK", *b"KEYM", *b"ALCH", *b"LIGH", *b"GRAS",
    *b"ASPC", *b"IDLM", *b"ARMA", *b"CHIP", *b"MSTT", *b"NOTE", *b"PWAT", *b"SCOL", *b"TACT",
    *b"TERM", *b"TXST", *b"CCRD", *b"IMOD", *b"CMNY",
];
const NPC_BASE_KIND: &[[u8; 4]] = &[*b"NPC_"];
const CREATURE_BASE_KIND: &[[u8; 4]] = &[*b"CREA"];

fn base_kinds(kind: [u8; 4]) -> &'static [[u8; 4]] {
    // NAME target kinds from the pinned FNV schema. Base-form runtime behavior
    // still belongs in dedicated game rules and presentation adapters.
    match &kind {
        b"ACHR" => NPC_BASE_KIND,
        b"ACRE" => CREATURE_BASE_KIND,
        _ => REFERENCE_BASE_KINDS,
    }
}

#[derive(Debug, Serialize)]
pub struct ModelDependency {
    pub base_key: FormKey,
    pub base_kind: String,
    pub source_plugin: String,
    pub record_offset: u64,
    pub model_field: Option<SourceField<Vec<u8>>>,
    pub asset_path: Option<AssetPath>,
    pub candidates: Vec<AssetSource>,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PlacedEntry {
    pub key: FormKey,
    pub source_plugin: String,
    pub record_offset: u64,
    pub record_kind: String,
    pub record_flags: u32,
    pub child_group: Option<i32>,
    pub placement: Option<Placement>,
    pub base: Option<Dependency>,
    pub enable_parent: Option<Dependency>,
    pub teleport_door: Option<Dependency>,
}

#[derive(Debug, Serialize)]
pub struct CellReport {
    pub schema_version: u32,
    pub key: FormKey,
    pub editor_id: Vec<u8>,
    pub source_plugin: String,
    pub record_offset: u64,
    pub cell: Cell,
    pub integrity_failures: usize,
    pub index_payloads_deferred: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_cache: Option<crate::index_cache::Report>,
    pub link_failures: usize,
    pub references: Vec<PlacedEntry>,
    pub models: Vec<ModelDependency>,
    pub model_probes: Vec<crate::model_probe::ModelProbe>,
    pub other_child_records: BTreeMap<String, usize>,
    pub runtime_ready: bool,
    pub unknown: Vec<&'static str>,
}

pub fn inspect_cell(
    store: &mut RecordStore,
    editor_id: &[u8],
    mounts: &MountIndex,
) -> Result<CellReport> {
    let (cell_key, cell_location) = store.cell_by_editor_id(editor_id)?;
    let cell_source = store.source_name(cell_location).to_owned();
    let cell_record = store.read(cell_location)?;
    let cell = decode_cell(&cell_record, &cell_source)?;
    let mut selected = Vec::new();
    // A moved override belongs to its winning parent. Earlier cell membership must
    // not leak into this cell, and a deleted winner must not resurrect its predecessor.
    for (key, location) in &store.winners {
        if let Some(raw) = store.definition(*location).parent.cell
            && store.key_for(*location, raw)?.as_ref() == Some(&cell_key)
        {
            selected.push((key.clone(), *location));
        }
    }
    let mut references = Vec::new();
    let mut bases = BTreeMap::new();
    let mut other_child_records = BTreeMap::new();
    for (key, location) in selected {
        let definition = store.definition(location);
        let header = definition.header.clone();
        if ![*b"REFR", *b"ACHR", *b"ACRE"].contains(&header.kind) {
            *other_child_records
                .entry(plugin::signature(header.kind))
                .or_default() += 1;
            continue;
        }
        let mut entry = PlacedEntry {
            key,
            source_plugin: store.source_name(location).to_owned(),
            record_offset: header.offset,
            record_kind: plugin::signature(header.kind),
            record_flags: header.flags,
            child_group: definition.parent.child_group,
            placement: None,
            base: None,
            enable_parent: None,
            teleport_door: None,
        };
        if header.flags & plugin::DELETED == 0 {
            let record = store.read(location)?;
            let placement = decode_placement(&record, &entry.source_plugin)?;
            let base = dependency(
                store,
                location,
                placement.base.value,
                base_kinds(header.kind),
            )?;
            if base.status == "resolved"
                && let Some(key) = &base.key
            {
                bases.insert(key.clone(), store.winners[key]);
            }
            entry.enable_parent = placement
                .enable_parent
                .as_ref()
                .map(|v| {
                    dependency(
                        store,
                        location,
                        v.value.target_raw,
                        &[
                            *b"PLYR", *b"REFR", *b"ACRE", *b"ACHR", *b"PGRE", *b"PMIS", *b"PBEA",
                        ],
                    )
                })
                .transpose()?;
            entry.teleport_door = placement
                .teleport
                .as_ref()
                .map(|v| dependency(store, location, v.value.door_raw, &[*b"REFR"]))
                .transpose()?;
            entry.base = Some(base);
            entry.placement = Some(placement);
        }
        references.push(entry);
    }
    let mut models = Vec::new();
    for (base_key, location) in bases {
        let name = store.source_name(location).to_owned();
        let record = store.read(location)?;
        let field = model_path(&record, &name)?;
        let asset_path = field
            .as_ref()
            .map(|f| {
                let mut path = b"meshes/".to_vec();
                path.extend(&f.value);
                AssetPath::new(&path)
            })
            .transpose()?;
        let candidates = asset_path
            .as_ref()
            .map(|p| mounts.candidates(p.bytes()).map(<[_]>::to_vec))
            .transpose()?
            .unwrap_or_default();
        let status = match (field.is_some(), candidates.len()) {
            (false, _) => "no-modl-field; runtime-model-selection-unimplemented",
            (true, 0) => "missing-in-archive-index; loose-lookup-unimplemented",
            (true, 1) => "one-archive-candidate; effective-retail-precedence-unverified",
            _ => "ambiguous; precedence-unverified",
        };
        models.push(ModelDependency {
            base_key,
            base_kind: plugin::signature(record.header.kind),
            source_plugin: name,
            record_offset: record.header.offset,
            model_field: field,
            asset_path,
            candidates,
            status,
        });
    }
    let link_failures = references
        .iter()
        .flat_map(|r| [&r.base, &r.enable_parent, &r.teleport_door])
        .flatten()
        .filter(|d| !["resolved", "runtime-player-binding-unimplemented"].contains(&d.status))
        .count();
    Ok(CellReport {
        schema_version: 1,
        key: cell_key,
        editor_id: editor_id.to_vec(),
        source_plugin: cell_source,
        record_offset: cell_record.header.offset,
        cell,
        integrity_failures: store.integrity_failures(),
        index_payloads_deferred: store.deferred_payloads(),
        index_cache: store.index_cache_report().cloned(),
        link_failures,
        references,
        models,
        model_probes: Vec::new(),
        other_child_records,
        runtime_ready: false,
        unknown: vec![
            "retail archive/loose lookup precedence",
            "non-MODL model selection and actors",
            "unhandled record fields and constraints beyond decoded links",
            "cell lighting/water/navmesh semantics",
            "enable-parent evaluation and teleport behavior",
            "axis/unit conversion, rendering, collision, animation, scripts",
        ],
    })
}
