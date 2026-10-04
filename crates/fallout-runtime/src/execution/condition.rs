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
    #[error("condition source receipt encoding failed: {0}")]
    SourceEncoding(#[from] serde_json::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("condition query contribution budget exceeded")]
    Capacity,
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
        // This is the existing prepared CTDA receipt domain, distinct from the
        // runtime catalogue fingerprint. Include every ordered source receipt.
        let mut hash = Sha256::new();
        hash.update(b"FNVCTDASOURCES1");
        hash.update(serde_json::to_vec(&world.catalogue().sources)?);
        if record.identity().source_cohort_sha256 != format!("{:x}", hash.finalize()) {
            return Err(Error::ContextChanged);
        }
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
