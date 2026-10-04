//! Source checks are explicit host admission rules, not measured retail policy.
use crate::{
    World,
    foreign::{Content, SourceForm},
    identity::{CampaignId, ReferenceId},
    inventory::{Facts, ItemId, Ownership},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};
pub type Result<T> = std::result::Result<T, Failure>;
pub use crate::inventory::{
    SourceFactsCountChange, SourceFactsLimits, SourceFactsReceipt, SourceFactsRow,
    SourceFactsUsage, SourceInventoryLimits, SourceInventoryOwnerReceipt, SourceInventoryReceipt,
    SourceInventoryUsage, StagedSourceFacts, StagedSourceInventory, StagedSourceInventoryAdditions,
};
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    Source(#[from] crate::foreign::Failure),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error("Item source policy is invalid: {0}")]
    Policy(&'static str),
    #[error("No explicit source-kind rule for {0:?}")]
    MissingRule(Role),
    #[error("Source form {key:?} has disallowed kind {actual:?} for {role:?}")]
    Kind {
        role: Role,
        key: FormKey,
        actual: [u8; 4],
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Base,
    ActorOwner,
    FactionOwner,
    Ammo,
    Modification,
}
/// A caller must supply each used role. There is no permissive missing-rule default.
#[derive(Debug, Clone, Serialize)]
pub struct Policy {
    rules: BTreeMap<Role, BTreeSet<[u8; 4]>>,
    sha256: String,
}
impl Policy {
    pub fn new(rules: &[(Role, &[[u8; 4]])]) -> Result<Self> {
        if rules.is_empty() || rules.len() > 5 {
            return Err(Failure::Policy("role budget"));
        }
        let mut map = BTreeMap::new();
        for &(role, kinds) in rules {
            if kinds.is_empty() || kinds.len() > 64 {
                return Err(Failure::Policy("kind budget"));
            }
            let mut values = BTreeSet::new();
            for &kind in kinds {
                if !kind.iter().all(u8::is_ascii_graphic) || !values.insert(kind) {
                    return Err(Failure::Policy("invalid or duplicate kind"));
                }
            }
            if map.insert(role, values).is_some() {
                return Err(Failure::Policy("duplicate role"));
            }
        }
        if !map.contains_key(&Role::Base) {
            return Err(Failure::MissingRule(Role::Base));
        }
        let mut hash = Sha256::new();
        hash.update(b"FRITEMPOLICY1");
        hash.update(serde_json::to_vec(&map).map_err(crate::Error::from)?);
        Ok(Self {
            rules: map,
            sha256: format!("{:x}", hash.finalize()),
        })
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}
#[derive(Debug, Serialize)]
pub struct CheckedForm {
    pub role: Role,
    pub key: FormKey,
    pub source: SourceForm,
}
/// A proof describes inputs before the mutation. It never substitutes for a new
/// validation after content/policy changes and is not part of canonical state.
#[derive(Debug, Serialize)]
pub struct Proof {
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub catalogue_sha256: String,
    pub policy_sha256: String,
    pub forms: Vec<CheckedForm>,
}
pub fn validate(
    world: &World<'_>,
    content: &Content,
    policy: &Policy,
    facts: &Facts,
) -> Result<Proof> {
    // Reuse canonical budgets/link validation before allocating diagnostic rows.
    world.validate_item_facts(facts)?;
    let mut forms = Vec::new();
    let mut check = |role, key: &FormKey| -> Result<()> {
        let source = content.source_form(world, key)?;
        let allowed = policy.rules.get(&role).ok_or(Failure::MissingRule(role))?;
        if !allowed.contains(&source.kind) {
            return Err(Failure::Kind {
                role,
                key: key.clone(),
                actual: source.kind,
            });
        }
        forms.push(CheckedForm {
            role,
            key: key.clone(),
            source,
        });
        Ok(())
    };
    check(Role::Base, &facts.base)?;
    match &facts.ownership {
        Some(Ownership::Actor { key }) => check(Role::ActorOwner, key)?,
        Some(Ownership::Faction { key, .. }) => check(Role::FactionOwner, key)?,
        _ => {}
    }
    if let Some(ammo) = &facts.ammo {
        check(Role::Ammo, &ammo.base)?;
    }
    if let Some(modifications) = &facts.modifications {
        for key in modifications {
            check(Role::Modification, key)?;
        }
    }
    Ok(Proof {
        campaign: world.campaign(),
        state_revision: world.revision(),
        catalogue_sha256: world.catalogue_fingerprint().into(),
        policy_sha256: policy.sha256.clone(),
        forms,
    })
}
impl World<'_> {
    pub fn add_source_item(
        &mut self,
        content: &Content,
        policy: &Policy,
        owner: ReferenceId,
        facts: Facts,
        count: NonZeroU32,
    ) -> Result<(ItemId, Proof)> {
        let proof = validate(self, content, policy, &facts)?;
        let id = self.add_item(owner, facts, count)?;
        Ok((id, proof))
    }
    pub fn replace_source_item_facts(
        &mut self,
        content: &Content,
        policy: &Policy,
        id: ItemId,
        facts: Facts,
    ) -> Result<Proof> {
        let proof = validate(self, content, policy, &facts)?;
        self.replace_item_facts(id, facts)?;
        Ok(proof)
    }
}
