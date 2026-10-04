//! A bounded source graph for one exact winning CELL. This is not activation state.
mod budget;
pub use budget::{Limits, Usage};

use super::{
    Cell, Dependency, ENABLE_PARENT_KINDS, Placement, base_kinds, decode_cell, decode_placement,
    dependency,
};
use crate::{
    Error, Result,
    content::ParentContext,
    graph::cyclic_components,
    identity::{FormKey, ProfileId},
    plugin::{self, RecordHeader},
    store::{Location, RecordStore, SourceReceipt},
    terrain::{self, Fields, Worldspace},
};
use budget::{Budget, charge, failure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Span {
    /// First data byte, after the six-byte subrecord header. Existing typed
    /// decoders separately retain their original header offsets unchanged.
    pub decoded_offset: usize,
    pub bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct FieldSite {
    pub kind: [u8; 4],
    pub decoded_header_offset: usize,
    pub span: Span,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "fields")]
pub enum Decoded {
    Cell(Cell),
    World(Box<Worldspace>),
    Placement(Placement),
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub key: FormKey,
    pub source_ordinal: usize,
    pub source_plugin: String,
    pub header: RecordHeader,
    pub parent: ParentContext,
    /// Header-only terminals do not establish payload integrity or semantics.
    pub decoded_sha256: Option<String>,
    pub decoded_body: Option<Vec<u8>>,
    pub fields: Option<Decoded>,
    pub field_sites: Vec<FieldSite>,
}

#[derive(Debug, Serialize)]
pub struct Site {
    pub source_ordinal: usize,
    pub record_offset: u64,
    /// Group labels come from indexed ancestry; their absolute group offsets are
    /// unavailable here. None never claims that a group label is a payload field.
    pub field: Option<Span>,
}

#[derive(Debug, Serialize)]
pub struct Edge {
    pub owner: FormKey,
    pub role: &'static str,
    pub raw_form_id: u32,
    pub site: Site,
    pub target: Dependency,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub profile: ProfileId,
    pub root: FormKey,
    pub sources: Vec<SourceReceipt>,
    pub source_cohort_sha256: String,
    /// Nodes sort by supplied source ordinal then physical winning-header offset.
    pub nodes: Vec<Node>,
    /// Field edges sort by physical decoded offset; contextual edges have no span.
    pub edges: Vec<Edge>,
    /// Indices into nodes. Only WNAM/XESP/XTEL enter this projection, so ordinary
    /// membership backlinks do not obscure actual typed-link cycles.
    pub cyclic_link_components: Vec<Vec<usize>>,
    pub root_members: usize,
    pub integrity_failures: usize,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
    pub unverified: Vec<&'static str>,
}

struct Builder<'a> {
    store: &'a mut RecordStore,
    budget: Budget,
    nodes: Vec<Node>,
    keys: BTreeMap<FormKey, usize>,
    expand_requested: Vec<bool>,
    pending: VecDeque<usize>,
    edges: Vec<Edge>,
}

