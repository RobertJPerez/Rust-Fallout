//! Explicit weapon/ammo declarations. No firing, ammo choice or projectile priority.
use super::effect_inputs::SourceRequest;
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
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_headers: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_visits: usize,
    pub max_fields: usize,
    pub max_bindings: usize,
    pub max_words: usize,
    pub max_raw_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_nodes: 64,
            max_depth: 2,
            max_headers: 4096,
            max_record_bytes: 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_field_visits: 200_000,
            max_fields: 65_536,
            max_bindings: 4096,
            max_words: 65_536,
            max_raw_bytes: 1024 * 1024,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub nodes: usize,
    pub source_depth: usize,
    pub headers: usize,
    pub decoded_bytes: usize,
    pub field_visits: usize,
    pub fields: usize,
    pub bindings: usize,
    pub words: usize,
    pub raw_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct Word {
    pub field_byte_offset: u32,
    pub raw: u32,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    WeaponData {
        value: i32,
        health: i32,
        weight_bits: u32,
        base_damage: i16,
        clip_size: u8,
    },
    WeaponAttack {
        raw_words: Vec<Word>,
        flags1: u8,
        grip_animation: u8,
        ammo_use: u8,
        reload_animation: u8,
        vats_to_hit_chance: u8,
        attack_animation: u8,
        projectile_count: u8,
        embedded_actor_value: u8,
        skill: i32,
        resist_type: Option<i32>,
    },
    WeaponCritical {
        critical_damage: u16,
        unused_prefix: [u8; 2],
        multiplier_bits: u32,
        flags: u8,
        unused_suffix: [u8; 3],
    },
    WeaponVats {
        skill_bits: u32,
        damage_multiplier_bits: u32,
        action_points_bits: u32,
        silent: Option<u8>,
        mod_required: Option<u8>,
        unused: Option<[u8; 2]>,
    },
    AmmoData {
        speed_bits: u32,
        flags: u8,
        unused: [u8; 3],
        value: i32,
        clip_rounds: u8,
    },
    AmmoAttack {
        projectiles_per_shot: u32,
        weight_bits: u32,
        consumed_percentage_bits: Option<u32>,
    },
    ProjectileData {
        flags: u16,
        projectile_type: u16,
        known_projectile_type: bool,
        raw_words: Vec<Word>,
    },
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub sha256: String,
    pub value: Value,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    WeaponAmmo,
    WeaponProjectile,
    CriticalEffect,
    VatsEffect,
    AmmoProjectile,
    ConsumedAmmo,
    AmmoEffect,
    Light,
    MuzzleLight,
    Explosion,
    Sound,
    CountdownSound,
    DisableSound,
    DefaultWeapon,
}
impl Role {
    fn allowed(self, kind: &[u8; 4]) -> bool {
        match self {
            Self::WeaponAmmo => matches!(kind, b"AMMO" | b"FLST"),
            Self::WeaponProjectile | Self::AmmoProjectile => kind == b"PROJ",
            Self::CriticalEffect | Self::VatsEffect => kind == b"SPEL",
            Self::ConsumedAmmo => matches!(kind, b"AMMO" | b"MISC"),
            Self::AmmoEffect => kind == b"AMEF",
            Self::Light | Self::MuzzleLight => kind == b"LIGH",
            Self::Explosion => kind == b"EXPL",
            Self::Sound | Self::CountdownSound | Self::DisableSound => kind == b"SOUN",
            Self::DefaultWeapon => kind == b"WEAP",
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub field_index: usize,
    pub field_byte_offset: u32,
    pub role: Role,
    pub binding: inventory::Binding,
    pub schema_kind_allowed: Option<bool>,
    pub target: Option<SourceRequest>,
    pub target_node: Option<usize>,
    pub repeated: bool,
    pub binding_admitted: bool,
    pub issues: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Issue {
    pub field_index: Option<usize>,
    pub field_kind: Option<[u8; 4]>,
    pub code: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub source: SourceRequest,
    pub decoded_record_sha256: String,
    pub record_version_supported: bool,
    pub fields: Vec<Field>,
    pub links: Vec<Link>,
    pub issues: Vec<Issue>,
}
#[derive(Debug, Serialize)]
pub struct AmmoRelation {
    pub weapon_link_index: usize,
    pub matches_direct_ammo: Option<bool>,
    pub list_membership_verified: bool,
}
#[derive(Debug, Serialize)]
pub struct AmmoChoice {
    pub key: FormKey,
    pub status: inventory::Status,
    pub schema_kind_allowed: Option<bool>,
    pub source: Option<SourceRequest>,
    pub node_index: Option<usize>,
    pub declared_relations: Vec<AmmoRelation>,
}
/// Owned immutable source observation; reports cannot deserialize into authority.
#[derive(Debug, Serialize)]
pub struct Manifest {
    sources: Vec<SourceReceipt>,
    winning_content_sha256: String,
    weapon: FormKey,
    explicit_ammo: Option<AmmoChoice>,
    source_nodes: Vec<Node>,
    counts: Counts,
    ammo_choice_verified: bool,
    projectile_priority_selected: bool,
    firing_supported: bool,
    scope: &'static str,
}
impl Manifest {
    pub fn nodes(&self) -> &[Node] {
        &self.source_nodes
    }
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor attack input {label} budget exceeded"
        )))
    }
}
fn add(value: &mut usize, amount: usize, maximum: usize, label: &str) -> Result<()> {
    *value = value
        .checked_add(amount)
        .ok_or_else(|| Error::Unsupported(format!("actor attack input {label} overflow")))?;
    budget(*value <= maximum, label)
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("admitted attack word"))
}
fn short(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().expect("admitted attack short"))
}
fn words(data: &[u8], start: usize, counts: &mut Counts, limits: Limits) -> Result<Vec<Word>> {
    add(
        &mut counts.words,
        (data.len() - start) / 4,
        limits.max_words,
        "word",
    )?;
    Ok(data[start..]
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, b)| Word {
            field_byte_offset: (start + i * 4) as u32,
            raw: u32::from_le_bytes(*b),
        })
        .collect())
}
fn source(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    key: &FormKey,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Option<SourceRequest>> {
    let Some(at) = store.winner(key) else {
        return Ok(None);
    };
    add(&mut counts.headers, 1, limits.max_headers, "header")?;
    let receipt = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("attack source receipt unavailable".into()))?;
    Ok(Some(SourceRequest {
        key: key.clone(),
        source_name: receipt.source_name.clone(),
        source_bytes: receipt.source_bytes,
        source_sha256: receipt.source_sha256.clone(),
        header: store.definition(at).header.clone(),
    }))
}
fn decode(
    store: &mut RecordStore,
    receipts: &[SourceReceipt],
    key: &FormKey,
    depth: usize,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Node> {
    budget(depth <= limits.max_depth, "depth")?;
    counts.source_depth = counts.source_depth.max(depth);
    add(&mut counts.nodes, 1, limits.max_nodes, "node")?;
    let provenance = source(store, receipts, key, counts, limits)?
        .ok_or_else(|| Error::Resolution("attack node missing".into()))?;
    let location = store.winner(key).expect("fresh attack source winner");
    let maximum = limits.max_record_bytes.min(
        limits
            .max_decoded_bytes
            .saturating_sub(counts.decoded_bytes),
    );
    let record = store.read_bounded(location, maximum)?;
    if record.integrity_issue.is_some() {
        return Err(Error::Resolution("attack source checksum differs".into()));
    }
    add(
        &mut counts.decoded_bytes,
        record.payload.len(),
        limits.max_decoded_bytes,
        "decoded byte",
    )?;
    let supported = record.header.version == 15;
    let mut node = Node {
        source: provenance,
        decoded_record_sha256: format!("{:x}", Sha256::digest(&record.payload)),
        record_version_supported: supported,
        fields: Vec::new(),
        links: Vec::new(),
        issues: Vec::new(),
    };
    if !supported {
        node.issues.push(Issue {
            field_index: None,
            field_kind: None,
            code: "unsupported_attack_record_version",
        });
    }
    let mut binding_counts = inventory::Counts::default();
    plugin::visit_subrecords(&record, store.source_name(location), |field| {
        add(
            &mut counts.field_visits,
            1,
            limits.max_field_visits,
            "field visit",
        )?;
        add(&mut counts.fields, 1, limits.max_fields, "field")?;
        add(
            &mut counts.raw_bytes,
            field.data.len(),
            limits.max_raw_bytes,
            "raw byte",
        )?;
        let index = node.fields.len();
        let raw = field.data;
        let mut declarations = Vec::new();
        let mut recognized = false;
        let mut layout = true;
        let value = if !supported {
            Value::Opaque
        } else {
            match (&record.header.kind, &field.kind) {
                (b"WEAP", b"NAM0") => {
                    recognized = true;
                    layout = raw.len() == 4;
                    if layout {
                        declarations.push((0, Role::WeaponAmmo));
                    }
                    Value::Opaque
                }
                (b"WEAP", b"DATA") => {
                    recognized = true;
                    layout = raw.len() == 15;
                    if layout {
                        Value::WeaponData {
                            value: word(raw, 0) as i32,
                            health: word(raw, 4) as i32,
                            weight_bits: word(raw, 8),
                            base_damage: short(raw, 12) as i16,
                            clip_size: raw[14],
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"WEAP", b"DNAM") => {
                    recognized = true;
                    layout = matches!(raw.len(), 120 | 204);
                    if layout {
                        declarations.push((36, Role::WeaponProjectile));
                        Value::WeaponAttack {
                            raw_words: words(raw, 0, counts, limits)?,
                            flags1: raw[12],
                            grip_animation: raw[13],
                            ammo_use: raw[14],
                            reload_animation: raw[15],
                            vats_to_hit_chance: raw[40],
                            attack_animation: raw[41],
                            projectile_count: raw[42],
                            embedded_actor_value: raw[43],
                            skill: word(raw, 104) as i32,
                            resist_type: (raw.len() == 204).then(|| word(raw, 120) as i32),
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"WEAP", b"CRDT") => {
                    recognized = true;
                    layout = raw.len() == 16;
                    if layout {
                        declarations.push((12, Role::CriticalEffect));
                        Value::WeaponCritical {
                            critical_damage: short(raw, 0),
                            unused_prefix: raw[2..4].try_into().expect("critical unused"),
                            multiplier_bits: word(raw, 4),
                            flags: raw[8],
                            unused_suffix: raw[9..12].try_into().expect("critical unused"),
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"WEAP", b"VATS") => {
                    recognized = true;
                    layout = matches!(raw.len(), 16 | 20);
                    if layout {
                        declarations.push((0, Role::VatsEffect));
                        Value::WeaponVats {
                            skill_bits: word(raw, 4),
                            damage_multiplier_bits: word(raw, 8),
                            action_points_bits: word(raw, 12),
                            silent: (raw.len() == 20).then(|| raw[16]),
                            mod_required: (raw.len() == 20).then(|| raw[17]),
                            unused: (raw.len() == 20)
                                .then(|| raw[18..20].try_into().expect("VATS unused")),
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"AMMO", b"DATA") => {
                    recognized = true;
                    layout = raw.len() == 13;
                    if layout {
                        Value::AmmoData {
                            speed_bits: word(raw, 0),
                            flags: raw[4],
                            unused: raw[5..8].try_into().expect("ammo unused"),
                            value: word(raw, 8) as i32,
                            clip_rounds: raw[12],
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"AMMO", b"DAT2") => {
                    recognized = true;
                    layout = matches!(raw.len(), 12 | 16 | 20);
                    if layout {
                        declarations.push((4, Role::AmmoProjectile));
                        if raw.len() >= 16 {
                            declarations.push((12, Role::ConsumedAmmo));
                        }
                        Value::AmmoAttack {
                            projectiles_per_shot: word(raw, 0),
                            weight_bits: word(raw, 8),
                            consumed_percentage_bits: (raw.len() == 20).then(|| word(raw, 16)),
                        }
                    } else {
                        Value::Opaque
                    }
                }
                (b"AMMO", b"RCIL") => {
                    recognized = true;
                    layout = raw.len() == 4;
                    if layout {
                        declarations.push((0, Role::AmmoEffect));
                    }
                    Value::Opaque
                }
                (b"PROJ", b"DATA") => {
                    recognized = true;
                    layout = matches!(raw.len(), 68 | 80 | 84);
                    if layout {
                        declarations.extend([
                            (16, Role::Light),
                            (20, Role::MuzzleLight),
                            (36, Role::Explosion),
                            (40, Role::Sound),
                            (56, Role::CountdownSound),
                            (60, Role::DisableSound),
                            (64, Role::DefaultWeapon),
                        ]);
                        Value::ProjectileData {
                            flags: short(raw, 0),
                            projectile_type: short(raw, 2),
                            known_projectile_type: matches!(short(raw, 2), 1 | 2 | 4 | 8 | 16),
                            raw_words: words(raw, 4, counts, limits)?,
                        }
                    } else {
                        Value::Opaque
                    }
                }
                _ => Value::Opaque,
            }
        };
        if recognized && !layout {
            node.issues.push(Issue {
                field_index: Some(index),
                field_kind: Some(field.kind),
                code: "unsupported_attack_field_layout",
            });
        }
        for (offset, role) in declarations {
            add(&mut counts.bindings, 1, limits.max_bindings, "binding")?;
            let binding =
                inventory::binding(store, location, word(raw, offset), &mut binding_counts)?;
            let allowed = binding.target.as_ref().map(|t| role.allowed(&t.kind));
            let mut issues = Vec::new();
            match binding.status {
                inventory::Status::Null => issues.push("null_attack_source_link"),
                inventory::Status::Missing => issues.push("missing_attack_source_link"),
                inventory::Status::Deleted => issues.push("deleted_attack_source_link"),
                inventory::Status::Defined => (),
            }
            if allowed == Some(false) {
                issues.push("attack_source_kind_not_allowed");
            }
            if role == Role::WeaponAmmo
                && binding.target.as_ref().is_some_and(|t| t.kind == *b"FLST")
            {
                issues.push("ammo_list_membership_unavailable");
            }
            let target = if let Some(key) = &binding.key {
                source(store, receipts, key, counts, limits)?
            } else {
                None
            };
            node.links.push(Link {
                field_index: index,
                field_byte_offset: offset as u32,
                role,
                binding_admitted: binding.status == inventory::Status::Defined
                    && allowed == Some(true),
                binding,
                schema_kind_allowed: allowed,
                target,
                target_node: None,
                repeated: false,
                issues,
            });
        }
        node.fields.push(Field {
            kind: field.kind,
            decoded_offset: u32::try_from(field.payload_offset)
                .map_err(|_| Error::Unsupported("attack field offset exceeds u32".into()))?,
            raw_bytes: raw.to_vec(),
            sha256: format!("{:x}", Sha256::digest(raw)),
            value,
        });
        Ok(())
    })?;
    add(
        &mut counts.field_visits,
        node.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    let mut occurrences = BTreeMap::<[u8; 4], usize>::new();
    for field in &node.fields {
        if matches!(
            &field.kind,
            b"NAM0" | b"DATA" | b"DNAM" | b"CRDT" | b"VATS" | b"DAT2"
        ) {
            *occurrences.entry(field.kind).or_default() += 1;
        }
    }
    add(
        &mut counts.field_visits,
        node.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    for (index, field) in node.fields.iter().enumerate() {
        if occurrences.get(&field.kind).is_some_and(|n| *n > 1) {
            node.issues.push(Issue {
                field_index: Some(index),
                field_kind: Some(field.kind),
                code: "repeated_singleton_source_field",
            });
        }
    }
    add(
        &mut counts.field_visits,
        node.links.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    for link in &mut node.links {
        link.repeated = link.role != Role::AmmoEffect
            && occurrences
                .get(&node.fields[link.field_index].kind)
                .is_some_and(|n| *n > 1);
        if link.repeated {
            link.binding_admitted = false;
            link.issues.push("repeated_singleton_source_field");
        }
    }
    let required = [*b"DATA", *b"DNAM", *b"CRDT"];
    let required_count = if record.header.kind == *b"WEAP" { 3 } else { 1 };
    for kind in &required[..required_count] {
        if !occurrences.contains_key(kind) {
            node.issues.push(Issue {
                field_index: None,
                field_kind: Some(*kind),
                code: "required_attack_field_unavailable",
            });
        }
    }
    Ok(node)
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor attack input projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn request(
    store: &mut RecordStore,
    weapon: &FormKey,
    explicit_ammo: Option<&FormKey>,
    limits: Limits,
) -> Result<Manifest> {
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let sources = store.source_receipts()?;
    let digest = record_metadata::inspect(store)?.winning_definitions_sha256;
    let weapon_at = store
        .winner(weapon)
        .ok_or_else(|| Error::Resolution("explicit weapon source missing".into()))?;
    let header = &store.definition(weapon_at).header;
    if header.kind != *b"WEAP" || header.flags & plugin::DELETED != 0 {
        return Err(Error::Resolution(
            "explicit weapon source is deleted or wrong kind".into(),
        ));
    }
    let mut counts = Counts::default();
    let mut nodes = vec![decode(store, &sources, weapon, 0, &mut counts, limits)?];
    let mut index = BTreeMap::from([(weapon.clone(), 0)]);
    let mut choice = None;
    if let Some(ammo) = explicit_ammo {
        let provenance = source(store, &sources, ammo, &mut counts, limits)?;
        let status = match &provenance {
            None => inventory::Status::Missing,
            Some(s) if s.header.flags & plugin::DELETED != 0 => inventory::Status::Deleted,
            _ => inventory::Status::Defined,
        };
        let allowed = provenance.as_ref().map(|s| s.header.kind == *b"AMMO");
        let mut node_index = None;
        if status == inventory::Status::Defined && allowed == Some(true) {
            let node = decode(store, &sources, ammo, 1, &mut counts, limits)?;
            node_index = Some(nodes.len());
            index.insert(ammo.clone(), nodes.len());
            nodes.push(node);
        }
        add(
            &mut counts.field_visits,
            nodes[0].links.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        let relations = nodes[0]
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| l.role == Role::WeaponAmmo)
            .map(|(i, l)| AmmoRelation {
                weapon_link_index: i,
                matches_direct_ammo: if l.binding_admitted
                    && l.binding
                        .target
                        .as_ref()
                        .is_some_and(|t| t.kind == *b"AMMO")
                    && status == inventory::Status::Defined
                    && allowed == Some(true)
                {
                    Some(l.binding.key.as_ref() == Some(ammo))
                } else {
                    None
                },
                list_membership_verified: false,
            })
            .collect();
        choice = Some(AmmoChoice {
            key: ammo.clone(),
            status,
            schema_kind_allowed: allowed,
            source: provenance,
            node_index,
            declared_relations: relations,
        });
    }
    // Only WEAP and the explicitly supplied AMMO seed projectile body requests.
    // PROJ default-weapon/effect/audio/light links stay header-only, even cycles.
    let parents = nodes.len();
    for parent in 0..parents {
        add(
            &mut counts.field_visits,
            nodes[parent].links.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        let selected: Vec<_> = nodes[parent]
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.binding_admitted
                    && matches!(l.role, Role::WeaponProjectile | Role::AmmoProjectile)
            })
            .map(|(at, l)| {
                (
                    at,
                    l.binding
                        .key
                        .as_ref()
                        .expect("defined projectile key")
                        .clone(),
                )
            })
            .collect();
        for (link, key) in selected {
            let depth = if parent == 0 { 1 } else { 2 };
            budget(depth <= limits.max_depth, "depth")?;
            counts.source_depth = counts.source_depth.max(depth);
            let child = if let Some(at) = index.get(&key) {
                *at
            } else {
                let node = decode(store, &sources, &key, depth, &mut counts, limits)?;
                let at = nodes.len();
                index.insert(key, at);
                nodes.push(node);
                at
            };
            nodes[parent].links[link].target_node = Some(child);
        }
    }
    let result = Manifest {
        sources,
        winning_content_sha256: digest,
        weapon: weapon.clone(),
        explicit_ammo: choice,
        source_nodes: nodes,
        counts,
        ammo_choice_verified: false,
        projectile_priority_selected: false,
        firing_supported: false,
        scope: "Explicit WEAP and optional caller AMMO physical declarations, independent weapon/ammo projectile inputs and winning header-only leaf links; raw signed/unsigned/float bits/flags, no inferred ammo or list membership, projectile precedence/fallback, editor normalization, fire/reload/consumption/damage/spread/ballistics/mod effects or execution",
    };
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|e| Error::Unsupported(format!("actor attack input projection: {e}")))?;
    Ok(result)
}
