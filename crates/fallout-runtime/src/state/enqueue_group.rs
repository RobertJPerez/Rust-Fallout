//! Atomic append of an explicit host-requested event vector. Neither source
//! event selection nor gameplay scheduling, dispatch or acknowledgement occurs.
use super::{InstanceHandle, World};
use crate::{
    Error, Result,
    events::{Clocks, Context, Pending, Trigger},
    identity::{CampaignId, InstanceId, ReferenceValue},
};
use fallout_data::{identity::FormKey, loaded_scripts::Handle};
use serde::Serialize;
use std::mem::size_of;

#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub instance: InstanceHandle,
    pub trigger: &'a Trigger,
    pub context: &'a Context,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Owner, ReferenceId};

    fn fixture(path: &std::path::Path) {
        fn field(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
            [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
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
        let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&14u32.to_le_bytes());
        schr[12..16].copy_from_slice(&1u32.to_le_bytes());
        let mut variable = [0; 24];
        variable[..4].copy_from_slice(&42u32.to_le_bytes());
        let body = [
            field(b"SCHR", &schr),
            field(b"SCDA", &compiled),
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
    fn private_stage_authority_source_and_late_commit_guards_preserve_state_and_cold_cache() {
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
        for refusal in 0..19 {
            let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
            let reference = world.register_reference(None).unwrap();
            let handles = catalogue
                .iter()
                .enumerate()
                .map(|(index, (_, script))| {
                    world
                        .create_instance(
                            script.handle(),
                            Owner::Fragment {
                                activation: (index as u64 + 1).try_into().unwrap(),
                            },
                            Context::default(),
                        )
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let block = Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            };
            let context = Context {
                target: Some(ReferenceValue::Live { id: reference }),
                arguments: vec![ReferenceValue::Null],
                ..Default::default()
            };
            world.definitions.clear();
            world.block_count = 0;
            let before = world.snapshot();
            let counts = handles
                .iter()
                .map(|handle| {
                    std::sync::Arc::strong_count(
                        &world.instance(*handle).unwrap().definition_schema,
                    )
                })
                .collect::<Vec<_>>();
            let requests = [
                Request {
                    instance: handles[1],
                    trigger: &block,
                    context: &context,
                },
                Request {
                    instance: handles[0],
                    trigger: &block,
                    context: &context,
                },
                Request {
                    instance: handles[1],
                    trigger: &block,
                    context: &context,
                },
            ];
            let mut stage = world
                .stage_pending_events(&requests, Limits::default())
                .unwrap();
            assert_eq!(world.snapshot(), before);
            assert!(world.definitions.is_empty());
            assert_eq!(world.block_count, 0);
            for (handle, count) in handles.iter().zip(&counts) {
                assert_eq!(
                    std::sync::Arc::strong_count(
                        &world.instance(*handle).unwrap().definition_schema
                    ),
                    *count
                );
            }
            match refusal {
                0 => stage.epoch = 0,
                1 => stage.campaign = CampaignId::from_bytes([99; 16]).unwrap(),
                2 => stage.cohort = "f".repeat(64),
                3 => stage.revision = 0,
                4 => stage.next_sequence = 0,
                5 => stage.boundary.tick = 99,
                6 => stage.usage.events = 2,
                7 => stage.rows[2].instance = InstanceId(999.try_into().unwrap()),
                8 => stage.rows[2].definition.version_sha256 = "f".repeat(64),
                9 => stage.rows[2].handle.generation += 1,
                10 => stage.rows[2].trigger = Trigger::ObjectEvent { mask: 0 },
                11 => {
                    stage.rows[2].trigger = Trigger::Block {
                        event_id: 999,
                        begin_byte_offset: 0,
                    }
                }
                12 => {
                    stage.rows[2].context.target = Some(ReferenceValue::Live {
                        id: ReferenceId(999.try_into().unwrap()),
                    })
                }
                13 => {
                    world.slots[handles[1].slot]
                        .value
                        .as_mut()
                        .unwrap()
                        .definition
                        .version_sha256 = "f".repeat(64)
                }
                14 => world.limits.max_pending_events = 2,
                15 => {
                    world.next_sequence = u64::MAX;
                    stage.next_sequence = u64::MAX;
                }
                16 => {
                    world.revision = u64::MAX;
                    stage.revision = u64::MAX;
                }
                17 => {
                    world.next_sequence = 0;
                    stage.next_sequence = 0;
                }
                _ => world.limits.max_event_arguments = 0,
            }
            let changed = world.snapshot();
            assert!(
                world.commit_pending_events(stage).is_err(),
                "refusal {refusal}"
            );
            assert_eq!(world.snapshot(), changed);
            assert!(world.definitions.is_empty());
            assert_eq!(world.block_count, 0);
            for (handle, count) in handles.iter().zip(&counts) {
                assert_eq!(
                    std::sync::Arc::strong_count(
                        &world.slots[handle.slot]
                            .value
                            .as_ref()
                            .unwrap()
                            .definition_schema
                    ),
                    *count
                );
            }
        }
    }

    #[test]
    fn copied_argument_key_and_queue_arithmetic_rejects_overflow() {
        let limits = Limits {
            max_copied_bytes: usize::MAX,
            max_source_keys: usize::MAX,
            max_source_key_bytes: usize::MAX,
            ..Default::default()
        };
        let mut usage = Usage::default();
        assert!(charge(&mut usage, usize::MAX, 2, limits).is_err());
        usage.copied_bytes = usize::MAX;
        assert!(charge(&mut usage, 1, 1, limits).is_err());
        let key = FormKey {
            profile: fallout_data::identity::ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x300,
        };
        usage = Usage {
            source_keys: usize::MAX,
            ..Default::default()
        };
        assert!(source_key(&key, &mut usage, limits).is_err());
        usage = Usage {
            source_key_bytes: usize::MAX,
            ..Default::default()
        };
        assert!(source_key(&key, &mut usage, limits).is_err());
        assert!(add(usize::MAX, 1, "enqueue group arguments").is_err());
        assert!(add(usize::MAX, 1, "pending events").is_err());
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_events: usize,
    pub max_arguments: usize,
    pub max_source_keys: usize,
    pub max_source_key_bytes: usize,
    /// Logical stage/receipt/row/Pending/argument tables and copied UTF-8,
    /// including each source Handle occurrence. Excludes allocator overhead
    /// and existing canonical queue storage moved during reservation.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_events: 256,
            max_arguments: 16_384,
            max_source_keys: 16_896,
            max_source_key_bytes: 1024 * 1024,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub events: usize,
    pub arguments: usize,
    pub source_keys: usize,
    pub source_key_bytes: usize,
    pub copied_bytes: usize,
}

/// Runtime-created immutable authority. Consuming commit prevents replay;
/// private fields and absence of Clone/Serde prevent constructing another plan.
#[derive(Debug)]
#[must_use = "staging does not queue events; commit or drop the stage"]
pub struct StagedEvents {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    next_sequence: u64,
    boundary: Clocks,
    rows: Vec<Row>,
    usage: Usage,
}
impl StagedEvents {
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn boundary(&self) -> Clocks {
        self.boundary
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
    trigger: Trigger,
    context: Context,
}
impl Row {
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    boundary: Clocks,
    next_sequence_before: u64,
    next_sequence_after: u64,
    rows: Vec<ReceiptRow>,
    usage: Usage,
}
impl Receipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn boundary(&self) -> Clocks {
        self.boundary
    }
    pub fn next_sequence_before(&self) -> u64 {
        self.next_sequence_before
    }
    pub fn next_sequence_after(&self) -> u64 {
        self.next_sequence_after
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
    sequence: u64,
    instance: InstanceId,
    definition: Handle,
}
impl ReceiptRow {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
}

fn add(left: usize, right: usize, label: &'static str) -> Result<usize> {
    left.checked_add(right).ok_or(Error::Capacity(label))
}
fn charge(usage: &mut Usage, count: usize, each: usize, limits: Limits) -> Result<()> {
    usage.copied_bytes = add(
        usage.copied_bytes,
        count
            .checked_mul(each)
            .ok_or(Error::Capacity("enqueue group copied bytes"))?,
        "enqueue group copied bytes",
    )?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("enqueue group copied bytes"));
    }
    Ok(())
}
fn source_key(key: &FormKey, usage: &mut Usage, limits: Limits) -> Result<()> {
    usage.source_keys = add(usage.source_keys, 1, "enqueue group source keys")?;
    if usage.source_keys > limits.max_source_keys {
        return Err(Error::Capacity("enqueue group source keys"));
    }
    let bytes = add(
        size_of::<FormKey>(),
        key.origin_plugin.len(),
        "enqueue group source key bytes",
    )?;
    usage.source_key_bytes = add(
        usage.source_key_bytes,
        bytes,
        "enqueue group source key bytes",
    )?;
    if usage.source_key_bytes > limits.max_source_key_bytes {
        return Err(Error::Capacity("enqueue group source key bytes"));
    }
    charge(usage, 1, key.origin_plugin.len(), limits)
}

impl World<'_> {
    fn pending_event_span(&self, events: usize) -> Result<(u64, u64)> {
        if events == 0 {
            return Ok((self.next_sequence, self.revision));
        }
        if add(self.pending.len(), events, "pending events")? > self.limits.max_pending_events {
            return Err(Error::Capacity("pending events"));
        }
        if self.next_sequence == 0 {
            return Err(Error::Invalid("zero event sequence allocator".into()));
        }
        let count = u64::try_from(events).map_err(|_| Error::Capacity("event sequences"))?;
        let next = self
            .next_sequence
            .checked_add(count)
            .ok_or(Error::Capacity("event sequences"))?;
        Ok((next, self.next_revision()?))
    }

    pub fn stage_pending_events(
        &self,
        requests: &[Request<'_>],
        limits: Limits,
    ) -> Result<StagedEvents> {
        if requests.len() > limits.max_events {
            return Err(Error::Capacity("enqueue group events"));
        }
        self.pending_event_span(requests.len())?;
        let mut usage = Usage {
            events: requests.len(),
            ..Default::default()
        };
        charge(&mut usage, 1, size_of::<StagedEvents>(), limits)?;
        charge(&mut usage, 1, size_of::<Receipt>(), limits)?;
        charge(&mut usage, 1, self.cohort.len(), limits)?;
        for each in [
            size_of::<Row>(),
            size_of::<ReceiptRow>(),
            size_of::<Pending>(),
        ] {
            charge(&mut usage, requests.len(), each, limits)?;
        }
        for request in requests {
            let instance = self.instance(request.instance)?;
            source_key(&instance.definition.key.record, &mut usage, limits)?;
            charge(
                &mut usage,
                1,
                instance.definition.version_sha256.len(),
                limits,
            )?;
            usage.arguments = add(
                usage.arguments,
                request.context.arguments.len(),
                "enqueue group arguments",
            )?;
            if usage.arguments > limits.max_arguments {
                return Err(Error::Capacity("enqueue group arguments"));
            }
            charge(
                &mut usage,
                request.context.arguments.len(),
                size_of::<ReferenceValue>(),
                limits,
            )?;
            for value in request
                .context
                .target
                .iter()
                .chain(request.context.arguments.iter())
            {
                if let ReferenceValue::Content { key } = value {
                    source_key(key, &mut usage, limits)?;
                }
            }
        }
        // Admit every row's storage before owned contexts/source strings. The
        // original validators own block identity, mask and reference semantics.
        for request in requests {
            let instance = self.instance(request.instance)?;
            Self::validate_trigger(&instance.definition_schema, request.trigger)?;
            self.validate_context(request.context)?;
        }
        let rows = requests
            .iter()
            .map(|request| {
                let instance = self.instance(request.instance)?;
                Ok(Row {
                    handle: request.instance,
                    instance: instance.id,
                    definition: instance.definition.clone(),
                    trigger: request.trigger.clone(),
                    context: request.context.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StagedEvents {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            next_sequence: self.next_sequence,
            boundary: self.clocks,
            rows,
            usage,
        })
    }

    pub fn commit_pending_events(&mut self, stage: StagedEvents) -> Result<Receipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.cohort != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision
            || stage.next_sequence != self.next_sequence
            || stage.boundary != self.clocks
        {
            return Err(Error::Invalid("enqueue group boundary changed".into()));
        }
        if stage.usage.events != stage.rows.len() {
            return Err(Error::Invalid("enqueue group count changed".into()));
        }
        for row in &stage.rows {
            let instance = self.instance(row.handle)?;
            if instance.id != row.instance || instance.definition != row.definition {
                return Err(Error::DefinitionChanged);
            }
            Self::validate_trigger(&instance.definition_schema, &row.trigger)?;
            self.validate_context(&row.context)?;
        }
        let (next_sequence, revision) = self.pending_event_span(stage.rows.len())?;
        let mut sequence = self.next_sequence;
        let mut pending = Vec::with_capacity(stage.rows.len());
        let mut rows = Vec::with_capacity(stage.rows.len());
        for row in stage.rows {
            rows.push(ReceiptRow {
                sequence,
                instance: row.instance,
                definition: row.definition,
            });
            pending.push(Pending {
                sequence,
                instance: row.instance,
                trigger: row.trigger,
                context: row.context,
                arrived: stage.boundary,
            });
            sequence = sequence
                .checked_add(1)
                .ok_or(Error::Capacity("event sequences"))?;
        }
        // Reserve the whole append before its first effect. No validation,
        // arithmetic, context clone or new allocation follows queue mutation.
        self.pending.reserve(pending.len());
        self.pending.extend(pending);
        self.next_sequence = next_sequence;
        self.revision = revision;
        Ok(Receipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            before_revision: stage.revision,
            after_revision: revision,
            boundary: stage.boundary,
            next_sequence_before: stage.next_sequence,
            next_sequence_after: next_sequence,
            rows,
            usage: stage.usage,
        })
    }
}
