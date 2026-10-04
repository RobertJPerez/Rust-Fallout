//! One explicit host initialization over verified compiled declarations. This
//! chooses no source attachment, constructor default or original startup order.
use super::{DefinitionSchema, Instance, InstanceHandle, World};
use crate::{
    Error, Result,
    events::Context,
    identity::{CampaignId, InstanceId, Owner, ReferenceValue, Value},
    schema::Local,
};
use fallout_data::loaded_scripts::Handle;
use serde::Serialize;
use std::{collections::BTreeMap, mem::size_of, num::NonZeroU64, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Physical declarations, including repeats, bound private schema work.
    pub max_source_declarations: usize,
    /// Physical source references bound the shared schema helper's membership
    /// scan and temporary dynamic-reference index.
    pub max_source_references: usize,
    /// The existing compiled decoder owns temporary instructions separately from
    /// the staged-value charge; bound its input before invoking it.
    pub max_compiled_bytes: usize,
    pub max_event_blocks: usize,
    pub max_assignments: usize,
    /// Conservative fixed/table/string charge before owned stage copies. Counts
    /// physical declaration upper bounds even when a schema Arc is reused.
    /// Excludes allocator overhead and the compiled decoder's transient storage.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_declarations: 1_000_000,
            max_source_references: 1_000_000,
            max_compiled_bytes: 64 * 1024 * 1024,
            max_event_blocks: 1_000_000,
            max_assignments: 1_000_000,
            max_copied_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
#[must_use = "staging reserves no public identity; commit or drop it"]
pub struct StagedInstanceInitialization {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    next_instance: u64,
    definition: Handle,
    owner: Owner,
    context: Context,
    schema: Arc<DefinitionSchema>,
    locals: BTreeMap<u32, Value>,
    assignments: usize,
    charged_bytes: usize,
}
impl StagedInstanceInitialization {
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn locals(&self) -> &BTreeMap<u32, Value> {
        &self.locals
    }
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub definition: Handle,
    pub owner: Owner,
    pub instance: InstanceId,
    pub before_revision: u64,
    pub after_revision: u64,
    pub next_instance_before: u64,
    pub next_instance_after: u64,
    pub explicit_assignments: usize,
    pub initialized_locals: usize,
    pub uninitialized_locals: usize,
}

fn reference_bytes(value: &ReferenceValue) -> usize {
    match value {
        ReferenceValue::Content { key } => key.origin_plugin.len(),
        _ => 0,
    }
}
fn add_charge(total: &mut usize, count: usize, each: usize) -> Result<()> {
    *total = total
        .checked_add(
            count
                .checked_mul(each)
                .ok_or(Error::Capacity("instance initialization copied bytes"))?,
        )
        .ok_or(Error::Capacity("instance initialization copied bytes"))?;
    Ok(())
}

impl World<'_> {
    pub fn stage_instance_initialization(
        &self,
        definition: &Handle,
        owner: &Owner,
        context: &Context,
        assignments: &[(u32, Value)],
        limits: Limits,
    ) -> Result<StagedInstanceInitialization> {
        let script = self
            .catalogue
            .get_handle(definition)
            .ok_or(Error::DefinitionChanged)?;
        self.validate_owner(owner)?;
        self.validate_context(context)?;
        if self.owners.contains_key(owner) {
            return Err(Error::Invalid(
                "owner already has a live script instance".into(),
            ));
        }
        if self.instances.len() >= self.limits.max_instances {
            return Err(Error::Capacity("script instances"));
        }
        if assignments.len() > limits.max_assignments {
            return Err(Error::Capacity("instance initializer assignments"));
        }
        if script.declarations().len() > limits.max_source_declarations {
            return Err(Error::Capacity("instance source declarations"));
        }
        if script.references().len() > limits.max_source_references {
            return Err(Error::Capacity("instance source references"));
        }
        if script.compiled().map_or(0, <[u8]>::len) > limits.max_compiled_bytes {
            return Err(Error::Capacity("instance compiled bytes"));
        }
        let mut charged_bytes =
            size_of::<StagedInstanceInitialization>() + size_of::<DefinitionSchema>();
        for bytes in [
            self.cohort.len(),
            definition.key.record.origin_plugin.len(),
            definition.version_sha256.len(),
            match owner {
                Owner::Quest { key } => key.origin_plugin.len(),
                _ => 0,
            },
        ] {
            add_charge(&mut charged_bytes, 1, bytes)?;
        }
        add_charge(
            &mut charged_bytes,
            context.arguments.len(),
            size_of::<ReferenceValue>(),
        )?;
        for value in context.target.iter().chain(context.arguments.iter()) {
            add_charge(&mut charged_bytes, 1, reference_bytes(value))?;
        }
        add_charge(&mut charged_bytes, assignments.len(), size_of::<u32>())?; // duplicate-check IDs
        add_charge(
            &mut charged_bytes,
            script.references().len(),
            size_of::<u32>(),
        )?;
        for (_, value) in assignments {
            if let Value::Reference { value } = value {
                add_charge(&mut charged_bytes, 1, reference_bytes(value))?;
            }
        }
        if charged_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("instance initialization copied bytes"));
        }
        let schema = self.prepare_runtime_definition(definition, |declarations, events| {
            if events > limits.max_event_blocks {
                return Err(Error::Capacity("instance source event blocks"));
            }
            add_charge(&mut charged_bytes, declarations, size_of::<(u32, Local)>())?;
            add_charge(&mut charged_bytes, declarations, size_of::<(u32, Value)>())?;
            add_charge(&mut charged_bytes, events, size_of::<(u32, u16)>())?;
            if charged_bytes > limits.max_copied_bytes {
                return Err(Error::Capacity("instance initialization copied bytes"));
            }
            Ok(())
        })?;
        if schema.locals.len() > self.limits.max_locals.saturating_sub(self.local_count) {
            return Err(Error::Capacity("local variables"));
        }
        self.validate_schema_assignments(&schema.locals, assignments)?;
        let mut locals: BTreeMap<_, _> = schema
            .locals
            .keys()
            .map(|&index| (index, Value::Uninitialized))
            .collect();
        for (index, value) in assignments {
            *locals.get_mut(index).expect("validated initializer") = value.clone();
        }
        Ok(StagedInstanceInitialization {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            next_instance: self.next_instance,
            definition: definition.clone(),
            owner: owner.clone(),
            context: context.clone(),
            schema,
            locals,
            assignments: assignments.len(),
            charged_bytes,
        })
    }

    pub fn commit_instance_initialization(
        &mut self,
        stage: StagedInstanceInitialization,
    ) -> Result<(Receipt, InstanceHandle)> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.cohort != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision || stage.next_instance != self.next_instance {
            return Err(Error::Invalid(
                "instance initialization revision changed".into(),
            ));
        }
        self.catalogue
            .get_handle(&stage.definition)
            .ok_or(Error::DefinitionChanged)?;
        self.validate_owner(&stage.owner)?;
        self.validate_context(&stage.context)?;
        if self.owners.contains_key(&stage.owner) {
            return Err(Error::Invalid(
                "owner already has a live script instance".into(),
            ));
        }
        if self.instances.len() >= self.limits.max_instances {
            return Err(Error::Capacity("script instances"));
        }
        if stage.locals.len() > self.limits.max_locals.saturating_sub(self.local_count) {
            return Err(Error::Capacity("local variables"));
        }
        for (index, value) in &stage.locals {
            self.validate_value(
                stage
                    .schema
                    .locals
                    .get(index)
                    .ok_or(Error::MissingLocal(*index))?,
                value,
            )?;
        }
        // A failed legacy create can warm the immutable cache without changing
        // canonical revision. Reuse its exact schema, and charge blocks once.
        let schema = if let Some(cached) = self.definitions.get(&stage.definition.key) {
            if cached.locals != stage.schema.locals || cached.blocks != stage.schema.blocks {
                return Err(Error::DefinitionChanged);
            }
            Arc::clone(cached)
        } else {
            if stage.schema.blocks.len()
                > self
                    .limits
                    .max_event_blocks
                    .saturating_sub(self.block_count)
            {
                return Err(Error::Capacity("compiled event blocks"));
            }
            Arc::clone(&stage.schema)
        };
        let next = self
            .next_instance
            .checked_add(1)
            .ok_or(Error::Capacity("instance identities"))?;
        let id = InstanceId(
            NonZeroU64::new(self.next_instance)
                .ok_or_else(|| Error::Invalid("zero instance allocator".into()))?,
        );
        let revision = self.next_revision()?;
        let uninitialized = stage
            .locals
            .values()
            .filter(|value| **value == Value::Uninitialized)
            .count();
        let receipt = Receipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            definition: stage.definition.clone(),
            owner: stage.owner.clone(),
            instance: id,
            before_revision: self.revision,
            after_revision: revision,
            next_instance_before: self.next_instance,
            next_instance_after: next,
            explicit_assignments: stage.assignments,
            initialized_locals: stage.locals.len() - uninitialized,
            uninitialized_locals: uninitialized,
        };
        let instance = Instance {
            id,
            definition: stage.definition,
            owner: stage.owner.clone(),
            context: stage.context,
            definition_schema: Arc::clone(&schema),
            locals: stage.locals,
        };
        if !self.definitions.contains_key(&instance.definition.key) {
            self.block_count += schema.blocks.len();
            self.definitions
                .insert(instance.definition.key.clone(), schema);
        }
        let handle = self.publish_instance(instance, stage.owner, next, revision);
        Ok((receipt, handle))
    }
}
