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
use std::{collections::BTreeMap, io::Write, sync::Arc};

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

/// Lowerable logical page admission; index construction keeps its existing limit.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageLimits {
    pub members: usize,
    /// Conservative member visits, including existing request membership searches.
    pub visited_members: usize,
    /// String bytes plus five fixed logical bytes per copied canonical key.
    pub copied_bytes: usize,
}
impl Default for PageLimits {
    fn default() -> Self {
        Self {
            members: 64,
            visited_members: 16_384,
            copied_bytes: 64 * 1024,
        }
    }
}
impl PageLimits {
    fn validate(self) -> Result<Self> {
        if !(1..=1024).contains(&self.members)
            || !(1..=32_768).contains(&self.visited_members)
            || !(1..=1024 * 1024).contains(&self.copied_bytes)
        {
            return Err(page_error("limits outside supported ceilings"));
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageUsage {
    pub returned_members: usize,
    pub visited_members: usize,
    pub copied_bytes: usize,
}
/// In-process authority only. Its index token never retains the membership graph.
pub struct PageCursor {
    index: Arc<()>,
    cohort: String,
    topic: FormKey,
    speaker: Option<FormKey>,
    next_index: usize,
    last_key: FormKey,
}
/// Canonical structural source members, not original dialogue menu eligibility.
#[derive(Serialize)]
pub struct MembershipPage {
    requests: Vec<Request>,
    usage: PageUsage,
    total_members: usize,
    start_index: usize,
    source_cohort_sha256: String,
    continuation_available: bool,
    canonical_structural_order: bool,
    original_order_verified: bool,
    payloads_prepared: bool,
    #[serde(skip)]
    cursor: Option<PageCursor>,
}
impl MembershipPage {
    pub fn requests(&self) -> &[Request] {
        &self.requests
    }
    pub fn cursor(&self) -> Option<&PageCursor> {
        self.cursor.as_ref()
    }
    pub fn usage(&self) -> PageUsage {
        self.usage
    }
    pub fn total_members(&self) -> usize {
        self.total_members
    }
    pub fn start_index(&self) -> usize {
        self.start_index
    }
}
fn page_error(message: &str) -> Error {
    Error::Unsupported(format!("conversation membership page {message}"))
}
fn key_copy_bytes(key: &FormKey) -> Result<usize> {
    key.origin_plugin
        .len()
        .checked_add(5)
        .ok_or_else(|| page_error("key byte overflow"))
}
fn page_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| page_error("admission overflow"))
}

pub struct DialogueSources {
    index_token: Arc<()>,
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
            index_token: Arc::new(()),
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
    /// Continuation is bound to this exact index and caller-declared topic/speaker.
    /// No source bodies are read; prepare validates current sources separately.
    pub fn page(
        &self,
        topic: &FormKey,
        speaker: Option<&FormKey>,
        cursor: Option<&PageCursor>,
        limits: PageLimits,
    ) -> Result<MembershipPage> {
        let limits = limits.validate()?;
        let members = self.topic_infos(topic);
        let start = if let Some(cursor) = cursor {
            if !Arc::ptr_eq(&cursor.index, &self.index_token)
                || cursor.cohort != self.cohort
                || &cursor.topic != topic
                || cursor.speaker.as_ref() != speaker
                || cursor.next_index == 0
                || cursor.next_index >= members.len()
                || members.get(cursor.next_index - 1) != Some(&cursor.last_key)
                || members[cursor.next_index] <= cursor.last_key
            {
                return Err(page_error(
                    "cursor does not match index/topic/speaker/progress",
                ));
            }
            cursor.next_index
        } else {
            0
        };
        let count = limits.members.min(members.len() - start);
        let end = page_add(start, count)?;
        // binary_search performs at most bit_length(len) comparisons plus one;
        // reserve that bound before any request copies rather than hiding scans.
        let search_bound = (usize::BITS - members.len().leading_zeros()) as usize + 1;
        let visits = count
            .checked_mul(page_add(search_bound, 2)?)
            .and_then(|n| n.checked_add(2 * usize::from(cursor.is_some())))
            .ok_or_else(|| page_error("visit overflow"))?;
        if visits > limits.visited_members {
            return Err(page_error("visited member budget exceeded"));
        }
        let topic_bytes = key_copy_bytes(topic)?;
        let speaker_bytes = speaker.map(key_copy_bytes).transpose()?.unwrap_or(0);
        let envelope = page_add(page_add(topic_bytes, speaker_bytes)?, self.cohort.len())?;
        let mut copied = self.cohort.len(); // the page's source identity string
        for info in &members[start..end] {
            copied = page_add(copied, page_add(envelope, key_copy_bytes(info)?)?)?;
        }
        let has_next = end < members.len();
        if has_next {
            copied = page_add(
                copied,
                page_add(envelope, key_copy_bytes(&members[end - 1])?)?,
            )?;
        }
        if copied > limits.copied_bytes {
            return Err(page_error("copied key/string byte budget exceeded"));
        }
        let mut requests = Vec::with_capacity(count);
        for info in &members[start..end] {
            requests.push(self.request(topic.clone(), info.clone(), speaker.cloned())?);
        }
        let next = has_next.then(|| PageCursor {
            index: Arc::clone(&self.index_token),
            cohort: self.cohort.clone(),
            topic: topic.clone(),
            speaker: speaker.cloned(),
            next_index: end,
            last_key: members[end - 1].clone(),
        });
        Ok(MembershipPage {
            requests,
            usage: PageUsage {
                returned_members: count,
                visited_members: visits,
                copied_bytes: copied,
            },
            total_members: members.len(),
            start_index: start,
            source_cohort_sha256: self.cohort.clone(),
            continuation_available: has_next,
            canonical_structural_order: true,
            original_order_verified: false,
            payloads_prepared: false,
            cursor: next,
        })
    }
    pub fn request(
        &self,
        topic: FormKey,
        info: FormKey,
        speaker: Option<FormKey>,
    ) -> Result<Request> {
        if self.topic_infos(&topic).binary_search(&info).is_err() {
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
    pub fn prepare_batch(
        &self,
        store: &mut RecordStore,
        requests: &[Request],
        signatures: &Signatures,
        limits: BatchLimits,
    ) -> Result<ConversationBatch> {
        prepare_batch(self, store, requests, signatures, limits)
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

/// Aggregate source admission, separate from membership and loaded catalogue budgets.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BatchLimits {
    pub requests: usize,
    pub record_bytes: usize,
    /// Preflight reads plus the existing typed preparer's topic/INFO/condition reads.
    pub read_bytes: usize,
    /// Topic and INFO decoded payload copies, conservatively repeated per request.
    pub raw_bytes: usize,
    pub fields: usize,
    /// Reserve a possible section per field, including duplicated INFO owner sections.
    pub section_slots: usize,
    pub conditions: usize,
    pub fragments: usize,
    /// Conservative copied source/envelope metadata reservation, not allocator use.
    pub source_metadata_bytes: usize,
    /// Sum of each existing closure's raw payloads and exact compact metadata.
    pub retained_bytes: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            requests: 8,
            record_bytes: 8 * 1024 * 1024,
            read_bytes: 128 * 1024 * 1024,
            raw_bytes: 64 * 1024 * 1024,
            fields: 8 * 2 * 65536,
            section_slots: 8 * (3 + 3 * 65536),
            conditions: 8 * 8192,
            fragments: 8 * 8192,
            source_metadata_bytes: 128 * 1024 * 1024,
            retained_bytes: 256 * 1024 * 1024,
        }
    }
}
impl BatchLimits {
    fn validate(self) -> Result<Self> {
        let ceiling = Self::default();
        for (value, max) in [
            (self.requests, ceiling.requests),
            (self.record_bytes, ceiling.record_bytes),
            (self.read_bytes, ceiling.read_bytes),
            (self.raw_bytes, ceiling.raw_bytes),
            (self.fields, ceiling.fields),
            (self.section_slots, ceiling.section_slots),
            (self.conditions, ceiling.conditions),
            (self.fragments, ceiling.fragments),
            (self.source_metadata_bytes, ceiling.source_metadata_bytes),
            (self.retained_bytes, ceiling.retained_bytes),
        ] {
            if value > max {
                return Err(batch_error("limit exceeds ceiling"));
            }
        }
        Ok(self)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct BatchUsage {
    pub requests: usize,
    pub maximum_record_bytes: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub fields: usize,
    pub section_slots: usize,
    pub conditions: usize,
    pub fragments: usize,
    pub source_metadata_bytes: usize,
    pub retained_bytes: usize,
}
#[derive(Serialize)]
pub struct BatchReceipt {
    pub schema_version: u32,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub requests: Vec<Request>,
    pub conversation_identities: Vec<String>,
    pub usage: BatchUsage,
    pub limits: BatchLimits,
    pub selection_order_verified: bool,
    pub condition_truth_verified: bool,
    pub speaker_assignment_verified: bool,
    pub fragment_timing_verified: bool,
    pub voice_filename_verified: bool,
}
/// Only the sealed source factory constructs this all-or-none immutable result.
pub struct ConversationBatch {
    receipt: BatchReceipt,
    conversations: Vec<ConversationSources>,
}
impl ConversationBatch {
    pub fn receipt(&self) -> &BatchReceipt {
        &self.receipt
    }
    pub fn conversations(&self) -> &[ConversationSources] {
        &self.conversations
    }
    pub fn identity(&self) -> &str {
        &self.receipt.identity
    }
    pub fn validate_sources(&self, store: &mut RecordStore) -> Result<()> {
        if !same_sources(&self.receipt.sources, &store.source_receipts()?) {
            return Err(batch_error("ordered source names/count/bytes changed"));
        }
        Ok(())
    }
}
impl Serialize for ConversationBatch {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut result = serializer.serialize_struct("ConversationBatch", 2)?;
        result.serialize_field("receipt", &self.receipt)?;
        result.serialize_field("conversations", &BatchMetadata(&self.conversations))?;
        result.end()
    }
}
struct BatchMetadata<'a>(&'a [ConversationSources]);
impl Serialize for BatchMetadata<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for source in self.0 {
            seq.serialize_element(source.metadata())?;
        }
        seq.end()
    }
}
struct BatchPlan {
    typed_read_bytes: usize,
    fields: usize,
    conditions: usize,
    fragments: usize,
}
fn prepare_batch(
    dialogue: &DialogueSources,
    store: &mut RecordStore,
    requests: &[Request],
    signatures: &Signatures,
    limits: BatchLimits,
) -> Result<ConversationBatch> {
    let limits = limits.validate()?;
    if requests.is_empty() {
        return Err(batch_error("explicit request batch is empty"));
    }
    let mut usage = BatchUsage::default();
    batch_charge(
        &mut usage.requests,
        requests.len(),
        limits.requests,
        "requests",
    )?;
    if !same_sources(&dialogue.sources, &store.source_receipts()?) {
        return Err(batch_error("ordered source names/count/bytes changed"));
    }
    // Admit copies before retaining the private request set and final envelope.
    let envelope = json_bytes(&(&dialogue.sources, requests), limits.source_metadata_bytes)?;
    batch_charge(
        &mut usage.source_metadata_bytes,
        batch_mul(envelope, 4)?,
        limits.source_metadata_bytes,
        "source metadata",
    )?;
    let mut unique = std::collections::BTreeSet::new();
    for request in requests {
        batch_charge(
            &mut usage.source_metadata_bytes,
            8192,
            limits.source_metadata_bytes,
            "source metadata",
        )?;
        if request.source_cohort_sha256 != dialogue.cohort
            || !dialogue.topic_infos(&request.topic).contains(&request.info)
        {
            return Err(batch_error("request cohort or live membership changed"));
        }
        if !unique.insert((
            request.topic.clone(),
            request.info.clone(),
            request.speaker.clone(),
        )) {
            return Err(batch_error("duplicate explicit topic/INFO/speaker request"));
        }
        live(store, &request.topic, &[*b"DIAL"])?;
        let info = live(store, &request.info, &[*b"INFO"])?;
        let parent = store
            .definition(info)
            .parent
            .topic
            .map(|raw| store.key_for(info, raw))
            .transpose()?
            .flatten();
        if parent.as_ref() != Some(&request.topic) {
            return Err(batch_error("winning INFO physical parent differs"));
        }
        if let Some(speaker) = &request.speaker {
            live(store, speaker, &[*b"ACHR", *b"ACRE", *b"NPC_", *b"CREA"])?;
        }
    }
    let mut plans = Vec::with_capacity(requests.len());
    for request in requests {
        let mut counts = [0_usize; 2];
        let mut conditions = 0;
        let mut fragments = 0;
        let mut typed_read_bytes = 0_usize;
        for (is_info, key) in [(false, &request.topic), (true, &request.info)] {
            let kinds = if is_info { [*b"INFO"] } else { [*b"DIAL"] };
            let location = live(store, key, &kinds)?;
            let mut remaining = limits.read_bytes - usage.read_bytes;
            let record = read(
                store,
                location,
                Limits {
                    read_bytes: remaining,
                    record_bytes: limits.record_bytes,
                    ..Default::default()
                },
                &mut remaining,
            )?;
            let cost = (record.header.stored_size as usize).max(record.payload.len());
            usage.maximum_record_bytes = usage.maximum_record_bytes.max(cost);
            // One preflight read, one typed topic read or two typed INFO reads.
            let future_reads = if is_info { 2 } else { 1 };
            batch_charge(
                &mut usage.read_bytes,
                batch_mul(cost, 1 + future_reads)?,
                limits.read_bytes,
                "read bytes",
            )?;
            typed_read_bytes = typed_read_bytes
                .checked_add(batch_mul(cost, future_reads)?)
                .ok_or_else(|| batch_error("read cost overflow"))?;
            batch_charge(
                &mut usage.raw_bytes,
                record.payload.len(),
                limits.raw_bytes,
                "raw bytes",
            )?;
            batch_charge(
                &mut usage.source_metadata_bytes,
                batch_mul(record.payload.len(), 4)?,
                limits.source_metadata_bytes,
                "source metadata",
            )?;
            plugin::visit_subrecords(&record, store.source_name(location), |field| {
                batch_charge(&mut usage.fields, 1, limits.fields, "fields")?;
                counts[usize::from(is_info)] += 1;
                // A conservative slot per source field requires no second ownership parser.
                batch_charge(
                    &mut usage.section_slots,
                    if is_info { 2 } else { 1 },
                    limits.section_slots,
                    "section slots",
                )?;
                batch_charge(
                    &mut usage.source_metadata_bytes,
                    3072,
                    limits.source_metadata_bytes,
                    "source metadata",
                )?;
                if is_info && field.kind == *b"CTDA" {
                    batch_charge(&mut usage.conditions, 1, limits.conditions, "conditions")?;
                    conditions += 1;
                }
                if is_info && field.kind == *b"SCHR" {
                    batch_charge(&mut usage.fragments, 1, limits.fragments, "fragments")?;
                    fragments += 1;
                }
                Ok(())
            })?;
        }
        batch_charge(
            &mut usage.section_slots,
            3,
            limits.section_slots,
            "section slots",
        )?;
        plans.push(BatchPlan {
            typed_read_bytes,
            fields: counts[0].max(counts[1]),
            conditions,
            fragments,
        });
    }
    // All raw counts/source reservations were admitted before any typed closure.
    let mut conversations = Vec::with_capacity(requests.len());
    let mut identities = Vec::with_capacity(requests.len());
    for (request, plan) in requests.iter().zip(plans) {
        let source = dialogue.prepare(
            store,
            request,
            signatures,
            Limits {
                read_bytes: plan.typed_read_bytes,
                record_bytes: limits.record_bytes,
                fields: plan.fields.min(Limits::default().fields),
                conditions: plan.conditions.min(Limits::default().conditions),
                retained_bytes: (limits.retained_bytes - usage.retained_bytes)
                    .min(Limits::default().retained_bytes),
                ..Default::default()
            },
        )?;
        let metadata = source.metadata();
        if metadata.conditions.conditions().sites().len() != plan.conditions
            || metadata.fragments.len() != plan.fragments
        {
            return Err(batch_error(
                "preflight condition or fragment identity differs",
            ));
        }
        if metadata
            .fragments
            .iter()
            .any(|fragment| !fragment.role_unique || fragment.role.is_none())
        {
            return Err(batch_error("fragment role is unverified or repeated"));
        }
        batch_charge(
            &mut usage.retained_bytes,
            source.retained_bytes(),
            limits.retained_bytes,
            "retained bytes",
        )?;
        identities.push(batch_digest(b"nv-conversation-closure-v1\0", metadata)?);
        conversations.push(source);
    }
    let sources = store.source_receipts()?;
    if !same_sources(&dialogue.sources, &sources) {
        return Err(batch_error(
            "ordered source bytes changed before publication",
        ));
    }
    let identity = batch_digest(
        b"nv-conversation-source-batch-v1\0",
        &(&dialogue.cohort, requests, &identities),
    )?;
    Ok(ConversationBatch {
        receipt: BatchReceipt {
            schema_version: 1,
            identity,
            source_cohort_sha256: dialogue.cohort.clone(),
            sources,
            requests: requests.to_vec(),
            conversation_identities: identities,
            usage,
            limits,
            selection_order_verified: false,
            condition_truth_verified: false,
            speaker_assignment_verified: false,
            fragment_timing_verified: false,
            voice_filename_verified: false,
        },
        conversations,
    })
}
fn batch_error(reason: &str) -> Error {
    Error::Unsupported(format!("conversation batch: {reason}"))
}
fn batch_charge(used: &mut usize, add: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(add)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| batch_error(&format!("{name} allowance exceeded")))?;
    Ok(())
}
fn batch_mul(first: usize, second: usize) -> Result<usize> {
    first
        .checked_mul(second)
        .ok_or_else(|| batch_error("source reservation overflow"))
}
struct BatchHash {
    hash: Sha256,
    bytes: usize,
}
impl Write for BatchHash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= 256 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("conversation batch identity ceiling"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn batch_digest(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut writer = BatchHash {
        hash: Sha256::new(),
        bytes: 0,
    };
    writer.hash.update(domain);
    serde_json::to_writer(&mut writer, value).map_err(|error| batch_error(&error.to_string()))?;
    Ok(format!("{:x}", writer.hash.finalize()))
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
