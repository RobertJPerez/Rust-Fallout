//! Shared host queries. Entry IDs are source facts; original coercion is unresolved.
use crate::{
    World,
    foreign::Content,
    identity::{CampaignId, ReferenceId, ReferenceValue, Value},
    inventory::CountTrace,
};
use fallout_data::identity::FormKey;
use serde::Serialize;
pub const GET_ITEM_COUNT_COMMAND: u16 = 0x102f;
pub const GET_ITEM_COUNT_CONDITION: u16 = 47;
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Native { command_id: u16 },
    Condition { function_id: u16 },
}
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("Query entry is unsupported")]
    UnsupportedEntry,
    #[error("Query needs an explicitly resolved subject")]
    MissingSubject,
    #[error("Query needs exactly one resolved content argument")]
    Arguments,
    #[error("Query belongs to another campaign or source cohort")]
    ContextChanged,
    #[error("Original GetItemCount form-list expansion is unverified")]
    UnverifiedFormList,
    #[error(transparent)]
    Source(#[from] crate::foreign::Failure),
    #[error(transparent)]
    State(#[from] crate::Error),
}
pub type Result<T> = std::result::Result<T, Failure>;
/// Prepared requests retain persistent identities, not transient handles. They
/// can be reevaluated after a same-campaign/cohort restore against current state.
pub struct Request {
    entry: Entry,
    campaign: CampaignId,
    cohort: String,
    subject: ReferenceId,
    item: FormKey,
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub entry: Entry,
    pub query: CountTrace,
    pub original_numeric_return: Option<Value>,
    pub original_behavior_verified: bool,
}
impl Request {
    pub fn prepare(
        world: &World<'_>,
        entry: Entry,
        subject: Option<ReferenceId>,
        arguments: &[Value],
    ) -> Result<Self> {
        match entry {
            Entry::Native {
                command_id: GET_ITEM_COUNT_COMMAND,
            }
            | Entry::Condition {
                function_id: GET_ITEM_COUNT_CONDITION,
            } => {}
            _ => return Err(Failure::UnsupportedEntry),
        }
        let subject = subject.ok_or(Failure::MissingSubject)?;
        world.reference_origin(subject)?;
        let [
            Value::Reference {
                value: ReferenceValue::Content { key },
            },
        ] = arguments
        else {
            return Err(Failure::Arguments);
        };
        crate::identity::valid_form(key)?;
        Ok(Self {
            entry,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            subject,
            item: key.clone(),
        })
    }
    pub fn evaluate(
        &self,
        world: &World<'_>,
        content: &Content,
        maximum_contributions: usize,
    ) -> Result<Trace> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Failure::ContextChanged);
        }
        let form = content.source_form(world, &self.item)?;
        // The actual descriptor accepts inventory objects OR form lists. Returning
        // a simple zero for an unimplemented list would conceal missing behavior.
        if form.kind == *b"FLST" {
            return Err(Failure::UnverifiedFormList);
        }
        Ok(Trace {
            entry: self.entry,
            query: world.inventory_count_trace_bounded(
                self.subject,
                &self.item,
                maximum_contributions,
            )?,
            original_numeric_return: None,
            original_behavior_verified: false,
        })
    }
}
