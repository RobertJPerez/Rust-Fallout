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

#[derive(Default)]
pub(super) struct Options {
    pub(super) include_associations: bool,
    pub(super) include_classes: bool,
    pub(super) include_factions: bool,
    pub(super) include_placements: bool,
    pub(super) include_races: bool,
    pub(super) include_packages: bool,
    pub(super) include_package_dependencies: bool,
    pub(super) include_dependencies: bool,
    pub(super) dependency_roots: Vec<FormKey>,
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

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    options: Options,
) -> Result<Value> {
    if options.include_package_dependencies && !options.include_packages {
        return Err("package dependencies require --include-packages".into());
    }
    if options.dependency_roots.len() > 64 {
        return Err("actor dependency root budget exceeds 64".into());
    }
    if !options.include_dependencies && !options.dependency_roots.is_empty() {
        return Err("actor dependency roots require --include-dependencies".into());
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
    let associations = if options.include_associations || options.include_dependencies {
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
    if options.include_races {
        let races = actors::races::Catalogue::load(&mut store, actors::races::Limits::default())?;
        report["actor_races"] = json!({"counts":races.counts(),"definitions":races.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored RACE scalar inputs, no race or FaceGen application",
            report["scope"].as_str().unwrap_or_default()
        ));
    }
    if options.include_packages {
        let packages =
            actors::packages::Catalogue::load(&mut store, actors::packages::Limits::default())?;
        report["actor_packages"] = json!({"counts":packages.counts(),"definitions":packages.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(format!(
            "{}; authored PACK scalar inputs, no scheduling, conditions or AI execution",
            report["scope"].as_str().unwrap_or_default()
        ));
        if options.include_package_dependencies {
            let descriptors = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
            let signatures: condition_operands::Signatures = descriptors
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
                .collect();
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
        // One aggregate admission budget covers every requested root report.
        let mut remaining = actors::dependencies::ManifestLimits::default();
        let mut manifests = Vec::new();
        for root in &options.dependency_roots {
            let manifest = dependencies.manifest(root, &assets, remaining)?;
            let counts = &manifest.counts;
            remaining.max_nodes -= counts.nodes;
            remaining.max_edges -= counts.inventory_edges + counts.model_edges;
            remaining.max_field_visits -= counts.field_visits;
            remaining.max_paths -= counts.paths;
            remaining.max_path_bytes -= counts.path_bytes;
            remaining.max_candidates -= counts.candidates;
            remaining.max_candidate_bytes -= counts.candidate_bytes;
            manifests.push(manifest);
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
    if report.get("actor_package_dependencies").is_some()
        && report.get("actor_package_dependencies") != oracle.get("actor_package_dependencies")
    {
        return Err(
            "independent actor source comparison differs in actor_package_dependencies".into(),
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
