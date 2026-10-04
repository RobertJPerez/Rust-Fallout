//! Exact actor/race voice declarations, never effective voice or audio selection.
use super::{
    Catalogue, Definition, associations,
    dependencies::{ConfigurationOrigin, Sex},
    fields, races,
};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_voice_sources: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_declarations: usize,
    pub max_visits: usize,
    pub max_issues: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_voice_sources: 3,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 128 * 1024 * 1024,
            max_fields: 16_384,
            max_declarations: 128,
            max_visits: 65_536,
            max_issues: 128,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub decoded_bytes: usize,
    pub fields: usize,
    pub declarations: usize,
    pub visits: usize,
}
#[derive(Debug, Serialize)]
pub struct VoiceField {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    /// Optional raw DNAM byte, with unknown bits unchanged.
    pub flags: Option<u8>,
}
#[derive(Debug, Serialize)]
pub struct VoiceSource {
    pub key: FormKey,
    pub source: inventory::Source,
    pub header: plugin::RecordHeader,
    pub deleted: bool,
    pub fields: Vec<VoiceField>,
    pub source_body_read: bool,
}
#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub source: FormKey,
    pub field_index: Option<usize>,
}
#[derive(Serialize)]
pub struct ActorVoice<'a> {
    pub association: &'a associations::Association,
    pub field: &'a fields::Field,
    pub ambiguous_source: bool,
    pub target_source_index: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct RaceVoice<'a> {
    pub race_field_index: usize,
    pub field: &'a races::Field,
    pub sex: Sex,
    pub binding: inventory::Binding,
    pub ambiguous_source: bool,
    pub matches_authored_actor_sex: Option<bool>,
    pub target_source_index: Option<usize>,
}
#[derive(Serialize)]
pub struct RaceRequest<'a> {
    pub association: &'a associations::Association,
    pub field: &'a fields::Field,
    pub ambiguous_source: bool,
    pub definition: Option<&'a races::Definition>,
    pub voices: Vec<RaceVoice<'a>>,
}
#[derive(Serialize)]
pub struct Manifest<'a> {
    pub sources: &'a [SourceReceipt],
    pub winning_content_sha256: &'a str,
    pub actor: &'a Definition<'a>,
    pub configuration: Option<ConfigurationOrigin>,
    pub authored_sex: Option<Sex>,
    pub traits_template_flag_present: Option<bool>,
    pub actor_voices: Vec<ActorVoice<'a>>,
    pub race_requests: Vec<RaceRequest<'a>>,
    pub voice_sources: Vec<VoiceSource>,
    pub issues: Vec<Issue>,
    pub counts: Counts,
    pub template_inheritance_supported: bool,
    pub effective_voice_selection_supported: bool,
    pub dialogue_truth_supported: bool,
    pub audio_path_selection_supported: bool,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}
