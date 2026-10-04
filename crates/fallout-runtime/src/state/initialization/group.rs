//! Caller-ordered explicit attachments, admitted and published as one revision.
//! No attachment discovery, startup policy, default values or execution.
use super::{add_charge, reference_bytes};
use crate::{
    Error, Result,
    events::Context,
    identity::{CampaignId, InstanceId, Owner, Value},
    schema::Local,
    state::{DefinitionSchema, Instance, InstanceHandle, World},
};
use fallout_data::loaded_scripts::{Handle, ScriptKey};
use serde::Serialize;
use std::{collections::BTreeMap, mem::size_of, num::NonZeroU64, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct Request<'a> {
    pub definition: &'a Handle,
    pub owner: &'a Owner,
    pub context: &'a Context,
    pub assignments: &'a [(u32, Value)],
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_instances: usize,
    /// Source work is charged once for each exact definition handle.
    pub max_source_declarations: usize,
    pub max_source_references: usize,
    pub max_compiled_bytes: usize,
    /// Existing preparation callback rows: physical decoded events for cold
    /// schemas, distinct cached blocks when no decoding is needed.
    pub max_event_blocks: usize,
    pub max_assignments: usize,
    pub max_context_arguments: usize,
    /// Conservative fixed tables, schemas, duplicate checks, values and UTF-8
    /// copies. Excludes allocator overhead and transient compiled decoding.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_instances: 256,
            max_source_declarations: 1_000_000,
            max_source_references: 1_000_000,
            max_compiled_bytes: 64 * 1024 * 1024,
            max_event_blocks: 1_000_000,
            max_assignments: 16_384,
            max_context_arguments: 16_384,
            max_copied_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub instances: usize,
    pub unique_definitions: usize,
    pub source_declarations: usize,
    pub source_references: usize,
    pub compiled_bytes: usize,
    pub event_blocks: usize,
    pub assignments: usize,
    pub context_arguments: usize,
    pub locals: usize,
    pub copied_bytes: usize,
}
#[derive(Debug)]
#[must_use = "staging neither reserves identities nor warms the world cache"]
pub struct StagedGroup {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    next_instance: u64,
    rows: Vec<Row>,
    usage: Usage,
}
impl StagedGroup {
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}
#[derive(Debug)]
pub struct Row {
    definition: Handle,
    owner: Owner,
    context: Context,
    schema: Arc<DefinitionSchema>,
    locals: BTreeMap<u32, Value>,
    assignments: usize,
}
impl Row {
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
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub before_revision: u64,
    pub after_revision: u64,
    pub next_instance_before: u64,
    pub next_instance_after: u64,
    pub rows: Vec<ReceiptRow>,
    pub usage: Usage,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReceiptRow {
    pub instance: InstanceId,
    pub definition: Handle,
    pub owner: Owner,
    pub explicit_assignments: usize,
    pub initialized_locals: usize,
    pub uninitialized_locals: usize,
}
struct Publication {
    instance: Instance,
    owner: Owner,
    next: u64,
}
fn bounded_add(total: &mut usize, value: usize, maximum: usize, code: &'static str) -> Result<()> {
    let next = total.checked_add(value).ok_or(Error::Capacity(code))?;
    if next > maximum {
        return Err(Error::Capacity(code));
    }
    *total = next;
    Ok(())
}
fn capacity(current: usize, added: usize, maximum: usize, code: &'static str) -> Result<()> {
    let mut total = current;
    bounded_add(&mut total, added, maximum, code)
}
impl World<'_> {
    fn initialization_group_next(&self, count: usize) -> Result<u64> {
        if count != 0 && self.next_instance == 0 {
            return Err(Error::Invalid("zero instance allocator".into()));
        }
        self.next_instance
            .checked_add(u64::try_from(count).map_err(|_| Error::Capacity("instance identities"))?)
            .ok_or(Error::Capacity("instance identities"))
    }
    pub fn stage_instance_initialization_group(
        &self,
        requests: &[Request<'_>],
        limits: Limits,
    ) -> Result<StagedGroup> {
        if requests.len() > limits.max_instances {
            return Err(Error::Capacity("instance initialization group instances"));
        }
        capacity(
            self.instances.len(),
            requests.len(),
            self.limits.max_instances,
            "script instances",
        )?;
        self.initialization_group_next(requests.len())?;
        if !requests.is_empty() {
            self.next_revision()?;
        }
        let mut usage = Usage {
            instances: requests.len(),
            unique_definitions: 0,
            source_declarations: 0,
            source_references: 0,
            compiled_bytes: 0,
            event_blocks: 0,
            assignments: 0,
            context_arguments: 0,
            locals: 0,
            copied_bytes: size_of::<StagedGroup>() + size_of::<Receipt>(),
        };
        add_charge(&mut usage.copied_bytes, 1, self.cohort.len())?;
        for each in [
            size_of::<Row>(),
            size_of::<ReceiptRow>(),
            size_of::<Publication>(),
            size_of::<InstanceHandle>(),
        ] {
            add_charge(&mut usage.copied_bytes, requests.len(), each)?;
        }
        // Admit every borrowed request and aggregate storage before private
        // schema preparation or caller-string copies. Equality includes source
        // version, not merely the persistent script key.
        for (position, request) in requests.iter().enumerate() {
            let script = self
                .catalogue
                .get_handle(request.definition)
                .ok_or(Error::DefinitionChanged)?;
            self.validate_owner(request.owner)?;
            self.validate_context(request.context)?;
            if self.owners.contains_key(request.owner)
                || requests[..position]
                    .iter()
                    .any(|old| old.owner == request.owner)
            {
                return Err(Error::Invalid(
                    "owner already has a live or grouped script instance".into(),
                ));
            }
            bounded_add(
                &mut usage.assignments,
                request.assignments.len(),
                limits.max_assignments,
                "instance initialization group assignments",
            )?;
            bounded_add(
                &mut usage.context_arguments,
                request.context.arguments.len(),
                limits.max_context_arguments,
                "instance initialization group context arguments",
            )?;
            // Stage and receipt each retain the handle; cache keys retain one
            // further origin below. Instance/owner-index/receipt own the owner.
            for bytes in [
                request.definition.key.record.origin_plugin.len(),
                request.definition.version_sha256.len(),
            ] {
                add_charge(&mut usage.copied_bytes, 2, bytes)?;
            }
            if let Owner::Quest { key } = request.owner {
                add_charge(&mut usage.copied_bytes, 3, key.origin_plugin.len())?;
            }
            add_charge(
                &mut usage.copied_bytes,
                request.context.arguments.len(),
                size_of::<crate::identity::ReferenceValue>(),
            )?;
            for value in request
                .context
                .target
                .iter()
                .chain(request.context.arguments.iter())
            {
                add_charge(&mut usage.copied_bytes, 1, reference_bytes(value))?;
            }
            add_charge(
                &mut usage.copied_bytes,
                request.assignments.len(),
                size_of::<u32>(),
            )?;
            for (_, value) in request.assignments {
                if let Value::Reference { value } = value {
                    add_charge(&mut usage.copied_bytes, 1, reference_bytes(value))?;
                }
            }
            // Physical declarations conservatively bound every staged local,
            // even if duplicate declarations collapse to one canonical entry.
            add_charge(
                &mut usage.copied_bytes,
                script.declarations().len(),
                size_of::<(u32, Value)>(),
            )?;
            if requests[..position]
                .iter()
                .all(|old| old.definition != request.definition)
            {
                usage.unique_definitions += 1; // bounded by requests.len()
                bounded_add(
                    &mut usage.source_declarations,
                    script.declarations().len(),
                    limits.max_source_declarations,
                    "instance initialization group declarations",
                )?;
                bounded_add(
                    &mut usage.source_references,
                    script.references().len(),
                    limits.max_source_references,
                    "instance initialization group references",
                )?;
                bounded_add(
                    &mut usage.compiled_bytes,
                    script.compiled().map_or(0, <[u8]>::len),
                    limits.max_compiled_bytes,
                    "instance initialization group compiled bytes",
                )?;
                for each in [
                    size_of::<DefinitionSchema>(),
                    size_of::<ScriptKey>(),
                    2 * size_of::<(&Handle, Arc<DefinitionSchema>)>(),
                ] {
                    add_charge(&mut usage.copied_bytes, 1, each)?;
                }
                add_charge(
                    &mut usage.copied_bytes,
                    1,
                    request.definition.key.record.origin_plugin.len(),
                )?;
                add_charge(
                    &mut usage.copied_bytes,
                    script.declarations().len(),
                    size_of::<(u32, Local)>(),
                )?;
                add_charge(
                    &mut usage.copied_bytes,
                    script.references().len(),
                    size_of::<u32>(),
                )?;
            }
        }
        if usage.copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("instance initialization copied bytes"));
        }
        let mut schemas: Vec<(&Handle, Arc<DefinitionSchema>)> =
            Vec::with_capacity(usage.unique_definitions);
        let mut added_blocks = 0;
        for request in requests {
            if schemas
                .iter()
                .any(|(handle, _)| *handle == request.definition)
            {
                continue;
            }
            let schema = self.prepare_runtime_definition(request.definition, |_, events| {
                bounded_add(
                    &mut usage.event_blocks,
                    events,
                    limits.max_event_blocks,
                    "instance initialization group event blocks",
                )?;
                add_charge(&mut usage.copied_bytes, events, size_of::<(u32, u16)>())?;
                if usage.copied_bytes > limits.max_copied_bytes {
                    return Err(Error::Capacity("instance initialization copied bytes"));
                }
                Ok(())
            })?;
            if !self.definitions.contains_key(&request.definition.key) {
                bounded_add(
                    &mut added_blocks,
                    schema.blocks.len(),
                    usize::MAX,
                    "compiled event blocks",
                )?;
            }
            schemas.push((request.definition, schema));
        }
        capacity(
            self.block_count,
            added_blocks,
            self.limits.max_event_blocks,
            "compiled event blocks",
        )?;
        // Whole source/value/canonical capacity checks precede all owned row
        // copies, and use the same validator as the single initializer.
        for request in requests {
            let schema = &schemas
                .iter()
                .find(|(handle, _)| *handle == request.definition)
                .expect("prepared exact definition")
                .1;
            self.validate_schema_assignments(&schema.locals, request.assignments)?;
            bounded_add(
                &mut usage.locals,
                schema.locals.len(),
                usize::MAX,
                "local variables",
            )?;
        }
        capacity(
            self.local_count,
            usage.locals,
            self.limits.max_locals,
            "local variables",
        )?;
        let rows = requests
            .iter()
            .map(|request| {
                let schema = Arc::clone(
                    &schemas
                        .iter()
                        .find(|(handle, _)| *handle == request.definition)
                        .expect("prepared exact definition")
                        .1,
                );
                let mut locals: BTreeMap<_, _> = schema
                    .locals
                    .keys()
                    .map(|&index| (index, Value::Uninitialized))
                    .collect();
                for (index, value) in request.assignments {
                    *locals.get_mut(index).expect("validated group initializer") = value.clone();
                }
                Row {
                    definition: request.definition.clone(),
                    owner: request.owner.clone(),
                    context: request.context.clone(),
                    schema,
                    locals,
                    assignments: request.assignments.len(),
                }
            })
            .collect();
        Ok(StagedGroup {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            next_instance: self.next_instance,
            rows,
            usage,
        })
    }
    pub fn commit_instance_initialization_group(
        &mut self,
        stage: StagedGroup,
    ) -> Result<(Receipt, Vec<InstanceHandle>)> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.cohort != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision || stage.next_instance != self.next_instance {
            return Err(Error::Invalid(
                "instance initialization group revision or allocator changed".into(),
            ));
        }
        capacity(
            self.instances.len(),
            stage.rows.len(),
            self.limits.max_instances,
            "script instances",
        )?;
        let next = self.initialization_group_next(stage.rows.len())?;
        let revision = if stage.rows.is_empty() {
            self.revision
        } else {
            self.next_revision()?
        };
        let mut schemas: Vec<(&Handle, Arc<DefinitionSchema>)> =
            Vec::with_capacity(stage.usage.unique_definitions);
        let mut added_blocks = 0;
        let mut added_locals = 0;
        for (position, row) in stage.rows.iter().enumerate() {
            self.catalogue
                .get_handle(&row.definition)
                .ok_or(Error::DefinitionChanged)?;
            self.validate_owner(&row.owner)?;
            self.validate_context(&row.context)?;
            if self.owners.contains_key(&row.owner)
                || stage.rows[..position]
                    .iter()
                    .any(|old| old.owner == row.owner)
            {
                return Err(Error::Invalid(
                    "owner already has a live or grouped script instance".into(),
                ));
            }
            for (index, value) in &row.locals {
                self.validate_value(
                    row.schema
                        .locals
                        .get(index)
                        .ok_or(Error::MissingLocal(*index))?,
                    value,
                )?;
            }
            bounded_add(
                &mut added_locals,
                row.locals.len(),
                usize::MAX,
                "local variables",
            )?;
            if let Some((_, schema)) = schemas
                .iter()
                .find(|(handle, _)| **handle == row.definition)
            {
                if schema.locals != row.schema.locals || schema.blocks != row.schema.blocks {
                    return Err(Error::DefinitionChanged);
                }
                continue;
            }
            let schema = if let Some(cached) = self.definitions.get(&row.definition.key) {
                if cached.locals != row.schema.locals || cached.blocks != row.schema.blocks {
                    return Err(Error::DefinitionChanged);
                }
                Arc::clone(cached)
            } else {
                bounded_add(
                    &mut added_blocks,
                    row.schema.blocks.len(),
                    usize::MAX,
                    "compiled event blocks",
                )?;
                Arc::clone(&row.schema)
            };
            schemas.push((&row.definition, schema));
        }
        capacity(
            self.local_count,
            added_locals,
            self.limits.max_locals,
            "local variables",
        )?;
        capacity(
            self.block_count,
            added_blocks,
            self.limits.max_event_blocks,
            "compiled event blocks",
        )?;
        let mut receipts = Vec::with_capacity(stage.rows.len());
        let mut publications = Vec::with_capacity(stage.rows.len());
        let mut handles = Vec::with_capacity(stage.rows.len());
        for (position, row) in stage.rows.iter().enumerate() {
            let id = InstanceId(
                NonZeroU64::new(self.next_instance + position as u64)
                    .expect("whole identity range checked"),
            );
            let uninitialized = row
                .locals
                .values()
                .filter(|value| **value == Value::Uninitialized)
                .count();
            receipts.push(ReceiptRow {
                instance: id,
                definition: row.definition.clone(),
                owner: row.owner.clone(),
                explicit_assignments: row.assignments,
                initialized_locals: row.locals.len() - uninitialized,
                uninitialized_locals: uninitialized,
            });
        }
        // Drop borrowed schema keys before consuming the stage rows. Schemas
        // are reused from the exact cached/private row below, without decoding.
        drop(schemas);
        for (position, row) in stage.rows.into_iter().enumerate() {
            let schema = self
                .definitions
                .get(&row.definition.key)
                .map_or_else(|| Arc::clone(&row.schema), Arc::clone);
            let owner = row.owner;
            publications.push(Publication {
                instance: Instance {
                    id: receipts[position].instance,
                    definition: row.definition,
                    owner: owner.clone(),
                    context: row.context,
                    definition_schema: schema,
                    locals: row.locals,
                },
                owner,
                next: self.next_instance + position as u64 + 1,
            });
        }
        self.slots
            .try_reserve(publications.len().saturating_sub(self.free.len()))
            .map_err(|_| Error::Capacity("instance slot allocation"))?;
        // No fallible validation/arithmetic remains. Existing BTree allocation
        // follows the world's ordinary abort-on-OOM behavior.
        for publication in publications {
            if !self
                .definitions
                .contains_key(&publication.instance.definition.key)
            {
                self.block_count += publication.instance.definition_schema.blocks.len();
                self.definitions.insert(
                    publication.instance.definition.key.clone(),
                    Arc::clone(&publication.instance.definition_schema),
                );
            }
            handles.push(self.publish_instance(
                publication.instance,
                publication.owner,
                publication.next,
                revision,
            ));
        }
        debug_assert_eq!(self.next_instance, next);
        Ok((
            Receipt {
                campaign: stage.campaign,
                catalogue_sha256: stage.cohort,
                before_revision: stage.revision,
                after_revision: revision,
                next_instance_before: stage.next_instance,
                next_instance_after: next,
                rows: receipts,
                usage: stage.usage,
            },
            handles,
        ))
    }
}
