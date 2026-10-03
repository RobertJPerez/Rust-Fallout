//! Authored quest and dialogue structure. These views preserve source order and
//! raw values; deciding whether a quest runs or a response is eligible is runtime work.
use crate::{Result, condition, malformed, plugin, script_units};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_fields: usize,
    pub max_sections: usize,
    pub max_findings: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_fields: 1_048_576,
            max_sections: 65_536,
            max_findings: 65_536,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Quest,
    Stage,
    LogEntry,
    Objective,
    Target,
    DialogueInfo,
    Response,
    BeginScript,
    EndScript,
    DialogueTopic,
    AddedQuest,
    InfoConnection,
}

#[derive(Debug, Serialize)]
pub struct Section {
    pub kind: SectionKind,
    /// Marker offset, not an index inferred from a stage number or FormID.
    pub marker_offset: Option<usize>,
    pub parent: Option<usize>,
    pub key: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub field_offset: usize,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct QuestData {
    pub flags: u8,
    pub priority: u8,
    pub padding: Option<[u8; 2]>,
    pub delay_bits: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct InfoData {
    pub dialogue_type: u8,
    pub next_speaker: u8,
    pub flags: u8,
    pub flags2: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct ResponseData {
    pub emotion: u32,
    pub emotion_value: i32,
    pub padding: [u8; 4],
    pub number: u8,
    pub number_padding: [u8; 3],
    pub sound_raw_form: u32,
    pub use_emotion_animation: Option<u8>,
    pub animation_padding: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetData {
    pub target_raw_form: u32,
    pub flags: u8,
    pub padding: [u8; 3],
}

#[derive(Debug)]
pub enum Value<'a> {
    Bytes,
    Text(&'a [u8]),
    RawForm(u32),
    Signed(i64),
    FloatBits(u32),
    Byte(u8),
    QuestData(QuestData),
    InfoData(InfoData),
    TopicData {
        dialogue_type: u8,
        flags: Option<u8>,
    },
    ResponseData(ResponseData),
    TargetData(TargetData),
    Empty,
    Condition(condition::Condition<'a>),
}

#[derive(Debug)]
pub struct Field<'a> {
    pub kind: [u8; 4],
    pub offset: usize,
    pub data: &'a [u8],
    /// Unknown/orphaned fields have no owner. They are retained, never reassigned.
    pub owner: Option<usize>,
    pub value: Value<'a>,
}

#[derive(Debug)]
pub struct Script<'a> {
    pub unit: script_units::Unit<'a>,
    pub owner: Option<usize>,
}

#[derive(Debug)]
pub struct Document<'a> {
    pub fields: Vec<Field<'a>>,
    pub sections: Vec<Section>,
    pub scripts: Vec<Script<'a>>,
    pub findings: Vec<Finding>,
}

pub fn narrative_record(kind: [u8; 4]) -> bool {
    matches!(&kind, b"QUST" | b"INFO" | b"DIAL")
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("checked narrative field"),
    )
}

/// Flat indexed sections keep repeated authored keys distinct and avoid recursive
/// allocation/traversal. A consumer can collect children without losing order.
pub fn decode<'a>(record: &'a plugin::Record, name: &str, limits: Limits) -> Result<Document<'a>> {
    let root = match &record.header.kind {
        b"QUST" => SectionKind::Quest,
        b"INFO" => SectionKind::DialogueInfo,
        b"DIAL" => SectionKind::DialogueTopic,
        _ => {
            return Err(crate::Error::Unsupported(
                "not a quest or dialogue record".into(),
            ));
        }
    };
    if limits.max_sections == 0 {
        return Err(crate::Error::Unsupported(
            "narrative section budget exceeded".into(),
        ));
    }
    let fail = |at: usize, reason: &str| {
        malformed(
            name,
            record.header.offset,
            format!("narrative field at decoded offset 0x{at:X}: {reason}"),
        )
    };
    let mut ranges = Vec::new();
    plugin::visit_subrecords(record, name, |field| {
        if ranges.len() >= limits.max_fields {
            return Err(fail(field.payload_offset, "field budget exceeded"));
        }
        ranges.push((field.kind, field.payload_offset, field.data.len()));
        Ok(())
    })?;
    let mut document = Document {
        fields: Vec::with_capacity(ranges.len()),
        sections: vec![Section {
            kind: root,
            marker_offset: None,
            parent: None,
            key: None,
        }],
        scripts: Vec::new(),
        findings: Vec::new(),
    };
    let mut state = State::default();
    for (kind, offset, size) in ranges {
        let data = &record.payload[offset + 6..offset + 6 + size];
        let value = decode_value(root, kind, data).map_err(|reason| fail(offset, reason))?;
        let (owner, marker, reason) = ownership(root, kind, &value, &mut state);
        let owner = if let Some((section_kind, parent, key)) = marker {
            if document.sections.len() >= limits.max_sections {
                return Err(fail(offset, "section budget exceeded"));
            }
            let index = document.sections.len();
            document.sections.push(Section {
                kind: section_kind,
                marker_offset: Some(offset),
                parent,
                key,
            });
            state.mark(section_kind, index);
            Some(index)
        } else {
            owner
        };
        if let Some(reason) = reason {
            if document.findings.len() >= limits.max_findings {
                return Err(fail(offset, "finding budget exceeded"));
            }
            document.findings.push(Finding {
                field_offset: offset,
                reason,
            });
        }
        document.fields.push(Field {
            kind,
            offset,
            data,
            owner,
            value,
        });
    }
    let script_limits = script_units::Limits {
        max_fields: limits.max_fields,
        max_units: limits.max_sections,
        ..script_units::Limits::default()
    };
    for unit in script_units::decode(record, name, script_limits)? {
        let field = document
            .fields
            .binary_search_by_key(&unit.header.offset, |field| field.offset)
            .expect("SCHR belongs to collected record fields");
        document.scripts.push(Script {
            owner: document.fields[field].owner,
            unit,
        });
    }
    Ok(document)
}

