use crate::identity::{InstanceId, ReferenceId, ReferenceValue};
use serde::{Deserialize, Serialize};

/// Host-supplied integer clocks. No fixed frame rate, quest delay, menu policy
/// or relationship between these clocks is inferred here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clocks {
    pub tick: u64,
    pub game_nanoseconds: u64,
    pub menu_nanoseconds: u64,
    pub real_nanoseconds: u64,
}

impl Clocks {
    pub(crate) fn follows(self, prior: Self) -> bool {
        self.tick > prior.tick
            && self.game_nanoseconds >= prior.game_nanoseconds
            && self.menu_nanoseconds >= prior.menu_nanoseconds
            && self.real_nanoseconds >= prior.real_nanoseconds
    }
    pub(crate) fn no_later_than(self, other: Self) -> bool {
        self.tick <= other.tick
            && self.game_nanoseconds <= other.game_nanoseconds
            && self.menu_nanoseconds <= other.menu_nanoseconds
            && self.real_nanoseconds <= other.real_nanoseconds
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    /// Descriptor IDs in compiled BEGIN headers are not object-event masks.
    Block {
        event_id: u16,
        begin_byte_offset: u32,
    },
    /// Preserve an explicit host observation without inventing its mapping to
    /// blocks. Unknown bits remain data; this queue does not dispatch them.
    ObjectEvent { mask: u32 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub calling_reference: Option<ReferenceId>,
    pub containing_reference: Option<ReferenceId>,
    pub target: Option<ReferenceValue>,
    pub arguments: Vec<ReferenceValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub sequence: u64,
    pub instance: InstanceId,
    pub trigger: Trigger,
    pub context: Context,
    pub arrived: Clocks,
}
