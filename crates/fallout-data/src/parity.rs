use serde::{Deserialize, Serialize};

/// These are evidence levels, not a percentage of the game that works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Unknown,
    Decoded,
    Implemented,
    UnitTested,
    OracleTested,
    Accepted,
}
