//! One explicit host write set over existing script instances. It establishes
//! no bytecode execution order and neither changes nor acknowledges events.
use super::{InstanceHandle, World};
use crate::{
    Error, Result,
    identity::{CampaignId, InstanceId, ReferenceValue, Value},
};
use fallout_data::loaded_scripts::Handle;
use serde::Serialize;
use std::mem::size_of;

#[derive(Debug)]
pub struct Request<'a> {
    pub instance: InstanceHandle,
    pub assignments: &'a [(u32, Value)],
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_instances: usize,
    pub max_assignments: usize,
    /// Logical fixed tables and copied string/value storage, including temporary
    /// duplicate-check indices and commit tables. Excludes allocator overhead.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_instances: 256,
            max_assignments: 4096,
            max_copied_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub instances: usize,
    pub assignments: usize,
    pub copied_bytes: usize,
}

/// Private, immutable authority created by this world. It cannot deserialize,
/// clone, survive a restore or be reused after consuming commit.
#[derive(Debug)]
#[must_use = "staging does not change locals; commit the group or drop it"]
pub struct StagedGroup {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
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
    handle: InstanceHandle,
    instance: InstanceId,
    definition: Handle,
    assignments: Vec<(u32, Value)>,
}
impl Row {
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn assignments(&self) -> &[(u32, Value)] {
        &self.assignments
    }
}

