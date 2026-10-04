//! Compose explicit engineering boot requests using the existing state owners.
//! No live world or save is published until every requested stage is admitted.
use super::{Result, command_catalogue, inspection_input::Order, script_profile};
use fallout_data::{
    actors,
    identity::FormKey,
    inventory, loaded_scripts, obscript, plugin, quest_scripts,
    vfs::MountIndex,
    world::{dependencies::Decoded, preparation::CellModelPlan},
};
use fallout_runtime::{
    Limits, World,
    actor_rules::inventory_boot,
    execution::attachment_boot,
    foreign::Content,
    identity::{CampaignId, InstanceId, ReferenceId},
    programs,
    reference_state::View,
    save::{
        Captured, LoadReceipt, Recovery, Repository, RequestIdentity, RestorePoll, RestoreTask,
        WriteReceipt, format,
    },
    snapshot::Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    fs::File,
    io::{self, BufReader, Seek, SeekFrom, Write},
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

mod input;
use input::{Destination, REPORT_BYTES, RepositoryInputs, RequestFile, SNAPSHOT_BYTES};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Intent {
    Engineering,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    intent: Intent,
    request_id: NonZeroU64,
    restore_timeout_ms: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    input_snapshot_sha256: String,
    input_revision: u64,
    cell: FormKey,
    required_references: Vec<ReferenceRequirement>,
    actor_inventory: Option<ActorInventory>,
    quest_owner: Option<QuestOwner>,
    require_faithful_simulation: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceRequirement {
    reference: ReferenceId,
    authored: FormKey,
    enabled: bool,
    require_scale: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorInventory {
    actor: FormKey,
    owner: ReferenceId,
    choices: Vec<inventory_boot::Choice>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestOwner {
    quest: FormKey,
    initialization: attachment_boot::Request,
}

impl Request {
    fn validate(&self) -> Result<()> {
        let Intent::Engineering = self.intent;
        if self.schema_version != 1 || self.require_faithful_simulation {
            return Err("route boot supports only schema1 explicit engineering admission".into());
        }
        CampaignId::from_bytes(self.campaign.bytes())?;
        for hash in [&self.catalogue_sha256, &self.input_snapshot_sha256] {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("route boot requires canonical lower-hex SHA256 identities".into());
            }
        }
        if !(1..=30_000).contains(&self.restore_timeout_ms) {
            return Err("route boot restore polling deadline must be 1..=30000 ms".into());
        }
        if self.required_references.is_empty() || self.required_references.len() > 64 {
            return Err("route boot requires 1..=64 explicit references".into());
        }
        let mut ids = BTreeSet::new();
        let mut authored = BTreeSet::new();
        for requirement in &self.required_references {
            if !ids.insert(requirement.reference) || !authored.insert(&requirement.authored) {
                return Err("route boot reference requirements must be unique".into());
            }
        }
        if self.actor_inventory.is_none() && self.quest_owner.is_none() {
            return Err("route boot requires an explicit inventory or quest boot operation".into());
        }
        if self
            .actor_inventory
            .as_ref()
            .is_some_and(|actor| actor.choices.len() > 64)
        {
            return Err("route boot selected inventory lot budget exceeds64".into());
        }
        if self
            .quest_owner
            .as_ref()
            .is_some_and(|quest| quest.initialization.initializers.len() > 128)
        {
            return Err("route boot initializer budget exceeds128".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ActorReport {
    actor: FormKey,
    owner: ReferenceId,
    source_definition_sha256: String,
    input_snapshot_sha256: String,
    mappings: Vec<inventory_boot::Mapping>,
}
#[derive(Serialize)]
struct QuestReport {
    quest: FormKey,
    instance: InstanceId,
    definition: loaded_scripts::Handle,
    executable_sha256: String,
    prepared_decoder_sha256: String,
}
#[derive(Serialize)]
struct PreparedReport {
    schema_version: u32,
    scope: &'static str,
    request_sha256: String,
    load_order_sha256: String,
    input_restore: LoadReceipt,
    input_snapshot_sha256: String,
    input_canonical_snapshot_sha256: String,
    final_snapshot_sha256: String,
    final_snapshot_bytes: usize,
    final_revision: u64,
    cell_plan_identity: String,
    model_assets_prepared: bool,
    references: Vec<View>,
    actor_inventory: Option<ActorReport>,
    quest_owner: Option<QuestReport>,
    canonical_cold_restore_verified: bool,
    faithful_simulation_admitted: bool,
    retail_parity_accepted: bool,
}
#[derive(Serialize)]
struct PublishedReport<'a> {
    prepared: &'a PreparedReport,
    destination: &'a Path,
    publication: &'a WriteReceipt,
    published_cold_restore_verified: bool,
}

#[derive(Debug)]
struct PublishedFailure {
    destination: PathBuf,
    receipt: WriteReceipt,
    cause: String,
}
impl fmt::Display for PublishedFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "route boot already published to {}; subsequent verification/report failed: {}; actual write receipt: {}",
            self.destination.display(),
            self.cause,
            serde_json::to_string(&self.receipt).map_err(|_| fmt::Error)?
        )
    }
}
impl Error for PublishedFailure {}

fn world_limits() -> Limits {
    Limits {
        max_snapshot_bytes: SNAPSHOT_BYTES,
        ..Limits::default()
    }
}

fn restore(
    repository: &Repository,
    catalogue: Arc<loaded_scripts::Catalogue>,
    request: &Request,
) -> Result<(World<'static>, LoadReceipt)> {
    let identity = RequestIdentity::new(
        request.request_id,
        request.campaign,
        &request.catalogue_sha256,
    )?;
    let mut task = RestoreTask::start(
        repository.clone(),
        catalogue,
        world_limits(),
        Recovery::Strict,
        identity.clone(),
    )?;
    let deadline = Instant::now() + Duration::from_millis(request.restore_timeout_ms);
    loop {
        if Instant::now() >= deadline {
            // Revocation is cooperative, so drop without joining opaque IO.
            task.cancel();
            return Err("route boot strict restore polling deadline expired".into());
        }
        match task.try_poll() {
            RestorePoll::Ready(candidate) => {
                if Instant::now() >= deadline {
                    task.cancel();
                    return Err("route boot strict restore polling deadline expired".into());
                }
                return Ok(candidate.take_for(&identity)?);
            }
            RestorePoll::Pending => thread::sleep(Duration::from_millis(2)),
            RestorePoll::Failed(error) => return Err(error.into()),
            state => return Err(format!("route boot restore unavailable: {state:?}").into()),
        }
    }
}

fn source_bounds(install: &Path, order: &Order) -> Result<Vec<File>> {
    let mut bytes = 0u64;
    let mut files = Vec::with_capacity(order.names.len());
    for name in &order.names {
        fallout_data::identity::plugin_name(name)?;
        let file = fallout_data::baseline::open_source(&install.join("Data").join(name))?;
        bytes = bytes
            .checked_add(file.metadata()?.len())
            .ok_or("route boot source size overflow")?;
        if bytes > 4 * 1024 * 1024 * 1024 {
            return Err("route boot plugin cohort exceeds4 GiB".into());
        }
        files.push(file);
    }
    // Charge the complete framing walk before RecordStore allocates its index.
    // The existing strict visitor retains only its bounded group stack and TES4.
    let mut headers = 0usize;
    for (name, file) in order.names.iter().zip(&mut files) {
        file.seek(SeekFrom::Start(0))?;
        let length = file.metadata()?.len();
        plugin::visit_selected(
            &mut BufReader::new(file),
            length,
            name,
            plugin::Limits {
                max_records: 1_000_000,
                ..Default::default()
            },
            |_| false,
            |_| {
                headers = headers
                    .checked_add(1)
                    .filter(|count| *count <= 1_000_000)
                    .ok_or_else(|| {
                        fallout_data::Error::Resolution(
                            "route boot aggregate structural header budget exceeded".into(),
                        )
                    })?;
                Ok(())
            },
        )?;
    }
    Ok(files)
}

fn require_references(
    world: &World<'_>,
    request: &Request,
    plan: &CellModelPlan,
) -> Result<Vec<View>> {
    let graph = plan.graph();
    if graph.integrity_failures != 0 || graph.root != request.cell {
        return Err("route boot selected CELL source is unavailable".into());
    }
    let members: BTreeSet<_> = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.owner == request.cell && edge.role == "member" && edge.target.status == "resolved"
        })
        .filter_map(|edge| edge.target.key.as_ref())
        .collect();
    let mut observations = Vec::with_capacity(request.required_references.len());
    for requirement in &request.required_references {
        if !members.contains(&requirement.authored) {
            return Err(
                "route boot required authored reference is not a resolved selected CELL member"
                    .into(),
            );
        }
        if !graph.nodes.iter().any(|node| {
            node.key == requirement.authored
                && matches!(&node.header.kind, b"REFR" | b"ACHR" | b"ACRE")
                && matches!(&node.fields, Some(Decoded::Placement(_)))
        }) {
            return Err(
                "route boot required source member is not a decoded placed reference".into(),
            );
        }
        let view = world.reference_view(requirement.reference)?;
        if view.authored() != Some(&requirement.authored) {
            return Err("route boot canonical reference has a different authored identity".into());
        }
        let state = view
            .state()
            .ok_or("route boot required reference component is unavailable")?;
        if state.cell() != &request.cell || state.enabled() != requirement.enabled {
            return Err("route boot required canonical CELL or enable state differs".into());
        }
        if requirement.require_scale && state.pose().source_scale().is_none() {
            return Err("route boot required canonical scale is unavailable".into());
        }
        observations.push(view);
    }
    Ok(observations)
}

fn require_actor_binding(
    world: &World<'_>,
    request: &Request,
    actor: &ActorInventory,
    plan: &CellModelPlan,
) -> Result<()> {
    let requirement = request
        .required_references
        .iter()
        .find(|r| r.reference == actor.owner)
        .ok_or("route boot actor inventory owner must be an explicit required reference")?;
    if world.reference_origin(actor.owner)? != Some(&requirement.authored) {
        return Err("route boot actor inventory owner authored identity differs".into());
    }
    let names: Vec<_> = plan
        .graph()
        .edges
        .iter()
        .filter(|edge| edge.owner == requirement.authored && edge.role == "NAME")
        .collect();
    if !matches!(names.as_slice(), [edge] if edge.target.status == "resolved" && edge.target.key.as_ref() == Some(&actor.actor))
    {
        return Err(
            "route boot actor inventory source is not the owner's unique resolved NAME base".into(),
        );
    }
    Ok(())
}

fn unchanged_boundary(
    before: &Snapshot,
    after: &Snapshot,
    actor: Option<ReferenceId>,
    quest: Option<InstanceId>,
) -> Result<()> {
    let existing_banks = after
        .inventory_banks
        .iter()
        .filter(|bank| Some(bank.owner) != actor);
    let existing_instances = after
        .instances
        .iter()
        .filter(|instance| Some(instance.id) != quest);
    if before.references != after.references
        || before.reference_states != after.reference_states
        || before.clocks != after.clocks
        || before.pending_events != after.pending_events
        || before.next_reference != after.next_reference
        || before.next_event_sequence != after.next_event_sequence
        || !existing_banks.eq(before.inventory_banks.iter())
        || !existing_instances.eq(before.instances.iter())
        || (actor.is_none() && before.next_item != after.next_item)
        || (quest.is_none() && before.next_instance != after.next_instance)
    {
        return Err("route boot producer changed unrelated canonical boundary state".into());
    }
    Ok(())
}

/// This command writes its publication report itself, so stdout failure retains
/// the actual native receipt instead of being mistaken for a refused mutation.
pub(super) fn run(
    install: &Path,
    order_path: &Path,
    source_root: &Path,
    request_path: &Path,
    destination: &Path,
) -> Result<()> {
    let input = RequestFile::read(request_path)?;
    let request: Request = serde_json::from_slice(&input.bytes)?;
    request.validate()?;
    let destination = Destination::inspect(
        destination,
        &[
            install.into(),
            source_root.into(),
            order_path.into(),
            request_path.into(),
        ],
    )?;
    let repository = Repository::open(source_root, &[install.into()])?;
    if repository.campaign() != request.campaign {
        return Err("route boot input campaign differs".into());
    }
    let mut input_guard = RepositoryInputs::inspect(repository.path())?;
    let order = Order::read(order_path)?;
    let _source_guards = source_bounds(install, &order)?;
    let mut store = order.store(install, None)?;
    let headers = store
        .indices()
        .iter()
        .try_fold(0usize, |sum, index| sum.checked_add(index.records.len()))
        .ok_or("route boot header count overflow")?;
    if headers > 1_000_000 {
        return Err("route boot aggregate source header budget exceeded".into());
    }
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        Default::default(),
        |_, _| Ok(()),
    )?);
    if fallout_runtime::snapshot::cohort(&catalogue)? != request.catalogue_sha256 {
        return Err("route boot input source catalogue differs".into());
    }
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    // Membership admission uses the sealed source graph. The preview remains
    // responsible for archive/model admission when it consumes this native save.
    let plan = CellModelPlan::load(
        &mut store,
        &request.cell,
        &MountIndex::default(),
        Default::default(),
    )?;
    let sources = store.source_receipts()?;
    if sources.len() != plan.graph().sources.len()
        || !sources.iter().zip(&plan.graph().sources).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
    {
        return Err("route boot CELL and script source cohorts differ".into());
    }
    let (mut world, load_receipt) = restore(&repository, Arc::clone(&catalogue), &request)?;
    let before = world.snapshot();
    let input_bytes = before.encode(SNAPSHOT_BYTES)?;
    if load_receipt.metadata.snapshot_sha256 != request.input_snapshot_sha256
        || before.state_revision != request.input_revision
    {
        return Err("route boot exact input snapshot or revision differs".into());
    }
    let input_canonical_snapshot_sha256 = input::digest(&input_bytes);
    drop(input_bytes);
    require_references(&world, &request, &plan)?;
    let mut actor_report = None;
    if let Some(actor) = &request.actor_inventory {
        require_actor_binding(&world, &request, actor, &plan)?;
        let inventory = inventory::Catalogue::load(&mut store, Default::default())?;
        let actors = actors::Catalogue::load(&inventory, Default::default())?;
        let boot_plan = inventory_boot::prepare(
            &world,
            &content,
            &actors,
            &actor.actor,
            actor.owner,
            &actor.choices,
            inventory_boot::Limits {
                max_lots: 64,
                max_snapshot_bytes: SNAPSHOT_BYTES,
                max_projection_bytes: REPORT_BYTES,
                ..Default::default()
            },
        )?;
        let boot =
            boot_plan.apply_private(&catalogue, &content, &world.snapshot(), world_limits())?;
        actor_report = Some(ActorReport {
            actor: actor.actor.clone(),
            owner: boot.owner,
            source_definition_sha256: input::admit(boot.source_definition, REPORT_BYTES)?.1,
            input_snapshot_sha256: boot.input_snapshot_sha256,
            mappings: boot.mappings,
        });
        world = World::restore(
            Arc::clone(&catalogue),
            boot.candidate_snapshot,
            world_limits(),
        )?;
    }
    let mut quest_report = None;
    if let Some(quest) = &request.quest_owner {
        let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
        let operators = script_profile::operators(&descriptors)?;
        let model = obscript::expression_plan::Model::vanilla(&operators)?;
        let signatures = script_profile::signatures(&descriptors);
        let attachments =
            quest_scripts::Attachments::load(&mut store, &catalogue, 65_536, |_, _| Ok(()))?;
        let handles: Vec<_> = attachments
            .get(&quest.quest)
            .and_then(|a| a.script.as_ref())
            .cloned()
            .into_iter()
            .collect();
        let prepared = programs::PreparedSources::load_selected(
            &catalogue,
            &model,
            &signatures,
            &handles,
            Default::default(),
        )?;
        let boot_plan = attachment_boot::prepare(
            &prepared,
            &attachments,
            &content,
            &quest.quest,
            &quest.initialization,
            Default::default(),
        )?;
        let boot = boot_plan.apply(world.snapshot(), world_limits())?;
        quest_report = Some(QuestReport {
            quest: quest.quest.clone(),
            instance: boot.instance,
            definition: boot_plan.definition().clone(),
            executable_sha256: descriptors.source_sha256,
            prepared_decoder_sha256: prepared.decoder_sha256().into(),
        });
        world = World::restore(Arc::clone(&catalogue), boot.snapshot, world_limits())?;
    }
    let final_snapshot = world.snapshot();
    unchanged_boundary(
        &before,
        &final_snapshot,
        actor_report.as_ref().map(|a| a.owner),
        quest_report.as_ref().map(|q| q.instance),
    )?;
    let final_bytes = final_snapshot.encode(SNAPSHOT_BYTES)?;
    let cold = World::restore(
        Arc::clone(&catalogue),
        Snapshot::decode(&final_bytes, world_limits())?,
        world_limits(),
    )?;
    if cold.snapshot() != final_snapshot {
        return Err("route boot complete cold snapshot differs from private candidate".into());
    }
    let references = require_references(&cold, &request, &plan)?;
    content.validate_world(&cold)?;
    let capture = Captured::at_boundary(&cold);
    let native_bytes = format::encode(&capture, 1)?;
    let native = format::decode(&native_bytes, world_limits())?;
    if native.snapshot != final_snapshot
        || World::restore(Arc::clone(&catalogue), native.snapshot, world_limits())?.snapshot()
            != final_snapshot
    {
        return Err("route boot prepublication native round trip differs".into());
    }
    drop(native_bytes);
    let report = PreparedReport {
        schema_version: 1,
        scope: "Explicit engineering inventory/quest initialization composed into a new project-native repository; no retail activation or playable route acceptance",
        request_sha256: input.sha256.clone(),
        load_order_sha256: order.sha256.clone(),
        input_restore: load_receipt,
        input_snapshot_sha256: request.input_snapshot_sha256.clone(),
        input_canonical_snapshot_sha256,
        final_snapshot_sha256: input::digest(&final_bytes),
        final_snapshot_bytes: final_bytes.len(),
        final_revision: final_snapshot.state_revision,
        cell_plan_identity: plan.identity().into(),
        model_assets_prepared: false,
        references,
        actor_inventory: actor_report,
        quest_owner: quest_report,
        canonical_cold_restore_verified: true,
        faithful_simulation_admitted: false,
        retail_parity_accepted: false,
    };
    // Reserve room for the fixed native receipt and wrapper; no snapshot body
    // or unbounded producer/source projection can first expand after creation.
    input::admit(
        &(&report, &destination.path, &native.metadata),
        REPORT_BYTES - 8192,
    )?;
    input_guard.recheck()?;
    destination.recheck()?;
    let output = Repository::create(&destination.path, destination.protected(), request.campaign)
        .map_err(|error| format!("route boot publication setup failed; preserve any incomplete fresh destination: {error}"))?;
    let receipt = output.commit(&capture)
        .map_err(|error| format!("route boot publication stage failed; current may already exist, preserve fresh destination {}: {error}", destination.path.display()))?;
    let verification = (|| -> Result<()> {
        if receipt.metadata != native.metadata || receipt.previous_generation.is_some() {
            return Err(
                "new route boot publication metadata differs from admitted generation1".into(),
            );
        }
        let (published, loaded) =
            output.load(Arc::clone(&catalogue), world_limits(), Recovery::Strict)?;
        if published.snapshot() != final_snapshot
            || loaded.metadata != receipt.metadata
            || loaded.current_repaired
        {
            return Err("published route boot cold restore differs from complete candidate".into());
        }
        input_guard.recheck()?;
        let report = PublishedReport {
            prepared: &report,
            destination: &destination.path,
            publication: &receipt,
            published_cold_restore_verified: true,
        };
        input::admit(&receipt, 4096)?;
        input::admit(&report, REPORT_BYTES)?;
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        serde_json::to_writer(&mut stdout, &report)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        Ok(())
    })();
    if let Err(error) = verification {
        return Err(Box::new(PublishedFailure {
            destination: destination.path,
            receipt,
            cause: error.to_string(),
        }));
    }
    Ok(())
}
