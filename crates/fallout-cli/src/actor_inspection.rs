//! Immutable source scalars over the existing inventory production load path.
use super::{Result, command_catalogue, inspection_input::Order};
use fallout_data::{
    actors,
    assets::ArchiveAssets,
    baseline, condition_operands,
    identity::{self, FormKey, ProfileId},
    inventory, leveled, loaded_scripts, record_metadata,
};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

fn condition_signatures(
    descriptors: &command_catalogue::Catalogue,
) -> condition_operands::Signatures {
    descriptors
        .script_commands
        .iter()
        .filter(|row| row.condition_handler_present)
        .map(|row| {
            (
                (row.id - 0x1000) as u16,
                condition_operands::Signature {
                    parameters: row
                        .parameters
                        .iter()
                        .map(|parameter| condition_operands::Parameter {
                            type_id: parameter.type_id,
                            optional_word: parameter.optional_word,
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

pub(super) struct ContextOptions<'a> {
    pub(super) actor_root: &'a FormKey,
    pub(super) snapshot: &'a Path,
    pub(super) explicit_subject: Option<std::num::NonZeroU64>,
    pub(super) engineering_observation: bool,
    pub(super) condition_executable: Option<&'a Path>,
    pub(super) include_faction_requests: bool,
    pub(super) include_stat_requests: bool,
    pub(super) package_capability: Option<fallout_runtime::actor_rules::packages::Operation>,
    pub(super) include_actor_context: bool,
    pub(super) equipment_item: Option<std::num::NonZeroU64>,
}

/// Restore the existing canonical snapshot, then make read-only host requests.
pub(super) fn package_context(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    options: ContextOptions<'_>,
) -> Result<Value> {
    use fallout_runtime::{
        World, actor_rules::packages, execution::condition, foreign::Content, snapshot::Snapshot,
    };
    let limits = fallout_runtime::Limits::default();
    let mut snapshot_source = baseline::open_source(options.snapshot)?;
    let mut snapshot_bytes = Vec::new();
    (&mut snapshot_source)
        .take(limits.max_snapshot_bytes as u64 + 1)
        .read_to_end(&mut snapshot_bytes)?;
    let snapshot = Snapshot::decode(&snapshot_bytes, limits)?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let scripts = loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(()))?;
    let world = World::restore(&scripts, snapshot, limits)?;
    let before_observation = (options.include_actor_context || options.equipment_item.is_some())
        .then(|| world.snapshot());
    let content = Content::load(&mut store, &scripts, 2_000_000)?;
    let inventory = inventory::Catalogue::load(&mut store, Default::default())?;
    let actors = actors::Catalogue::load(&inventory, Default::default())?;
    let associations =
        actors::associations::Catalogue::load(&mut store, &actors, Default::default())?;
    let package_sources = actors::packages::Catalogue::load(&mut store, Default::default())?;
    let executable = install.join("FalloutNV.exe");
    let descriptors =
        command_catalogue::inspect(options.condition_executable.unwrap_or(&executable))?;
    let dependencies = actors::package_dependencies::Catalogue::load(
        &mut store,
        &package_sources,
        &scripts,
        &condition_signatures(&descriptors),
        Default::default(),
    )?;
    let requests = packages::Requests::prepare(
        &world,
        &actors,
        &associations,
        &dependencies,
        options.actor_root,
        packages::Limits::default(),
    )?;
    let intent = if options.engineering_observation {
        condition::Intent::EngineeringObservation
    } else {
        condition::Intent::Faithful
    };
    let observation = requests.observe(
        &world,
        &content,
        options
            .explicit_subject
            .map(fallout_runtime::identity::ReferenceId),
        intent,
        packages::Limits::default(),
    )?;
    let (snapshot_bytes, snapshot_sha256) = baseline::digest_file(options.snapshot)?;
    let mut report = json!({"schema_version":1,"profile":"nv-original",
        "snapshot_input":{"bytes":snapshot_bytes,"sha256":snapshot_sha256},
        "descriptor_receipt":{"source_bytes":descriptors.source_bytes,"source_sha256":descriptors.source_sha256,
            "source_version_profile":descriptors.source_version_profile},
        "observation":observation,"state_changed":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    if options.include_faction_requests {
        use fallout_runtime::actor_rules::factions;
        let faction_sources = actors::factions::Catalogue::load(&mut store, Default::default())?;
        let faction_requests = factions::Requests::prepare(
            &world,
            &actors,
            &associations,
            &faction_sources,
            options.actor_root,
            factions::Limits::default(),
        )?;
        report["faction_requests"] = serde_json::to_value(
            faction_requests.observe(
                &world,
                &content,
                options
                    .explicit_subject
                    .map(fallout_runtime::identity::ReferenceId),
                factions::Limits::default(),
            )?,
        )?;
    }
    if options.include_stat_requests {
        use fallout_runtime::actor_rules::stats;
        let lists = leveled::Catalogue::load(&mut store, Default::default())?;
        let sources = actors::dependencies::Catalogue::load(
            &mut store,
            &actors,
            &associations,
            &lists,
            Default::default(),
        )?;
        let requests = stats::Requests::prepare(
            &world,
            &actors,
            &sources,
            options.actor_root,
            stats::Limits::default(),
        )?;
        report["stat_requests"] =
            serde_json::to_value(requests.observe(&world, stats::Limits::default())?)?;
    }
    if let Some(operation) = options.package_capability {
        let capability = requests.capability(
            &world,
            &content,
            options
                .explicit_subject
                .map(fallout_runtime::identity::ReferenceId),
            operation,
            packages::CapabilityLimits::default(),
        )?;
        capability
            .require_execution()
            .expect_err("package execution is unsupported");
        report["package_capability"] = serde_json::to_value(capability)?;
    }
    if options.include_actor_context {
        let reference = options
            .explicit_subject
            .map(fallout_runtime::identity::ReferenceId)
            .ok_or("actor context requires an explicit canonical subject")?;
        let placements = actors::placements::Catalogue::load(&mut store, Default::default())?;
        let observation = fallout_runtime::actor_rules::context::observe(
            &world,
            &content,
            &placements,
            &actors,
            reference,
            Default::default(),
        )?;
        if observation.actor.key != options.actor_root {
            return Err("canonical actor placement base differs from --actor-root".into());
        }
        report["actor_context"] = serde_json::to_value(observation)?;
        if before_observation.as_ref() != Some(&world.snapshot()) {
            return Err("actor context changed canonical state".into());
        }
    }
    if let Some(item) = options.equipment_item {
        let owner = options
            .explicit_subject
            .map(fallout_runtime::identity::ReferenceId)
            .ok_or("equipment item requires an explicit canonical owner")?;
        let selection = fallout_runtime::actor_rules::equipment::observe(
            &world,
            &content,
            owner,
            fallout_runtime::inventory::ItemId(item),
            Default::default(),
        )?;
        report["equipment_item"] = serde_json::to_value(selection)?;
    }
    if before_observation
        .as_ref()
        .is_some_and(|before| before != &world.snapshot())
    {
        return Err("actor item/context observation changed canonical state".into());
    }
    Ok(report)
}

pub(super) fn parse_package_operation(
    raw: &str,
) -> std::result::Result<fallout_runtime::actor_rules::packages::Operation, String> {
    use fallout_runtime::actor_rules::packages::Operation;
    match raw {
        "eligibility" => Ok(Operation::Eligibility),
        "selection" => Ok(Operation::Selection),
        "scheduling" => Ok(Operation::Scheduling),
        _ => Err("expected eligibility, selection or scheduling".into()),
    }
}

#[derive(Default)]
pub(super) struct Options {
    pub(super) include_associations: bool,
    pub(super) include_classes: bool,
    pub(super) include_factions: bool,
    pub(super) include_placements: bool,
    pub(super) include_races: bool,
    pub(super) include_packages: bool,
    pub(super) include_package_dependencies: bool,
    pub(super) package_destination: Option<FormKey>,
    pub(super) include_dependencies: bool,
    pub(super) include_render_dependencies: bool,
    pub(super) include_template_dependencies: bool,
    pub(super) dependency_roots: Vec<FormKey>,
    pub(super) equipment_source: Option<FormKey>,
    pub(super) equipment_role: Option<actors::dependencies::equipment::Role>,
    pub(super) voice_root: Option<FormKey>,
    pub(super) creature_model_directory: Option<fallout_data::vfs::AssetPath>,
}

pub(super) fn parse_equipment_role(
    raw: &str,
) -> std::result::Result<actors::dependencies::equipment::Role, String> {
    use actors::dependencies::{Sex, equipment::Role};
    Ok(match raw {
        "armor-male-biped" => Role::ArmorBiped { sex: Sex::Male },
        "armor-female-biped" => Role::ArmorBiped { sex: Sex::Female },
        "armor-male-world" => Role::ArmorWorld { sex: Sex::Male },
        "armor-female-world" => Role::ArmorWorld { sex: Sex::Female },
        "weapon-shell" => Role::WeaponShell,
        "weapon-scope" => Role::WeaponScope,
        "weapon-world" => Role::WeaponWorld,
        _ => {
            let (role, mask)=raw.split_once(':').ok_or("expected armor-sex-biped/world, weapon-shell/scope/world or weapon-model/first-person:0..7")?;
            let mod_mask = mask
                .parse::<u8>()
                .ok()
                .filter(|mask| *mask <= 7)
                .ok_or("weapon source mask requires 0..7")?;
            match role {
                "weapon-model" => Role::WeaponModel { mod_mask },
                "weapon-first-person" => Role::WeaponFirstPerson { mod_mask },
                _ => return Err("unknown equipment source role".into()),
            }
        }
    })
}

pub(super) fn parse_root(raw: &str) -> std::result::Result<FormKey, String> {
    let (origin, local) = raw
        .split_once(':')
        .ok_or("expected ORIGIN_PLUGIN:LOCAL_HEX_ID")?;
    let local_id =
        u32::from_str_radix(local.trim_start_matches("0x"), 16).map_err(|e| e.to_string())?;
    if local_id == 0 || local_id > 0x00ff_ffff {
        return Err(
            "actor dependency root requires a nonzero local ID without load-order bits".into(),
        );
    }
    Ok(FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: identity::plugin_name(origin).map_err(|e| e.to_string())?,
        local_id,
    })
}

pub(super) fn parse_creature_directory(
    raw: &str,
) -> std::result::Result<fallout_data::vfs::AssetPath, String> {
    let path = fallout_data::vfs::AssetPath::new(raw.as_bytes()).map_err(|e| e.to_string())?;
    if !path.bytes().starts_with(b"meshes/") || path.bytes().len() > 4096 {
        return Err(
            "creature model directory must be rooted beneath meshes/ and at most 4096 bytes".into(),
        );
    }
    Ok(path)
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    options: Options,
) -> Result<Value> {
    if options.include_package_dependencies && !options.include_packages {
        return Err("package dependencies require --include-packages".into());
    }
    if options.package_destination.is_some() && !options.include_packages {
        return Err("package destination requires --include-packages".into());
    }
    if (options.equipment_source.is_some() || options.equipment_role.is_some())
        && (!options.include_dependencies
            || options.dependency_roots.len() != 1
            || options.equipment_source.is_none()
            || options.equipment_role.is_none())
    {
        return Err(
            "equipment requests require both source and role and exactly one dependency root"
                .into(),
        );
    }
    if options.dependency_roots.len() > 64 {
        return Err("actor dependency root budget exceeds 64".into());
    }
    if options.creature_model_directory.is_some()
        && (!options.include_dependencies || options.dependency_roots.len() != 1)
    {
        return Err("creature model directory requires exactly one dependency root and --include-dependencies".into());
    }
    if !options.include_dependencies && !options.dependency_roots.is_empty() {
        return Err("actor dependency roots require --include-dependencies".into());
    }
    if options.include_render_dependencies
        && (!options.include_dependencies || options.dependency_roots.is_empty())
    {
        return Err(
            "render dependencies require --include-dependencies and an explicit --dependency-root"
                .into(),
        );
    }
    if options.include_template_dependencies
        && (!options.include_dependencies || options.dependency_roots.is_empty())
    {
        return Err("template dependencies require --include-dependencies and an explicit --dependency-root".into());
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let inventory = inventory::Catalogue::load(&mut store, inventory::Limits::default())?;
    let catalogue = actors::Catalogue::load(&inventory, actors::Limits::default())?;
    let definitions = catalogue
        .iter()
        .map(|(_, definition)| definition)
        .collect::<Vec<_>>();
    let mut report = json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources(),
        "metadata":metadata,"winning_content_sha256":catalogue.winning_content_sha256(),
        "counts":catalogue.counts(),"definitions":definitions,"index_cache":store.index_cache_report(),
        "scope":"Exact authored NPC_/CREA scalar source fields joined to existing inventory provenance; no actor initialization, inheritance, automatic statistics or runtime conversion",
        "actors_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    let associations = if options.include_associations
        || options.include_dependencies
        || options.voice_root.is_some()
    {
        Some(actors::associations::Catalogue::load(
            &mut store,
            &catalogue,
            actors::associations::Limits::default(),
        )?)
    } else {
        None
    };
    if options.include_associations {
        let associations = associations
            .as_ref()
            .expect("requested associations loaded");
        report["actor_associations"] = json!({"counts":associations.counts(),"definitions":associations.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(
            "Exact authored NPC_/CREA scalar fields and ordered source associations; no inheritance, initialization, effect, faction or AI execution"
        );
    }
    if options.include_classes {
        let classes =
            actors::classes::Catalogue::load(&mut store, actors::classes::Limits::default())?;
        report["actor_classes"] = json!({"counts":classes.counts(),"definitions":classes.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored CLAS DATA/ATTR inputs, no class application",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    if options.include_factions {
        let factions =
            actors::factions::Catalogue::load(&mut store, actors::factions::Limits::default())?;
        report["actor_factions"] = json!({"counts":factions.counts(),"definitions":factions.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored FACT inputs, no faction state initialization",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    if options.include_placements {
        let placements =
            actors::placements::Catalogue::load(&mut store, actors::placements::Limits::default())?;
        report["actor_placements"] = json!({"counts":placements.counts(),"definitions":placements.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored placed actor inputs using the existing world decoder, no actor initialization",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    let races = if options.include_races || options.voice_root.is_some() {
        Some(actors::races::Catalogue::load(
            &mut store,
            actors::races::Limits::default(),
        )?)
    } else {
        None
    };
    if options.include_races {
        let races = races.as_ref().expect("requested races loaded");
        report["actor_races"] = json!({"counts":races.counts(),"definitions":races.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored RACE scalar inputs, no race or FaceGen application",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    if let Some(root) = &options.voice_root {
        let voices = actors::voices::request(
            &mut store,
            &catalogue,
            associations.as_ref().expect("voice associations loaded"),
            races.as_ref().expect("voice races loaded"),
            root,
            Default::default(),
        )?;
        report["actor_voice_requests"] = json!({"manifest":voices});
    }
    if options.include_packages {
        let packages =
            actors::packages::Catalogue::load(&mut store, actors::packages::Limits::default())?;
        report["actor_packages"] = json!({"counts":packages.counts(),"definitions":packages.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        if let Some(root) = &options.package_destination {
            let destination = actors::packages::destinations::request(
                &mut store,
                &packages,
                root,
                Default::default(),
            )?;
            report["actor_package_destination"] = json!({"manifest":destination});
        }
        report["scope"] = json!(format!(
            "{}; authored PACK scalar inputs, no scheduling, conditions or AI execution",
            report["scope"].as_str().unwrap_or_default()
        ));
        if options.include_package_dependencies {
            let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
            let signatures = condition_signatures(&descriptors);
            let scripts =
                loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(()))?;
            let dependencies = actors::package_dependencies::Catalogue::load(
                &mut store,
                &packages,
                &scripts,
                &signatures,
                Default::default(),
            )?;
            let descriptor_receipt = json!({
                "source_bytes": descriptors.source_bytes,
                "source_sha256": descriptors.source_sha256,
                "source_version_profile": descriptors.source_version_profile,
                "image_base": descriptors.image_base,
                "pe_timestamp": descriptors.pe_timestamp,
                "condition_descriptors": descriptors.script_commands.iter()
                    .filter(|row| row.condition_handler_present)
                    .map(|row| json!({"function_id": row.id - 0x1000,
                        "descriptor_file_offset": row.descriptor_file_offset,
                        "parameters": row.parameters})).collect::<Vec<_>>(),
            });
            report["actor_package_dependencies"] = json!({
                "counts": dependencies.counts(),
                "definitions": dependencies.iter().map(|(_, definition)| definition).collect::<Vec<_>>(),
                "descriptor_receipt": descriptor_receipt,
            });
            report["scope"] = json!(format!(
                "{}; physical PACK CTDA and embedded script source dependencies with separate fingerprinted descriptor receipt; no grouping, condition truth, event ownership, script execution or AI",
                report["scope"].as_str().unwrap_or_default()
            ));
        }
    }
    if options.include_dependencies {
        let lists = leveled::Catalogue::load(&mut store, leveled::Limits::default())?;
        let dependencies = actors::dependencies::Catalogue::load(
            &mut store,
            &catalogue,
            associations
                .as_ref()
                .expect("dependency associations loaded"),
            &lists,
            Default::default(),
        )?;
        let assets = ArchiveAssets::open_nv(install)?;
        if let Some(directory) = &options.creature_model_directory {
            report["actor_creature_parts"] = json!({"manifest":dependencies.creature_parts_manifest(&options.dependency_roots[0], directory, &assets, Default::default())?});
        }
        if let (Some(equipment), Some(role)) = (&options.equipment_source, options.equipment_role) {
            let selected = actors::dependencies::equipment::request(
                &mut store,
                &catalogue,
                &options.dependency_roots[0],
                actors::dependencies::equipment::Choice {
                    equipment: equipment.clone(),
                    role,
                },
                &assets,
                Default::default(),
            )?;
            report["actor_equipment_dependencies"] = json!({"manifest":selected});
        }
        // One aggregate admission budget covers every requested root report.
        let mut remaining = actors::dependencies::ManifestLimits::default();
        let mut manifests = Vec::new();
        let mut render_manifests = Vec::new();
        let mut render_remaining = actors::dependencies::RenderLimits::default();
        let mut template_remaining = actors::dependencies::TemplateLimits::default();
        let mut template_manifests = Vec::new();
        for root in &options.dependency_roots {
            if options.include_template_dependencies {
                let template = dependencies.template_manifest(root, template_remaining)?;
                template_remaining.closure.max_nodes -= template.structural_closure.nodes.len();
                template_remaining.closure.max_edges -=
                    template.structural_closure.edge_indices.len();
                template_remaining.max_sources -= template.candidate_sources.len();
                template_remaining.max_links -= template.links.len();
                template_remaining.max_field_visits -= template.field_visits;
                template_remaining.max_issues -= template.issues.len();
                template_manifests.push(template);
            }
            let render = if options.include_render_dependencies {
                Some(dependencies.render_manifest(
                    root,
                    &assets,
                    actors::dependencies::RenderLimits {
                        manifest: remaining,
                        ..render_remaining
                    },
                )?)
            } else {
                None
            };
            let manifest = if let Some(render) = &render {
                &render.manifest
            } else {
                manifests.push(dependencies.manifest(root, &assets, remaining)?);
                manifests.last().expect("manifest just appended")
            };
            let counts = &manifest.counts;
            remaining.max_nodes -= counts.nodes;
            remaining.max_edges -= counts.inventory_edges + counts.model_edges;
            remaining.max_field_visits -= counts.field_visits;
            remaining.max_paths -= counts.paths;
            remaining.max_path_bytes -= counts.path_bytes;
            remaining.max_candidates -= counts.candidates;
            remaining.max_candidate_bytes -= counts.candidate_bytes;
            if let Some(render) = render {
                render_remaining.max_sources -= render.sources.len();
                render_remaining.max_requests -= render.requests.len();
                render_remaining.max_issues -= render.issues.len();
                render_remaining.max_visits -= render.visits;
                render_manifests.push(render);
            }
        }
        if options.include_render_dependencies {
            report["actor_render_dependencies"] = json!({"manifests":render_manifests});
            report["scope"] = json!(format!(
                "{}; selected authored actor render roles with explicit unsupported template/equipment selection",
                report["scope"].as_str().unwrap_or_default()
            ));
        }
        if options.include_template_dependencies {
            report["actor_template_dependencies"] = json!({"manifests":template_manifests});
            report["scope"] = json!(format!(
                "{}; pinned template category declaration requests and exact candidate origins, no effective inheritance",
                report["scope"].as_str().unwrap_or_default()
            ));
        }
        report["actor_dependencies"] = json!({"counts":dependencies.counts(),
            "definitions":dependencies.iter().map(|(_, definition)|definition).collect::<Vec<_>>(),
            "inventory_graph":dependencies.inventory_graph(), "manifests":manifests});
        report["scope"] = json!(format!(
            "{}; exact actor model/head-part/string inputs and bounded explicit-root archive candidates; no initialization, part/clip selection or resolved NIFZ/KFFZ relative base",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    Ok(report)
}

/// Compare complete projections with an external direct-source reader. Every
/// field (including opaque hashes), occurrence and provenance must agree.
pub(super) fn compare(report: &mut Value, oracle_path: &Path) -> Result<()> {
    let mut source = baseline::open_source(oracle_path)?;
    let mut bytes = Vec::new();
    (&mut source)
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("actor oracle report exceeds 256 MiB".into());
    }
    let oracle: Value = serde_json::from_slice(&bytes)?;
    for key in [
        "schema_version",
        "profile",
        "sources",
        "metadata",
        "winning_content_sha256",
        "counts",
        "definitions",
    ] {
        if oracle.get(key).is_none() || report.get(key) != oracle.get(key) {
            return Err(format!("independent actor source comparison differs in {key}").into());
        }
    }
    if report.get("actor_associations").is_some()
        && report.get("actor_associations") != oracle.get("actor_associations")
    {
        return Err("independent actor source comparison differs in actor_associations".into());
    }
    if report.get("actor_classes").is_some()
        && report.get("actor_classes") != oracle.get("actor_classes")
    {
        return Err("independent actor source comparison differs in actor_classes".into());
    }
    if report.get("actor_factions").is_some()
        && report.get("actor_factions") != oracle.get("actor_factions")
    {
        return Err("independent actor source comparison differs in actor_factions".into());
    }
    if report.get("actor_placements").is_some()
        && report.get("actor_placements") != oracle.get("actor_placements")
    {
        return Err("independent actor source comparison differs in actor_placements".into());
    }
    if report.get("actor_races").is_some() && report.get("actor_races") != oracle.get("actor_races")
    {
        return Err("independent actor source comparison differs in actor_races".into());
    }
    if report.get("actor_packages").is_some()
        && report.get("actor_packages") != oracle.get("actor_packages")
    {
        return Err("independent actor source comparison differs in actor_packages".into());
    }
    if report.get("actor_dependencies").is_some()
        && report.get("actor_dependencies") != oracle.get("actor_dependencies")
    {
        return Err("independent actor source comparison differs in actor_dependencies".into());
    }
    if report.get("actor_render_dependencies").is_some()
        && report.get("actor_render_dependencies") != oracle.get("actor_render_dependencies")
    {
        return Err(
            "independent actor source comparison differs in actor_render_dependencies".into(),
        );
    }
    if report.get("actor_template_dependencies").is_some()
        && report.get("actor_template_dependencies") != oracle.get("actor_template_dependencies")
    {
        return Err(
            "independent actor source comparison differs in actor_template_dependencies".into(),
        );
    }
    if report.get("actor_voice_requests").is_some()
        && report.get("actor_voice_requests") != oracle.get("actor_voice_requests")
    {
        return Err("independent actor source comparison differs in actor_voice_requests".into());
    }
    if report.get("actor_equipment_dependencies").is_some()
        && report.get("actor_equipment_dependencies") != oracle.get("actor_equipment_dependencies")
    {
        return Err(
            "independent actor source comparison differs in actor_equipment_dependencies".into(),
        );
    }
    if report.get("actor_creature_parts").is_some()
        && report.get("actor_creature_parts") != oracle.get("actor_creature_parts")
    {
        return Err("independent actor source comparison differs in actor_creature_parts".into());
    }
    if report.get("actor_package_dependencies").is_some()
        && report.get("actor_package_dependencies") != oracle.get("actor_package_dependencies")
    {
        return Err(
            "independent actor source comparison differs in actor_package_dependencies".into(),
        );
    }
    if report.get("actor_package_destination").is_some()
        && report.get("actor_package_destination") != oracle.get("actor_package_destination")
    {
        return Err(
            "independent actor source comparison differs in actor_package_destination".into(),
        );
    }
    let (oracle_bytes, oracle_sha256) = baseline::digest_file(oracle_path)?;
    report["independent_comparison"] = json!({"equal":true,"oracle_bytes":oracle_bytes,
        "oracle_sha256":oracle_sha256,"records_checked":report["counts"]["records"],
        "fields_checked":report["counts"]["fields"],"scalar_fields_checked":report["counts"]["scalar_fields"],
        "scope":"Complete source projection against a separate direct plugin reader; no retail behavior acceptance"});
    if report.get("actor_associations").is_some() {
        report["independent_comparison"]["association_bindings_checked"] =
            report["actor_associations"]["counts"]["bindings"].clone();
    }
    if report.get("actor_classes").is_some() {
        report["independent_comparison"]["classes_checked"] =
            report["actor_classes"]["counts"]["records"].clone();
    }
    if report.get("actor_factions").is_some() {
        report["independent_comparison"]["factions_checked"] =
            report["actor_factions"]["counts"]["records"].clone();
    }
    if report.get("actor_placements").is_some() {
        report["independent_comparison"]["placements_checked"] =
            report["actor_placements"]["counts"]["records"].clone();
    }
    if report.get("actor_races").is_some() {
        report["independent_comparison"]["races_checked"] =
            report["actor_races"]["counts"]["records"].clone();
    }
    if report.get("actor_packages").is_some() {
        report["independent_comparison"]["packages_checked"] =
            report["actor_packages"]["counts"]["records"].clone();
    }
    if report.get("actor_package_dependencies").is_some() {
        report["independent_comparison"]["package_dependency_records_checked"] =
            report["actor_package_dependencies"]["counts"]["records"].clone();
    }
    if report.get("actor_dependencies").is_some() {
        report["independent_comparison"]["dependency_records_checked"] =
            report["actor_dependencies"]["counts"]["records"].clone();
        report["independent_comparison"]["dependency_roots_checked"] = json!(
            report["actor_dependencies"]["manifests"]
                .as_array()
                .map_or(0, Vec::len)
        );
    }
    Ok(())
}