fn decode_value(
    root: SectionKind,
    kind: [u8; 4],
    data: &[u8],
) -> std::result::Result<Value<'_>, &'static str> {
    let exact = |length| {
        if data.len() == length {
            Ok(())
        } else {
            Err("invalid field length")
        }
    };
    let form = || {
        exact(4)?;
        Ok(Value::RawForm(word(data, 0)))
    };
    match &kind {
        b"CTDA" => condition::decode(data)
            .map(Value::Condition)
            .map_err(|_| "invalid CTDA length"),
        b"DATA" => match root {
            SectionKind::Quest => {
                if !matches!(data.len(), 2 | 4 | 8) {
                    return Err("QUST DATA must contain two, four or eight bytes");
                }
                Ok(Value::QuestData(QuestData {
                    flags: data[0],
                    priority: data[1],
                    padding: (data.len() >= 4).then(|| [data[2], data[3]]),
                    delay_bits: (data.len() == 8).then(|| word(data, 4)),
                }))
            }
            SectionKind::DialogueInfo => {
                if !matches!(data.len(), 3 | 4) {
                    return Err("INFO DATA must contain three or four bytes");
                }
                Ok(Value::InfoData(InfoData {
                    dialogue_type: data[0],
                    next_speaker: data[1],
                    flags: data[2],
                    flags2: data.get(3).copied(),
                }))
            }
            SectionKind::DialogueTopic => {
                if !matches!(data.len(), 1 | 2) {
                    return Err("DIAL DATA must contain one or two bytes");
                }
                Ok(Value::TopicData {
                    dialogue_type: data[0],
                    flags: data.get(1).copied(),
                })
            }
            _ => unreachable!("root section"),
        },
        b"INDX" if root == SectionKind::Quest => {
            exact(2)?;
            Ok(Value::Signed(
                i16::from_le_bytes(data.try_into().expect("checked index")) as i64,
            ))
        }
        b"QOBJ" if root == SectionKind::Quest => {
            exact(4)?;
            Ok(Value::Signed(word(data, 0) as i32 as i64))
        }
        b"QSDT" if root == SectionKind::Quest => {
            exact(1)?;
            Ok(Value::Byte(data[0]))
        }
        b"QSTA" if root == SectionKind::Quest => {
            exact(8)?;
            Ok(Value::TargetData(TargetData {
                target_raw_form: word(data, 0),
                flags: data[4],
                padding: data[5..8].try_into().expect("checked target"),
            }))
        }
        b"TRDT" if root == SectionKind::DialogueInfo => {
            if !matches!(data.len(), 20 | 24) {
                return Err("TRDT must contain twenty or twenty-four bytes");
            }
            Ok(Value::ResponseData(ResponseData {
                emotion: word(data, 0),
                emotion_value: word(data, 4) as i32,
                padding: data[8..12].try_into().expect("checked response"),
                number: data[12],
                number_padding: data[13..16].try_into().expect("checked response"),
                sound_raw_form: word(data, 16),
                use_emotion_animation: data.get(20).copied(),
                animation_padding: (data.len() == 24)
                    .then(|| data[21..24].try_into().expect("checked response")),
            }))
        }
        b"NEXT" if root == SectionKind::DialogueInfo => {
            exact(0)?;
            Ok(Value::Empty)
        }
        b"PNAM" if root == SectionKind::DialogueTopic => {
            exact(4)?;
            Ok(Value::FloatBits(word(data, 0)))
        }
        b"INFX" if root == SectionKind::DialogueTopic => {
            exact(4)?;
            Ok(Value::Signed(word(data, 0) as i32 as i64))
        }
        b"DNAM" if root == SectionKind::DialogueInfo => {
            exact(4)?;
            Ok(Value::Signed(i64::from(word(data, 0))))
        }
        b"SCRI" | b"NAM0" if root == SectionKind::Quest => form(),
        b"QSTI" | b"TPIC" | b"PNAM" | b"NAME" | b"SNAM" | b"LNAM" | b"TCLT" | b"TCLF" | b"TCFU"
        | b"SNDD" | b"ANAM" | b"KNAM"
            if root == SectionKind::DialogueInfo =>
        {
            form()
        }
        b"QSTI" | b"INFC" | b"QSTR" if root == SectionKind::DialogueTopic => form(),
        b"EDID" | b"FULL" | b"ICON" | b"CNAM" | b"NNAM" if root == SectionKind::Quest => {
            Ok(Value::Text(data))
        }
        b"NAM1" | b"NAM2" | b"NAM3" | b"RNAM" if root == SectionKind::DialogueInfo => {
            Ok(Value::Text(data))
        }
        b"EDID" | b"FULL" | b"TDUM" if root == SectionKind::DialogueTopic => Ok(Value::Text(data)),
        _ => Ok(Value::Bytes),
    }
}

