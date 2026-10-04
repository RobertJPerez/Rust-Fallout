//! Source-bound condition host observations; no condition truth or retail rules.
use crate::{
    World,
    foreign::Content,
    identity::{CampaignId, ReferenceId, ReferenceValue, Value},
    query,
};
use fallout_data::condition_operands::{
    ConditionSite, Domain, FormStatus, PreparedRecord, RecordIdentity, SignatureStatus, Subject,
    Value as SourceValue,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Faithful,
    EngineeringObservation,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("condition request belongs to another campaign or source cohort")]
    ContextChanged,
    #[error("condition site at decoded field offset {0} is absent")]
    MissingSite(usize),
    #[error("condition source record index {0} is absent")]
    MissingRecord(usize),
    #[error("condition source record selection contains a duplicate identity")]
    DuplicateRecord,
    #[error("condition source receipt encoding failed: {0}")]
    SourceEncoding(#[from] serde_json::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("condition query contribution budget exceeded")]
    Capacity,
    #[error("condition batch budget exceeded: {0}")]
    BatchCapacity(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    MissingImplementation,
    UnverifiedRetailSemantics,
    SourceFinding,
    Signature,
    SubjectSelection,
    MissingSubject,
    Argument,
    HostQueryUnavailable,
    UnverifiedFormList,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Unsupported { reason: Unsupported, detail: String },
    EngineeringObservation { trace: Box<query::Trace> },
}
fn unsupported(reason: Unsupported, detail: impl ToString) -> Outcome {
    Outcome::Unsupported {
        reason,
        detail: detail.to_string(),
    }
}

/// Borrows privately admitted source bytes; contains no mutable world or values.
/// Requests may be observed again against a same-campaign canonical restore.
pub struct Request<'a> {
    record: &'a PreparedRecord,
    site: &'a ConditionSite,
    campaign: CampaignId,
    cohort: String,
}

#[derive(Debug, Clone, Copy)]
pub struct BatchLimits {
    pub maximum_requests: usize,
    pub maximum_source_receipt_bytes: usize,
    pub maximum_site_comparisons: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            maximum_requests: 65_536,
            maximum_source_receipt_bytes: 1024 * 1024,
            maximum_site_comparisons: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BatchCounts {
    pub cohort_validations: usize,
    /// Compact ordered receipt JSON bytes; the fixed hash domain is excluded.
    pub source_receipt_bytes: usize,
    pub record_sites: usize,
    pub requests: usize,
    pub site_comparisons: usize,
}

/// A complete ordered selection over privately admitted immutable sites. This
/// is a read-only request batch, never condition truth or mutation authority.
pub struct Requests<'a> {
    requests: Vec<Request<'a>>,
    counts: BatchCounts,
    campaign: CampaignId,
    cohort: String,
}

struct ReceiptHash {
    hash: Sha256,
    bytes: usize,
    maximum: usize,
    exceeded: bool,
}
impl Write for ReceiptHash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "condition source receipt byte budget exceeded",
            ));
        }
        self.hash.update(bytes);
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn source_cohort(world: &World<'_>, maximum: usize) -> Result<(String, usize), Error> {
    // Same existing CTDA receipt domain and ordered complete source list. Stream
    // once into the digest, bounding work before an owned serialization exists.
    let mut writer = ReceiptHash {
        hash: Sha256::new(),
        bytes: 0,
        maximum,
        exceeded: false,
    };
    writer.hash.update(b"FNVCTDASOURCES1");
    if let Err(error) = serde_json::to_writer(&mut writer, &world.catalogue().sources) {
        return Err(if writer.exceeded {
            Error::BatchCapacity("source receipt bytes")
        } else {
            Error::SourceEncoding(error)
        });
    }
    Ok((format!("{:x}", writer.hash.finalize()), writer.bytes))
}

fn validate_record_source(
    world: &World<'_>,
    record: &PreparedRecord,
    maximum: usize,
) -> Result<usize, Error> {
    let (digest, bytes) = source_cohort(world, maximum)?;
    if record.identity().source_cohort_sha256 != digest {
        return Err(Error::ContextChanged);
    }
    Ok(bytes)
}

