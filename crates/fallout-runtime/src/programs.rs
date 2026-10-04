//! Immutable source plans prepared once, independently of live campaign banks.
//! Findings remain findings; a cached plan grants no execution permission.
use crate::{snapshot, state::World};
use fallout_data::{
    loaded_scripts::{Catalogue, Handle, LoadedScript},
    obscript::{
        self, argument_census::Signatures, arguments::Convention, control_flow, definition_plan,
        expression_plan::Model, operand_binding,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{iter::Peekable, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub source: definition_plan::Limits,
    pub maximum_definitions: usize,
    pub maximum_source_receipts: usize,
    pub maximum_parameters: usize,
    pub maximum_attempted_bytes: usize,
    pub maximum_attempted_record_bytes: usize,
    pub maximum_instructions: usize,
    pub maximum_expressions: usize,
    pub maximum_tokens: usize,
    pub maximum_nodes: usize,
    pub maximum_uses: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: definition_plan::Limits::default(),
            maximum_definitions: 262_144,
            maximum_source_receipts: 256,
            maximum_parameters: 1_000_000,
            maximum_attempted_bytes: 66 * 1024 * 1024,
            maximum_attempted_record_bytes: 512 * 1024 * 1024,
            maximum_instructions: 2_000_000,
            maximum_expressions: 262_144,
            maximum_tokens: 2_000_000,
            maximum_nodes: 2_000_000,
            maximum_uses: 1_000_000,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub definitions: usize,
    pub source_receipts: usize,
    pub absent_compiled_fields: usize,
    pub preparation_attempts: usize,
    pub prepared: usize,
    pub rejected: usize,
    pub attempted_source_bytes: usize,
    pub attempted_record_bytes: usize,
    pub instructions: usize,
    pub expressions: usize,
    pub tokens: usize,
    pub nodes: usize,
    pub uses: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("prepared-source catalogue budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("source preparation is incomplete: {remaining_definitions} definitions remain")]
    Incomplete { remaining_definitions: usize },
    #[error("explicit source selection contains a duplicate definition key")]
    DuplicateSelection,
    #[error(transparent)]
    Lookup(#[from] LookupError),
    #[error(transparent)]
    State(#[from] crate::Error),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LookupError {
    #[error("prepared-source catalogue does not match this world's complete source cohort")]
    ContentChanged,
    #[error("prepared script definition is missing or has changed")]
    DefinitionChanged,
    #[error("exact script definition was not selected for this prepared-source cache")]
    NotSelected,
    #[error(transparent)]
    Source(Arc<definition_plan::Error>),
}

pub struct PreparedDefinition<'a> {
    plan: definition_plan::Plan<'a>,
    binding_sha256: String,
}
impl<'a> PreparedDefinition<'a> {
    pub fn plan(&self) -> &definition_plan::Plan<'a> {
        &self.plan
    }
    pub fn binding_sha256(&self) -> &str {
        &self.binding_sha256
    }
}

enum Admission<'a> {
    Prepared(Box<PreparedDefinition<'a>>),
    Rejected(Arc<definition_plan::Error>),
}
struct Entry<'a> {
    source: &'a LoadedScript,
    admission: Admission<'a>,
}

/// All source borrows belong to the immutable input catalogue. Construction is
/// atomic; there is no mutable public cache or self-referential allocation.
/// Worlds may share it while retaining independent live values and journals.
pub struct PreparedSources<'a> {
    catalogue: &'a Catalogue,
    cohort: String,
    decoder_sha256: String,
    entries: Vec<Entry<'a>>,
    // Canonical borrowed sources in key order, including absent/rejected bodies.
    // None is the historical full-cache behavior; Some(empty) selects nothing.
    selection: Option<Vec<&'a LoadedScript>>,
    absent: Arc<definition_plan::Error>,
    counts: Counts,
}

/// Cooperative allowances for one call, in addition to the existing total and
/// per-definition limits. A definition is indivisible: an oversized next body
/// yields without decoding it. Zero allowances are valid and cannot busy-loop.
#[derive(Debug, Clone, Copy)]
pub struct StepBudget {
    pub maximum_definitions: usize,
    pub maximum_source_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationStatus {
    Pending,
    Complete,
    Failed,
}

/// Historical counters only. This grants neither cached-plan access nor
/// execution authority, even when status is complete. Consume `finish` first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub status: PreparationStatus,
    pub processed_definitions: usize,
    pub step_definitions: usize,
    pub step_source_bytes: usize,
    pub next_definition_bytes: Option<usize>,
    pub counts: Counts,
}

/// Privately accumulates the same immutable plans as `PreparedSources::load`.
/// Dropping this value cancels; failure/incompletion cannot expose its cache.
/// Setup checks/hashes the bounded catalogue and decoder up front. `advance`
/// provides boundaries between definitions, not a hard wall-clock time slice.
pub struct PreparationJob<'a, 'm, 'o> {
    sources: PreparedSources<'a>,
    model: &'m Model<'o>,
    signatures: &'m Signatures,
    limits: Limits,
    pending: Peekable<Box<dyn Iterator<Item = &'a LoadedScript> + 'a>>,
    status: PreparationStatus,
    processed: usize,
    failure: Option<Error>,
}

