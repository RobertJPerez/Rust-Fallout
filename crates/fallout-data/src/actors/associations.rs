//! Ordered authored links over an exact scalar-source/store cohort. Resolving a
//! declaration does not select a runtime race, faction, effect or AI package.
use super::{Catalogue as Actors, fields::Finding};
use crate::{
    Error, Result,
    identity::{self, FormKey},
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_fields: usize,
    pub max_bindings: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_fields: 2_000_000,
            max_bindings: 1_000_000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Faction,
    Race,
    Class,
    Voice,
    DeathItem,
    ActorEffect,
    UnarmedEffect,
    Package,
}
impl Role {
    fn allowed(self, kind: &[u8; 4]) -> bool {
        match self {
            Self::Faction => kind == b"FACT",
            Self::Race => kind == b"RACE",
            Self::Class => kind == b"CLAS",
            Self::Voice => kind == b"VTYP",
            Self::DeathItem => kind == b"LVLI",
            Self::ActorEffect => kind == b"SPEL",
            Self::UnarmedEffect => matches!(kind, b"ENCH" | b"SPEL"),
            Self::Package => kind == b"PACK",
        }
    }
    fn repeated(self) -> bool {
        matches!(self, Self::Faction | Self::ActorEffect | Self::Package)
    }
    fn label(self) -> &'static str {
        match self {
            Self::Faction => "faction",
            Self::Race => "race",
            Self::Class => "class",
            Self::Voice => "voice",
            Self::DeathItem => "death_item",
            Self::ActorEffect => "actor_effect",
            Self::UnarmedEffect => "unarmed_effect",
            Self::Package => "package",
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Association {
    /// Index in the joined actor scalar definition's physical fields.
    pub field_index: usize,
    pub role: Role,
    pub binding: inventory::Binding,
    pub schema_kind_allowed: Option<bool>,
    pub faction_rank: Option<i8>,
    pub faction_unused: Option<[u8; 3]>,
}
#[derive(Debug, Serialize)]
pub struct Definition<'a> {
    pub key: &'a FormKey,
    pub associations: Vec<Association>,
    pub findings: Vec<Finding>,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub bindings: usize,
    pub source_findings: usize,
    pub binding_statuses: BTreeMap<String, usize>,
    pub roles: BTreeMap<String, usize>,
}
pub struct Catalogue<'a> {
    definitions: BTreeMap<FormKey, Definition<'a>>,
    sources: &'a [SourceReceipt],
    winning_content_sha256: &'a str,
    counts: Counts,
}
fn cohort(sources: &[SourceReceipt]) -> Result<BTreeMap<String, (u64, &str)>> {
    let mut result = BTreeMap::new();
    for source in sources {
        if result
            .insert(
                identity::plugin_name(&source.source_name)?,
                (source.source_bytes, source.source_sha256.as_str()),
            )
            .is_some()
        {
            return Err(Error::Resolution(
                "duplicate normalized actor source receipt".into(),
            ));
        }
    }
    Ok(result)
}
impl<'a> Catalogue<'a> {
    pub fn sources(&self) -> &[SourceReceipt] {
        self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        self.winning_content_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition<'a>> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition<'a>)> {
        self.definitions.iter()
    }
    pub fn load(store: &mut RecordStore, actors: &'a Actors<'_>, limits: Limits) -> Result<Self> {
        let sources = store.source_receipts()?;
        if cohort(&sources)? != cohort(actors.sources())?
            || record_metadata::inspect(store)?.winning_definitions_sha256
                != actors.winning_content_sha256()
        {
            return Err(Error::Resolution(
                "actor association source cohorts or winners differ".into(),
            ));
        }
        if actors.counts().records > limits.max_records
            || actors.counts().fields > limits.max_fields
        {
            return Err(Error::Unsupported(
                "actor association record/field budget exceeded".into(),
            ));
        }
        let mut result = Self {
            definitions: BTreeMap::new(),
            sources: actors.sources(),
            winning_content_sha256: actors.winning_content_sha256(),
            counts: Counts::default(),
        };
        let mut binding_counts = inventory::Counts::default();
        for (key, actor) in actors.iter() {
            // Locations are resolved in the supplied store after both cohort
            // checks. An old plugin index cannot alias a reordered source.
            let location = store
                .winner(key)
                .ok_or_else(|| Error::Resolution("joined actor winner missing".into()))?;
            let header = &store.definition(location).header;
            if identity::plugin_name(store.source_name(location))?
                != identity::plugin_name(&actor.source.plugin)?
                || header.offset != actor.source.record_file_offset
                || header.flags != actor.source.record_flags
                || header.kind != actor.kind
            {
                return Err(Error::Resolution(
                    "joined actor winner provenance differs".into(),
                ));
            }
            let mut definition = Definition {
                key: actor.key,
                associations: Vec::new(),
                findings: Vec::new(),
            };
            if let Some(record) = actor.record() {
                if header != &record.header {
                    return Err(Error::Resolution("joined actor header differs".into()));
                }
                let npc = actor.kind == *b"NPC_";
                let mut seen = BTreeMap::<Role, usize>::new();
                let mut index = 0usize;
                plugin::visit_subrecords(record, &actor.source.plugin, |field| {
                    let field_index = index;
                    index += 1;
                    let role = match &field.kind {
                        b"SNAM" => Role::Faction,
                        b"RNAM" if npc => Role::Race,
                        b"CNAM" if npc => Role::Class,
                        b"VTCK" => Role::Voice,
                        b"INAM" => Role::DeathItem,
                        b"SPLO" => Role::ActorEffect,
                        b"EITM" => Role::UnarmedEffect,
                        b"PKID" => Role::Package,
                        _ => return Ok(()),
                    };
                    if result.counts.bindings >= limits.max_bindings {
                        return Err(Error::Unsupported(
                            "actor association binding budget exceeded".into(),
                        ));
                    }
                    let required = if role == Role::Faction { 8 } else { 4 };
                    if field.data.len() != required {
                        return Err(crate::malformed(
                            &actor.source.plugin,
                            record.header.offset,
                            format!(
                                "{} at decoded +0x{:X} needs {required} bytes, found {}",
                                plugin::signature(field.kind),
                                field.payload_offset,
                                field.data.len()
                            ),
                        ));
                    }
                    let offset = actor.fields[field_index].decoded_offset;
                    let count = seen.entry(role).or_default();
                    *count += 1;
                    if *count > 1 && !role.repeated() {
                        definition.findings.push(Finding {
                            field_decoded_offset: Some(offset),
                            code: "multiple_singleton_associations",
                        });
                    }
                    let raw = u32::from_le_bytes(
                        field.data[..4].try_into().expect("checked association"),
                    );
                    let binding = inventory::binding(store, location, raw, &mut binding_counts)?;
                    let allowed = binding
                        .target
                        .as_ref()
                        .map(|target| role.allowed(&target.kind));
                    let code = match binding.status {
                        inventory::Status::Missing => Some("association_target_missing"),
                        inventory::Status::Deleted => Some("association_target_deleted"),
                        _ if allowed == Some(false) => Some("association_target_wrong_kind"),
                        _ => None,
                    };
                    if let Some(code) = code {
                        definition.findings.push(Finding {
                            field_decoded_offset: Some(offset),
                            code,
                        });
                    }
                    definition.associations.push(Association {
                        field_index,
                        role,
                        binding,
                        schema_kind_allowed: allowed,
                        faction_rank: if role == Role::Faction {
                            Some(field.data[4] as i8)
                        } else {
                            None
                        },
                        faction_unused: if role == Role::Faction {
                            Some(field.data[5..].try_into().expect("checked faction"))
                        } else {
                            None
                        },
                    });
                    result.counts.bindings += 1;
                    *result.counts.roles.entry(role.label().into()).or_default() += 1;
                    Ok(())
                })?;
                if npc {
                    for (role, code) in [
                        (Role::Voice, "missing_voice_association"),
                        (Role::Race, "missing_race_association"),
                        (Role::Class, "missing_class_association"),
                    ] {
                        if !seen.contains_key(&role) {
                            definition.findings.push(Finding {
                                field_decoded_offset: None,
                                code,
                            });
                        }
                    }
                }
            }
            result.counts.records += 1;
            result.counts.source_findings += definition.findings.len();
            result.definitions.insert(key.clone(), definition);
        }
        result.counts.binding_statuses = binding_counts.binding_statuses;
        Ok(result)
    }
}
