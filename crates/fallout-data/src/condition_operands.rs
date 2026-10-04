//! FNV's on-disk condition words have their own type boundary. Executable
//! descriptors supply parameter IDs; pinned xEdit supplies the CTDA word schema.
//! This module resolves immutable form dependencies, never query results.
use crate::{
    Result,
    condition::{Condition, RunOnDomain},
    content,
    identity::FormKey,
    plugin,
    store::{Location, RecordStore},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[path = "condition_record.rs"]
mod record;
pub use record::{ConditionSite, PreparedRecord, RecordIdentity, RecordLimits, prepare_record};

#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    pub type_id: u32,
    pub optional_word: u32,
}
#[derive(Debug, Clone)]
pub struct Signature {
    pub parameters: Vec<Parameter>,
}
pub type Signatures = BTreeMap<u16, Signature>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    ActorValue,
    Axis,
    Sex,
    CrimeType,
    FormType,
    MiscStat,
    Alignment,
    EquipType,
    CriticalStage,
    MenuMode,
    CreatureType,
    QuestObjective,
    QuestStage,
    VatsFunction,
    VatsAction,
    WeaponType,
    BodyLocation,
    Reference,
    Actor,
    InventoryObject,
    Cell,
    EffectItem,
    Quest,
    Race,
    Class,
    Faction,
    Global,
    Furniture,
    BaseObject,
    ActorBase,
    Worldspace,
    Package,
    BaseEffect,
    Weather,
    Owner,
    FormList,
    Perk,
    Note,
    EncounterZone,
    Idle,
    VoiceType,
    Reputation,
    Casino,
    Challenge,
    Region,
    Form,
    Weapon,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Unused {
        raw_word: u32,
    },
    SignedInteger {
        raw_word: u32,
        value: i32,
    },
    FloatBits {
        raw_word: u32,
    },
    SignedDomain {
        raw_word: u32,
        value: i32,
        domain: Domain,
    },
    UnsignedDomain {
        raw_word: u32,
        domain: Domain,
    },
    VariableIndex {
        raw_word: u32,
        signed_index: i32,
    },
    FormId {
        raw_word: u32,
        domain: Domain,
    },
    Unknown {
        raw_word: u32,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureStatus {
    DescriptorBound,
    MissingDescriptor,
    UnverifiedOptionalWord,
    AdditionalParameters,
    SchemaDisagreement,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    Absent,
    Subject,
    Target,
    Reference { raw_word: Option<u32> },
    CombatTarget,
    LinkedReference,
    AnimationGroup { raw_word: Option<u32> },
    Unknown { raw_word: u32 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormStatus {
    Null,
    Defined,
    Deleted,
    Missing,
    RuntimeDependency,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub source_name: String,
    pub record_kind: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormDependency {
    pub raw_word: u32,
    pub key: Option<FormKey>,
    pub status: FormStatus,
    pub runtime_binding: Option<content::RuntimeBinding>,
    pub target: Option<Target>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Operand {
    pub parameter_type_id: Option<u32>,
    pub optional_word: Option<u32>,
    pub value: Value,
    pub form_dependency: Option<FormDependency>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub signature_status: SignatureStatus,
    pub signature_parameter_count: Option<usize>,
    pub operands: [Operand; 2],
    pub comparison_global: Option<FormDependency>,
    pub subject: Subject,
    pub subject_reference: Option<FormDependency>,
    pub legacy_target_flag_present: bool,
    pub live_values_resolved: bool,
    pub evaluation_ready: bool,
}

fn signed(raw_word: u32, domain: Domain) -> Value {
    Value::SignedDomain {
        raw_word,
        value: raw_word as i32,
        domain,
    }
}
fn unsigned(raw_word: u32, domain: Domain) -> Value {
    Value::UnsignedDomain { raw_word, domain }
}
fn form(raw_word: u32, domain: Domain) -> Value {
    Value::FormId { raw_word, domain }
}

/// Only the inspected CTDA/native type associations enter this profile. Other
/// descriptors remain visible as unknown, even if a native decoder accepts them.
fn word_value(function: u16, index: usize, raw_word: u32, type_id: u32, first: u32) -> Value {
    use Domain::*;
    match (function, index, type_id) {
        (408, 0, 1) => unsigned(raw_word, VatsFunction),
        (408, 1, 1) => match first {
            0 => form(raw_word, Weapon),
            1 | 3 | 10 => form(raw_word, FormList),
            2 => form(raw_word, ActorBase),
            5 => signed(raw_word, ActorValue),
            6 => unsigned(raw_word, VatsAction),
            9 => form(raw_word, EffectItem),
            15 => unsigned(raw_word, WeaponType),
            4 | 7 | 8 | 11..=14 | 16 | 17 => Value::Unused { raw_word },
            _ => Value::Unknown { raw_word },
        },
        (420 | 421, 1, 1) => signed(raw_word, QuestObjective),
        (36, 0, 1) => unsigned(raw_word, MenuMode),
        (438, 0, 1) => unsigned(raw_word, CreatureType),
        (398, 0, 1) => signed(raw_word, BodyLocation),
        (408, _, _) | (427, _, 0..=45 | 47..) => Value::Unknown { raw_word },
        (_, _, 1) => Value::SignedInteger {
            raw_word,
            value: raw_word as i32,
        },
        (_, _, 2) => Value::FloatBits { raw_word },
        (_, _, 5) => signed(raw_word, ActorValue),
        (_, _, 8) => unsigned(raw_word, Axis),
        (_, _, 18) => unsigned(raw_word, Sex),
        (_, _, 22) => Value::VariableIndex {
            raw_word,
            signed_index: raw_word as i32,
        },
        (_, _, 23) => signed(raw_word, QuestStage),
        (_, _, 28) => unsigned(raw_word, CrimeType),
        (_, _, 32) => unsigned(raw_word, FormType),
        (_, _, 41) => unsigned(raw_word, MiscStat),
        (_, _, 51) => unsigned(raw_word, Alignment),
        (_, _, 52) => unsigned(raw_word, EquipType),
        (_, _, 55) => unsigned(raw_word, CriticalStage),
        (_, _, 3 | 50) => form(raw_word, InventoryObject),
        (_, _, 4) => form(raw_word, Reference),
        (_, _, 6) => form(raw_word, Actor),
        (_, _, 9) => form(raw_word, Cell),
        (_, _, 11) => form(raw_word, EffectItem),
        (_, _, 14) => form(raw_word, Quest),
        (_, _, 15) => form(raw_word, Race),
        (_, _, 16) => form(raw_word, Class),
        (_, _, 17) => form(raw_word, Faction),
        (_, _, 19) => form(raw_word, Global),
        (_, _, 20) => form(raw_word, Furniture),
        (_, _, 21 | 53) => form(raw_word, BaseObject),
        (_, _, 25) => form(raw_word, ActorBase),
        (_, _, 27) => form(raw_word, Worldspace),
        (_, _, 29) => form(raw_word, Package),
        (_, _, 31) => form(raw_word, BaseEffect),
        (_, _, 33) => form(raw_word, Weather),
        (_, _, 35) => form(raw_word, Owner),
        (_, _, 37) => form(raw_word, FormList),
        (_, _, 39) => form(raw_word, Perk),
        (_, _, 40) => form(raw_word, Note),
        (427, _, 46) => form(raw_word, VoiceType),
        (_, _, 47) => form(raw_word, EncounterZone),
        (_, _, 48) => form(raw_word, Idle),
        (_, _, 61) => form(raw_word, Form),
        (_, _, 62) => form(raw_word, Reputation),
        (_, _, 63) => form(raw_word, Casino),
        (_, _, 65) => form(raw_word, Challenge),
        (_, _, 69) => form(raw_word, Region),
        _ => Value::Unknown { raw_word },
    }
}

pub fn form_dependency(
    store: &RecordStore,
    source: Location,
    raw_word: u32,
) -> Result<FormDependency> {
    let key = store.key_for(source, raw_word)?;
    let mut result = FormDependency {
        raw_word,
        key: key.clone(),
        status: FormStatus::Null,
        runtime_binding: None,
        target: None,
    };
    if let Some(key) = key {
        if let Some(location) = store.winner(&key) {
            let header = &store.definition(location).header;
            result.status = if header.flags & plugin::DELETED != 0 {
                FormStatus::Deleted
            } else {
                FormStatus::Defined
            };
            result.target = Some(Target {
                source_name: store.source_name(location).into(),
                record_kind: plugin::signature(header.kind),
                record_file_offset: header.offset,
                record_flags: header.flags,
            });
        } else if let Some(binding) = content::runtime_binding(&key) {
            result.status = FormStatus::RuntimeDependency;
            result.runtime_binding = Some(binding);
        } else {
            result.status = FormStatus::Missing;
        }
    }
    Ok(result)
}

pub fn bind(
    store: &RecordStore,
    source: Location,
    condition: &Condition<'_>,
    signature: Option<&Signature>,
) -> Result<Binding> {
    let mut status = match signature {
        None => SignatureStatus::MissingDescriptor,
        Some(s) if s.parameters.iter().any(|p| p.optional_word > 1) => {
            SignatureStatus::UnverifiedOptionalWord
        }
        Some(s) if s.parameters.len() > 2 => SignatureStatus::AdditionalParameters,
        Some(_) => SignatureStatus::DescriptorBound,
    };
    let mut operands = Vec::with_capacity(2);
    for (index, raw_word) in condition.parameter_words.iter().copied().enumerate() {
        let parameter = signature.and_then(|s| s.parameters.get(index));
        let value = match (status, parameter) {
            (SignatureStatus::DescriptorBound, Some(p)) => word_value(
                condition.function_id,
                index,
                raw_word,
                p.type_id,
                condition.parameter_words[0],
            ),
            (SignatureStatus::DescriptorBound, None) => Value::Unused { raw_word },
            _ => Value::Unknown { raw_word },
        };
        let dependency = if matches!(value, Value::FormId { .. }) {
            Some(form_dependency(store, source, raw_word)?)
        } else {
            None
        };
        operands.push(Operand {
            parameter_type_id: parameter.map(|p| p.type_id),
            optional_word: parameter.map(|p| p.optional_word),
            value,
            form_dependency: dependency,
        });
    }
    if status == SignatureStatus::DescriptorBound
        && operands
            .iter()
            .any(|p| matches!(p.value, Value::Unknown { .. }))
    {
        status = SignatureStatus::SchemaDisagreement;
    }
    let subject = if condition.run_on_domain() == RunOnDomain::AnimationGroup {
        Subject::AnimationGroup {
            raw_word: condition.run_on_word,
        }
    } else {
        match condition.run_on_word {
            None => Subject::Absent,
            Some(0) => Subject::Subject,
            Some(1) => Subject::Target,
            Some(2) => Subject::Reference {
                raw_word: condition.reference_word,
            },
            Some(3) => Subject::CombatTarget,
            Some(4) => Subject::LinkedReference,
            Some(raw_word) => Subject::Unknown { raw_word },
        }
    };
    let subject_reference = if condition.reference_is_subject_selector() {
        condition
            .reference_word
            .map(|raw| form_dependency(store, source, raw))
            .transpose()?
    } else {
        None
    };
    let comparison_global = if condition.flags & 4 != 0 {
        Some(form_dependency(store, source, condition.comparison_word)?)
    } else {
        None
    };
    Ok(Binding {
        signature_status: status,
        signature_parameter_count: signature.map(|s| s.parameters.len()),
        operands: operands.try_into().expect("two fixed CTDA words"),
        comparison_global,
        subject,
        subject_reference,
        legacy_target_flag_present: condition.flags & 2 != 0,
        live_values_resolved: false,
        evaluation_ready: false,
    })
}
