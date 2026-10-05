//! Static PEX control-flow and callsite evidence; this module does not execute code.
use crate::{Result, pex};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchDestination {
    Instruction { index: usize },
    FunctionEnd,
}

/// Convert the FO4 relative instruction displacement to its in-function destination.
/// The end boundary is retained because the retail reader accepts a target one past
/// the last instruction; that boundary is not interpreted as a return or continuation.
pub fn branch_destination(
    instruction: &pex::Instruction,
    pc: usize,
    instruction_count: usize,
    source_name: &str,
) -> Result<Option<BranchDestination>> {
    let operand = match instruction.opcode {
        20 => 0,
        21 | 22 => 1,
        _ => return Ok(None),
    };
    let Some(pex::Value::Integer(delta)) = instruction.arguments.get(operand) else {
        return Err(crate::bad(
            source_name,
            instruction.offset,
            "branch displacement must be integer",
        ));
    };
    let target = (pc as i64)
        .checked_add(i64::from(*delta))
        .ok_or_else(|| crate::bad(source_name, instruction.offset, "branch target overflow"))?;
    if target < 0 || target > instruction_count as i64 {
        return Err(crate::bad(
            source_name,
            instruction.offset,
            "branch target outside function",
        ));
    }
    Ok(Some(if target == instruction_count as i64 {
        BranchDestination::FunctionEnd
    } else {
        BranchDestination::Instruction {
            index: target as usize,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(opcode: u8, arguments: Vec<pex::Value>) -> pex::Instruction {
        pex::Instruction {
            offset: 12,
            opcode,
            arguments,
            varargs: Vec::new(),
        }
    }

    #[test]
    fn branches_keep_instruction_and_one_past_end_destinations() {
        assert_eq!(
            branch_destination(
                &instruction(20, vec![pex::Value::Integer(-1)]),
                2,
                4,
                "f.pex"
            )
            .unwrap(),
            Some(BranchDestination::Instruction { index: 1 })
        );
        assert_eq!(
            branch_destination(
                &instruction(21, vec![pex::Value::None, pex::Value::Integer(2)]),
                2,
                4,
                "f.pex"
            )
            .unwrap(),
            Some(BranchDestination::FunctionEnd)
        );
        assert_eq!(
            branch_destination(&instruction(23, Vec::new()), 0, 0, "f.pex").unwrap(),
            None
        );
    }

    #[test]
    fn malformed_branch_operands_and_out_of_range_targets_fail() {
        let bad_type = branch_destination(
            &instruction(20, vec![pex::Value::Identifier(0)]),
            0,
            1,
            "bad.pex",
        )
        .unwrap_err();
        assert!(
            bad_type
                .to_string()
                .contains("branch displacement must be integer")
        );
        let bad_target = branch_destination(
            &instruction(22, vec![pex::Value::None, pex::Value::Integer(2)]),
            0,
            1,
            "bad.pex",
        )
        .unwrap_err();
        assert!(
            bad_target
                .to_string()
                .contains("branch target outside function")
        );
    }
}
