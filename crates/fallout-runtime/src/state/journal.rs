//! Owned, bounded observations of the canonical pending journal. Pages retain
//! host-supplied order and context; observing never dispatches or consumes events.
use super::World;
use crate::{
    Error, Result,
    events::{Clocks, Pending},
    identity::{CampaignId, ReferenceValue},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::{cmp::Ordering, mem::size_of};

#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub after: Option<&'a Cursor>,
    /// An explicit initial anchor must still be pending. It cannot be combined
    /// with a cursor, whose sealed position already names its consumed sequence.
    pub start_after: Option<u64>,
    pub rows: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Includes sequence probes when finding an explicit initial anchor.
    pub max_visited: usize,
    pub max_rows: usize,
    pub max_arguments: usize,
    pub max_source_keys: usize,
    pub max_source_key_bytes: usize,
    /// Logical page/cursor/row/argument tables and copied UTF-8; excludes
    /// allocator overhead. FormKey fixed storage is already inside those tables.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_visited: 512,
            max_rows: 256,
            max_arguments: 16_384,
            max_source_keys: 16_640,
            max_source_key_bytes: 1024 * 1024,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub visited: usize,
    pub returned: usize,
    pub arguments: usize,
    pub source_keys: usize,
    pub source_key_bytes: usize,
    pub copied_bytes: usize,
}

