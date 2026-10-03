//! Exercise every compiled foreign-local request with explicit host event lists.
//! These lists and values are engineering inputs, never original live captures.
use super::{Result, command_catalogue, inspection_input::Order, script_profile};
use fallout_data::{
    loaded_scripts::{self, Catalogue, Handle, OwnerKind, ScriptKey},
    quest_scripts, record_metadata,
};
use fallout_runtime::{
    Limits, World,
    events::Context,
    foreign::{Content, Request},
    identity::{CampaignId, InstanceId, Owner, ReferenceValue, Value},
    schema::{self, Kind},
    snapshot::Snapshot,
    state::InstanceHandle,
};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

struct Use {
    definition: Handle,
    scda_offset: usize,
    role: u8,
    context: u16,
    index: u16,
}

pub(super) fn cold(
    install: &Path,
    order_path: &Path,
    repository_path: &Path,
    player_id: u64,
) -> Result<Json> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue = Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| Ok(()))?;
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let Compiled { uses, .. } = compiled(&catalogue, install)?;
    let repository = fallout_runtime::save::Repository::open(repository_path, &[install.into()])?;
    let (world, receipt) = repository.load(
        &catalogue,
        Limits::default(),
        fallout_runtime::save::Recovery::Strict,
    )?;
    let player = fallout_runtime::identity::ReferenceId(player_id.try_into()?);
    world.reference_origin(player)?;
    let snapshot = world.snapshot();
    let mut sources = BTreeMap::new();
    for instance in &snapshot.instances {
        if matches!(instance.owner, Owner::Fragment { .. })
            && sources
                .insert(instance.definition.key.clone(), instance.id)
                .is_some()
        {
            return Err("Cold engineering probe has duplicate source activations".into());
        }
    }
    let mut statuses = BTreeMap::new();
    let mut hash = Sha256::new();
    for use_ in &uses {
        let id = sources
            .get(&use_.definition.key)
            .ok_or("Cold probe has no saved source instance")?;
        let row = outcome(
            &world,
            &content,
            Request {
                source: world.handle(*id)?,
                context_reference: use_.context,
                local_index: use_.index,
                player: Some(player),
            },
        )?;
        count(&mut statuses, &row)?;
        lookup_digest(&mut hash, use_, &row)?;
    }
    let bytes = snapshot.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Cold native restore of explicit engineering lists; player identity supplied as harness input",
        "receipt":receipt,"foreign_requests":uses.len(),"bound_statuses":statuses,"context_content":content.report(),
        "instances":world.instance_count(),"lookup_results_sha256":format!("{:x}",hash.finalize()),
        "snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"source_bound_restore":true,
        "original_live_values_captured":false,"retail_parity_accepted":false}),
    )
}
fn seed(
    world: &mut World<'_>,
    catalogue: &Catalogue,
    definition: &Handle,
    owner: Owner,
    bits: u64,
) -> Result<InstanceHandle> {
    let script = catalogue
        .get_handle(definition)
        .ok_or("Stale engineering definition")?;
    let instance = world.create_instance(definition, owner, Context::default())?;
    let values = schema::locals(script)
        .values()
        .filter_map(|local| {
            let value = match local.kind {
                Kind::Float | Kind::Integer => Value::Number { bits },
                Kind::Reference => Value::Reference {
                    value: ReferenceValue::Null,
                },
                _ => return None,
            };
            Some((local.index, value))
        })
        .collect::<Vec<_>>();
    world.assign(instance, &values)?;
    Ok(instance)
}
fn outcome(world: &World<'_>, content: &Content, request: Request) -> Result<Json> {
    Ok(match world.read_foreign(content, request) {
        Ok(read) => json!({"status":"resolved","read":read}),
        Err(error) => json!({"status":error.code(),"reason":error.to_string()}),
    })
}
fn count(counts: &mut BTreeMap<String, usize>, row: &Json) -> Result<()> {
    let status = row["status"].as_str().ok_or("Foreign lookup lost status")?;
    *counts.entry(status.into()).or_default() += 1;
    Ok(())
}