fn find_site<'a>(
    record: &'a PreparedRecord,
    offset: usize,
    comparisons: &mut usize,
    maximum: usize,
) -> Result<&'a ConditionSite, Error> {
    // Strict source preparation supplies the ordered vector. Share this exact
    // physical lookup between legacy and cross-record batches, without an index.
    let mut lower = 0;
    let mut upper = record.sites().len();
    while lower < upper {
        if *comparisons >= maximum {
            return Err(Error::BatchCapacity("site comparisons"));
        }
        *comparisons += 1;
        let middle = lower + (upper - lower) / 2;
        let site = &record.sites()[middle];
        match site.field_decoded_offset().cmp(&offset) {
            std::cmp::Ordering::Less => lower = middle + 1,
            std::cmp::Ordering::Greater => upper = middle,
            std::cmp::Ordering::Equal => return Ok(site),
        }
    }
    Err(Error::MissingSite(offset))
}

impl<'a> Requests<'a> {
    pub fn prepare(
        world: &World<'_>,
        record: &'a PreparedRecord,
        requested_offsets: &[usize],
        limits: BatchLimits,
    ) -> Result<Self, Error> {
        if requested_offsets.len() > limits.maximum_requests {
            return Err(Error::BatchCapacity("requests"));
        }
        let receipt_bytes =
            validate_record_source(world, record, limits.maximum_source_receipt_bytes)?;
        let mut counts = BatchCounts {
            cohort_validations: 1,
            source_receipt_bytes: receipt_bytes,
            record_sites: record.sites().len(),
            requests: requested_offsets.len(),
            site_comparisons: 0,
        };
        let mut requests = Vec::with_capacity(requested_offsets.len());
        for &offset in requested_offsets {
            let site = find_site(
                record,
                offset,
                &mut counts.site_comparisons,
                limits.maximum_site_comparisons,
            )?;
            requests.push(Request {
                record,
                site,
                campaign: world.campaign(),
                cohort: world.catalogue_fingerprint().into(),
            });
        }
        Ok(Self {
            requests,
            counts,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
        })
    }
    pub fn counts(&self) -> BatchCounts {
        self.counts
    }
    pub fn len(&self) -> usize {
        self.requests.len()
    }
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Reuse individual request observations with one contribution allowance.
    /// An error drops earlier read-only observations and returns no partial batch.
    pub fn observe<'b>(
        &'b self,
        world: &World<'_>,
        content: &Content,
        explicit_subject: Option<ReferenceId>,
        intent: Intent,
        maximum_contributions: usize,
    ) -> Result<Vec<Observation<'b>>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        content.validate_world(world)?;
        let mut used = 0_usize;
        let mut observations = Vec::with_capacity(self.requests.len());
        for request in &self.requests {
            let observation = request.observe(
                world,
                content,
                explicit_subject,
                intent,
                maximum_contributions.saturating_sub(used),
            )?;
            if let Outcome::EngineeringObservation { trace } = &observation.outcome {
                used = used
                    .checked_add(trace.query.contributions.len())
                    .filter(|&used| used <= maximum_contributions)
                    .ok_or(Error::Capacity)?;
            }
            observations.push(observation);
        }
        Ok(observations)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RecordSelection {
    pub record_index: usize,
    pub field_decoded_offset: usize,
    pub explicit_subject: Option<ReferenceId>,
}
#[derive(Debug, Clone, Copy)]
pub struct CrossLimits {
    pub maximum_records: usize,
    pub maximum_source_bytes: usize,
    pub maximum_record_fields: usize,
    pub maximum_record_sites: usize,
    pub maximum_retained_source_bytes: usize,
    pub maximum_requests: usize,
    pub maximum_source_receipt_bytes: usize,
    pub maximum_receipt_comparisons: usize,
    pub maximum_site_comparisons: usize,
    pub maximum_query_variable_bytes: usize,
}
impl Default for CrossLimits {
    fn default() -> Self {
        Self {
            maximum_records: 64,
            maximum_source_bytes: 8 * 1024 * 1024,
            maximum_record_fields: 262_144,
            maximum_record_sites: 65_536,
            maximum_retained_source_bytes: 16 * 1024 * 1024,
            maximum_requests: 4096,
            maximum_source_receipt_bytes: 1024 * 1024,
            maximum_receipt_comparisons: 262_144,
            maximum_site_comparisons: 1_048_576,
            maximum_query_variable_bytes: 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct CrossObservationLimits {
    pub maximum_contributions: usize,
    /// Entire compact ordered observation array, including brackets/separators.
    pub maximum_observation_bytes: usize,
}
impl Default for CrossObservationLimits {
    fn default() -> Self {
        Self {
            maximum_contributions: 65_536,
            maximum_observation_bytes: 8 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CrossCounts {
    pub cohort_validations: usize,
    pub source_receipt_bytes: usize,
    pub records: usize,
    pub source_bytes: usize,
    pub record_fields: usize,
    pub record_sites: usize,
    pub retained_source_bytes: usize,
    pub receipt_comparisons: usize,
    pub requests: usize,
    pub site_comparisons: usize,
    pub query_variable_bytes: usize,
}
struct SelectedRequest<'a> {
    request: Request<'a>,
    subject: Option<ReferenceId>,
}
/// Borrowed immutable source authority with explicit per-site subjects. Every
/// record is privately source-admitted; diagnostic JSON cannot construct this.
pub struct CrossRequests<'a> {
    requests: Vec<SelectedRequest<'a>>,
    counts: CrossCounts,
    campaign: CampaignId,
    cohort: String,
}

fn charge(
    used: &mut usize,
    additional: usize,
    maximum: usize,
    reason: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(additional)
        .filter(|&total| total <= maximum)
        .ok_or(Error::BatchCapacity(reason))?;
    Ok(())
}
struct ObservationAdmission {
    bytes: usize,
    maximum: usize,
    exceeded: bool,
}
impl Write for ObservationAdmission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "condition observation byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> CrossRequests<'a> {
    pub fn prepare(
        world: &World<'_>,
        records: &[&'a PreparedRecord],
        selections: &[RecordSelection],
        limits: CrossLimits,
    ) -> Result<Self, Error> {
        if records.len() > limits.maximum_records {
            return Err(Error::BatchCapacity("records"));
        }
        if selections.len() > limits.maximum_requests {
            return Err(Error::BatchCapacity("requests"));
        }
        let query_variable_bytes = selections
            .len()
            .checked_add(1)
            .and_then(|count| count.checked_mul(world.catalogue_fingerprint().len()))
            .filter(|&bytes| bytes <= limits.maximum_query_variable_bytes)
            .ok_or(Error::BatchCapacity("query variable bytes"))?;
        let (digest, source_receipt_bytes) =
            source_cohort(world, limits.maximum_source_receipt_bytes)?;
        let mut counts = CrossCounts {
            cohort_validations: 1,
            source_receipt_bytes,
            records: records.len(),
            source_bytes: 0,
            record_fields: 0,
            record_sites: 0,
            retained_source_bytes: 0,
            receipt_comparisons: 0,
            requests: selections.len(),
            site_comparisons: 0,
            query_variable_bytes,
        };
        let mut unique = BTreeSet::new();
        for &record in records {
            let identity = record.identity();
            if !unique.insert(&identity.key) {
                return Err(Error::DuplicateRecord);
            }
            if identity.source_cohort_sha256 != digest {
                return Err(Error::ContextChanged);
            }
            charge(
                &mut counts.source_bytes,
                identity.decoded_bytes,
                limits.maximum_source_bytes,
                "source bytes",
            )?;
            charge(
                &mut counts.record_fields,
                record.fields(),
                limits.maximum_record_fields,
                "record fields",
            )?;
            charge(
                &mut counts.record_sites,
                record.sites().len(),
                limits.maximum_record_sites,
                "record sites",
            )?;
            charge(
                &mut counts.retained_source_bytes,
                record.retained_bytes(),
                limits.maximum_retained_source_bytes,
                "retained source bytes",
            )?;
            // The private producer already verified the exact winning header,
            // decoded hash and site bytes. Check its own receipt in the shared
            // complete current cohort as well, bounding every comparison.
            let mut found = false;
            for receipt in &world.catalogue().sources {
                charge(
                    &mut counts.receipt_comparisons,
                    1,
                    limits.maximum_receipt_comparisons,
                    "receipt comparisons",
                )?;
                if receipt.source_name == identity.source_name {
                    found = receipt.source_sha256 == identity.source_sha256;
                    break;
                }
            }
            if !found {
                return Err(Error::ContextChanged);
            }
        }
        let mut requests = Vec::with_capacity(selections.len());
        for selection in selections {
            let record = *records
                .get(selection.record_index)
                .ok_or(Error::MissingRecord(selection.record_index))?;
            let site = find_site(
                record,
                selection.field_decoded_offset,
                &mut counts.site_comparisons,
                limits.maximum_site_comparisons,
            )?;
            requests.push(SelectedRequest {
                request: Request {
                    record,
                    site,
                    campaign: world.campaign(),
                    cohort: world.catalogue_fingerprint().into(),
                },
                subject: selection.explicit_subject,
            });
        }
        Ok(Self {
            requests,
            counts,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
        })
    }
    pub fn counts(&self) -> CrossCounts {
        self.counts
    }
    pub fn len(&self) -> usize {
        self.requests.len()
    }
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
    pub fn observe<'b>(
        &'b self,
        world: &World<'_>,
        content: &Content,
        intent: Intent,
        limits: CrossObservationLimits,
    ) -> Result<Vec<Observation<'b>>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        content.validate_world(world)?;
        let mut contributions = 0;
        let mut retained = 0;
        charge(
            &mut retained,
            2,
            limits.maximum_observation_bytes,
            "observation bytes",
        )?;
        let mut observations = Vec::with_capacity(self.requests.len());
        for selected in &self.requests {
            let observation = selected.request.observe(
                world,
                content,
                selected.subject,
                intent,
                limits.maximum_contributions.saturating_sub(contributions),
            )?;
            if let Outcome::EngineeringObservation { trace } = &observation.outcome {
                contributions = contributions
                    .checked_add(trace.query.contributions.len())
                    .filter(|&count| count <= limits.maximum_contributions)
                    .ok_or(Error::Capacity)?;
            }
            charge(
                &mut retained,
                usize::from(!observations.is_empty()),
                limits.maximum_observation_bytes,
                "observation bytes",
            )?;
            let mut admission = ObservationAdmission {
                bytes: 0,
                maximum: limits.maximum_observation_bytes - retained,
                exceeded: false,
            };
            if let Err(error) = serde_json::to_writer(&mut admission, &observation) {
                return Err(if admission.exceeded {
                    Error::BatchCapacity("observation bytes")
                } else {
                    Error::SourceEncoding(error)
                });
            }
            retained += admission.bytes;
            observations.push(observation);
        }
        Ok(observations)
    }
}

#[derive(Debug, Serialize)]
pub struct Observation<'a> {
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub source: &'a RecordIdentity,
    pub site: &'a ConditionSite,
    pub explicit_subject: Option<ReferenceId>,
    pub intent: Intent,
    pub outcome: Outcome,
    pub condition_truth: Option<bool>,
    pub condition_evaluation_ready: bool,
    pub original_behavior_verified: bool,
}

impl<'a> Request<'a> {
    pub fn prepare(
        world: &World<'_>,
        record: &'a PreparedRecord,
        field_decoded_offset: usize,
    ) -> Result<Self, Error> {
        validate_record_source(world, record, usize::MAX)?;
        let site = record
            .sites()
            .iter()
            .find(|site| site.field_decoded_offset() == field_decoded_offset)
            .ok_or(Error::MissingSite(field_decoded_offset))?;
        Ok(Self {
            record,
            site,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
        })
    }