/// Read continuation sealed by the runtime. This is neither saved authority nor
/// permission to acknowledge any event. Restoration always changes its epoch.
#[derive(Debug, Clone)]
pub struct Cursor {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    head: Option<u64>,
    position: usize,
    consumed: Option<u64>,
}
impl Cursor {
    fn check(&self, world: &World<'_>) -> Result<()> {
        if self.epoch != world.epoch {
            return Err(Error::StaleHandle);
        }
        if self.campaign != world.campaign || self.cohort != world.cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != world.revision {
            return Err(Error::Invalid("journal page revision changed".into()));
        }
        if self.head != world.pending.front().map(|event| event.sequence) {
            return Err(Error::Invalid("journal page head changed".into()));
        }
        if self.position > world.pending.len()
            || (self.position == 0 && self.consumed.is_some())
            || (self.position != 0
                && world
                    .pending
                    .get(self.position - 1)
                    .map(|event| event.sequence)
                    != self.consumed)
        {
            return Err(Error::Invalid("journal page sequence is missing".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Page {
    campaign: CampaignId,
    catalogue_sha256: String,
    state_revision: u64,
    boundary: Clocks,
    head: Option<u64>,
    start_after: Option<u64>,
    events: Vec<Pending>,
    complete: bool,
    usage: Usage,
    #[serde(skip)]
    cursor: Cursor,
}
impl Page {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.state_revision
    }
    pub fn boundary(&self) -> Clocks {
        self.boundary
    }
    pub fn head(&self) -> Option<u64> {
        self.head
    }
    pub fn start_after(&self) -> Option<u64> {
        self.start_after
    }
    pub fn events(&self) -> &[Pending] {
        &self.events
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }
    pub fn next_cursor(&self) -> Option<&Cursor> {
        (!self.complete).then_some(&self.cursor)
    }
    pub fn usage(&self) -> Usage {
        self.usage
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
            .ok_or(Error::Capacity("journal page copied bytes"))?,
        "journal page copied bytes",
    )?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("journal page copied bytes"));
    }
    Ok(())
}
fn visit(usage: &mut Usage, limits: Limits) -> Result<()> {
    usage.visited = add(usage.visited, 1, "journal page visited")?;
    if usage.visited > limits.max_visited {
        return Err(Error::Capacity("journal page visited"));
    }
    Ok(())
}
fn source_key(key: &FormKey, usage: &mut Usage, limits: Limits) -> Result<()> {
    usage.source_keys = add(usage.source_keys, 1, "journal page source keys")?;
    if usage.source_keys > limits.max_source_keys {
        return Err(Error::Capacity("journal page source keys"));
    }
    let bytes = add(
        size_of::<FormKey>(),
        key.origin_plugin.len(),
        "journal page source key bytes",
    )?;
    usage.source_key_bytes = add(
        usage.source_key_bytes,
        bytes,
        "journal page source key bytes",
    )?;
    if usage.source_key_bytes > limits.max_source_key_bytes {
        return Err(Error::Capacity("journal page source key bytes"));
    }
    charge(usage, 1, key.origin_plugin.len(), limits)
}

impl World<'_> {
    pub fn pending_page(&self, request: Request<'_>, limits: Limits) -> Result<Page> {
        if request.rows == 0 || request.rows > limits.max_rows {
            return Err(Error::Capacity("journal page rows"));
        }
        if request.after.is_some() && request.start_after.is_some() {
            return Err(Error::Invalid(
                "journal page has two initial positions".into(),
            ));
        }
        let mut usage = Usage::default();
        charge(&mut usage, 1, size_of::<Page>(), limits)?;
        charge(&mut usage, 2, self.cohort.len(), limits)?;
        let start_after = request
            .after
            .map_or(request.start_after, |cursor| cursor.consumed);
        let start = if let Some(cursor) = request.after {
            cursor.check(self)?;
            cursor.position
        } else if let Some(sequence) = request.start_after {
            // Only sequence metadata is probed. VecDeque indexing does not copy
            // or traverse every earlier record, even when its storage wraps.
            let (mut lower, mut upper) = (0, self.pending.len());
            let mut found = None;
            while lower < upper {
                visit(&mut usage, limits)?;
                let middle = lower + (upper - lower) / 2;
                match self.pending[middle].sequence.cmp(&sequence) {
                    Ordering::Less => lower = middle + 1,
                    Ordering::Greater => upper = middle,
                    Ordering::Equal => {
                        found = Some(middle);
                        break;
                    }
                }
            }
            found
                .ok_or_else(|| Error::Invalid("journal page start sequence is not pending".into()))?
                .checked_add(1)
                .ok_or(Error::Capacity("journal page sequence position"))?
        } else {
            0
        };
        let count = request.rows.min(self.pending.len() - start);
        let end = start
            .checked_add(count)
            .ok_or(Error::Capacity("journal page sequence position"))?;
        let mut consumed = start_after;
        for event in self.pending.range(start..end) {
            visit(&mut usage, limits)?;
            if event.sequence == 0
                || event.sequence >= self.next_sequence
                || consumed.is_some_and(|prior| event.sequence <= prior)
            {
                return Err(Error::Invalid(
                    "journal page sequence order is invalid".into(),
                ));
            }
            usage.arguments = add(
                usage.arguments,
                event.context.arguments.len(),
                "journal page arguments",
            )?;
            if usage.arguments > limits.max_arguments {
                return Err(Error::Capacity("journal page arguments"));
            }
            charge(&mut usage, 1, size_of::<Pending>(), limits)?;
            charge(
                &mut usage,
                event.context.arguments.len(),
                size_of::<ReferenceValue>(),
                limits,
            )?;
            for value in event
                .context
                .target
                .iter()
                .chain(event.context.arguments.iter())
            {
                if let ReferenceValue::Content { key } = value {
                    source_key(key, &mut usage, limits)?;
                }
            }
            consumed = Some(event.sequence);
            usage.returned = add(usage.returned, 1, "journal page rows")?;
        }
        // Admit the whole slice before schema/context validation or owned copies.
        for event in self.pending.range(start..end) {
            let instance = self.instance(self.handle(event.instance)?)?;
            Self::validate_trigger(&instance.definition_schema, &event.trigger)?;
            self.validate_context(&event.context)?;
            if !event.arrived.no_later_than(self.clocks) {
                return Err(Error::Invalid(
                    "journal page arrival is after current clocks".into(),
                ));
            }
        }
        let head = self.pending.front().map(|event| event.sequence);
        Ok(Page {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            state_revision: self.revision,
            boundary: self.clocks,
            head,
            start_after,
            events: self.pending.range(start..end).cloned().collect(),
            complete: end == self.pending.len(),
            usage,
            cursor: Cursor {
                epoch: self.epoch,
                campaign: self.campaign,
                cohort: self.cohort.clone(),
                revision: self.revision,
                head,
                position: end,
                consumed,
            },
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
                record(
                    b"SCPT",
                    0x300,
                    &[
                        field(b"SCHR", &schr),
                        field(b"SCDA", &compiled),
                        field(b"SLSD", &variable),
                        field(b"SCVR", b"local_42\0"),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        )
        .unwrap();
    }
    #[test]
    fn private_cursor_source_and_bad_final_event_guards_preserve_all_state_and_cold_cache() {
        use crate::{
            events::{Context, Trigger},
            identity::{InstanceId, Owner, ReferenceId},
        };
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
        for refusal in 0..15 {
            let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
            let reference = world.register_reference(None).unwrap();
            let definition = catalogue.iter().next().unwrap().1.handle();
            let context = Context {
                target: Some(ReferenceValue::Live { id: reference }),
                ..Default::default()
            };
            let first = world
                .create_instance(
                    definition,
                    Owner::Fragment {
                        activation: 1.try_into().unwrap(),
                    },
                    Context::default(),
                )
                .unwrap();
            let last = world
                .create_instance(
                    definition,
                    Owner::Fragment {
                        activation: 2.try_into().unwrap(),
                    },
                    Context::default(),
                )
                .unwrap();
            world
                .enqueue(first, Trigger::ObjectEvent { mask: 1 }, context.clone())
                .unwrap();
            world
                .enqueue(
                    last,
                    Trigger::Block {
                        event_id: 0,
                        begin_byte_offset: 0,
                    },
                    context,
                )
                .unwrap();
            world.definitions.clear();
            world.block_count = 0;
            let before = world.snapshot();
            let count =
                std::sync::Arc::strong_count(&world.instance(first).unwrap().definition_schema);
            let page = world
                .pending_page(
                    Request {
                        after: None,
                        start_after: None,
                        rows: 1,
                    },
                    Limits::default(),
                )
                .unwrap();
            assert_eq!(world.snapshot(), before);
            assert!(world.definitions.is_empty());
            assert_eq!(world.block_count, 0);
            assert_eq!(
                std::sync::Arc::strong_count(&world.instance(first).unwrap().definition_schema),
                count
            );
            let mut cursor = page.cursor().clone();
            match refusal {
                0 => cursor.epoch = 0,
                1 => cursor.campaign = CampaignId::from_bytes([99; 16]).unwrap(),
                2 => cursor.cohort = "f".repeat(64),
                3 => cursor.revision = 0,
                4 => cursor.head = None,
                5 => cursor.position = 3,
                6 => cursor.consumed = Some(999),
                7 => world.pending[1].trigger = Trigger::ObjectEvent { mask: 0 },
                8 => {
                    world.pending[1].trigger = Trigger::Block {
                        event_id: 999,
                        begin_byte_offset: 0,
                    }
                }
                9 => world.pending[1].instance = InstanceId(999.try_into().unwrap()),
                10 => {
                    world.pending[1].context.target = Some(ReferenceValue::Live {
                        id: ReferenceId(999.try_into().unwrap()),
                    })
                }
                11 => world.pending[1]
                    .context
                    .arguments
                    .push(ReferenceValue::Content {
                        key: FormKey {
                            profile: fallout_data::identity::ProfileId::NvOriginal,
                            origin_plugin: "BAD.ESM".into(),
                            local_id: 0x300,
                        },
                    }),
                12 => world.pending[1].arrived.tick = 1,
                13 => world.pending[1].sequence = 1,
                _ => {
                    world.slots[last.slot]
                        .value
                        .as_mut()
                        .unwrap()
                        .definition
                        .version_sha256 = "f".repeat(64)
                }
            }
            let changed = world.snapshot();
            let request = if refusal < 7 {
                Request {
                    after: Some(&cursor),
                    start_after: None,
                    rows: 1,
                }
            } else {
                Request {
                    after: None,
                    start_after: None,
                    rows: 2,
                }
            };
            assert!(world.pending_page(request, Limits::default()).is_err());
            assert_eq!(world.snapshot(), changed);
            assert!(world.definitions.is_empty());
            assert_eq!(world.block_count, 0);
        }
    }
    #[test]
    fn charge_and_usage_arithmetic_refuse_overflow() {
        let mut usage = Usage::default();
        assert!(
            charge(
                &mut usage,
                usize::MAX,
                2,
                Limits {
                    max_copied_bytes: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
        usage.copied_bytes = usize::MAX;
        assert!(
            charge(
                &mut usage,
                1,
                1,
                Limits {
                    max_copied_bytes: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
        usage.visited = usize::MAX;
        assert!(
            visit(
                &mut usage,
                Limits {
                    max_visited: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let key = FormKey {
            profile: fallout_data::identity::ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x300,
        };
        usage = Usage {
            source_keys: usize::MAX,
            ..Default::default()
        };
        assert!(
            source_key(
                &key,
                &mut usage,
                Limits {
                    max_source_keys: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
        usage = Usage {
            source_key_bytes: usize::MAX,
            ..Default::default()
        };
        assert!(
            source_key(
                &key,
                &mut usage,
                Limits {
                    max_source_key_bytes: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(add(usize::MAX, 1, "journal page arguments").is_err());
    }
}