impl Builder<'_> {
    fn admit(&mut self, key: &FormKey, expand: bool) -> Result<Option<usize>> {
        if let Some(&index) = self.keys.get(key) {
            if expand
                && !self.expand_requested[index]
                && matches!(
                    &self.nodes[index].header.kind,
                    b"CELL" | b"WRLD" | b"REFR" | b"ACHR" | b"ACRE"
                )
            {
                self.expand_requested[index] = true;
                self.pending.push_back(index);
            }
            return Ok(Some(index));
        }
        let Some(location) = self.store.winner(key) else {
            return Ok(None);
        };
        let name = self.store.source_name(location);
        let expand = expand
            && matches!(
                &self.store.definition(location).header.kind,
                b"CELL" | b"WRLD" | b"REFR" | b"ACHR" | b"ACRE"
            );
        // Includes key map copies, queue/graph/sort storage and fixed headers.
        self.budget
            .node(768 + 4 * key.origin_plugin.len() + 2 * name.len())?;
        let definition = self.store.definition(location);
        let index = self.nodes.len();
        self.nodes.push(Node {
            key: key.clone(),
            source_ordinal: location.plugin,
            source_plugin: name.to_owned(),
            header: definition.header.clone(),
            parent: definition.parent.clone(),
            decoded_sha256: None,
            decoded_body: None,
            fields: None,
            field_sites: Vec::new(),
        });
        self.keys.insert(key.clone(), index);
        self.expand_requested.push(expand);
        if expand {
            self.pending.push_back(index);
        }
        Ok(Some(index))
    }

    fn edge(
        &mut self,
        owner: &FormKey,
        location: Location,
        site: (&'static str, Option<Span>),
        raw: u32,
        expected: &[[u8; 4]],
        expand: bool,
    ) -> Result<()> {
        let (role, field) = site;
        // key_for/dependency allocate a target key. Precharge its longest possible
        // owner/master origin before invoking either existing resolver.
        let origin_maximum = self.store.indices()[location.plugin]
            .census
            .masters
            .iter()
            .map(String::len)
            .chain(std::iter::once(self.store.source_name(location).len()))
            .max()
            .unwrap_or(0);
        self.budget
            .edge(512 + 2 * owner.origin_plugin.len() + 2 * origin_maximum + 64 * expected.len())?;
        let target = dependency(self.store, location, raw, expected)?;
        if let Some(key) = &target.key {
            self.admit(key, expand && target.status == "resolved")?;
        }
        self.edges.push(Edge {
            owner: owner.clone(),
            role,
            raw_form_id: raw,
            site: Site {
                source_ordinal: location.plugin,
                record_offset: self.store.definition(location).header.offset,
                field,
            },
            target,
        });
        Ok(())
    }

    fn expand(&mut self, index: usize) -> Result<()> {
        let node = &self.nodes[index];
        if node.header.flags & plugin::DELETED != 0 {
            return Ok(());
        }
        self.budget.metadata(node.key.origin_plugin.len())?;
        let key = node.key.clone();
        let location = self.store.winner(&key).expect("admitted winning identity");
        let maximum = self.budget.read_maximum();
        if maximum == 0 {
            return Err(failure("decoded bytes", "no record allowance remains"));
        }
        // Both stored and declared decoded lengths are rejected before allocation
        // by the existing strict reader. The aggregate remainder cannot be bypassed.
        let record = self.store.read_bounded(location, maximum)?;
        if record.integrity_issue.is_some() {
            return Err(Error::Resolution(
                "world dependency refuses tainted requested body".into(),
            ));
        }
        self.budget.decoded(record.payload.len())?;
        // Decoder-owned strings/raw fields copy at most one body; reserve that
        // and a conservative per-site allowance before collecting or decoding.
        self.budget.metadata(record.payload.len())?;
        let name = &self.nodes[index].source_plugin;
        let mut count = 0;
        plugin::visit_subrecords(&record, name, |sub| {
            self.budget.field(256 + std::mem::size_of::<FieldSite>())?;
            let _ = sub;
            count += 1;
            Ok(())
        })?;
        let mut sites = Vec::with_capacity(count);
        plugin::visit_subrecords(&record, name, |sub| {
            sites.push(FieldSite {
                kind: sub.kind,
                decoded_header_offset: sub.payload_offset,
                span: Span {
                    decoded_offset: sub.payload_offset + 6,
                    bytes: sub.data.len(),
                },
            });
            Ok(())
        })?;
        let decoded = match &record.header.kind {
            b"CELL" => {
                let cell = decode_cell(&record, name)?;
                if let Some(raw) = self.nodes[index].parent.world {
                    self.edge(
                        &key,
                        location,
                        ("group.world", None),
                        raw,
                        &[*b"WRLD"],
                        true,
                    )?;
                }
                Decoded::Cell(cell)
            }
            b"WRLD" => {
                let Fields::World(world) = terrain::decode(&record, name)? else {
                    unreachable!("WRLD decoder")
                };
                for (role, link, expected, expand) in [
                    ("WNAM", &world.parent, *b"WRLD", true),
                    ("CNAM", &world.climate, *b"CLMT", false),
                    ("NAM2", &world.water, *b"WATR", false),
                    ("NAM3", &world.lod_water, *b"WATR", false),
                    ("INAM", &world.image_space, *b"IMGS", false),
                    ("XEZN", &world.encounter_zone, *b"ECZN", false),
                    ("ZNAM", &world.music, *b"MUSC", false),
                ] {
                    if let Some(link) = link {
                        self.edge(
                            &key,
                            location,
                            (
                                role,
                                Some(Span {
                                    decoded_offset: link.decoded_offset + 6,
                                    bytes: 4,
                                }),
                            ),
                            link.value,
                            &[expected],
                            expand,
                        )?;
                    }
                }
                Decoded::World(Box::new(world))
            }
            b"REFR" | b"ACHR" | b"ACRE" => {
                let placement = decode_placement(&record, name)?;
                if let Some(raw) = self.nodes[index].parent.cell {
                    self.edge(
                        &key,
                        location,
                        ("group.cell", None),
                        raw,
                        &[*b"CELL"],
                        false,
                    )?;
                }
                self.edge(
                    &key,
                    location,
                    (
                        "NAME",
                        Some(Span {
                            decoded_offset: placement.base.decoded_offset + 6,
                            bytes: 4,
                        }),
                    ),
                    placement.base.value,
                    base_kinds(record.header.kind),
                    false,
                )?;
                if let Some(link) = &placement.enable_parent {
                    self.edge(
                        &key,
                        location,
                        (
                            "XESP",
                            Some(Span {
                                decoded_offset: link.decoded_offset + 6,
                                bytes: 8,
                            }),
                        ),
                        link.value.target_raw,
                        ENABLE_PARENT_KINDS,
                        true,
                    )?;
                }
                if let Some(link) = &placement.teleport {
                    self.edge(
                        &key,
                        location,
                        (
                            "XTEL",
                            Some(Span {
                                decoded_offset: link.decoded_offset + 6,
                                bytes: 32,
                            }),
                        ),
                        link.value.door_raw,
                        &[*b"REFR"],
                        true,
                    )?;
                }
                Decoded::Placement(placement)
            }
            _ => unreachable!("only explicit root, worlds and generic placements expand"),
        };
        let node = &mut self.nodes[index];
        node.decoded_sha256 = Some(format!("{:x}", Sha256::digest(&record.payload)));
        node.decoded_body = Some(record.payload);
        node.field_sites = sites;
        node.fields = Some(decoded);
        Ok(())
    }
}

