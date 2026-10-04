//! Inspect source-less local declarations and exercise canonical runtime state
//! using explicit synthetic values. No retail event list is captured or run.
use super::{Result, inspection_input::Order};
use fallout_data::{
    identity::FormKey,
    loaded_scripts::{self, Catalogue},
    record_metadata,
};
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
    reference_state::{
        Pose, SourceReferenceGroupLimits, SourceReferenceLimits, SourceReferenceRequest, State,
    },
    save::{Captured, Recovery, Repository},
    schema::{self, Kind},
    snapshot::Snapshot,
    state::{HostLimits, HostRequirements, initialization},
    state::{assignment_group, enqueue_group, journal, observation},
};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

struct HostCheckInput {
    snapshot: Snapshot,
    requirements: HostRequirements,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplicitSourceReference {
    authored: FormKey,
    allowed_kinds: Vec<[u8; 4]>,
    cell: FormKey,
    pose: Pose,
    enabled: bool,
}
impl ExplicitSourceReference {
    fn request(&self) -> SourceReferenceRequest<'_> {
        SourceReferenceRequest {
            authored: &self.authored,
            allowed_kinds: &self.allowed_kinds,
            cell: &self.cell,
            pose: &self.pose,
            enabled: self.enabled,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceGroupInput {
    existing: ExplicitSourceReference,
    retained_state: State,
    requests: Vec<ExplicitSourceReference>,
}
impl SourceGroupInput {
    fn read(path: &Path) -> Result<Self> {
        let bytes = read_bounded(
            path,
            64 * 1024,
            "Source reference group input byte budget exceeded",
        )?;
        let input: Self = serde_json::from_slice(&bytes)?;
        let limits = SourceReferenceGroupLimits::default();
        if input.requests.len() > limits.max_requests {
            return Err("Source reference group request budget exceeded".into());
        }
        Ok(input)
    }
    fn probe(
        self,
        catalogue: &Catalogue,
        content: &Content,
        install: &Path,
        repository_path: &Path,
    ) -> Result<Json> {
        let mut harness = engineering_world(catalogue)?;
        let world = &mut harness.world;
        let created = world.commit_source_reference(world.stage_source_reference(
            self.existing.request(),
            content,
            SourceReferenceLimits::default(),
        )?)?;
        let existing_id = created.view().reference();
        // Retained state is another explicit host input, still source-cell-bound.
        if content.source_form(world, self.retained_state.cell())?.kind != *b"CELL" {
            return Err("Retained source reference cell must be CELL".into());
        }
        let retained_edit = world.commit_reference_state(
            world.stage_reference_state(created.view(), self.retained_state)?,
        )?;
        let before = world.snapshot();
        let requests = self
            .requests
            .iter()
            .map(ExplicitSourceReference::request)
            .collect::<Vec<_>>();
        let stage = world.stage_source_reference_group(
            &requests,
            content,
            SourceReferenceGroupLimits::default(),
        )?;
        if world.snapshot() != before {
            return Err("Source group staging changed canonical state".into());
        }
        let admission = world.commit_source_reference_group(stage)?;
        let current = world.snapshot();
        let mut restored = World::restore(catalogue, current.clone(), Limits::default())?;
        let reused =
            restored.commit_source_reference_group(restored.stage_source_reference_group(
                &requests,
                content,
                SourceReferenceGroupLimits::default(),
            )?)?;
        if restored.snapshot() != current || reused.usage().created != 0 {
            return Err("Restored source group changed retained canonical state".into());
        }
        for row in admission.rows() {
            if serde_json::to_value(row.view())?
                != serde_json::to_value(restored.reference_view(row.view().reference())?)?
            {
                return Err("Source group view differs after restoration".into());
            }
        }
        // Create only after the whole request, group and round-trip checks pass.
        let repository = Repository::create(repository_path, &[install.into()], world.campaign())?;
        let before_world = World::restore(catalogue, before.clone(), Limits::default())?;
        let before_write = repository.commit(&Captured::at_boundary(&before_world))?;
        let current_write = repository.commit(&Captured::at_boundary(world))?;
        let (loaded, load) = repository.load(catalogue, Limits::default(), Recovery::Strict)?;
        if loaded.snapshot() != current {
            return Err("Native source group boundary differs".into());
        }
        Ok(
            json!({"scope":"Explicit source-header-admitted reference identities and host-selected state",
            "existing_initialization":created,"retained_edit":retained_edit,"existing_id":existing_id,
            "admission":admission,"restored_reuse":reused,"before_snapshot":before,"current_snapshot":current,
            "before_write":before_write,"current_write":current_write,"native_load":load,
            "snapshot_sha256":format!("{:x}",Sha256::digest(current.encode(Limits::default().max_snapshot_bytes)?)),
            "stage_preserved_state":true,"canonical_state_round_trip_equal":true,
            "source_membership_inferred":false,"bytecode_executed":false,"retail_parity_accepted":false}),
        )
    }
}
fn read_bounded(path: &Path, maximum: u64, message: &'static str) -> Result<Vec<u8>> {
    let file = fallout_data::baseline::open_source(path)?;
    if file.metadata()?.len() > maximum {
        return Err(message.into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(message.into());
    }
    Ok(bytes)
}
impl HostCheckInput {
    fn read(snapshot: &Path, requirements: &Path) -> Result<Self> {
        let limits = Limits::default();
        let requirements = read_bounded(
            requirements,
            1024 * 1024,
            "Host requirements input byte budget exceeded",
        )?;
        let requirements = serde_json::from_slice(&requirements)?;
        let snapshot = read_bounded(
            snapshot,
            limits.max_snapshot_bytes as u64,
            "Host snapshot input byte budget exceeded",
        )?;
        Ok(Self {
            snapshot: Snapshot::decode(&snapshot, limits)?,
            requirements,
        })
    }
    fn probe(self, catalogue: &Catalogue) -> Result<Json> {
        let limits = Limits::default();
        let world = World::restore(catalogue, self.snapshot, limits)?;
        let before = world.snapshot();
        let readiness = world.check_host_requirements(&self.requirements, HostLimits::default())?;
        if world.snapshot() != before {
            return Err("Host requirements check changed canonical state".into());
        }
        Ok(json!({
            "scope":"Read-only availability of explicitly required canonical data at this revision",
            "readiness":readiness,"canonical_data_available":readiness.canonical_data_available(),
            "first_unavailable":readiness.first_unavailable(),
            "restored_snapshot_sha256":format!("{:x}",Sha256::digest(before.encode(limits.max_snapshot_bytes)?)),
            "canonical_state_unchanged":true,"bytecode_executed":false,"retail_parity_accepted":false
        }))
    }
}

/// A bounded explicit inspector request, not a persisted script continuation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EngineeringCommit {
    sequence: u64,
    assignments: Vec<fallout_runtime::snapshot::Local>,
    acknowledge: bool,
}
impl EngineeringCommit {
    pub(super) fn read(path: &Path) -> Result<Self> {
        const MAXIMUM_BYTES: u64 = 64 * 1024;
        let bytes = read_bounded(
            path,
            MAXIMUM_BYTES,
            "Engineering event-commit input byte budget exceeded",
        )?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub(super) fn stage(
        self,
        world: &World<'_>,
    ) -> fallout_runtime::Result<fallout_runtime::state::event_commit::StagedEventChanges> {
        let assignments: Vec<_> = self
            .assignments
            .into_iter()
            .map(|local| (local.index, local.value))
            .collect();
        world.stage_event_changes(self.sequence, &assignments, self.acknowledge)
    }
}

pub(super) struct EngineeringWorld<'a> {
    pub world: World<'a>,
    pub handles: Vec<(
        fallout_runtime::identity::InstanceId,
        fallout_runtime::state::InstanceHandle,
    )>,
    pub numbers: u64,
    pub references: u64,
    pub unknown: u64,
    pub initializations: Vec<initialization::Receipt>,
}

/// Shared deterministic harness inputs. This does not initialize a game session.
pub(super) fn engineering_world(catalogue: &Catalogue) -> Result<EngineeringWorld<'_>> {
    let limits = Limits::default();
    let mut world = World::with_campaign(catalogue, limits, CampaignId::from_bytes([0x28; 16])?)?;
    let reference = world.register_reference(None)?;
    world.advance_clocks(Clocks {
        tick: 1,
        game_nanoseconds: 100,
        menu_nanoseconds: 20,
        real_nanoseconds: 120,
    })?;
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
    };
    let mut handles = Vec::new();
    let mut initializations = Vec::new();
    let mut numbers = 0_u64;
    let mut references = 0_u64;
    let mut unknown = 0_u64;
    let patterns = [
        0,
        1,
        u64::MAX,
        0x8000000000000000,
        0x7ff0000000000000,
        0x7ff8123456789abc,
        0xfff0123456789abc,
        0x4340000000000001,
    ];
    for (_, script) in catalogue.iter() {
        let locals = schema::locals(script);
        if locals.is_empty() {
            continue;
        }
        let activation = (handles.len() as u64 + 1).try_into()?;
        let mut assignments = Vec::with_capacity(locals.len());
        for local in locals.values() {
            let value = match local.kind {
                Kind::Float | Kind::Integer => {
                    let bits = patterns[numbers as usize % patterns.len()];
                    numbers += 1;
                    Value::Number { bits }
                }
                Kind::Reference => {
                    references += 1;
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    }
                }
                _ => {
                    unknown += 1;
                    continue;
                }
            };
            assignments.push((local.index, value));
        }
        let stage = world.stage_instance_initialization(
            script.handle(),
            &Owner::Fragment { activation },
            &context,
            &assignments,
            initialization::Limits::default(),
        )?;
        let (receipt, handle) = world.commit_instance_initialization(stage)?;
        initializations.push(receipt);
        if let Some(instruction) = script
            .program()?
            .iter()
            .flat_map(|program| program.instructions.iter())
            .find(|instruction| instruction.event.is_some())
        {
            let event = instruction.event.expect("selected event");
            world.enqueue(
                handle,
                Trigger::Block {
                    event_id: event.id,
                    begin_byte_offset: instruction.bytes.start as u32,
                },
                context.clone(),
            )?;
        }
        handles.push((world.instance(handle)?.id(), handle));
    }
    Ok(EngineeringWorld {
        world,
        handles,
        numbers,
        references,
        unknown,
        initializations,
    })
}
fn selected_local_probe(catalogue: &Catalogue) -> Result<Option<Json>> {
    for (_, script) in catalogue.iter() {
        let indices = schema::locals(script)
            .into_values()
            .filter(|local| matches!(local.kind, Kind::Float | Kind::Integer | Kind::Reference))
            .take(7)
            .map(|local| local.index)
            .collect::<Vec<_>>();
        let Some(&unset_index) = indices.last() else {
            continue;
        };
        let mut world = World::with_campaign(
            catalogue,
            Limits::default(),
            CampaignId::from_bytes([0x29; 16])?,
        )?;
        let reference = world.register_reference(None)?;
        let context = Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Live { id: reference }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
                ReferenceValue::Live { id: reference },
            ],
        };
        let handle = world.create_instance(
            script.handle(),
            Owner::Fragment {
                activation: 1.try_into()?,
            },
            context,
        )?;
        let mut assignments = Vec::new();
        let mut numbers = 0;
        let mut references = 0;
        let declarations = schema::locals(script);
        for &index in indices.iter().filter(|&&index| index != unset_index) {
            let value = match declarations[&index].kind {
                Kind::Float | Kind::Integer => {
                    let bits = [0x8000000000000000_u64, 0x7ff8123456789abc][numbers % 2];
                    numbers += 1;
                    Value::Number { bits }
                }
                Kind::Reference => {
                    let value = match references % 3 {
                        0 => ReferenceValue::Null,
                        1 => ReferenceValue::Content {
                            key: script.handle().key.record.clone(),
                        },
                        _ => ReferenceValue::Live { id: reference },
                    };
                    references += 1;
                    Value::Reference { value }
                }
                _ => return Err("Selected observation declaration changed".into()),
            };
            assignments.push((index, value));
        }
        world.assign(handle, &assignments)?;
        let indices = indices.into_iter().rev().collect::<Vec<_>>();
        let before = world.snapshot();
        let observed = world.observe_locals(handle, &indices, observation::Limits::default())?;
        if world.snapshot() != before {
            return Err("Local observation changed canonical state".into());
        }
        let restored = World::restore(catalogue, before.clone(), Limits::default())?;
        let cold = restored.observe_locals(
            restored.handle(observed.instance())?,
            &indices,
            observation::Limits::default(),
        )?;
        if observed != cold || restored.snapshot() != before {
            return Err("Local observation differs after restore".into());
        }
        return Ok(Some(
            json!({"scope":"Explicit isolated source-loaded engineering instance; no inferred local defaults",
            "indices":indices,"assignments":assignments,"retained_uninitialized_index":unset_index,
            "observation":observed,"restored_observation":cold,"snapshot":before,
            "read_preserved_state":true,"persistent_observations_equal":true,"bytecode_executed":false,"retail_parity_accepted":false}),
        ));
    }
    Ok(None)
}
fn local_assignment_group_probe(catalogue: &Catalogue) -> Result<Option<Json>> {
    let mut selected = Vec::new();
    for (_, script) in catalogue.iter() {
        let declarations = schema::locals(script);
        let indices = declarations
            .values()
            .filter(|local| matches!(local.kind, Kind::Float | Kind::Integer | Kind::Reference))
            .take(7)
            .map(|local| local.index)
            .collect::<Vec<_>>();
        if indices.len() >= 2 {
            selected.push((script, declarations, indices));
        }
        if selected.len() == 2 {
            break;
        }
    }
    if selected.len() != 2 {
        return Ok(None);
    }
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x2a; 16])?,
    )?;
    let reference = world.register_reference(None)?;
    let mut inputs = Vec::new();
    for (position, (script, declarations, indices)) in selected.into_iter().enumerate() {
        let context = Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Live { id: reference }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
                ReferenceValue::Live { id: reference },
            ],
        };
        let handle = world.create_instance(
            script.handle(),
            Owner::Fragment {
                activation: (position as u64 + 1).try_into()?,
            },
            context.clone(),
        )?;
        if let Some(instruction) = script
            .program()?
            .iter()
            .flat_map(|program| program.instructions.iter())
            .find(|instruction| instruction.event.is_some())
        {
            world.enqueue(
                handle,
                Trigger::Block {
                    event_id: instruction.event.expect("selected group event").id,
                    begin_byte_offset: instruction.bytes.start as u32,
                },
                context,
            )?;
        }
        let mut assignments = Vec::new();
        let mut numbers = position;
        let mut references = 0;
        for &index in &indices[..indices.len() - 1] {
            let value = match declarations[&index].kind {
                Kind::Float | Kind::Integer => {
                    let bits = [
                        0x8000000000000000_u64,
                        0x7ff8123456789abc,
                        0x7ff8123456789abd,
                    ][numbers % 3];
                    numbers += 1;
                    Value::Number { bits }
                }
                Kind::Reference => {
                    let value = match references % 3 {
                        0 => ReferenceValue::Null,
                        1 => ReferenceValue::Content {
                            key: script.handle().key.record.clone(),
                        },
                        _ => ReferenceValue::Live { id: reference },
                    };
                    references += 1;
                    Value::Reference { value }
                }
                _ => return Err("Group assignment declaration changed".into()),
            };
            assignments.push((index, value));
        }
        inputs.push((handle, assignments));
    }
    // Caller order deliberately differs from persistent instance/slot order.
    inputs.reverse();
    let before = world.snapshot();
    let requests = inputs
        .iter()
        .map(|(instance, assignments)| assignment_group::Request {
            instance: *instance,
            assignments,
        })
        .collect::<Vec<_>>();
    let stage = world.stage_local_assignments(&requests, assignment_group::Limits::default())?;
    if world.snapshot() != before {
        return Err("Group assignment staging changed canonical state".into());
    }
    let explicit = stage.rows().iter().map(|row| json!({
        "instance":row.instance(),"definition":row.definition(),"assignments":row.assignments(),
    })).collect::<Vec<_>>();
    let receipt = world.commit_local_assignments(stage)?;
    let current = world.snapshot();
    if before.pending_events != current.pending_events || before.clocks != current.clocks {
        return Err("Group assignment changed the event journal or clocks".into());
    }
    let restored = World::restore(catalogue, current.clone(), Limits::default())?;
    if restored.snapshot() != current {
        return Err("Group assignment differs after restore".into());
    }
    Ok(Some(json!({
        "scope":"Explicit isolated host writes to two source-loaded instances; pending events retained",
        "inputs":explicit,"receipt":receipt,"before_snapshot":before,"current_snapshot":current,
        "restored_snapshot":restored.snapshot(),"stage_preserved_state":true,"journal_unchanged":true,
        "canonical_round_trip_equal":true,"bytecode_executed":false,"retail_parity_accepted":false,
    })))
}
fn journal_pages(world: &World<'_>) -> fallout_runtime::Result<Vec<journal::Page>> {
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = world.pending_page(
            journal::Request {
                after: cursor.as_ref(),
                start_after: None,
                rows: 2,
            },
            journal::Limits::default(),
        )?;
        let complete = page.is_complete();
        cursor = Some(page.cursor().clone());
        pages.push(page);
        if complete {
            return Ok(pages);
        }
    }
}
fn journal_page_probe(catalogue: &Catalogue) -> Result<Option<Json>> {
    for (_, script) in catalogue.iter() {
        let Some(trigger) = script
            .program()?
            .iter()
            .flat_map(|program| program.instructions.iter())
            .find_map(|instruction| {
                instruction.event.map(|event| Trigger::Block {
                    event_id: event.id,
                    begin_byte_offset: instruction.bytes.start as u32,
                })
            })
        else {
            continue;
        };
        let mut world = World::with_campaign(
            catalogue,
            Limits::default(),
            CampaignId::from_bytes([0x2b; 16])?,
        )?;
        let reference = world.register_reference(None)?;
        let a = Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Content {
                key: script.handle().key.record.clone(),
            }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
                ReferenceValue::Live { id: reference },
            ],
        };
        let b = Context {
            calling_reference: None,
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Live { id: reference }),
            arguments: vec![
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
                ReferenceValue::Null,
                ReferenceValue::Live { id: reference },
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
            ],
        };
        let c = Context {
            calling_reference: Some(reference),
            containing_reference: None,
            target: Some(ReferenceValue::Null),
            arguments: vec![
                ReferenceValue::Live { id: reference },
                ReferenceValue::Content {
                    key: script.handle().key.record.clone(),
                },
                ReferenceValue::Null,
            ],
        };
        let handle = world.create_instance(
            script.handle(),
            Owner::Fragment {
                activation: 1.try_into()?,
            },
            a.clone(),
        )?;
        world.advance_clocks(Clocks {
            tick: 7,
            game_nanoseconds: 100,
            menu_nanoseconds: 200,
            real_nanoseconds: 300,
        })?;
        world.enqueue(handle, Trigger::ObjectEvent { mask: 0x80000001 }, a.clone())?;
        world.enqueue(handle, trigger.clone(), b)?;
        world.advance_clocks(Clocks {
            tick: 9,
            game_nanoseconds: 101,
            menu_nanoseconds: 202,
            real_nanoseconds: 303,
        })?;
        world.enqueue(handle, Trigger::ObjectEvent { mask: 0xdeadbeef }, c)?;
        world.enqueue(handle, trigger, a)?;
        let before = world.snapshot();
        let pages = journal_pages(&world)?;
        if world.snapshot() != before {
            return Err("Journal paging changed canonical state".into());
        }
        let restored = World::restore(catalogue, before.clone(), Limits::default())?;
        let restored_pages = journal_pages(&restored)?;
        if restored.snapshot() != before
            || serde_json::to_value(&pages)? != serde_json::to_value(&restored_pages)?
        {
            return Err("Journal pages differ after restore".into());
        }
        return Ok(Some(json!({
            "scope":"Exact isolated host-supplied pending journal observations; no dispatch or retail scheduling",
            "rows_per_page":2,"pages":pages,"restored_pages":restored_pages,"snapshot":before,
            "read_preserved_state":true,"persistent_pages_equal":true,"bytecode_executed":false,"retail_parity_accepted":false,
        })));
    }
    Ok(None)
}
fn enqueue_group_probe(catalogue: &Catalogue) -> Result<Option<Json>> {
    let mut selected = Vec::new();
    for (_, script) in catalogue.iter() {
        if let Some(trigger) = script
            .program()?
            .iter()
            .flat_map(|program| program.instructions.iter())
            .find_map(|instruction| {
                instruction.event.map(|event| Trigger::Block {
                    event_id: event.id,
                    begin_byte_offset: instruction.bytes.start as u32,
                })
            })
        {
            selected.push((script, trigger));
        }
        if selected.len() == 2 {
            break;
        }
    }
    if selected.len() != 2 {
        return Ok(None);
    }
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([45; 16])?,
    )?;
    let reference = world.register_reference(None)?;
    let mut handles = Vec::new();
    for (position, (script, _)) in selected.iter().enumerate() {
        handles.push(world.create_instance(
            script.handle(),
            Owner::Fragment {
                activation: (position as u64 + 1).try_into()?,
            },
            Context::default(),
        )?);
    }
    world.advance_clocks(Clocks {
        tick: 11,
        game_nanoseconds: 111,
        menu_nanoseconds: 222,
        real_nanoseconds: 333,
    })?;
    let a = Context {
        calling_reference: Some(reference),
        containing_reference: None,
        target: Some(ReferenceValue::Content {
            key: selected[1].0.handle().key.record.clone(),
        }),
        arguments: vec![ReferenceValue::Null, ReferenceValue::Live { id: reference }],
    };
    let b = Context {
        calling_reference: None,
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![
            ReferenceValue::Content {
                key: selected[0].0.handle().key.record.clone(),
            },
            ReferenceValue::Null,
        ],
    };
    let c = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Null),
        arguments: vec![
            ReferenceValue::Live { id: reference },
            ReferenceValue::Content {
                key: selected[1].0.handle().key.record.clone(),
            },
            ReferenceValue::Null,
        ],
    };
    // Keep one prior observation so appending also proves retained journal order.
    world.enqueue(
        handles[0],
        Trigger::ObjectEvent { mask: 0x40000000 },
        Context::default(),
    )?;
    let mask = Trigger::ObjectEvent { mask: 0x80000001 };
    let requests = [
        enqueue_group::Request {
            instance: handles[1],
            trigger: &selected[1].1,
            context: &a,
        },
        enqueue_group::Request {
            instance: handles[0],
            trigger: &selected[0].1,
            context: &b,
        },
        enqueue_group::Request {
            instance: handles[1],
            trigger: &mask,
            context: &c,
        },
    ];
    let before = world.snapshot();
    let stage = world.stage_pending_events(&requests, enqueue_group::Limits::default())?;
    if world.snapshot() != before {
        return Err("Event batch staging changed canonical state".into());
    }
    let inputs=stage.rows().iter().map(|row|json!({
        "instance":row.instance(),"definition":row.definition(),"trigger":row.trigger(),"context":row.context(),
    })).collect::<Vec<_>>();
    let receipt = world.commit_pending_events(stage)?;
    let current = world.snapshot();
    let restored = World::restore(catalogue, current.clone(), Limits::default())?;
    if restored.snapshot() != current {
        return Err("Event batch differs after restore".into());
    }
    Ok(Some(json!({
        "scope":"Explicit host-selected ordered event append; source block identities validated without execution",
        "inputs":inputs,"receipt":receipt,"before_snapshot":before,"current_snapshot":current,
        "restored_snapshot":restored.snapshot(),"stage_preserved_state":true,"canonical_round_trip_equal":true,
        "bytecode_executed":false,"retail_parity_accepted":false,
    })))
}
fn instance_initialization_group_probe(catalogue: &Catalogue) -> Result<Option<Json>> {
    use initialization::group;
    let selected = catalogue
        .iter()
        .map(|(_, script)| script)
        .filter(|script| {
            schema::locals(script)
                .values()
                .filter(|local| matches!(local.kind, Kind::Float | Kind::Integer | Kind::Reference))
                .count()
                >= 2
        })
        .take(2)
        .collect::<Vec<_>>();
    if selected.len() != 2 {
        return Ok(None);
    }
    let scripts = [selected[0], selected[0], selected[1]];
    let local_capacity = schema::locals(selected[0])
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(schema::locals(selected[1]).len()))
        .ok_or("Group local capacity overflow")?;
    let mut block_capacity = 0usize;
    for script in &selected {
        block_capacity = block_capacity
            .checked_add(
                script
                    .program()?
                    .iter()
                    .flat_map(|program| program.instructions.iter())
                    .filter(|instruction| instruction.event.is_some())
                    .count(),
            )
            .ok_or("Group block capacity overflow")?;
    }
    let limits = Limits {
        max_instances: 3,
        max_locals: local_capacity,
        max_event_blocks: block_capacity,
        ..Default::default()
    };
    let mut world = World::with_campaign(catalogue, limits, CampaignId::from_bytes([53; 16])?)?;
    let reference = world.register_reference(None)?;
    world.advance_clocks(Clocks {
        tick: 12,
        game_nanoseconds: 400,
        menu_nanoseconds: 500,
        real_nanoseconds: 600,
    })?;
    let owners = [1_u64, 2, 3].map(|activation| Owner::Fragment {
        activation: activation.try_into().expect("positive explicit activation"),
    });
    let contexts = [
        Context {
            calling_reference: Some(reference),
            containing_reference: None,
            target: Some(ReferenceValue::Content {
                key: scripts[0].handle().key.record.clone(),
            }),
            arguments: vec![ReferenceValue::Null, ReferenceValue::Live { id: reference }],
        },
        Context {
            calling_reference: None,
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Live { id: reference }),
            arguments: vec![ReferenceValue::Content {
                key: scripts[1].handle().key.record.clone(),
            }],
        },
        Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Null),
            arguments: vec![
                ReferenceValue::Live { id: reference },
                ReferenceValue::Content {
                    key: scripts[2].handle().key.record.clone(),
                },
                ReferenceValue::Null,
            ],
        },
    ];
    let patterns = [
        [0x8000000000000000, 0x7ff8123456789abc],
        [0x7ff8123456789abd, u64::MAX],
        [1, 0x7ff8123456789abc],
    ];
    let mut assignments = Vec::new();
    for (position, script) in scripts.iter().enumerate() {
        let locals = schema::locals(script)
            .into_values()
            .filter(|local| matches!(local.kind, Kind::Float | Kind::Integer | Kind::Reference))
            .collect::<Vec<_>>();
        let mut numbers = 0;
        let mut references = position;
        let mut values = Vec::new();
        // Omit the last supported declaration as an explicit uninitialized
        // value, alongside every unsupported/zero-index source declaration.
        for local in &locals[..locals.len() - 1] {
            let value = match local.kind {
                Kind::Float | Kind::Integer => {
                    let bits = patterns[position][numbers % 2];
                    numbers += 1;
                    Value::Number { bits }
                }
                Kind::Reference => {
                    let value = match references % 3 {
                        0 => ReferenceValue::Null,
                        1 => ReferenceValue::Content {
                            key: script.handle().key.record.clone(),
                        },
                        _ => ReferenceValue::Live { id: reference },
                    };
                    references += 1;
                    Value::Reference { value }
                }
                _ => unreachable!("selected supported declaration"),
            };
            values.push((local.index, value));
        }
        assignments.push(values);
    }
    let requests = (0..3)
        .map(|position| group::Request {
            definition: scripts[position].handle(),
            owner: &owners[position],
            context: &contexts[position],
            assignments: &assignments[position],
        })
        .collect::<Vec<_>>();
    let before = world.snapshot();
    let stage = world.stage_instance_initialization_group(&requests, group::Limits::default())?;
    if world.snapshot() != before {
        return Err("Group initialization stage changed canonical state".into());
    }
    let inputs = requests.iter().map(|request| json!({"definition":request.definition,"owner":request.owner,"context":request.context,"assignments":request.assignments})).collect::<Vec<_>>();
    let (receipt, handles) = world.commit_instance_initialization_group(stage)?;
    let current = world.snapshot();
    if receipt.after_revision != receipt.before_revision + 1
        || receipt.next_instance_after != receipt.next_instance_before + 3
    {
        return Err("Group publication identity/revision differs".into());
    }
    let restored = World::restore(catalogue, current.clone(), limits)?;
    if restored.snapshot() != current || handles.iter().any(|old| restored.instance(*old).is_ok()) {
        return Err("Group cold state or handle epoch differs".into());
    }
    let fourth_owner = Owner::Fragment {
        activation: 99.try_into()?,
    };
    let fourth = [group::Request {
        definition: selected[0].handle(),
        owner: &fourth_owner,
        context: &contexts[0],
        assignments: &[],
    }];
    if world
        .stage_instance_initialization_group(&fourth, group::Limits::default())
        .is_ok()
        || world.snapshot() != current
    {
        return Err("Tight instance capacity consumed group state".into());
    }
    let local_world = World::restore(
        catalogue,
        current.clone(),
        Limits {
            max_instances: 4,
            ..limits
        },
    )?;
    if local_world
        .stage_instance_initialization_group(&fourth, group::Limits::default())
        .is_ok()
        || local_world.snapshot() != current
    {
        return Err("Tight local capacity consumed group state".into());
    }
    if block_capacity != 0 {
        // Restore the reference boundary so source/owner contexts remain exact
        // while this independent witness has one fewer shared block slot.
        let block_world = World::restore(
            catalogue,
            before.clone(),
            Limits {
                max_event_blocks: block_capacity - 1,
                ..limits
            },
        )?;
        if block_world
            .stage_instance_initialization_group(&requests, group::Limits::default())
            .is_ok()
            || block_world.snapshot() != before
        {
            return Err("Combined block capacity consumed group state".into());
        }
    }
    Ok(Some(
        json!({"scope":"Three explicit isolated source attachments; two share an exact definition",
        "inputs":inputs,"receipt":receipt,"before_snapshot":before,"current_snapshot":current,"restored_snapshot":restored.snapshot(),
        "capacity":{"instances":3,"locals":local_capacity,"blocks":block_capacity,"next_instance":receipt.next_instance_after},
        "stage_preserved_state":true,"one_revision":true,"caller_order_preserved":true,"tight_instance_refusal_unchanged":true,
        "tight_local_refusal_unchanged":true,"combined_block_refusal_unchanged":block_capacity != 0,"old_handles_rejected":true,
        "canonical_round_trip_equal":true,"bytecode_executed":false,"retail_parity_accepted":false}),
    ))
}
fn probe(catalogue: &Catalogue) -> Result<Json> {
    let EngineeringWorld {
        world,
        handles,
        numbers,
        references,
        unknown,
        initializations,
    } = engineering_world(catalogue)?;
    let limits = Limits::default();
    let snapshot = world.snapshot();
    let bytes = snapshot.encode(limits.max_snapshot_bytes)?;
    let restored = World::restore(catalogue, Snapshot::decode(&bytes, limits)?, limits)?;
    if restored.snapshot() != snapshot
        || restored.snapshot().encode(limits.max_snapshot_bytes)? != bytes
    {
        return Err("Canonical state round trip differs".into());
    }
    for (id, old) in &handles {
        if restored.instance(*old).is_ok() || restored.instance(restored.handle(*id)?)?.id() != *id
        {
            return Err("Restored runtime handle identity checks failed".into());
        }
    }
    let mut report = json!({"scope":"Engineering inputs on original compiled declaration schemas; no original running event lists or initialization defaults",
        "instances":world.instance_count(),"numeric_values":numbers,"typed_reference_values":references,
        "unsupported_slots_retained_uninitialized":unknown,"pending_events":world.pending_events().len(),
        "initializations":initializations,"initialization_is_one_revision":true,
        "snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "catalogue_sha256":world.catalogue_fingerprint(),"canonical_bytes_equal":true,
        "old_handles_rejected":true,"persistent_ids_preserved":true,
        "original_state_captured":false,"bytecode_executed":false,"retail_parity_accepted":false});
    if let Some(observation) = selected_local_probe(catalogue)? {
        report["selected_local_probe"] = observation;
    }
    if let Some(group) = local_assignment_group_probe(catalogue)? {
        report["local_assignment_group"] = group;
    }
    if let Some(journal) = journal_page_probe(catalogue)? {
        report["journal_page_probe"] = journal;
    }
    if let Some(group) = enqueue_group_probe(catalogue)? {
        report["enqueue_group_probe"] = group;
    }
    if let Some(group) = instance_initialization_group_probe(catalogue)? {
        report["instance_initialization_group"] = group;
    }
    Ok(report)
}