#[derive(Default)]
struct State {
    stage: Option<usize>,
    entry: Option<usize>,
    objective: Option<usize>,
    target: Option<usize>,
    response: Option<usize>,
    script: Option<usize>,
    end_script: bool,
    begin_count: usize,
    end_count: usize,
    added_quest: Option<usize>,
    connection: Option<usize>,
}
impl State {
    fn mark(&mut self, kind: SectionKind, index: usize) {
        match kind {
            SectionKind::Stage => {
                self.stage = Some(index);
                self.entry = None;
                self.objective = None;
                self.target = None;
            }
            SectionKind::LogEntry => self.entry = Some(index),
            SectionKind::Objective => {
                self.stage = None;
                self.entry = None;
                self.objective = Some(index);
                self.target = None;
            }
            SectionKind::Target => self.target = Some(index),
            SectionKind::Response => self.response = Some(index),
            SectionKind::BeginScript | SectionKind::EndScript => self.script = Some(index),
            SectionKind::AddedQuest => {
                self.added_quest = Some(index);
                self.connection = None;
            }
            SectionKind::InfoConnection => self.connection = Some(index),
            _ => {}
        }
    }
}
type Marker = Option<(SectionKind, Option<usize>, Option<i64>)>;
type Ownership = (Option<usize>, Marker, Option<&'static str>);
fn orphan(owner: Option<usize>) -> Ownership {
    (
        owner,
        None,
        owner.is_none().then_some("missing authored owner"),
    )
}
fn marker(kind: SectionKind, parent: Option<usize>, key: Option<i64>) -> Ownership {
    (
        None,
        Some((kind, parent, key)),
        parent.is_none().then_some("missing authored parent"),
    )
}
fn ownership(root: SectionKind, kind: [u8; 4], value: &Value<'_>, state: &mut State) -> Ownership {
    let key = match value {
        Value::Signed(value) => Some(*value),
        Value::RawForm(value) => Some(i64::from(*value)),
        _ => None,
    };
    if root == SectionKind::Quest {
        return match &kind {
            b"INDX" => marker(SectionKind::Stage, Some(0), key),
            b"QSDT" => marker(SectionKind::LogEntry, state.stage, None),
            b"QOBJ" => marker(SectionKind::Objective, Some(0), key),
            b"QSTA" => marker(SectionKind::Target, state.objective, None),
            b"CNAM" | b"NAM0" => orphan(state.entry),
            b"NNAM" => orphan(state.objective),
            b"CTDA" => orphan(if state.target.is_some() || state.objective.is_some() {
                state.target
            } else if state.entry.is_some() || state.stage.is_some() {
                state.entry
            } else {
                Some(0)
            }),
            b"DATA" => (
                Some(0),
                None,
                match value {
                    Value::QuestData(data) if data.padding.is_none() => {
                        Some("short quest header; loading unverified")
                    }
                    _ => None,
                },
            ),
            b"EDID" | b"SCRI" | b"FULL" | b"ICON" => (Some(0), None, None),
            _ if script_units::script_field(kind) => orphan(state.entry),
            _ => (None, None, Some("unrecognized field")),
        };
    }
    if root == SectionKind::DialogueInfo {
        match &kind {
            b"TRDT" => {
                state.script = None;
                return marker(SectionKind::Response, Some(0), None);
            }
            b"NAM1" | b"NAM2" | b"NAM3" | b"SNAM" | b"LNAM" => return orphan(state.response),
            b"NEXT" => {
                state.response = None;
                state.script = None;
                state.end_script = true;
                return (Some(0), None, None);
            }
            b"SCHR" => {
                state.response = None;
                let script_kind = if state.end_script {
                    SectionKind::EndScript
                } else {
                    SectionKind::BeginScript
                };
                let mut result = marker(script_kind, Some(0), None);
                let count = if state.end_script {
                    &mut state.end_count
                } else {
                    &mut state.begin_count
                };
                if *count != 0 {
                    result.2 = Some("repeated script role");
                }
                *count += 1;
                return result;
            }
            _ if script_units::script_field(kind) => return orphan(state.script),
            b"CTDA" | b"DATA" | b"QSTI" | b"TPIC" | b"PNAM" | b"NAME" | b"TCLT" | b"TCLF"
            | b"TCFU" | b"SNDD" | b"RNAM" | b"ANAM" | b"KNAM" | b"DNAM" => {
                state.response = None;
                state.script = None;
                return (Some(0), None, None);
            }
            _ => return (None, None, Some("unrecognized field")),
        }
    }
    match &kind {
        b"QSTI" => marker(SectionKind::AddedQuest, Some(0), key),
        b"INFC" => marker(SectionKind::InfoConnection, state.added_quest, key),
        b"INFX" => orphan(state.connection),
        b"EDID" | b"QSTR" | b"FULL" | b"PNAM" | b"TDUM" | b"DATA" => {
            state.added_quest = None;
            state.connection = None;
            (Some(0), None, None)
        }
        _ => (None, None, Some("unrecognized field")),
    }
}

/// A compact independent-comparison recipe. Signed values are sign-extended to
/// u64; optional values use a presence word followed by the unchanged source bits.
pub fn evidence_words(value: &Value<'_>) -> (u8, Vec<u64>) {
    let optional =
        |value: Option<u32>| vec![u64::from(value.is_some()), u64::from(value.unwrap_or(0))];
    match value {
        Value::Bytes => (0, vec![]),
        Value::Text(_) => (1, vec![]),
        Value::RawForm(value) => (2, vec![u64::from(*value)]),
        Value::Signed(value) => (3, vec![*value as u64]),
        Value::FloatBits(value) => (4, vec![u64::from(*value)]),
        Value::Byte(value) => (5, vec![u64::from(*value)]),
        Value::QuestData(value) => {
            let mut words = vec![
                value.flags as u64,
                value.priority as u64,
                u64::from(value.padding.is_some()),
                value.padding.unwrap_or([0; 2])[0] as u64,
                value.padding.unwrap_or([0; 2])[1] as u64,
            ];
            words.extend(optional(value.delay_bits));
            (6, words)
        }
        Value::InfoData(value) => {
            let mut words = vec![
                value.dialogue_type as u64,
                value.next_speaker as u64,
                value.flags as u64,
            ];
            words.extend(optional(value.flags2.map(u32::from)));
            (7, words)
        }
        Value::TopicData {
            dialogue_type,
            flags,
        } => {
            let mut words = vec![*dialogue_type as u64];
            words.extend(optional(flags.map(u32::from)));
            (8, words)
        }
        Value::ResponseData(value) => {
            let mut words = vec![value.emotion as u64, value.emotion_value as i64 as u64];
            words.extend(value.padding.map(u64::from));
            words.push(value.number as u64);
            words.extend(value.number_padding.map(u64::from));
            words.push(value.sound_raw_form as u64);
            words.extend(optional(value.use_emotion_animation.map(u32::from)));
            words.extend(value.animation_padding.unwrap_or([0; 3]).map(u64::from));
            (9, words)
        }
        Value::TargetData(value) => {
            let mut words = vec![value.target_raw_form as u64, value.flags as u64];
            words.extend(value.padding.map(u64::from));
            (10, words)
        }
        Value::Empty => (11, vec![]),
        Value::Condition(value) => {
            let mut words = vec![value.flags as u64];
            words.extend(value.flag_padding.map(u64::from));
            words.extend([value.comparison_word as u64, value.function_id as u64]);
            words.extend(value.function_padding.map(u64::from));
            words.extend(value.parameter_words.map(u64::from));
            words.extend(optional(value.run_on_word));
            words.extend(optional(value.reference_word));
            (12, words)
        }
    }
}