fn budget(label: &str) -> Error {
    Error::Unsupported(format!("actor voice {label} budget exceeded"))
}
fn admit(value: usize, maximum: usize, label: &str) -> Result<()> {
    if value > maximum {
        Err(budget(label))
    } else {
        Ok(())
    }
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .filter(|next| *next <= self.maximum)
            .ok_or_else(|| std::io::Error::other("actor voice projection byte budget"))?;
        self.bytes = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn joined_field<'a>(
    actor: &'a Definition<'a>,
    association: &associations::Association,
    tag: [u8; 4],
) -> Result<&'a fields::Field> {
    actor
        .fields
        .get(association.field_index)
        .filter(|field| field.kind == tag)
        .ok_or_else(|| Error::Resolution("voice actor physical association origin differs".into()))
}
fn matches_target(
    binding: &inventory::Binding,
    kind: [u8; 4],
    source: &inventory::Source,
    header: &plugin::RecordHeader,
) -> bool {
    binding.target.as_ref().is_some_and(|target| {
        target.kind == kind
            && target.source_plugin == source.plugin
            && target.record_file_offset == header.offset
            && target.record_flags == header.flags
    })
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
    fn declaration(&mut self, limits: Limits) -> Result<()> {
        self.counts.declarations = self
            .counts
            .declarations
            .checked_add(1)
            .ok_or_else(|| budget("declaration"))?;
        admit(
            self.counts.declarations,
            limits.max_declarations,
            "declaration",
        )
    }
    fn issue(
        &mut self,
        code: &'static str,
        source: &FormKey,
        index: Option<usize>,
        limits: Limits,
    ) -> Result<()> {
        admit(self.issues.len() + 1, limits.max_issues, "issue")?;
        self.issues.push(Issue {
            code,
            source: source.clone(),
            field_index: index,
        });
        Ok(())
    }
    fn voice(
        &mut self,
        store: &mut RecordStore,
        binding: &inventory::Binding,
        origin: &FormKey,
        index: usize,
        limits: Limits,
    ) -> Result<Option<usize>> {
        if binding.status != inventory::Status::Defined
            || binding
                .target
                .as_ref()
                .is_none_or(|target| target.kind != *b"VTYP")
        {
            self.issue("unavailable_voice_type", origin, Some(index), limits)?;
            return Ok(None);
        }
        let key = binding
            .key
            .as_ref()
            .ok_or_else(|| Error::Resolution("defined voice lacks key".into()))?;
        if let Some(index) = self
            .voice_sources
            .iter()
            .position(|source| source.key == *key)
        {
            return Ok(Some(index));
        }
        admit(
            self.voice_sources.len() + 1,
            limits.max_voice_sources,
            "source",
        )?;
        let location = store
            .winner(key)
            .ok_or_else(|| Error::Resolution("defined voice winner unavailable".into()))?;
        let header = store.definition(location).header.clone();
        let mut source = VoiceSource {
            key: key.clone(),
            source: inventory::Source {
                plugin: store.source_name(location).into(),
                sha256: store.source_digest(location)?,
                record_file_offset: header.offset,
                record_flags: header.flags,
                decoded_record_sha256: None,
            },
            deleted: header.flags & plugin::DELETED != 0,
            header,
            fields: Vec::new(),
            source_body_read: false,
        };
        if !matches_target(binding, *b"VTYP", &source.source, &source.header) || source.deleted {
            return Err(Error::Resolution("voice target provenance differs".into()));
        }
        // Original base ESM independently observed these versions; version1
        // omits optional DNAM rather than supplying a synthesized flag default.
        if !matches!(source.header.version, 1 | 4 | 9 | 11 | 13 | 14 | 15) {
            self.issue("unsupported_voice_type_version", key, None, limits)?;
        } else {
            let maximum = limits.max_record_bytes.min(
                limits
                    .max_decoded_bytes
                    .saturating_sub(self.counts.decoded_bytes),
            );
            let record = store.read_bounded(location, maximum)?;
            if record.integrity_issue.is_some() {
                return Err(Error::Resolution("tainted voice source".into()));
            }
            let mut flag_fields = 0usize;
            plugin::visit_subrecords(&record, &source.source.plugin, |field| {
                admit(
                    self.counts.fields + source.fields.len() + 1,
                    limits.max_fields,
                    "field",
                )?;
                let flags = if field.kind == *b"DNAM" {
                    if field.data.len() != 1 {
                        return Err(crate::malformed(
                            &source.source.plugin,
                            record.header.offset,
                            "VTYP DNAM requires one byte",
                        ));
                    }
                    flag_fields += 1;
                    Some(field.data[0])
                } else {
                    None
                };
                source.fields.push(VoiceField {
                    kind: field.kind,
                    decoded_offset: u32::try_from(field.payload_offset)
                        .map_err(|_| budget("field offset"))?,
                    bytes: field.data.len(),
                    sha256: format!("{:x}", Sha256::digest(field.data)),
                    flags,
                });
                Ok(())
            })?;
            if flag_fields > 1 {
                self.issue("ambiguous_voice_type_flags", key, None, limits)?;
            }
            source.source.decoded_record_sha256 =
                Some(format!("{:x}", Sha256::digest(&record.payload)));
            source.source_body_read = true;
            self.counts.decoded_bytes = self
                .counts
                .decoded_bytes
                .checked_add(record.payload.len())
                .ok_or_else(|| budget("decoded byte"))?;
            self.counts.fields += source.fields.len();
            self.visit(source.fields.len(), limits)?;
        }
        let index = self.voice_sources.len();
        self.voice_sources.push(source);
        Ok(Some(index))
    }
}
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    races: &'a races::Catalogue,
    actor_key: &FormKey,
    limits: Limits,
) -> Result<Manifest<'a>> {
    let sources = store.source_receipts()?;
    let digest = record_metadata::inspect(store)?.winning_definitions_sha256;
    for (receipts, winners) in [
        (actors.sources(), actors.winning_content_sha256()),
        (
            associations.sources(),
            associations.winning_content_sha256(),
        ),
        (races.sources(), races.winning_content_sha256()),
    ] {
        if !same_sources(receipts, &sources) || winners != digest {
            return Err(Error::Resolution(
                "voice source cohorts or winners differ".into(),
            ));
        }
    }
    let actor = actors
        .get(actor_key)
        .filter(|actor| !actor.deleted)
        .ok_or_else(|| Error::Resolution("voice actor winner unavailable".into()))?;
    let links = associations
        .get(actor_key)
        .ok_or_else(|| Error::Resolution("voice actor associations unavailable".into()))?;
    let mut result = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration: None,
        authored_sex: None,
        traits_template_flag_present: None,
        actor_voices: Vec::new(),
        race_requests: Vec::new(),
        voice_sources: Vec::new(),
        issues: Vec::new(),
        counts: Counts::default(),
        template_inheritance_supported: false,
        effective_voice_selection_supported: false,
        dialogue_truth_supported: false,
        audio_path_selection_supported: false,
        original_behavior_verified: false,
        scope: "Exact authored actor VTCK/RNAM, sex-bound RACE voice declarations and winning VTYP physical flags; no template inheritance, effective voice/default-dialogue selection, audio language/path, dialogue conditions or gameplay",
    };
    let inventory_fields = &actor.inventory_definition().fields;
    let actor_record = actor
        .record()
        .ok_or_else(|| Error::Resolution("voice actor body unavailable".into()))?;
    admit(
        actor_record.payload.len(),
        limits.max_record_bytes,
        "actor record byte",
    )?;
    result.counts.decoded_bytes = actor_record.payload.len();
    admit(
        result.counts.decoded_bytes,
        limits.max_decoded_bytes,
        "decoded byte",
    )?;
    result.counts.fields = actor.fields.len();
    admit(result.counts.fields, limits.max_fields, "field")?;
    result.visit(
        actor.fields.len() + inventory_fields.len() + links.associations.len(),
        limits,
    )?;
    let configurations: Vec<_> = inventory_fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            if let inventory::Value::ActorBase {
                flags,
                template_flags,
                ..
            } = field.value
            {
                Some(ConfigurationOrigin {
                    inventory_field_index: index,
                    field_decoded_offset: field.decoded_offset,
                    flags,
                    template_flags,
                })
            } else {
                None
            }
        })
        .collect();
    if configurations.len() == 1 {
        result.configuration = configurations.into_iter().next();
        let configuration = result.configuration.as_ref().expect("one configuration");
        let inherited = configuration.template_flags & 1 != 0;
        result.traits_template_flag_present = Some(inherited);
        if inherited {
            result.issue(
                "traits_template_selection_unsupported",
                actor_key,
                None,
                limits,
            )?;
        } else if actor.kind == *b"NPC_" {
            result.authored_sex = Some(if configuration.flags & 1 == 0 {
                Sex::Male
            } else {
                Sex::Female
            });
        }
    } else {
        result.issue(
            if configurations.is_empty() {
                "missing_actor_configuration"
            } else {
                "ambiguous_actor_configuration"
            },
            actor_key,
            None,
            limits,
        )?;
    }
    let voice_count = links
        .associations
        .iter()
        .filter(|link| link.role == associations::Role::Voice)
        .count();
    let race_count = links
        .associations
        .iter()
        .filter(|link| link.role == associations::Role::Race)
        .count();
    if voice_count == 0 {
        result.issue("missing_actor_voice_declaration", actor_key, None, limits)?;
    }
    if actor.kind == *b"NPC_" && race_count == 0 {
        result.issue("missing_actor_race_declaration", actor_key, None, limits)?;
    }
    for association in &links.associations {
        if association.role == associations::Role::Voice {
            result.declaration(limits)?;
            let field = joined_field(actor, association, *b"VTCK")?;
            let target_source_index = if voice_count > 1 {
                result.issue(
                    "ambiguous_actor_voice_declaration",
                    actor_key,
                    Some(association.field_index),
                    limits,
                )?;
                None
            } else {
                result.voice(
                    store,
                    &association.binding,
                    actor_key,
                    association.field_index,
                    limits,
                )?
            };
            result.actor_voices.push(ActorVoice {
                association,
                field,
                ambiguous_source: voice_count > 1,
                target_source_index,
            });
        } else if association.role == associations::Role::Race {
            result.declaration(limits)?;
            let field = joined_field(actor, association, *b"RNAM")?;
            let mut request = RaceRequest {
                association,
                field,
                ambiguous_source: race_count > 1,
                definition: None,
                voices: Vec::new(),
            };
            if race_count > 1 {
                result.issue(
                    "ambiguous_actor_race_declaration",
                    actor_key,
                    Some(association.field_index),
                    limits,
                )?;
            } else if association.binding.status != inventory::Status::Defined
                || association.schema_kind_allowed != Some(true)
            {
                result.issue(
                    "unavailable_actor_race",
                    actor_key,
                    Some(association.field_index),
                    limits,
                )?;
            } else {
                let key = association
                    .binding
                    .key
                    .as_ref()
                    .ok_or_else(|| Error::Resolution("defined race lacks key".into()))?;
                let race = races
                    .get(key)
                    .filter(|race| !race.deleted)
                    .ok_or_else(|| Error::Resolution("defined race source unavailable".into()))?;
                if !matches_target(&association.binding, *b"RACE", &race.source, &race.header) {
                    return Err(Error::Resolution(
                        "voice race target provenance differs".into(),
                    ));
                }
                request.definition = Some(race);
                let record = race
                    .record()
                    .ok_or_else(|| Error::Resolution("voice race body unavailable".into()))?;
                admit(
                    record.payload.len(),
                    limits.max_record_bytes,
                    "race record byte",
                )?;
                result.counts.decoded_bytes = result
                    .counts
                    .decoded_bytes
                    .checked_add(record.payload.len())
                    .ok_or_else(|| budget("decoded byte"))?;
                admit(
                    result.counts.decoded_bytes,
                    limits.max_decoded_bytes,
                    "decoded byte",
                )?;
                result.counts.fields = result
                    .counts
                    .fields
                    .checked_add(race.fields.len())
                    .ok_or_else(|| budget("field"))?;
                admit(result.counts.fields, limits.max_fields, "field")?;
                result.visit(race.fields.len(), limits)?;
                let count = race
                    .fields
                    .iter()
                    .filter(|field| field.kind == *b"VTCK")
                    .count();
                if count == 0 {
                    result.issue("missing_race_voice_declaration", key, None, limits)?;
                }
                let location = store
                    .winner(key)
                    .ok_or_else(|| Error::Resolution("race winner disappeared".into()))?;
                if store.definition(location).header != race.header
                    || store.source_name(location) != race.source.plugin
                    || store.source_digest(location)? != race.source.sha256
                {
                    return Err(Error::Resolution("voice race winner source differs".into()));
                }
                let mut index = 0usize;
                let mut binding_counts = inventory::Counts::default();
                plugin::visit_subrecords(record, &race.source.plugin, |raw| {
                    let field = race
                        .fields
                        .get(index)
                        .ok_or_else(|| Error::Resolution("race field origin absent".into()))?;
                    let field_index = index;
                    index += 1;
                    if raw.kind != field.kind
                        || raw.payload_offset != field.decoded_offset as usize
                        || raw.data.len() != field.bytes
                        || format!("{:x}", Sha256::digest(raw.data)) != field.sha256
                    {
                        return Err(Error::Resolution(
                            "voice race physical field origin differs".into(),
                        ));
                    }
                    if raw.kind != *b"VTCK" {
                        return Ok(());
                    }
                    if raw.data.len() != 8 {
                        return Err(crate::malformed(
                            &race.source.plugin,
                            record.header.offset,
                            "RACE VTCK requires male/female FormID pair",
                        ));
                    }
                    if count > 1 {
                        result.issue(
                            "ambiguous_race_voice_declaration",
                            key,
                            Some(field_index),
                            limits,
                        )?;
                    }
                    for (slot, sex) in [Sex::Male, Sex::Female].into_iter().enumerate() {
                        result.declaration(limits)?;
                        let binding = inventory::binding(
                            store,
                            location,
                            u32::from_le_bytes(
                                raw.data[slot * 4..slot * 4 + 4]
                                    .try_into()
                                    .expect("checked voice pair"),
                            ),
                            &mut binding_counts,
                        )?;
                        let target_source_index = if count > 1 {
                            None
                        } else {
                            result.voice(store, &binding, key, field_index, limits)?
                        };
                        request.voices.push(RaceVoice {
                            race_field_index: field_index,
                            field,
                            sex,
                            binding,
                            ambiguous_source: count > 1,
                            matches_authored_actor_sex: result
                                .authored_sex
                                .map(|actor_sex| actor_sex == sex),
                            target_source_index,
                        });
                    }
                    Ok(())
                })?;
            }
            result.race_requests.push(request);
        }
    }
    serde_json::to_writer(
        &mut ProjectionBudget {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|_| budget("projection byte"))?;
    Ok(result)
}