pub(super) fn event_commit_probe(
    world: &mut World<'_>,
    request: EngineeringCommit,
) -> Result<Json> {
    let before = world.snapshot();
    let stage = request.stage(world)?;
    let assignments = stage.assignments().to_vec();
    let acknowledge = stage.acknowledges();
    if world.snapshot() != before {
        return Err("Staging changed canonical state".into());
    }
    let instance_id = stage.instance();
    let mut expected = before.clone();
    let instance = expected
        .instances
        .iter_mut()
        .find(|instance| instance.id == instance_id)
        .ok_or("Staged instance missing from snapshot")?;
    for (index, value) in &assignments {
        instance
            .locals
            .iter_mut()
            .find(|local| local.index == *index)
            .ok_or("Staged local missing from snapshot")?
            .value = value.clone();
    }
    expected.state_revision = expected
        .state_revision
        .checked_add(1)
        .ok_or("State revision exhausted")?;
    if acknowledge {
        expected.pending_events.remove(0);
    }
    let receipt = world.commit_event_changes(stage)?;
    let after = world.snapshot();
    if after != expected {
        return Err("Staged event commit differs from requested canonical changes".into());
    }
    let limits = Limits::default();
    for snapshot in [&before, &after] {
        let bytes = snapshot.encode(limits.max_snapshot_bytes)?;
        let restored =
            World::restore(world.catalogue(), Snapshot::decode(&bytes, limits)?, limits)?;
        if restored.snapshot() != *snapshot {
            return Err("Staged event boundary restoration differs".into());
        }
    }
    Ok(json!({
        "scope":"Explicit engineering typed-local/head transaction; no bytecode execution or original timing claim",
        "receipt":receipt,"requested_assignments":assignments,
        "before_snapshot_sha256":format!("{:x}",Sha256::digest(before.encode(limits.max_snapshot_bytes)?)),
        "after_snapshot_sha256":format!("{:x}",Sha256::digest(after.encode(limits.max_snapshot_bytes)?)),
        "pending_before":before.pending_events.len(),"pending_after":after.pending_events.len(),
        "staging_changed_state":false,"only_requested_changes":true,
        "before_after_restoration_equal":true,"bytecode_executed":false,"retail_parity_accepted":false
    }))
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    engineering_event_commit: Option<&Path>,
    host_inputs: Option<(&Path, &Path)>,
    source_group_inputs: Option<(&Path, &Path)>,
) -> Result<Json> {
    // Bound and validate the opt-in request before loading the content corpus.
    if engineering_event_commit.is_some() && host_inputs.is_some() {
        return Err("Host requirements and engineering event commit are mutually exclusive".into());
    }
    if source_group_inputs.is_some()
        && (engineering_event_commit.is_some() || host_inputs.is_some())
    {
        return Err(
            "Source reference group and other opt-in state modes are mutually exclusive".into(),
        );
    }
    let source_group = source_group_inputs
        .map(|(input, repository)| {
            Ok::<_, Box<dyn std::error::Error>>((SourceGroupInput::read(input)?, repository))
        })
        .transpose()?;
    let request = engineering_event_commit
        .map(EngineeringCommit::read)
        .transpose()?;
    let host = host_inputs
        .map(|(snapshot, requirements)| HostCheckInput::read(snapshot, requirements))
        .transpose()?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let mut payloads = BTreeMap::new();
    eprintln!(
        "Inspecting compiled local schemas and canonical state across {} plugins",
        order.names.len()
    );
    let catalogue = Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |source, record| {
            payloads.insert(
                (order.names[source].clone(), record.header.offset),
                (record.header.kind, record.payload.len()),
            );
            Ok(())
        },
    )?;
    let mut records = Vec::new();
    let mut kinds = BTreeMap::<String, u64>::new();
    let mut count = 0_u64;
    for (key, script) in catalogue.iter() {
        let locals = schema::locals(script).into_values().collect::<Vec<_>>();
        for local in &locals {
            let name = serde_json::to_value(local.kind)?["kind"]
                .as_str()
                .ok_or("Local kind name")?
                .to_string();
            *kinds.entry(name).or_default() += 1;
            count += 1;
        }
        let unit = json!({"header_decoded_offset":key.header_decoded_offset,"locals":locals});
        if records
            .last()
            .is_some_and(|record: &Json| record["key"] == json!(key.record))
        {
            records.last_mut().expect("matching record")["units"]
                .as_array_mut()
                .expect("unit rows")
                .push(unit);
        } else {
            let version = script.version();
            let &(kind, bytes) = payloads
                .get(&(version.source_plugin.clone(), version.record_file_offset))
                .ok_or("Retained script payload provenance")?;
            records.push(json!({"key":key.record,"source_name":version.source_plugin,
                "record_kind":std::str::from_utf8(&kind)?,"record_file_offset":version.record_file_offset,
                "record_flags":version.record_flags,"decoded_bytes":bytes,"decoded_sha256":version.decoded_record_sha256,"units":[unit]}));
        }
    }
    let mut report = json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"metadata":metadata,
        "counts":{"candidate_records":catalogue.counts.candidate_records_read+catalogue.counts.deleted_candidates_skipped,
            "deleted_candidate_records":catalogue.counts.deleted_candidates_skipped,"decoded_candidate_bytes":catalogue.counts.payload_bytes_scanned,
            "scripts":catalogue.counts.scripts,"unique_locals":count,"duplicate_declarations":catalogue.counts.duplicate_variable_indices,"local_kinds":kinds},
        "records":records,"explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "index_cache":store.index_cache_report(),"catalogue_source_findings":catalogue.counts.scripts_with_issues,
        "constructor_defaults_verified":false,"retail_parity_accepted":false});
    if let Some(host) = host {
        report["host_readiness"] = host.probe(&catalogue)?;
    } else {
        report["state_probe"] = probe(&catalogue)?;
    }
    if let Some(request) = request {
        let mut harness = engineering_world(&catalogue)?;
        report["engineering_event_commit"] = event_commit_probe(&mut harness.world, request)?;
    }
    if let Some((input, repository)) = source_group {
        let content = Content::load(&mut store, &catalogue, 1_000_000)?;
        report["source_reference_group"] =
            input.probe(&catalogue, &content, install, repository)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_INPUT: AtomicU64 = AtomicU64::new(1);
    struct InputFile(PathBuf);
    impl InputFile {
        fn new() -> Self {
            let root = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("target"));
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join(format!(
                "runtime-event-commit-input-{}-{}-{}.json",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_INPUT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            Self(path)
        }
    }
    impl Drop for InputFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn explicit_source_group_input_preserves_bits_and_refuses_ambiguous_fields() {
        let file = InputFile::new();
        let key = json!({"profile":"nv-original","origin_plugin":"falloutnv.esm","local_id":1280});
        let cell = json!({"profile":"nv-original","origin_plugin":"falloutnv.esm","local_id":1024});
        let pose = json!({"position_bits":[1065353216,2147483648_u32,1],"rotation_bits":[0,0,0],"scale_bits":null});
        let row = json!({"authored":key,"allowed_kinds":[[82,69,70,82]],"cell":cell,"pose":pose,"enabled":true});
        let valid = json!({"existing":row,"retained_state":{"schema_version":1,"cell":cell,"pose":pose,"enabled":false},"requests":[row]});
        std::fs::write(&file.0, serde_json::to_vec(&valid).unwrap()).unwrap();
        let input = SourceGroupInput::read(&file.0).unwrap();
        assert_eq!(
            input.requests[0].pose.source_transform().position[1].to_bits(),
            0x80000000
        );
        assert_eq!(
            input.requests[0].pose.source_transform().position[2].to_bits(),
            1
        );
        for bad in [
            {
                let mut v = valid.clone();
                v["unexpected"] = json!(true);
                v
            },
            {
                let mut v = valid.clone();
                v.as_object_mut().unwrap().remove("retained_state");
                v
            },
            {
                let mut v = valid.clone();
                v["requests"][0]["enabled"] = json!(null);
                v
            },
            {
                let mut v = valid.clone();
                v["existing"]["unknown"] = json!(1);
                v
            },
            {
                let mut v = valid.clone();
                v["requests"][0]["pose"]["position_bits"][0] = json!(4294967296_u64);
                v
            },
        ] {
            std::fs::write(&file.0, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(SourceGroupInput::read(&file.0).is_err());
        }
    }
    #[test]
    fn source_group_input_and_paired_mode_flags_are_bounded_and_exclusive() {
        use clap::Parser;
        let file = InputFile::new();
        std::fs::write(&file.0, vec![b' '; 65537]).unwrap();
        assert_eq!(
            SourceGroupInput::read(&file.0).err().unwrap().to_string(),
            "Source reference group input byte budget exceeded"
        );
        let base = [
            "fallout",
            "script-state",
            "--install",
            "install",
            "--load-order",
            "order.json",
        ];
        let pair = [
            "--source-reference-group",
            "group.json",
            "--source-reference-repository",
            "new-native",
        ];
        assert!(super::super::Args::try_parse_from(base.into_iter().chain(pair)).is_ok());
        for extra in [
            vec!["--source-reference-group", "group.json"],
            vec!["--source-reference-repository", "new-native"],
            pair.into_iter()
                .chain(["--engineering-event-commit", "commit.json"])
                .collect::<Vec<_>>(),
            pair.into_iter()
                .chain([
                    "--host-snapshot",
                    "snapshot.json",
                    "--host-requirements",
                    "requirements.json",
                ])
                .collect::<Vec<_>>(),
        ] {
            assert!(super::super::Args::try_parse_from(base.into_iter().chain(extra)).is_err());
        }
    }

    #[test]
    fn explicit_event_commit_input_preserves_bits_and_requires_every_field() {
        let file = InputFile::new();
        let path = &file.0;
        std::fs::write(path, br#"{"sequence":1,"assignments":[{"index":42,"value":{"kind":"number","bits":18446744073709551615}}],"acknowledge":true}"#).unwrap();
        let input = EngineeringCommit::read(path).unwrap();
        assert_eq!(input.sequence, 1);
        assert_eq!(input.assignments[0].value, Value::Number { bits: u64::MAX });
        assert!(input.acknowledge);
        for bytes in [
            br#"{"sequence":1,"assignments":[],"acknowledge":false,"unknown":true}"#.as_slice(),
            br#"{"sequence":1,"sequence":2,"assignments":[],"acknowledge":false}"#,
            br#"{"sequence":1,"assignments":[]}"#,
            br#"{"sequence":1,"assignments":[],"acknowledge":null}"#,
            br#"{"sequence":1,"assignments":[{"index":42,"value":{"kind":"number","bits":18446744073709551616}}],"acknowledge":true}"#,
        ] {
            std::fs::write(path, bytes).unwrap();
            assert!(EngineeringCommit::read(path).is_err());
        }
    }

    #[test]
    fn explicit_event_commit_input_byte_budget_fails_for_the_intended_reason() {
        let file = InputFile::new();
        let path = &file.0;
        std::fs::write(path, vec![b' '; 65_537]).unwrap();
        assert_eq!(
            EngineeringCommit::read(path).err().unwrap().to_string(),
            "Engineering event-commit input byte budget exceeded"
        );
    }

    #[test]
    fn host_requirements_inputs_are_strict_typed_and_bounded_before_snapshot_loading() {
        let request = json!({
            "campaign":vec![25_u8;16],"catalogue_sha256":"a".repeat(64),
            "reference_states":[1],"inventory_owners":[2],
            "instances":[{"owner":{"kind":"placed","reference":1},"instance":1,
                "definition":{"key":{"record":{"profile":fallout_data::identity::ProfileId::NvOriginal,"origin_plugin":"falloutnv.esm","local_id":768},
                    "header_decoded_offset":0},"version_sha256":"b".repeat(64)}}],
            "expected_journal_head":{"kind":"event","sequence":7,"instance":1}
        });
        let mut request = request;
        let parsed: HostRequirements = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(parsed.reference_states[0].0.get(), 1);
        assert_eq!(parsed.instances[0].instance.0.get(), 1);
        assert_eq!(parsed.instances[0].definition.key.record.local_id, 0x300);
        for field in ["unexpected", "reference_states"] {
            let bytes = serde_json::to_string(&request).unwrap();
            let duplicate_or_unknown = format!("{{\"{field}\":[],{}", &bytes[1..]);
            assert!(serde_json::from_str::<HostRequirements>(&duplicate_or_unknown).is_err());
        }
        request["instances"][0]["instance"] = json!(0);
        assert!(serde_json::from_value::<HostRequirements>(request).is_err());
        let file = InputFile::new();
        std::fs::write(&file.0, vec![b' '; 1024 * 1024 + 1]).unwrap();
        assert_eq!(
            HostCheckInput::read(Path::new("nonexistent-host-snapshot.json"), &file.0)
                .err()
                .unwrap()
                .to_string(),
            "Host requirements input byte budget exceeded"
        );
    }

    #[test]
    fn host_check_flags_require_both_inputs_and_exclude_the_mutating_commit_mode() {
        use clap::Parser;
        let base = [
            "fallout",
            "script-state",
            "--install",
            "private-install",
            "--load-order",
            "private-order.json",
        ];
        for extra in [
            vec!["--host-snapshot", "snapshot.json"],
            vec!["--host-requirements", "requirements.json"],
            vec![
                "--host-snapshot",
                "snapshot.json",
                "--host-requirements",
                "requirements.json",
                "--engineering-event-commit",
                "commit.json",
            ],
        ] {
            assert!(super::super::Args::try_parse_from(base.into_iter().chain(extra)).is_err());
        }
        assert!(
            super::super::Args::try_parse_from(base.into_iter().chain([
                "--host-snapshot",
                "snapshot.json",
                "--host-requirements",
                "requirements.json"
            ]))
            .is_ok()
        );
        assert!(super::super::Args::try_parse_from(base).is_ok());
    }
}
