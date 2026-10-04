//! Read-only availability of explicitly requested canonical inputs. This report
//! grants no execution authority and establishes no scene, physics or gameplay
//! readiness. Observations describe only their reported canonical revision.
use super::World;
use crate::{
    Error, Result,
    identity::{CampaignId, InstanceId, Owner, ReferenceId, valid_form},
};
use fallout_data::loaded_scripts::Handle;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, mem::size_of};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceRequirement {
    pub owner: Owner,
    pub definition: Handle,
    pub instance: InstanceId,
}

/// An optional journal check observes identity only, not trigger/context data.
/// `None` in HostRequirements skips the check; Empty explicitly requires no head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalHead {
    Empty,
    Event { sequence: u64, instance: InstanceId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRequirements {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub reference_states: Vec<ReferenceId>,
    pub inventory_owners: Vec<ReferenceId>,
    pub instances: Vec<InstanceRequirement>,
    pub expected_journal_head: Option<JournalHead>,
}

#[derive(Debug, Clone, Copy)]
pub struct HostLimits {
    pub max_requirements: usize,
    pub max_unavailable: usize,
    /// Conservative fixed/table/UTF8 charge for the owned report and temporary
    /// duplicate/identity validation work. Caller-owned request storage, tree
    /// allocator overhead and serialization buffers are separate.
    pub max_copied_bytes: usize,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_requirements: 4096,
            max_unavailable: 4096,
            max_copied_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentUnavailable {
    MissingReference,
    Uninitialized,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstanceUnavailable {
    DefinitionUnavailable,
    OwnerReferenceMissing,
    MissingOwner,
    InstanceMismatch { actual: InstanceId },
    DefinitionMismatch { actual: Handle },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostUnavailable {
    ReferenceState {
        index: usize,
        reference: ReferenceId,
        reason: ComponentUnavailable,
    },
    Inventory {
        index: usize,
        owner: ReferenceId,
        reason: ComponentUnavailable,
    },
    Instance {
        index: usize,
        expected: InstanceRequirement,
        reason: InstanceUnavailable,
    },
    JournalHead {
        expected: JournalHead,
        actual: JournalHead,
    },
}

/// A report is an observation, never a mutation ticket or boot permission.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct HostReadiness {
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    requested_requirements: usize,
    charged_bytes: usize,
    unavailable: Vec<HostUnavailable>,
}
impl HostReadiness {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn requested_requirements(&self) -> usize {
        self.requested_requirements
    }
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
    pub fn canonical_data_available(&self) -> bool {
        self.unavailable.is_empty()
    }
    pub fn unavailable(&self) -> &[HostUnavailable] {
        &self.unavailable
    }
    pub fn first_unavailable(&self) -> Option<&HostUnavailable> {
        self.unavailable.first()
    }
}

enum InstanceProblem<'a> {
    DefinitionUnavailable,
    OwnerReferenceMissing,
    MissingOwner,
    InstanceMismatch(InstanceId),
    DefinitionMismatch(&'a Handle),
}
enum Missing<'a> {
    Reference(usize, ReferenceId, ComponentUnavailable),
    Inventory(usize, ReferenceId, ComponentUnavailable),
    Instance(usize, &'a InstanceRequirement, InstanceProblem<'a>),
    Journal(&'a JournalHead, JournalHead),
}
fn owner_bytes(owner: &Owner) -> usize {
    match owner {
        Owner::Quest { key } => key.origin_plugin.len(),
        _ => 0,
    }
}
fn definition_bytes(definition: &Handle) -> Result<usize> {
    definition
        .key
        .record
        .origin_plugin
        .len()
        .checked_add(definition.version_sha256.len())
        .ok_or(Error::Capacity("host readiness copied bytes"))
}
fn charge(total: &mut usize, count: usize, each: usize) -> Result<()> {
    *total = total
        .checked_add(
            count
                .checked_mul(each)
                .ok_or(Error::Capacity("host readiness copied bytes"))?,
        )
        .ok_or(Error::Capacity("host readiness copied bytes"))?;
    Ok(())
}
fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Missing<'_> {
    fn charge(&self, total: &mut usize) -> Result<()> {
        charge(total, 1, size_of::<HostUnavailable>())?;
        if let Self::Instance(_, expected, problem) = self {
            charge(total, 1, owner_bytes(&expected.owner))?;
            charge(total, 1, definition_bytes(&expected.definition)?)?;
            if let InstanceProblem::DefinitionMismatch(actual) = problem {
                charge(total, 1, definition_bytes(actual)?)?;
            }
        }
        Ok(())
    }
    fn into_owned(self) -> HostUnavailable {
        match self {
            Self::Reference(index, reference, reason) => HostUnavailable::ReferenceState {
                index,
                reference,
                reason,
            },
            Self::Inventory(index, owner, reason) => HostUnavailable::Inventory {
                index,
                owner,
                reason,
            },
            Self::Journal(expected, actual) => HostUnavailable::JournalHead {
                expected: expected.clone(),
                actual,
            },
            Self::Instance(index, expected, problem) => HostUnavailable::Instance {
                index,
                expected: expected.clone(),
                reason: match problem {
                    InstanceProblem::DefinitionUnavailable => {
                        InstanceUnavailable::DefinitionUnavailable
                    }
                    InstanceProblem::OwnerReferenceMissing => {
                        InstanceUnavailable::OwnerReferenceMissing
                    }
                    InstanceProblem::MissingOwner => InstanceUnavailable::MissingOwner,
                    InstanceProblem::InstanceMismatch(actual) => {
                        InstanceUnavailable::InstanceMismatch { actual }
                    }
                    InstanceProblem::DefinitionMismatch(actual) => {
                        InstanceUnavailable::DefinitionMismatch {
                            actual: actual.clone(),
                        }
                    }
                },
            },
        }
    }
}

impl World<'_> {
    /// Deterministic order: reference states, inventory banks, instances, journal;
    /// each list retains caller order. No missing requirement is initialized.
    pub fn check_host_requirements(
        &self,
        request: &HostRequirements,
        limits: HostLimits,
    ) -> Result<HostReadiness> {
        let total = request
            .reference_states
            .len()
            .checked_add(request.inventory_owners.len())
            .and_then(|n| n.checked_add(request.instances.len()))
            .and_then(|n| n.checked_add(usize::from(request.expected_journal_head.is_some())))
            .ok_or(Error::Capacity("host requirements"))?;
        if total > limits.max_requirements {
            return Err(Error::Capacity("host requirements"));
        }
        CampaignId::from_bytes(request.campaign.bytes())?;
        if request.campaign != self.campaign {
            return Err(Error::Invalid("host requirements campaign changed".into()));
        }
        if request.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        let mut charged_bytes = size_of::<HostReadiness>();
        charge(&mut charged_bytes, 1, self.cohort.len())?;
        charge(
            &mut charged_bytes,
            request.reference_states.len(),
            size_of::<ReferenceId>(),
        )?;
        charge(
            &mut charged_bytes,
            request.inventory_owners.len(),
            size_of::<ReferenceId>(),
        )?;
        charge(
            &mut charged_bytes,
            request.instances.len(),
            size_of::<&Owner>() + size_of::<InstanceId>(),
        )?;
        for instance in &request.instances {
            charge(&mut charged_bytes, 1, owner_bytes(&instance.owner))?;
            charge(
                &mut charged_bytes,
                1,
                definition_bytes(&instance.definition)?,
            )?;
        }
        if charged_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("host readiness copied bytes"));
        }
        self.validate_host_requirements(request)?;
        let mut unavailable_count = 0;
        self.visit_host_unavailable(request, |missing| {
            unavailable_count += 1; // already bounded by total above
            if unavailable_count > limits.max_unavailable {
                return Err(Error::Capacity("host unavailable inputs"));
            }
            missing.charge(&mut charged_bytes)?;
            if charged_bytes > limits.max_copied_bytes {
                return Err(Error::Capacity("host readiness copied bytes"));
            }
            Ok(())
        })?;
        // All copies are admitted before any report-owned diagnostic is made.
        let mut unavailable = Vec::with_capacity(unavailable_count);
        self.visit_host_unavailable(request, |missing| {
            unavailable.push(missing.into_owned());
            Ok(())
        })?;
        Ok(HostReadiness {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            requested_requirements: total,
            charged_bytes,
            unavailable,
        })
    }
    fn validate_host_requirements(&self, request: &HostRequirements) -> Result<()> {
        let mut references = BTreeSet::new();
        for id in &request.reference_states {
            if !references.insert(*id) {
                return Err(Error::Invalid(
                    "duplicate reference-state requirement".into(),
                ));
            }
        }
        let mut inventories = BTreeSet::new();
        for id in &request.inventory_owners {
            if !inventories.insert(*id) {
                return Err(Error::Invalid("duplicate inventory requirement".into()));
            }
        }
        let mut owners = BTreeSet::new();
        let mut instances = BTreeSet::new();
        for instance in &request.instances {
            if !owners.insert(&instance.owner) {
                return Err(Error::Invalid(
                    "duplicate instance owner requirement".into(),
                ));
            }
            if !instances.insert(instance.instance) {
                return Err(Error::Invalid(
                    "duplicate instance identity requirement".into(),
                ));
            }
            if let Owner::Quest { key } = &instance.owner {
                valid_form(key)?;
            }
            valid_form(&instance.definition.key.record)?;
            if !valid_sha(&instance.definition.version_sha256) {
                return Err(Error::Invalid(
                    "noncanonical required definition digest".into(),
                ));
            }
        }
        if let Some(JournalHead::Event { sequence: 0, .. }) = request.expected_journal_head {
            return Err(Error::Invalid("zero required journal sequence".into()));
        }
        Ok(())
    }
    fn visit_host_unavailable<'a>(
        &'a self,
        request: &'a HostRequirements,
        mut visit: impl FnMut(Missing<'a>) -> Result<()>,
    ) -> Result<()> {
        for (index, &id) in request.reference_states.iter().enumerate() {
            let reason = if !self.references.contains_key(&id) {
                Some(ComponentUnavailable::MissingReference)
            } else if !self.reference_states.contains_key(&id) {
                Some(ComponentUnavailable::Uninitialized)
            } else {
                None
            };
            if let Some(reason) = reason {
                visit(Missing::Reference(index, id, reason))?;
            }
        }
        for (index, &id) in request.inventory_owners.iter().enumerate() {
            let reason = if !self.references.contains_key(&id) {
                Some(ComponentUnavailable::MissingReference)
            } else if !self.inventory_banks.contains_key(&id) {
                Some(ComponentUnavailable::Uninitialized)
            } else {
                None
            };
            if let Some(reason) = reason {
                visit(Missing::Inventory(index, id, reason))?;
            }
        }
        for (index, expected) in request.instances.iter().enumerate() {
            let reason = if self.catalogue.get_handle(&expected.definition).is_none() {
                Some(InstanceProblem::DefinitionUnavailable)
            } else if matches!(expected.owner, Owner::Placed { reference } if !self.references.contains_key(&reference))
            {
                Some(InstanceProblem::OwnerReferenceMissing)
            } else if let Some(&id) = self.owners.get(&expected.owner) {
                if id != expected.instance {
                    Some(InstanceProblem::InstanceMismatch(id))
                } else {
                    let instance = self.slots[self.instances[&id]]
                        .value
                        .as_ref()
                        .expect("live owner instance");
                    if instance.definition != expected.definition {
                        Some(InstanceProblem::DefinitionMismatch(&instance.definition))
                    } else {
                        None
                    }
                }
            } else {
                Some(InstanceProblem::MissingOwner)
            };
            if let Some(reason) = reason {
                visit(Missing::Instance(index, expected, reason))?;
            }
        }
        if let Some(expected) = &request.expected_journal_head {
            let actual =
                self.pending
                    .front()
                    .map_or(JournalHead::Empty, |head| JournalHead::Event {
                        sequence: head.sequence,
                        instance: head.instance,
                    });
            if *expected != actual {
                visit(Missing::Journal(expected, actual))?;
            }
        }
        Ok(())
    }
}
