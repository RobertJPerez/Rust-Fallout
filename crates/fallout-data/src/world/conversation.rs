//! Exact winning dialogue inputs for presentation and script consumers. A
//! caller requests an INFO explicitly; canonical membership is not retail
//! selection, condition truth, speaker assignment or result-script timing.
use crate::{
    Error, Result,
    condition_operands::{self, OwnerLimits, PreparedOwnerRecord, RecordLimits, Signatures},
    content,
    dialogue_membership::MembershipIndex,
    identity::FormKey,
    loaded_scripts::{Catalogue, LoadedScript, OwnerKind, ScriptKey},
    narrative::{self, SectionKind, Value},
    plugin, script_bindings,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub infos: usize,
    /// Aggregate max(stored, decoded) reads of topic, INFO and the INFO reread
    /// used by the existing condition owner consumer. Not total allocator use.
    pub read_bytes: usize,
    pub record_bytes: usize,
    pub fields: usize,
    pub sections: usize,
    pub conditions: usize,
    /// Compact JSON metadata plus retained raw record payloads. Independently
    /// bounded field/section counts also constrain temporary decoder storage.
    pub retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            infos: 262_144,
            read_bytes: 24 * 1024 * 1024,
            record_bytes: 8 * 1024 * 1024,
            fields: 65_536,
            sections: 8_192,
            conditions: 8_192,
            retained_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Private cohort seal prevents a request from silently changing content.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    topic: FormKey,
    info: FormKey,
    speaker: Option<FormKey>,
    source_cohort_sha256: String,
}
impl Request {
    pub fn topic(&self) -> &FormKey {
        &self.topic
    }
    pub fn info(&self) -> &FormKey {
        &self.info
    }
    /// Explicit caller identity, never an inferred default or evaluated speaker.
    pub fn speaker(&self) -> Option<&FormKey> {
        self.speaker.as_ref()
    }
    pub fn source_cohort_sha256(&self) -> &str {
        &self.source_cohort_sha256
    }
}

