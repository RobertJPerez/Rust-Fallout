use crate::{Error, Result};
use fallout_data::identity::{FormKey, ProfileId, plugin_name};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// Opaque persistent campaign namespace. Reference/instance counters are local
/// to this identity, while transient world epochs change after restoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CampaignId([u8; 16]);
impl CampaignId {
    pub fn generate() -> Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes)
            .map_err(|error| Error::Invalid(format!("campaign entropy source: {error}")))?;
        Self::from_bytes(bytes)
    }
    /// An explicit identity is useful for imports and deterministic engineering
    /// fixtures. Callers must use a distinct identity when starting a new game.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self> {
        if bytes == [0; 16] {
            return Err(Error::Invalid("zero campaign identity".into()));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReferenceId(pub NonZeroU64);

/// Numeric locals use the original double-width storage, including NaN payloads
/// and signed zero. Reference identity never passes through float conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Value {
    Uninitialized,
    Number { bits: u64 },
    Reference { value: ReferenceValue },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceValue {
    Null,
    Content { key: FormKey },
    Live { id: ReferenceId },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Owner {
    Quest {
        key: FormKey,
    },
    Placed {
        reference: ReferenceId,
    },
    /// Separate activations of the same embedded definition may have separate
    /// instances. The caller supplies the activation identity and context.
    Fragment {
        activation: NonZeroU64,
    },
}

pub(crate) fn valid_form(key: &FormKey) -> Result<()> {
    if key.profile != ProfileId::NvOriginal
        || key.local_id > 0x00ff_ffff
        || plugin_name(&key.origin_plugin).ok().as_ref() != Some(&key.origin_plugin)
    {
        return Err(Error::Invalid("noncanonical NV form identity".into()));
    }
    Ok(())
}

pub(crate) fn check_reference(
    value: &ReferenceValue,
    exists: &impl Fn(ReferenceId) -> Result<()>,
) -> Result<()> {
    match value {
        ReferenceValue::Null => Ok(()),
        ReferenceValue::Content { key } => valid_form(key),
        ReferenceValue::Live { id } => exists(*id),
    }
}
pub(crate) fn check_owner(
    owner: &Owner,
    exists: &impl Fn(ReferenceId) -> Result<()>,
) -> Result<()> {
    match owner {
        Owner::Quest { key } => valid_form(key),
        Owner::Placed { reference } => exists(*reference),
        Owner::Fragment { .. } => Ok(()),
    }
}
pub(crate) fn check_context(
    context: &crate::events::Context,
    max_arguments: usize,
    exists: &impl Fn(ReferenceId) -> Result<()>,
) -> Result<()> {
    if context.arguments.len() > max_arguments {
        return Err(Error::Capacity("event arguments"));
    }
    for id in [context.calling_reference, context.containing_reference]
        .into_iter()
        .flatten()
    {
        exists(id)?;
    }
    for value in context.target.iter().chain(context.arguments.iter()) {
        check_reference(value, exists)?;
    }
    Ok(())
}
