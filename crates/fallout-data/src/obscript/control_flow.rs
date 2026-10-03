//! Bounded source structure, without adopting the original VM's branch rules.
//! A matched delimiter and a matching raw skip word are observations about bytes.
//! They do not establish condition truth, event dispatch or command effects.
use super::{Instruction, Program};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub decode: super::Limits,
    pub maximum_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            decode: super::Limits::default(),
            maximum_depth: 65_536,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    EventByteSpan,
    InterveningInstructions,
}

/// The destination is a structural delimiter, not an executable successor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub source_instruction: usize,
    pub delimiter_instruction: usize,
    pub raw_word: u32,
    pub observed_distance: usize,
    pub relation: Relation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub begin_instruction: usize,
    pub end_instruction: usize,
    pub event_id: u16,
}

/// Each arm keeps its enclosing if and final endif, including empty arms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Arm {
    pub instruction: usize,
    pub enclosing_if: usize,
    pub next_delimiter: usize,
    pub end_if: usize,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub kind: &'static str,
    pub instruction_scda_offset: usize,
    pub opcode: Option<u16>,
    pub raw_word: Option<u32>,
    pub observed_distance: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Decode(#[from] super::DecodeError),
    #[error("SCDA byte 0x{offset:X}: {kind}", offset = .0.instruction_scda_offset, kind = .0.kind)]
    Structure(Diagnostic),
}

/// Private, freshly decoded source prevents a caller from forging instructions
/// or attaching structural links to a different SCDA buffer.
#[derive(Debug)]
pub struct Plan<'a> {
    program: Program<'a>,
    events: Vec<Event>,
    arms: Vec<Arm>,
    links: Vec<Link>,
    maximum_depth: usize,
}
impl Plan<'_> {
    pub fn bytes(&self) -> &[u8] {
        self.program.bytes
    }
    pub fn instructions(&self) -> &[Instruction<'_>] {
        &self.program.instructions
    }
    pub fn events(&self) -> &[Event] {
        &self.events
    }
    pub fn arms(&self) -> &[Arm] {
        &self.arms
    }
    pub fn links(&self) -> &[Link] {
        &self.links
    }
    pub fn maximum_depth(&self) -> usize {
        self.maximum_depth
    }
}

fn issue(instruction: &Instruction<'_>, kind: &'static str) -> Error {
    Error::Structure(Diagnostic {
        kind,
        instruction_scda_offset: instruction.bytes.start,
        opcode: Some(instruction.opcode),
        raw_word: None,
        observed_distance: None,
    })
}

