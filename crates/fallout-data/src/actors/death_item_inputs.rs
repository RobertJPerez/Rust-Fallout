//! Selected authored death-item declarations over the existing leveled producer.
//! Structural candidates are never rolled items or a death-event authority.
use super::{Catalogue, Definition, associations, effect_inputs::SourceRequest};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, leveled, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_depth: usize,
    pub max_lists: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_visits: usize,
    pub max_fields: usize,
    pub max_entries: usize,
    pub max_bindings: usize,
    pub max_headers: usize,
    pub max_raw_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_depth: 32,
            max_lists: 4096,
            max_record_bytes: 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_field_visits: 200_000,
            max_fields: 65_536,
            max_entries: 16_384,
            max_bindings: 65_536,
            max_headers: 65_536,
            max_raw_bytes: 1024 * 1024,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Field<'a> {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub sha256: &'a str,
    /// Existing typed producer value, withheld for unverified record versions.
    pub value: Option<&'a leveled::Value>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Item,
    ChanceGlobal,
    Owner,
    ExtraGlobal,
}
#[derive(Debug, Serialize)]
pub struct Link<'a> {
    pub field_index: usize,
    pub role: Role,
    pub binding: &'a inventory::Binding,
    pub source: Option<SourceRequest>,
    pub schema_kind_allowed: Option<bool>,
    pub structural_binding_available: bool,
    pub nested_node: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Entry<'a> {
    pub lvlo_field: usize,
    pub coed_fields: &'a [usize],
    pub item_link: usize,
    pub unique_extra_available: bool,
}
#[derive(Debug, Serialize)]
pub struct Node<'a> {
    pub source: SourceRequest,
    pub decoded_record_sha256: String,
    pub record_version_supported: bool,
    pub fields: Vec<Field<'a>>,
    pub entries: Vec<Entry<'a>>,
    pub links: Vec<Link<'a>>,
    pub chance_field_indices: Vec<usize>,
    pub flag_field_indices: Vec<usize>,
    pub global_field_indices: Vec<usize>,
    pub metadata_singletons_unambiguous: bool,
    pub findings: &'a [inventory::fields::Finding],
    pub issues: Vec<&'static str>,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub lists: usize,
    pub source_depth: usize,
    pub field_visits: usize,
    pub fields: usize,
    pub entries: usize,
    pub bindings: usize,
    pub headers: usize,
    pub decoded_bytes: usize,
    pub raw_bytes: usize,
}
/// Private construction and no Deserialize prevent a report becoming authority.
#[derive(Debug, Serialize)]
pub struct Manifest<'a> {
    sources: &'a [SourceReceipt],
    winning_content_sha256: &'a str,
    actor: &'a Definition<'a>,
    configuration_fields: Vec<&'a inventory::Field>,
    traits_template_flag: Option<bool>,
    association: &'a associations::Association,
    actor_field: &'a super::fields::Field,
    actor_raw_bytes: Vec<u8>,
    singleton_repeated: bool,
    declaration_binding_available: bool,
    death_item_source: Option<SourceRequest>,
    root_node: Option<usize>,
    nodes: Vec<Node<'a>>,
    counts: Counts,
    issues: Vec<&'static str>,
    items_created: bool,
    death_event_verified: bool,
    roll_supported: bool,
    scope: &'static str,
}
impl Manifest<'_> {
    pub fn nodes(&self) -> &[Node<'_>] {
        &self.nodes
    }
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor death-item input {label} budget exceeded"
        )))
    }
}
fn add(v: &mut usize, n: usize, max: usize, label: &str) -> Result<()> {
    *v = v
        .checked_add(n)
        .ok_or_else(|| Error::Unsupported(format!("actor death-item input {label} overflow")))?;
    budget(*v <= max, label)
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn source(store: &RecordStore, receipts: &[SourceReceipt], key: &FormKey) -> Result<SourceRequest> {
    let at = store
        .winner(key)
        .ok_or_else(|| Error::Resolution("actor death-item winner missing".into()))?;
    let r = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor death-item source missing".into()))?;
    Ok(SourceRequest {
        key: key.clone(),
        source_name: r.source_name.clone(),
        source_bytes: r.source_bytes,
        source_sha256: r.source_sha256.clone(),
        header: store.definition(at).header.clone(),
    })
}
fn bound_source(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    binding: &inventory::Binding,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Option<SourceRequest>> {
    add(&mut counts.bindings, 1, limits.max_bindings, "binding")?;
    let Some(key) = &binding.key else {
        return Ok(None);
    };
    if store.winner(key).is_none() {
        return Ok(None);
    }
    add(&mut counts.headers, 1, limits.max_headers, "header")?;
    let s = source(store, receipts, key)?;
    let t = binding
        .target
        .as_ref()
        .ok_or_else(|| Error::Resolution("actor death-item bound header missing".into()))?;
    if t.kind != s.header.kind
        || t.source_plugin != s.source_name
        || t.record_file_offset != s.header.offset
        || t.record_flags != s.header.flags
    {
        return Err(Error::Resolution(
            "actor death-item bound header differs".into(),
        ));
    }
    Ok(Some(s))
}
fn node<'a>(
    store: &mut RecordStore,
    receipts: &[SourceReceipt],
    lists: &'a leveled::Catalogue,
    key: &FormKey,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Node<'a>> {
    add(&mut counts.lists, 1, limits.max_lists, "list")?;
    let d = lists
        .get(key)
        .filter(|d| !d.deleted && d.kind == *b"LVLI")
        .ok_or_else(|| Error::Resolution("actor death-item retained list missing".into()))?;
    let r = d
        .record()
        .ok_or_else(|| Error::Resolution("actor death-item retained list body missing".into()))?;
    budget(r.payload.len() <= limits.max_record_bytes, "record byte")?;
    add(
        &mut counts.decoded_bytes,
        r.payload.len(),
        limits.max_decoded_bytes,
        "decoded byte",
    )?;
    add(
        &mut counts.fields,
        d.fields.len(),
        limits.max_fields,
        "field",
    )?;
    add(
        &mut counts.field_visits,
        d.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    let s = source(store, receipts, key)?;
    if r.header != s.header
        || d.source.plugin != s.source_name
        || d.source.sha256 != s.source_sha256
        || d.source.record_file_offset != s.header.offset
        || d.source.record_flags != s.header.flags
    {
        return Err(Error::Resolution(
            "actor death-item retained list identity differs".into(),
        ));
    }
    let digest = format!("{:x}", Sha256::digest(&r.payload));
    if r.integrity_issue.is_some()
        || d.source.decoded_record_sha256.as_deref() != Some(digest.as_str())
    {
        return Err(Error::Resolution(
            "actor death-item retained list bytes differ".into(),
        ));
    }
    let at = store.winner(key).expect("source admitted winner");
    let current = store.read_bounded(at, limits.max_record_bytes)?;
    if current.header != r.header
        || current.payload != r.payload
        || current.integrity_issue.is_some()
    {
        return Err(Error::Resolution(
            "actor death-item current list bytes differ".into(),
        ));
    }
    let supported = r.header.version == 15;
    let mut n = Node {
        source: s,
        decoded_record_sha256: digest,
        record_version_supported: supported,
        fields: Vec::new(),
        entries: Vec::new(),
        links: Vec::new(),
        chance_field_indices: Vec::new(),
        flag_field_indices: Vec::new(),
        global_field_indices: Vec::new(),
        metadata_singletons_unambiguous: false,
        findings: &d.findings,
        issues: Vec::new(),
    };
    if !supported {
        n.issues.push("unsupported_list_record_version");
    }
    plugin::visit_subrecords(r, &d.source.plugin, |f| {
        add(
            &mut counts.field_visits,
            1,
            limits.max_field_visits,
            "field visit",
        )?;
        add(
            &mut counts.raw_bytes,
            f.data.len(),
            limits.max_raw_bytes,
            "raw byte",
        )?;
        let index = n.fields.len();
        let cached = d
            .fields
            .get(index)
            .ok_or_else(|| Error::Resolution("actor death-item list field missing".into()))?;
        if cached.kind != f.kind
            || cached.decoded_offset as usize != f.payload_offset
            || cached.bytes != f.data.len()
            || cached.sha256 != format!("{:x}", Sha256::digest(f.data))
        {
            return Err(Error::Resolution(
                "actor death-item physical list field differs".into(),
            ));
        }
        n.fields.push(Field {
            kind: f.kind,
            decoded_offset: cached.decoded_offset,
            raw_bytes: f.data.to_vec(),
            sha256: &cached.sha256,
            value: supported.then_some(&cached.value),
        });
        if !supported {
            return Ok(());
        }
        match &cached.value {
            leveled::Value::ChanceNone { .. } => n.chance_field_indices.push(index),
            leveled::Value::Flags { .. } => n.flag_field_indices.push(index),
            leveled::Value::Global { .. } => n.global_field_indices.push(index),
            _ => (),
        }
        let mut link =
            |role, binding: &'a inventory::Binding, allowed: Option<bool>| -> Result<()> {
                let source = bound_source(store, receipts, binding, counts, limits)?;
                n.links.push(Link {
                    field_index: index,
                    role,
                    binding,
                    source,
                    schema_kind_allowed: allowed,
                    structural_binding_available: binding.status == inventory::Status::Defined
                        && allowed == Some(true),
                    nested_node: None,
                });
                Ok(())
            };
        match &cached.value {
            leveled::Value::Entry {
                item,
                schema_kind_allowed,
                ..
            } => link(Role::Item, item, *schema_kind_allowed)?,
            leveled::Value::Global { global } => link(
                Role::ChanceGlobal,
                global,
                global.target.as_ref().map(|t| t.kind == *b"GLOB"),
            )?,
            leveled::Value::Extra {
                owner, union_word, ..
            } => {
                link(
                    Role::Owner,
                    owner,
                    owner
                        .target
                        .as_ref()
                        .map(|t| matches!(&t.kind, b"NPC_" | b"FACT")),
                )?;
                if let inventory::ExtraWord::Global { binding } = union_word {
                    link(
                        Role::ExtraGlobal,
                        binding,
                        binding.target.as_ref().map(|t| t.kind == *b"GLOB"),
                    )?;
                }
            }
            _ => (),
        }
        Ok(())
    })?;
    if n.fields.len() != d.fields.len() {
        return Err(Error::Resolution(
            "actor death-item list field count differs".into(),
        ));
    }
    if supported {
        add(
            &mut counts.entries,
            d.entries.len(),
            limits.max_entries,
            "entry",
        )?;
        add(
            &mut counts.field_visits,
            n.links.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        let indices = n
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.role, Role::Item))
            .map(|(i, l)| (l.field_index, i))
            .collect::<BTreeMap<_, _>>();
        for e in &d.entries {
            add(
                &mut counts.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            let item_link = *indices
                .get(&e.lvlo_field)
                .ok_or_else(|| Error::Resolution("actor death-item entry link missing".into()))?;
            n.entries.push(Entry {
                lvlo_field: e.lvlo_field,
                coed_fields: &e.coed_fields,
                item_link,
                unique_extra_available: e.coed_fields.len() <= 1,
            });
        }
        n.metadata_singletons_unambiguous = n.chance_field_indices.len() == 1
            && n.flag_field_indices.len() == 1
            && n.global_field_indices.len() <= 1;
        if !n.metadata_singletons_unambiguous {
            n.issues.push("unique_list_metadata_unavailable");
        }
    }
    Ok(n)
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor death-item projection budget"));
        }
        self.bytes += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One physical INAM selection, using existing typed producers and strict Store.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    lists: &'a leveled::Catalogue,
    root: &FormKey,
    actor_field_index: usize,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    let winners = record_metadata::inspect(store)?.winning_definitions_sha256;
    for (sources, digest) in [
        (actors.sources(), actors.winning_content_sha256()),
        (
            associations.sources(),
            associations.winning_content_sha256(),
        ),
        (lists.sources.as_slice(), lists.winning_content_sha256()),
    ] {
        budget(sources.len() <= limits.max_sources, "source")?;
        if !same_sources(sources, &receipts) || digest != winners {
            return Err(Error::Resolution(
                "actor death-item input source cohorts differ".into(),
            ));
        }
    }
    let actor = actors
        .get(root)
        .filter(|a| !a.deleted)
        .ok_or_else(|| Error::Resolution("actor death-item root unavailable or deleted".into()))?;
    let retained = actor
        .record()
        .ok_or_else(|| Error::Resolution("actor death-item actor body missing".into()))?;
    let actor_source = source(store, &receipts, root)?;
    if retained.header != actor_source.header
        || actor.source.plugin != actor_source.source_name
        || actor.source.sha256 != actor_source.source_sha256
        || actor.source.record_file_offset != actor_source.header.offset
        || actor.source.record_flags != actor_source.header.flags
    {
        return Err(Error::Resolution(
            "actor death-item retained actor differs".into(),
        ));
    }
    let joined = associations
        .get(root)
        .ok_or_else(|| Error::Resolution("actor death-item associations missing".into()))?;
    let mut counts = Counts::default();
    for amount in [
        joined.associations.len(),
        joined.associations.len(),
        actor.fields.len(),
        actor.inventory_definition().fields.len(),
    ] {
        add(
            &mut counts.field_visits,
            amount,
            limits.max_field_visits,
            "field visit",
        )?;
    }
    add(
        &mut counts.fields,
        actor.fields.len(),
        limits.max_fields,
        "field",
    )?;
    budget(
        retained.payload.len() <= limits.max_record_bytes,
        "record byte",
    )?;
    add(
        &mut counts.decoded_bytes,
        retained.payload.len(),
        limits.max_decoded_bytes,
        "decoded byte",
    )?;
    add(&mut counts.headers, 1, limits.max_headers, "header")?;
    let association = joined
        .associations
        .iter()
        .find(|a| a.field_index == actor_field_index)
        .filter(|a| a.role == associations::Role::DeathItem)
        .ok_or_else(|| {
            Error::Resolution("chosen actor field is not a death-item association".into())
        })?;
    let actor_field = actor
        .fields
        .get(actor_field_index)
        .filter(|f| f.kind == *b"INAM")
        .ok_or_else(|| Error::Resolution("actor death-item physical field differs".into()))?;
    let configurations = actor
        .inventory_definition()
        .fields
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect::<Vec<_>>();
    add(
        &mut counts.fields,
        configurations.len(),
        limits.max_fields,
        "field",
    )?;
    let traits_template_flag = match configurations.as_slice() {
        [f] => match f.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags & 1 != 0),
            _ => None,
        },
        _ => None,
    };
    let singleton_repeated = joined
        .associations
        .iter()
        .filter(|a| a.role == associations::Role::DeathItem)
        .count()
        > 1;
    let death_item_source =
        bound_source(store, &receipts, &association.binding, &mut counts, limits)?;
    if death_item_source.is_some() {
        budget(limits.max_depth >= 1, "depth")?;
        counts.source_depth = 1;
    }
    let mut m = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields: configurations,
        traits_template_flag,
        association,
        actor_field,
        actor_raw_bytes: Vec::new(),
        singleton_repeated,
        declaration_binding_available: traits_template_flag == Some(false)
            && !singleton_repeated
            && association.binding.status == inventory::Status::Defined
            && association.schema_kind_allowed == Some(true),
        death_item_source,
        root_node: None,
        nodes: Vec::new(),
        counts,
        issues: Vec::new(),
        items_created: false,
        death_event_verified: false,
        roll_supported: false,
        scope: "One physical actor INAM with bounded existing LVLI declaration candidates, unsigned u16 level/count bits and explicit absent counts, raw chance/flags/COED and exact winning headers. Traits inheritance and repeated singleton ambiguity remain explicit. No RNG, probability, level threshold, respawn, death event, item creation or runtime inventory mutation",
    };
    if traits_template_flag.is_none() {
        m.issues.push("unique_configuration_unavailable");
    }
    if traits_template_flag == Some(true) {
        m.issues.push("traits_template_inheritance_unsupported");
    }
    if singleton_repeated {
        m.issues.push("repeated_death_item_association");
    }
    match association.binding.status {
        inventory::Status::Null => m.issues.push("null_death_item_association"),
        inventory::Status::Missing => m.issues.push("missing_death_item_target"),
        inventory::Status::Deleted => m.issues.push("deleted_death_item_target"),
        inventory::Status::Defined => (),
    }
    if association.schema_kind_allowed == Some(false) {
        m.issues.push("death_item_target_kind_not_allowed");
    }
    let mut seen = 0;
    plugin::visit_subrecords(retained, &actor.source.plugin, |f| {
        let i = seen;
        seen += 1;
        let cached = actor
            .fields
            .get(i)
            .ok_or_else(|| Error::Resolution("actor death-item actor field missing".into()))?;
        if cached.kind != f.kind
            || cached.decoded_offset as usize != f.payload_offset
            || cached.bytes != f.data.len()
            || cached.sha256 != format!("{:x}", Sha256::digest(f.data))
        {
            return Err(Error::Resolution(
                "actor death-item physical actor field differs".into(),
            ));
        }
        if i == actor_field_index {
            add(
                &mut m.counts.raw_bytes,
                f.data.len(),
                limits.max_raw_bytes,
                "raw byte",
            )?;
            if f.data.len() != 4
                || u32::from_le_bytes(f.data.try_into().expect("four physical bytes"))
                    != association.binding.raw_form
            {
                return Err(Error::Resolution(
                    "actor death-item physical binding differs".into(),
                ));
            }
            m.actor_raw_bytes = f.data.to_vec();
        }
        Ok(())
    })?;
    if seen != actor.fields.len() {
        return Err(Error::Resolution(
            "actor death-item actor field count differs".into(),
        ));
    }
    if association.binding.status == inventory::Status::Defined
        && association.schema_kind_allowed == Some(true)
    {
        let key = association
            .binding
            .key
            .as_ref()
            .ok_or_else(|| Error::Resolution("actor death-item list key missing".into()))?;
        m.root_node = Some(0);
        m.nodes
            .push(node(store, &receipts, lists, key, &mut m.counts, limits)?);
        let mut indices = BTreeMap::from([(key.clone(), 0)]);
        let mut minimal_depths = vec![1usize];
        let mut cursor = 0;
        while cursor < m.nodes.len() {
            let next_depth = minimal_depths[cursor]
                .checked_add(1)
                .ok_or_else(|| Error::Unsupported("actor death-item depth overflow".into()))?;
            for i in 0..m.nodes[cursor].links.len() {
                add(
                    &mut m.counts.field_visits,
                    1,
                    limits.max_field_visits,
                    "field visit",
                )?;
                let l = &m.nodes[cursor].links[i];
                if l.source.is_some() {
                    budget(next_depth <= limits.max_depth, "depth")?;
                    m.counts.source_depth = m.counts.source_depth.max(next_depth);
                }
                if !matches!(l.role, Role::Item)
                    || !l.structural_binding_available
                    || !l.source.as_ref().is_some_and(|s| s.header.kind == *b"LVLI")
                {
                    continue;
                }
                let key = l.binding.key.as_ref().expect("defined list key").clone();
                let target = if let Some(&n) = indices.get(&key) {
                    n
                } else {
                    let n = m.nodes.len();
                    m.nodes
                        .push(node(store, &receipts, lists, &key, &mut m.counts, limits)?);
                    indices.insert(key, n);
                    minimal_depths.push(next_depth);
                    n
                };
                m.nodes[cursor].links[i].nested_node = Some(target);
            }
            cursor += 1;
        }
        // Preserve every physical duplicate edge. Reserve shared iterative SCC
        // work before its bounded allocations, then refuse every selected cycle.
        let edges = m
            .nodes
            .iter()
            .map(|n| n.links.iter().filter(|l| l.nested_node.is_some()).count())
            .try_fold(0usize, |sum, n| sum.checked_add(n))
            .ok_or_else(|| Error::Unsupported("actor death-item edge overflow".into()))?;
        let graph_work = m
            .nodes
            .len()
            .checked_add(edges)
            .and_then(|n| n.checked_mul(6))
            .ok_or_else(|| Error::Unsupported("actor death-item graph work overflow".into()))?;
        add(
            &mut m.counts.field_visits,
            graph_work,
            limits.max_field_visits,
            "field visit",
        )?;
        let children = m
            .nodes
            .iter()
            .map(|n| {
                n.links
                    .iter()
                    .filter_map(|l| l.nested_node)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if !crate::graph::cyclic_components(&children).is_empty() {
            return Err(Error::Resolution(
                "actor death-item selected list cycle".into(),
            ));
        }
        // Longest path in the DAG catches a cached shared child reached deeper
        // than its first BFS encounter, without expanding exponentially many paths.
        let mut incoming = vec![0usize; m.nodes.len()];
        for targets in &children {
            for &to in targets {
                incoming[to] += 1;
            }
        }
        let mut queue = (0..m.nodes.len())
            .filter(|&i| incoming[i] == 0)
            .collect::<VecDeque<_>>();
        let mut depths = vec![0usize; m.nodes.len()];
        depths[0] = 1;
        while let Some(from) = queue.pop_front() {
            add(
                &mut m.counts.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            let next = depths[from]
                .checked_add(1)
                .ok_or_else(|| Error::Unsupported("actor death-item depth overflow".into()))?;
            for l in &m.nodes[from].links {
                add(
                    &mut m.counts.field_visits,
                    1,
                    limits.max_field_visits,
                    "field visit",
                )?;
                if l.source.is_some() {
                    budget(next <= limits.max_depth, "depth")?;
                    m.counts.source_depth = m.counts.source_depth.max(next);
                }
                if let Some(to) = l.nested_node {
                    depths[to] = depths[to].max(next);
                    incoming[to] -= 1;
                    if incoming[to] == 0 {
                        queue.push_back(to);
                    }
                }
            }
        }
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &m,
    )
    .map_err(|e| Error::Unsupported(format!("actor death-item input projection: {e}")))?;
    Ok(m)
}