impl<'a, 'm, 'o> PreparationJob<'a, 'm, 'o> {
    pub fn new(
        catalogue: &'a Catalogue,
        model: &'m Model<'o>,
        signatures: &'m Signatures,
        limits: Limits,
    ) -> Result<Self, Error> {
        let sources = PreparedSources::initialize(catalogue, model, signatures, limits)?;
        let status = if sources.counts.definitions == 0 {
            PreparationStatus::Complete
        } else {
            PreparationStatus::Pending
        };
        let pending: Box<dyn Iterator<Item = &'a LoadedScript> + 'a> =
            Box::new(catalogue.iter().map(|(_, source)| source));
        Ok(Self {
            sources,
            model,
            signatures,
            limits,
            pending: pending.peekable(),
            status,
            processed: 0,
            failure: None,
        })
    }

    pub fn advance(&mut self, budget: StepBudget) -> Progress {
        let mut step_definitions = 0;
        let mut step_source_bytes = 0;
        while self.status == PreparationStatus::Pending {
            let Some(source) = self.pending.peek() else {
                self.status = PreparationStatus::Complete;
                break;
            };
            let bytes = source.compiled().map_or(0, <[u8]>::len);
            if step_definitions == budget.maximum_definitions
                || bytes > budget.maximum_source_bytes - step_source_bytes
            {
                break;
            }
            let source = self.pending.next().expect("peeked definition");
            step_definitions += 1;
            step_source_bytes += bytes;
            self.processed += 1;
            if let Err(error) =
                self.sources
                    .prepare_one(source, self.model, self.signatures, self.limits)
            {
                self.failure = Some(error);
                self.status = PreparationStatus::Failed;
            }
        }
        Progress {
            status: self.status,
            processed_definitions: self.processed,
            step_definitions,
            step_source_bytes,
            next_definition_bytes: if self.status == PreparationStatus::Pending {
                self.pending
                    .peek()
                    .map(|s| s.compiled().map_or(0, <[u8]>::len))
            } else {
                None
            },
            counts: self.sources.counts.clone(),
        }
    }

    pub fn finish(self) -> Result<PreparedSources<'a>, Error> {
        match self.status {
            PreparationStatus::Complete => Ok(self.sources),
            PreparationStatus::Failed => Err(self.failure.expect("failed preparation")),
            PreparationStatus::Pending => Err(Error::Incomplete {
                remaining_definitions: self.sources.counts.definitions - self.processed,
            }),
        }
    }
}