pub struct DialogueSources {
    membership: MembershipIndex,
    sources: Vec<SourceReceipt>,
    cohort: String,
    retained_bytes: usize,
}
impl DialogueSources {
    pub fn build(store: &mut RecordStore, limits: Limits) -> Result<Self> {
        let sources = store.source_receipts()?;
        let membership = MembershipIndex::build(store, limits.infos)?;
        let retained_bytes = json_bytes(&(&sources, membership.report()), limits.retained_bytes)?;
        Ok(Self {
            cohort: cohort(&sources)?,
            sources,
            membership,
            retained_bytes,
        })
    }
    /// Canonical-key sorted lookup only. Selection/eligibility remain unverified.
    pub fn topic_infos(&self, topic: &FormKey) -> &[FormKey] {
        self.membership.topic_infos(topic)
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub fn request(
        &self,
        topic: FormKey,
        info: FormKey,
        speaker: Option<FormKey>,
    ) -> Result<Request> {
        if !self.topic_infos(&topic).contains(&info) {
            return Err(Error::Resolution(
                "requested INFO is not a live winning member of topic".into(),
            ));
        }
        Ok(Request {
            topic,
            info,
            speaker,
            source_cohort_sha256: self.cohort.clone(),
        })
    }
    pub fn prepare(
        &self,
        store: &mut RecordStore,
        request: &Request,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<ConversationSources> {
        let current = store.source_receipts()?;
        if request.source_cohort_sha256 != self.cohort || !same_sources(&self.sources, &current) {
            return Err(Error::Resolution(
                "conversation source cohort changed".into(),
            ));
        }
        if !self.topic_infos(&request.topic).contains(&request.info) {
            return Err(Error::Resolution("conversation membership changed".into()));
        }
        let topic_location = live(store, &request.topic, &[*b"DIAL"])?;
        let info_location = live(store, &request.info, &[*b"INFO"])?;
        let parent = store
            .definition(info_location)
            .parent
            .topic
            .map(|raw| store.key_for(info_location, raw))
            .transpose()?
            .flatten();
        if parent.as_ref() != Some(&request.topic) {
            return Err(Error::Resolution(
                "winning INFO parent differs from request topic".into(),
            ));
        }
        let speaker = request
            .speaker
            .as_ref()
            .map(|key| {
                let location = live(store, key, &[*b"ACHR", *b"ACRE", *b"NPC_", *b"CREA"])?;
                identity(store, key, location, None)
            })
            .transpose()?;
        let mut remaining = limits.read_bytes;
        let topic_record = read(store, topic_location, limits, &mut remaining)?;
        let info_record = read(store, info_location, limits, &mut remaining)?;
        let topic = identity(store, &request.topic, topic_location, Some(&topic_record))?;
        let info = identity(store, &request.info, info_location, Some(&info_record))?;
        let payload_bytes = topic_record
            .payload
            .len()
            .checked_add(info_record.payload.len())
            .ok_or_else(|| Error::Unsupported("conversation payload byte overflow".into()))?;
        let metadata_maximum = limits
            .retained_bytes
            .checked_sub(payload_bytes)
            .ok_or_else(|| {
                Error::Unsupported("conversation retained byte budget exceeded".into())
            })?;
        // The condition preparation performs one additional bounded INFO read.
        let reread_cost = (info_record.header.stored_size as usize).max(info_record.payload.len());
        if reread_cost > remaining {
            return Err(Error::Unsupported(
                "conversation read byte budget exceeded".into(),
            ));
        }
        let conditions = condition_operands::prepare_record_with_owners(
            store,
            info_location,
            signatures,
            RecordLimits {
                maximum_decoded_bytes: limits.record_bytes.min(remaining),
                maximum_fields: limits.fields,
                maximum_conditions: limits.conditions,
                maximum_retained_bytes: metadata_maximum,
            },
            OwnerLimits {
                maximum_sections: limits.sections,
                maximum_findings: limits.fields,
                maximum_retained_bytes: metadata_maximum,
            },
        )?;
        if conditions.conditions().identity().decoded_sha256
            != info.decoded_sha256.as_deref().expect("body")
        {
            return Err(Error::Resolution(
                "conversation condition body differs from INFO".into(),
            ));
        }
        let decoder_limits = narrative::Limits {
            max_fields: limits.fields,
            max_sections: limits.sections,
            max_findings: limits.fields,
        };
        let topic_doc = narrative::decode(&topic_record, &topic.source_plugin, decoder_limits)?;
        let info_doc = narrative::decode(&info_record, &info.source_plugin, decoder_limits)?;
        let topic_fields = fields(&topic_doc);
        let info_fields = fields(&info_doc);
        let mut response_fields = index_responses(&info_doc)?;
        let mut responses = Vec::new();
        for (section, owner) in info_doc.sections.iter().enumerate() {
            if owner.kind != SectionKind::Response {
                continue;
            }
            let marker = owner
                .marker_offset
                .ok_or_else(|| Error::Resolution("response lacks source marker".into()))?;
            let data = response_fields[section]
                .marker
                .ok_or_else(|| Error::Resolution("response marker lacks decoded TRDT".into()))?;
            responses.push(ResponseSource {
                section,
                marker_decoded_offset: marker,
                number: data.number,
                sound: link(store, info_location, data.sound_raw_form)?,
                fields: std::mem::take(&mut response_fields[section].fields),
            });
        }
        let mut links = Vec::new();
        for (record, location, document) in [
            (RecordRole::Topic, topic_location, &topic_doc),
            (RecordRole::Info, info_location, &info_doc),
        ] {
            for (index, field) in document.fields.iter().enumerate() {
                if let Value::RawForm(raw) = field.value {
                    links.push(LinkSource {
                        record,
                        field_index: index,
                        target: link(store, location, raw)?,
                    });
                }
            }
        }
        let mut fragments = Vec::new();
        // At most one key per SectionKind (plus None); never rescan all units
        // for each fragment. Discriminants are private temporary grouping keys,
        // not source/save identities or an exported ordering convention.
        let mut role_counts = BTreeMap::new();
        for script in &info_doc.scripts {
            *role_counts
                .entry(script_section_kind(script, &info_doc).map(|kind| kind as usize))
                .or_insert(0usize) += 1;
        }
        for script in &info_doc.scripts {
            let kind = script_section_kind(script, &info_doc);
            let role = match kind {
                Some(SectionKind::BeginScript) => Some(OwnerKind::DialogueBegin),
                Some(SectionKind::EndScript) => Some(OwnerKind::DialogueEnd),
                _ => None,
            };
            fragments.push(FragmentRequest {
                key: ScriptKey {
                    record: request.info.clone(),
                    header_decoded_offset: u32::try_from(script.unit.header.offset)
                        .map_err(|_| Error::Unsupported("script marker offset overflow".into()))?,
                },
                role,
                role_unique: role_counts[&kind.map(|kind| kind as usize)] == 1,
                source: info.clone(),
                metadata_sha256: script_bindings::metadata_digest(&script.unit),
                compiled_sha256: script.unit.compiled.map(|field| digest(field.data)),
                compiled_bytes: script.unit.compiled.map(|field| field.data.len()),
            });
        }
        let metadata = Metadata {
            request: request.clone(),
            topic,
            info,
            speaker,
            topic_fields,
            info_fields,
            responses,
            links,
            fragments,
            topic_sections: topic_doc.sections,
            info_sections: info_doc.sections,
            topic_findings: topic_doc.findings,
            info_findings: info_doc.findings,
            conditions,
            selection_order_verified: false,
            condition_truth_verified: false,
            speaker_assignment_verified: false,
            fragment_timing_verified: false,
            voice_filename_verified: false,
        };
        let retained_bytes = payload_bytes + json_bytes(&metadata, metadata_maximum)?;
        Ok(ConversationSources {
            metadata,
            topic_record,
            info_record,
            retained_bytes,
        })
    }
}

#[derive(Default)]
struct ResponseFields {
    marker: Option<narrative::ResponseData>,
    fields: Vec<usize>,
}
fn index_responses(document: &narrative::Document<'_>) -> Result<Vec<ResponseFields>> {
    // The existing decoder bounds both tables and owns their physical indices.
    // One pass preserves every response-owned field's exact source order; orphan
    // fields remain only in the original field table. No response-number lookup.
    let mut rows: Vec<ResponseFields> = (0..document.sections.len())
        .map(|_| Default::default())
        .collect();
    for (index, field) in document.fields.iter().enumerate() {
        let Some(owner) = field.owner else {
            continue;
        };
        let section = document.sections.get(owner).ok_or_else(|| {
            Error::Resolution("narrative field owner outside section table".into())
        })?;
        if section.kind != SectionKind::Response {
            continue;
        }
        let row = &mut rows[owner];
        row.fields.push(index);
        if section.marker_offset == Some(field.offset)
            && let Value::ResponseData(data) = field.value
        {
            row.marker.get_or_insert(data);
        }
    }
    Ok(rows)
}
fn script_section_kind(
    script: &narrative::Script<'_>,
    document: &narrative::Document<'_>,
) -> Option<SectionKind> {
    script
        .owner
        .and_then(|index| document.sections.get(index))
        .map(|section| section.kind)
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceIdentity {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub record_kind: [u8; 4],
    pub decoded_sha256: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct FieldSource {
    pub kind: [u8; 4],
    pub header_decoded_offset: usize,
    pub owner_section: Option<usize>,
    data_offset: usize,
    bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub raw: u32,
    pub key: Option<FormKey>,
    pub status: &'static str,
    pub target: Option<SourceIdentity>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecordRole {
    Topic,
    Info,
}
#[derive(Debug, Serialize)]
pub struct LinkSource {
    pub record: RecordRole,
    pub field_index: usize,
    pub target: Link,
}
#[derive(Debug, Serialize)]
pub struct ResponseSource {
    pub section: usize,
    pub marker_decoded_offset: usize,
    pub number: u8,
    pub sound: Link,
    /// Authored field indices in the retained INFO, including repeated text.
    pub fields: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub struct FragmentRequest {
    key: ScriptKey,
    role: Option<OwnerKind>,
    role_unique: bool,
    source: SourceIdentity,
    metadata_sha256: String,
    compiled_sha256: Option<String>,
    compiled_bytes: Option<usize>,
}
impl FragmentRequest {
    pub fn key(&self) -> &ScriptKey {
        &self.key
    }
    pub fn role(&self) -> Option<OwnerKind> {
        self.role
    }
    /// Bind to immutable definitions already owned by scripts. Mutable public
    /// catalogue reports never confer source authority; every version field is
    /// checked against the original body and script metadata.
    /// This resolves one source unit. Full runtime cohort admission, condition
    /// eligibility and fragment execution timing require the scripts consumer.
    pub fn resolve<'a>(&self, catalogue: &'a Catalogue) -> Result<&'a LoadedScript> {
        if !self.role_unique || self.role.is_none() {
            return Err(Error::Unsupported(
                "conversation fragment role is unverified or repeated".into(),
            ));
        }
        let script = catalogue
            .get(&self.key)
            .ok_or_else(|| Error::Resolution("conversation fragment not loaded".into()))?;
        let version = script.version();
        if version.source_plugin != self.source.source_plugin
            || version.source_sha256 != self.source.source_sha256
            || version.record_file_offset != self.source.record_file_offset
            || version.record_flags != self.source.record_flags
            || Some(&version.decoded_record_sha256) != self.source.decoded_sha256.as_ref()
            || version.metadata_sha256 != self.metadata_sha256
            || version.compiled_sha256 != self.compiled_sha256
            || version.compiled_bytes != self.compiled_bytes
            || Some(script.owner().kind) != self.role
            || !script.owner().schema_ownership_verified
            || script.owner().section_marker != Some(self.key.header_decoded_offset)
        {
            return Err(Error::Resolution(
                "conversation fragment source version or ownership differs".into(),
            ));
        }
        Ok(script)
    }
}

#[derive(Serialize)]
pub struct Metadata {
    pub request: Request,
    pub topic: SourceIdentity,
    pub info: SourceIdentity,
    pub speaker: Option<SourceIdentity>,
    pub topic_fields: Vec<FieldSource>,
    pub info_fields: Vec<FieldSource>,
    pub responses: Vec<ResponseSource>,
    pub links: Vec<LinkSource>,
    pub fragments: Vec<FragmentRequest>,
    pub topic_sections: Vec<narrative::Section>,
    pub info_sections: Vec<narrative::Section>,
    pub topic_findings: Vec<narrative::Finding>,
    pub info_findings: Vec<narrative::Finding>,
    pub conditions: PreparedOwnerRecord,
    pub selection_order_verified: bool,
    pub condition_truth_verified: bool,
    pub speaker_assignment_verified: bool,
    pub fragment_timing_verified: bool,
    pub voice_filename_verified: bool,
}
pub struct ConversationSources {
    metadata: Metadata,
    topic_record: plugin::Record,
    info_record: plugin::Record,
    retained_bytes: usize,
}
impl ConversationSources {
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub fn topic_bytes(&self, field_index: usize) -> Option<&[u8]> {
        field_bytes(
            &self.topic_record,
            self.metadata.topic_fields.get(field_index)?,
        )
    }
    pub fn info_bytes(&self, field_index: usize) -> Option<&[u8]> {
        field_bytes(
            &self.info_record,
            self.metadata.info_fields.get(field_index)?,
        )
    }
    /// Original NAM1 bytes, including terminators and non-UTF8 bytes. Multiple
    /// NAM1 fields remain distinct; the caller supplies the occurrence explicitly.
    pub fn subtitle_bytes(&self, response: usize, occurrence: usize) -> Option<&[u8]> {
        let field = *self
            .metadata
            .responses
            .get(response)?
            .fields
            .iter()
            .filter(|&&index| self.metadata.info_fields[index].kind == *b"NAM1")
            .nth(occurrence)?;
        self.info_bytes(field)
    }
}

fn fields(document: &narrative::Document<'_>) -> Vec<FieldSource> {
    document
        .fields
        .iter()
        .map(|field| FieldSource {
            kind: field.kind,
            header_decoded_offset: field.offset,
            owner_section: field.owner,
            data_offset: field.offset + 6,
            bytes: field.data.len(),
            sha256: digest(field.data),
        })
        .collect()
}
fn field_bytes<'a>(record: &'a plugin::Record, field: &FieldSource) -> Option<&'a [u8]> {
    record
        .payload
        .get(field.data_offset..field.data_offset.checked_add(field.bytes)?)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn cohort(sources: &[SourceReceipt]) -> Result<String> {
    // Serialization includes source order, spelling, full-file size and digest.
    Ok(digest(
        &serde_json::to_vec(sources).map_err(|error| Error::Resolution(error.to_string()))?,
    ))
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn live(store: &RecordStore, key: &FormKey, kinds: &[[u8; 4]]) -> Result<Location> {
    let location = store
        .winner(key)
        .ok_or_else(|| Error::Resolution("conversation source missing".into()))?;
    let header = &store.definition(location).header;
    if header.flags & plugin::DELETED != 0 || !kinds.contains(&header.kind) {
        return Err(Error::Resolution(
            "conversation source deleted or wrong record kind".into(),
        ));
    }
    Ok(location)
}
fn identity(
    store: &mut RecordStore,
    key: &FormKey,
    location: Location,
    body: Option<&plugin::Record>,
) -> Result<SourceIdentity> {
    let source_sha256 = store.source_digest(location)?;
    let header = &store.definition(location).header;
    Ok(SourceIdentity {
        key: key.clone(),
        source_plugin: store.source_name(location).into(),
        source_sha256,
        record_file_offset: header.offset,
        record_flags: header.flags,
        record_kind: header.kind,
        decoded_sha256: body.map(|record| digest(&record.payload)),
    })
}
fn link(store: &mut RecordStore, owner: Location, raw: u32) -> Result<Link> {
    let key = store.key_for(owner, raw)?;
    let location = key.as_ref().and_then(|key| store.winner(key));
    let status = match (&key, location) {
        (None, _) => "null",
        (_, Some(location)) if store.definition(location).header.flags & plugin::DELETED != 0 => {
            "deleted"
        }
        (_, Some(_)) => "defined",
        (Some(key), None) if content::runtime_binding(key).is_some() => "runtime-dependency",
        _ => "missing",
    };
    let target = location
        .map(|location| {
            identity(
                store,
                key.as_ref().expect("non-null target"),
                location,
                None,
            )
        })
        .transpose()?;
    Ok(Link {
        raw,
        key,
        status,
        target,
    })
}
fn read(
    store: &mut RecordStore,
    location: Location,
    limits: Limits,
    remaining: &mut usize,
) -> Result<plugin::Record> {
    let record = store.read_bounded(location, limits.record_bytes.min(*remaining))?;
    if record.integrity_issue.is_some() {
        return Err(Error::Resolution(
            "conversation source checksum is untrusted".into(),
        ));
    }
    *remaining -= (record.header.stored_size as usize).max(record.payload.len());
    Ok(record)
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "conversation retained byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn json_bytes(value: &impl Serialize, maximum: usize) -> Result<usize> {
    let mut admission = Admission { bytes: 0, maximum };
    serde_json::to_writer(&mut admission, value)
        .map_err(|error| Error::Unsupported(error.to_string()))?;
    Ok(admission.bytes)
}
