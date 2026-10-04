//! Read-only requests over protected winning world/content sources.
use super::{Result, inspection_input::Order};
use fallout_data::{
    condition_operands::Signatures,
    identity::FormKey,
    loaded_scripts::{Catalogue, Limits as ScriptLimits},
    resource_jobs::{self, Generation, ResourceJobs},
    store::RecordStore,
    terrain::preparation::{Receipt as TerrainReceipt, TexturePreparation, TextureSourcePlan},
    vfs::MountIndex,
    world::{
        activation::PlacedActivationSources,
        cells::CellGridSources,
        conversation::{DialogueSources, Limits},
        doors::DoorDestination,
        environment::CellEnvironmentSources,
        lighting::CellLightingSources,
        linked::PlacedLinkedSources,
        ownership::CellOwnershipSources,
        preparation::CellModelPlan,
        regions::CellRegionSources,
        residency::{CellResidency, Snapshot, Stage, TerrainState, TexturePlan, TextureState},
        water::CellWaterSources,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) struct ResidencyInput {
    pub cell: FormKey,
    pub source_timeout_ms: u64,
}

/// An executable source consumer over the same leased model/texture jobs used
/// by the host. It never reports GPU, collision, behavior or dependency Ready.
pub(super) fn residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: ResidencyInput,
) -> Result<Value> {
    let deadline = source_deadline(input.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
    let plan = CellModelPlan::load(&mut store, &input.cell, assets.mounts(), Default::default())?;
    let mut report = consume_plan(install, resource_cache, deadline, plan, assets.mounts())?;
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) struct DoorInput {
    pub source: ResidencyInput,
    pub door: FormKey,
}

pub(super) struct GridInput {
    pub world: FormKey,
    pub grid: [i32; 2],
    pub source_timeout_ms: u64,
}

pub(super) struct GridSetInput {
    pub world: FormKey,
    pub grids: Vec<[i32; 2]>,
}

pub(super) fn parse_grid_set(values: &[String]) -> Result<Vec<[i32; 2]>> {
    if values.is_empty() || values.len() > 8 {
        return Err("explicit source grid set requires 1..=8 pairs".into());
    }
    values
        .iter()
        .map(|value| {
            let (x, y) = value.split_once(',').ok_or("grid must be signed i32 x,y")?;
            Ok([x.parse::<i32>()?, y.parse::<i32>()?])
        })
        .collect()
}

pub(super) struct GridPatchInput {
    pub world: FormKey,
    pub grids: Vec<[i32; 2]>,
    pub seams: Vec<[usize; 2]>,
}
pub(super) fn parse_seam_set(values: &[String]) -> Result<Vec<[usize; 2]>> {
    if values.len() > 8 {
        return Err("explicit seam set permits at most 8 pairs".into());
    }
    values
        .iter()
        .map(|value| {
            let (first, second) = value
                .split_once(',')
                .ok_or("seam must be two unsigned patch indices")?;
            Ok([first.parse::<usize>()?, second.parse::<usize>()?])
        })
        .collect()
}
/// Consumes the same private immutable CPU bundle that a future upload caller borrows.
pub(super) fn terrain_patches(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    input: GridPatchInput,
) -> Result<Value> {
    if input.grids.is_empty() || input.grids.len() > 8 || input.seams.len() > 8 {
        return Err("explicit terrain bundle requires 1..=8 grids and at most 8 seams".into());
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Explicit private grid requests consume an immutable source CPU terrain patch bundle",
        "requested_world":input.world,"explicit_grids":input.grids,"explicit_seams":input.seams,
        "grid_directory":null,"cell_grid_requests":null,"terrain_patch_bundle":null,
        "patch_hashes":[],"source_error":null,"cpu_bundle_prepared":false,
        "source_materials_prepared":false,"texture_images_decoded":false,
        "seams_repaired":false,"rendering_admitted":false,"collision_admitted":false,
        "current_cell_changed":false,"activation_applied":false,"runtime_ready":false,
        "lookup_precedence_verified":false,"retail_parity_accepted":false});
    let consumed = (|| -> Result<()> {
        let directory = CellGridSources::load(&mut store, &input.world, Default::default())?;
        let metadata = directory.metadata();
        report["grid_directory"] = json!({"world":metadata.world,
            "world_source_ordinal":metadata.world_source_ordinal,"world_header":metadata.world_header,
            "source_cohort_sha256":metadata.source_cohort_sha256,"usage":metadata.usage});
        let requests = input
            .grids
            .iter()
            .map(|grid| directory.request(*grid))
            .collect::<fallout_data::Result<Vec<_>>>()?;
        report["cell_grid_requests"] = serde_json::to_value(&requests)?;
        let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
        let bundle = fallout_data::terrain::patches::TerrainPatchBundle::load(
            &directory,
            &mut store,
            &requests,
            assets.mounts(),
            &input.seams,
            Default::default(),
        )?;
        bundle.validate_sources(&mut store)?;
        // These bytes hash actual borrowed outputs, never a second terrain evaluator.
        let mut hashes = Vec::with_capacity(bundle.patches().len());
        for patch in bundle.patches() {
            let surface = serde_json::to_vec(&patch.surface)?;
            let geometry = serde_json::to_vec(&patch.mesh)?;
            let blend = serde_json::to_vec(&patch.blends)?;
            hashes.push(json!({"patch_identity":patch.identity,
                "source_plan_identity":patch.source().identity,"cell":patch.source().root,
                "surface_sha256":format!("{:x}",Sha256::digest(&surface)),"surface_json_bytes":surface.len(),
                "geometry_sha256":format!("{:x}",Sha256::digest(&geometry)),"geometry_json_bytes":geometry.len(),
                "blend_sha256":format!("{:x}",Sha256::digest(&blend)),"blend_json_bytes":blend.len()}));
        }
        // Publish together only after source validation, all builders, seams and hashes succeed.
        let receipt = serde_json::to_value(bundle.receipt())?;
        report["terrain_patch_bundle"] = receipt;
        report["patch_hashes"] = json!(hashes);
        report["cpu_bundle_prepared"] = json!(true);
        Ok(())
    })();
    if let Err(error) = consumed {
        report["source_error"] = json!(error.to_string());
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn water(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: ResidencyInput,
) -> Result<Value> {
    let timeout = source_deadline(input.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact CELL water declarations and existing noise member source job",
        "requested_cell":input.cell,"water_sources":null,"source_error":null,
        "source_request_prepared":false,"source_inputs_available":false,
        "noise_requested":false,"noise_available":false,"noise_payload":null,
        "noise_job_usage_while_retained":null,"noise_job_usage_after_release":null,
        "finite_plane_computed":false,"inheritance_evaluated":false,
        "rendering_admitted":false,"runtime_ready":false,
        "lookup_precedence_verified":false,"retail_parity_accepted":false});
    let consumed = (|| -> Result<()> {
        let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
        let sources =
            CellWaterSources::load(&mut store, &input.cell, assets.mounts(), Default::default())?;
        report["water_sources"] = serde_json::to_value(&sources)?;
        report["source_request_prepared"] = json!(true);
        let receipt = sources.receipt();
        let noise_requested = receipt.noise.as_ref().is_some_and(|n| n.request.is_some());
        report["noise_requested"] = json!(noise_requested);
        let owner = Generation::new(sources.identity().to_owned())?;
        let jobs = ResourceJobs::new(
            resource_jobs::Limits {
                workers: 1,
                outstanding: 1,
                decoded_bytes: 64 * 1024 * 1024,
            },
            owner.clone(),
        )?;
        let cache = resource_cache.map(|root| (root.to_path_buf(), install.to_path_buf()));
        let handle = sources.submit_noise(&mut store, &jobs, owner.token()?, cache)?;
        if let Some(handle) = handle {
            let start = Instant::now();
            let mut artifact = loop {
                if let Some(artifact) = handle.try_take()? {
                    break artifact;
                }
                if start.elapsed() >= timeout {
                    handle.cancel();
                    return Err("water noise source job exceeded polling deadline".into());
                }
                thread::sleep(Duration::from_millis(1));
            };
            let request = receipt
                .noise
                .as_ref()
                .and_then(|n| n.request.as_ref())
                .ok_or("admitted water noise job has no source request")?;
            if artifact.bytes().len() != request.decoded_bytes {
                return Err("water noise source extent differs from admitted request".into());
            }
            report["noise_payload"] = json!({"bytes":artifact.bytes().len(),
                "sha256":format!("{:x}",Sha256::digest(artifact.bytes())),
                "path":request.path,"archive_sha256":request.archive_sha256,
                "source_identity":sources.identity(),"generation":owner.token()?.generation(),
                "cache":artifact.take_cache_receipt()});
            report["noise_available"] = json!(true);
            let usage = jobs.usage();
            report["noise_job_usage_while_retained"] =
                json!({"outstanding":usage.outstanding,"decoded_bytes":usage.decoded_bytes});
            drop(artifact);
        }
        let usage = jobs.usage();
        report["noise_job_usage_after_release"] =
            json!({"outstanding":usage.outstanding,"decoded_bytes":usage.decoded_bytes});
        let declared = receipt.xclw.is_some()
            || receipt
                .water_type
                .as_ref()
                .is_some_and(|w| w.target.status == "resolved")
            || noise_requested;
        let water_type_available = receipt
            .water_type
            .as_ref()
            .is_none_or(|w| matches!(w.target.status, "resolved" | "null"));
        let noise_available = receipt.noise.as_ref().is_none_or(|n| {
            n.status == "empty-declaration"
                || (n.request.is_some() && report["noise_available"] == true)
        });
        report["source_inputs_available"] =
            json!(declared && water_type_available && noise_available);
        Ok(())
    })();
    if let Err(error) = consumed {
        report["source_error"] = json!(error.to_string());
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn activation(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    reference: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = PlacedActivationSources::load(&mut store, &reference, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact placed activation-parent, raw delay word and prompt source declarations",
        "requested_reference":reference,"activation_sources":null,"source_request_prepared":false,"source_error":null,
        "activation_flags_declared":false,"parent_count":0,"parent_resolution_statuses":[],
        "parent_header_sources_available":false,"prompt_declared":false,
        "target_bodies_decoded":false,"parent_graph_walked":false,"delay_interpreted":false,"timer_scheduled":false,
        "native_activation_executed":false,"condition_truth_evaluated":false,"actor_item_state_mutated":false,
        "current_cell_mutated":false,"prompt_decoded":false,"default_prompt_selected":false,
        "runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            let r = sources.receipt();
            report["activation_flags_declared"] = json!(r.activation_flags.is_some());
            report["parent_count"] = json!(r.parents.len());
            report["parent_resolution_statuses"] = json!(
                r.parents
                    .iter()
                    .map(|p| p.target.status)
                    .collect::<Vec<_>>()
            );
            report["parent_header_sources_available"] =
                json!(!r.parents.is_empty() && r.parents.iter().all(|p| p.header_source_available));
            report["prompt_declared"] = json!(r.prompt.is_some());
            report["activation_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn linked(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    reference: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = PlacedLinkedSources::load(&mut store, &reference, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact placed linked-reference and raw color source declarations",
        "requested_reference":reference,"linked_sources":null,"source_request_prepared":false,"source_error":null,
        "link_declared":false,"color_declared":false,"link_header_source_available":false,"link_resolution_status":"absent",
        "target_body_decoded":false,"target_placement_admitted":false,"runtime_binding_evaluated":false,
        "native_get_linked_ref_executed":false,"actor_package_selected":false,"chain_walked":false,
        "runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            let r = sources.receipt();
            report["link_declared"] = json!(r.link.is_some());
            report["color_declared"] = json!(r.color.is_some());
            report["link_header_source_available"] =
                json!(r.link.as_ref().is_some_and(|l| l.header_source_available));
            report["link_resolution_status"] =
                json!(r.link.as_ref().map_or("absent", |l| l.target.status));
            report["linked_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn ownership(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    cell: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = CellOwnershipSources::load(&mut store, &cell, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact FNV CELL owner and signed rank source declarations",
        "requested_cell":cell,"ownership_sources":null,"source_request_prepared":false,"source_error":null,
        "owner_declared":false,"rank_declared":false,"owner_header_source_available":false,"owner_resolution_status":"absent",
        "ownership_evaluated":false,"actor_faction_rank_evaluated":false,"access_evaluated":false,"theft_evaluated":false,
        "target_behavior_decoded":false,"runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            let receipt = sources.receipt();
            report["owner_declared"] = json!(receipt.owner.is_some());
            report["rank_declared"] = json!(receipt.rank.is_some());
            report["owner_header_source_available"] = json!(
                receipt
                    .owner
                    .as_ref()
                    .is_some_and(|o| o.header_source_available)
            );
            report["owner_resolution_status"] =
                json!(receipt.owner.as_ref().map_or("absent", |o| o.target.status));
            report["ownership_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn regions(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    cell: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = CellRegionSources::load(&mut store, &cell, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact on-disk CELL region occurrences and winning REGN header inputs",
        "requested_cell":cell,"region_sources":null,"source_request_prepared":false,"source_error":null,
        "region_fields_declared":false,"region_element_count":0,"resolved_header_count":0,"unavailable_header_count":0,
        "target_behavior_decoded":false,"world_parent_evaluated":false,"region_geometry_evaluated":false,
        "region_priority_evaluated":false,"region_effects_applied":false,"runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            let receipt = sources.receipt();
            let resolved = receipt
                .occurrences
                .iter()
                .flat_map(|o| &o.elements)
                .filter(|e| e.header_source_available)
                .count();
            report["region_fields_declared"] = json!(!receipt.occurrences.is_empty());
            report["region_element_count"] = json!(receipt.usage.elements);
            report["resolved_header_count"] = json!(resolved);
            report["unavailable_header_count"] = json!(receipt.usage.elements - resolved);
            report["region_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn environment(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    cell: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = CellEnvironmentSources::load(&mut store, &cell, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Explicit CELL environment declarations and winning target header inputs",
        "requested_cell":cell,"environment_sources":null,"field_statuses":[],
        "source_request_prepared":false,"source_error":null,
        "target_behavior_decoded":false,"world_parent_evaluated":false,
        "environment_selected":false,"runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            let mut statuses = Vec::new();
            for kind in [b"XCCM", b"XCIM", b"XEZN", b"XCAS", b"XCMO"] {
                let row = sources.receipt().links.iter().find(|row| &row.kind == kind);
                statuses.push(json!({"kind":std::str::from_utf8(kind)?,
                    "declared":row.is_some(),
                    "header_source_available":row.is_some_and(|row|row.header_source_available),
                    "resolution_status":row.map_or("absent",|row|row.target.status),
                    "behavior_status":"unknown; target body and behavior not decoded"}));
            }
            report["field_statuses"] = json!(statuses);
            report["environment_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn lighting(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    cell: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let prepared = CellLightingSources::load(&mut store, &cell, Default::default());
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact winning CELL lighting and declared template source inputs",
        "requested_cell":cell,"lighting_sources":null,"source_error":null,
        "source_request_prepared":false,"source_inputs_available":false,
        "inheritance_evaluated":false,"rendering_admitted":false,
        "runtime_ready":false,"retail_parity_accepted":false});
    match prepared {
        Ok(sources) => {
            report["source_inputs_available"] = json!(sources.source_inputs_available());
            report["lighting_sources"] = serde_json::to_value(&sources)?;
            report["source_request_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn persistent_cell(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    world: FormKey,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = CellGridSources::load(&mut store, &world, Default::default())?;
    let mut selected = None;
    let prepared = (|| -> fallout_data::Result<_> {
        let request = sources.request_persistent()?;
        selected = Some(request.clone());
        let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
        sources.prepare_persistent(&mut store, &request, assets.mounts(), Default::default())
    })();
    let entry = selected.as_ref().and_then(|request| {
        sources
            .metadata()
            .entries
            .iter()
            .find(|entry| &entry.key == request.cell())
    });
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Exact source persistent CELL group, separately requested without grid authority",
        "cell_grid_sources":sources.metadata(),"persistent_request":selected,
        "persistent_cell":entry,"cell_models":null,"dependency_usage":null,
        "root_members":null,"source_error":null,"source_plan_prepared":false,
        "complete_model_selection":false,"residency":null,"runtime_ready":false,
        "current_cell_changed":false,"activation_applied":false,
        "lookup_precedence_verified":false,"retail_parity_accepted":false});
    match prepared {
        Ok(plan) => {
            report["complete_model_selection"] = json!(
                plan.receipt()
                    .coverage
                    .iter()
                    .all(|base| base.status == "one-archive-source; retail-precedence-unverified")
            );
            report["cell_models"] = serde_json::to_value(plan.receipt())?;
            report["dependency_usage"] = serde_json::to_value(&plan.graph().usage)?;
            report["root_members"] = json!(plan.graph().root_members);
            report["source_plan_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

/// Source planning only: each selected plan remains owned by the aggregate set.
pub(super) fn grid_set(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    input: GridSetInput,
) -> Result<Value> {
    // Refuse an oversized direct caller before touching its source tree as well.
    if input.grids.is_empty() || input.grids.len() > 8 {
        return Err("explicit source grid set requires 1..=8 pairs".into());
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = CellGridSources::load(&mut store, &input.world, Default::default())?;
    let mut selected = None;
    let prepared = (|| -> fallout_data::Result<_> {
        let request = sources.request_set(&input.grids)?;
        selected = Some(request.clone());
        let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
        sources.prepare_cells(&mut store, &request, assets.mounts(), Default::default())
    })();
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Explicit ordered WRLD/XCLC CELL source plans under one construction budget",
        "cell_grid_sources":sources.metadata(),"explicit_grids":input.grids,
        "cell_grid_set_request":selected,"cell_model_plan_set":null,
        "selected_cells":[],"source_error":null,"source_plans_prepared":false,
        "complete_model_selection":false,"current_cell_changed":false,
        "activation_applied":false,"runtime_ready":false,"lookup_precedence_verified":false,
        "retail_parity_accepted":false});
    match prepared {
        Ok(set) => {
            let mut entries = Vec::with_capacity(set.requests().len());
            for request in set.requests() {
                entries.push(
                    sources
                        .metadata()
                        .entries
                        .iter()
                        .find(|entry| &entry.key == request.cell())
                        .ok_or("selected CELL absent from sealed directory")?,
                );
            }
            report["selected_cells"] = serde_json::to_value(entries)?;
            report["complete_model_selection"] = json!((0..set.requests().len()).all(|index| {
                set.plan(index)
                    .expect("private source set plan count matches requests")
                    .receipt()
                    .coverage
                    .iter()
                    .all(|base| base.status == "one-archive-source; retail-precedence-unverified")
            }));
            report["cell_model_plan_set"] = serde_json::to_value(&set)?;
            report["source_plans_prepared"] = json!(true);
        }
        Err(error) => report["source_error"] = json!(error.to_string()),
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) fn grid_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::Models,
    )
}

pub(super) fn grid_terrain(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::TerrainTextures,
    )
}

pub(super) fn grid_cell_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
) -> Result<Value> {
    grid_report(
        install,
        order_path,
        index_cache,
        resource_cache,
        input,
        GridKind::CellSources,
    )
}

#[derive(Clone, Copy)]
enum GridKind {
    Models,
    TerrainTextures,
    CellSources,
}

fn grid_report(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: GridInput,
    kind: GridKind,
) -> Result<Value> {
    let deadline = source_deadline(input.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = CellGridSources::load(&mut store, &input.world, Default::default())?;
    let request = sources.request(input.grid);
    let mut selected = None;
    let plan = match request {
        Ok(request) => {
            selected = Some(serde_json::to_value(&request)?);
            let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
            match kind {
                GridKind::Models => match sources.prepare_cell(
                    &mut store,
                    &request,
                    assets.mounts(),
                    Default::default(),
                ) {
                    Ok(plan) => Ok(consume_plan(
                        install,
                        resource_cache,
                        deadline,
                        plan,
                        assets.mounts(),
                    )?),
                    Err(error) => Err(error.to_string()),
                },
                GridKind::TerrainTextures => match sources.prepare_terrain(
                    &mut store,
                    &request,
                    assets.mounts(),
                    Default::default(),
                ) {
                    Ok(plan) => Ok(consume_terrain(install, resource_cache, deadline, plan)?),
                    Err(error) => Err(error.to_string()),
                },
                GridKind::CellSources => {
                    let prepared = (|| -> fallout_data::Result<_> {
                        let models = sources.prepare_cell(
                            &mut store,
                            &request,
                            assets.mounts(),
                            Default::default(),
                        )?;
                        let terrain = sources.prepare_terrain(
                            &mut store,
                            &request,
                            assets.mounts(),
                            Default::default(),
                        )?;
                        Ok((models, terrain))
                    })();
                    match prepared {
                        Ok((models, terrain)) => Ok(consume_cell_plan(
                            install,
                            resource_cache,
                            deadline,
                            models,
                            assets.mounts(),
                            Some(terrain),
                        )?),
                        Err(error) => Err(error.to_string()),
                    }
                }
            }
        }
        Err(error) => Err(error.to_string()),
    };
    let mut report = match plan {
        Ok(report) => report,
        Err(error) => json!({"schema_version":1,"profile":"nv-original",
            "source_error":error,"cell_models":null,"cell_textures":null,
            "texture_payloads":[],"residency":null,"captured_sources_available":false,
            "lookup_precedence_verified":false,"runtime_ready":false,"retail_parity_accepted":false}),
    };
    provenance(&mut report, &order, &mut store)?;
    report["cell_grid_sources"] = serde_json::to_value(sources.metadata())?;
    report["explicit_grid"] = json!(input.grid);
    report["cell_grid_request"] = json!(selected);
    report["current_cell_changed"] = json!(false);
    report["activation_applied"] = json!(false);
    match kind {
        GridKind::Models => {
            report["scope"] = json!(
                "Explicit WRLD/XCLC source CELL model/texture jobs; persistent groups remain separate, no position/grid inference or runtime activation"
            )
        }
        GridKind::TerrainTextures => {
            report["surface_prepared"] = json!(false);
            report["scope"] = json!(
                "Explicit WRLD/XCLC CELL strict LAND/world/layer external texture source jobs; surface, inheritance and runtime activation remain unadmitted"
            );
        }
        GridKind::CellSources => {
            report["terrain_scope_requested"] = json!(true);
            report["surface_prepared"] = json!(false);
            report["scope"] = json!(
                "Explicit WRLD/XCLC CELL model and both texture source batches in one residency epoch; retained leases checked across unload and final quota drain; no surface, inheritance or runtime activation"
            );
        }
    }
    Ok(report)
}

fn consume_terrain(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: TextureSourcePlan,
) -> Result<Value> {
    let source_plan = serde_json::to_value(plan.receipt())?;
    let mut preparation =
        TexturePreparation::new(plan, install, resource_cache, Default::default())?;
    let start = Instant::now();
    let result = (|| -> Result<TerrainReceipt> {
        loop {
            if preparation.poll()? {
                let ready = preparation
                    .take_ready()?
                    .ok_or("terrain source batch completed without receipt")?;
                return Ok(ready.publish_for(preparation.plan().terrain())?);
            }
            if start.elapsed() >= deadline {
                return Err(
                    "terrain source polling deadline exceeded; owned request cancelled".into(),
                );
            }
            thread::sleep(Duration::from_millis(1));
        }
    })();
    let (textures, error, available) = match result {
        Ok(receipt) => {
            let available = receipt.all_requested_texture_sources_ready
                && receipt.all_authored_texture_sources_resolved;
            (Some(serde_json::to_value(receipt)?), None, available)
        }
        Err(error) => {
            preparation.cancel();
            (None, Some(error.to_string()), false)
        }
    };
    let usage = preparation.usage();
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "terrain_source_plan":source_plan,"terrain_textures":textures,
        "terrain_job_usage":{"outstanding":usage.outstanding,"decoded_bytes":usage.decoded_bytes},"source_error":error,
        "captured_sources_available":available,"lookup_precedence_verified":false,
        "surface_prepared":false,"runtime_ready":false,"retail_parity_accepted":false}))
}

pub(super) fn door_residency(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    resource_cache: Option<&Path>,
    input: DoorInput,
) -> Result<Value> {
    let deadline = source_deadline(input.source.source_timeout_ms)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let sources = DoorDestination::load(
        &mut store,
        &input.source.cell,
        &input.door,
        Default::default(),
    )?;
    let assets = fallout_data::assets::ArchiveAssets::open_nv(install)?;
    let mut report = match sources.prepare_cell(&mut store, assets.mounts(), Default::default()) {
        Ok(plan) => consume_plan(install, resource_cache, deadline, plan, assets.mounts())?,
        Err(error) => json!({"schema_version":1,"profile":"nv-original",
            "source_error":error.to_string(),"cell_models":null,"cell_textures":null,
            "texture_payloads":[],"residency":null,"captured_sources_available":false,
            "lookup_precedence_verified":false,"runtime_ready":false,"retail_parity_accepted":false}),
    };
    provenance(&mut report, &order, &mut store)?;
    report["door_destination"] = serde_json::to_value(sources.metadata())?;
    report["door_source_graph"] = serde_json::to_value(sources.graph())?;
    report["destination_applied"] = json!(false);
    report["current_cell_changed"] = json!(false);
    report["scope"] = json!(
        "Source-selected XTEL destination CELL model/texture jobs; no movement, activation, GPU, physics or behavior admission"
    );
    Ok(report)
}

fn source_deadline(milliseconds: u64) -> Result<Duration> {
    if !(1..=120_000).contains(&milliseconds) {
        return Err("source polling timeout must be 1..=120000 milliseconds".into());
    }
    Ok(Duration::from_millis(milliseconds))
}

fn provenance(report: &mut Value, order: &Order, store: &mut RecordStore) -> Result<()> {
    report["explicit_load_order"] = json!(order.names);
    report["load_order_sha256"] = json!(order.sha256);
    report["plugins"] = serde_json::to_value(store.source_receipts()?)?;
    Ok(())
}

fn consume_plan(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: CellModelPlan,
    mounts: &MountIndex,
) -> Result<Value> {
    consume_cell_plan(install, resource_cache, deadline, plan, mounts, None)
}

fn consume_cell_plan(
    install: &Path,
    resource_cache: Option<&Path>,
    deadline: Duration,
    plan: CellModelPlan,
    mounts: &MountIndex,
    terrain: Option<TextureSourcePlan>,
) -> Result<Value> {
    let model_receipt = serde_json::to_value(plan.receipt())?;
    let terrain_requested = terrain.is_some();
    let terrain_receipt = terrain
        .as_ref()
        .map(|plan| serde_json::to_value(plan.receipt()))
        .transpose()?;
    let mut owner = CellResidency::new(install, resource_cache, Default::default())?;
    let ticket = owner.request(plan)?;
    let mut error = None;
    let mut textures = None;
    let mut texture_payloads = Vec::new();
    let mut terrain_payloads = Vec::new();
    if let Err(failed) = poll_sources(&mut owner, false, deadline) {
        error = Some(failed.to_string());
    } else {
        match TexturePlan::load(owner.sources(&ticket)?, mounts, Default::default()) {
            Ok(plan) => {
                textures = Some(serde_json::to_value(plan.receipt())?);
                owner.request_textures(&ticket, plan)?;
                if let Some(plan) = terrain {
                    owner.request_terrain(&ticket, plan)?;
                }
                if let Err(failed) =
                    poll_cell_sources(&mut owner, true, terrain_requested, deadline)
                {
                    error = Some(failed.to_string());
                } else {
                    // Consume the retained lease, not another archive/cache read.
                    let sources = owner.texture_sources(&ticket)?;
                    for index in 0..sources.receipt()?.requests.len() {
                        let bytes = sources.texture(index)?;
                        if bytes.len() != sources.receipt()?.requests[index].decoded_bytes {
                            return Err(
                                "resident texture extent differs from sealed source request".into(),
                            );
                        }
                        texture_payloads.push(json!({"request":index,"bytes":bytes.len(),
                            "sha256":format!("{:x}", Sha256::digest(bytes))}));
                    }
                    if terrain_requested {
                        let sources = owner.terrain_sources(&ticket)?;
                        for (index, request) in sources.receipt()?.requests.iter().enumerate() {
                            let bytes = sources.texture(index)?;
                            if bytes.len() != request.decoded_bytes {
                                return Err("resident terrain texture extent differs from sealed source request".into());
                            }
                            terrain_payloads.push(json!({"request":index,"bytes":bytes.len(),
                                "sha256":format!("{:x}", Sha256::digest(bytes)),
                                "cell_identity":sources.ticket().identity(),"generation":sources.ticket().generation()}));
                        }
                    }
                }
            }
            Err(failed) => error = Some(failed.to_string()),
        }
    }
    let snapshot = owner.snapshot();
    let available = error.is_none()
        && snapshot.complete_model_coverage
        && snapshot.complete_texture_coverage
        && (!terrain_requested || snapshot.complete_terrain_coverage);
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "cell_models":model_receipt,"cell_textures":textures,"texture_payloads":texture_payloads,
        "residency":snapshot,"source_error":error,
        "captured_sources_available":available,"lookup_precedence_verified":false,
        "runtime_ready":false,"retail_parity_accepted":false,
        "scope":"Protected exact CELL/model/external-texture source jobs; no GPU/DDS/physics/behavior admission"});
    if terrain_requested {
        report["cell_terrain"] = json!(terrain_receipt);
        report["terrain_payloads"] = json!(terrain_payloads);
        // Exercise the real lifetime with leases held across unload. Payloads
        // are consumed above; no source read or staging can follow revocation.
        let models = owner.sources(&ticket).ok();
        let textures = owner.texture_sources(&ticket).ok();
        let terrain = owner.terrain_sources(&ticket).ok();
        let terrain_ticket_matches = terrain.as_ref().is_some_and(|sources| {
            sources.ticket().identity() == ticket.identity()
                && sources.ticket().generation() == ticket.generation()
                && sources.ticket().root() == ticket.root()
        });
        owner.unload()?;
        report["residency_after_unload"] = serde_json::to_value(owner.poll()?)?;
        report["retained_source_lifetime"] = json!({
            "terrain_ticket_matches_cell":terrain_ticket_matches,
            "old_ticket_rejected":ticket.check().is_err(),
            "old_owner_access_rejected":owner.terrain_sources(&ticket).is_err(),
            "borrowed_model_access_rejected":models.as_ref().map(|s| s.plan().is_err()),
            "borrowed_texture_access_rejected":textures.as_ref().map(|s| s.receipt().is_err()),
            "borrowed_terrain_access_rejected":terrain.as_ref().map(|s| s.receipt().is_err()),
            "terrain_lease_retained":terrain.is_some()});
        drop(terrain);
        drop(textures);
        drop(models);
        let start = Instant::now();
        loop {
            let drained = owner.poll()?;
            if drained.stage == Stage::Unrequested {
                report["residency_after_release"] = serde_json::to_value(drained)?;
                break;
            }
            if start.elapsed() >= deadline {
                return Err(
                    "unloaded CELL source reservations did not drain within deadline".into(),
                );
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(report)
}

fn poll_sources(owner: &mut CellResidency, textures: bool, timeout: Duration) -> Result<Snapshot> {
    poll_cell_sources(owner, textures, false, timeout)
}
fn poll_cell_sources(
    owner: &mut CellResidency,
    textures: bool,
    terrain: bool,
    timeout: Duration,
) -> Result<Snapshot> {
    let start = Instant::now();
    loop {
        let snapshot = owner.poll()?;
        let complete = if textures {
            matches!(
                snapshot.texture_state,
                TextureState::Decoded | TextureState::Unsupported
            )
        } else {
            snapshot.stage == Stage::Decoded
        };
        let terrain_complete = !terrain
            || matches!(
                snapshot.terrain_state,
                TerrainState::Decoded | TerrainState::Unsupported
            );
        if complete && terrain_complete {
            return Ok(snapshot);
        }
        if start.elapsed() >= timeout {
            owner.unload()?;
            return Err("source polling deadline exceeded; owned cell request cancelled".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BatchRequestSpec {
    topic: String,
    info: String,
    speaker: Option<String>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchInputSpec {
    schema_version: u32,
    requests: Vec<BatchRequestSpec>,
}
/// One protected input and one source-bound all-or-none batch; no selection or execution.
pub(super) fn conversation_batch(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    requests_path: &Path,
    bind_fragments: bool,
) -> Result<Value> {
    use std::io::Read;
    let mut input_source = fallout_data::baseline::open_source(requests_path)?;
    let mut input_bytes = Vec::new();
    (&mut input_source)
        .take(65537)
        .read_to_end(&mut input_bytes)?;
    if input_bytes.len() > 65536 {
        return Err("conversation batch input exceeds 64 KiB".into());
    }
    let input: BatchInputSpec = serde_json::from_slice(&input_bytes)?;
    if input.schema_version != 1 || input.requests.is_empty() || input.requests.len() > 8 {
        return Err("conversation batch schema1 requires 1..=8 explicit requests".into());
    }
    let input_sha256 = format!("{:x}", Sha256::digest(&input_bytes));
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Explicit source conversation batch consumes retained subtitle spans and loaded fragment definitions",
        "input_sha256":input_sha256,"explicit_requests":input.requests,
        "fragment_binding_requested":bind_fragments,"membership_metadata_bytes":null,
        "conversation_batch":null,"subtitle_payloads":[],"loaded_fragments":[],
        "source_error":null,"batch_request_prepared":false,"fragment_bindings_available":false,
        "condition_signatures_supplied":false,"selection_order_verified":false,
        "condition_truth_verified":false,"speaker_assignment_verified":false,
        "voice_filename_verified":false,"fragment_timing_verified":false,
        "fragment_execution_admitted":false,"runtime_ready":false,"retail_parity_accepted":false});
    let consumed = (|| -> Result<()> {
        let sources = DialogueSources::build(&mut store, Limits::default())?;
        report["membership_metadata_bytes"] = json!(sources.retained_bytes());
        let requests = input
            .requests
            .iter()
            .map(|entry| {
                Ok(sources.request(
                    crate::parse_cell_key(&entry.topic)?,
                    crate::parse_cell_key(&entry.info)?,
                    entry
                        .speaker
                        .as_ref()
                        .map(|key| crate::parse_cell_key(key))
                        .transpose()?,
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        let batch = sources.prepare_batch(
            &mut store,
            &requests,
            &Signatures::new(),
            Default::default(),
        )?;
        let mut payloads = Vec::new();
        for (request_index, prepared) in batch.conversations().iter().enumerate() {
            for (response_index, response) in prepared.metadata().responses.iter().enumerate() {
                let mut occurrence = 0;
                for &field_index in &response.fields {
                    let field = &prepared.metadata().info_fields[field_index];
                    if field.kind != *b"NAM1" {
                        continue;
                    }
                    let bytes = prepared
                        .info_bytes(field_index)
                        .ok_or("batch subtitle field has no retained source span")?;
                    let sha256 = format!("{:x}", Sha256::digest(bytes));
                    if sha256 != field.sha256 {
                        return Err("batch retained subtitle bytes differ from source field".into());
                    }
                    payloads.push(json!({"request":request_index,"response":response_index,
                        "source_response_number":response.number,"occurrence":occurrence,
                        "info_field":field_index,"bytes":bytes.len(),"sha256":sha256}));
                    occurrence += 1;
                }
            }
        }
        let mut loaded = Vec::new();
        if bind_fragments {
            let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(()))?;
            for (request_index, prepared) in batch.conversations().iter().enumerate() {
                for (fragment_index, fragment) in prepared.metadata().fragments.iter().enumerate() {
                    let script = fragment.resolve(&catalogue)?;
                    loaded.push(json!({"request":request_index,"fragment":fragment_index,
                        "handle":script.handle(),"version":script.version(),"owner":script.owner(),
                        "issues":script.issues(),"execution_admitted":false}));
                }
            }
        }
        batch.validate_sources(&mut store)?;
        let projection = serde_json::to_value(&batch)?;
        report["conversation_batch"] = projection;
        report["subtitle_payloads"] = json!(payloads);
        report["loaded_fragments"] = json!(loaded);
        report["fragment_bindings_available"] = json!(bind_fragments);
        report["batch_request_prepared"] = json!(true);
        Ok(())
    })();
    if let Err(error) = consumed {
        report["source_error"] = json!(error.to_string());
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) struct ConversationPagesInput {
    pub topic: FormKey,
    pub info: Option<FormKey>,
    pub speaker: Option<FormKey>,
    pub page_size: usize,
    pub max_pages: usize,
}
/// One index, bounded in-process pages, then at most one caller-selected body.
pub(super) fn conversation_pages(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    input: ConversationPagesInput,
) -> Result<Value> {
    use fallout_data::world::conversation::{MembershipPage, PageLimits};
    if !(1..=256).contains(&input.page_size) || !(1..=16).contains(&input.max_pages) {
        return Err("conversation pages require page-size1..=256 and max-pages1..=16".into());
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "scope":"Canonical structural source pages and one explicitly selected returned conversation",
        "topic":input.topic,"selected_info":input.info,"explicit_speaker":input.speaker,
        "page_size":input.page_size,"max_pages":input.max_pages,"pages":[],
        "pages_prepared":false,"membership_state":"unavailable","membership_metadata_bytes":null,
        "conversation":null,"subtitle_payloads":[],"selected_request_prepared":false,
        "selected_page":null,"selected_member":null,"source_error":null,
        "original_menu_order_verified":false,"condition_truth_verified":false,
        "speaker_assignment_verified":false,"voice_filename_verified":false,
        "fragment_execution_admitted":false,"runtime_ready":false,"retail_parity_accepted":false});
    let consumed = (|| -> Result<()> {
        let sources = DialogueSources::build(&mut store, Limits::default())?;
        report["membership_metadata_bytes"] = json!(sources.retained_bytes());
        let mut pages: Vec<MembershipPage> = Vec::new();
        for _ in 0..input.max_pages {
            let page = sources.page(
                &input.topic,
                input.speaker.as_ref(),
                pages.last().and_then(MembershipPage::cursor),
                PageLimits {
                    members: input.page_size,
                    ..PageLimits::default()
                },
            )?;
            let done = page.cursor().is_none();
            pages.push(page);
            if done {
                break;
            }
        }
        report["membership_state"] = json!(if pages[0].total_members() == 0 {
            "empty_structural_membership"
        } else {
            "canonical_structural_members"
        });
        report["pages"] = serde_json::to_value(&pages)?;
        report["pages_prepared"] = json!(true);
        let Some(selected) = input.info.as_ref() else {
            return Ok(());
        };
        let (page_index, member_index, request) = pages
            .iter()
            .enumerate()
            .find_map(|(p, page)| {
                page.requests()
                    .iter()
                    .enumerate()
                    .find(|(_, request)| request.info() == selected)
                    .map(|(r, request)| (p, r, request))
            })
            .ok_or("explicit INFO was not returned in the bounded source pages")?;
        let prepared =
            sources.prepare(&mut store, request, &Signatures::new(), Limits::default())?;
        let mut payloads = Vec::new();
        for (response_index, response) in prepared.metadata().responses.iter().enumerate() {
            let mut occurrence = 0;
            for &field_index in &response.fields {
                let field = &prepared.metadata().info_fields[field_index];
                if field.kind != *b"NAM1" {
                    continue;
                }
                let bytes = prepared
                    .info_bytes(field_index)
                    .ok_or("selected subtitle field has no retained source span")?;
                let sha256 = format!("{:x}", Sha256::digest(bytes));
                if sha256 != field.sha256 {
                    return Err("selected subtitle bytes differ from source field".into());
                }
                payloads.push(json!({"response":response_index,"source_response_number":response.number,
                    "occurrence":occurrence,"info_field":field_index,"bytes":bytes.len(),"sha256":sha256}));
                occurrence += 1;
            }
        }
        report["conversation"] = serde_json::to_value(prepared.metadata())?;
        report["retained_conversation_bytes"] = json!(prepared.retained_bytes());
        report["subtitle_payloads"] = json!(payloads);
        report["selected_page"] = json!(page_index);
        report["selected_member"] = json!(member_index);
        report["selected_request_prepared"] = json!(true);
        Ok(())
    })();
    if let Err(error) = consumed {
        report["source_error"] = json!(error.to_string());
    }
    provenance(&mut report, &order, &mut store)?;
    Ok(report)
}

pub(super) struct Input {
    pub topic: FormKey,
    pub info: FormKey,
    pub speaker: Option<FormKey>,
    pub bind_result_fragments: bool,
}
pub(super) fn conversation(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    input: Input,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let sources = DialogueSources::build(&mut store, Limits::default())?;
    let request = sources.request(input.topic, input.info, input.speaker)?;
    // This is an exact source inspection, not a native function metadata capture.
    // Unknown function signatures remain explicit in each existing CTDA binding.
    let prepared = sources.prepare(&mut store, &request, &Signatures::new(), Limits::default())?;
    let mut subtitle_payloads = Vec::new();
    for (response_index, response) in prepared.metadata().responses.iter().enumerate() {
        let mut occurrence = 0;
        for &field_index in &response.fields {
            let field = &prepared.metadata().info_fields[field_index];
            if field.kind != *b"NAM1" {
                continue;
            }
            // Indexed access over retained bytes avoids repeated nth scans when
            // one response has many distinct NAM1 occurrences.
            let bytes = prepared
                .info_bytes(field_index)
                .ok_or("prepared subtitle field has no retained source span")?;
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            if sha256 != field.sha256 {
                return Err("retained subtitle bytes differ from prepared source field".into());
            }
            subtitle_payloads.push(json!({"response":response_index,
                "source_response_number":response.number,"occurrence":occurrence,
                "info_field":field_index,"bytes":bytes.len(),"sha256":sha256}));
            occurrence += 1;
        }
    }
    let fragments = if input.bind_result_fragments {
        let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(()))?;
        Some(prepared.metadata().fragments.iter().map(|fragment| {
            let loaded = fragment.resolve(&catalogue)?;
            Ok(json!({"handle":loaded.handle(),"version":loaded.version(),"owner":loaded.owner(),
                "issues":loaded.issues(),"execution_admitted":false}))
        }).collect::<fallout_data::Result<Vec<_>>>()?)
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,
        "load_order_sha256":order.sha256,"plugins":store.source_receipts()?,
        "membership_metadata_bytes":sources.retained_bytes(),"retained_conversation_bytes":prepared.retained_bytes(),
        "conversation":prepared.metadata(),"subtitle_payloads":subtitle_payloads,"loaded_fragments":fragments,
        "condition_signatures_supplied":false,"runtime_ready":false,"retail_parity_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Private ignored fixtures, kept for review rather than adding a dependency
    // or deleting an unrelated temporary tree during a shared team run.
    fn directory() -> std::path::PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/world-conversation-cli-fixtures");
        fs::create_dir_all(&root).unwrap();
        let mut ordinal = 0;
        loop {
            let path = root.join(format!("{}-{ordinal}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ordinal += 1,
                Err(error) => panic!("private conversation fixture: {error}"),
            }
        }
    }

    fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
    }
    fn record(kind: &[u8; 4], form: u32, body: &[u8]) -> Vec<u8> {
        [
            kind.as_slice(),
            &(body.len() as u32).to_le_bytes(),
            &[0; 4],
            &form.to_le_bytes(),
            &[0; 8],
            body,
        ]
        .concat()
    }
    fn fixture(root: &Path) {
        fixture_subtitles(root, &[b"authored_fixture_line\0"], None);
    }
    fn fixture_subtitles(root: &Path, subtitles: &[&[u8]], orphan: Option<&[u8]>) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut schr = [0; 20];
        schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
        let mut body = Vec::new();
        if let Some(orphan) = orphan {
            body.extend(field(b"NAM1", orphan));
        }
        body.extend(field(b"TRDT", &[0; 24]));
        for subtitle in subtitles {
            body.extend(field(b"NAM1", subtitle));
        }
        body.extend(field(b"SCHR", &schr));
        body.extend(field(b"SCDA", &[0x1d, 0, 0, 0]));
        let info = record(b"INFO", 0x300, &body);
        let group = [
            b"GRUP".as_slice(),
            &(info.len() as u32 + 24).to_le_bytes(),
            &0x100_u32.to_le_bytes(),
            &7_i32.to_le_bytes(),
            &[0; 8],
            &info,
        ]
        .concat();
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"DIAL", 0x100, &field(b"FULL", b"authored_fixture_topic\0")),
                group,
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
    }
    fn input(info: &str) -> Input {
        Input {
            topic: crate::parse_cell_key("Base.esm:100").unwrap(),
            info: crate::parse_cell_key(info).unwrap(),
            speaker: None,
            bind_result_fragments: true,
        }
    }
    #[test]
    fn cli_consumer_prepares_exact_sources_and_binds_existing_loaded_unit() {
        let directory = directory();
        fixture(&directory);
        let report = conversation(
            &directory,
            &directory.join("order.json"),
            None,
            input("Base.esm:300"),
        )
        .unwrap();
        assert_eq!(report["conversation"]["info"]["key"]["local_id"], 0x300);
        assert_eq!(
            report["subtitle_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored_fixture_line\0"))
        );
        assert_eq!(
            report["conversation"]["responses"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            report["loaded_fragments"][0]["handle"]["key"]["record"]["local_id"],
            0x300
        );
        assert_eq!(report["loaded_fragments"][0]["execution_admitted"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(report["condition_signatures_supplied"], false);
        assert_eq!(report["retail_parity_accepted"], false);
        assert!(!report.to_string().contains("authored_fixture"));
    }
    #[test]
    fn cli_subtitle_consumer_keeps_repeated_non_utf8_occurrences_and_orphans_distinct() {
        let directory = directory();
        let subtitles: [&[u8]; 3] = [b"same\0", &[0xff, 0x80, 0], b"same\0"];
        fixture_subtitles(&directory, &subtitles, Some(b"orphan\0"));
        let report = conversation(
            &directory,
            &directory.join("order.json"),
            None,
            input("Base.esm:300"),
        )
        .unwrap();
        let payloads = report["subtitle_payloads"].as_array().unwrap();
        assert_eq!(payloads.len(), 3);
        for (occurrence, (payload, bytes)) in payloads.iter().zip(subtitles).enumerate() {
            assert_eq!(payload["response"], 0);
            assert_eq!(payload["source_response_number"], 0);
            assert_eq!(payload["occurrence"], occurrence);
            assert_eq!(payload["info_field"], occurrence + 2);
            assert_eq!(payload["bytes"], bytes.len());
            assert_eq!(payload["sha256"], format!("{:x}", Sha256::digest(bytes)));
            assert!(payload.get("text").is_none());
        }
        assert_eq!(
            report["conversation"]["info_fields"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"orphan\0"))
        );
        assert_eq!(
            report["conversation"]["info_fields"][0]["owner_section"],
            serde_json::Value::Null
        );
        assert_eq!(report["loaded_fragments"].as_array().unwrap().len(), 1);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(report["conversation"]["voice_filename_verified"], false);
        assert_eq!(report["conversation"]["condition_truth_verified"], false);
    }
    #[test]
    fn cli_consumer_refuses_info_outside_requested_winning_topic() {
        let directory = directory();
        fixture(&directory);
        assert!(
            conversation(
                &directory,
                &directory.join("order.json"),
                None,
                input("Base.esm:100")
            )
            .is_err()
        );
    }

    fn archive(root: &Path, label: &str, folder: &[u8], name: &[u8], payload: &[u8]) {
        // Independently authored one-folder/file BSA104; use the real importer.
        let table = 54 + folder.len();
        let offset = table + 16 + name.len() + 1;
        let mut bytes = vec![0; offset];
        bytes[..4].copy_from_slice(b"BSA\0");
        for (at, word) in [
            (4, 104),
            (8, 36),
            (12, 3),
            (16, 1),
            (20, 1),
            (24, folder.len() as u32 + 1),
            (28, name.len() as u32 + 1),
            (44, 1),
            (48, 52),
        ] {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[52] = folder.len() as u8 + 1;
        bytes[53..53 + folder.len()].copy_from_slice(folder);
        bytes[table..table + 8].copy_from_slice(&1u64.to_le_bytes());
        bytes[table + 8..table + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes[table + 12..table + 16].copy_from_slice(&(offset as u32).to_le_bytes());
        bytes[table + 16..offset - 1].copy_from_slice(name);
        bytes.extend(payload);
        fs::write(root.join(format!("Data/{label}.bsa")), bytes).unwrap();
    }
    fn cell_fixture(root: &Path, model_path: &[u8], texture_path: &[u8]) {
        fs::create_dir(root.join("Data")).unwrap();
        let payload = [
            1u32.to_le_bytes().as_slice(),
            &(texture_path.len() as u32).to_le_bytes(),
            texture_path,
        ]
        .concat();
        let kind = b"BSShaderTextureSet";
        let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        nif.extend(0x14020007u32.to_le_bytes());
        nif.push(1);
        for value in [11u32, 1, 34] {
            nif.extend(value.to_le_bytes());
        }
        nif.extend([0; 3]);
        nif.extend(1u16.to_le_bytes());
        nif.extend((kind.len() as u32).to_le_bytes());
        nif.extend(kind);
        nif.extend(0u16.to_le_bytes());
        nif.extend((payload.len() as u32).to_le_bytes());
        nif.extend([0; 12]);
        nif.extend(payload);
        nif.extend([0; 4]);
        archive(root, "models", b"meshes", b"m.nif", &nif);
        archive(
            root,
            "textures",
            b"textures",
            b"t.dds",
            b"authored-source-texture",
        );
        let reference = record(
            b"REFR",
            0x300,
            &[
                field(b"NAME", &0x400u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        );
        let group = |kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &0x200u32.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        fs::write(
            root.join("Data/Base.esm"),
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
                    b"STAT",
                    0x400,
                    &field(b"MODL", &[model_path, &[0]].concat()),
                ),
                record(b"CELL", 0x200, &field(b"DATA", &[1])),
                group(6, &group(9, &reference)),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
    }
    fn residency_input() -> ResidencyInput {
        ResidencyInput {
            cell: crate::parse_cell_key("Base.esm:200").unwrap(),
            source_timeout_ms: 10_000,
        }
    }

    fn patch_pair_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let layer = |tag: &[u8; 4], quad: u8, index: i16| {
            field(
                tag,
                &[
                    if mode == "default-material" {
                        0_u32
                    } else {
                        0x400_u32
                    }
                    .to_le_bytes()
                    .as_slice(),
                    &[quad, 77],
                    &index.to_le_bytes(),
                ]
                .concat(),
            )
        };
        let mut cells = Vec::new();
        for i in 0..2_u32 {
            let hidden = if mode == "hide" && i == 1 { 16 } else { 1 << i };
            let x = if mode == "noncardinal" && i == 1 {
                2_i32
            } else {
                i as i32
            };
            let grid = [
                x.to_le_bytes(),
                0_i32.to_le_bytes(),
                (0xaabbcc00_u32 | hidden).to_le_bytes(),
            ]
            .concat();
            let cell = record(
                b"CELL",
                0x200 + i,
                &[field(b"DATA", &[0]), field(b"XCLC", &grid)].concat(),
            );
            let mut land = field(b"DATA", &[7, 0, 0, 0]);
            let mut normals = [255_u8, 0, 0].repeat(1089);
            normals[3..6].copy_from_slice(&[0, 0, 127]);
            if i == 1 && mode == "zero-normal" {
                normals[..3].fill(0);
            }
            if i == 1 && mode == "short-normal" {
                normals.pop();
            }
            land.extend(field(b"VNML", &normals));
            let mut heights = if i == 0 {
                0x3fa00000_u32
            } else {
                0x40100000_u32
            }
            .to_le_bytes()
            .to_vec();
            let mut deltas = vec![0; 1089];
            deltas[..3].copy_from_slice(&[2, 3, 255]);
            deltas[33..35].copy_from_slice(&[252, 5]);
            heights.extend(deltas);
            heights.extend([7, 11, 13]);
            if !(i == 1 && mode == "missing-height") {
                land.extend(field(b"VHGT", &heights));
            }
            let colors: Vec<u8> = (0..1089)
                .flat_map(|j| {
                    [
                        (j % 256) as u8,
                        ((3 * j) % 256) as u8,
                        (255 - j % 256) as u8,
                    ]
                })
                .collect();
            land.extend(field(b"VCLR", &colors));
            for q in 0..4 {
                land.extend(layer(b"BTXT", q, -1));
            }
            land.extend(layer(
                b"ATXT",
                0,
                if i == 1 && mode == "layer-order" {
                    1
                } else {
                    0
                },
            ));
            let alpha: Vec<u8> = [
                (0_u16, 0x3f000000_u32),
                (1, 0x3fa00000),
                (2, 0xbe800000),
                (288, 0x3e800000),
            ]
            .into_iter()
            .flat_map(|(at, bits)| {
                [at.to_le_bytes().as_slice(), &[13, 29], &bits.to_le_bytes()].concat()
            })
            .collect();
            land.extend(field(b"VTXT", &alpha));
            land.extend(layer(b"ATXT", 0, 1));
            land.extend(field(
                b"VTXT",
                &[
                    0_u16.to_le_bytes().as_slice(),
                    &[17, 19],
                    &0x3f400000_u32.to_le_bytes(),
                ]
                .concat(),
            ));
            let mut lands = record(b"LAND", 0x300 + i, &land);
            if i == 1 && mode == "missing-land" {
                lands.clear();
            }
            if i == 1 && mode == "multiple-land" {
                lands.extend(record(b"LAND", 0x399, &land));
            }
            if i == 1 && mode == "deleted-land" {
                lands[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
            }
            cells.extend([cell, group(0x200 + i, 6, &group(0x200 + i, 9, &lands))].concat());
        }
        let header = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let texture = if mode == "missing-material" {
            b"missing.dds\0".as_slice()
        } else {
            b"land/a.dds\0"
        };
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(b"TES4", 0, &header),
                record(b"WRLD", 0x100, &field(b"DATA", &[0])),
                group(0x100, 1, &cells),
                record(b"LTEX", 0x400, &field(b"TNAM", &0x500_u32.to_le_bytes())),
                record(b"TXST", 0x500, &field(b"TX00", texture)),
            ]
            .concat(),
        )
        .unwrap();
        archive(root, "terrain", b"textures\\land", b"a.dds", &[31, 47, 63]);
        if mode == "ambiguous-material" {
            archive(
                root,
                "other-terrain",
                b"textures\\land",
                b"a.dds",
                &[79, 83],
            );
        }
        fs::write(root.join("order.json"), r#"["Base.esm"]"#).unwrap();
        fs::write(
            root.join("terrain-patch-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_TERRAIN_PATCH_FIXTURE={}", root.display());
    }
    fn patch_input(grids: Vec<[i32; 2]>, seams: Vec<[usize; 2]>) -> GridPatchInput {
        GridPatchInput {
            world: crate::parse_cell_key("Base.esm:100").unwrap(),
            grids,
            seams,
        }
    }
    #[test]
    fn cli_patch_pair_consumes_literal_geometry_weights_source_spans_and_actual_hashes() {
        let root = directory();
        patch_pair_fixture(&root, "valid");
        let before = Sha256::digest(fs::read(root.join("Data/Base.esm")).unwrap());
        let report = terrain_patches(
            &root,
            &root.join("order.json"),
            None,
            patch_input(vec![[0, 0], [1, 0]], vec![[0, 1]]),
        )
        .unwrap();
        assert_eq!(report["cpu_bundle_prepared"], true);
        let bundle = &report["terrain_patch_bundle"];
        assert_eq!(bundle["usage"]["vertices"], 2178);
        assert_eq!(bundle["usage"]["indices"], 9216);
        assert_eq!(bundle["usage"]["weights"], 3468);
        assert_eq!(bundle["usage"]["source_read_bytes"], 15706);
        let first = &bundle["patches"][0];
        let second = &bundle["patches"][1];
        assert_eq!(
            first["source_plan"]["terrain"]["cell"]["header"]["offset"],
            97
        );
        assert_eq!(
            first["source_plan"]["terrain"]["landscapes"][0]["header"]["offset"],
            194
        );
        assert_eq!(first["mesh"]["local_positions"][0], json!([0., 0., 26.]));
        assert_eq!(first["mesh"]["local_positions"][1], json!([128., 0., 50.]));
        assert_eq!(first["mesh"]["local_positions"][33], json!([0., 128., -6.]));
        assert_eq!(
            first["mesh"]["indices"].as_array().unwrap()[..6],
            json!([16, 17, 50, 16, 50, 49]).as_array().unwrap()[..]
        );
        assert_eq!(second["mesh"]["local_positions"][0], json!([0., 0., 34.]));
        assert_eq!(
            first["mesh"]["normal_bits"][0],
            json!([0xbf800000_u32, 0, 0])
        );
        assert_eq!(first["mesh"]["colors"][1088], json!([64, 192, 191]));
        assert_eq!(
            first["blends"]["quadrants"][0]["overlays"][0]["weights"][0],
            127
        );
        assert_eq!(
            first["blends"]["quadrants"][0]["overlays"][1]["weights"][0],
            191
        );
        assert_eq!(first["blends"]["quadrants"][0]["base"]["weights"][288], 192);
        assert_eq!(
            bundle["seams"][0]["comparison"]["mismatches"]
                .as_array()
                .unwrap()
                .len(),
            33
        );
        for (index, patch) in bundle["patches"].as_array().unwrap().iter().enumerate() {
            for (object, key) in [
                ("surface", "surface_sha256"),
                ("mesh", "geometry_sha256"),
                ("blends", "blend_sha256"),
            ] {
                // JSON Value object order differs from the typed serialization; compare an independent
                // fresh factory's borrowed typed outputs below instead of treating a report as authority.
                assert!(patch[object].is_object());
                assert_eq!(
                    report["patch_hashes"][index][key].as_str().unwrap().len(),
                    64
                );
            }
        }
        let order = Order::read(&root.join("order.json")).unwrap();
        let mut store = order.store(&root, None).unwrap();
        let dir = CellGridSources::load(
            &mut store,
            &crate::parse_cell_key("Base.esm:100").unwrap(),
            Default::default(),
        )
        .unwrap();
        let requests = [dir.request([0, 0]).unwrap(), dir.request([1, 0]).unwrap()];
        let assets = fallout_data::assets::ArchiveAssets::open_nv(&root).unwrap();
        let actual = fallout_data::terrain::patches::TerrainPatchBundle::load(
            &dir,
            &mut store,
            &requests,
            assets.mounts(),
            &[[0, 1]],
            Default::default(),
        )
        .unwrap();
        for (i, patch) in actual.patches().iter().enumerate() {
            for (key, bytes) in [
                (
                    "surface_sha256",
                    serde_json::to_vec(&patch.surface).unwrap(),
                ),
                ("geometry_sha256", serde_json::to_vec(&patch.mesh).unwrap()),
                ("blend_sha256", serde_json::to_vec(&patch.blends).unwrap()),
            ] {
                assert_eq!(
                    report["patch_hashes"][i][key],
                    format!("{:x}", Sha256::digest(bytes))
                );
            }
        }
        assert_eq!(
            report["terrain_patch_bundle"]["identity"],
            actual.identity()
        );
        assert_eq!(
            Sha256::digest(fs::read(root.join("Data/Base.esm")).unwrap()),
            before
        );
        for flag in [
            "source_materials_prepared",
            "texture_images_decoded",
            "seams_repaired",
            "rendering_admitted",
            "collision_admitted",
            "current_cell_changed",
            "activation_applied",
            "runtime_ready",
            "lookup_precedence_verified",
            "retail_parity_accepted",
        ] {
            assert_eq!(report[flag], false);
        }
    }
    #[test]
    fn cli_patch_refusals_publish_no_partial_bundle_and_material_coverage_stays_separate() {
        for mode in [
            "missing-land",
            "multiple-land",
            "deleted-land",
            "missing-height",
            "short-normal",
            "zero-normal",
            "hide",
            "layer-order",
            "noncardinal",
        ] {
            let root = directory();
            patch_pair_fixture(&root, mode);
            let grids = if mode == "noncardinal" {
                vec![[0, 0], [2, 0]]
            } else {
                vec![[0, 0], [1, 0]]
            };
            let report = terrain_patches(
                &root,
                &root.join("order.json"),
                None,
                patch_input(grids, vec![[0, 1]]),
            )
            .unwrap();
            assert_eq!(report["cpu_bundle_prepared"], false, "{mode}");
            assert!(report["terrain_patch_bundle"].is_null(), "{mode}");
            assert_eq!(report["patch_hashes"], json!([]), "{mode}");
            assert!(report["source_error"].is_string(), "{mode}");
        }
        let root = directory();
        patch_pair_fixture(&root, "valid");
        for (grids, seams) in [
            (vec![[0, 0], [0, 0]], vec![]),
            (vec![[2, 0]], vec![]),
            (vec![[0, 0], [1, 0]], vec![[0, 2]]),
        ] {
            let report = terrain_patches(
                &root,
                &root.join("order.json"),
                None,
                patch_input(grids, seams),
            )
            .unwrap();
            assert_eq!(report["cpu_bundle_prepared"], false);
            assert!(report["terrain_patch_bundle"].is_null());
        }
        for mode in ["missing-material", "ambiguous-material", "default-material"] {
            let root = directory();
            patch_pair_fixture(&root, mode);
            let report = terrain_patches(
                &root,
                &root.join("order.json"),
                None,
                patch_input(vec![[0, 0], [1, 0]], vec![]),
            )
            .unwrap();
            assert_eq!(report["cpu_bundle_prepared"], true, "{mode}");
            assert_eq!(report["source_materials_prepared"], false);
            assert_eq!(report["texture_images_decoded"], false);
            assert_eq!(report["runtime_ready"], false);
        }
    }
    #[test]
    fn cli_patch_grid_and_seam_flags_preserve_explicit_order_and_bounds() {
        use clap::Parser;
        let args = [
            "fallout-cli",
            "terrain-patch-sources",
            "--install",
            "absent",
            "--load-order",
            "absent",
            "--world",
            "Base.esm:100",
        ];
        assert!(crate::Args::try_parse_from(args).is_err());
        let parsed = crate::Args::try_parse_from(args.into_iter().chain([
            "--grid=-18,0",
            "--grid=1,0",
            "--seam=1,0",
            "--seam=0,1",
        ]))
        .unwrap();
        match parsed.command {
            crate::Command::TerrainPatchSources { grid, seam, .. } => {
                assert_eq!(parse_grid_set(&grid).unwrap(), [[-18, 0], [1, 0]]);
                assert_eq!(parse_seam_set(&seam).unwrap(), [[1, 0], [0, 1]]);
            }
            _ => panic!("terrain patch command changed"),
        }
        for value in ["0", "0,1,2", "-1,0", ",", "18446744073709551616,0"] {
            assert!(parse_seam_set(&[value.into()]).is_err());
        }
        assert!(parse_seam_set(&vec!["0,1".into(); 9]).is_err());
        assert!(parse_seam_set(&[]).unwrap().is_empty());
        for (grids, seams) in [
            (vec![], vec![]),
            (vec![[0, 0]; 9], vec![]),
            (vec![[0, 0]], vec![[0, 1]; 9]),
        ] {
            assert!(
                terrain_patches(
                    Path::new("absent"),
                    Path::new("absent"),
                    None,
                    patch_input(grids, seams)
                )
                .is_err()
            );
        }
    }

    fn conversation_pages_fixture(root: &Path) {
        fs::create_dir(root.join("Data")).unwrap();
        let group = |topic: u32, data: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(data.len() as u32 + 24).to_le_bytes(),
                &topic.to_le_bytes(),
                &7_i32.to_le_bytes(),
                &[0; 8],
                data,
            ]
            .concat()
        };
        let header = |master: bool| {
            let mut body = field(
                b"HEDR",
                &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            );
            if master {
                body.extend(field(b"MAST", b"Base.esm\0"));
                body.extend(field(b"DATA", &[0; 8]));
            }
            record(b"TES4", 0, &body)
        };
        let info = |text: &[u8]| {
            let mut trdt = [0; 24];
            trdt[12] = 9;
            [
                field(b"DATA", &[0, 7, 0, 0]),
                field(b"TPIC", &0x100_u32.to_le_bytes()),
                field(b"TRDT", &trdt),
                field(b"NAM1", text),
            ]
            .concat()
        };
        let mut deleted = record(b"INFO", 0x301, &[]);
        deleted[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        fs::write(
            root.join("Data/Base.esm"),
            [
                header(false),
                record(b"DIAL", 0x100, &field(b"DATA", &[0])),
                record(b"DIAL", 0x200, &[]),
                record(b"NPC_", 0x400, &[]),
                group(
                    0x100,
                    &[
                        record(b"INFO", 0x303, b"NAM1\x05\0x"),
                        record(b"INFO", 0x302, &info(b"moved\0")),
                        record(b"INFO", 0x301, &info(b"deleted\0")),
                        record(b"INFO", 0x300, &info(b"old\0")),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(
            root.join("Data/Patch.esp"),
            [
                header(true),
                group(
                    0x100,
                    &[
                        record(b"INFO", 0x01000700, &info(b"own\0")),
                        deleted,
                        record(b"INFO", 0x300, &info(&[255, 128, 65, 0])),
                    ]
                    .concat(),
                ),
                group(0x200, &record(b"INFO", 0x302, &[])),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), r#"["Base.esm","Patch.esp"]"#).unwrap();
        fs::write(
            root.join("conversation-pages-case.json"),
            b"{\"mode\":\"literal-pages\"}",
        )
        .unwrap();
        eprintln!("WORLD_CONVERSATION_PAGES_FIXTURE {}", root.display());
    }
    fn pages_input(
        topic: &str,
        info: Option<&str>,
        size: usize,
        count: usize,
    ) -> ConversationPagesInput {
        ConversationPagesInput {
            topic: crate::parse_cell_key(topic).unwrap(),
            info: info.map(crate::parse_cell_key).transpose().unwrap(),
            speaker: Some(crate::parse_cell_key("Base.esm:400").unwrap()),
            page_size: size,
            max_pages: count,
        }
    }
    #[test]
    fn pages_cli_consumes_one_literal_override_from_three_canonical_pages() {
        let root = directory();
        conversation_pages_fixture(&root);
        let report = conversation_pages(
            &root,
            &root.join("order.json"),
            None,
            pages_input("Base.esm:100", Some("Base.esm:300"), 1, 3),
        )
        .unwrap();
        assert_eq!(report["pages_prepared"], true);
        assert_eq!(report["selected_request_prepared"], true);
        assert_eq!(report["selected_page"], 0);
        assert_eq!(report["selected_member"], 0);
        assert!(report["source_error"].is_null());
        let pages = report["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages
                .iter()
                .map(|p| p["requests"][0]["info"]["local_id"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0x300, 0x303, 0x700]
        );
        assert_eq!(
            pages[2]["requests"][0]["info"]["origin_plugin"],
            "patch.esp"
        );
        assert_eq!(pages[0]["usage"]["copied_bytes"], 270);
        assert_eq!(pages[1]["usage"]["copied_bytes"], 270);
        assert_eq!(pages[2]["usage"]["copied_bytes"], 168);
        assert_eq!(report["conversation"]["info"]["source_plugin"], "Patch.esp");
        assert_eq!(report["conversation"]["info"]["record_file_offset"], 203);
        assert_eq!(report["subtitle_payloads"][0]["bytes"], 4);
        assert_eq!(
            report["subtitle_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest([255, 128, 65, 0]))
        );
        assert_eq!(report["conversation"]["responses"][0]["number"], 9);
        assert_eq!(
            report["conversation"]["info_fields"][3]["header_decoded_offset"],
            50
        );
        for page in pages {
            assert!(page.get("cursor").is_none());
            assert_eq!(page["payloads_prepared"], false);
            assert_eq!(page["original_order_verified"], false);
        }
        for flag in [
            "original_menu_order_verified",
            "condition_truth_verified",
            "speaker_assignment_verified",
            "voice_filename_verified",
            "fragment_execution_admitted",
            "runtime_ready",
            "retail_parity_accepted",
        ] {
            assert_eq!(report[flag], false, "{flag}");
        }
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("moved\\u0000") && !json.contains("old\\u0000"));
    }
    #[test]
    fn pages_cli_refuses_live_member_outside_returned_window_and_missing_member() {
        let root = directory();
        conversation_pages_fixture(&root);
        for info in [
            "Patch.esp:700",
            "Base.esm:999",
            "Base.esm:301",
            "Base.esm:302",
        ] {
            let report = conversation_pages(
                &root,
                &root.join("order.json"),
                None,
                pages_input("Base.esm:100", Some(info), 1, 1),
            )
            .unwrap();
            assert_eq!(report["pages_prepared"], true);
            assert_eq!(report["pages"].as_array().unwrap().len(), 1);
            assert_eq!(report["selected_request_prepared"], false);
            assert!(report["conversation"].is_null());
            assert!(report["subtitle_payloads"].as_array().unwrap().is_empty());
            assert!(
                report["source_error"]
                    .as_str()
                    .unwrap()
                    .contains("not returned")
            );
        }
    }
    #[test]
    fn pages_cli_distinguishes_absent_structural_topic_from_failed_explicit_choice() {
        let root = directory();
        conversation_pages_fixture(&root);
        let report = conversation_pages(
            &root,
            &root.join("order.json"),
            None,
            pages_input("Base.esm:999", None, 1, 16),
        )
        .unwrap();
        assert_eq!(report["membership_state"], "empty_structural_membership");
        assert_eq!(report["pages_prepared"], true);
        assert!(
            report["pages"][0]["requests"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(report["pages"].as_array().unwrap().len(), 1);
        assert!(report["source_error"].is_null() && report["conversation"].is_null());
        assert_eq!(report["selected_request_prepared"], false);
        let failed = conversation_pages(
            &root,
            &root.join("order.json"),
            None,
            pages_input("Base.esm:999", Some("Base.esm:300"), 1, 1),
        )
        .unwrap();
        assert_eq!(failed["pages_prepared"], true);
        assert_eq!(failed["membership_state"], "empty_structural_membership");
        assert!(failed["source_error"].is_string() && failed["conversation"].is_null());
    }
    #[test]
    fn pages_cli_keeps_header_pages_and_refuses_selected_bad_body_or_actor() {
        let root = directory();
        conversation_pages_fixture(&root);
        let mut bad_actor = pages_input("Base.esm:100", Some("Base.esm:300"), 1, 1);
        bad_actor.speaker = Some(crate::parse_cell_key("Base.esm:999").unwrap());
        for input in [
            pages_input("Base.esm:100", Some("Base.esm:303"), 2, 1),
            bad_actor,
        ] {
            let report = conversation_pages(&root, &root.join("order.json"), None, input).unwrap();
            assert_eq!(report["pages_prepared"], true);
            assert_eq!(report["selected_request_prepared"], false);
            assert!(report["source_error"].is_string());
            assert!(report["conversation"].is_null());
            assert!(report["subtitle_payloads"].as_array().unwrap().is_empty());
        }
        let structural = conversation_pages(
            &root,
            &root.join("order.json"),
            None,
            pages_input("Base.esm:100", None, 3, 1),
        )
        .unwrap();
        assert!(structural["source_error"].is_null());
        assert_eq!(
            structural["pages"][0]["requests"].as_array().unwrap().len(),
            3
        );
        assert!(structural["conversation"].is_null());
    }
    #[test]
    fn pages_cli_bounds_refuse_before_source_access() {
        for (size, count) in [(0, 1), (257, 1), (1, 0), (1, 17), (usize::MAX, usize::MAX)] {
            assert!(
                conversation_pages(
                    Path::new("absent"),
                    Path::new("absent"),
                    None,
                    pages_input("Base.esm:100", None, size, count)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn pages_command_parses_explicit_member_and_no_serialized_cursor_authority() {
        use clap::Parser;
        let args = [
            "fallout",
            "conversation-pages-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--topic",
            "Base.esm:100",
            "--info",
            "Base.esm:300",
            "--page-size",
            "1",
            "--max-pages",
            "3",
        ];
        let cli = crate::Args::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            crate::Command::ConversationPagesSources {
                page_size: 1,
                max_pages: 3,
                info: Some(_),
                ..
            }
        ));
        assert!(
            crate::Args::try_parse_from(args.into_iter().chain(["--cursor", "forged"])).is_err()
        );
    }

    fn conversation_batch_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let group = |topic: u32, data: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(data.len() as u32 + 24).to_le_bytes(),
                &topic.to_le_bytes(),
                &7_i32.to_le_bytes(),
                &[0; 8],
                data,
            ]
            .concat()
        };
        let script = || {
            let mut schr = [0; 20];
            schr[..4].copy_from_slice(&[61, 67, 71, 73]);
            schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
            [field(b"SCHR", &schr), field(b"SCDA", &[0x1d, 0, 0, 0])].concat()
        };
        let body = |second: bool| {
            let mut ct = [0; 28];
            ct[..4].copy_from_slice(&[0x21, 13, 17, 19]);
            ct[4..8].copy_from_slice(&0x80000000_u32.to_le_bytes());
            ct[8..10].copy_from_slice(&(if second { 65534_u16 } else { 65535 }).to_le_bytes());
            ct[10..12].copy_from_slice(&[23, 29]);
            ct[12..16].copy_from_slice(&0x11112222_u32.to_le_bytes());
            ct[16..20].copy_from_slice(&0x33334444_u32.to_le_bytes());
            ct[20..24].copy_from_slice(&0xfffffff0_u32.to_le_bytes());
            ct[24..].copy_from_slice(&0x400_u32.to_le_bytes());
            let mut trdt = [0; 24];
            trdt[..4].copy_from_slice(&7_u32.to_le_bytes());
            trdt[4..8].copy_from_slice(&(-3_i32).to_le_bytes());
            trdt[8..12].copy_from_slice(&[31, 37, 41, 43]);
            trdt[12] = if second { 2 } else { 9 };
            trdt[13..16].copy_from_slice(&[47, 53, 59]);
            let text = if second {
                [254, 129, 66, 0]
            } else {
                [255, 128, 65, 0]
            };
            let mut data = [
                field(b"DATA", &[0, 7, 0, 0]),
                field(b"TPIC", &0x100_u32.to_le_bytes()),
                field(b"CTDA", &ct),
                field(b"TRDT", &trdt),
                field(b"NAM1", &text),
                field(b"NAM1", b"repeat\0"),
                script(),
            ]
            .concat();
            if second && mode == "ambiguous-last" {
                data.extend(script());
            }
            data.extend(field(b"NEXT", &[]));
            data.extend(script());
            if second && mode == "truncated-last" {
                data.extend(b"NAM1\x08\0x");
            }
            data
        };
        let header = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let mut second = record(
            if mode == "wrong-kind-last" {
                b"DIAL"
            } else {
                b"INFO"
            },
            0x301,
            &body(true),
        );
        if mode == "deleted-last" {
            second[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        let first = record(b"INFO", 0x300, &body(false));
        let groups = if mode == "moved-last" {
            [group(0x100, &first), group(0x200, &second)].concat()
        } else {
            group(0x100, &[first, second].concat())
        };
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(b"TES4", 0, &header),
                record(
                    b"DIAL",
                    0x100,
                    &[
                        field(b"EDID", b"batch\0"),
                        field(b"FULL", &[255, 128, 84, 0]),
                        field(b"DATA", &[0]),
                        field(b"QSTI", &0x500_u32.to_le_bytes()),
                    ]
                    .concat(),
                ),
                record(b"NPC_", 0x400, &[]),
                record(b"QUST", 0x500, &[]),
                record(b"DIAL", 0x200, &field(b"DATA", &[0])),
                groups,
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), r#"["Base.esm"]"#).unwrap();
        if mode == "missing-master" {
            fs::write(
                root.join("Data/Patch.esp"),
                record(
                    b"TES4",
                    0,
                    &[
                        header,
                        field(b"MAST", b"Missing.esm\0"),
                        field(b"DATA", &[0; 8]),
                    ]
                    .concat(),
                ),
            )
            .unwrap();
            fs::write(root.join("order.json"), r#"["Base.esm","Patch.esp"]"#).unwrap();
        }
        let second = json!({"topic":if mode=="bad-last-parent" {"Base.esm:200"} else {"Base.esm:100"},
            "info":if mode=="invalid-key-last" {"not-canonical"} else {"Base.esm:301"},
            "speaker":if mode=="bad-last-speaker" {Some("Base.esm:500")} else {None}});
        fs::write(
            root.join("requests.json"),
            serde_json::to_vec(&json!({"schema_version":1,"requests":[
            {"topic":"Base.esm:100","info":"Base.esm:300","speaker":"Base.esm:400"},second]}))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("conversation-batch-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_CONVERSATION_BATCH_FIXTURE={}", root.display());
    }
    #[test]
    fn cli_conversation_batch_consumes_exact_retained_subtitles_and_real_fragment_sources() {
        let root = directory();
        conversation_batch_fixture(&root, "valid");
        let files = ["Data/Base.esm", "order.json", "requests.json"];
        let hashes = files.map(|path| Sha256::digest(fs::read(root.join(path)).unwrap()));
        for bind in [false, true] {
            let report = conversation_batch(
                &root,
                &root.join("order.json"),
                None,
                &root.join("requests.json"),
                bind,
            )
            .unwrap();
            assert_eq!(report["batch_request_prepared"], true);
            assert!(report["source_error"].is_null());
            assert_eq!(report["fragment_bindings_available"], bind);
            let batch = &report["conversation_batch"];
            assert_eq!(batch["receipt"]["usage"]["read_bytes"], 1266);
            assert_eq!(batch["receipt"]["usage"]["raw_bytes"], 448);
            assert_eq!(batch["receipt"]["usage"]["conditions"], 2);
            assert_eq!(batch["conversations"][0]["topic"]["record_file_offset"], 42);
            assert_eq!(batch["conversations"][0]["info"]["record_file_offset"], 208);
            assert_eq!(batch["conversations"][1]["info"]["record_file_offset"], 417);
            assert_eq!(
                batch["conversations"][0]["info_fields"][4]["header_decoded_offset"],
                84
            );
            assert_eq!(
                batch["conversations"][1]["fragments"][1]["key"]["header_decoded_offset"],
                149
            );
            assert_eq!(report["subtitle_payloads"].as_array().unwrap().len(), 4);
            let payloads = &report["subtitle_payloads"];
            for (index, bytes) in [(0, [255, 128, 65, 0]), (2, [254, 129, 66, 0])] {
                assert_eq!(payloads[index]["request"], index / 2);
                assert_eq!(payloads[index]["occurrence"], 0);
                assert_eq!(payloads[index]["info_field"], 4);
                assert_eq!(payloads[index]["bytes"], 4);
                assert_eq!(
                    payloads[index]["sha256"],
                    format!("{:x}", Sha256::digest(bytes))
                );
            }
            for index in [1, 3] {
                assert_eq!(payloads[index]["occurrence"], 1);
                assert_eq!(payloads[index]["info_field"], 5);
                assert_eq!(
                    payloads[index]["sha256"],
                    format!("{:x}", Sha256::digest(b"repeat\0"))
                );
            }
            assert_eq!(
                report["loaded_fragments"].as_array().unwrap().len(),
                if bind { 4 } else { 0 }
            );
            if bind {
                for (index, fragment) in report["loaded_fragments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    assert_eq!(fragment["request"], index / 2);
                    assert_eq!(fragment["fragment"], index % 2);
                    assert_eq!(
                        fragment["owner"]["kind"],
                        if index % 2 == 0 {
                            "dialogue_begin"
                        } else {
                            "dialogue_end"
                        }
                    );
                    assert_eq!(
                        fragment["version"]["record_file_offset"],
                        if index / 2 == 0 { 208 } else { 417 }
                    );
                    assert_eq!(fragment["version"]["compiled_bytes"], 4);
                    assert_eq!(fragment["execution_admitted"], false);
                }
            }
            for flag in [
                "condition_signatures_supplied",
                "selection_order_verified",
                "condition_truth_verified",
                "speaker_assignment_verified",
                "voice_filename_verified",
                "fragment_timing_verified",
                "fragment_execution_admitted",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[flag], false);
            }
        }
        for (path, expected) in files.into_iter().zip(hashes) {
            assert_eq!(Sha256::digest(fs::read(root.join(path)).unwrap()), expected);
        }
    }
    #[test]
    fn cli_conversation_batch_last_failure_leaks_no_prepared_prefix_or_fragment_payloads() {
        for mode in [
            "bad-last-parent",
            "bad-last-speaker",
            "ambiguous-last",
            "truncated-last",
            "moved-last",
            "deleted-last",
            "wrong-kind-last",
            "invalid-key-last",
        ] {
            let root = directory();
            conversation_batch_fixture(&root, mode);
            let report = conversation_batch(
                &root,
                &root.join("order.json"),
                None,
                &root.join("requests.json"),
                true,
            )
            .unwrap();
            assert_eq!(report["batch_request_prepared"], false, "{mode}");
            assert!(report["conversation_batch"].is_null());
            assert_eq!(report["subtitle_payloads"], json!([]));
            assert_eq!(report["loaded_fragments"], json!([]));
            assert_eq!(report["fragment_bindings_available"], false);
            assert!(report["source_error"].is_string());
        }
        let root = directory();
        conversation_batch_fixture(&root, "missing-master");
        assert!(
            conversation_batch(
                &root,
                &root.join("order.json"),
                None,
                &root.join("requests.json"),
                true
            )
            .is_err()
        );
        let root = directory();
        conversation_batch_fixture(&root, "valid");
        let request = json!({"topic":"Base.esm:100","info":"Base.esm:300"});
        fs::write(
            root.join("requests.json"),
            serde_json::to_vec(&json!({"schema_version":1,"requests":[request,request]})).unwrap(),
        )
        .unwrap();
        let report = conversation_batch(
            &root,
            &root.join("order.json"),
            None,
            &root.join("requests.json"),
            true,
        )
        .unwrap();
        assert_eq!(report["batch_request_prepared"], false);
        assert!(report["conversation_batch"].is_null());
        assert_eq!(report["subtitle_payloads"], json!([]));
    }
    #[test]
    fn cli_conversation_batch_closed_input_refuses_bad_nested_and_top_level_fields_before_source_access()
     {
        let root = directory();
        let input = root.join("bad-requests.json");
        let valid =
            json!({"schema_version":1,"requests":[{"topic":"Base.esm:100","info":"Base.esm:300"}]});
        let mut bad_top = valid.clone();
        bad_top["runtime_ready"] = json!(true);
        let mut bad_nested = valid.clone();
        bad_nested["requests"][0]["source_cohort_sha256"] = json!("forged");
        let mut bad_version = valid.clone();
        bad_version["schema_version"] = json!(2);
        let mut bad_type = valid.clone();
        bad_type["requests"][0]["info"] = json!(300);
        for value in [
            bad_top,
            bad_nested,
            bad_version,
            bad_type,
            json!({"schema_version":1,"requests":[]}),
            json!({"schema_version":1,"requests":vec![valid["requests"][0].clone();9]}),
        ] {
            fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                conversation_batch(
                    Path::new("absent"),
                    Path::new("absent"),
                    None,
                    &input,
                    false
                )
                .is_err()
            );
        }
        for bytes in [b"{".as_slice(),br#"{"schema_version":1,"schema_version":1,"requests":[]}"#,
            br#"{"schema_version":1,"requests":[{"topic":"Base.esm:100","info":"Base.esm:300","info":"Base.esm:301"}]}"#] {
            fs::write(&input,bytes).unwrap();
            assert!(conversation_batch(Path::new("absent"),Path::new("absent"),None,&input,false).is_err());
        }
        fs::write(&input, vec![b' '; 65537]).unwrap();
        assert!(
            conversation_batch(
                Path::new("absent"),
                Path::new("absent"),
                None,
                &input,
                false
            )
            .is_err()
        );
    }
    #[test]
    fn cli_conversation_batch_requires_input_flag_and_preserves_the_existing_single_command() {
        use clap::Parser;
        let args = [
            "fallout-cli",
            "conversation-batch-sources",
            "--install",
            "absent",
            "--load-order",
            "absent",
        ];
        assert!(crate::Args::try_parse_from(args).is_err());
        let parsed = crate::Args::try_parse_from(args.into_iter().chain([
            "--requests",
            "r.json",
            "--bind-result-fragments",
        ]))
        .unwrap();
        match parsed.command {
            crate::Command::ConversationBatchSources {
                requests,
                bind_result_fragments,
                ..
            } => {
                assert_eq!(requests, Path::new("r.json"));
                assert!(bind_result_fragments);
            }
            _ => panic!("conversation batch command changed"),
        }
        let root = directory();
        conversation_batch_fixture(&root, "valid");
        let single = conversation(
            &root,
            &root.join("order.json"),
            None,
            Input {
                topic: crate::parse_cell_key("Base.esm:100").unwrap(),
                info: crate::parse_cell_key("Base.esm:300").unwrap(),
                speaker: None,
                bind_result_fragments: true,
            },
        )
        .unwrap();
        assert_eq!(single["subtitle_payloads"].as_array().unwrap().len(), 2);
        assert_eq!(single["loaded_fragments"].as_array().unwrap().len(), 2);
        assert!(single.get("conversation_batch").is_none());
        assert_eq!(single["runtime_ready"], false);
    }

    fn grid_fixture(root: &Path, duplicate: bool) {
        cell_fixture(root, b"m.nif", b"t.dds");
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let grid = [-18_i32, 0]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        let cell = |id| {
            record(
                b"CELL",
                id,
                &[field(b"DATA", &[0]), field(b"XCLC", &grid)].concat(),
            )
        };
        let mut persistent = record(b"CELL", 0x202, &field(b"DATA", &[0]));
        persistent[8..12].copy_from_slice(&fallout_data::plugin::PERSISTENT.to_le_bytes());
        let mut children = [
            cell(0x200),
            group(
                0x200,
                6,
                &group(
                    0x200,
                    9,
                    &record(
                        b"REFR",
                        0x300,
                        &[
                            field(b"NAME", &0x400_u32.to_le_bytes()),
                            field(b"DATA", &[0; 24]),
                        ]
                        .concat(),
                    ),
                ),
            ),
            persistent,
        ]
        .concat();
        if duplicate {
            children.extend(cell(0x201));
        }
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"STAT", 0x400, &field(b"MODL", b"m.nif\0")),
                record(b"WRLD", 0x100, &[]),
                group(0x100, 1, &children),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(
            root.join("grid-case.json"),
            serde_json::to_vec(&json!({"duplicate":duplicate})).unwrap(),
        )
        .unwrap();
    }
    fn grid_input(grid: [i32; 2]) -> GridInput {
        GridInput {
            world: crate::parse_cell_key("Base.esm:100").unwrap(),
            grid,
            source_timeout_ms: 10_000,
        }
    }
    fn terrain_grid_fixture(root: &Path, mode: &str) {
        grid_fixture(root, false);
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let texture = if mode == "default" { 0_u32 } else { 0x600 };
        let land = record(
            b"LAND",
            0x500,
            &field(
                b"BTXT",
                &[texture.to_le_bytes().as_slice(), &[0; 4]].concat(),
            ),
        );
        let mut lands = land;
        if mode == "duplicate" {
            lands.extend(record(
                b"LAND",
                0x501,
                &field(
                    b"BTXT",
                    &[texture.to_le_bytes().as_slice(), &[0; 4]].concat(),
                ),
            ));
        }
        let path = root.join("Data/Base.esm");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend(group(0x100, 1, &group(0x200, 6, &group(0x200, 9, &lands))));
        bytes.extend(record(
            b"LTEX",
            0x600,
            &field(b"TNAM", &0x700_u32.to_le_bytes()),
        ));
        bytes.extend(record(
            b"TXST",
            0x700,
            &field(
                b"TX00",
                if mode == "missing" {
                    b"missing.dds\0"
                } else {
                    b"t.dds\0"
                },
            ),
        ));
        fs::write(path, bytes).unwrap();
        if mode == "ambiguous" {
            archive(
                root,
                "other-terrain",
                b"textures",
                b"t.dds",
                b"other-source",
            );
        }
        fs::write(
            root.join("terrain-grid-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn cli_terrain_grid_consumes_exact_existing_jobs_and_private_cache_without_surface_admission() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| Sha256::digest(fs::read(directory.join(path)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-terrain-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        for reused in [false, true] {
            let report = grid_terrain(
                &directory,
                &directory.join("order.json"),
                None,
                Some(&cache),
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
            assert_eq!(report["terrain_source_plan"]["root"]["local_id"], 0x200);
            assert_eq!(
                report["terrain_source_plan"]["source_cohort_sha256"],
                report["cell_grid_sources"]["source_cohort_sha256"]
            );
            assert_eq!(
                report["terrain_textures"]["textures"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                report["terrain_textures"]["textures"][0]["sha256"],
                format!("{:x}", Sha256::digest(b"authored-source-texture"))
            );
            assert_eq!(
                report["terrain_textures"]["textures"][0]["cache"]["reused"],
                reused
            );
            assert_eq!(report["captured_sources_available"], true);
            assert_eq!(report["terrain_job_usage"]["outstanding"], 0);
            assert_eq!(report["terrain_job_usage"]["decoded_bytes"], 0);
            assert_eq!(report["surface_prepared"], false);
            assert_eq!(report["current_cell_changed"], false);
            assert_eq!(report["activation_applied"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(report["terrain_textures"]["runtime_ready"], false);
        }
        for (path, sha) in paths.iter().zip(before) {
            assert_eq!(Sha256::digest(fs::read(directory.join(path)).unwrap()), sha);
        }
    }
    #[test]
    fn cli_combined_grid_consumes_one_cell_epoch_then_proves_unload_and_final_release() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|p| Sha256::digest(fs::read(directory.join(p)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-combined-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        for _ in 0..2 {
            let report = grid_cell_residency(
                &directory,
                &directory.join("order.json"),
                None,
                Some(&cache),
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], true);
            assert_eq!(report["terrain_scope_requested"], true);
            assert_eq!(
                report["cell_terrain"]["source_cohort_sha256"],
                report["cell_models"]["source_cohort_sha256"]
            );
            assert_eq!(
                report["cell_terrain"]["root"],
                report["cell_grid_request"]["cell"]
            );
            assert_eq!(report["residency"]["terrain_state"], "Decoded");
            assert_eq!(report["residency"]["completed_models"], 1);
            assert_eq!(report["residency"]["completed_textures"], 1);
            assert_eq!(report["residency"]["completed_terrain_textures"], 1);
            assert_eq!(report["residency"]["outstanding"], 3);
            let payload = &report["terrain_payloads"][0];
            assert_eq!(
                payload["sha256"],
                format!("{:x}", Sha256::digest(b"authored-source-texture"))
            );
            assert_eq!(payload["bytes"], 23);
            assert_eq!(payload["cell_identity"], report["residency"]["identity"]);
            assert_eq!(payload["generation"], report["residency"]["generation"]);
            for field in [
                "terrain_ticket_matches_cell",
                "old_ticket_rejected",
                "old_owner_access_rejected",
                "borrowed_model_access_rejected",
                "borrowed_texture_access_rejected",
                "borrowed_terrain_access_rejected",
                "terrain_lease_retained",
            ] {
                assert_eq!(report["retained_source_lifetime"][field], true, "{field}");
            }
            assert_eq!(report["residency_after_unload"]["stage"], "Unloading");
            for field in [
                "outstanding",
                "pinned_source_bytes",
                "retained_plans",
                "plan_metadata_bytes",
                "mapped_source_bytes",
            ] {
                assert_eq!(
                    report["residency_after_unload"][field], report["residency"][field],
                    "{field}"
                );
                assert_eq!(report["residency_after_release"][field], 0, "{field}");
            }
            assert_eq!(report["residency_after_release"]["stage"], "Unrequested");
            for field in ["dependencies", "collision", "behavior"] {
                assert_eq!(report["residency"][field], "Pending");
            }
            for field in ["simulation_ready", "render_published"] {
                assert_eq!(report["residency"][field], false);
            }
            for field in [
                "surface_prepared",
                "current_cell_changed",
                "activation_applied",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[field], false);
            }
        }
        for (path, sha) in paths.iter().zip(before) {
            assert_eq!(Sha256::digest(fs::read(directory.join(path)).unwrap()), sha);
        }
    }
    #[test]
    fn cli_combined_grid_keeps_source_refusals_and_no_job_selection_failures() {
        for mode in ["missing", "ambiguous", "default", "duplicate"] {
            let directory = directory();
            terrain_grid_fixture(&directory, mode);
            let report = grid_cell_residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false, "{mode}");
            assert_eq!(report["terrain_scope_requested"], true);
            assert_eq!(report["runtime_ready"], false);
            if mode == "duplicate" {
                assert!(report["residency"].is_null());
                assert!(report["cell_terrain"].is_null());
                assert!(
                    report["source_error"]
                        .as_str()
                        .unwrap()
                        .contains("exactly one winning present LAND")
                );
            } else {
                assert_eq!(report["residency"]["terrain_state"], "Unsupported");
                assert_eq!(report["residency"]["dependencies"], "Pending");
                assert_eq!(report["residency"]["simulation_ready"], false);
                assert_eq!(report["residency_after_release"]["stage"], "Unrequested");
                assert_eq!(report["residency_after_release"]["outstanding"], 0);
            }
        }
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let report = grid_cell_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([i32::MIN, i32::MAX]),
        )
        .unwrap();
        assert!(report["residency"].is_null());
        assert!(report["cell_grid_request"].is_null());
        for timeout in [0, 120001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_cell_residency(Path::new("absent"), Path::new("absent"), None, None, input)
                    .is_err()
            );
        }
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-residency-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-18",
            "--grid-y",
            "0",
            "--include-terrain",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::GridResidencySources {
                include_terrain: true,
                grid_x: -18,
                ..
            }
        ));
    }
    #[test]
    fn cli_terrain_grid_retains_missing_ambiguous_default_and_duplicate_land_refusals() {
        for mode in ["missing", "ambiguous", "default", "duplicate"] {
            let directory = directory();
            terrain_grid_fixture(&directory, mode);
            let report = grid_terrain(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input([-18, 0]),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false, "{mode}");
            assert_eq!(report["surface_prepared"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
            if mode == "duplicate" {
                assert!(report["terrain_source_plan"].is_null());
                assert!(report["terrain_textures"].is_null());
                assert!(
                    report["source_error"]
                        .as_str()
                        .unwrap()
                        .contains("exactly one winning present LAND")
                );
            } else {
                assert_eq!(
                    report["terrain_textures"]["all_requested_texture_sources_ready"],
                    true
                );
                assert_eq!(
                    report["terrain_textures"]["all_authored_texture_sources_resolved"],
                    false
                );
                let sources = &report["terrain_source_plan"]["texture_sources"];
                assert!(
                    sources["failures"].as_u64().unwrap() > 0
                        || sources["unapplied_default_layers"].as_u64().unwrap() > 0
                );
            }
        }
    }
    #[test]
    fn cli_terrain_grid_missing_selection_and_bad_deadline_never_start_jobs() {
        let directory = directory();
        terrain_grid_fixture(&directory, "valid");
        let report = grid_terrain(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([i32::MAX, i32::MIN]),
        )
        .unwrap();
        assert!(report["cell_grid_request"].is_null());
        assert!(report["terrain_source_plan"].is_null());
        assert!(report["terrain_textures"].is_null());
        assert_eq!(report["captured_sources_available"], false);
        assert!(report["source_error"].as_str().unwrap().contains("no live"));
        for timeout in [0, 120_001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_terrain(Path::new("absent"), Path::new("absent"), None, None, input).is_err()
            );
        }
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-terrain-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-2147483648",
            "--grid-y",
            "2147483647",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::GridTerrainSources {
                grid_x: i32::MIN,
                grid_y: i32::MAX,
                ..
            }
        ));
    }
    #[test]
    fn cli_grid_consumer_selects_explicit_cell_and_uses_existing_resident_sources() {
        let directory = directory();
        grid_fixture(&directory, false);
        let before = Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap());
        let report = grid_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            grid_input([-18, 0]),
        )
        .unwrap();
        assert_eq!(report["cell_grid_sources"]["world"]["local_id"], 0x100);
        assert_eq!(
            report["cell_grid_sources"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            report["cell_grid_sources"]["entries"][1]["role"],
            "persistent-group"
        );
        assert_eq!(report["explicit_grid"], json!([-18, 0]));
        assert_eq!(report["cell_grid_request"]["cell"]["local_id"], 0x200);
        assert_eq!(report["cell_models"]["root"]["local_id"], 0x200);
        assert_eq!(
            report["cell_models"]["source_cohort_sha256"],
            report["cell_grid_sources"]["source_cohort_sha256"]
        );
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["activation_applied"], false);
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap()),
            before
        );
    }
    #[test]
    fn cli_grid_missing_and_ambiguous_requests_preserve_directory_without_residency() {
        for (duplicate, grid, expected) in [
            (false, [i32::MAX, i32::MIN], "no live"),
            (true, [-18, 0], "ambiguous"),
        ] {
            let directory = directory();
            grid_fixture(&directory, duplicate);
            let report = grid_residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                grid_input(grid),
            )
            .unwrap();
            assert!(report["source_error"].as_str().unwrap().contains(expected));
            assert!(report["cell_grid_request"].is_null());
            assert!(report["cell_models"].is_null());
            assert!(report["residency"].is_null());
            assert_eq!(report["captured_sources_available"], false);
            assert_eq!(report["explicit_grid"], json!(grid));
            assert_eq!(report["activation_applied"], false);
            assert_eq!(report["current_cell_changed"], false);
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(
                report["cell_grid_sources"]["entries"]
                    .as_array()
                    .unwrap()
                    .len(),
                if duplicate { 3 } else { 2 }
            );
        }
    }
    #[test]
    fn cli_grid_flags_preserve_signed_values_and_refuse_out_of_domain_inputs() {
        use clap::Parser;
        let args = crate::Args::try_parse_from([
            "fallout",
            "grid-residency-sources",
            "--install",
            "fixture",
            "--load-order",
            "order.json",
            "--world",
            "Base.esm:100",
            "--grid-x",
            "-2147483648",
            "--grid-y",
            "2147483647",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::GridResidencySources {
                grid_x: i32::MIN,
                grid_y: i32::MAX,
                ..
            }
        ));
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "grid-residency-sources",
                "--install",
                "fixture",
                "--load-order",
                "order.json",
                "--world",
                "Base.esm:100",
                "--grid-x",
                "2147483648",
                "--grid-y",
                "0"
            ])
            .is_err()
        );
        for timeout in [0, 120_001] {
            let mut input = grid_input([-18, 0]);
            input.source_timeout_ms = timeout;
            assert!(
                grid_residency(Path::new("absent"), Path::new("absent"), None, None, input)
                    .is_err()
            );
        }
    }
    fn door_fixture(root: &Path, target: u32) {
        cell_fixture(root, b"m.nif", b"t.dds");
        let group = |cell: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &cell.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let reference = |id: u32, teleport: u32, x: f32| {
            let pose = [x, 2.0, 3.0, 0.0, -0.0, -0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            record(
                b"REFR",
                id,
                &[
                    field(b"NAME", &0x401_u32.to_le_bytes()),
                    field(b"DATA", &[0; 24]),
                    field(
                        b"XTEL",
                        &[
                            teleport.to_le_bytes().as_slice(),
                            &pose,
                            &7_u32.to_le_bytes(),
                        ]
                        .concat(),
                    ),
                ]
                .concat(),
            )
        };
        let path = root.join("Data/Base.esm");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend(record(b"DOOR", 0x401, &field(b"MODL", b"m.nif\0")));
        bytes.extend(group(
            0x200,
            6,
            &group(0x200, 9, &reference(0x310, target, 42.0)),
        ));
        bytes.extend(record(b"CELL", 0x201, &field(b"DATA", &[1])));
        bytes.extend(group(
            0x201,
            6,
            &group(0x201, 9, &reference(0x311, 0x310, 99.0)),
        ));
        fs::write(path, bytes).unwrap();
    }
    fn door_input() -> DoorInput {
        DoorInput {
            source: residency_input(),
            door: crate::parse_cell_key("Base.esm:310").unwrap(),
        }
    }
    #[test]
    fn cli_door_destination_prepares_exact_target_sources_without_moving_current_cell() {
        let directory = directory();
        door_fixture(&directory, 0x311);
        let source_before = Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap());
        let report = door_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            door_input(),
        )
        .unwrap();
        assert_eq!(report["door_destination"]["source_cell"]["local_id"], 0x200);
        assert_eq!(
            report["door_destination"]["destination"]["cell"]["local_id"],
            0x201
        );
        assert_eq!(report["residency"]["root"]["local_id"], 0x201);
        assert_eq!(report["cell_models"]["root"]["local_id"], 0x201);
        assert_eq!(
            report["door_destination"]["destination"]["authored_transform"]["position"][0],
            42.0
        );
        assert_eq!(report["door_destination"]["destination"]["raw_flags"], 7);
        assert_eq!(
            report["door_destination"]["destination"]["authored_transform_words"],
            json!([
                0x4228_0000_u32,
                0x4000_0000,
                0x4040_0000,
                0,
                0x8000_0000_u32,
                0xbf00_0000_u32
            ])
        );
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["destination_applied"], false);
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            Sha256::digest(fs::read(directory.join("Data/Base.esm")).unwrap()),
            source_before
        );
    }
    #[test]
    fn cli_unresolved_door_emits_link_evidence_without_creating_residency() {
        let directory = directory();
        door_fixture(&directory, 0x999);
        let report = door_residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            door_input(),
        )
        .unwrap();
        assert_eq!(
            report["door_destination"]["source_destination_resolved"],
            false
        );
        assert!(
            !report["door_destination"]["issues"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(report["captured_sources_available"], false);
        assert!(report["source_error"].is_string());
        assert!(report["residency"].is_null());
        assert!(report["cell_models"].is_null());
        assert_eq!(report["current_cell_changed"], false);
        assert_eq!(report["runtime_ready"], false);
    }
    #[test]
    fn cli_residency_consumes_leased_texture_bytes_and_leaves_sources_unchanged() {
        let directory = directory();
        cell_fixture(&directory, b"m.nif", b"t.dds");
        let paths = [
            "Data/Base.esm",
            "Data/models.bsa",
            "Data/textures.bsa",
            "order.json",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|p| Sha256::digest(fs::read(directory.join(p)).unwrap()))
            .collect();
        let cache = directory.with_file_name(format!(
            "{}-cache",
            directory.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&cache).unwrap();
        let report = residency(
            &directory,
            &directory.join("order.json"),
            None,
            Some(&cache),
            residency_input(),
        )
        .unwrap();
        assert_eq!(report["captured_sources_available"], true);
        assert_eq!(report["residency"]["completed_models"], 1);
        assert_eq!(report["residency"]["completed_textures"], 1);
        assert_eq!(report["residency"]["outstanding"], 2);
        assert_eq!(report["residency"]["texture_state"], "Decoded");
        assert_eq!(report["residency"]["dependencies"], "Pending");
        assert_eq!(report["residency"]["simulation_ready"], false);
        assert_eq!(report["runtime_ready"], false);
        assert_eq!(
            report["texture_payloads"][0]["sha256"],
            format!("{:x}", Sha256::digest(b"authored-source-texture"))
        );
        assert_eq!(fs::read_dir(cache).unwrap().count(), 4);
        for (path, hash) in paths.iter().zip(before) {
            assert_eq!(
                Sha256::digest(fs::read(directory.join(path)).unwrap()),
                hash
            );
        }
    }
    #[test]
    fn cli_residency_retains_missing_absolute_and_ambiguous_texture_refusals() {
        for (path, ambiguous) in [
            (b"missing.dds".as_slice(), false),
            (b"C:\\export\\t.dds", false),
            (b"t.dds", true),
        ] {
            let directory = directory();
            cell_fixture(&directory, b"m.nif", path);
            if ambiguous {
                archive(
                    &directory,
                    "second-texture",
                    b"textures",
                    b"t.dds",
                    b"other-source",
                );
            }
            let report = residency(
                &directory,
                &directory.join("order.json"),
                None,
                None,
                residency_input(),
            )
            .unwrap();
            assert_eq!(report["captured_sources_available"], false);
            assert_eq!(report["residency"]["texture_state"], "Unsupported");
            assert_eq!(report["cell_textures"]["missing_or_ambiguous"], 1);
            assert_eq!(
                report["cell_textures"]["usages"][0]["raw_path"],
                json!(path)
            );
            assert_eq!(
                report["cell_textures"]["models"][0]["sha256"]
                    .as_str()
                    .unwrap()
                    .len(),
                64
            );
            assert_eq!(report["residency"]["dependencies"], "Pending");
        }
    }
    fn persistent_fixture(root: &Path, mode: &str) {
        grid_fixture(root, false);
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let mut base = fs::read(root.join("Data/Base.esm")).unwrap();
        base.extend(group(
            0x100,
            1,
            &group(
                0x202,
                6,
                &group(
                    0x202,
                    9,
                    &record(
                        b"REFR",
                        0x350,
                        &[
                            field(b"NAME", &0x400_u32.to_le_bytes()),
                            field(b"DATA", &[0; 24]),
                        ]
                        .concat(),
                    ),
                ),
            ),
        ));
        fs::write(root.join("Data/Base.esm"), base).unwrap();
        let (raw, flags) = match mode {
            "missing" => (0x202, 0),
            "deleted" => (
                0x202,
                fallout_data::plugin::PERSISTENT | fallout_data::plugin::DELETED,
            ),
            "multiple" => (0x0100_0203, fallout_data::plugin::PERSISTENT),
            _ => (0x202, fallout_data::plugin::PERSISTENT),
        };
        let mut persistent = record(
            b"CELL",
            raw,
            &[
                field(b"DATA", &[0]),
                field(b"XCLC", &[0xee, 0xff, 0xff, 0xff, 0, 0, 0, 0]),
            ]
            .concat(),
        );
        persistent[8..12].copy_from_slice(&flags.to_le_bytes());
        let mut patch = [
            record(
                b"TES4",
                0,
                &[
                    field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                    field(b"MAST", b"Base.esm\0"),
                    field(b"DATA", &[0; 8]),
                ]
                .concat(),
            ),
            group(0x100, 1, &persistent),
        ]
        .concat();
        if mode == "malformed" {
            patch.extend(record(b"STAT", 0x400, &field(b"MODL", b"m.nif")));
        }
        if mode == "no-modl" {
            patch.extend(record(b"STAT", 0x400, &[]));
        }
        fs::write(root.join("Data/Patch.esp"), patch).unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("persistent-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_PERSISTENT_FIXTURE={}", root.display());
    }
    #[test]
    fn cli_persistent_group_consumes_exact_source_models_without_grid_or_residency() {
        for mode in ["valid", "no-modl"] {
            let directory = directory();
            persistent_fixture(&directory, mode);
            let paths = [
                "Data/Base.esm",
                "Data/Patch.esp",
                "Data/models.bsa",
                "Data/textures.bsa",
                "order.json",
            ];
            let before = paths.map(|path| Sha256::digest(fs::read(directory.join(path)).unwrap()));
            let report = persistent_cell(
                &directory,
                &directory.join("order.json"),
                None,
                crate::parse_cell_key("Base.esm:100").unwrap(),
            )
            .unwrap();
            assert_eq!(report["source_plan_prepared"], true);
            assert_eq!(report["complete_model_selection"], mode == "valid");
            assert!(report["source_error"].is_null());
            assert_eq!(report["persistent_request"]["cell"]["local_id"], 0x202);
            assert!(report["persistent_request"].get("grid").is_none());
            assert_eq!(report["persistent_cell"]["role"], "persistent-group");
            assert_eq!(report["persistent_cell"]["header"]["offset"], 95);
            assert_eq!(report["persistent_cell"]["header"]["flags"], 0x400);
            assert_eq!(
                report["persistent_cell"]["fields"]["grid"]["value"],
                json!([-18, 0])
            );
            assert_eq!(
                report["persistent_cell"]["fields"]["grid"]["decoded_offset"],
                7
            );
            assert_eq!(report["cell_models"]["root"]["local_id"], 0x202);
            assert_eq!(report["root_members"], 1);
            assert_eq!(
                report["cell_models"]["requests"].as_array().unwrap().len(),
                usize::from(mode == "valid")
            );
            assert_eq!(
                report["cell_models"]["source_cohort_sha256"],
                report["cell_grid_sources"]["source_cohort_sha256"]
            );
            assert!(report["residency"].is_null());
            for field in [
                "runtime_ready",
                "activation_applied",
                "current_cell_changed",
                "lookup_precedence_verified",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[field], false);
            }
            for (path, hash) in paths.into_iter().zip(before) {
                assert_eq!(
                    Sha256::digest(fs::read(directory.join(path)).unwrap()),
                    hash
                );
            }
        }
    }
    #[test]
    fn cli_persistent_group_selection_and_factory_refusals_do_not_publish_partial_state() {
        for mode in ["missing", "deleted", "multiple", "malformed"] {
            let directory = directory();
            persistent_fixture(&directory, mode);
            let report = persistent_cell(
                &directory,
                &directory.join("order.json"),
                None,
                crate::parse_cell_key("Base.esm:100").unwrap(),
            )
            .unwrap();
            assert_eq!(report["source_plan_prepared"], false);
            assert!(report["cell_models"].is_null());
            assert!(report["residency"].is_null());
            assert!(!report["source_error"].as_str().unwrap().is_empty());
            assert_eq!(report["persistent_request"].is_null(), mode != "malformed");
            assert_eq!(report["persistent_cell"].is_null(), mode != "malformed");
            assert_eq!(report["runtime_ready"], false);
        }
    }
    #[test]
    fn cli_persistent_group_flags_do_not_accept_grid_authority() {
        use clap::Parser;
        let args = [
            "fallout",
            "persistent-cell-sources",
            "--install",
            "absent",
            "--load-order",
            "absent",
            "--world",
            "Base.esm:100",
        ];
        let parsed = crate::Args::try_parse_from(args).unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::PersistentCellSources { .. }
        ));
        assert!(crate::Args::try_parse_from(args.into_iter().chain(["--grid=0,0"])).is_err());
    }
    fn grid_set_fixture(root: &Path, mode: &str) {
        grid_fixture(root, false);
        archive(root, "second-model", b"meshes", b"n.nif", &[5, 6, 7, 8, 9]);
        let group = |label: u32, kind: i32, body: &[u8]| {
            [
                b"GRUP".as_slice(),
                &(body.len() as u32 + 24).to_le_bytes(),
                &label.to_le_bytes(),
                &kind.to_le_bytes(),
                &[0; 8],
                body,
            ]
            .concat()
        };
        let cell = |id, grid: [i32; 2]| {
            record(
                b"CELL",
                id,
                &[
                    field(b"DATA", &[0]),
                    field(
                        b"XCLC",
                        &grid
                            .into_iter()
                            .flat_map(i32::to_le_bytes)
                            .collect::<Vec<_>>(),
                    ),
                ]
                .concat(),
            )
        };
        let mut children = cell(0x201, [-17, 0]);
        children.extend(group(
            0x201,
            6,
            &group(
                0x201,
                9,
                &[
                    record(
                        b"REFR",
                        0x301,
                        &[
                            field(b"NAME", &0x400_u32.to_le_bytes()),
                            field(b"DATA", &[0; 24]),
                        ]
                        .concat(),
                    ),
                    record(
                        b"REFR",
                        0x302,
                        &[
                            field(b"NAME", &0x401_u32.to_le_bytes()),
                            field(b"DATA", &[0; 24]),
                        ]
                        .concat(),
                    ),
                ]
                .concat(),
            ),
        ));
        if mode == "ambiguous" {
            children.extend(cell(0x203, [-17, 0]));
        }
        let mut base = fs::read(root.join("Data/Base.esm")).unwrap();
        assert_eq!(base.len(), 314); // Literal authored header/group/record extents.
        base.extend(group(0x100, 1, &children));
        let model = match mode {
            "malformed" => field(b"MODL", b"n.nif"),
            "no-modl" => vec![],
            _ => field(b"MODL", b"n.nif\0"),
        };
        base.extend(record(b"STAT", 0x401, &model));
        fs::write(root.join("Data/Base.esm"), base).unwrap();
        fs::write(
            root.join("Data/Patch.esp"),
            [
                record(
                    b"TES4",
                    0,
                    &[
                        field(
                            b"HEDR",
                            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                        ),
                        field(b"MAST", b"Base.esm\0"),
                        field(b"DATA", &[0; 8]),
                    ]
                    .concat(),
                ),
                group(0x100, 1, &cell(0x200, [5, -6])),
                record(b"STAT", 0x400, &field(b"MODL", b"m.nif\0")),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("grid-set-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_GRID_SET_FIXTURE={}", root.display());
    }
    fn grid_set_input(grids: Vec<[i32; 2]>) -> GridSetInput {
        GridSetInput {
            world: crate::parse_cell_key("Base.esm:100").unwrap(),
            grids,
        }
    }
    #[test]
    fn cli_grid_set_preserves_literal_unequal_override_pair_and_source_quota() {
        let directory = directory();
        grid_set_fixture(&directory, "valid");
        let paths = [
            "Data/Base.esm",
            "Data/Patch.esp",
            "Data/models.bsa",
            "Data/second-model.bsa",
            "order.json",
        ];
        let before = paths.map(|path| Sha256::digest(fs::read(directory.join(path)).unwrap()));
        let report = grid_set(
            &directory,
            &directory.join("order.json"),
            None,
            grid_set_input(vec![[-17, 0], [5, -6]]),
        )
        .unwrap();
        assert_eq!(report["source_plans_prepared"], true);
        assert_eq!(report["complete_model_selection"], true);
        assert!(report["source_error"].is_null());
        for (index, cell, offset) in [(0, 0x201, 338), (1, 0x200, 95)] {
            assert_eq!(report["selected_cells"][index]["key"]["local_id"], cell);
            assert_eq!(report["selected_cells"][index]["header"]["offset"], offset);
            assert_eq!(
                report["cell_model_plan_set"]["plans"][index]["root"]["local_id"],
                cell
            );
            assert_eq!(
                report["cell_model_plan_set"]["plans"][index]["coverage"][0]["header"]["offset"],
                140
            );
            assert_eq!(
                report["cell_model_plan_set"]["plans"][index]["coverage"][0]["source_plugin"],
                "Patch.esp"
            );
            assert_eq!(
                report["cell_model_plan_set"]["plans"][index]["coverage"][0]["model_field"]["decoded_offset"],
                0
            );
        }
        assert_eq!(
            report["cell_model_plan_set"]["plans"][0]["requests"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            report["cell_model_plan_set"]["plans"][1]["requests"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(report["cell_model_plan_set"]["usage"]["models"], 3);
        assert_eq!(report["cell_model_plan_set"]["usage"]["archives"], 2);
        assert_eq!(
            report["cell_model_plan_set"]["usage"]["mapped_bytes"],
            fs::metadata(directory.join("Data/models.bsa"))
                .unwrap()
                .len()
                + fs::metadata(directory.join("Data/second-model.bsa"))
                    .unwrap()
                    .len()
        );
        assert_eq!(
            report["cell_model_plan_set"]["source_cohort_sha256"],
            report["cell_grid_sources"]["source_cohort_sha256"]
        );
        for flag in [
            "runtime_ready",
            "activation_applied",
            "current_cell_changed",
            "lookup_precedence_verified",
            "retail_parity_accepted",
        ] {
            assert_eq!(report[flag], false);
        }
        for (path, hash) in paths.into_iter().zip(before) {
            assert_eq!(
                Sha256::digest(fs::read(directory.join(path)).unwrap()),
                hash
            );
        }
    }
    #[test]
    fn cli_grid_set_refusals_never_emit_partial_bundle_and_coverage_remains_separate() {
        for mode in ["valid", "ambiguous", "malformed", "no-modl"] {
            let directory = directory();
            grid_set_fixture(&directory, mode);
            let report = grid_set(
                &directory,
                &directory.join("order.json"),
                None,
                grid_set_input(vec![[5, -6], [-17, 0]]),
            )
            .unwrap();
            if mode == "no-modl" {
                assert_eq!(report["source_plans_prepared"], true);
                assert_eq!(report["complete_model_selection"], false);
            } else if mode != "valid" {
                assert_eq!(report["source_plans_prepared"], false);
                assert!(report["cell_model_plan_set"].is_null());
                assert!(report["selected_cells"].as_array().unwrap().is_empty());
                assert!(!report["source_error"].as_str().unwrap().is_empty());
                assert_eq!(
                    report["cell_grid_set_request"].is_null(),
                    mode == "ambiguous"
                );
            }
            for grids in [vec![[5, -6], [999, 999]], vec![[5, -6], [5, -6]]] {
                let report = grid_set(
                    &directory,
                    &directory.join("order.json"),
                    None,
                    grid_set_input(grids),
                )
                .unwrap();
                assert_eq!(report["source_plans_prepared"], false);
                assert!(report["cell_model_plan_set"].is_null());
                assert!(report["cell_grid_set_request"].is_null());
            }
        }
    }
    #[test]
    fn cli_grid_set_explicit_pairs_require_signed_bounds_and_repeated_flags() {
        use clap::Parser;
        let args = [
            "fallout-cli",
            "grid-set-sources",
            "--install",
            "absent",
            "--load-order",
            "absent",
            "--world",
            "Base.esm:100",
        ];
        assert!(crate::Args::try_parse_from(args).is_err());
        let parsed = crate::Args::try_parse_from(
            args.into_iter()
                .chain(["--grid=-2147483648,2147483647", "--grid=0,0"]),
        )
        .unwrap();
        match parsed.command {
            crate::Command::GridSetSources { grid, .. } => assert_eq!(
                parse_grid_set(&grid).unwrap(),
                [[i32::MIN, i32::MAX], [0, 0]]
            ),
            _ => panic!("explicit set command changed"),
        }
        for value in ["0", "0,1,2", "2147483648,0", "0,-2147483649", ","] {
            assert!(parse_grid_set(&[value.into()]).is_err());
        }
        assert!(parse_grid_set(&[]).is_err());
        assert!(parse_grid_set(&vec!["0,0".into(); 9]).is_err());
        assert!(
            grid_set(
                Path::new("absent"),
                Path::new("absent"),
                None,
                grid_set_input(vec![])
            )
            .is_err()
        );
    }
    #[test]
    fn cli_residency_missing_model_and_bad_timeout_never_claim_availability() {
        let directory = directory();
        cell_fixture(&directory, b"missing.nif", b"t.dds");
        let report = residency(
            &directory,
            &directory.join("order.json"),
            None,
            None,
            residency_input(),
        )
        .unwrap();
        assert_eq!(report["captured_sources_available"], false);
        assert_eq!(report["residency"]["completed_models"], 0);
        assert_eq!(report["residency"]["complete_model_coverage"], false);
        for timeout in [0, 120_001] {
            let mut input = residency_input();
            input.source_timeout_ms = timeout;
            assert!(
                residency(&directory, &directory.join("order.json"), None, None, input).is_err()
            );
        }
    }

    fn activation_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let origin = if mode == "player" {
            "FalloutNV.esm"
        } else {
            "Base.esm"
        };
        let has_parents = !["absent", "flags-only", "prompt-only"].contains(&mode);
        let mut body = field(b"NAME", &[0, 5, 0, 0]);
        if has_parents {
            let raw = match mode {
                "null" => 0_u32,
                "missing" => 0x999,
                "player" => 0x14,
                _ => 0x201,
            };
            let parent = [raw.to_le_bytes(), 0x80000000_u32.to_le_bytes()].concat();
            {
                body.extend(field(
                    b"XAPR",
                    if mode == "bad-parent-width" {
                        &parent[..7]
                    } else {
                        &parent
                    },
                ));
            }
        }
        if !["absent", "prompt-only"].contains(&mode) {
            body.extend(field(
                b"XAPD",
                if mode == "bad-flags-width" {
                    &[254, 0]
                } else {
                    &[254]
                },
            ));
        }
        if has_parents {
            body.extend(field(b"XAPR", &[0, 2, 0, 0, 1, 0, 0, 0]));
        }
        if !["absent", "flags-only"].contains(&mode) {
            body.extend(field(
                b"XATO",
                if mode == "bad-prompt-nul" {
                    &[255, 128, b'O']
                } else {
                    &[255, 128, b'O', 0]
                },
            ));
        }
        if has_parents {
            body.extend(field(b"ZZZZ", &[91, 92]));
            body.extend(field(b"XAPR", &[1, 2, 0, 0, 0, 0, 128, 191]));
            body.extend(field(b"XAPR", &[2, 2, 0, 0, 69, 35, 193, 127]));
        }
        if mode != "bad-core" {
            body.extend(field(b"DATA", &[0; 24]));
        }
        if mode == "duplicate-flags" {
            body.extend(field(b"XAPD", &[1]));
        }
        if mode == "duplicate-prompt" {
            body.extend(field(b"XATO", &[0]));
        }
        if mode == "truncated" {
            // Terminal framing prevents another field from filling its payload.
            body.extend(b"XAPR\x08\0\0");
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let kind = match mode {
            "root-achr" => b"ACHR",
            "root-acre" => b"ACRE",
            _ => b"REFR",
        };
        fs::write(
            root.join("Data").join(origin),
            [
                record(b"TES4", 0, &hedr),
                record(kind, 0x100, &body),
                record(b"PGRE", 0x200, &field(b"ZZZZ", &[1])),
                record(b"PMIS", 0x201, &field(b"ZZZZ", &[2])),
                record(b"PBEA", 0x202, &field(b"ZZZZ", &[3])),
            ]
            .concat(),
        )
        .unwrap();
        let master = if mode == "missing-master" {
            "Missing.esm"
        } else {
            origin
        };
        let patch_header = [
            hedr,
            field(b"MAST", &[master.as_bytes(), &[0]].concat()),
            field(b"DATA", &[0; 8]),
        ]
        .concat();
        let mut target = record(
            if mode == "wrong-record-kind" {
                b"STAT"
            } else {
                b"PBEA"
            },
            0x201,
            &[1, 2, 3],
        );
        if mode == "deleted" {
            target[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        fs::write(
            root.join("Data/Patch.esp"),
            [record(b"TES4", 0, &patch_header), target].concat(),
        )
        .unwrap();
        fs::write(
            root.join("order.json"),
            serde_json::to_vec(&json!([origin, "Patch.esp"])).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("activation-case.json"),
            serde_json::to_vec(&json!({"mode":mode,"origin":origin})).unwrap(),
        )
        .unwrap();
        println!("WORLD_ACTIVATION_FIXTURE={}", root.display());
    }
    fn activation_report(root: &Path, mode: &str) -> Value {
        activation(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key(if mode == "player" {
                "FalloutNV.esm:100"
            } else {
                "Base.esm:100"
            })
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_activation_keeps_literal_unsorted_repeats_delay_words_prompt_and_actual_target_source() {
        for mode in ["valid", "root-achr", "root-acre"] {
            let root = directory();
            activation_fixture(&root, mode);
            let report = activation_report(&root, mode);
            assert_eq!(report["source_request_prepared"], true);
            assert!(report["source_error"].is_null());
            assert_eq!(report["activation_flags_declared"], true);
            assert_eq!(report["parent_count"], 4);
            assert_eq!(report["parent_header_sources_available"], true);
            assert_eq!(report["prompt_declared"], true);
            let s = &report["activation_sources"];
            assert_eq!(s["placed"]["header"]["offset"], 42);
            assert_eq!(s["placed"]["header"]["stored_size"], 121);
            assert_eq!(s["placement"]["unhandled_fields"]["XAPR"], 4);
            assert_eq!(s["activation_flags"]["raw"], 254);
            assert_eq!(s["prompt"]["raw"], json!([255, 128, 79, 0]));
            assert_eq!(s["prompt"]["field"]["site"]["decoded_header_offset"], 45);
            assert_eq!(s["prompt"]["field"]["site"]["span"]["decoded_offset"], 51);
            assert_eq!(s["prompt"]["field"]["physical_framing_offset"], 111);
            let prompt_sha = format!("{:x}", Sha256::digest([255, 128, b'O', 0]));
            assert_eq!(s["prompt"]["sha256"], prompt_sha);
            for (index, (raw, bits, ordinal, offset)) in [
                (0x201, 0x80000000_u32, 1, 10),
                (0x200, 1, 3, 31),
                (0x201, 0xbf800000, 6, 63),
                (0x202, 0x7fc12345, 7, 77),
            ]
            .into_iter()
            .enumerate()
            {
                let p = &s["parents"][index];
                assert_eq!(p["parent_raw"], raw);
                assert_eq!(p["delay_bits"], bits);
                assert_eq!(p["field"]["physical_field_ordinal"], ordinal);
                assert_eq!(p["field"]["logical_field_ordinal"], ordinal);
                assert_eq!(p["field"]["site"]["decoded_header_offset"], offset);
                assert_eq!(p["field"]["site"]["span"]["decoded_offset"], offset + 6);
                assert_eq!(p["field"]["physical_framing_offset"], 66 + offset);
                assert_eq!(p["target"]["status"], "resolved");
            }
            assert_eq!(s["parents"][0]["target"]["key"]["local_id"], 0x201);
            assert_eq!(s["parents"][0]["source"]["source_ordinal"], 1);
            assert_eq!(s["parents"][0]["source"]["source_plugin"], "Patch.esp");
            assert_eq!(s["parents"][0]["source"]["header"]["offset"], 71);
            assert_eq!(
                s["parents"][0]["source"]["header"]["kind"],
                json!([80, 66, 69, 65])
            );
            assert_eq!(s["usage"]["read_bytes"], 121);
            assert_eq!(s["usage"]["records"], 5);
            assert_eq!(s["usage"]["raw_bytes"], 73);
            for field in [
                "target_bodies_decoded",
                "parent_graph_walked",
                "delay_interpreted",
                "timer_scheduled",
                "native_activation_executed",
                "condition_truth_evaluated",
                "actor_item_state_mutated",
                "current_cell_mutated",
                "prompt_decoded",
                "default_prompt_selected",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[field], false, "{field}");
            }
        }
    }
    #[test]
    fn cli_activation_absence_unavailable_and_strict_refusals_never_produce_a_timer_or_prompt_default()
     {
        for (mode, refused) in [
            ("absent", false),
            ("flags-only", false),
            ("prompt-only", false),
            ("null", false),
            ("missing", false),
            ("deleted", false),
            ("wrong-record-kind", false),
            ("player", false),
            ("duplicate-flags", true),
            ("duplicate-prompt", true),
            ("bad-parent-width", true),
            ("bad-flags-width", true),
            ("bad-prompt-nul", true),
            ("truncated", true),
            ("bad-core", true),
            ("missing-master", true),
        ] {
            let root = directory();
            activation_fixture(&root, mode);
            if mode == "missing-master" {
                assert!(
                    activation(
                        &root,
                        &root.join("order.json"),
                        None,
                        crate::parse_cell_key("Base.esm:100").unwrap()
                    )
                    .is_err()
                );
                continue;
            }
            let r = activation_report(&root, mode);
            assert_eq!(r["source_request_prepared"], !refused, "{mode}");
            assert_eq!(r["activation_sources"].is_null(), refused, "{mode}");
            if refused {
                assert!(!r["source_error"].is_null(), "{mode}");
            } else {
                assert!(r["source_error"].is_null());
                assert_eq!(
                    r["parent_count"],
                    if ["absent", "flags-only", "prompt-only"].contains(&mode) {
                        0
                    } else {
                        4
                    }
                );
                let expected = match mode {
                    "null" => Some("null"),
                    "missing" => Some("missing"),
                    "deleted" => Some("deleted"),
                    "wrong-record-kind" => Some("wrong-record-kind"),
                    "player" => Some("runtime-player-binding-unimplemented"),
                    _ => None,
                };
                if let Some(status) = expected {
                    assert_eq!(r["parent_resolution_statuses"][0], status);
                    assert_eq!(
                        r["activation_sources"]["parents"][0]["header_source_available"],
                        false
                    );
                    assert_eq!(r["parent_header_sources_available"], false);
                }
            }
            for field in [
                "delay_interpreted",
                "timer_scheduled",
                "native_activation_executed",
                "prompt_decoded",
                "default_prompt_selected",
                "runtime_ready",
            ] {
                assert_eq!(r[field], false, "{mode} {field}");
            }
        }
        let root = directory();
        activation_fixture(&root, "valid");
        for key in ["Base.esm:999", "Base.esm:201"] {
            let r = activation(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(key).unwrap(),
            )
            .unwrap();
            assert_eq!(r["source_request_prepared"], false);
            assert!(r["activation_sources"].is_null());
            assert!(!r["source_error"].is_null());
        }
    }
    #[test]
    fn cli_activation_requires_an_explicit_canonical_reference() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "placed-activation-sources",
                "--install",
                "x",
                "--load-order",
                "o"
            ])
            .is_err()
        );
        let args = crate::Args::try_parse_from([
            "fallout",
            "placed-activation-sources",
            "--install",
            "x",
            "--load-order",
            "o",
            "--reference",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            crate::Command::PlacedActivationSources { .. }
        ));
    }

    fn linked_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let origin = if mode == "player" {
            "FalloutNV.esm"
        } else {
            "Base.esm"
        };
        let mut body = field(b"NAME", &[0, 5, 0, 0]);
        if mode != "absent" {
            body.extend(field(b"XCLP", &[1, 2, 3, 170, 4, 5, 6, 187]));
            body.extend(field(b"ZZZZ", &[91, 92]));
        }
        if !["absent", "color-only"].contains(&mode) {
            let raw = match mode {
                "null" => 0_u32,
                "missing" => 0x999,
                "player" => 0x14,
                _ => 0x200,
            };
            if mode == "truncated" {
                body.extend(b"XLKR\x04\0\0");
            } else {
                body.extend(field(b"XLKR", &raw.to_le_bytes()));
            }
        }
        if mode != "bad-core" {
            body.extend(field(b"DATA", &[0; 24]));
        }
        match mode {
            "duplicate-link" => body.extend(field(b"XLKR", &[0; 4])),
            "duplicate-color" => body.extend(field(b"XCLP", &[0; 8])),
            "bad-link-width" => {
                body = [
                    field(b"NAME", &[0, 5, 0, 0]),
                    field(b"DATA", &[0; 24]),
                    field(b"XLKR", &[0; 3]),
                ]
                .concat()
            }
            "bad-color-width" => {
                body = [
                    field(b"NAME", &[0, 5, 0, 0]),
                    field(b"DATA", &[0; 24]),
                    field(b"XCLP", &[0; 7]),
                ]
                .concat()
            }
            _ => {}
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let kind = match mode {
            "root-achr" => b"ACHR",
            "root-acre" => b"ACRE",
            _ => b"REFR",
        };
        let base = [
            record(b"TES4", 0, &hedr),
            record(kind, 0x100, &body),
            record(b"PGRE", 0x200, &field(b"ZZZZ", &[1])),
        ]
        .concat();
        fs::write(root.join("Data").join(origin), base).unwrap();
        let master = if mode == "missing-master" {
            "Missing.esm"
        } else {
            origin
        };
        let patch_header = [
            hedr,
            field(b"MAST", &[master.as_bytes(), &[0]].concat()),
            field(b"DATA", &[0; 8]),
        ]
        .concat();
        let mut target = record(
            if mode == "wrong-record-kind" {
                b"STAT"
            } else {
                b"PBEA"
            },
            0x200,
            &[1, 2, 3],
        );
        if mode == "deleted" {
            target[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        fs::write(
            root.join("Data/Patch.esp"),
            [record(b"TES4", 0, &patch_header), target].concat(),
        )
        .unwrap();
        fs::write(
            root.join("order.json"),
            serde_json::to_vec(&json!([origin, "Patch.esp"])).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("linked-case.json"),
            serde_json::to_vec(&json!({"mode":mode,"origin":origin})).unwrap(),
        )
        .unwrap();
        println!("WORLD_LINKED_FIXTURE={}", root.display());
    }
    fn linked_report(root: &Path, mode: &str) -> Value {
        let key = if mode == "player" {
            "FalloutNV.esm:100"
        } else {
            "Base.esm:100"
        };
        linked(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key(key).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_linked_preserves_exact_field_spans_raw_unused_color_bytes_and_deferred_projectile_header()
     {
        for mode in ["valid", "root-achr", "root-acre"] {
            let root = directory();
            linked_fixture(&root, mode);
            let report = linked_report(&root, mode);
            assert_eq!(report["source_request_prepared"], true);
            assert!(report["source_error"].is_null());
            assert_eq!(report["link_declared"], true);
            assert_eq!(report["color_declared"], true);
            assert_eq!(report["link_header_source_available"], true);
            assert_eq!(report["link_resolution_status"], "resolved");
            let s = &report["linked_sources"];
            assert_eq!(s["placed"]["header"]["offset"], 42);
            assert_eq!(s["usage"]["read_bytes"], 72);
            assert_eq!(s["placement"]["unhandled_fields"]["XLKR"], 1);
            assert_eq!(s["placement"]["unhandled_fields"]["XCLP"], 1);
            let link = &s["link"];
            assert_eq!(link["raw"], 0x200);
            assert_eq!(link["target"]["key"]["local_id"], 0x200);
            assert_eq!(link["target"]["status"], "resolved");
            assert_eq!(link["source"]["source_ordinal"], 1);
            assert_eq!(link["source"]["source_plugin"], "Patch.esp");
            assert_eq!(link["source"]["header"]["offset"], 71);
            assert_eq!(link["source"]["header"]["kind"], json!(b"PBEA")); // Body intentionally lacks a valid placement.
            assert_eq!(link["field"]["physical_field_ordinal"], 3);
            assert_eq!(link["field"]["logical_field_ordinal"], 3);
            assert_eq!(link["field"]["site"]["decoded_header_offset"], 32);
            assert_eq!(link["field"]["site"]["span"]["decoded_offset"], 38);
            assert_eq!(link["field"]["physical_framing_offset"], 98);
            let color = &s["color"];
            assert_eq!(color["raw"], json!([1, 2, 3, 170, 4, 5, 6, 187]));
            assert_eq!(color["field"]["physical_field_ordinal"], 1);
            assert_eq!(color["field"]["site"]["decoded_header_offset"], 10);
            assert_eq!(color["field"]["site"]["span"]["decoded_offset"], 16);
            assert_eq!(color["field"]["site"]["span"]["bytes"], 8);
            assert_eq!(color["field"]["physical_framing_offset"], 76);
            for flag in [
                "target_body_decoded",
                "target_placement_admitted",
                "runtime_binding_evaluated",
                "native_get_linked_ref_executed",
                "actor_package_selected",
                "chain_walked",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[flag], false);
            }
        }
    }
    #[test]
    fn cli_linked_absent_color_only_unavailable_player_and_strict_factory_failures_remain_distinct()
    {
        for mode in [
            "absent",
            "color-only",
            "null",
            "missing",
            "deleted",
            "wrong-record-kind",
            "player",
            "duplicate-link",
            "duplicate-color",
            "bad-link-width",
            "bad-color-width",
            "truncated",
            "bad-core",
            "missing-master",
        ] {
            let root = directory();
            linked_fixture(&root, mode);
            if mode == "missing-master" {
                assert!(
                    linked(
                        &root,
                        &root.join("order.json"),
                        None,
                        crate::parse_cell_key("Base.esm:100").unwrap()
                    )
                    .is_err()
                );
                continue;
            }
            let report = linked_report(&root, mode);
            let refused = [
                "duplicate-link",
                "duplicate-color",
                "bad-link-width",
                "bad-color-width",
                "truncated",
                "bad-core",
            ]
            .contains(&mode);
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["linked_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if !refused {
                let declared = !["absent", "color-only"].contains(&mode);
                let status = match mode {
                    "absent" | "color-only" => "absent",
                    "player" => "runtime-player-binding-unimplemented",
                    _ => mode,
                };
                assert_eq!(report["link_declared"], declared);
                assert_eq!(report["color_declared"], mode != "absent");
                assert_eq!(report["link_resolution_status"], status);
                assert_eq!(report["link_header_source_available"], false);
                assert_eq!(report["linked_sources"]["link"].is_null(), !declared);
                if mode == "player" {
                    assert_eq!(
                        report["linked_sources"]["link"]["target"]["key"]["origin_plugin"],
                        "falloutnv.esm"
                    );
                    assert_eq!(
                        report["linked_sources"]["link"]["target"]["key"]["local_id"],
                        0x14
                    );
                    assert!(report["linked_sources"]["link"]["source"].is_null());
                }
            }
            assert_eq!(report["target_placement_admitted"], false);
            assert_eq!(report["native_get_linked_ref_executed"], false);
        }
        let root = directory();
        linked_fixture(&root, "valid");
        for key in ["Base.esm:999", "Base.esm:200"] {
            let r = linked(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(key).unwrap(),
            )
            .unwrap();
            assert_eq!(r["source_request_prepared"], false);
            assert!(r["linked_sources"].is_null());
        }
    }
    #[test]
    fn cli_linked_requires_an_explicit_canonical_reference() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "placed-linked-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json"
            ])
            .is_err()
        );
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "placed-linked-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--reference",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::PlacedLinkedSources { .. }
        ));
    }

    fn ownership_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut cell = field(b"DATA", &[1]);
        match mode {
            "absent" => {}
            "owner-only" | "null" | "missing" => cell.extend(field(
                b"XOWN",
                &(match mode {
                    "null" => 0_u32,
                    "missing" => 0x999,
                    _ => 0x200,
                })
                .to_le_bytes(),
            )),
            "rank-only" => cell.extend(field(b"XRNK", &[0, 0, 0, 128])),
            "wide-owner" => cell.extend(field(b"XOWN", &[0; 12])),
            "short-rank" => cell.extend(field(b"XRNK", &[0; 3])),
            "truncated" => cell.extend(b"XOWN\x04\0\0"),
            _ => {
                let word = match mode {
                    "signed-min" => [0, 0, 0, 128],
                    "signed-max" => [255, 255, 255, 127],
                    _ => [249, 255, 255, 255],
                };
                cell.extend(field(b"XRNK", &word));
                cell.extend(field(b"ZZZZ", &[91, 92]));
                cell.extend(field(
                    b"XOWN",
                    &(if mode == "npc" { 0x201_u32 } else { 0x200 }).to_le_bytes(),
                ));
            }
        }
        match mode {
            "duplicate-owner" => cell.extend(field(b"XOWN", &[0; 4])),
            "duplicate-rank" => cell.extend(field(b"XRNK", &[0; 4])),
            "xglb" => cell.extend(field(b"XGLB", &[0, 2, 0, 0])),
            _ => {}
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let base = [
            record(b"TES4", 0, &hedr),
            record(b"CELL", 0x100, &cell),
            record(b"FACT", 0x200, &field(b"ZZZZ", &[1])),
            record(b"NPC_", 0x201, &field(b"ZZZZ", &[2])),
        ]
        .concat();
        fs::write(root.join("Data/Base.esm"), base).unwrap();
        let patch_header = [
            hedr,
            field(
                b"MAST",
                if mode == "missing-master" {
                    b"Missing.esm\0"
                } else {
                    b"Base.esm\0"
                },
            ),
            field(b"DATA", &[0; 8]),
        ]
        .concat();
        let mut target = record(
            if mode == "wrong-record-kind" {
                b"MUSC"
            } else {
                b"FACT"
            },
            0x200,
            &[1, 2, 3],
        );
        if mode == "deleted" {
            target[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        fs::write(
            root.join("Data/Patch.esp"),
            [record(b"TES4", 0, &patch_header), target].concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("ownership-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_OWNERSHIP_FIXTURE={}", root.display());
    }
    fn ownership_report(root: &Path) -> Value {
        ownership(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key("Base.esm:100").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_ownership_preserves_fact_npc_signed_extremes_raw_spans_and_actual_header_sources() {
        for mode in ["valid", "npc", "signed-min", "signed-max"] {
            let root = directory();
            ownership_fixture(&root, mode);
            let report = ownership_report(&root);
            assert_eq!(report["source_request_prepared"], true);
            assert!(report["source_error"].is_null());
            assert_eq!(report["owner_declared"], true);
            assert_eq!(report["rank_declared"], true);
            assert_eq!(report["owner_header_source_available"], true);
            assert_eq!(report["owner_resolution_status"], "resolved");
            let s = &report["ownership_sources"];
            assert_eq!(s["cell"]["header"]["offset"], 42);
            assert_eq!(s["usage"]["read_bytes"], 35);
            let owner = &s["owner"];
            let id = if mode == "npc" { 0x201_u32 } else { 0x200 };
            assert_eq!(owner["raw"], id);
            assert_eq!(owner["target"]["key"]["local_id"], id);
            assert_eq!(owner["target"]["status"], "resolved");
            assert_eq!(owner["field"]["physical_field_ordinal"], 3);
            assert_eq!(owner["field"]["logical_field_ordinal"], 3);
            assert_eq!(owner["field"]["site"]["decoded_header_offset"], 25);
            assert_eq!(owner["field"]["site"]["span"]["decoded_offset"], 31);
            assert_eq!(owner["field"]["physical_framing_offset"], 91);
            assert_eq!(
                owner["source"]["source_ordinal"],
                usize::from(mode != "npc")
            );
            assert_eq!(
                owner["source"]["header"]["offset"],
                if mode == "npc" { 132 } else { 71 }
            );
            let rank = &s["rank"];
            let (raw, value) = match mode {
                "signed-min" => (0x80000000_u32, i32::MIN),
                "signed-max" => (0x7fffffff, i32::MAX),
                _ => (0xfffffff9, -7),
            };
            assert_eq!(rank["raw"], raw);
            assert_eq!(rank["value"], value);
            assert_eq!(rank["field"]["physical_field_ordinal"], 1);
            assert_eq!(rank["field"]["logical_field_ordinal"], 1);
            assert_eq!(rank["field"]["site"]["decoded_header_offset"], 7);
            assert_eq!(rank["field"]["site"]["span"]["decoded_offset"], 13);
            assert_eq!(rank["field"]["physical_framing_offset"], 73);
            assert_eq!(
                owner["ownership_status"],
                "unknown; target body and ownership not evaluated"
            );
            assert_eq!(
                rank["applicability_status"],
                "unknown; actor/faction rank and ownership not evaluated"
            );
            for flag in [
                "ownership_evaluated",
                "actor_faction_rank_evaluated",
                "access_evaluated",
                "theft_evaluated",
                "target_behavior_decoded",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[flag], false);
            }
        }
    }
    #[test]
    fn cli_ownership_absence_rank_only_unavailable_and_strict_non_nv_fields_remain_explicit() {
        for mode in [
            "absent",
            "owner-only",
            "rank-only",
            "null",
            "missing",
            "deleted",
            "wrong-record-kind",
            "duplicate-owner",
            "duplicate-rank",
            "wide-owner",
            "short-rank",
            "xglb",
            "truncated",
            "missing-master",
        ] {
            let root = directory();
            ownership_fixture(&root, mode);
            if ["truncated", "missing-master"].contains(&mode) {
                assert!(
                    ownership(
                        &root,
                        &root.join("order.json"),
                        None,
                        crate::parse_cell_key("Base.esm:100").unwrap()
                    )
                    .is_err()
                );
                continue;
            }
            let report = ownership_report(&root);
            let refused = [
                "duplicate-owner",
                "duplicate-rank",
                "wide-owner",
                "short-rank",
                "xglb",
            ]
            .contains(&mode);
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["ownership_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if !refused {
                let owner_declared = !["absent", "rank-only"].contains(&mode);
                let rank_declared = ["rank-only", "deleted", "wrong-record-kind"].contains(&mode);
                assert_eq!(report["owner_declared"], owner_declared);
                assert_eq!(report["rank_declared"], rank_declared);
                assert_eq!(
                    report["ownership_sources"]["owner"].is_null(),
                    !owner_declared
                );
                assert_eq!(
                    report["ownership_sources"]["rank"].is_null(),
                    !rank_declared
                );
                assert_eq!(
                    report["owner_resolution_status"],
                    match mode {
                        "absent" | "rank-only" => "absent",
                        "owner-only" => "resolved",
                        _ => mode,
                    }
                );
                assert_eq!(
                    report["owner_header_source_available"],
                    mode == "owner-only"
                );
                if mode == "rank-only" {
                    assert_eq!(report["ownership_sources"]["rank"]["value"], i32::MIN);
                }
            } else if mode == "xglb" {
                assert!(
                    report["source_error"]
                        .as_str()
                        .unwrap()
                        .contains("raw unknown/unsupported for FNV ownership")
                );
            }
            assert_eq!(report["ownership_evaluated"], false);
            assert_eq!(report["access_evaluated"], false);
        }
        let root = directory();
        ownership_fixture(&root, "valid");
        for key in ["Base.esm:999", "Base.esm:200"] {
            let r = ownership(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(key).unwrap(),
            )
            .unwrap();
            assert_eq!(r["source_request_prepared"], false);
            assert!(r["ownership_sources"].is_null());
        }
    }
    #[test]
    fn cli_ownership_requires_an_explicit_canonical_cell() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "cell-ownership-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json"
            ])
            .is_err()
        );
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "cell-ownership-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--cell",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::CellOwnershipSources { .. }
        ));
    }

    fn region_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut cell = field(b"DATA", &[2]);
        match mode {
            "absent" => {}
            "empty" => cell.extend(field(b"XCLR", &[])),
            "null" | "missing" | "noncanonical-self" => cell.extend(field(
                b"XCLR",
                &(match mode {
                    "null" => 0_u32,
                    "missing" => 0x999,
                    _ => 0x02000200,
                })
                .to_le_bytes(),
            )),
            "unsupported" => cell.extend(field(b"XCLR", &[0; 3])),
            "missing-master" => cell.extend(field(b"XCLR", &[0, 2, 0, 0])),
            "truncated" => cell.extend(b"XCLR\x04\0\0"),
            _ => {
                cell.extend(field(b"XCLR", &[1, 2, 0, 0, 0, 2, 0, 0, 1, 2, 0, 0]));
                cell.extend(field(b"ZZZZ", &[91, 92]));
                cell.extend(field(b"XCLR", &[2, 2, 0, 0, 0, 2, 0, 0]));
                cell.extend(field(b"XCLR", &[]));
            }
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let mut base = [record(b"TES4", 0, &hedr), record(b"CELL", 0x100, &cell)].concat();
        for id in 0x200..=0x202 {
            base.extend(record(b"REGN", id, &field(b"ZZZZ", &[1])));
        }
        fs::write(root.join("Data/Base.esm"), base).unwrap();
        let patch_header = [
            hedr,
            field(
                b"MAST",
                if mode == "missing-master" {
                    b"Missing.esm\0"
                } else {
                    b"Base.esm\0"
                },
            ),
            field(b"DATA", &[0; 8]),
        ]
        .concat();
        let mut target = record(
            if mode == "wrong-record-kind" {
                b"MUSC"
            } else {
                b"REGN"
            },
            0x200,
            &[1, 2, 3],
        );
        if mode == "deleted" {
            target[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        fs::write(
            root.join("Data/Patch.esp"),
            [record(b"TES4", 0, &patch_header), target].concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("region-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_REGION_FIXTURE={}", root.display());
    }
    fn region_report(root: &Path) -> Value {
        regions(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key("Base.esm:100").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_regions_preserve_literal_repeats_occurrences_spans_and_winning_regn_headers() {
        let root = directory();
        region_fixture(&root, "valid");
        let report = region_report(&root);
        assert_eq!(report["source_request_prepared"], true);
        assert!(report["source_error"].is_null());
        assert_eq!(report["region_fields_declared"], true);
        assert_eq!(report["region_element_count"], 5);
        assert_eq!(report["resolved_header_count"], 5);
        assert_eq!(report["unavailable_header_count"], 0);
        let sources = &report["region_sources"];
        assert_eq!(sources["cell"]["header"]["offset"], 42);
        assert_eq!(sources["usage"]["read_bytes"], 53);
        let occurrences = sources["occurrences"].as_array().unwrap();
        assert_eq!(occurrences.len(), 3);
        for (o, (ordinal, offset, ids)) in occurrences.iter().zip([
            (1, 7, vec![0x201_u32, 0x200, 0x201]),
            (3, 33, vec![0x202, 0x200]),
            (4, 47, vec![]),
        ]) {
            assert_eq!(o["physical_field_ordinal"], ordinal);
            assert_eq!(o["logical_field_ordinal"], ordinal);
            assert_eq!(o["site"]["decoded_header_offset"], offset);
            assert_eq!(o["physical_framing_offset"], 66 + offset);
            let elements = o["elements"].as_array().unwrap();
            assert_eq!(elements.len(), ids.len());
            for (index, (e, id)) in elements.iter().zip(ids).enumerate() {
                assert_eq!(e["element_ordinal"], index);
                assert_eq!(e["raw"], id);
                assert_eq!(e["span"]["decoded_offset"], offset + 6 + index * 4);
                assert_eq!(e["span"]["bytes"], 4);
                assert_eq!(e["physical_payload_offset"], 72 + offset + index * 4);
                assert_eq!(e["target"]["key"]["local_id"], id);
                assert_eq!(e["target"]["status"], "resolved");
                assert_eq!(e["header_source_available"], true);
                assert_eq!(
                    e["behavior_status"],
                    "unknown; REGN body and behavior not decoded"
                );
                if id == 0x200 {
                    assert_eq!(e["source"]["source_ordinal"], 1);
                    assert_eq!(e["source"]["header"]["offset"], 71);
                }
            }
        }
        for flag in [
            "target_behavior_decoded",
            "world_parent_evaluated",
            "region_geometry_evaluated",
            "region_priority_evaluated",
            "region_effects_applied",
            "runtime_ready",
            "retail_parity_accepted",
        ] {
            assert_eq!(report[flag], false);
        }
    }
    #[test]
    fn cli_regions_keep_absent_empty_and_unavailable_distinct_and_refuse_bad_arrays_or_keys() {
        for mode in [
            "absent",
            "empty",
            "null",
            "missing",
            "deleted",
            "wrong-record-kind",
            "unsupported",
            "noncanonical-self",
            "truncated",
            "missing-master",
        ] {
            let root = directory();
            region_fixture(&root, mode);
            if ["truncated", "missing-master"].contains(&mode) {
                assert!(
                    regions(
                        &root,
                        &root.join("order.json"),
                        None,
                        crate::parse_cell_key("Base.esm:100").unwrap()
                    )
                    .is_err()
                );
                continue;
            }
            let report = region_report(&root);
            let refused = mode == "unsupported";
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["region_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if !refused {
                assert_eq!(report["region_fields_declared"], mode != "absent");
                if ["absent", "empty"].contains(&mode) {
                    assert_eq!(report["region_element_count"], 0);
                    assert_eq!(report["resolved_header_count"], 0);
                    assert_eq!(report["unavailable_header_count"], 0);
                    assert_eq!(
                        report["region_sources"]["occurrences"]
                            .as_array()
                            .unwrap()
                            .len(),
                        usize::from(mode == "empty")
                    );
                } else {
                    let occurrences = report["region_sources"]["occurrences"].as_array().unwrap();
                    let statuses = occurrences
                        .iter()
                        .flat_map(|o| o["elements"].as_array().unwrap())
                        .map(|e| e["target"]["status"].as_str().unwrap())
                        .collect::<Vec<_>>();
                    if mode == "noncanonical-self" {
                        assert_eq!(statuses, vec!["resolved"]);
                        assert_eq!(occurrences[0]["elements"][0]["raw"], 0x02000200_u32);
                        assert_eq!(
                            occurrences[0]["elements"][0]["target"]["key"]["origin_plugin"],
                            "base.esm"
                        );
                    } else if ["null", "missing"].contains(&mode) {
                        assert_eq!(statuses, vec![mode]);
                    } else {
                        assert_eq!(
                            statuses,
                            vec!["resolved", mode, "resolved", "resolved", mode]
                        );
                    }
                    assert_eq!(
                        report["unavailable_header_count"],
                        if mode == "noncanonical-self" {
                            0
                        } else if ["null", "missing"].contains(&mode) {
                            1
                        } else {
                            2
                        }
                    );
                }
            }
            assert_eq!(report["runtime_ready"], false);
            assert_eq!(report["region_effects_applied"], false);
        }
        let root = directory();
        region_fixture(&root, "valid");
        for key in ["Base.esm:999", "Base.esm:200"] {
            let report = regions(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(key).unwrap(),
            )
            .unwrap();
            assert_eq!(report["source_request_prepared"], false);
            assert!(report["region_sources"].is_null());
        }
    }
    #[test]
    fn cli_regions_require_an_explicit_canonical_cell() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "cell-region-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json"
            ])
            .is_err()
        );
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "cell-region-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--cell",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::CellRegionSources { .. }
        ));
    }

    fn environment_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let mut cell = field(b"DATA", &[1]);
        match mode {
            "absent" => {}
            "null" | "missing" => cell.extend(field(
                b"XCCM",
                &(if mode == "null" { 0_u32 } else { 0x999 }).to_le_bytes(),
            )),
            "unsupported" => cell.extend(field(b"XEZN", &[0; 3])),
            "truncated" => cell.extend(b"XCCM\x04\0\0"),
            _ => {
                cell.extend(field(b"XCMO", &[4, 2, 0, 0]));
                cell.extend(field(b"ZZZZ", &[91, 92]));
                cell.extend(field(b"XCCM", &[0, 2, 0, 0]));
                cell.extend(field(b"XEZN", &[2, 2, 0, 0]));
                cell.extend(field(b"XCIM", &[1, 2, 0, 0]));
                cell.extend(field(b"XCAS", &[3, 2, 0, 0]));
            }
        }
        if mode == "duplicate" {
            cell.extend(field(b"XCCM", &[0; 4]));
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        let mut base = [record(b"TES4", 0, &hedr), record(b"CELL", 0x100, &cell)].concat();
        for (index, kind) in [b"CLMT", b"IMGS", b"ECZN", b"ASPC", b"MUSC"]
            .into_iter()
            .enumerate()
        {
            base.extend(record(
                kind,
                0x200 + index as u32,
                &field(b"ZZZZ", &[index as u8]),
            ));
        }
        fs::write(root.join("Data/Base.esm"), base).unwrap();
        let patch_header = [hedr, field(b"MAST", b"Base.esm\0"), field(b"DATA", &[0; 8])].concat();
        let mut target = record(
            if mode == "wrong-record-kind" {
                b"MUSC"
            } else {
                b"CLMT"
            },
            0x200,
            &[1, 2, 3],
        );
        if mode == "deleted" {
            target[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
        }
        fs::write(
            root.join("Data/Patch.esp"),
            [record(b"TES4", 0, &patch_header), target].concat(),
        )
        .unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("environment-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_ENVIRONMENT_FIXTURE={}", root.display());
    }
    fn environment_report(root: &Path) -> Value {
        environment(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key("Base.esm:100").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_environment_preserves_exact_source_order_and_winning_header_inputs_per_field() {
        let root = directory();
        environment_fixture(&root, "valid");
        let report = environment_report(&root);
        assert_eq!(report["source_request_prepared"], true);
        assert!(report["source_error"].is_null());
        let sources = &report["environment_sources"];
        assert_eq!(sources["cell"]["header"]["offset"], 42);
        assert_eq!(sources["usage"]["read_bytes"], 65);
        let rows = sources["links"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        for (row, (kind, raw, ordinal, offset)) in rows.iter().zip([
            (b"XCMO", 0x204_u32, 1, 7),
            (b"XCCM", 0x200, 3, 25),
            (b"XEZN", 0x202, 4, 35),
            (b"XCIM", 0x201, 5, 45),
            (b"XCAS", 0x203, 6, 55),
        ]) {
            assert_eq!(row["kind"], json!(kind));
            assert_eq!(row["raw"], raw);
            assert_eq!(row["physical_field_ordinal"], ordinal);
            assert_eq!(row["logical_field_ordinal"], ordinal);
            assert_eq!(row["site"]["decoded_header_offset"], offset);
            assert_eq!(row["physical_framing_offset"], 66 + offset);
            assert_eq!(row["target"]["key"]["local_id"], raw);
            assert_eq!(row["target"]["status"], "resolved");
            assert_eq!(row["header_source_available"], true);
            assert_eq!(
                row["behavior_status"],
                "unknown; target body and behavior not decoded"
            );
        }
        assert_eq!(rows[1]["source"]["source_ordinal"], 1);
        assert_eq!(rows[1]["source"]["header"]["offset"], 71);
        let statuses = report["field_statuses"].as_array().unwrap();
        assert_eq!(statuses.len(), 5);
        for status in statuses {
            assert_eq!(status["declared"], true);
            assert_eq!(status["header_source_available"], true);
        }
        for flag in [
            "target_behavior_decoded",
            "world_parent_evaluated",
            "environment_selected",
            "runtime_ready",
            "retail_parity_accepted",
        ] {
            assert_eq!(report[flag], false);
        }
    }
    #[test]
    fn cli_environment_absent_null_and_unavailable_rows_are_not_factory_successful_behavior() {
        for mode in [
            "absent",
            "null",
            "missing",
            "deleted",
            "wrong-record-kind",
            "duplicate",
            "unsupported",
            "truncated",
        ] {
            let root = directory();
            environment_fixture(&root, mode);
            if mode == "truncated" {
                assert!(
                    environment(
                        &root,
                        &root.join("order.json"),
                        None,
                        crate::parse_cell_key("Base.esm:100").unwrap()
                    )
                    .is_err()
                );
                continue;
            }
            let report = environment_report(&root);
            let refused = ["duplicate", "unsupported"].contains(&mode);
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["environment_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if !refused {
                let climate = &report["field_statuses"][0];
                assert_eq!(climate["kind"], "XCCM");
                assert_eq!(climate["declared"], mode != "absent");
                assert_eq!(climate["header_source_available"], false);
                assert_eq!(climate["resolution_status"], mode);
                if mode == "absent" {
                    assert!(
                        report["environment_sources"]["links"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                }
            }
            assert_eq!(report["environment_selected"], false);
            assert_eq!(report["runtime_ready"], false);
        }
        let root = directory();
        environment_fixture(&root, "valid");
        for key in ["Base.esm:999", "Base.esm:200"] {
            let report = environment(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(key).unwrap(),
            )
            .unwrap();
            assert_eq!(report["source_request_prepared"], false);
            assert!(report["environment_sources"].is_null());
        }
    }
    #[test]
    fn cli_environment_requires_an_explicit_canonical_cell() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "cell-environment-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json"
            ])
            .is_err()
        );
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "cell-environment-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--cell",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::CellEnvironmentSources { .. }
        ));
    }

    fn water_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let word = if mode == "negative-zero" {
            0x80000000_u32
        } else {
            0x7fc12345
        };
        let mut cell = field(b"DATA", &[2]);
        if mode != "absent" {
            cell.extend(field(b"XCLW", &word.to_le_bytes()));
            cell.extend(field(
                b"XCWT",
                &(if mode == "missing-type" {
                    0x999_u32
                } else {
                    0x200
                })
                .to_le_bytes(),
            ));
            let path = match mode {
                "empty-noise" => b"\0".as_slice(),
                "missing-noise" => b"missing.dds\0",
                "unsafe" => b"..\\noise.dds\0",
                _ => b"Noise.dds\0",
            };
            cell.extend(field(b"XNAM", path));
        }
        if mode == "duplicate" {
            cell.extend(field(b"XCLW", &[0; 4]));
        }
        fs::write(
            root.join("Data/Base.esm"),
            [
                record(
                    b"TES4",
                    0,
                    &field(
                        b"HEDR",
                        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                    ),
                ),
                record(b"CELL", 0x100, &cell),
                record(b"WATR", 0x200, &field(b"DATA", &[3, 0])),
            ]
            .concat(),
        )
        .unwrap();
        archive(
            root,
            "noise",
            b"textures",
            b"noise.dds",
            &[17, 34, 51, 68, 85],
        );
        if mode == "ambiguous" {
            archive(root, "other-noise", b"textures", b"noise.dds", &[99]);
        }
        fs::write(root.join("order.json"), b"[\"Base.esm\"]").unwrap();
        fs::write(
            root.join("water-case.json"),
            serde_json::to_vec(&json!({"mode":mode})).unwrap(),
        )
        .unwrap();
        println!("WORLD_WATER_FIXTURE={}", root.display());
    }
    fn water_input() -> ResidencyInput {
        ResidencyInput {
            cell: crate::parse_cell_key("Base.esm:100").unwrap(),
            source_timeout_ms: 10_000,
        }
    }
    #[test]
    fn cli_water_consumes_exact_source_job_cache_and_releases_retained_bytes() {
        for mode in ["valid", "negative-zero", "empty-noise"] {
            let root = directory();
            water_fixture(&root, mode);
            let cache = directory();
            for reused in [false, true] {
                let report = water(
                    &root,
                    &root.join("order.json"),
                    None,
                    Some(&cache),
                    water_input(),
                )
                .unwrap();
                assert_eq!(report["source_request_prepared"], true);
                assert_eq!(report["source_inputs_available"], true);
                assert!(report["source_error"].is_null());
                let source = &report["water_sources"];
                assert_eq!(source["cell"]["header"]["offset"], 42);
                assert_eq!(source["cell_flags"]["value"], 2);
                assert_eq!(
                    source["xclw"]["value"],
                    if mode == "negative-zero" {
                        0x80000000_u32
                    } else {
                        0x7fc12345
                    }
                );
                assert_eq!(source["xclw"]["physical_framing_offset"], 73);
                assert_eq!(source["xcwt"]["site"]["decoded_header_offset"], 17);
                assert_eq!(source["water_type"]["target"]["key"]["local_id"], 0x200);
                assert_eq!(source["water_type"]["target"]["status"], "resolved");
                let requested = mode != "empty-noise";
                assert_eq!(report["noise_requested"], requested);
                assert_eq!(report["noise_available"], requested);
                if requested {
                    let payload = &report["noise_payload"];
                    assert_eq!(payload["bytes"], 5);
                    assert_eq!(
                        payload["sha256"],
                        format!("{:x}", Sha256::digest([17, 34, 51, 68, 85]))
                    );
                    assert_eq!(payload["cache"]["reused"], reused);
                    assert_eq!(payload["source_identity"], source["identity"]);
                    assert_eq!(
                        payload["cache"]["manifest"]["identity"]["path_bytes"],
                        json!(b"textures/noise.dds".as_slice())
                    );
                    assert_eq!(
                        report["noise_job_usage_while_retained"],
                        json!({"outstanding":1,"decoded_bytes":5})
                    );
                    assert_eq!(source["xnam"]["value"], json!(b"Noise.dds".as_slice()));
                } else {
                    assert!(report["noise_payload"].is_null());
                    assert_eq!(source["noise"]["status"], "empty-declaration");
                }
                assert_eq!(
                    report["noise_job_usage_after_release"],
                    json!({"outstanding":0,"decoded_bytes":0})
                );
                for flag in [
                    "finite_plane_computed",
                    "inheritance_evaluated",
                    "rendering_admitted",
                    "runtime_ready",
                    "lookup_precedence_verified",
                    "retail_parity_accepted",
                ] {
                    assert_eq!(report[flag], false);
                }
            }
        }
    }
    #[test]
    fn cli_water_unavailable_declarations_and_factory_refusals_stay_distinct() {
        for mode in [
            "absent",
            "missing-type",
            "missing-noise",
            "unsafe",
            "ambiguous",
            "duplicate",
        ] {
            let root = directory();
            water_fixture(&root, mode);
            let report = water(&root, &root.join("order.json"), None, None, water_input()).unwrap();
            let refused = ["unsafe", "ambiguous", "duplicate"].contains(&mode);
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["source_inputs_available"], false);
            assert_eq!(report["water_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if mode == "absent" {
                for field in ["xclw", "xcwt", "xnam", "water_type", "noise"] {
                    assert!(report["water_sources"][field].is_null());
                }
            }
            if mode == "missing-type" {
                assert_eq!(
                    report["water_sources"]["water_type"]["target"]["status"],
                    "missing"
                );
                assert_eq!(report["noise_available"], true);
            }
            if mode == "missing-noise" {
                assert_eq!(report["water_sources"]["noise"]["status"], "missing");
                assert_eq!(report["noise_requested"], false);
            }
            assert_eq!(report["runtime_ready"], false);
        }
        let root = directory();
        water_fixture(&root, "valid");
        for cell in ["Base.esm:999", "Base.esm:200"] {
            let mut input = water_input();
            input.cell = crate::parse_cell_key(cell).unwrap();
            let report = water(&root, &root.join("order.json"), None, None, input).unwrap();
            assert_eq!(report["source_request_prepared"], false);
            assert!(report["water_sources"].is_null());
        }
    }
    #[test]
    fn cli_water_requires_explicit_cell_and_bounds_polling_before_opening_sources() {
        use clap::Parser;
        assert!(
            crate::Args::try_parse_from([
                "fallout",
                "cell-water-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json"
            ])
            .is_err()
        );
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "cell-water-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--cell",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::CellWaterSources { .. }
        ));
        for timeout in [0, 120_001] {
            let mut input = water_input();
            input.source_timeout_ms = timeout;
            assert!(water(Path::new("absent"), Path::new("absent"), None, None, input).is_err());
        }
    }

    const LIGHTING_WORDS: [u8; 40] = [
        1, 2, 3, 241, 4, 5, 6, 242, 7, 8, 9, 243, 69, 35, 193, 127, 0, 0, 0, 128, 0, 0, 0, 128,
        255, 255, 255, 127, 0, 0, 192, 63, 0, 0, 128, 127, 0, 0, 0, 64,
    ];
    fn lighting_fixture(root: &Path, mode: &str) {
        fs::create_dir(root.join("Data")).unwrap();
        let prefix = match mode {
            "full" => 40,
            "unsupported" => 31,
            _ => 28,
        };
        let mut cell = field(b"DATA", &[1]);
        if mode != "absent" {
            cell.extend(field(b"XCLL", &LIGHTING_WORDS[..prefix]));
            cell.extend(field(
                b"LTMP",
                &(if mode == "null" { 0_u32 } else { 0x200 }).to_le_bytes(),
            ));
            cell.extend(field(b"LNAM", &[9, 1, 0, 128]));
        }
        if mode == "duplicate" {
            cell.extend(field(b"XCLL", &LIGHTING_WORDS[..28]));
        }
        let hedr = field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        );
        fs::write(
            root.join("Data/Base.esm"),
            [record(b"TES4", 0, &hedr), record(b"CELL", 0x100, &cell)].concat(),
        )
        .unwrap();
        let mut patch_header = hedr;
        patch_header.extend(field(b"MAST", b"Base.esm\0"));
        patch_header.extend(field(b"DATA", &[0; 8]));
        let mut patch = record(b"TES4", 0, &patch_header);
        if mode != "missing" {
            let body = if mode == "missing-data" {
                Vec::new()
            } else {
                field(b"DATA", &LIGHTING_WORDS)
            };
            let mut template = record(
                if mode == "wrong-record-kind" {
                    b"STAT"
                } else {
                    b"LGTM"
                },
                0x200,
                &body,
            );
            if mode == "deleted" {
                template[8..12].copy_from_slice(&fallout_data::plugin::DELETED.to_le_bytes());
            }
            patch.extend(template);
        }
        fs::write(root.join("Data/Patch.esp"), patch).unwrap();
        fs::write(root.join("order.json"), b"[\"Base.esm\",\"Patch.esp\"]").unwrap();
        fs::write(
            root.join("lighting-case.json"),
            serde_json::to_vec(&json!({"mode":mode,"prefix":prefix})).unwrap(),
        )
        .unwrap();
        println!("WORLD_LIGHTING_FIXTURE={}", root.display());
    }
    fn lighting_report(root: &Path) -> Value {
        lighting(
            root,
            &root.join("order.json"),
            None,
            crate::parse_cell_key("Base.esm:100").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn cli_lighting_retains_literal_short_and_full_inputs_and_winning_template() {
        for (mode, prefix) in [("short", 28), ("full", 40)] {
            let root = directory();
            lighting_fixture(&root, mode);
            let report = lighting_report(&root);
            assert_eq!(report["source_request_prepared"], true);
            assert_eq!(report["source_inputs_available"], true);
            assert!(report["source_error"].is_null());
            let sources = &report["lighting_sources"];
            assert_eq!(sources["cell"]["key"]["local_id"], 0x100);
            assert_eq!(sources["cell"]["header"]["offset"], 42);
            assert_eq!(sources["cell_flags"]["value"], 1);
            assert_eq!(sources["xcll"]["site"]["decoded_header_offset"], 7);
            assert_eq!(sources["xcll"]["site"]["span"]["decoded_offset"], 13);
            assert_eq!(sources["xcll"]["site"]["span"]["bytes"], prefix);
            assert_eq!(sources["xcll"]["physical_framing_offset"], 73);
            assert_eq!(
                sources["xcll"]["framing"],
                json!(field(b"XCLL", &LIGHTING_WORDS[..prefix]))
            );
            assert_eq!(
                sources["xcll"]["value"]["ambient"],
                json!({"red":1,"green":2,"blue":3,"unused":241})
            );
            assert_eq!(sources["xcll"]["value"]["fog_near_word"], 0x7fc12345_u32);
            assert_eq!(sources["xcll"]["value"]["fog_far_word"], 0x80000000_u32);
            assert_eq!(sources["xcll"]["value"]["rotation_xy"], i32::MIN);
            assert_eq!(sources["xcll"]["value"]["rotation_z"], i32::MAX);
            assert_eq!(
                sources["xcll"]["value"]["directional_fade_word"].is_null(),
                prefix == 28
            );
            assert_eq!(sources["lnam"]["value"], 0x80000109_u32);
            assert_eq!(
                sources["ltmp"]["site"]["decoded_header_offset"],
                13 + prefix
            );
            assert_eq!(sources["template"]["input_status"], "resolved");
            assert_eq!(sources["template"]["source"]["source_ordinal"], 1);
            assert_eq!(sources["template"]["source"]["header"]["offset"], 71);
            assert_eq!(
                sources["template"]["lighting"]["physical_framing_offset"],
                95
            );
            assert_eq!(
                sources["template"]["lighting"]["value"]["fog_power_word"],
                0x40000000_u32
            );
            for flag in [
                "inheritance_evaluated",
                "rendering_admitted",
                "runtime_ready",
                "retail_parity_accepted",
            ] {
                assert_eq!(report[flag], false);
            }
        }
    }
    #[test]
    fn cli_lighting_missing_status_and_factory_refusal_stay_distinct() {
        for mode in [
            "null",
            "missing",
            "deleted",
            "wrong-record-kind",
            "missing-data",
            "absent",
            "duplicate",
            "unsupported",
        ] {
            let root = directory();
            lighting_fixture(&root, mode);
            let report = lighting_report(&root);
            let refused = ["duplicate", "unsupported"].contains(&mode);
            assert_eq!(report["source_request_prepared"], !refused);
            assert_eq!(report["source_inputs_available"], mode == "null");
            assert_eq!(report["lighting_sources"].is_null(), refused);
            assert_eq!(report["source_error"].is_null(), !refused);
            if !refused && mode != "absent" {
                assert_eq!(report["lighting_sources"]["template"]["input_status"], mode);
            }
            if mode == "absent" {
                for field in ["xcll", "ltmp", "lnam", "template"] {
                    assert!(report["lighting_sources"][field].is_null());
                }
            }
            assert_eq!(report["runtime_ready"], false);
        }
        let root = directory();
        lighting_fixture(&root, "full");
        for cell in ["Base.esm:999", "Base.esm:200"] {
            let report = lighting(
                &root,
                &root.join("order.json"),
                None,
                crate::parse_cell_key(cell).unwrap(),
            )
            .unwrap();
            assert_eq!(report["source_request_prepared"], false);
            assert_eq!(report["source_inputs_available"], false);
            assert!(report["lighting_sources"].is_null());
            assert!(report["source_error"].is_string());
        }
    }
    #[test]
    fn cli_lighting_requires_canonical_cell_input() {
        use clap::Parser;
        for arguments in [
            vec![
                "fallout",
                "cell-lighting-sources",
                "--install",
                "authored",
                "--load-order",
                "order.json",
            ],
            vec![
                "fallout",
                "cell-lighting-sources",
                "--install",
                "authored",
                "--cell",
                "Base.esm:100",
            ],
        ] {
            assert!(crate::Args::try_parse_from(arguments).is_err());
        }
        let parsed = crate::Args::try_parse_from([
            "fallout",
            "cell-lighting-sources",
            "--install",
            "authored",
            "--load-order",
            "order.json",
            "--cell",
            "Base.esm:100",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            crate::Command::CellLightingSources { .. }
        ));
    }
}
