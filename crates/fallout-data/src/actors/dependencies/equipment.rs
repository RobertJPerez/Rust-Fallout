//! Explicit equipment source roles. Inventory presence never selects equipment.
use super::{
    Body, Definition, Field, LinkRole, LookupStatus, ManifestCounts, ManifestLimits, PathRequest,
    PathRole, RenderSource, Sex, Value, fields, manifest,
};
use crate::{
    Error, Result, actors,
    assets::ArchiveAssets,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Role {
    ArmorBiped { sex: Sex },
    ArmorWorld { sex: Sex },
    WeaponModel { mod_mask: u8 },
    WeaponFirstPerson { mod_mask: u8 },
    WeaponShell,
    WeaponScope,
    WeaponWorld,
}
impl Role {
    fn field(self) -> Result<[u8; 4]> {
        let model = [
            *b"MODL", *b"MWD1", *b"MWD2", *b"MWD3", *b"MWD4", *b"MWD5", *b"MWD6", *b"MWD7",
        ];
        let first = [
            *b"WNAM", *b"WNM1", *b"WNM2", *b"WNM3", *b"WNM4", *b"WNM5", *b"WNM6", *b"WNM7",
        ];
        Ok(match self {
            Self::ArmorBiped { sex: Sex::Male } => *b"MODL",
            Self::ArmorBiped { sex: Sex::Female } => *b"MOD3",
            Self::ArmorWorld { sex: Sex::Male } => *b"MOD2",
            Self::ArmorWorld { sex: Sex::Female } => *b"MOD4",
            Self::WeaponModel { mod_mask } => {
                *model.get(usize::from(mod_mask)).ok_or_else(|| {
                    Error::Unsupported("equipment weapon source mask must be 0..7".into())
                })?
            }
            Self::WeaponFirstPerson { mod_mask } => {
                *first.get(usize::from(mod_mask)).ok_or_else(|| {
                    Error::Unsupported("equipment first-person source mask must be 0..7".into())
                })?
            }
            Self::WeaponShell => *b"MOD2",
            Self::WeaponScope => *b"MOD3",
            Self::WeaponWorld => *b"MOD4",
        })
    }
    fn allowed(self, kind: &[u8; 4]) -> bool {
        match self {
            Self::ArmorBiped { .. } | Self::ArmorWorld { .. } => matches!(kind, b"ARMO" | b"ARMA"),
            _ => kind == b"WEAP",
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub equipment: FormKey,
    pub role: Role,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_strings: usize,
    pub max_path_bytes: usize,
    pub max_bindings: usize,
    pub max_requests: usize,
    pub max_selected_links: usize,
    pub max_issues: usize,
    pub max_visits: usize,
    pub lookup: ManifestLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 2,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 128 * 1024 * 1024,
            max_fields: 16_384,
            max_strings: 4096,
            max_path_bytes: 1024 * 1024,
            max_bindings: 4096,
            max_requests: 32,
            max_selected_links: 32,
            max_issues: 128,
            max_visits: 65_536,
            lookup: ManifestLimits::default(),
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub decoded_bytes: usize,
    pub fields: usize,
    pub strings: usize,
    pub path_bytes: usize,
    pub bindings: usize,
    pub visits: usize,
    pub lookup: ManifestCounts,
}
#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub source_index: Option<usize>,
    pub field_index: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct ModelRequest {
    pub source_index: usize,
    pub role: Role,
    pub ambiguous_source: bool,
    pub path: PathRequest,
}
#[derive(Debug, Serialize)]
pub struct SelectedLink {
    pub source_index: usize,
    pub field_index: usize,
    /// Index of a privately joined singleton STAT winner, when available.
    pub target_source_index: Option<usize>,
    pub ambiguous_source: bool,
}
#[derive(Serialize)]
pub struct Manifest<'a> {
    pub sources: &'a [SourceReceipt],
    pub winning_content_sha256: &'a str,
    pub actor: RenderSource<'a>,
    pub explicit_choice: Choice,
    /// Selected equipment, then optional singleton first-person STAT. Other
    /// inventory items and unselected model links are never read here.
    pub source_records: Vec<Definition<'static>>,
    pub selected_links: Vec<SelectedLink>,
    pub requests: Vec<ModelRequest>,
    pub issues: Vec<Issue>,
    pub counts: Counts,
    pub equipped_state_verified: bool,
    pub effective_model_selection_supported: bool,
    pub slot_conflicts_evaluated: bool,
    pub texture_swaps_applied: bool,
    pub attachment_target_selected: bool,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}

fn budget(label: &str) -> Error {
    Error::Unsupported(format!("equipment source {label} budget exceeded"))
}
fn admit(value: usize, maximum: usize, label: &str) -> Result<()> {
    if value > maximum {
        Err(budget(label))
    } else {
        Ok(())
    }
}
impl Manifest<'_> {
    fn visit(&mut self, count: usize, limits: Limits) -> Result<()> {
        self.counts.visits = self
            .counts
            .visits
            .checked_add(count)
            .ok_or_else(|| budget("visit"))?;
        admit(self.counts.visits, limits.max_visits, "visit")
    }
    fn issue(
        &mut self,
        code: &'static str,
        source: Option<usize>,
        field: Option<usize>,
        limits: Limits,
    ) -> Result<()> {
        admit(self.issues.len() + 1, limits.max_issues, "issue")?;
        self.issues.push(Issue {
            code,
            source_index: source,
            field_index: field,
        });
        Ok(())
    }
    fn load_source(
        &mut self,
        store: &mut RecordStore,
        location: Location,
        key: &FormKey,
        read_body: bool,
        limits: Limits,
    ) -> Result<usize> {
        admit(self.source_records.len() + 1, limits.max_sources, "source")?;
        let header = store.definition(location).header.clone();
        let deleted = header.flags & plugin::DELETED != 0;
        let mut definition = Definition {
            key: key.clone(),
            source: inventory::Source {
                plugin: store.source_name(location).into(),
                sha256: store.source_digest(location)?,
                record_file_offset: header.offset,
                record_flags: header.flags,
                decoded_record_sha256: None,
            },
            header,
            deleted,
            fields: Vec::new(),
            race_links: Vec::new(),
            findings: Vec::new(),
            body: None,
        };
        if read_body && !deleted {
            // This bounded source slice admits the observed v15 layout. Older
            // versions are not silently treated as the current equipment schema.
            if definition.header.version != 15 {
                return Err(Error::Unsupported(format!(
                    "equipment {} source version {} unsupported",
                    plugin::signature(definition.header.kind),
                    definition.header.version
                )));
            }
            let maximum = limits.max_record_bytes.min(
                limits
                    .max_decoded_bytes
                    .saturating_sub(self.counts.decoded_bytes),
            );
            let record = store.read_bounded(location, maximum)?;
            let document = fields::decode(
                store,
                location,
                &record,
                fields::Limits {
                    max_fields: limits.max_fields.saturating_sub(self.counts.fields),
                    max_strings: limits.max_strings.saturating_sub(self.counts.strings),
                    max_path_bytes: limits.max_path_bytes.saturating_sub(self.counts.path_bytes),
                    max_bindings: limits.max_bindings.saturating_sub(self.counts.bindings),
                },
            )?;
            self.counts.decoded_bytes += record.payload.len();
            self.counts.fields += document.counts.fields;
            self.counts.strings += document.counts.strings;
            self.counts.path_bytes += document.counts.path_bytes;
            self.counts.bindings += document.counts.bindings;
            definition.source.decoded_record_sha256 =
                Some(format!("{:x}", Sha256::digest(&record.payload)));
            definition.fields = document.fields;
            definition.findings = document.findings;
            definition.body = Some(Body::Owned(record));
        }
        let index = self.source_records.len();
        self.source_records.push(definition);
        Ok(index)
    }
    fn models(
        &mut self,
        index: usize,
        tag: [u8; 4],
        assets: &ArchiveAssets,
        limits: Limits,
    ) -> Result<()> {
        self.visit(self.source_records[index].fields.len(), limits)?;
        let fields: Vec<usize> = self.source_records[index]
            .fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| (f.kind == tag).then_some(i))
            .collect();
        if fields.is_empty() {
            self.issue("missing_selected_model_field", Some(index), None, limits)?;
        }
        let ambiguous_source = fields.len() > 1;
        if ambiguous_source {
            self.issue("ambiguous_selected_model_fields", Some(index), None, limits)?;
        }
        for field_index in fields {
            admit(self.requests.len() + 1, limits.max_requests, "request")?;
            let definition = &self.source_records[index];
            let field = &definition.fields[field_index];
            let Value::Paths {
                role: PathRole::Model,
                context,
                strings,
            } = &field.value
            else {
                return Err(Error::Resolution(
                    "selected equipment model was not decoded as a model path".into(),
                ));
            };
            if strings.len() != 1 {
                return Err(Error::Resolution(
                    "selected model has multiple physical string frames".into(),
                ));
            }
            let raw = &strings[0].raw;
            admit(
                self.counts.lookup.paths + 1,
                limits.lookup.max_paths,
                "lookup path",
            )?;
            admit(
                raw.len(),
                limits
                    .lookup
                    .max_path_bytes
                    .saturating_sub(self.counts.lookup.path_bytes),
                "lookup path byte",
            )?;
            let (asset_path, lookup_status, candidates) = manifest::path(
                assets,
                PathRole::Model,
                raw,
                &mut self.counts.lookup,
                limits.lookup,
            )?;
            self.counts.lookup.paths += 1;
            self.counts.lookup.path_bytes += raw.len();
            *self
                .counts
                .lookup
                .lookup_statuses
                .entry(lookup_status.label().into())
                .or_default() += 1;
            let path = PathRequest {
                source: definition.key.clone(),
                field_index,
                field_decoded_offset: field.decoded_offset,
                field_byte_offset: strings[0].field_byte_offset,
                role: PathRole::Model,
                context: *context,
                raw: raw.clone(),
                asset_path,
                lookup_status,
                candidates,
            };
            if lookup_status != LookupStatus::OneArchiveCandidate {
                self.issue(
                    "selected_model_asset_unavailable_or_ambiguous",
                    Some(index),
                    Some(field_index),
                    limits,
                )?;
            }
            self.requests.push(ModelRequest {
                source_index: index,
                role: self.explicit_choice.role,
                ambiguous_source,
                path,
            });
        }
        Ok(())
    }
}

pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a actors::Catalogue<'a>,
    actor_key: &FormKey,
    choice: Choice,
    assets: &ArchiveAssets,
    limits: Limits,
) -> Result<Manifest<'a>> {
    let tag = choice.role.field()?;
    let sources = store.source_receipts()?;
    if sources.len() != actors.sources().len()
        || !sources.iter().zip(actors.sources()).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
        || record_metadata::inspect(store)?.winning_definitions_sha256
            != actors.winning_content_sha256()
    {
        return Err(Error::Resolution(
            "equipment source cohort or winners differ from actor".into(),
        ));
    }
    let actor = actors
        .get(actor_key)
        .filter(|actor| !actor.deleted)
        .ok_or_else(|| Error::Resolution("equipment request actor winner unavailable".into()))?;
    let mut result = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor: RenderSource {
            key: actor.key,
            source: actor.source,
            header: &actor
                .record()
                .ok_or_else(|| {
                    Error::Resolution("equipment request actor body unavailable".into())
                })?
                .header,
        },
        explicit_choice: choice,
        source_records: Vec::new(),
        selected_links: Vec::new(),
        requests: Vec::new(),
        issues: Vec::new(),
        counts: Counts::default(),
        equipped_state_verified: false,
        effective_model_selection_supported: false,
        slot_conflicts_evaluated: false,
        texture_swaps_applied: false,
        attachment_target_selected: false,
        original_behavior_verified: false,
        scope: "Explicit caller equipment/source role and winning model/slot/first-person declarations with physical archive candidates; no inferred equipped inventory, actor sex fallback, active weapon mods, slot conflicts, texture swaps, attachment node selection, NIF decoding, playback or retail model choice",
    };
    let key = result.explicit_choice.equipment.clone();
    let Some(location) = store.winner(&key) else {
        result.issue("selected_equipment_missing", None, None, limits)?;
        return Ok(result);
    };
    let header = &store.definition(location).header;
    let allowed = result.explicit_choice.role.allowed(&header.kind);
    let deleted = header.flags & plugin::DELETED != 0;
    let index = result.load_source(store, location, &key, allowed, limits)?;
    if deleted {
        result.issue("selected_equipment_deleted", Some(index), None, limits)?;
        return Ok(result);
    }
    if !allowed {
        result.issue("selected_equipment_wrong_kind", Some(index), None, limits)?;
        return Ok(result);
    }
    result.visit(result.source_records[index].fields.len(), limits)?;
    for slot_tag in [*b"BMDT", *b"ETYP"] {
        if slot_tag == *b"BMDT" && result.source_records[index].header.kind == *b"WEAP" {
            continue;
        }
        // Two bounded physical scans; preserve both repeated singleton fields.
        result.visit(result.source_records[index].fields.len(), limits)?;
        let count = result.source_records[index]
            .fields
            .iter()
            .filter(|f| f.kind == slot_tag)
            .count();
        if count == 0 {
            result.issue("missing_equipment_slot_field", Some(index), None, limits)?;
        }
        if count > 1 {
            result.issue("ambiguous_equipment_slot_fields", Some(index), None, limits)?;
        }
    }
    if !matches!(result.explicit_choice.role, Role::WeaponFirstPerson { .. }) {
        result.models(index, tag, assets, limits)?;
        return Ok(result);
    }
    result.visit(result.source_records[index].fields.len(), limits)?;
    let links: Vec<usize> = result.source_records[index]
        .fields
        .iter()
        .enumerate()
        .filter_map(|(i, f)| (f.kind == tag).then_some(i))
        .collect();
    if links.is_empty() {
        result.issue(
            "missing_selected_first_person_link",
            Some(index),
            None,
            limits,
        )?;
    }
    let ambiguous = links.len() > 1;
    if ambiguous {
        result.issue(
            "ambiguous_selected_first_person_links",
            Some(index),
            None,
            limits,
        )?;
    }
    for field_index in links {
        admit(
            result.selected_links.len() + 1,
            limits.max_selected_links,
            "selected link",
        )?;
        let field: &Field = &result.source_records[index].fields[field_index];
        let Value::Links {
            role: LinkRole::FirstPersonModel,
            bindings,
        } = &field.value
        else {
            return Err(Error::Resolution(
                "first-person physical link join differs".into(),
            ));
        };
        if bindings.len() != 1 {
            return Err(Error::Resolution(
                "first-person singleton has multiple bindings".into(),
            ));
        }
        let link = &bindings[0];
        let mut target_index = None;
        if !ambiguous
            && link.binding.status == inventory::Status::Defined
            && link.schema_kind_allowed == Some(true)
        {
            let key = link
                .binding
                .key
                .as_ref()
                .ok_or_else(|| Error::Resolution("defined STAT key absent".into()))?
                .clone();
            let location = store
                .winner(&key)
                .ok_or_else(|| Error::Resolution("defined STAT winner absent".into()))?;
            let target = link
                .binding
                .target
                .as_ref()
                .ok_or_else(|| Error::Resolution("defined STAT provenance absent".into()))?;
            let header = &store.definition(location).header;
            if header.kind != target.kind
                || header.offset != target.record_file_offset
                || header.flags != target.record_flags
                || store.source_name(location) != target.source_plugin
            {
                return Err(Error::Resolution(
                    "first-person STAT target provenance differs".into(),
                ));
            }
            let selected = result.load_source(store, location, &key, true, limits)?;
            target_index = Some(selected);
            result.models(selected, *b"MODL", assets, limits)?;
        } else if !ambiguous {
            result.issue(
                "selected_first_person_target_unavailable",
                Some(index),
                Some(field_index),
                limits,
            )?;
        }
        result.selected_links.push(SelectedLink {
            source_index: index,
            field_index,
            target_source_index: target_index,
            ambiguous_source: ambiguous,
        });
    }
    Ok(result)
}