struct Compiled {
    units: Vec<Json>,
    uses: Vec<Use>,
    operand_uses: u64,
}
fn compiled(catalogue: &Catalogue, install: &Path) -> Result<Compiled> {
    let executable = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&executable)?;
    let signatures = script_profile::signatures(&executable);
    let mut units = Vec::new();
    let mut uses = Vec::new();
    let mut operand_uses = 0_u64;
    for (_, script) in catalogue.iter() {
        let Some(binding) = script.bind_operands(&operators, &signatures, 262_144)? else {
            continue;
        };
        operand_uses += binding.counts.uses;
        if operand_uses > 2_000_000 {
            return Err("Foreign probe operand budget exceeded".into());
        }
        let first = uses.len();
        for operand in &binding.uses {
            if operand.status != 4 {
                continue;
            }
            let context = operand
                .context_reference
                .ok_or("Foreign binding lost context")?;
            if uses.len() >= 1_000_000 {
                return Err("Foreign probe request budget exceeded".into());
            }
            uses.push(Use {
                definition: script.handle().clone(),
                scda_offset: operand.scda_offset,
                role: operand.role,
                context,
                index: operand.index,
            });
        }
        units.push(json!({"handle":script.handle(),"binding_sha256":fallout_data::obscript::operand_binding::digest(&binding.uses),"counts":binding.counts,"foreign_uses":uses.len()-first}));
    }
    Ok(Compiled {
        units,
        uses,
        operand_uses,
    })
}