fn decoder_identity(
    model: &Model<'_>,
    signatures: &Signatures,
    maximum: usize,
) -> Result<String, Error> {
    let mut parameters = 0;
    let mut hash = Sha256::new();
    hash.update(b"FRPREPAREDDECODER1");
    hash.update(model.descriptor_sha256().as_bytes());
    hash.update((signatures.len() as u64).to_le_bytes());
    for (opcode, signature) in signatures {
        if signature.parameters.len() > maximum.saturating_sub(parameters) {
            return Err(Error::Capacity("native signature parameters"));
        }
        parameters += signature.parameters.len();
        hash.update(opcode.to_le_bytes());
        hash.update([match signature.convention {
            Convention::Default => 0,
            Convention::Message => 1,
            Convention::Unknown => 2,
        }]);
        hash.update((signature.parameters.len() as u64).to_le_bytes());
        for parameter in &signature.parameters {
            hash.update(parameter.type_id.to_le_bytes());
            hash.update(parameter.optional_word.to_le_bytes());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn resource_failure(error: &definition_plan::Error) -> bool {
    // Configured structural/preparation allowances abort construction. The
    // existing native/table readers also have fixed admission limits; their
    // diagnostic findings remain rejections and can never yield a cached plan.
    use definition_plan::Error as E;
    match error {
        E::Capacity(_) => true,
        E::Control(control_flow::Error::Decode(
            obscript::DecodeError::ByteLimit { .. }
            | obscript::DecodeError::InstructionLimit { .. },
        )) => true,
        E::Control(control_flow::Error::Structure(issue)) => issue.kind == "depth_budget",
        E::ExpressionEnvelope {
            source: obscript::expression::DecodeError::Limit { .. },
            ..
        } => true,
        E::ExpressionPlan {
            source: obscript::expression_plan::Error::Limit { .. },
            ..
        } => true,
        E::ExpressionPlan {
            source:
                obscript::expression_plan::Error::Decode(obscript::expression::DecodeError::Limit {
                    ..
                }),
            ..
        } => true,
        E::Source(fallout_data::Error::Unsupported(reason)) => {
            reason == "operand binding use budget exceeded"
        }
        _ => false,
    }
}

impl<'a> PreparedSources<'a> {
    pub fn load(
        catalogue: &'a Catalogue,
        model: &Model<'_>,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<Self, Error> {
        let mut job = PreparationJob::new(catalogue, model, signatures, limits)?;
        job.advance(StepBudget {
            maximum_definitions: usize::MAX,
            maximum_source_bytes: usize::MAX,
        });
        job.finish()
    }

    /// Prepare only explicitly selected exact versions, with no inferred closure.
    /// Validate every handle and reject duplicate keys before preparing anything.
    /// Full catalogue/decoder identity and the catalogue admission cap remain;
    /// attempt/plan/body counters and aggregate work allowances cover selection.
    pub fn load_selected(
        catalogue: &'a Catalogue,
        model: &Model<'_>,
        signatures: &Signatures,
        exact_handles: &[Handle],
        limits: Limits,
    ) -> Result<Self, Error> {
        if exact_handles.len() > limits.maximum_definitions {
            return Err(Error::Capacity("selected definitions"));
        }
        let mut sources = Self::initialize(catalogue, model, signatures, limits)?;
        let mut selection = Vec::with_capacity(exact_handles.len());
        for handle in exact_handles {
            selection.push(
                catalogue
                    .get_handle(handle)
                    .ok_or(LookupError::DefinitionChanged)?,
            );
        }
        selection.sort_by(|a, b| a.handle().key.cmp(&b.handle().key));
        if selection
            .windows(2)
            .any(|pair| pair[0].handle().key == pair[1].handle().key)
        {
            return Err(Error::DuplicateSelection);
        }
        for source in &selection {
            sources.prepare_one(source, model, signatures, limits)?;
        }
        sources.selection = Some(selection);
        Ok(sources)
    }

    fn initialize(
        catalogue: &'a Catalogue,
        model: &Model<'_>,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<Self, Error> {
        // Check the candidate bound before the cohort helper allocates its
        // ordered source identity projection or any cache entry is retained.
        let definitions = catalogue.iter().count();
        if definitions > limits.maximum_definitions {
            return Err(Error::Capacity("definitions"));
        }
        if catalogue.sources.len() > limits.maximum_source_receipts {
            return Err(Error::Capacity("source receipts"));
        }
        let decoder_sha256 = decoder_identity(model, signatures, limits.maximum_parameters)?;
        let cohort = snapshot::cohort(catalogue)?;
        Ok(Self {
            catalogue,
            cohort,
            decoder_sha256,
            entries: Vec::new(),
            selection: None,
            absent: Arc::new(definition_plan::Error::MissingBody),
            counts: Counts {
                definitions,
                source_receipts: catalogue.sources.len(),
                ..Counts::default()
            },
        })
    }

    fn prepare_one(
        &mut self,
        source: &'a LoadedScript,
        model: &Model<'_>,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<(), Error> {
        let counts = &mut self.counts;
        let catalogue = self.catalogue;
        if source.compiled().is_none() && source.issues().is_empty() {
            counts.absent_compiled_fields += 1;
            return Ok(());
        }
        let bytes = source.compiled().map_or(0, <[u8]>::len);
        if bytes
            > limits
                .maximum_attempted_bytes
                .saturating_sub(counts.attempted_source_bytes)
        {
            return Err(Error::Capacity("attempted source bytes"));
        }
        // A source rejection still consumed preparation work. It is charged
        // once and retained; repeated event lookups never decode it again.
        counts.attempted_source_bytes += bytes;
        let record_bytes = source.decoded_record_bytes();
        if record_bytes
            > limits
                .maximum_attempted_record_bytes
                .saturating_sub(counts.attempted_record_bytes)
        {
            return Err(Error::Capacity("attempted owning-record bytes"));
        }
        counts.attempted_record_bytes += record_bytes;
        counts.preparation_attempts += 1;
        let mut source_limits = limits.source;
        source_limits.control.decode.max_instructions =
            source_limits.control.decode.max_instructions.min(
                limits
                    .maximum_instructions
                    .saturating_sub(counts.instructions),
            );
        source_limits.maximum_expressions = source_limits.maximum_expressions.min(
            limits
                .maximum_expressions
                .saturating_sub(counts.expressions),
        );
        source_limits.maximum_tokens = source_limits
            .maximum_tokens
            .min(limits.maximum_tokens.saturating_sub(counts.tokens));
        source_limits.maximum_nodes = source_limits
            .maximum_nodes
            .min(limits.maximum_nodes.saturating_sub(counts.nodes));
        source_limits.maximum_operand_uses = source_limits
            .maximum_operand_uses
            .min(limits.maximum_uses.saturating_sub(counts.uses));
        source_limits.expression.decoding.max_tokens = source_limits
            .expression
            .decoding
            .max_tokens
            .min(source_limits.maximum_tokens);
        source_limits.expression.max_nodes = source_limits
            .expression
            .max_nodes
            .min(source_limits.maximum_nodes);
        let admission = match definition_plan::prepare(
            catalogue,
            source.handle(),
            model,
            signatures,
            source_limits,
        ) {
            Ok(plan) => {
                counts.prepared += 1;
                counts.instructions += plan.control().instructions().len();
                counts.expressions += plan.statements().len();
                counts.tokens += plan.tokens();
                counts.nodes += plan.nodes();
                counts.uses += plan.bindings().uses.len();
                let binding_sha256 = operand_binding::digest(&plan.bindings().uses);
                Admission::Prepared(Box::new(PreparedDefinition {
                    plan,
                    binding_sha256,
                }))
            }
            Err(error) if resource_failure(&error) => {
                // Do not mislabel a caller's exhausted aggregate allowance
                // as an intrinsic source finding, or publish a partial cache.
                return Err(Error::Capacity("source preparation"));
            }
            Err(error) => {
                counts.rejected += 1;
                Admission::Rejected(Arc::new(error))
            }
        };
        self.entries.push(Entry { source, admission });
        Ok(())
    }

    pub fn source_cohort_sha256(&self) -> &str {
        &self.cohort
    }
    /// The immutable catalogue that owns every cached source plan and table.
    pub fn catalogue(&self) -> &'a Catalogue {
        self.catalogue
    }
    pub fn decoder_sha256(&self) -> &str {
        &self.decoder_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn validate_world(&self, world: &World<'_>) -> Result<(), LookupError> {
        if self.cohort != world.catalogue_fingerprint() {
            return Err(LookupError::ContentChanged);
        }
        Ok(())
    }
    pub fn get(&self, handle: &Handle) -> Result<&PreparedDefinition<'a>, LookupError> {
        let source = self
            .catalogue
            .get_handle(handle)
            .ok_or(LookupError::DefinitionChanged)?;
        if self.selection.as_ref().is_some_and(|selection| {
            selection
                .binary_search_by(|source| source.handle().key.cmp(&handle.key))
                .is_err()
        }) {
            return Err(LookupError::NotSelected);
        }
        if source.compiled().is_none() && source.issues().is_empty() {
            return Err(LookupError::Source(Arc::clone(&self.absent)));
        }
        let index = self
            .entries
            .binary_search_by(|entry| entry.source.handle().key.cmp(&handle.key))
            .map_err(|_| LookupError::DefinitionChanged)?;
        match &self.entries[index].admission {
            Admission::Prepared(prepared) => Ok(prepared),
            Admission::Rejected(error) => Err(LookupError::Source(Arc::clone(error))),
        }
    }
}
