//! Source-bound structural preparation. Success is not VM execution permission.
use super::{
    argument_census::Signatures, control_flow, expression, expression_plan, operand_binding,
};
use crate::loaded_scripts::{Catalogue, Handle, LoadedScript};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub control: control_flow::Limits,
    pub expression: expression_plan::Limits,
    pub maximum_expressions: usize,
    pub maximum_tokens: usize,
    pub maximum_nodes: usize,
    pub maximum_operand_uses: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            control: control_flow::Limits::default(),
            expression: expression_plan::Limits::default(),
            maximum_expressions: 262_144,
            maximum_tokens: 1_000_000,
            maximum_nodes: 1_000_000,
            maximum_operand_uses: 262_144,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("script definition is missing or has changed")]
    DefinitionChanged,
    #[error("script unit has no authored SCDA field")]
    MissingBody,
    #[error("script source metadata has unresolved findings: {0:?}")]
    SourceMetadata(Vec<String>),
    #[error(transparent)]
    Control(#[from] control_flow::Error),
    #[error("SCDA instruction 0x{instruction_offset:X}: {source}")]
    ExpressionEnvelope {
        instruction_offset: usize,
        source: expression::DecodeError,
    },
    #[error("SCDA instruction 0x{instruction_offset:X}: {source}")]
    ExpressionPlan {
        instruction_offset: usize,
        source: expression_plan::Error,
    },
    #[error("SCDA instruction 0x{0:X}: uninterpreted expression tail")]
    ExpressionTail(usize),
    #[error("source-plan budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Source(#[from] crate::Error),
    #[error("encoded operands contain unresolved source bindings or decode findings")]
    OperandFindings,
}

#[derive(Debug)]
pub struct Statement<'a> {
    instruction: usize,
    expression_scda_offset: usize,
    kind: expression::StatementKind,
    plan: expression_plan::Plan<'a>,
}
impl<'a> Statement<'a> {
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    pub fn expression_scda_offset(&self) -> usize {
        self.expression_scda_offset
    }
    pub fn kind(&self) -> &expression::StatementKind {
        &self.kind
    }
    pub fn plan(&self) -> &expression_plan::Plan<'a> {
        &self.plan
    }
}

/// All borrowed bytes come from the exact winning source version selected by a
/// checked handle. No mutable record, token vector or operand table is exposed.
pub struct Plan<'a> {
    source: &'a LoadedScript,
    cohort: &'a str,
    control: control_flow::Plan<'a>,
    statements: Vec<Statement<'a>>,
    bindings: operand_binding::Binding,
    tokens: usize,
    nodes: usize,
}
impl<'a> Plan<'a> {
    pub fn source(&self) -> &'a LoadedScript {
        self.source
    }
    pub fn handle(&self) -> &'a Handle {
        self.source.handle()
    }
    pub fn source_cohort_sha256(&self) -> &'a str {
        self.cohort
    }
    pub fn control(&self) -> &control_flow::Plan<'a> {
        &self.control
    }
    pub fn statements(&self) -> &[Statement<'a>] {
        &self.statements
    }
    pub fn bindings(&self) -> &operand_binding::Binding {
        &self.bindings
    }
    pub fn tokens(&self) -> usize {
        self.tokens
    }
    pub fn nodes(&self) -> usize {
        self.nodes
    }
    pub fn statement(&self, instruction: usize) -> Option<&Statement<'a>> {
        self.statements
            .binary_search_by_key(&instruction, |s| s.instruction)
            .ok()
            .map(|i| &self.statements[i])
    }
    /// Select an event by its exact source header offset. Event IDs alone are
    /// insufficient because a definition can contain multiple matching blocks.
    pub fn event_at_scda_offset(&self, offset: usize) -> Option<&control_flow::Event> {
        let instruction = self
            .control
            .instructions()
            .binary_search_by_key(&offset, |i| i.bytes.start)
            .ok()?;
        let event = self
            .control
            .events()
            .binary_search_by_key(&instruction, |e| e.begin_instruction)
            .ok()?;
        Some(&self.control.events()[event])
    }
}

pub fn prepare<'a>(
    catalogue: &'a Catalogue,
    handle: &Handle,
    model: &expression_plan::Model<'_>,
    signatures: &Signatures,
    limits: Limits,
) -> Result<Plan<'a>, Error> {
    let source = catalogue
        .get_handle(handle)
        .ok_or(Error::DefinitionChanged)?;
    if !source.issues().is_empty() {
        return Err(Error::SourceMetadata(source.issues().to_vec()));
    }
    let bytes = source.compiled().ok_or(Error::MissingBody)?;
    let control = control_flow::decode(bytes, limits.control)?;
    let mut statements = Vec::new();
    let mut tokens = 0;
    let mut nodes = 0;
    for (index, instruction) in control.instructions().iter().enumerate() {
        if matches!(instruction.opcode, 0x15 | 0x16 | 0x18)
            && statements.len() >= limits.maximum_expressions
        {
            return Err(Error::Capacity("expression statements"));
        }
        let mut expression_limits = limits.expression;
        expression_limits.decoding.max_tokens = expression_limits
            .decoding
            .max_tokens
            .min(limits.maximum_tokens.saturating_sub(tokens));
        expression_limits.max_nodes = expression_limits
            .max_nodes
            .min(limits.maximum_nodes.saturating_sub(nodes));
        let statement =
            expression::statement(instruction, model.operators(), expression_limits.decoding)
                .map_err(|source| Error::ExpressionEnvelope {
                    instruction_offset: instruction.bytes.start,
                    source,
                })?;
        let Some(statement) = statement else {
            continue;
        };
        if statements.len() >= limits.maximum_expressions {
            return Err(Error::Capacity("expression statements"));
        }
        if !statement.trailing.is_empty() {
            return Err(Error::ExpressionTail(instruction.bytes.start));
        }
        // The envelope already decoded this exact token stream. Reusing that
        // private view avoids reparsing it merely to construct its arena.
        let plan = expression_plan::from_decoded(statement.expression, expression_limits).map_err(
            |source| Error::ExpressionPlan {
                instruction_offset: instruction.bytes.start,
                source,
            },
        )?;
        if plan.tokens().len() > limits.maximum_tokens.saturating_sub(tokens) {
            return Err(Error::Capacity("expression tokens"));
        }
        if plan.nodes().len() > limits.maximum_nodes.saturating_sub(nodes) {
            return Err(Error::Capacity("expression nodes"));
        }
        tokens += plan.tokens().len();
        nodes += plan.nodes().len();
        statements.push(Statement {
            instruction: index,
            expression_scda_offset: instruction.operand_offset
                + statement.expression_operand_offset,
            kind: statement.kind,
            plan,
        });
    }
    let bindings = source
        .bind_operands(model.operators(), signatures, limits.maximum_operand_uses)?
        .ok_or(Error::MissingBody)?;
    if bindings.counts.missing_bindings != 0 || !bindings.decode_issues.is_empty() {
        return Err(Error::OperandFindings);
    }
    Ok(Plan {
        source,
        cohort: catalogue.winning_content_sha256(),
        control,
        statements,
        bindings,
        tokens,
        nodes,
    })
}
