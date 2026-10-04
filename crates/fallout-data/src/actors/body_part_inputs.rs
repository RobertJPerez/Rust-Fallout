//! Physical creature PNAM/BPTD declarations, without contact or limb selection.
//! Optional/any-member editor groups cannot authorize a nearest-name grouping.
use super::{Catalogue, Definition, effect_inputs::SourceRequest};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_visits: usize,
    pub max_fields: usize,
    pub max_parts: usize,
    pub max_names: usize,
    pub max_name_bytes: usize,
    pub max_raw_bytes: usize,
    pub max_bindings: usize,
    pub max_headers: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_record_bytes: 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_field_visits: 200_000,
            max_fields: 65_536,
            max_parts: 4096,
            max_names: 4096,
            max_name_bytes: 1024 * 1024,
            max_raw_bytes: 2 * 1024 * 1024,
            max_bindings: 4096,
            max_headers: 4096,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub decoded_bytes: usize,
    pub field_visits: usize,
    pub fields: usize,
    pub parts: usize,
    pub names: usize,
    pub name_bytes: usize,
    pub raw_bytes: usize,
    pub bindings: usize,
    pub headers: usize,
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub field_index: usize,
    pub field_byte_offset: u32,
    pub role: &'static str,
    pub binding: inventory::Binding,
    pub source: Option<SourceRequest>,
    pub schema_kind_allowed: Option<bool>,
    pub structural_binding_available: bool,
}
#[derive(Debug, Serialize)]
pub struct NodeData {
    pub damage_mult_bits: u32,
    pub flags: u8,
    pub flags_known: bool,
    pub part_type: i8,
    pub part_type_known: bool,
    pub health_percent: u8,
    pub actor_value: i8,
    pub actor_value_known: bool,
    pub to_hit_chance: u8,
    pub explosion_chance: u8,
    pub explodable_debris_count: u16,
    pub tracking_max_angle_bits: u32,
    pub explodable_debris_scale_bits: u32,
    pub severable_debris_count: i32,
    pub severable_debris_scale_bits: u32,
    pub gore_position_bits: [u32; 3],
    pub gore_rotation_bits: [u32; 3],
    pub severable_decal_count: u8,
    pub explodable_decal_count: u8,
    pub unused: [u8; 2],
    pub limb_replacement_scale_bits: u32,
}
#[derive(Debug, Serialize)]
pub struct Part {
    pub field_index: usize,
    pub layout_supported: bool,
    pub data: Option<NodeData>,
    pub duplicate_part_type: bool,
}
#[derive(Debug, Serialize)]
pub struct Declaration {
    pub source: SourceRequest,
    pub decoded_record_sha256: String,
    pub record_version_supported: bool,
    pub fields: Vec<Field>,
    /// Exact physical string/model field occurrences, never assigned to a limb.
    pub name_field_indices: Vec<usize>,
    pub parts: Vec<Part>,
    pub links: Vec<Link>,
    pub source_grouping_verified: bool,
    pub issues: Vec<&'static str>,
}
/// No report deserialization or mutable accessor can create source authority.
#[derive(Serialize)]
pub struct Manifest<'a> {
    sources: &'a [SourceReceipt],
    winning_content_sha256: &'a str,
    actor: &'a Definition<'a>,
    configuration_fields: Vec<&'a inventory::Field>,
    model_animation_template_flag: Option<bool>,
    actor_record_version_supported: bool,
    pnam_fields: Vec<Field>,
    pnam_links: Vec<Link>,
    pnam_repeated: bool,
    declaration_binding_available: bool,
    declaration: Option<Declaration>,
    counts: Counts,
    issues: Vec<&'static str>,
    contact_mapped: bool,
    damage_evaluated: bool,
    dismemberment_supported: bool,
    scope: &'static str,
}
impl Manifest<'_> {
    pub fn declaration(&self) -> Option<&Declaration> {
        self.declaration.as_ref()
    }
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor body-part {label} budget exceeded"
        )))
    }
}
fn add(v: &mut usize, n: usize, max: usize, label: &str) -> Result<()> {
    *v = v
        .checked_add(n)
        .ok_or_else(|| Error::Unsupported(format!("actor body-part {label} overflow")))?;
    budget(*v <= max, label)
}
fn sources_match(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn source(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    key: &FormKey,
    c: &mut Counts,
    limits: Limits,
) -> Result<SourceRequest> {
    let at = store
        .winner(key)
        .ok_or_else(|| Error::Resolution("actor body-part source winner absent".into()))?;
    add(&mut c.headers, 1, limits.max_headers, "header")?;
    let s = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor body-part source receipt absent".into()))?;
    Ok(SourceRequest {
        key: key.clone(),
        source_name: s.source_name.clone(),
        source_bytes: s.source_bytes,
        source_sha256: s.source_sha256.clone(),
        header: store.definition(at).header.clone(),
    })
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("checked body-part word extent"),
    )
}
struct LinkSpec {
    field: usize,
    offset: u32,
    raw: u32,
    role: &'static str,
    allowed: [u8; 4],
}
fn link(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    at: crate::store::Location,
    spec: LinkSpec,
    c: &mut Counts,
    limits: Limits,
) -> Result<Link> {
    add(&mut c.bindings, 1, limits.max_bindings, "binding")?;
    let b = inventory::binding(store, at, spec.raw, &mut inventory::Counts::default())?;
    let s = b
        .key
        .as_ref()
        .filter(|key| store.winner(key).is_some())
        .map(|key| source(store, receipts, key, c, limits))
        .transpose()?;
    if let Some(s) = &s {
        let t = b
            .target
            .as_ref()
            .ok_or_else(|| Error::Resolution("actor body-part bound header absent".into()))?;
        if t.kind != s.header.kind
            || t.record_file_offset != s.header.offset
            || t.record_flags != s.header.flags
            || t.source_plugin != s.source_name
        {
            return Err(Error::Resolution(
                "actor body-part bound winner differs".into(),
            ));
        }
    }
    let permit = b.target.as_ref().map(|t| t.kind == spec.allowed);
    let available = b.status == inventory::Status::Defined && permit == Some(true);
    Ok(Link {
        field_index: spec.field,
        field_byte_offset: spec.offset,
        role: spec.role,
        binding: b,
        source: s,
        schema_kind_allowed: permit,
        structural_binding_available: available,
    })
}
fn node(raw: &[u8]) -> NodeData {
    let part_type = raw[5] as i8;
    let actor_value = raw[7] as i8;
    NodeData {
        damage_mult_bits: word(raw, 0),
        flags: raw[4],
        flags_known: raw[4] & 0x80 == 0,
        part_type,
        part_type_known: (-1..=14).contains(&part_type),
        health_percent: raw[6],
        actor_value,
        actor_value_known: (-1..=76).contains(&actor_value),
        to_hit_chance: raw[8],
        explosion_chance: raw[9],
        explodable_debris_count: u16::from_le_bytes(
            raw[10..12].try_into().expect("two count bytes"),
        ),
        tracking_max_angle_bits: word(raw, 20),
        explodable_debris_scale_bits: word(raw, 24),
        severable_debris_count: word(raw, 28) as i32,
        severable_debris_scale_bits: word(raw, 40),
        gore_position_bits: [word(raw, 44), word(raw, 48), word(raw, 52)],
        gore_rotation_bits: [word(raw, 56), word(raw, 60), word(raw, 64)],
        severable_decal_count: raw[76],
        explodable_decal_count: raw[77],
        unused: [raw[78], raw[79]],
        limb_replacement_scale_bits: word(raw, 80),
    }
}
fn field(
    kind: [u8; 4],
    offset: usize,
    raw: &[u8],
    c: &mut Counts,
    limits: Limits,
) -> Result<Field> {
    add(
        &mut c.raw_bytes,
        raw.len(),
        limits.max_raw_bytes,
        "raw byte",
    )?;
    Ok(Field {
        kind,
        decoded_offset: u32::try_from(offset)
            .map_err(|_| Error::Unsupported("actor body-part offset exceeds u32".into()))?,
        raw_bytes: raw.to_vec(),
        sha256: format!("{:x}", Sha256::digest(raw)),
    })
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor body-part projection budget"));
        }
        self.bytes += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Construct one fresh source request; no PNAM or part occurrence wins by order.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    root: &FormKey,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(
        store.indices().len() <= limits.max_sources && actors.sources().len() <= limits.max_sources,
        "source",
    )?;
    let receipts = store.source_receipts()?;
    if !sources_match(&receipts, actors.sources())
        || record_metadata::inspect(store)?.winning_definitions_sha256
            != actors.winning_content_sha256()
    {
        return Err(Error::Resolution(
            "actor body-part source cohort differs".into(),
        ));
    }
    let actor = actors
        .get(root)
        .filter(|a| !a.deleted && a.kind == *b"CREA")
        .ok_or_else(|| Error::Resolution("actor body-part root is not a live creature".into()))?;
    let retained = actor
        .record()
        .ok_or_else(|| Error::Resolution("actor body-part retained creature absent".into()))?;
    let mut c = Counts::default();
    let a = source(store, &receipts, root, &mut c, limits)?;
    budget(
        a.header.stored_size as usize <= limits.max_record_bytes
            && retained.payload.len() <= limits.max_record_bytes,
        "record byte",
    )?;
    add(
        &mut c.decoded_bytes,
        retained.payload.len(),
        limits.max_decoded_bytes,
        "decoded byte",
    )?;
    let actor_hash = format!("{:x}", Sha256::digest(&retained.payload));
    if a.header != retained.header
        || a.source_name != actor.source.plugin
        || a.source_sha256 != actor.source.sha256
        || a.header.offset != actor.source.record_file_offset
        || a.header.flags != actor.source.record_flags
        || actor.source.decoded_record_sha256.as_deref() != Some(actor_hash.as_str())
    {
        return Err(Error::Resolution(
            "actor body-part retained creature differs".into(),
        ));
    }
    let inv = &actor.inventory_definition().fields;
    add(
        &mut c.field_visits,
        inv.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    let configuration_fields = inv
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect::<Vec<_>>();
    let mask = match configuration_fields.as_slice() {
        [f] => match f.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags),
            _ => None,
        },
        _ => None,
    };
    add(
        &mut c.field_visits,
        actor.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut c.fields,
        actor.fields.len(),
        limits.max_fields,
        "field",
    )?;
    let occurrences = actor.fields.iter().filter(|f| f.kind == *b"PNAM").count();
    let supported = actor.record_version == Some(15);
    let mut m = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields,
        model_animation_template_flag: mask.map(|f| f & 64 != 0),
        actor_record_version_supported: supported,
        pnam_fields: Vec::new(),
        pnam_links: Vec::new(),
        pnam_repeated: occurrences > 1,
        declaration_binding_available: false,
        declaration: None,
        counts: c,
        issues: Vec::new(),
        contact_mapped: false,
        damage_evaluated: false,
        dismemberment_supported: false,
        scope: "Exact creature PNAM and physical BPTD names/BPND part declarations, signed part/actor-value/count words, raw hit/float/flag/unused bytes and winning debris/explosion/impact/ragdoll headers. ModelAnimation inheritance and repeated/unknown source declarations remain explicit. No nearest-name grouping, editor sorting, contact/limb matching, model choice, damage, limb health, gore/dismemberment evaluation or runtime mutation",
    };
    if !supported {
        m.issues.push("unsupported_creature_record_version");
    }
    if mask.is_none() {
        m.issues.push("unique_configuration_unavailable");
    }
    if m.model_animation_template_flag == Some(true) {
        m.issues
            .push("model_animation_template_inheritance_unsupported");
    }
    if occurrences == 0 {
        m.issues.push("body_part_link_absent");
    }
    if occurrences > 1 {
        m.issues.push("repeated_body_part_link");
    }
    let at = store
        .winner(root)
        .ok_or_else(|| Error::Resolution("actor body-part creature winner absent".into()))?;
    let mut seen = 0;
    plugin::visit_subrecords(retained, &actor.source.plugin, |f| {
        add(
            &mut m.counts.field_visits,
            1,
            limits.max_field_visits,
            "field visit",
        )?;
        let index = seen;
        seen += 1;
        let cached = actor
            .fields
            .get(index)
            .ok_or_else(|| Error::Resolution("actor body-part cached actor field absent".into()))?;
        if cached.kind != f.kind
            || cached.decoded_offset as usize != f.payload_offset
            || cached.bytes != f.data.len()
            || cached.sha256 != format!("{:x}", Sha256::digest(f.data))
        {
            return Err(Error::Resolution(
                "actor body-part physical actor field differs".into(),
            ));
        }
        if f.kind != *b"PNAM" {
            return Ok(());
        }
        m.pnam_fields.push(field(
            f.kind,
            f.payload_offset,
            f.data,
            &mut m.counts,
            limits,
        )?);
        if !supported || f.data.len() != 4 {
            m.issues.push("unsupported_body_part_link_layout");
            return Ok(());
        }
        m.pnam_links.push(link(
            store,
            &receipts,
            at,
            LinkSpec {
                field: index,
                offset: 0,
                raw: word(f.data, 0),
                role: "body_part",
                allowed: *b"BPTD",
            },
            &mut m.counts,
            limits,
        )?);
        Ok(())
    })?;
    if seen != actor.fields.len() {
        return Err(Error::Resolution(
            "actor body-part physical actor field count differs".into(),
        ));
    }
    let selected = match m.pnam_links.as_slice() {
        [l] if occurrences == 1 && l.structural_binding_available => l.binding.key.clone(),
        _ => None,
    };
    m.declaration_binding_available =
        selected.is_some() && m.model_animation_template_flag == Some(false);
    if let Some(key) = selected {
        let at = store.winner(&key).ok_or_else(|| {
            Error::Resolution("actor body-part selected BPTD winner absent".into())
        })?;
        let s = source(store, &receipts, &key, &mut m.counts, limits)?;
        let r = store.read_bounded(
            at,
            limits.max_record_bytes.min(
                limits
                    .max_decoded_bytes
                    .saturating_sub(m.counts.decoded_bytes),
            ),
        )?;
        if r.header != s.header {
            return Err(Error::Resolution(
                "actor body-part selected BPTD header differs".into(),
            ));
        }
        add(
            &mut m.counts.decoded_bytes,
            r.payload.len(),
            limits.max_decoded_bytes,
            "decoded byte",
        )?;
        let version = r.header.version == 15;
        let mut d = Declaration {
            source: s,
            decoded_record_sha256: format!("{:x}", Sha256::digest(&r.payload)),
            record_version_supported: version,
            fields: Vec::new(),
            name_field_indices: Vec::new(),
            parts: Vec::new(),
            links: Vec::new(),
            source_grouping_verified: false,
            issues: Vec::new(),
        };
        if !version {
            d.issues.push("unsupported_body_part_record_version");
        }
        plugin::visit_subrecords(&r, &d.source.source_name, |f| {
            add(
                &mut m.counts.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            add(&mut m.counts.fields, 1, limits.max_fields, "field")?;
            let index = d.fields.len();
            d.fields.push(field(
                f.kind,
                f.payload_offset,
                f.data,
                &mut m.counts,
                limits,
            )?);
            if matches!(
                &f.kind,
                b"BPTN" | b"BPNN" | b"BPNT" | b"BPNI" | b"NAM1" | b"NAM4" | b"MODL"
            ) {
                add(&mut m.counts.names, 1, limits.max_names, "name")?;
                add(
                    &mut m.counts.name_bytes,
                    f.data.len(),
                    limits.max_name_bytes,
                    "name byte",
                )?;
                d.name_field_indices.push(index);
            }
            if f.kind == *b"BPND" {
                add(&mut m.counts.parts, 1, limits.max_parts, "part")?;
                let known = version && f.data.len() == 84;
                d.parts.push(Part {
                    field_index: index,
                    layout_supported: known,
                    data: known.then(|| node(f.data)),
                    duplicate_part_type: false,
                });
                if known {
                    for (offset, role, kind) in [
                        (12, "explodable_debris", *b"DEBR"),
                        (16, "explodable_explosion", *b"EXPL"),
                        (32, "severable_debris", *b"DEBR"),
                        (36, "severable_explosion", *b"EXPL"),
                        (68, "severable_impact_dataset", *b"IPDS"),
                        (72, "explodable_impact_dataset", *b"IPDS"),
                    ] {
                        add(
                            &mut m.counts.field_visits,
                            1,
                            limits.max_field_visits,
                            "field visit",
                        )?;
                        d.links.push(link(
                            store,
                            &receipts,
                            at,
                            LinkSpec {
                                field: index,
                                offset: offset as u32,
                                raw: word(f.data, offset),
                                role,
                                allowed: kind,
                            },
                            &mut m.counts,
                            limits,
                        )?);
                    }
                }
            } else if f.kind == *b"RAGA" && version && f.data.len() == 4 {
                d.links.push(link(
                    store,
                    &receipts,
                    at,
                    LinkSpec {
                        field: index,
                        offset: 0,
                        raw: word(f.data, 0),
                        role: "ragdoll",
                        allowed: *b"RGDL",
                    },
                    &mut m.counts,
                    limits,
                )?);
            }
            Ok(())
        })?;
        add(
            &mut m.counts.field_visits,
            d.parts.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        let mut types = BTreeMap::<i8, usize>::new();
        for p in &d.parts {
            if let Some(n) = &p.data {
                *types.entry(n.part_type).or_default() += 1;
            }
        }
        add(
            &mut m.counts.field_visits,
            d.parts.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        for p in &mut d.parts {
            p.duplicate_part_type = p
                .data
                .as_ref()
                .is_some_and(|n| types.get(&n.part_type).is_some_and(|c| *c > 1));
        }
        m.declaration = Some(d);
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &m,
    )
    .map_err(|e| Error::Unsupported(format!("actor body-part projection: {e}")))?;
    Ok(m)
}
