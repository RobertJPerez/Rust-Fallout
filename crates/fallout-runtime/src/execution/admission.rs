//! Bounded admission diagnostics for caller-selected source roots. Declared
//! dependencies are source relationships, not a recovered retail call schedule.
use crate::{
    execution::native,
    programs::{LookupError, PreparedSources},
};
use fallout_data::{
    loaded_scripts::{Handle, ReferenceStatus, ScriptKey},
    obscript::{
        self, control_flow,
        definition_plan::{self, Plan},
    },
    quest_scripts::{self, Attachments, DeclarationStatus},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_roots: usize,
    pub maximum_definitions: usize,
    pub maximum_dependencies: usize,
    pub maximum_instructions: usize,
    /// Total cached operand uses visited, including own locals without edges.
    pub maximum_operand_uses: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_roots: 32,
            maximum_definitions: 256,
            maximum_dependencies: 4_096,
            maximum_instructions: 65_536,
            maximum_operand_uses: 65_536,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("execution admission budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(
        "execution admission operand-use budget exceeded at SCDA operand 0x{source_scda_offset:X} in {definition:?}"
    )]
    OperandUseBudget {
        definition: Box<Handle>,
        source_scda_offset: usize,
    },
    #[error("execution admission needs at least one exact source root")]
    EmptyRoots,
    #[error("execution admission has conflicting versions for one source key")]
    ConflictingVersion,
    #[error(transparent)]
    Source(#[from] LookupError),
    #[error("unsupported execution admission request schema {0}")]
    SchemaVersion(u32),
    #[error("execution admission request has a different source receipt cohort")]
    CohortChanged,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub source_cohort_sha256: String,
    pub roots: Vec<Handle>,
}
impl Request {
    pub fn check(
        &self,
        sources: &PreparedSources<'_>,
        attachments: &Attachments,
        limits: Limits,
    ) -> Result<Report, Error> {
        if self.schema_version != 1 {
            return Err(Error::SchemaVersion(self.schema_version));
        }
        if self.source_cohort_sha256 != sources.source_cohort_sha256() {
            return Err(Error::CohortChanged);
        }
        check(sources, attachments, &self.roots, limits)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    SourcePlanUnavailable,
    UnverifiedAssignment,
    UnverifiedControlFlow,
    UnverifiedReturn,
    UnverifiedStatement,
    MissingNativeHandler,
    UnverifiedNativeSemantics,
    UnverifiedEventLifecycle,
    ForeignContextUnavailable,
}
#[derive(Debug, Clone, Serialize)]
pub struct Unsupported {
    pub definition: Handle,
    pub source_scda_offset: Option<usize>,
    pub instruction_scda_bytes: Option<Range<usize>>,
    pub operand_scda_bytes: Option<Range<usize>>,
    pub opcode: Option<u16>,
    pub calling_reference_index: Option<u16>,
    pub code: Code,
    pub detail: String,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    DeclaredScriptReference,
    ForeignQuestDeclaration,
}
#[derive(Debug, Clone, Serialize)]
pub struct Dependency {
    pub from: Handle,
    pub to: Handle,
    pub kind: DependencyKind,
    pub operand_scda_offset: usize,
    pub reference_index: u16,
}
#[derive(Debug, Serialize)]
pub struct BackEdge {
    pub from: ScriptKey,
    pub to: ScriptKey,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub source_cohort_sha256: String,
    pub roots: Vec<Handle>,
    pub definitions: Vec<Handle>,
    pub dependencies: Vec<Dependency>,
    pub dependency_findings: Vec<Unsupported>,
    pub cycle_back_edges: Vec<BackEdge>,
    pub first_unsupported: Unsupported,
    pub faithful_execution_admitted: bool,
    pub retail_lifecycle_verified: bool,
}

fn operation(plan: &Plan<'_>) -> Unsupported {
    for instruction in plan.control().instructions() {
        // Delimiters locate the event; their lifecycle remains an independent
        // final gate even for a source that has no executable body operation.
        let (code, detail) = match instruction.kind() {
            obscript::Kind::Statement("begin" | "end") => continue,
            obscript::Kind::Statement("set_to") => (
                Code::UnverifiedAssignment,
                "Original operand evaluation, assignment and conversion are not measured",
            ),
            obscript::Kind::Statement("if" | "else_if" | "else" | "end_if") => (
                Code::UnverifiedControlFlow,
                "Original truth, evaluation order and branch successor semantics are not measured",
            ),
            obscript::Kind::Statement("return") => (
                Code::UnverifiedReturn,
                "Original return and event completion semantics are not measured",
            ),
            obscript::Kind::NativeCommand => {
                if native::capability(instruction.opcode).engineering_host_read {
                    (
                        Code::UnverifiedNativeSemantics,
                        "Engineering host reads do not implement the original native return",
                    )
                } else {
                    (
                        Code::MissingNativeHandler,
                        "The source native command has no replacement implementation",
                    )
                }
            }
            _ => (
                Code::UnverifiedStatement,
                "The source statement has no measured execution contract",
            ),
        };
        return Unsupported {
            definition: plan.handle().clone(),
            source_scda_offset: Some(instruction.bytes.start),
            instruction_scda_bytes: Some(instruction.bytes.clone()),
            operand_scda_bytes: Some(instruction.operand_offset..instruction.bytes.end),
            opcode: Some(instruction.opcode),
            calling_reference_index: instruction.calling_reference,
            code,
            detail: detail.into(),
        };
    }
    Unsupported {
        definition: plan.handle().clone(),
        source_scda_offset: plan.control().instructions().first().map(|i| i.bytes.start),
        instruction_scda_bytes: plan.control().instructions().first().map(|i| i.bytes.clone()),
        operand_scda_bytes: None, opcode: None, calling_reference_index: None,
        code: Code::UnverifiedEventLifecycle,
        detail: "A structural empty event does not establish original activation, timing or acknowledgment".into(),
    }
}

fn source_offset(error: &definition_plan::Error) -> Option<usize> {
    match error {
        definition_plan::Error::Control(control_flow::Error::Structure(issue)) => {
            Some(issue.instruction_scda_offset)
        }
        definition_plan::Error::Control(control_flow::Error::Decode(
            obscript::DecodeError::Truncated { offset, .. }
            | obscript::DecodeError::InstructionLimit { offset, .. }
            | obscript::DecodeError::ShortEvent { offset, .. },
        )) => Some(*offset),
        definition_plan::Error::ExpressionEnvelope {
            instruction_offset, ..
        }
        | definition_plan::Error::ExpressionPlan {
            instruction_offset, ..
        }
        | definition_plan::Error::ExpressionTail(instruction_offset) => Some(*instruction_offset),
        _ => None,
    }
}

fn back_edges(nodes: &[Handle], edges: &[Dependency]) -> Vec<BackEdge> {
    let mut adjacency = BTreeMap::<ScriptKey, Vec<ScriptKey>>::new();
    for edge in edges {
        adjacency
            .entry(edge.from.key.clone())
            .or_default()
            .push(edge.to.key.clone());
    }
    for next in adjacency.values_mut() {
        next.sort();
        next.dedup();
    }
    let mut colors = BTreeMap::<ScriptKey, u8>::new();
    let mut result = Vec::new();
    let mut ordered: Vec<_> = nodes.iter().map(|h| h.key.clone()).collect();
    ordered.sort();
    for root in ordered {
        if colors.contains_key(&root) {
            continue;
        }
        colors.insert(root.clone(), 1);
        let mut stack = vec![(root, 0)];
        while let Some((node, next_index)) = stack.last_mut() {
            let Some(next) = adjacency
                .get(node)
                .and_then(|next| next.get(*next_index))
                .cloned()
            else {
                colors.insert(node.clone(), 2);
                stack.pop();
                continue;
            };
            *next_index += 1;
            match colors.get(&next) {
                Some(1) => result.push(BackEdge {
                    from: node.clone(),
                    to: next,
                }),
                Some(_) => {}
                None => {
                    colors.insert(next.clone(), 1);
                    stack.push((next, 0));
                }
            }
        }
    }
    result
}

/// Root priority is canonical ScriptKey order, then breadth-first declared
/// dependencies in physical operand order. Operation priority is source header
/// order, which is explicitly not a claim about retail evaluation order.
pub fn check(
    sources: &PreparedSources<'_>,
    attachments: &Attachments,
    roots: &[Handle],
    limits: Limits,
) -> Result<Report, Error> {
    if roots.is_empty() {
        return Err(Error::EmptyRoots);
    }
    if roots.len() > limits.maximum_roots {
        return Err(Error::Capacity("roots"));
    }
    let catalogue = sources.catalogue();
    let mut roots = roots.to_vec();
    roots.sort_by(|left, right| left.key.cmp(&right.key));
    for pair in roots.windows(2) {
        if pair[0].key == pair[1].key && pair[0] != pair[1] {
            return Err(Error::ConflictingVersion);
        }
    }
    roots.dedup();
    let mut queue: VecDeque<_> = roots.iter().cloned().collect();
    let mut scheduled: BTreeSet<_> = roots.iter().map(|h| h.key.clone()).collect();
    if scheduled.len() > limits.maximum_definitions {
        return Err(Error::Capacity("definitions"));
    }
    let mut definitions = Vec::new();
    let mut dependencies = Vec::new();
    let mut dependency_findings = Vec::new();
    let mut first = None;
    let mut instructions = 0;
    let mut operand_uses = 0;
    while let Some(handle) = queue.pop_front() {
        // A stale requested version is a caller error, never a source capability
        // rejection silently substituted with the current winning version.
        if catalogue.get_handle(&handle).is_none() {
            return Err(LookupError::DefinitionChanged.into());
        }
        definitions.push(handle.clone());
        let prepared = match sources.get(&handle) {
            Ok(prepared) => prepared,
            Err(LookupError::Source(error)) => {
                first.get_or_insert(Unsupported {
                    definition: handle,
                    source_scda_offset: source_offset(&error),
                    instruction_scda_bytes: None,
                    operand_scda_bytes: None,
                    opcode: None,
                    calling_reference_index: None,
                    code: Code::SourcePlanUnavailable,
                    detail: error.to_string(),
                });
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let plan = prepared.plan();
        if plan.control().instructions().len()
            > limits.maximum_instructions.saturating_sub(instructions)
        {
            return Err(Error::Capacity("instructions"));
        }
        instructions += plan.control().instructions().len();
        let remaining = limits.maximum_operand_uses.saturating_sub(operand_uses);
        if let Some(excluded) = plan.bindings().uses.get(remaining) {
            return Err(Error::OperandUseBudget {
                definition: Box::new(handle),
                source_scda_offset: excluded.scda_offset,
            });
        }
        operand_uses = operand_uses
            .checked_add(plan.bindings().uses.len())
            .ok_or(Error::Capacity("operand uses"))?;
        first.get_or_insert_with(|| operation(plan));
        for use_ in &plan.bindings().uses {
            let mut targets = Vec::new();
            if use_.status == 4 {
                let context = use_.context_reference.expect("prepared foreign context");
                let lookup = quest_scripts::declaration(
                    catalogue,
                    attachments,
                    &handle,
                    context,
                    use_.index,
                );
                if lookup.status == DeclarationStatus::StaticQuestDeclaration {
                    if let Some(target) = lookup.target_script {
                        targets.push((target, DependencyKind::ForeignQuestDeclaration, context));
                    }
                } else {
                    if dependency_findings.len() >= limits.maximum_dependencies {
                        return Err(Error::Capacity("dependency findings"));
                    }
                    dependency_findings.push(Unsupported {
                        definition: handle.clone(), instruction_scda_bytes: None,
                        source_scda_offset: Some(use_.scda_offset),
                        operand_scda_bytes: Some(use_.scda_offset..use_.scda_offset + 2),
                        opcode: None, calling_reference_index: Some(context), code: Code::ForeignContextUnavailable,
                        detail: format!("Exact static foreign declaration request: {:?}; live context remains separate", lookup.status),
                    });
                }
            } else if use_.status == 2
                && let Some(reference) = plan.source().reference(u32::from(use_.index))
                && reference.status == ReferenceStatus::DefinedForm
                && reference
                    .target
                    .as_ref()
                    .is_some_and(|target| target.record_kind == "SCPT")
                && let Some(form) = &reference.form_key
            {
                for script in catalogue.record_scripts(form) {
                    if targets.len() >= limits.maximum_definitions {
                        return Err(Error::Capacity("referenced script units"));
                    }
                    targets.push((
                        script.handle().clone(),
                        DependencyKind::DeclaredScriptReference,
                        use_.index,
                    ));
                }
            }
            for (to, kind, reference_index) in targets {
                if dependencies.len() >= limits.maximum_dependencies {
                    return Err(Error::Capacity("dependencies"));
                }
                if !scheduled.contains(&to.key) {
                    if scheduled.len() >= limits.maximum_definitions {
                        return Err(Error::Capacity("definitions"));
                    }
                    scheduled.insert(to.key.clone());
                    queue.push_back(to.clone());
                }
                dependencies.push(Dependency {
                    from: handle.clone(),
                    to,
                    kind,
                    operand_scda_offset: use_.scda_offset,
                    reference_index,
                });
            }
        }
    }
    let cycle_back_edges = back_edges(&definitions, &dependencies);
    Ok(Report {
        source_cohort_sha256: sources.source_cohort_sha256().into(),
        roots,
        definitions,
        dependencies,
        dependency_findings,
        cycle_back_edges,
        first_unsupported: first.expect("nonempty checked roots"),
        faithful_execution_admitted: false,
        retail_lifecycle_verified: false,
    })
}
