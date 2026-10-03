//! Inspect the live values associated with one exact authored event window.
//! A resolved source operand does not certify native argument or VM semantics.

use crate::{
    World,
    events::Pending,
    foreign::{self, Content},
    identity::{CampaignId, InstanceId, ReferenceId, ReferenceValue, Value},
    preparation,
    programs::PreparedSources,
    schema::{Kind, Local},
};
use fallout_data::{
    loaded_scripts::Handle,
    obscript::{argument_census::Signatures, expression_plan::Model, operand_binding::Use},
};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub preparation: preparation::Limits,
    pub maximum_uses: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct CachedLimits {
    pub maximum_event_instructions: usize,
    pub maximum_uses: usize,
}
impl Default for CachedLimits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 262_144,
            maximum_uses: 262_144,
        }
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            preparation: preparation::Limits::default(),
            maximum_uses: 262_144,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Content(#[from] foreign::Failure),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error("event operand probe budget exceeded")]
    Capacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Destination,
    Reference,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resolution {
    Local {
        instance: InstanceId,
        declaration: Local,
        value: Option<Value>,
    },
    Foreign {
        target: foreign::Target,
        value: Option<Value>,
    },
    Reference {
        value: ReferenceValue,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Resolved {
        access: Access,
        resolution: Resolution,
    },
    Unresolved {
        code: String,
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Operand {
    pub binding: Use,
    pub outcome: Outcome,
}

/// Owned diagnostic observations, never a write permit or a saved continuation.
/// A later execution step must resolve again against the then-current state.
#[derive(Debug, Serialize)]
pub struct Probe {
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub catalogue_sha256: String,
    pub pending: Pending,
    pub definition: Handle,
    pub full_definition_binding_sha256: String,
    pub begin_scda_offset: usize,
    pub end_scda_offset: usize,
    pub operands: Vec<Operand>,
}

fn unresolved(code: &str, reason: impl ToString) -> Outcome {
    Outcome::Unresolved {
        code: code.into(),
        reason: reason.to_string(),
    }
}
fn local_failure(error: crate::Error) -> Outcome {
    let code = match &error {
        crate::Error::MissingLocal(_) => "missing_local",
        crate::Error::UninitializedLocal(_) => "uninitialized_local",
        crate::Error::IncompatibleLocal(_) => "incompatible_local",
        crate::Error::UnsupportedLocal(_) => "unsupported_local",
        crate::Error::UnresolvedDependency(_) => "unresolved_context_reference",
        crate::Error::MissingReference => "missing_live_reference",
        _ => "invalid_runtime_state",
    };
    unresolved(code, error)
}
fn supported(declaration: &Local) -> crate::Result<()> {
    match declaration.kind {
        Kind::Float | Kind::Integer | Kind::Reference => Ok(()),
        _ => Err(crate::Error::UnsupportedLocal(declaration.index)),
    }
}

impl World<'_> {
    pub fn probe_event_operands(
        &self,
        sequence: u64,
        model: &Model<'_>,
        signatures: &Signatures,
        content: &Content,
        player: Option<ReferenceId>,
        limits: Limits,
    ) -> Result<Probe, Error> {
        content.validate_world(self)?;
        let frame = self.prepare_event(sequence, model, signatures, limits.preparation)?;
        self.probe_prepared_operands(frame, content, player, limits.maximum_uses)
    }

    pub fn probe_event_operands_with_sources(
        &self,
        sequence: u64,
        sources: &PreparedSources<'_>,
        content: &Content,
        player: Option<ReferenceId>,
        limits: CachedLimits,
    ) -> Result<Probe, Error> {
        content.validate_world(self)?;
        let frame =
            self.prepare_event_with_sources(sequence, sources, limits.maximum_event_instructions)?;
        self.probe_prepared_operands(frame, content, player, limits.maximum_uses)
    }

    fn probe_prepared_operands(
        &self,
        frame: preparation::PreparedEvent<'_>,
        content: &Content,
        player: Option<ReferenceId>,
        maximum_uses: usize,
    ) -> Result<Probe, Error> {
        let instructions = frame.instructions();
        let begin = instructions
            .first()
            .expect("checked event begin")
            .bytes
            .start;
        let end = instructions.last().expect("checked event end").bytes.end;
        let handle = self.handle(frame.instance().id())?;
        let mut operands = Vec::new();
        for binding in &frame.source().bindings().uses {
            if !(begin..end).contains(&binding.scda_offset) {
                continue;
            }
            if operands.len() >= maximum_uses {
                return Err(Error::Capacity);
            }
            // Keep binder occurrence order, including repeated uses. Sorting or
            // deduplicating would change the source association being inspected.
            let outcome = self.probe_bound_operand(handle, content, player, binding);
            operands.push(Operand {
                binding: binding.clone(),
                outcome,
            });
        }
        Ok(Probe {
            campaign: self.campaign(),
            state_revision: self.revision,
            catalogue_sha256: self.catalogue_fingerprint().into(),
            pending: frame.pending().clone(),
            definition: frame.source().handle().clone(),
            full_definition_binding_sha256: frame.binding_sha256().into_owned(),
            begin_scda_offset: begin,
            end_scda_offset: end,
            operands,
        })
    }

    fn probe_bound_operand(
        &self,
        handle: crate::state::InstanceHandle,
        content: &Content,
        player: Option<ReferenceId>,
        binding: &Use,
    ) -> Outcome {
        match binding.role {
            // A global's source reference is not its live numeric value. Global
            // storage and conversions need their own runtime contract.
            3 | 5 | 9 => unresolved(
                "unverified_global_value",
                "Global reads and destinations are not implemented",
            ),
            1 | 6 | 7 | 10 | 12 => {
                match self.resolve_script_reference(handle, u32::from(binding.index), player) {
                    Ok(value) => Outcome::Resolved {
                        access: Access::Reference,
                        resolution: Resolution::Reference { value },
                    },
                    Err(error) => local_failure(error),
                }
            }
            2 | 4 | 8 | 11 => {
                let access = if binding.role == 2 {
                    Access::Destination
                } else {
                    Access::Read
                };
                if let Some(context_reference) = binding.context_reference {
                    let request = foreign::Request {
                        source: handle,
                        context_reference,
                        local_index: binding.index,
                        player,
                    };
                    let target = match self.foreign_target(content, request) {
                        Ok(target) => target,
                        Err(error) => return unresolved(error.code(), error),
                    };
                    if let Err(error) = supported(&target.declaration) {
                        return local_failure(error);
                    }
                    let value = if access == Access::Destination {
                        // An uninitialized local is still a valid destination.
                        // Reading it here would wrongly reject a first assignment.
                        None
                    } else {
                        match self.read_foreign(content, request) {
                            Ok(read) => Some(read.value),
                            Err(error) => return unresolved(error.code(), error),
                        }
                    };
                    Outcome::Resolved {
                        access,
                        resolution: Resolution::Foreign { target, value },
                    }
                } else {
                    let instance = match self.instance(handle) {
                        Ok(instance) => instance,
                        Err(error) => return local_failure(error),
                    };
                    let index = u32::from(binding.index);
                    let Some(declaration) = instance.definition_schema.locals.get(&index) else {
                        return local_failure(crate::Error::MissingLocal(index));
                    };
                    if let Err(error) = supported(declaration) {
                        return local_failure(error);
                    }
                    let value = if access == Access::Destination {
                        None
                    } else {
                        match instance.local(index) {
                            Ok(value)
                                if binding.role != 11
                                    || matches!(value, Value::Reference { .. }) =>
                            {
                                Some(value.clone())
                            }
                            Ok(_) => return local_failure(crate::Error::IncompatibleLocal(index)),
                            Err(error) => return local_failure(error),
                        }
                    };
                    Outcome::Resolved {
                        access,
                        resolution: Resolution::Local {
                            instance: instance.id(),
                            declaration: declaration.clone(),
                            value,
                        },
                    }
                }
            }
            _ => unresolved(
                "unsupported_operand_role",
                format!("Source binder role {} is unsupported", binding.role),
            ),
        }
    }
}