    pub fn observe<'b>(
        &'b self,
        world: &World<'_>,
        content: &Content,
        explicit_subject: Option<ReferenceId>,
        intent: Intent,
        maximum_contributions: usize,
    ) -> Result<Observation<'b>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        content.validate_world(world)?;
        Ok(Observation {
            campaign: self.campaign,
            source_cohort_sha256: &self.cohort,
            state_revision: world.revision(),
            source: self.record.identity(),
            site: self.site,
            explicit_subject,
            intent,
            outcome: self.dispatch(
                world,
                content,
                explicit_subject,
                intent,
                maximum_contributions,
            )?,
            condition_truth: None,
            condition_evaluation_ready: false,
            original_behavior_verified: false,
        })
    }

    fn dispatch(
        &self,
        world: &World<'_>,
        content: &Content,
        explicit_subject: Option<ReferenceId>,
        intent: Intent,
        maximum_contributions: usize,
    ) -> Result<Outcome, Error> {
        if self.site.condition().function_id != query::GET_ITEM_COUNT_CONDITION {
            return Ok(unsupported(
                Unsupported::MissingImplementation,
                "Condition function has no replacement query adapter",
            ));
        }
        if intent == Intent::Faithful {
            return Ok(unsupported(
                Unsupported::UnverifiedRetailSemantics,
                "Original condition return, comparison, subject and grouping rules are unverified",
            ));
        }
        if !self.site.source_findings().is_empty() {
            return Ok(unsupported(
                Unsupported::SourceFinding,
                "Unverified source flags, padding, threshold or selector",
            ));
        }
        let binding = self.site.binding();
        let [argument, unused] = &binding.operands;
        if binding.signature_status != SignatureStatus::DescriptorBound
            || binding.signature_parameter_count != Some(1)
            || argument.parameter_type_id != Some(50)
            || argument.optional_word != Some(0)
            || !matches!(unused.value, SourceValue::Unused { raw_word: 0 })
        {
            return Ok(unsupported(
                Unsupported::Signature,
                "Engineering observation requires the bound single type-50 condition parameter",
            ));
        }
        if binding.subject != Subject::Subject || binding.legacy_target_flag_present {
            return Ok(unsupported(
                Unsupported::SubjectSelection,
                "Only an authored Subject selector with an explicit host subject is admitted",
            ));
        }
        if explicit_subject.is_none() {
            return Ok(unsupported(
                Unsupported::MissingSubject,
                "Condition query requires an explicit host subject",
            ));
        }
        if !matches!(
            argument.value,
            SourceValue::FormId {
                domain: Domain::InventoryObject,
                ..
            }
        ) {
            return Ok(unsupported(
                Unsupported::Argument,
                "Condition argument is not a bound inventory-object FormID",
            ));
        }
        let Some(dependency) = &argument.form_dependency else {
            return Ok(unsupported(
                Unsupported::Argument,
                "Condition form dependency is absent",
            ));
        };
        let (FormStatus::Defined, Some(key)) = (dependency.status, &dependency.key) else {
            return Ok(unsupported(
                Unsupported::Argument,
                "Condition argument must name a defined content form",
            ));
        };
        let argument = Value::Reference {
            value: ReferenceValue::Content { key: key.clone() },
        };
        let query = query::Request::prepare(
            world,
            query::Entry::Condition {
                function_id: query::GET_ITEM_COUNT_CONDITION,
            },
            explicit_subject,
            &[argument],
        )
        .and_then(|request| request.evaluate(world, content, maximum_contributions));
        Ok(match query {
            Ok(trace) => Outcome::EngineeringObservation {
                trace: Box::new(trace),
            },
            Err(query::Failure::UnverifiedFormList) => unsupported(
                Unsupported::UnverifiedFormList,
                "Original condition form-list expansion is unverified",
            ),
            Err(query::Failure::State(crate::Error::Capacity(_))) => return Err(Error::Capacity),
            Err(error) => unsupported(Unsupported::HostQueryUnavailable, error),
        })
    }
}
