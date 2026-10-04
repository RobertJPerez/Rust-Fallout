//! Bounded owned observations of explicit canonical locals. These values carry
//! persistent provenance, never a transient instance handle or write authority.
use super::InstanceHandle;
use crate::{
    Error, Result, World,
    events::Context,
    identity::{CampaignId, InstanceId, Owner, ReferenceValue, Value},
    schema::{Kind, Local},
};
use fallout_data::{identity::FormKey, loaded_scripts::Handle};
use serde::Serialize;
use std::mem::size_of;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_indices: usize,
    pub max_source_keys: usize,
    /// Sum of fixed FormKey storage and UTF-8 plugin names for copied occurrences.
    pub max_source_key_bytes: usize,
    pub max_context_arguments: usize,
    /// Fixed observation, rows/argument storage and all owned UTF-8 strings.
    /// Excludes allocator overhead and borrowed input/storage.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_indices: 256,
            max_source_keys: 1024,
            max_source_key_bytes: 64 * 1024,
            max_context_arguments: 64,
            max_copied_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub indices: usize,
    pub source_keys: usize,
    pub source_key_bytes: usize,
    pub context_arguments: usize,
    pub copied_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    declaration: Local,
    value: Value,
}
impl Row {
    pub fn index(&self) -> u32 {
        self.declaration.index
    }
    pub fn declaration(&self) -> &Local {
        &self.declaration
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observation {
    campaign: CampaignId,
    catalogue_sha256: String,
    state_revision: u64,
    definition: Handle,
    instance: InstanceId,
    owner: Owner,
    context: Context,
    rows: Vec<Row>,
    usage: Usage,
}
impl Observation {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.state_revision
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}

fn add(before: usize, bytes: usize) -> Result<usize> {
    before
        .checked_add(bytes)
        .ok_or(Error::Capacity("local observation copied bytes"))
}
fn table(rows: usize, bytes: usize) -> Result<usize> {
    rows.checked_mul(bytes)
        .ok_or(Error::Capacity("local observation copied bytes"))
}
fn charge_key(key: &FormKey, usage: &mut Usage, limits: Limits) -> Result<()> {
    usage.source_keys = usage
        .source_keys
        .checked_add(1)
        .ok_or(Error::Capacity("local observation source keys"))?;
    if usage.source_keys > limits.max_source_keys {
        return Err(Error::Capacity("local observation source keys"));
    }
    usage.source_key_bytes = usage
        .source_key_bytes
        .checked_add(size_of::<FormKey>())
        .and_then(|n| n.checked_add(key.origin_plugin.len()))
        .ok_or(Error::Capacity("local observation source key bytes"))?;
    if usage.source_key_bytes > limits.max_source_key_bytes {
        return Err(Error::Capacity("local observation source key bytes"));
    }
    // Fixed keys already reside in Observation/Row/Context argument storage.
    usage.copied_bytes = add(usage.copied_bytes, key.origin_plugin.len())?;
    Ok(())
}
fn charge_reference(value: &ReferenceValue, usage: &mut Usage, limits: Limits) -> Result<()> {
    if let ReferenceValue::Content { key } = value {
        charge_key(key, usage, limits)?;
    }
    Ok(())
}

impl World<'_> {
    pub fn observe_locals(
        &self,
        handle: InstanceHandle,
        indices: &[u32],
        limits: Limits,
    ) -> Result<Observation> {
        let instance = self.instance(handle)?;
        if indices.len() > limits.max_indices {
            return Err(Error::Capacity("local observation indices"));
        }
        if instance.context.arguments.len() > limits.max_context_arguments {
            return Err(Error::Capacity("local observation context arguments"));
        }
        let mut usage = Usage {
            indices: indices.len(),
            source_keys: 0,
            source_key_bytes: 0,
            context_arguments: instance.context.arguments.len(),
            copied_bytes: add(
                add(
                    add(
                        add(size_of::<Observation>(), self.cohort.len())?,
                        instance.definition.version_sha256.len(),
                    )?,
                    table(indices.len(), size_of::<Row>())?,
                )?,
                table(
                    instance.context.arguments.len(),
                    size_of::<ReferenceValue>(),
                )?,
            )?,
        };
        charge_key(&instance.definition.key.record, &mut usage, limits)?;
        if let Owner::Quest { key } = &instance.owner {
            charge_key(key, &mut usage, limits)?;
        }
        for value in instance
            .context
            .target
            .iter()
            .chain(&instance.context.arguments)
        {
            charge_reference(value, &mut usage, limits)?;
        }
        // Borrow the schema retained by this exact instance. No cache entry,
        // source schema rebuild, whole snapshot or Arc copy is needed to read.
        for (position, index) in indices.iter().enumerate() {
            if indices[..position].contains(index) {
                return Err(Error::Invalid("duplicate observed local index".into()));
            }
            let declaration = instance
                .definition_schema
                .locals
                .get(index)
                .ok_or(Error::MissingLocal(*index))?;
            if !matches!(
                declaration.kind,
                Kind::Float | Kind::Integer | Kind::Reference
            ) {
                return Err(Error::UnsupportedLocal(*index));
            }
            let value = instance
                .locals
                .get(index)
                .ok_or(Error::MissingLocal(*index))?;
            if let Value::Reference { value } = value {
                charge_reference(value, &mut usage, limits)?;
            }
        }
        if usage.copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("local observation copied bytes"));
        }
        self.validate_owner(&instance.owner)?;
        self.validate_context(&instance.context)?;
        for index in indices {
            self.validate_value(
                &instance.definition_schema.locals[index],
                &instance.locals[index],
            )?;
        }
        // All budget arithmetic and compatibility checks precede the first copy.
        Ok(Observation {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            state_revision: self.revision,
            definition: instance.definition.clone(),
            instance: instance.id,
            owner: instance.owner.clone(),
            context: instance.context.clone(),
            rows: indices
                .iter()
                .map(|index| Row {
                    declaration: instance.definition_schema.locals[index].clone(),
                    value: instance.locals[index].clone(),
                })
                .collect(),
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> FormKey {
        FormKey {
            profile: fallout_data::identity::ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x300,
        }
    }
    fn source_fixture(path: &std::path::Path) {
        fn field(tag: &[u8; 4], value: &[u8]) -> Vec<u8> {
            [tag.as_slice(), &(value.len() as u16).to_le_bytes(), value].concat()
        }
        fn record(tag: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
            [
                tag.as_slice(),
                &(body.len() as u32).to_le_bytes(),
                &0u32.to_le_bytes(),
                &id.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        }
        let hedr = [1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat();
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&14u32.to_le_bytes());
        schr[12..16].copy_from_slice(&1u32.to_le_bytes());
        let mut variable = [0; 24];
        variable[..4].copy_from_slice(&42u32.to_le_bytes());
        let body = [
            field(b"SCHR", &schr),
            field(b"SCDA", &[0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0]),
            field(b"SLSD", &variable),
            field(b"SCVR", b"local_42\0"),
        ]
        .concat();
        std::fs::write(
            path.join("FalloutNV.esm"),
            [
                record(b"TES4", 0, &field(b"HEDR", &hedr)),
                record(b"SCPT", 0x300, &body),
            ]
            .concat(),
        )
        .unwrap();
    }
    #[test]
    fn owned_observation_byte_and_source_occurrence_arithmetic_never_wraps() {
        assert!(add(1, usize::MAX).is_err());
        assert!(table(usize::MAX, 2).is_err());
        let mut usage = Usage {
            indices: 0,
            source_keys: usize::MAX,
            source_key_bytes: 0,
            context_arguments: 0,
            copied_bytes: 0,
        };
        assert!(
            charge_key(
                &key(),
                &mut usage,
                Limits {
                    max_source_keys: usize::MAX,
                    ..Limits::default()
                }
            )
            .is_err()
        );
        usage.source_keys = 0;
        usage.source_key_bytes = usize::MAX;
        assert!(
            charge_key(
                &key(),
                &mut usage,
                Limits {
                    max_source_key_bytes: usize::MAX,
                    ..Limits::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn actual_source_read_and_budget_refusal_leave_cleared_schema_cache_cold() {
        let root = tempfile::tempdir().unwrap();
        source_fixture(root.path());
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            root.path(),
            &["FalloutNV.esm".into()],
            fallout_data::plugin::Limits::default(),
        )
        .unwrap();
        let catalogue = fallout_data::loaded_scripts::Catalogue::load(
            &mut store,
            Default::default(),
            |_, _| Ok(()),
        )
        .unwrap();
        let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
        let handle = world
            .create_instance(
                catalogue.iter().next().unwrap().1.handle(),
                Owner::Fragment {
                    activation: 1.try_into().unwrap(),
                },
                Context::default(),
            )
            .unwrap();
        world.definitions.clear();
        world.block_count = 0;
        let before = world.snapshot();
        let count =
            std::sync::Arc::strong_count(&world.instance(handle).unwrap().definition_schema);
        let observation = world
            .observe_locals(handle, &[42], Limits::default())
            .unwrap();
        assert!(
            observation
                .rows()
                .iter()
                .all(|row| row.value() == &Value::Uninitialized)
        );
        assert!(
            world
                .observe_locals(
                    handle,
                    &[42],
                    Limits {
                        max_copied_bytes: observation.usage().copied_bytes - 1,
                        ..Limits::default()
                    }
                )
                .is_err()
        );
        assert!(world.definitions.is_empty());
        assert_eq!(world.block_count, 0);
        assert_eq!(
            std::sync::Arc::strong_count(&world.instance(handle).unwrap().definition_schema),
            count
        );
        assert_eq!(world.snapshot(), before);
        world.slots[handle.slot]
            .value
            .as_mut()
            .unwrap()
            .definition
            .version_sha256 = "f".repeat(64);
        let changed = world.snapshot();
        assert!(matches!(
            world.observe_locals(handle, &[42], Limits::default()),
            Err(Error::DefinitionChanged)
        ));
        assert!(world.definitions.is_empty());
        assert_eq!(world.snapshot(), changed);
    }
}