/// Read the structural fields without interpreting expression tokens or native
/// operands. Their separate decoders and capability gates still apply.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Plan<'_>, Error> {
    let program = super::decode(bytes, limits.decode)?;
    struct OpenIf {
        first_arm: usize,
        last_arm: usize,
        has_else: bool,
    }
    let mut plan = Plan {
        program,
        events: Vec::new(),
        arms: Vec::new(),
        links: Vec::new(),
        maximum_depth: 0,
    };
    let mut event = None;
    let mut stack: Vec<OpenIf> = Vec::new();
    let mut siblings: Vec<Option<usize>> = Vec::new();
    let mut links: Vec<Option<Link>> = vec![None; plan.program.instructions.len()];
    for (index, instruction) in plan.program.instructions.iter().enumerate() {
        if instruction.opcode < 0x1000 && instruction.calling_reference.is_some() {
            return Err(issue(instruction, "statement_reference_prefix"));
        }
        match instruction.opcode {
            0x10 => {
                if event.is_some() || !stack.is_empty() {
                    return Err(issue(instruction, "nested_event"));
                }
                event = Some(index);
            }
            0x11 => {
                if !instruction.operands.is_empty() {
                    return Err(issue(instruction, "statement_operands"));
                }
                let begin: usize = event.ok_or_else(|| issue(instruction, "orphan_end"))?;
                if !stack.is_empty() {
                    return Err(issue(instruction, "conditional_crosses_event"));
                }
                let source = &plan.program.instructions[begin];
                let header = source.event.expect("freshly decoded begin");
                plan.events.push(Event {
                    begin_instruction: begin,
                    end_instruction: index,
                    event_id: header.id,
                });
                links[begin] = Some(Link {
                    source_instruction: begin,
                    delimiter_instruction: index,
                    raw_word: header.end_jump_bytes,
                    observed_distance: instruction.bytes.end - source.bytes.end,
                    relation: Relation::EventByteSpan,
                });
                event = None;
            }
            0x16..=0x18 => {
                let length = instruction.operands.len();
                if instruction.opcode == 0x17 {
                    if length != 2 {
                        return Err(issue(instruction, "statement_operands"));
                    }
                } else if length < 4
                    || usize::from(u16::from_le_bytes([
                        instruction.operands[2],
                        instruction.operands[3],
                    ])) != length - 4
                {
                    return Err(issue(instruction, "conditional_expression_extent"));
                }
                let arm_index = plan.arms.len();
                if instruction.opcode == 0x16 {
                    if stack.len() >= limits.maximum_depth {
                        return Err(issue(instruction, "depth_budget"));
                    }
                    stack.push(OpenIf {
                        first_arm: arm_index,
                        last_arm: arm_index,
                        has_else: false,
                    });
                    plan.maximum_depth = plan.maximum_depth.max(stack.len());
                    plan.arms.push(Arm {
                        instruction: index,
                        enclosing_if: index,
                        next_delimiter: usize::MAX,
                        end_if: usize::MAX,
                        depth: stack.len(),
                    });
                } else {
                    let open = stack
                        .last_mut()
                        .ok_or_else(|| issue(instruction, "orphan_arm"))?;
                    if open.has_else {
                        return Err(issue(instruction, "arm_after_else"));
                    }
                    plan.arms[open.last_arm].next_delimiter = index;
                    siblings[open.last_arm] = Some(arm_index);
                    let enclosing_if = plan.arms[open.first_arm].instruction;
                    open.last_arm = arm_index;
                    open.has_else = instruction.opcode == 0x17;
                    plan.arms.push(Arm {
                        instruction: index,
                        enclosing_if,
                        next_delimiter: usize::MAX,
                        end_if: usize::MAX,
                        depth: stack.len(),
                    });
                }
                siblings.push(None);
            }
            0x19 => {
                if !instruction.operands.is_empty() {
                    return Err(issue(instruction, "statement_operands"));
                }
                let open = stack
                    .pop()
                    .ok_or_else(|| issue(instruction, "orphan_end_if"))?;
                plan.arms[open.last_arm].next_delimiter = index;
                // Walk this group's sibling chain, not the nested arms between
                // them. Every arm is completed once, so total work stays linear.
                let mut arm_index = open.first_arm;
                loop {
                    let arm = &mut plan.arms[arm_index];
                    arm.end_if = index;
                    if arm_index == open.last_arm {
                        break;
                    }
                    arm_index = siblings[arm_index].expect("matched sibling arm");
                }
            }
            0x1d | 0x1e => {
                if !instruction.operands.is_empty() {
                    return Err(issue(instruction, "statement_operands"));
                }
            }
            0x12..=0x15 | 0x1f | 0x1000..=u16::MAX => {}
            _ => return Err(issue(instruction, "unsupported_opcode")),
        }
    }
    if event.is_some() || !stack.is_empty() {
        return Err(Error::Structure(Diagnostic {
            kind: if event.is_some() {
                "unclosed_event"
            } else {
                "unclosed_conditional"
            },
            instruction_scda_offset: bytes.len(),
            opcode: None,
            raw_word: None,
            observed_distance: None,
        }));
    }
    for arm in &plan.arms {
        let source = &plan.program.instructions[arm.instruction];
        links[arm.instruction] = Some(Link {
            source_instruction: arm.instruction,
            delimiter_instruction: arm.next_delimiter,
            raw_word: u32::from(u16::from_le_bytes([source.operands[0], source.operands[1]])),
            observed_distance: arm.next_delimiter - arm.instruction - 1,
            relation: Relation::InterveningInstructions,
        });
    }
    plan.links = links.into_iter().flatten().collect();
    for link in &plan.links {
        if usize::try_from(link.raw_word).ok() != Some(link.observed_distance) {
            let source = &plan.program.instructions[link.source_instruction];
            return Err(Error::Structure(Diagnostic {
                kind: "raw_distance_mismatch",
                instruction_scda_offset: source.bytes.start,
                opcode: Some(source.opcode),
                raw_word: Some(link.raw_word),
                observed_distance: Some(link.observed_distance),
            }));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests;