/// Persistable observation of explicit writes, without replay authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    rows: Vec<ReceiptRow>,
    usage: Usage,
}
impl Receipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_sha256(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn rows(&self) -> &[ReceiptRow] {
        &self.rows
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReceiptRow {
    instance: InstanceId,
    definition: Handle,
    assignments: usize,
}
impl ReceiptRow {
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn assignments(&self) -> usize {
        self.assignments
    }
}

struct Write {
    slot: usize,
    assignments: Vec<(u32, Value)>,
}

fn charge(total: &mut usize, count: usize, each: usize) -> Result<()> {
    *total = total
        .checked_add(
            count
                .checked_mul(each)
                .ok_or(Error::Capacity("local assignment group copied bytes"))?,
        )
        .ok_or(Error::Capacity("local assignment group copied bytes"))?;
    Ok(())
}

impl World<'_> {
    pub fn stage_local_assignments(
        &self,
        requests: &[Request<'_>],
        limits: Limits,
    ) -> Result<StagedGroup> {
        if requests.len() > limits.max_instances {
            return Err(Error::Capacity("local assignment group instances"));
        }
        let mut usage = Usage {
            instances: requests.len(),
            assignments: 0,
            copied_bytes: size_of::<StagedGroup>(),
        };
        charge(&mut usage.copied_bytes, 1, size_of::<Receipt>())?;
        charge(&mut usage.copied_bytes, 1, self.cohort.len())?;
        for each in [
            size_of::<Row>(),
            size_of::<ReceiptRow>(),
            size_of::<Write>(),
        ] {
            charge(&mut usage.copied_bytes, requests.len(), each)?;
        }
        // First pass borrows source identities and values. Admit aggregate work
        // before cloning caller strings or allocating schema duplicate checks.
        for (position, request) in requests.iter().enumerate() {
            if requests[..position]
                .iter()
                .any(|earlier| earlier.instance == request.instance)
            {
                return Err(Error::Invalid("duplicate group instance".into()));
            }
            let instance = self.instance(request.instance)?;
            usage.assignments = usage
                .assignments
                .checked_add(request.assignments.len())
                .ok_or(Error::Capacity("local assignment group assignments"))?;
            if usage.assignments > limits.max_assignments {
                return Err(Error::Capacity("local assignment group assignments"));
            }
            for bytes in [
                instance.definition.key.record.origin_plugin.len(),
                instance.definition.version_sha256.len(),
            ] {
                charge(&mut usage.copied_bytes, 1, bytes)?;
            }
            charge(
                &mut usage.copied_bytes,
                request.assignments.len(),
                size_of::<(u32, Value)>(),
            )?;
            charge(
                &mut usage.copied_bytes,
                request.assignments.len(),
                size_of::<u32>(),
            )?;
            for (_, value) in request.assignments {
                if let Value::Reference {
                    value: ReferenceValue::Content { key },
                } = value
                {
                    charge(&mut usage.copied_bytes, 1, key.origin_plugin.len())?;
                }
            }
        }
        if usage.copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("local assignment group copied bytes"));
        }
        for request in requests {
            // Retain assign's exact uninitialized/kind/reference policy and
            // first-declaration schema rather than inventing a second validator.
            self.validate_assignments(request.instance, request.assignments)?;
        }
        if usage.assignments != 0 {
            self.next_revision()?;
        }
        let rows = requests
            .iter()
            .map(|request| {
                let instance = self.instance(request.instance)?;
                Ok(Row {
                    handle: request.instance,
                    instance: instance.id,
                    definition: instance.definition.clone(),
                    assignments: request.assignments.to_vec(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StagedGroup {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            rows,
            usage,
        })
    }

    /// Revalidate the entire group before preparing all commit tables and moving
    /// values. Empty rows still bind identities; an empty write set is a no-op.
    pub fn commit_local_assignments(&mut self, stage: StagedGroup) -> Result<Receipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.cohort != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision {
            return Err(Error::Invalid(
                "local assignment group revision changed".into(),
            ));
        }
        for row in &stage.rows {
            let instance = self.instance(row.handle)?;
            if instance.id != row.instance || instance.definition != row.definition {
                return Err(Error::DefinitionChanged);
            }
            self.validate_assignments(row.handle, &row.assignments)?;
        }
        let revision = if stage.usage.assignments == 0 {
            self.revision
        } else {
            self.next_revision()?
        };
        let mut rows = Vec::with_capacity(stage.rows.len());
        let mut writes = Vec::with_capacity(stage.rows.len());
        for row in stage.rows {
            // Every slot and all allocations are ready before canonical writes.
            let slot = self.slot(row.handle)?;
            rows.push(ReceiptRow {
                instance: row.instance,
                definition: row.definition,
                assignments: row.assignments.len(),
            });
            writes.push(Write {
                slot,
                assignments: row.assignments,
            });
        }
        for write in writes {
            let instance = self.slots[write.slot]
                .value
                .as_mut()
                .expect("validated assignment group instance");
            for (index, value) in write.assignments {
                *instance
                    .locals
                    .get_mut(&index)
                    .expect("validated group local") = value;
            }
        }
        self.revision = revision;
        Ok(Receipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            before_revision: stage.revision,
            after_revision: revision,
            rows,
            usage: stage.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &std::path::Path) {
        fn field(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
            [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
        }
        fn record(tag: &[u8; 4], id: u32, bytes: &[u8]) -> Vec<u8> {
            [
                tag.as_slice(),
                &(bytes.len() as u32).to_le_bytes(),
                &0u32.to_le_bytes(),
                &id.to_le_bytes(),
                &[0; 8],
                bytes,
            ]
            .concat()
        }
        let mut schr = [0; 20];
        schr[12..16].copy_from_slice(&1u32.to_le_bytes());
        let mut variable = [0; 24];
        variable[..4].copy_from_slice(&42u32.to_le_bytes());
        let body = [
            field(b"SCHR", &schr),
            field(b"SLSD", &variable),
            field(b"SCVR", b"local_42\0"),
        ]
        .concat();
        std::fs::write(
            path.join("FalloutNV.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"SCPT", 0x300, &body),
                record(b"SCPT", 0x301, &body),
            ]
            .concat(),
        )
        .unwrap();
    }

    #[test]
    fn source_guards_revalidate_final_row_without_warming_schema_or_writing_first_row() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            root.path(),
            &["FalloutNV.esm".into()],
            Default::default(),
        )
        .unwrap();
        let catalogue = fallout_data::loaded_scripts::Catalogue::load(
            &mut store,
            Default::default(),
            |_, _| Ok(()),
        )
        .unwrap();
        let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
        let handles = catalogue
            .iter()
            .enumerate()
            .map(|(position, (_, script))| {
                world
                    .create_instance(
                        script.handle(),
                        crate::identity::Owner::Fragment {
                            activation: (position as u64 + 1).try_into().unwrap(),
                        },
                        crate::events::Context::default(),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        world.definitions.clear();
        world.block_count = 0;
        let values = [(42, Value::Number { bits: 17 })];
        let requests = handles
            .iter()
            .map(|instance| Request {
                instance: *instance,
                assignments: &values,
            })
            .collect::<Vec<_>>();
        let before = world.snapshot();
        let counts = handles
            .iter()
            .map(|handle| {
                std::sync::Arc::strong_count(&world.instance(*handle).unwrap().definition_schema)
            })
            .collect::<Vec<_>>();
        let stage = world
            .stage_local_assignments(&requests, Limits::default())
            .unwrap();
        assert_eq!(world.snapshot(), before);
        assert!(world.definitions.is_empty());
        assert_eq!(world.block_count, 0);
        for (handle, count) in handles.iter().zip(counts) {
            assert_eq!(
                std::sync::Arc::strong_count(&world.instance(*handle).unwrap().definition_schema),
                count
            );
        }
        let copied = stage.usage().copied_bytes;
        assert!(
            world
                .stage_local_assignments(
                    &requests,
                    Limits {
                        max_copied_bytes: copied - 1,
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        assert!(world.definitions.is_empty());
        world.slots[handles[1].slot]
            .value
            .as_mut()
            .unwrap()
            .definition
            .version_sha256 = "f".repeat(64);
        let changed = world.snapshot();
        assert!(matches!(
            world.commit_local_assignments(stage),
            Err(Error::DefinitionChanged)
        ));
        assert!(matches!(
            world.stage_local_assignments(&requests, Limits::default()),
            Err(Error::DefinitionChanged)
        ));
        assert_eq!(world.snapshot(), changed);
        assert!(world.definitions.is_empty());
        assert_eq!(world.block_count, 0);
    }

    #[test]
    fn private_commit_binding_value_and_revision_guards_precede_all_effects() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            root.path(),
            &["FalloutNV.esm".into()],
            Default::default(),
        )
        .unwrap();
        let catalogue = fallout_data::loaded_scripts::Catalogue::load(
            &mut store,
            Default::default(),
            |_, _| Ok(()),
        )
        .unwrap();
        for refusal in 0..6 {
            let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
            let handles = catalogue
                .iter()
                .enumerate()
                .map(|(position, (_, script))| {
                    world
                        .create_instance(
                            script.handle(),
                            crate::identity::Owner::Fragment {
                                activation: (position as u64 + 1).try_into().unwrap(),
                            },
                            crate::events::Context::default(),
                        )
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let values = [(42, Value::Number { bits: 17 })];
            let requests = handles
                .iter()
                .map(|instance| Request {
                    instance: *instance,
                    assignments: &values,
                })
                .collect::<Vec<_>>();
            let mut stage = world
                .stage_local_assignments(&requests, Limits::default())
                .unwrap();
            match refusal {
                0 => stage.campaign = CampaignId::from_bytes([99; 16]).unwrap(),
                1 => stage.cohort = "f".repeat(64),
                2 => stage.rows[1].instance = InstanceId(999.try_into().unwrap()),
                3 => stage.rows[1].definition.version_sha256 = "f".repeat(64),
                4 => {
                    stage.rows[1].assignments[0].1 = Value::Reference {
                        value: ReferenceValue::Null,
                    }
                }
                _ => {
                    world.revision = u64::MAX;
                    stage.revision = u64::MAX;
                }
            }
            let before = world.snapshot();
            assert!(world.commit_local_assignments(stage).is_err());
            assert_eq!(world.snapshot(), before);
        }
    }

    #[test]
    fn copied_charge_checks_addition_and_multiplication_overflow() {
        let mut total = usize::MAX;
        assert!(charge(&mut total, 1, 1).is_err());
        assert_eq!(total, usize::MAX);
        let mut total = 17;
        assert!(charge(&mut total, usize::MAX, 2).is_err());
        assert_eq!(total, 17);
    }
}
