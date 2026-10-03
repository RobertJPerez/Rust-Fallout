//! FNV CTDA fields, separate from condition execution and native arguments.
//! Layout: pinned xEdit FNV definitions. Older records keep omitted words absent;
//! no editor migration, loaded form resolution or default subject is applied.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonValue {
    FloatBits(u32),
    GlobalRawForm(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOnDomain {
    SubjectSelection,
    AnimationGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condition<'a> {
    pub bytes: &'a [u8],
    pub flags: u8,
    pub flag_padding: [u8; 3],
    pub comparison_word: u32,
    /// The on-disk function is u16 followed by two preserved unused bytes.
    /// xNVSE's in-memory Condition u32 member is a different boundary.
    pub function_id: u16,
    pub function_padding: [u8; 2],
    pub parameter_words: [u32; 2],
    pub run_on_word: Option<u32>,
    pub reference_word: Option<u32>,
}

impl Condition<'_> {
    pub fn comparison_operator(&self) -> ComparisonOperator {
        match self.flags >> 5 {
            0 => ComparisonOperator::Equal,
            1 => ComparisonOperator::NotEqual,
            2 => ComparisonOperator::Greater,
            3 => ComparisonOperator::GreaterOrEqual,
            4 => ComparisonOperator::Less,
            5 => ComparisonOperator::LessOrEqual,
            code => ComparisonOperator::Unknown(code),
        }
    }

    pub fn comparison_value(&self) -> ComparisonValue {
        if self.flags & 4 != 0 {
            ComparisonValue::GlobalRawForm(self.comparison_word)
        } else {
            ComparisonValue::FloatBits(self.comparison_word)
        }
    }

    /// Preserve the authored OR bit. Group boundaries and evaluation order must
    /// come from the owning record schema, not a global list of CTDA fields.
    pub fn or_flag(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn run_on_domain(&self) -> RunOnDomain {
        match self.function_id {
            106 | 285 => RunOnDomain::AnimationGroup,
            _ => RunOnDomain::SubjectSelection,
        }
    }

    pub fn reference_is_subject_selector(&self) -> bool {
        self.run_on_domain() == RunOnDomain::SubjectSelection && self.run_on_word == Some(2)
    }

    pub fn finite_float_comparison_word(&self) -> Option<bool> {
        match self.comparison_value() {
            ComparisonValue::FloatBits(bits) => Some(bits & 0x7f80_0000 != 0x7f80_0000),
            ComparisonValue::GlobalRawForm(_) => None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("FNV CTDA length {0} is outside the verified 20/24/28-byte layouts")]
pub struct DecodeError(pub usize);

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked CTDA extent"))
}

pub fn decode(bytes: &[u8]) -> Result<Condition<'_>, DecodeError> {
    if !matches!(bytes.len(), 20 | 24 | 28) {
        return Err(DecodeError(bytes.len()));
    }
    Ok(Condition {
        bytes,
        flags: bytes[0],
        flag_padding: bytes[1..4].try_into().expect("checked padding"),
        comparison_word: word(bytes, 4),
        function_id: u16::from_le_bytes([bytes[8], bytes[9]]),
        function_padding: bytes[10..12].try_into().expect("checked padding"),
        parameter_words: [word(bytes, 12), word(bytes, 16)],
        run_on_word: (bytes.len() >= 24).then(|| word(bytes, 20)),
        reference_word: (bytes.len() >= 28).then(|| word(bytes, 24)),
    })
}
