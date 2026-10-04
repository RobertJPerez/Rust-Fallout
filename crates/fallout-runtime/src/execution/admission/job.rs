//! Private progress over already cached source plans; never live execution.
use super::*;
use std::{
    collections::{BTreeSet, VecDeque},
    fmt::{self, Write},
};

#[derive(Debug, Clone, Copy)]
pub struct JobLimits {
    pub maximum_frontier: usize,
    /// Cumulative admitted variable copies/reservations, including temporary
    /// existing declaration lookups. This is not peak allocator accounting.
    pub maximum_variable_bytes: usize,
    /// Conservatively charge the complete receipt list for each foreign lookup.
    pub maximum_lookup_source_visits: usize,
    pub maximum_indivisible_instructions: usize,
}
impl Default for JobLimits {
    fn default() -> Self {
        Self {
            maximum_frontier: 256,
            maximum_variable_bytes: 2 * 1024 * 1024,
            maximum_lookup_source_visits: 16 * 1024 * 1024,
            maximum_indivisible_instructions: 65_536,
        }
    }
}
impl JobLimits {
    // Preserve historical check's public five limits and accepted reports.
    // Only the explicitly bounded job/CLI adds these independent ceilings.
    pub(super) fn legacy(limits: Limits) -> Self {
        Self {
            maximum_frontier: limits.maximum_definitions,
            maximum_variable_bytes: usize::MAX,
            maximum_lookup_source_visits: usize::MAX,
            maximum_indivisible_instructions: usize::MAX,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct StepBudget {
    pub maximum_definition_expansions: usize,
    pub maximum_operand_visits: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionStatus {
    Pending,
    Complete,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub status: AdmissionStatus,
    pub step_definition_expansions: usize,
    pub step_operand_visits: usize,
    pub definition_expansions: usize,
    pub operand_visits: usize,
    pub charged_instructions: usize,
    pub charged_operand_uses: usize,
    pub dependencies: usize,
    pub dependency_findings: usize,
    pub frontier_definitions: usize,
    pub variable_bytes: usize,
    pub lookup_source_visits: usize,
}
pub(super) struct Variables {
    maximum: usize,
    pub(super) bytes: usize,
}
impl Variables {
    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|&n| n <= self.maximum)
            .ok_or(Error::Capacity("variable bytes"))?;
        Ok(())
    }
    fn formatted(&mut self, arguments: fmt::Arguments<'_>) -> Result<String, Error> {
        struct Counter {
            bytes: usize,
            maximum: usize,
        }
        impl Write for Counter {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                self.bytes = self
                    .bytes
                    .checked_add(text.len())
                    .filter(|&n| n <= self.maximum)
                    .ok_or(fmt::Error)?;
                Ok(())
            }
        }
        let mut count = Counter {
            bytes: 0,
            maximum: self.maximum - self.bytes,
        };
        fmt::write(&mut count, arguments).map_err(|_| Error::Capacity("variable bytes"))?;
        self.charge(count.bytes)?;
        Ok(fmt::format(arguments))
    }
}
pub(super) fn handle_bytes(handle: &Handle) -> Result<usize, Error> {
    handle
        .key
        .record
        .origin_plugin
        .len()
        .checked_add(handle.version_sha256.len())
        .ok_or(Error::Capacity("variable bytes"))
}
struct Current<'a, 'source> {
    handle: &'a Handle,
    plan: &'a Plan<'source>,
    next_operand: usize,
}
/// Dropping cancels without a report. The only retained authority is a borrow
/// of immutable cached source; progress and finished reports grant no execution.
pub struct AdmissionJob<'a, 'source> {
    sources: &'a PreparedSources<'source>,
    attachments: &'a Attachments,
    roots: Vec<Handle>,
    queue: VecDeque<&'a Handle>,
    scheduled: BTreeSet<&'a ScriptKey>,
    current: Option<Current<'a, 'source>>,
    definitions: Vec<Handle>,
    dependencies: Vec<Dependency>,
    findings: Vec<Unsupported>,
    first: Option<Unsupported>,
    cycles: Vec<BackEdge>,
    limits: Limits,
    job_limits: JobLimits,
    variables: Variables,
    status: AdmissionStatus,
    failure: Option<Error>,
    expansions: usize,
    visits: usize,
    instructions: usize,
    operand_uses: usize,
    lookup_visits: usize,
}
impl<'a, 'source> AdmissionJob<'a, 'source> {
    pub fn new(
        sources: &'a PreparedSources<'source>,
        attachments: &'a Attachments,
        roots: &'a [Handle],
        limits: Limits,
        job_limits: JobLimits,
    ) -> Result<Self, Error> {
        if roots.is_empty() {
            return Err(Error::EmptyRoots);
        }
        if roots.len() > limits.maximum_roots {
            return Err(Error::Capacity("roots"));
        }
        let mut variables = Variables {
            maximum: job_limits.maximum_variable_bytes,
            bytes: 0,
        };
        // Admit raw root strings before comparisons; canonical output copies
        // are charged separately. Duplicate input does not evade setup work.
        for root in roots {
            variables.charge(handle_bytes(root)?)?;
        }
        let mut ordered: Vec<_> = roots.iter().collect();
        ordered.sort_by(|a, b| a.key.cmp(&b.key));
        for pair in ordered.windows(2) {
            if pair[0].key == pair[1].key && pair[0] != pair[1] {
                return Err(Error::ConflictingVersion);
            }
        }
        ordered.dedup();
        if ordered.len() > limits.maximum_definitions {
            return Err(Error::Capacity("definitions"));
        }
        if ordered.len() > job_limits.maximum_frontier {
            return Err(Error::Capacity("frontier"));
        }
        variables.charge(sources.source_cohort_sha256().len())?;
        let mut copied_roots = Vec::with_capacity(ordered.len());
        for root in &ordered {
            variables.charge(handle_bytes(root)?)?;
            copied_roots.push((*root).clone());
        }
        let scheduled = ordered.iter().map(|h| &h.key).collect();
        Ok(Self {
            sources,
            attachments,
            roots: copied_roots,
            queue: ordered.into(),
            scheduled,
            current: None,
            definitions: Vec::new(),
            dependencies: Vec::new(),
            findings: Vec::new(),
            first: None,
            cycles: Vec::new(),
            limits,
            job_limits,
            variables,
            status: AdmissionStatus::Pending,
            failure: None,
            expansions: 0,
            visits: 0,
            instructions: 0,
            operand_uses: 0,
            lookup_visits: 0,
        })
    }
    pub fn progress(&self) -> Progress {
        self.observation(0, 0)
    }
    fn observation(&self, expanded: usize, visited: usize) -> Progress {
        Progress {
            status: self.status,
            step_definition_expansions: expanded,
            step_operand_visits: visited,
            definition_expansions: self.expansions,
            operand_visits: self.visits,
            charged_instructions: self.instructions,
            charged_operand_uses: self.operand_uses,
            dependencies: self.dependencies.len(),
            dependency_findings: self.findings.len(),
            frontier_definitions: self.queue.len(),
            variable_bytes: self.variables.bytes,
            lookup_source_visits: self.lookup_visits,
        }
    }
    fn open(&mut self, handle: &'a Handle) -> Result<(), Error> {
        if self.sources.catalogue().get_handle(handle).is_none() {
            return Err(LookupError::DefinitionChanged.into());
        }
        self.variables.charge(handle_bytes(handle)?)?;
        self.definitions.push(handle.clone());
        let prepared = match self.sources.get(handle) {
            Ok(prepared) => prepared,
            Err(LookupError::Source(error)) => {
                if self.first.is_none() {
                    self.variables.charge(handle_bytes(handle)?)?;
                    let detail = self.variables.formatted(format_args!("{error}"))?;
                    self.first = Some(Unsupported {
                        definition: handle.clone(),
                        source_scda_offset: source_offset(&error),
                        instruction_scda_bytes: None,
                        operand_scda_bytes: None,
                        opcode: None,
                        calling_reference_index: None,
                        code: Code::SourcePlanUnavailable,
                        detail,
                    });
                }
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let plan = prepared.plan();
        let instructions = plan.control().instructions().len();
        if instructions
            > self
                .limits
                .maximum_instructions
                .saturating_sub(self.instructions)
        {
            return Err(Error::Capacity("instructions"));
        }
        if instructions > self.job_limits.maximum_indivisible_instructions {
            return Err(Error::Capacity("indivisible instructions"));
        }
        self.instructions += instructions;
        // Keep historical per-definition aggregate rejection priority and its
        // exact first excluded site. Actual visits still resume independently.
        let remaining = self
            .limits
            .maximum_operand_uses
            .saturating_sub(self.operand_uses);
        if let Some(excluded) = plan.bindings().uses.get(remaining) {
            self.variables.charge(handle_bytes(handle)?)?;
            return Err(Error::OperandUseBudget {
                definition: Box::new(handle.clone()),
                source_scda_offset: excluded.scda_offset,
            });
        }
        self.operand_uses = self
            .operand_uses
            .checked_add(plan.bindings().uses.len())
            .ok_or(Error::Capacity("operand uses"))?;
        if self.first.is_none() {
            self.first = Some(operation(plan, &mut self.variables)?);
        }
        self.current = Some(Current {
            handle,
            plan,
            next_operand: 0,
        });
        Ok(())
    }
    fn edge(
        &mut self,
        from: &Handle,
        to: &'a Handle,
        kind: DependencyKind,
        offset: usize,
        reference_index: u16,
    ) -> Result<(), Error> {
        if self.dependencies.len() >= self.limits.maximum_dependencies {
            return Err(Error::Capacity("dependencies"));
        }
        if !self.scheduled.contains(&to.key) {
            if self.scheduled.len() >= self.limits.maximum_definitions {
                return Err(Error::Capacity("definitions"));
            }
            if self.queue.len() >= self.job_limits.maximum_frontier {
                return Err(Error::Capacity("frontier"));
            }
        }
        self.variables.charge(handle_bytes(from)?)?;
        self.variables.charge(handle_bytes(to)?)?;
        if self.scheduled.insert(&to.key) {
            self.queue.push_back(to);
        }
        self.dependencies.push(Dependency {
            from: from.clone(),
            to: to.clone(),
            kind,
            operand_scda_offset: offset,
            reference_index,
        });
        Ok(())
    }
    fn visit(&mut self, current: &Current<'a, 'source>, index: usize) -> Result<(), Error> {
        let use_ = &current.plan.bindings().uses[index];
        let catalogue = self.sources.catalogue();
        if use_.status == 4 {
            let context = use_.context_reference.expect("prepared foreign context");
            self.lookup_visits = self
                .lookup_visits
                .checked_add(catalogue.sources.len())
                .filter(|&n| n <= self.job_limits.maximum_lookup_source_visits)
                .ok_or(Error::Capacity("lookup source visits"))?;
            // Existing declaration() creates a diagnostic source key/context
            // key/target handle/declaration digest. Admit their borrowed extents
            // first, including copies it may discard on a narrower rejection.
            self.variables
                .charge(current.handle.key.record.origin_plugin.len())?;
            if let Some(form) = current
                .plan
                .source()
                .reference(u32::from(context))
                .and_then(|r| r.form_key.as_ref())
            {
                self.variables.charge(form.origin_plugin.len())?;
                if let Some(target) = self.attachments.get(form).and_then(|a| a.script.as_ref()) {
                    self.variables.charge(handle_bytes(target)?)?;
                    if let Some(declaration) = catalogue
                        .get_handle(target)
                        .and_then(|s| s.declaration(u32::from(use_.index)))
                    {
                        self.variables.charge(declaration.name_sha256.len())?;
                    }
                }
            }
            let lookup = quest_scripts::declaration(
                catalogue,
                self.attachments,
                current.handle,
                context,
                use_.index,
            );
            if lookup.status == DeclarationStatus::StaticQuestDeclaration {
                if let Some(target) = lookup.target_script {
                    let target = catalogue
                        .get_handle(&target)
                        .ok_or(LookupError::DefinitionChanged)?
                        .handle();
                    self.edge(
                        current.handle,
                        target,
                        DependencyKind::ForeignQuestDeclaration,
                        use_.scda_offset,
                        context,
                    )?;
                }
            } else {
                if self.findings.len() >= self.limits.maximum_dependencies {
                    return Err(Error::Capacity("dependency findings"));
                }
                self.variables.charge(handle_bytes(current.handle)?)?;
                let detail = self.variables.formatted(format_args!(
                    "Exact static foreign declaration request: {:?}; live context remains separate",
                    lookup.status
                ))?;
                self.findings.push(Unsupported {
                    definition: current.handle.clone(),
                    instruction_scda_bytes: None,
                    source_scda_offset: Some(use_.scda_offset),
                    operand_scda_bytes: Some(use_.scda_offset..use_.scda_offset + 2),
                    opcode: None,
                    calling_reference_index: Some(context),
                    code: Code::ForeignContextUnavailable,
                    detail,
                });
            }
        } else if use_.status == 2
            && let Some(reference) = current.plan.source().reference(u32::from(use_.index))
            && reference.status == ReferenceStatus::DefinedForm
            && reference
                .target
                .as_ref()
                .is_some_and(|t| t.record_kind == "SCPT")
            && let Some(form) = &reference.form_key
        {
            // record_scripts() builds two owning range keys. It remains the
            // sole catalogue lookup; no alternate relationship importer.
            self.variables.charge(form.origin_plugin.len())?;
            self.variables.charge(form.origin_plugin.len())?;
            let mut targets = Vec::new();
            for script in catalogue.record_scripts(form) {
                if targets.len() >= self.limits.maximum_definitions {
                    return Err(Error::Capacity("referenced script units"));
                }
                targets.push(script.handle());
            }
            for target in targets {
                self.edge(
                    current.handle,
                    target,
                    DependencyKind::DeclaredScriptReference,
                    use_.scda_offset,
                    use_.index,
                )?;
            }
        }
        Ok(())
    }
    pub fn advance(&mut self, budget: StepBudget) -> Progress {
        let mut expanded = 0;
        let mut visited = 0;
        while self.status == AdmissionStatus::Pending {
            let result = if let Some(mut current) = self.current.take() {
                if current.next_operand == current.plan.bindings().uses.len() {
                    continue;
                }
                if visited == budget.maximum_operand_visits {
                    self.current = Some(current);
                    break;
                }
                let index = current.next_operand;
                current.next_operand += 1;
                visited += 1;
                self.visits += 1;
                let result = self.visit(&current, index);
                self.current = Some(current);
                result
            } else if self.queue.is_empty() {
                // One bounded finalization unit: at most the already admitted
                // definition/edge rows, no recursion or source decoding.
                match back_edges(&self.definitions, &self.dependencies, &mut self.variables) {
                    Ok(cycles) => {
                        self.cycles = cycles;
                        self.status = AdmissionStatus::Complete;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                if expanded == budget.maximum_definition_expansions {
                    break;
                }
                let handle = self.queue.pop_front().expect("checked frontier");
                expanded += 1;
                self.expansions += 1;
                self.open(handle)
            };
            if let Err(error) = result {
                self.failure = Some(error);
                self.status = AdmissionStatus::Failed;
            }
        }
        self.observation(expanded, visited)
    }
    pub fn finish(self) -> Result<Report, Error> {
        match self.status {
            AdmissionStatus::Pending => Err(Error::Incomplete),
            AdmissionStatus::Failed => Err(self.failure.expect("failed admission")),
            AdmissionStatus::Complete => Ok(Report {
                source_cohort_sha256: self.sources.source_cohort_sha256().into(),
                roots: self.roots,
                definitions: self.definitions,
                dependencies: self.dependencies,
                dependency_findings: self.findings,
                cycle_back_edges: self.cycles,
                first_unsupported: self.first.expect("nonempty complete roots"),
                faithful_execution_admitted: false,
                retail_lifecycle_verified: false,
            }),
        }
    }
}
