//! Source-less compiled local schema. SCTX is optional and is never used to
//! replace authored compiled declarations. Duplicate indices keep first-match
//! lookup, while the loaded definition preserves every declaration for audits.
use fallout_data::loaded_scripts::{LoadedScript, ReferenceStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Storage compatibility is shared by mutation, restoration and publication.
/// Persistent reference links are validated separately against the snapshot or
/// live world; numeric bit patterns, including NaNs, remain exact storage.
pub(crate) fn check_value(local: &Local, value: &crate::identity::Value) -> crate::Result<()> {
    use crate::{Error, identity::Value};
    match (local.kind, value) {
        (_, Value::Uninitialized)
        | (Kind::Float | Kind::Integer, Value::Number { .. })
        | (Kind::Reference, Value::Reference { .. }) => Ok(()),
        (Kind::Unsupported { .. } | Kind::UnverifiedZeroIndex { .. }, _) => {
            Err(Error::UnsupportedLocal(local.index))
        }
        _ => Err(Error::IncompatibleLocal(local.index)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Float,
    Integer,
    Reference,
    /// Index zero overlaps the static-reference sentinel in the reviewed
    /// upstream model. It does not occur in the original compiled declarations.
    UnverifiedZeroIndex {
        type_byte: u8,
    },
    Unsupported {
        type_byte: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Local {
    pub index: u32,
    pub declaration_decoded_offset: u32,
    pub kind: Kind,
}

pub fn locals(script: &LoadedScript) -> BTreeMap<u32, Local> {
    let references: BTreeSet<_> = script
        .references()
        .iter()
        .filter(|r| r.status == ReferenceStatus::DynamicVariable)
        .map(|r| r.value)
        .collect();
    let mut result = BTreeMap::new();
    for declaration in script.declarations() {
        result.entry(declaration.index).or_insert_with(|| Local {
            index: declaration.index,
            declaration_decoded_offset: declaration.decoded_offset,
            kind: if declaration.index == 0 {
                Kind::UnverifiedZeroIndex {
                    type_byte: declaration.type_byte,
                }
            } else if references.contains(&declaration.index) {
                Kind::Reference
            } else {
                match declaration.type_byte {
                    0 => Kind::Float,
                    1 => Kind::Integer,
                    type_byte => Kind::Unsupported { type_byte },
                }
            },
        });
    }
    result
}