/// Inspect exact winning source dependencies, preserving unresolved edges and
/// tombstones. Other CELLs, actor extras, LAND and navmesh bodies stay deferred.
/// No result escapes on an admission, framing, integrity or decoding failure.
pub fn inspect_cell_key(store: &mut RecordStore, root: &FormKey, limits: Limits) -> Result<Report> {
    let mut budget = Budget::new(limits)?;
    budget.metadata(1024 + 2 * root.origin_plugin.len())?;
    if root.profile != ProfileId::NvOriginal {
        return Err(Error::Unsupported(
            "world dependency root requires nv-original identity".into(),
        ));
    }
    let root_location = store
        .winner(root)
        .ok_or_else(|| Error::Resolution("world dependency root CELL missing".into()))?;
    let header = &store.definition(root_location).header;
    if header.kind != *b"CELL" || header.flags & plugin::DELETED != 0 {
        return Err(Error::Resolution(
            "world dependency root CELL deleted or wrong-record-kind".into(),
        ));
    }
    charge(
        &mut budget.usage.winners_scanned,
        store.winners.len(),
        limits.max_winners_scanned,
        "winning scan",
    )?;
    charge(
        &mut budget.usage.sources,
        store.indices().len(),
        limits.max_sources,
        "sources",
    )?;
    for source in store.indices() {
        budget.metadata(512 + 2 * source.census.name.len())?;
    }
    // Admission precedes the shared receipt method's bounded vector/name clones.
    let sources = store.source_receipts()?;
    let cohort = source_cohort(&sources);
    let mut builder = Builder {
        store,
        budget,
        nodes: Vec::new(),
        keys: BTreeMap::new(),
        expand_requested: Vec::new(),
        pending: VecDeque::new(),
        edges: Vec::new(),
    };
    builder.admit(root, true)?;
    let mut members = Vec::new();
    // The scan reads only existing indexed metadata. Reserve each candidate
    // before its key clone; resolve group labels in that winning source's table.
    for (key, location) in builder.store.winning_definitions() {
        let definition = builder.store.definition(location);
        if let Some(raw) = definition.parent.cell {
            let census = &builder.store.indices()[location.plugin].census;
            let origin_maximum = census
                .masters
                .iter()
                .map(String::len)
                .chain(std::iter::once(census.name.len()))
                .max()
                .unwrap_or(0);
            // A single temporary resolver key is bounded independently of the
            // number of unrelated headers scanned; it is dropped each iteration.
            if origin_maximum
                > builder
                    .budget
                    .limits
                    .max_metadata_bytes
                    .saturating_sub(builder.budget.usage.metadata_bytes)
            {
                return Err(failure(
                    "metadata bytes",
                    "temporary group identity exceeds remainder",
                ));
            }
            if builder.store.key_for(location, raw)?.as_ref() == Some(root) {
                // The member vector is capped before any collection. admit later
                // deduplicates targets, but each root member needs its own node.
                if builder.budget.usage.nodes + members.len() >= builder.budget.limits.max_nodes {
                    return Err(failure("nodes", "root member collection exceeded"));
                }
                builder.budget.metadata(128 + key.origin_plugin.len())?;
                members.push((key.clone(), location));
            }
        }
    }
    members.sort_unstable_by_key(|(_, location)| {
        (
            location.plugin,
            builder.store.definition(*location).header.offset,
        )
    });
    let root_members = members.len();
    for (key, location) in members {
        let header = &builder.store.definition(location).header;
        let kind = header.kind;
        let raw = header.form_id;
        builder.edge(
            root,
            location,
            ("member", None),
            raw,
            &[kind],
            matches!(&kind, b"REFR" | b"ACHR" | b"ACRE"),
        )?;
        debug_assert!(builder.keys.contains_key(&key));
    }
    while let Some(index) = builder.pending.pop_front() {
        builder.expand(index)?;
    }
    builder
        .nodes
        .sort_unstable_by_key(|node| (node.source_ordinal, node.header.offset));
    builder.edges.sort_by_key(|edge| {
        (
            edge.site.source_ordinal,
            edge.site.record_offset,
            edge.site.field.map(|span| span.decoded_offset),
            edge.role,
        )
    });
    let ordinals: BTreeMap<_, _> = builder
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.key, index))
        .collect();
    let mut children = vec![Vec::new(); builder.nodes.len()];
    for edge in &builder.edges {
        if matches!(edge.role, "WNAM" | "XESP" | "XTEL")
            && edge.target.status == "resolved"
            && let Some(target) = edge.target.key.as_ref().and_then(|key| ordinals.get(key))
        {
            children[ordinals[&edge.owner]].push(*target);
        }
    }
    let cyclic_link_components = cyclic_components(&children);
    Ok(Report {
        schema_version: 1,
        profile: ProfileId::NvOriginal,
        root: root.clone(),
        sources,
        source_cohort_sha256: cohort,
        nodes: builder.nodes,
        edges: builder.edges,
        cyclic_link_components,
        root_members,
        integrity_failures: builder.store.integrity_failures(),
        usage: builder.budget.usage,
        limits,
        runtime_ready: false,
        unverified: vec![
            "terminal payloads, actor extras, scripts, LAND/navmesh and unhandled field dependencies",
            "retail parent-world inheritance, enablement, teleport and destination-cell activation",
            "archive/model precedence, streaming, physics, input and gameplay acceptance",
        ],
    })
}

pub(super) fn source_cohort(sources: &[SourceReceipt]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"nv-world-source-cohort-v1\0");
    for (ordinal, source) in sources.iter().enumerate() {
        hash.update((ordinal as u64).to_le_bytes());
        hash.update((source.source_name.len() as u64).to_le_bytes());
        hash.update(source.source_name.as_bytes());
        hash.update(source.source_bytes.to_le_bytes());
        hash.update(source.source_sha256.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
