use crate::{Error, Result};
use fallout_data::identity::{FormKey, ProfileId, plugin_name};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

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