fn lookup_digest(hash: &mut Sha256, use_: &Use, outcome: &Json) -> Result<()> {
    let bytes = serde_json::to_vec(&json!({"source_definition":use_.definition,
        "scda_offset":use_.scda_offset,"role":use_.role,
        "context_reference":use_.context,"local_index":use_.index,"bound":outcome}))?;
    hash.update(u32::try_from(bytes.len())?.to_le_bytes());
    hash.update(bytes);
    Ok(())
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    new_repository: Option<&Path>,
) -> Result<Json> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let metadata = record_metadata::inspect(&store)?;
    let catalogue = Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| Ok(()))?;
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let attachments =
        quest_scripts::Attachments::load(&mut store, &catalogue, 65_536, |_, _| Ok(()))?;
    let Compiled {
        units,
        uses,
        operand_uses,
    } = compiled(&catalogue, install)?;
    let mut world = World::with_campaign(
        &catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x30; 16])?,
    )?;
    let mut sources = BTreeMap::<ScriptKey, (InstanceId, InstanceHandle)>::new();
    for use_ in &uses {
        if sources.contains_key(&use_.definition.key) {
            continue;
        }
        let activation = (sources.len() as u64 + 1).try_into()?;
        let handle = seed(
            &mut world,
            &catalogue,
            &use_.definition,
            Owner::Fragment { activation },
            1_f64.to_bits(),
        )?;
        sources.insert(
            use_.definition.key.clone(),
            (world.instance(handle)?.id(), handle),
        );
    }
    let mut unbound_counts = BTreeMap::new();
    let mut unbound = Vec::with_capacity(uses.len());
    for use_ in &uses {
        let request = Request {
            source: sources[&use_.definition.key].1,
            context_reference: use_.context,
            local_index: use_.index,
            player: None,
        };
        let row = outcome(&world, &content, request)?;
        count(&mut unbound_counts, &row)?;
        unbound.push(row);
    }

    // Static quest attachments are explicit harness inputs here. The runtime
    // resolver itself always reads the owner's current live definition.
    let mut quest_instances = 0;
    for (key, attachment) in attachments.iter() {
        let Some(definition) = &attachment.script else {
            continue;
        };
        seed(
            &mut world,
            &catalogue,
            definition,
            Owner::Quest { key: key.clone() },
            10_f64.to_bits(),
        )?;
        quest_instances += 1;
    }
    let placed_definition = catalogue
        .iter()
        .map(|(_, script)| script)
        .find(|script| {
            script.owner().kind == OwnerKind::Standalone
                && schema::locals(script)
                    .values()
                    .any(|local| matches!(local.kind, Kind::Float | Kind::Integer))
        })
        .ok_or("No standalone engineering template with numeric locals")?
        .handle()
        .clone();
    let mut placed = BTreeSet::new();
    for use_ in &uses {
        let script = catalogue
            .get_handle(&use_.definition)
            .ok_or("Stale foreign source")?;
        let reference = script
            .reference(u32::from(use_.context))
            .ok_or("Foreign context entry disappeared")?;
        if reference.target.as_ref().is_some_and(|target| {
            matches!(
                target.record_kind.as_str(),
                "REFR" | "ACHR" | "ACRE" | "PGRE" | "PMIS" | "PBEA"
            )
        }) {
            placed.insert(
                reference
                    .form_key
                    .clone()
                    .ok_or("Placed context lost form key")?,
            );
        }
    }
    for key in &placed {
        let reference = world.register_reference(Some(key.clone()))?;
        seed(
            &mut world,
            &catalogue,
            &placed_definition,
            Owner::Placed { reference },
            10_f64.to_bits(),
        )?;
    }
    let player = world.register_reference(None)?;
    seed(
        &mut world,
        &catalogue,
        &placed_definition,
        Owner::Placed { reference: player },
        10_f64.to_bits(),
    )?;
    let snapshot = world.snapshot();
    let bytes = snapshot.encode(Limits::default().max_snapshot_bytes)?;
    let restored = World::restore(
        &catalogue,
        Snapshot::decode(&bytes, Limits::default())?,
        Limits::default(),
    )?;
    if restored
        .snapshot()
        .encode(Limits::default().max_snapshot_bytes)?
        != bytes
    {
        return Err("Foreign probe state round trip differs".into());
    }
    let mut lookup_hash = Sha256::new();
    let mut bound_counts = BTreeMap::new();
    let mut rows = Vec::with_capacity(uses.len());
    for (use_, unbound) in uses.iter().zip(unbound) {
        let (source_id, source_handle) = sources[&use_.definition.key];
        let request = Request {
            source: source_handle,
            context_reference: use_.context,
            local_index: use_.index,
            player: Some(player),
        };
        let bound = outcome(&world, &content, request)?;
        let restored_request = Request {
            source: restored.handle(source_id)?,
            ..request
        };
        if outcome(&restored, &content, restored_request)? != bound {
            return Err("Foreign lookup changed after canonical restoration".into());
        }
        if restored.foreign_target(&content, request).is_ok() {
            return Err("Foreign probe accepted a pre-restore source handle".into());
        }
        count(&mut bound_counts, &bound)?;
        lookup_digest(&mut lookup_hash, use_, &bound)?;
        rows.push(json!({"source_definition":use_.definition,"scda_offset":use_.scda_offset,"role":use_.role,"context_reference":use_.context,"local_index":use_.index,"unbound":unbound,"bound":bound}));
    }
    let native_write = new_repository
        .map(|path| {
            let repository = fallout_runtime::save::Repository::create(
                path,
                &[install.into()],
                world.campaign(),
            )?;
            repository.commit(&fallout_runtime::save::Captured::at_boundary(&world))
        })
        .transpose()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"metadata":metadata,
        "context_content":content.report(),"compiled_units":units,"operand_uses":operand_uses,"foreign_uses":uses.len(),"foreign_requests":rows,
        "engineering_inputs":{"source_instances":sources.len(),"quest_instances":quest_instances,"placed_instances":placed.len(),"player_instances":1,"player_reference":player,
            "source_numeric_bits":1_f64.to_bits(),"target_numeric_bits":10_f64.to_bits(),"reference_values":"Explicit null; no original SCRV values captured",
            "placed_definition":placed_definition,"placed_definition_scope":"Explicit host template; does not claim original placed-reference script attachment",
            "quest_definition_scope":"Authored SCRI selected as explicit harness input; retail live list not captured"},
        "unbound_statuses":unbound_counts,"bound_statuses":bound_counts,"instances":world.instance_count(),
        "native_write":native_write,"lookup_results_sha256":format!("{:x}",lookup_hash.finalize()),"snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "canonical_state_round_trip_equal":true,"all_lookup_results_equal_after_restore":true,"old_handles_rejected":true,
        "original_live_values_captured":false,"bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
