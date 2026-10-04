mod commands;
use commands::{
    ActorsCommand, AssetsCommand, Command, PhysicsCommand, RuntimeCommand, ScriptsCommand,
    SourcesCommand, WorldCommand,
};
mod actor_inspection;
mod argument_inspection;
mod collision;
mod command_catalogue;
mod compressed_record_inspection;
mod condition_dependency_inspection;
mod condition_inspection;
mod control_flow_inspection;
mod definition_plan_inspection;
mod dialogue_inspection;
mod event_frame_inspection;
mod event_operand_inspection;
mod expression_inspection;
mod expression_plan_inspection;
mod foreign_context_inspection;
mod form_list_inspection;
mod inspection_input;
mod inventory_inspection;
mod item_state_inspection;
mod leveled_inspection;
mod loaded_script_inspection;
mod narrative_inspection;
mod native_migration_inspection;
mod native_save_inspection;
mod navigation_inspection;
mod nif_animation_inspection;
mod nif_skin_inspection;
mod operand_inspection;
mod pe_image;
mod query_inspection;
mod quest_script_inspection;
#[path = "../../../tools/retail-profile/capture.rs"]
mod retail_profile;
mod script_profile;
mod script_state_inspection;
#[path = "../../../tools/retail-script-probes/runner.rs"]
mod script_trace;
mod shared_runtime_inspection;
mod source_item_inspection;
mod terrain_compare;
mod world_preparation_inspection;

use clap::Parser;
use fallout_data::{
    archive::NvArchive,
    baseline, content,
    identity::{self, ProfileId},
    plugin,
    vfs::MountIndex,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn cell_selector_preserves_origin_and_rejects_runtime_high_bytes() {
        let key = parse_cell_key("FalloutNV.esm:DAEB9").unwrap();
        assert_eq!(key.origin_plugin, "falloutnv.esm");
        assert_eq!(key.local_id, 0xDAEB9);
        for value in [
            "FalloutNV.esm:0",
            "FalloutNV.esm:010DAEB9",
            "../Base.esm:12",
            "Base.esm",
            "Base.esm:1:2",
        ] {
            assert!(parse_cell_key(value).is_err());
        }
    }
}

#[cfg(test)]
mod command_stack_tests {
    use super::Args;
    use clap::{CommandFactory, Parser};

    #[test]
    fn complete_cli_parses_on_a_one_mib_stack() {
        // Windows executables normally start with a 1 MiB stack. Growing the
        // command set must not require a larger test or application stack.
        std::thread::Builder::new()
            .name("cli-stack-bound".into())
            .stack_size(1024 * 1024)
            .spawn(|| {
                Args::command().debug_assert();
                let base = [
                    "fallout",
                    "script-state",
                    "--install",
                    "private-install",
                    "--load-order",
                    "order.json",
                ];
                assert!(Args::try_parse_from(base).is_ok());
                assert!(
                    Args::try_parse_from(base.into_iter().chain([
                        "--host-snapshot",
                        "snapshot.json",
                        "--host-requirements",
                        "requirements.json",
                    ]))
                    .is_ok()
                );
                assert!(
                    Args::try_parse_from(
                        base.into_iter()
                            .chain(["--host-snapshot", "snapshot.json",])
                    )
                    .is_err()
                );
                assert!(
                    Args::try_parse_from([
                        "fallout",
                        "nif-source-pose-set",
                        "source.nif",
                        "--request",
                        "request.json",
                    ])
                    .is_ok()
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[derive(Parser)]
#[command(
    name = "fallout",
    version,
    about = "Read-only Fallout content inspection. No gameplay parity is claimed."
)]
struct Args {
    /// Write JSON to a new file outside the input tree; otherwise use stdout.
    #[arg(long, global = true)]
    output: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

fn parse_cell_key(raw: &str) -> std::result::Result<identity::FormKey, String> {
    let (origin, local) = raw
        .split_once(':')
        .ok_or("expected ORIGIN_PLUGIN:LOCAL_HEX_ID")?;
    let local_id = parse_form(local)?;
    if local_id == 0 || local_id > 0x00ff_ffff {
        return Err("cell identity requires a nonzero local ID without load-order bits".into());
    }
    Ok(identity::FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: identity::plugin_name(origin).map_err(|error| error.to_string())?,
        local_id,
    })
}

fn parse_form(raw: &str) -> std::result::Result<u32, String> {
    u32::from_str_radix(raw.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    // Clap's generated command builder has a large debug frame as the inspector
    // grows. Windows gives the initial thread a smaller stack than our tool needs.
    // Keep this explicit allowance in the CLI, away from simulation and parsers.
    let worker = std::thread::Builder::new()
        .name("fallout-inspector".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| execute(Args::parse()));
    match worker {
        Ok(worker) => match worker.join() {
            Ok(code) => code,
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(error) => {
            eprintln!("error: could not start inspector: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(args: Args) -> ExitCode {
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn emit(value: &impl Serialize, output: Option<&Path>, source: &Path) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    match output {
        Some(path) => {
            let protected = protected_tree(source)?;
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent = parent.canonicalize()?;
            if parent.starts_with(&protected) {
                return Err("report output must be outside the source directory".into());
            }
            // create_new also refuses a pre-existing symlink; reports never clobber inputs.
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            eprintln!("Wrote {}", path.display());
        }
        None => {
            let mut out = io::stdout().lock();
            out.write_all(&bytes)?;
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod numeric_transport_tests {
    #[test]
    fn json_ray_coordinates_keep_the_requested_binary64_words() {
        // These words come from a near-tangent analytic ray. Moving either
        // value by one ULP changes the intersection, before physics even runs.
        let text = "[-229.11445911534562,610.4858273951352,-758.1652980544283,0.23005015640173943,-0.6101330235002654,0.7581652980544282]";
        let expected = [
            0xc06ca3a9a629a46f,
            0x408313e2f9792cda,
            0xc087b15287c94ee5,
            0x3fcd72489517b330,
            0xbfe38635b0c4956b,
            0x3fe842e3df036338,
        ];
        let typed: Vec<f64> = serde_json::from_str(text).unwrap();
        assert_eq!(
            typed
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            expected
        );
        let report: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            report
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_f64().unwrap().to_bits())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn json_round_trips_finite_words_across_the_binary64_range() {
        let mut words = vec![
            0,
            1 << 63,
            1,
            (1 << 63) | 1,
            f64::MAX.to_bits(),
            (-f64::MAX).to_bits(),
        ];
        let mut seed = 0x8a5cd789635d2dff_u64;
        for _ in 0..1024 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            if f64::from_bits(seed).is_finite() {
                words.push(seed);
            }
        }
        for word in words {
            let text = serde_json::to_string(&f64::from_bits(word)).unwrap();
            let restored: f64 = serde_json::from_str(&text).unwrap();
            assert_eq!(restored.to_bits(), word, "JSON changed {word:016x}: {text}");
        }
    }
}

fn protected_tree(source: &Path) -> Result<PathBuf> {
    let source = source.canonicalize()?;
    let directory = if source.is_file() {
        source.parent().ok_or("source has no parent")?
    } else {
        &source
    };
    if directory
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("Data"))
    {
        Ok(directory
            .parent()
            .ok_or("Data directory has no parent")?
            .to_owned())
    } else {
        Ok(directory.to_owned())
    }
}

fn protect_native_report(output: Option<&Path>, repository: &Path) -> Result<()> {
    if let Some(path) = output {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(repository.canonicalize()?) {
            return Err("report output must be outside the native repository".into());
        }
    }
    Ok(())
}

fn data_files(install: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(install.join("Data"))? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    extensions
                        .iter()
                        .any(|expected| e.eq_ignore_ascii_case(expected))
                })
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

fn run(args: Args) -> Result<()> {
    let output = args.output.as_deref();
    match args.command {
        Command::Assets(command) => run_assets(command, output),
        Command::Runtime(command) => run_runtime(command, output),
        Command::Actors(command) => run_actors(command, output),
        Command::Scripts(command) => run_scripts(command, output),
        Command::World(command) => run_world(command, output),
        Command::Physics(command) => run_physics(command, output),
        Command::Sources(command) => run_sources(command, output),
    }
}

fn run_assets(command: AssetsCommand, output: Option<&Path>) -> Result<()> {
    match command {
        AssetsCommand::NifSourcePoseSet { input, request } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                for source in [&input, &request] {
                    if parent.starts_with(protected_tree(source)?) {
                        return Err("report output must be outside every source directory".into());
                    }
                }
            }
            let report = nif_animation_inspection::inspect_pose_set(&input, &request)?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err("explicit source pose set refused; see report".into());
            }
        }
        AssetsCommand::NifSourcePoseBatch { input, request } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                for source in [&input, &request] {
                    if parent.starts_with(protected_tree(source)?) {
                        return Err("report output must be outside every source directory".into());
                    }
                }
            }
            let report = nif_animation_inspection::inspect_pose_batch(&input, &request)?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err("prepared source pose batch refused; see report".into());
            }
        }
        AssetsCommand::NifClipPose {
            skeleton,
            clip,
            request,
        } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                for input in [&skeleton, &clip, &request] {
                    if parent.starts_with(protected_tree(input)?) {
                        return Err("report output must be outside every source directory".into());
                    }
                }
            }
            let report = nif_animation_inspection::inspect_clip(&skeleton, &clip, &request)?;
            emit(&report, output, &skeleton)?;
            if report.failures != 0 {
                return Err("external source clip pose refused; see report".into());
            }
        }
        AssetsCommand::NifRigidAttachment {
            skeleton,
            attachment,
            request,
        } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                for input in [&skeleton, &attachment, &request] {
                    if parent.starts_with(protected_tree(input)?) {
                        return Err("report output must be outside every source directory".into());
                    }
                }
            }
            let report =
                nif_animation_inspection::inspect_attachment(&skeleton, &attachment, &request)?;
            emit(&report, output, &skeleton)?;
            if report.failures != 0 {
                return Err("rigid source attachment refused; see report".into());
            }
        }
        AssetsCommand::NifSourcePose {
            input,
            local_visibility,
            object,
            controller,
            source_time,
        } => {
            if local_visibility {
                let report = nif_animation_inspection::inspect_visibility(
                    &input,
                    fallout_data::nif_animation::visibility::Request {
                        object,
                        controller,
                        source_time,
                    },
                )?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("local visibility sample refused; see report".into());
                }
                return Ok(());
            }
            let report = nif_animation_inspection::inspect_pose(
                &input,
                fallout_data::nif_animation::pose::Request {
                    object,
                    controller,
                    source_time,
                },
            )?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err("linked source pose refused; see report".into());
            }
        }
        AssetsCommand::NifAnimation {
            input,
            oracle_report,
            include_keyframes,
            include_splines,
            include_spline_components,
            include_bool_interpolators,
            include_bool_keys,
            sample_time,
            sample_block,
            sample_channel,
            markers_request,
        } => {
            if let Some(request) = markers_request {
                if let Some(path) = output {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    for source in [&input, &request] {
                        if parent.starts_with(protected_tree(source)?) {
                            return Err(
                                "report output must be outside every source directory".into()
                            );
                        }
                    }
                }
                let report = nif_animation_inspection::inspect_markers(&input, &request)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("source text-key interval refused; see report".into());
                }
                return Ok(());
            }
            let sample = match (sample_time, sample_block, sample_channel) {
                (Some(time), Some(block), Some(channel)) => {
                    Some(nif_animation_inspection::SampleRequest {
                        time,
                        block,
                        channel,
                    })
                }
                (None, None, None) => None,
                _ => {
                    return Err(
                        "sample time, source block and channel are required together".into(),
                    );
                }
            };
            let report = nif_animation_inspection::inspect(
                &input,
                oracle_report.as_deref(),
                nif_animation_inspection::SourceOptions {
                    include_keyframes,
                    include_splines,
                    include_spline_components,
                    include_bool_interpolators,
                    include_bool_keys,
                },
                sample,
            )?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err(
                    "animation source decoding or independent comparison failed; see report".into(),
                );
            }
        }
        AssetsCommand::NifSkin {
            input,
            oracle_report,
            include_partitions,
            include_bindings,
            pose_geometry,
            pose_weight_tolerance,
            sampled_pose_request,
            influences_request,
            external_rig,
            external_skin_request,
            shared_skin_request,
        } => {
            if let Some(request) = shared_skin_request {
                if let Some(path) = output {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    for source in [&input, &request] {
                        if parent.starts_with(protected_tree(source)?) {
                            return Err(
                                "report output must be outside every source directory".into()
                            );
                        }
                    }
                }
                let report = nif_skin_inspection::inspect_shared_skin(&input, &request)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("shared source skin batch refused; see report".into());
                }
                return Ok(());
            }
            if let Some(request) = external_skin_request {
                let rig = external_rig.ok_or("external rig input missing")?;
                if let Some(path) = output {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    for source in [&input, &rig, &request] {
                        if parent.starts_with(protected_tree(source)?) {
                            return Err(
                                "report output must be outside every source directory".into()
                            );
                        }
                    }
                }
                let report = nif_skin_inspection::inspect_external_skin(&input, &rig, &request)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("explicit external source skin pose refused; see report".into());
                }
                return Ok(());
            }
            if let Some(request) = influences_request {
                if let Some(path) = output {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    for source in [&input, &request] {
                        if parent.starts_with(protected_tree(source)?) {
                            return Err(
                                "report output must be outside every source directory".into()
                            );
                        }
                    }
                }
                let report = nif_skin_inspection::inspect_influences(&input, &request)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("exact raw influence reconstruction refused; see report".into());
                }
                return Ok(());
            }
            if let Some(request) = sampled_pose_request {
                if let Some(path) = output {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    for source in [&input, &request] {
                        if parent.starts_with(protected_tree(source)?) {
                            return Err(
                                "report output must be outside every source directory".into()
                            );
                        }
                    }
                }
                let report = nif_skin_inspection::inspect_sampled_pose(&input, &request)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("source-linked sampled skin pose refused; see report".into());
                }
                return Ok(());
            }
            if let Some(geometry) = pose_geometry {
                let tolerance = pose_weight_tolerance.ok_or("pose weight tolerance missing")?;
                let report = nif_skin_inspection::inspect_pose(&input, geometry, tolerance)?;
                emit(&report, output, &input)?;
                if report.failures != 0 {
                    return Err("source-local skin pose refused; see report".into());
                }
                return Ok(());
            }
            let report = nif_skin_inspection::inspect(
                &input,
                oracle_report.as_deref(),
                include_partitions,
                include_bindings,
            )?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err("skin decoding or independent comparison failed; see report".into());
            }
        }
        AssetsCommand::NifAssets {
            input,
            install,
            texture_cache,
        } => {
            let mut assets = fallout_data::assets::ArchiveAssets::open_nv(&install)?;
            let report = fallout_data::texture_probe::inspect(
                &input,
                &mut assets,
                texture_cache.as_deref(),
            )?;
            emit(&report, output, &install)?;
            if report.failures != 0 {
                return Err(
                    "model texture dependencies contain unresolved or failed inputs; see report"
                        .into(),
                );
            }
        }
        AssetsCommand::NifScene { input } => {
            use std::io::Read;
            let mut file = baseline::open_source(&input)?.take(64 * 1024 * 1024 + 1);
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let (index, scene) =
                fallout_data::nif_scene::decode(&bytes, &input.display().to_string())?;
            emit(
                &json!({"schema_version":1,"profile":"nv-original","input":input,
                "sha256":format!("{:x}", Sha256::digest(&bytes)),"decoded_bytes":bytes.len(),
                "index":index,"scene":scene}),
                output,
                &input,
            )?;
        }
        AssetsCommand::NifCensus {
            install,
            inspect_scenes,
        } => {
            let mut archives = Vec::new();
            for path in data_files(&install, &["bsa"])? {
                eprintln!("Inspecting NIF/KF members in {}", path.display());
                archives.push(fallout_data::nif_census::scan_with_scenes(
                    &path,
                    inspect_scenes,
                )?);
            }
            let failures: usize = archives.iter().map(|a| a.failures.len()).sum();
            let scene_failures: usize = archives
                .iter()
                .filter_map(|a| a.scene_payloads.as_ref())
                .map(|s| s.failures.len())
                .sum();
            emit(
                &json!({"schema_version":1,"profile":"nv-original","archives":archives,"failures":failures,"scene_failures":scene_failures,
                "scope":"all NIF/KF members in discovered top-level BSA files; loose assets not included",
                "accepted_gameplay":false}),
                output,
                &install,
            )?;
            if failures > 0 || scene_failures > 0 {
                return Err(
                    "NIF census contains unsupported or invalid containers; see report".into(),
                );
            }
        }
    }
    Ok(())
}

fn run_runtime(command: RuntimeCommand, output: Option<&Path>) -> Result<()> {
    match command {
        RuntimeCommand::PrimitiveQueryState {
            install,
            load_order,
            new_repository,
        } => {
            let report = query_inspection::probe(&install, &load_order, &new_repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::PrimitiveQueryLoadProbe {
            install,
            load_order,
            repository,
            query_inputs,
        } => {
            let (owners, keys) = item_state_inspection::decode_query_inputs(&query_inputs)?;
            let report =
                query_inspection::cold(&install, &load_order, &repository, &owners, &keys)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::SourceItemState {
            install,
            load_order,
            new_repository,
        } => {
            let report = source_item_inspection::probe(&install, &load_order, &new_repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::SourceItemLoadProbe {
            install,
            load_order,
            repository,
            query_inputs,
        } => {
            let (owners, keys) = item_state_inspection::decode_query_inputs(&query_inputs)?;
            let report =
                source_item_inspection::cold(&install, &load_order, &repository, &owners, &keys)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::ItemState {
            install,
            load_order,
            new_repository,
        } => {
            let report = item_state_inspection::probe(&install, &load_order, &new_repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::ItemLoadProbe {
            install,
            load_order,
            repository,
            query_inputs,
        } => {
            let (owners, keys) = item_state_inspection::decode_query_inputs(&query_inputs)?;
            let report =
                item_state_inspection::cold(&install, &load_order, &repository, &owners, &keys)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::ForeignContext {
            install,
            load_order,
            new_repository,
        } => {
            let report = foreign_context_inspection::inspect(
                &install,
                &load_order,
                new_repository.as_deref(),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::ForeignLoadProbe {
            install,
            load_order,
            repository,
            player_id,
        } => {
            // A read-only load must also leave the input directory unchanged.
            protect_native_report(output, &repository)?;
            let report =
                foreign_context_inspection::cold(&install, &load_order, &repository, player_id)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeSaveProbe {
            install,
            load_order,
            new_repository,
            engineering_event_commit,
        } => {
            let report = native_save_inspection::probe(
                &install,
                &load_order,
                &new_repository,
                engineering_event_commit.as_deref(),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeLoadProbe {
            install,
            load_order,
            repository,
        } => {
            protect_native_report(output, &repository)?;
            let report = native_save_inspection::load(&install, &load_order, &repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeSlotAvailability {
            install,
            load_order,
            repository,
        } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                if parent.starts_with(repository.canonicalize()?) {
                    return Err(
                        "availability report output must be outside the native repository".into(),
                    );
                }
            }
            let report = native_save_inspection::availability(&install, &load_order, &repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeRestoreProbe {
            install,
            load_order,
            repository,
            request_id,
            recover_previous,
        } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                if parent.starts_with(repository.canonicalize()?) {
                    return Err(
                        "restore report output must be outside the native repository".into(),
                    );
                }
            }
            let report = native_save_inspection::restore_probe(
                &install,
                &load_order,
                &repository,
                request_id,
                recover_previous,
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeMigrateV2 {
            install,
            load_order,
            file,
            new_repository,
        } => {
            let report =
                native_migration_inspection::import(&install, &load_order, &file, &new_repository)?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeMigrateV3 {
            install,
            load_order,
            file,
            new_repository,
        } => {
            let report = native_migration_inspection::import_v3(
                &install,
                &load_order,
                &file,
                &new_repository,
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::NativeSaveFile { file } => {
            let report = native_save_inspection::file(&file)?;
            emit(&report, output, &file.canonicalize()?)?;
        }
        RuntimeCommand::ScriptState {
            install,
            load_order,
            index_cache,
            engineering_event_commit,
            host_snapshot,
            host_requirements,
        } => {
            let report = script_state_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                engineering_event_commit.as_deref(),
                host_snapshot.as_deref().zip(host_requirements.as_deref()),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::EventFrames {
            install,
            load_order,
            index_cache,
            comparison_bundle,
            owned_observation,
            snapshot_input,
        } => {
            let report = event_frame_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                comparison_bundle.as_deref(),
                owned_observation.as_deref(),
                snapshot_input.as_deref(),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
            if owned_observation.is_none()
                && report["prepared_frames"] != report["pending_events_checked"]
            {
                return Err("Pending events retain unresolved source findings; see report".into());
            }
        }
        RuntimeCommand::ScriptFixture {
            install,
            request,
            profile_receipt,
            destination,
        } => {
            let report = script_trace::fixtures::generate(
                &install,
                &request,
                &profile_receipt,
                &destination,
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        RuntimeCommand::ScriptTrace {
            install,
            load_order,
            index_cache,
            manifest,
            profile_receipt,
            original_trace,
            replacement_trace,
            replacement_copy,
        } => {
            let report = script_trace::inspect(script_trace::Inputs {
                install: &install,
                load_order: &load_order,
                cache: index_cache.as_deref(),
                manifest: &manifest,
                profile_receipt: &profile_receipt,
                original: original_trace.as_deref(),
                replacement: replacement_trace.as_deref(),
                replacement_copy: replacement_copy.as_deref(),
            })?;
            emit(&report, output, &protected_tree(&install)?)?;
            if report["comparison"]["status"] != "matched" {
                return Err(
                    "Semantic trace comparison is blocked or mismatched; see report".into(),
                );
            }
        }
        RuntimeCommand::EventOperands {
            install,
            load_order,
            index_cache,
            player_id,
            prepared_sources,
            native_capabilities,
            engineering_local_copy,
            snapshot_copy_request,
            snapshot_native_request,
            quest_boot_request,
            quest_boot_output,
            snapshot_input,
            snapshot_output,
        } => {
            if let Some(request) = quest_boot_request {
                let report = event_operand_inspection::boot_saved_quest(
                    &install,
                    &load_order,
                    index_cache.as_deref(),
                    &request,
                    snapshot_input.as_deref().ok_or("Missing snapshot input")?,
                    quest_boot_output
                        .as_deref()
                        .ok_or("Missing quest boot output")?,
                )?;
                emit(&report, output, &protected_tree(&install)?)?;
                return Ok(());
            }
            if let Some(request) = snapshot_native_request {
                let report = event_operand_inspection::observe_saved_native(
                    &install,
                    &load_order,
                    index_cache.as_deref(),
                    &request,
                    snapshot_input.as_deref().ok_or("Missing snapshot input")?,
                )?;
                emit(&report, output, &protected_tree(&install)?)?;
                if report["observations"]
                    .as_array()
                    .ok_or("Missing native observations")?
                    .iter()
                    .any(|row| row["observation"]["outcome"]["status"] != "engineering_observation")
                {
                    return Err(
                        "Saved native observations retain unsupported semantics; see report".into(),
                    );
                }
                return Ok(());
            }
            if let Some(request) = snapshot_copy_request {
                let report = event_operand_inspection::copy_saved(
                    &install,
                    &load_order,
                    index_cache.as_deref(),
                    &request,
                    snapshot_input.as_deref().ok_or("Missing snapshot input")?,
                    snapshot_output
                        .as_deref()
                        .ok_or("Missing snapshot output")?,
                )?;
                emit(&report, output, &protected_tree(&install)?)?;
                if report["snapshot_copy"]["status"] != "engineering_committed" {
                    return Err(
                        "Saved source copy remains unsupported; see engineering report".into(),
                    );
                }
                return Ok(());
            }
            let report = event_operand_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                player_id,
                prepared_sources,
                native_capabilities,
                engineering_local_copy.as_deref(),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
            if report["engineering_local_copy"]["status"] == "unsupported" {
                return Err("Source local copy remains unsupported; see engineering report".into());
            }
            if native_capabilities && report["native_unsupported"] != 0 {
                return Err(
                    "Pending native calls retain unsupported faithful semantics; see report".into(),
                );
            }
            if report["prepared_probes"] != report["pending_events_checked"]
                || report["unresolved_operands"] != 0
            {
                return Err(
                    "Pending operands retain unresolved source or storage findings; see report"
                        .into(),
                );
            }
        }
        RuntimeCommand::SharedRuntime {
            install,
            load_order,
            index_cache,
        } => {
            let report =
                shared_runtime_inspection::inspect(&install, &load_order, index_cache.as_deref())?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
    }
    Ok(())
}

fn run_actors(command: ActorsCommand, output: Option<&Path>) -> Result<()> {
    match command {
        ActorsCommand::ActorSources {
            install,
            load_order,
            index_cache,
            compare_oracle,
            include_associations,
            include_classes,
            include_factions,
            include_placements,
            include_races,
            include_packages,
            include_package_dependencies,
            package_destination,
            include_dependencies,
            dependency_roots,
            include_render_dependencies,
            include_template_dependencies,
            equipment_source,
            equipment_role,
            voice_root,
            creature_model_directory,
        } => {
            let mut report = actor_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                actor_inspection::Options {
                    include_associations,
                    include_classes,
                    include_factions,
                    include_placements,
                    include_races,
                    include_packages,
                    include_package_dependencies,
                    package_destination,
                    include_dependencies,
                    dependency_roots,
                    include_render_dependencies,
                    include_template_dependencies,
                    equipment_source,
                    equipment_role,
                    voice_root,
                    creature_model_directory,
                },
            )?;
            if let Some(oracle) = compare_oracle {
                actor_inspection::compare(&mut report, &oracle)?;
            }
            emit(&report, output, &protected_tree(&install)?)?;
            if report["counts"]["source_findings"] != 0
                || report["actor_associations"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_classes"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_factions"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_placements"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_races"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_packages"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_package_dependencies"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
                || report["actor_dependencies"]["counts"]["source_findings"]
                    .as_u64()
                    .unwrap_or(0)
                    != 0
            {
                return Err("actor source inspection retains source findings; see report".into());
            }
        }
        ActorsCommand::ActorPackageContext {
            install,
            load_order,
            index_cache,
            actor_root,
            native_snapshot,
            explicit_subject,
            engineering_observation,
            condition_executable,
            include_faction_requests,
            include_stat_requests,
            package_capability,
            include_actor_context,
            equipment_item,
        } => {
            let report = actor_inspection::package_context(
                &install,
                &load_order,
                index_cache.as_deref(),
                actor_inspection::ContextOptions {
                    actor_root: &actor_root,
                    snapshot: &native_snapshot,
                    explicit_subject,
                    engineering_observation,
                    condition_executable: condition_executable.as_deref(),
                    include_faction_requests,
                    include_stat_requests,
                    package_capability,
                    include_actor_context,
                    equipment_item,
                },
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
            if report.get("package_capability").is_some() {
                return Err("actor package execution capability is unsupported; see refusal dependencies in report".into());
            }
        }
        ActorsCommand::BaseInventory {
            install,
            load_order,
            index_cache,
        } => {
            let report =
                inventory_inspection::inspect(&install, &load_order, index_cache.as_deref())?;
            emit(&report, output, &protected_tree(&install)?)?;
            if report["counts"]["source_findings"] != 0 {
                return Err(
                    "base inventory inspection retains source association findings; see report"
                        .into(),
                );
            }
        }
        ActorsCommand::FormLists {
            install,
            load_order,
            index_cache,
            root_plugin,
            root_id,
        } => {
            let report = form_list_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                root_plugin.as_deref().zip(root_id),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
        }
        ActorsCommand::LeveledLists {
            install,
            load_order,
            index_cache,
            root_plugin,
            root_id,
        } => {
            let report = leveled_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                root_plugin.as_deref().zip(root_id),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
            if report["counts"]["source_findings"] != 0 {
                return Err(
                    "leveled list inspection retains source association findings; see report"
                        .into(),
                );
            }
        }
    }
    Ok(())
}

fn run_scripts(command: ScriptsCommand, output: Option<&Path>) -> Result<()> {
    match command {
        ScriptsCommand::ConditionDependencies {
            install,
            load_order,
            index_cache,
            include_source_owners,
            include_source_runs,
            engineering_query_input,
            engineering_query_batch,
        } => {
            let report = condition_dependency_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                include_source_owners,
                include_source_runs,
                engineering_query_input.as_deref(),
                engineering_query_batch.as_deref(),
            )?;
            emit(&report, output, &protected_tree(&install)?)?;
            if engineering_query_batch.is_some()
                && report["engineering_batch"]["engineering"]
                    .as_array()
                    .ok_or("Missing condition batch observations")?
                    .iter()
                    .any(|row| row["outcome"]["status"] != "engineering_observation")
            {
                return Err(
                    "Condition batch retains unsupported query outcomes; see report".into(),
                );
            }
            if report["counts"]["source_findings"] != 0
                || report["counts"]["unknown_parameters"] != 0
                || ((include_source_owners || include_source_runs)
                    && (report["source_owner_counts"]["source_findings"] != 0
                        || report["source_owner_counts"]["orphan_conditions"] != 0
                        || report["source_owner_counts"]["unmapped_conditions"] != 0))
            {
                return Err(
                    "condition dependency inspection retains source or schema findings; see report"
                        .into(),
                );
            }
        }
        ScriptsCommand::CompressedRecords {
            install,
            load_order,
            inspect_checksum_mismatches,
        } => {
            let report = compressed_record_inspection::inspect(
                &install,
                &load_order,
                inspect_checksum_mismatches,
            )?;
            let failed = report["counts"]["checksum_mismatches"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err(
                    "compressed record inspection retains checksum findings; see report".into(),
                );
            }
        }
        ScriptsCommand::QuestScripts {
            install,
            load_order,
            index_cache,
            script_comparison_bundle,
            quest_comparison_bundle,
        } => {
            let report = quest_script_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                script_comparison_bundle.as_deref(),
                quest_comparison_bundle.as_deref(),
            )?;
            let failed = report["catalogue_counts"]["scripts_with_issues"] != 0
                || report["catalogue_counts"]["source_ownership_findings"] != 0
                || report["quest_counts"]["source_findings"] != 0
                || report["operand_counts"]["missing_bindings"] != 0
                || report["operand_counts"]["decode_issues"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err(
                    "quest script inspection retains source or operand findings; see report".into(),
                );
            }
        }
        ScriptsCommand::LoadedScripts {
            install,
            load_order,
            index_cache,
            comparison_bundle,
        } => {
            let report = loaded_script_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                comparison_bundle.as_deref(),
            )?;
            let failed = report["counts"]["scripts_with_issues"] != 0
                || report["counts"]["source_ownership_findings"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err("loaded script catalogue retains source findings; see report".into());
            }
        }
        ScriptsCommand::Conditions {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let report = condition_inspection::inspect(
                &install,
                defer_unrelated_payloads,
                comparison_bundle.as_deref(),
            )?;
            let failed = !report["unresolved_condition_function_ids"]
                .as_array()
                .ok_or("Missing condition links")?
                .is_empty();
            emit(&report, output, &install)?;
            if failed {
                return Err("condition function metadata has unresolved links; see report".into());
            }
        }
        ScriptsCommand::OperandBindings {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let report = operand_inspection::inspect(
                &install,
                defer_unrelated_payloads,
                comparison_bundle.as_deref(),
            )?;
            let failed = report["table_units_with_issues"] != 0
                || report["decode_issues"] != 0
                || report["missing_bindings"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err("operand table associations have issues; see report".into());
            }
        }
        ScriptsCommand::NativeArguments {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let report = argument_inspection::inspect(
                &install,
                defer_unrelated_payloads,
                comparison_bundle.as_deref(),
            )?;
            let failed = report["issues"] != 0
                || !report["unresolved_command_ids"]
                    .as_array()
                    .ok_or("Missing command bindings")?
                    .is_empty();
            emit(&report, output, &install)?;
            if failed {
                return Err("native argument inspection has issues; see report".into());
            }
        }
        ScriptsCommand::SourcePlans {
            install,
            load_order,
            index_cache,
            comparison_bundle,
            execution_admission,
            cooperative_preparation,
            selected_source,
        } => {
            let report = definition_plan_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                comparison_bundle.as_deref(),
                execution_admission.as_deref(),
                cooperative_preparation.as_deref(),
                selected_source.as_deref(),
            )?;
            let failed = if selected_source.is_some() {
                report["selected_source"]["unavailable_definitions"] != 0
            } else if cooperative_preparation.is_some() {
                report["cooperative_preparation"]["outcome"] != "complete"
                    || report["cooperative_preparation"]["cache"]["counts"]["rejected"] != 0
            } else if execution_admission.is_some() {
                !report["execution_admission"]["faithful_execution_admitted"]
                    .as_bool()
                    .unwrap_or(false)
            } else {
                report["counts"]
                    .as_object()
                    .ok_or("Missing source-plan counts")?
                    .iter()
                    .any(|(kind, count)| {
                        kind != "prepared_source_structure"
                            && kind != "absent_compiled_field"
                            && count.as_u64().unwrap_or(1) != 0
                    })
            };
            emit(&report, output, &install)?;
            if failed {
                return Err(
                    "winning script source findings remain unresolved; see source-plan report"
                        .into(),
                );
            }
        }
        ScriptsCommand::ControlFlow {
            install,
            bundle,
            diagnose_structure,
        } => {
            let report = control_flow_inspection::inspect(&bundle, diagnose_structure)?;
            let failed = report["structure"]["counts"]["structural_issues"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err(
                    "source control-flow findings remain unverified; see diagnostic report".into(),
                );
            }
        }
        ScriptsCommand::ExpressionPlans {
            install,
            bundle,
            diagnose_structure,
        } => {
            let report =
                expression_plan_inspection::inspect(&install, &bundle, diagnose_structure)?;
            let failed = report["plans"]["counts"]["structural_issues"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err(
                    "expression structural findings remain unverified; see diagnostic report"
                        .into(),
                );
            }
        }
        ScriptsCommand::Expressions {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let report = expression_inspection::inspect(
                &install,
                defer_unrelated_payloads,
                comparison_bundle.as_deref(),
            )?;
            let failed = report["issues"] != 0
                || !report["unresolved_command_ids"]
                    .as_array()
                    .ok_or("Missing command bindings")?
                    .is_empty();
            emit(&report, output, &install)?;
            if failed {
                return Err("expression framing, token decoding or command descriptor links have issues; see report".into());
            }
        }
        ScriptsCommand::Bindings {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let mut bundle = comparison_bundle
                .as_ref()
                .map(|path| -> Result<_> {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    if parent.starts_with(protected_tree(&install)?) {
                        return Err("script binding bundle must be outside the installation".into());
                    }
                    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
                    file.write_all(b"FRUNIT01")?;
                    Ok(io::BufWriter::new(file))
                })
                .transpose()?;
            let mut bundle_bytes = 8_u64;
            let mut reports = Vec::new();
            for path in data_files(&install, &["esm", "esp"])? {
                eprintln!("Inspecting script tables in {}", path.display());
                reports.push(fallout_data::script_bindings::inspect(
                    &path,
                    defer_unrelated_payloads,
                    |record, _| {
                        if let Some(bundle) = &mut bundle {
                            bundle_bytes += 20 + record.payload.len() as u64;
                            if bundle_bytes > 512 * 1024 * 1024 {
                                return Err(fallout_data::Error::Unsupported(
                                    "script binding bundle exceeds 512 MiB".into(),
                                ));
                            }
                            let write = |error: std::io::Error| {
                                fallout_data::Error::Resolution(error.to_string())
                            };
                            bundle.write_all(&record.header.kind).map_err(write)?;
                            bundle
                                .write_all(&record.header.form_id.to_le_bytes())
                                .map_err(write)?;
                            bundle
                                .write_all(&record.header.offset.to_le_bytes())
                                .map_err(write)?;
                            bundle
                                .write_all(&(record.payload.len() as u32).to_le_bytes())
                                .map_err(write)?;
                            bundle.write_all(&record.payload).map_err(write)?;
                        }
                        Ok(())
                    },
                )?);
            }
            let bundle_receipt = if let Some(mut bundle) = bundle {
                bundle.flush()?;
                bundle.get_ref().sync_all()?;
                drop(bundle);
                let (bytes, sha256) =
                    baseline::digest_file(comparison_bundle.as_ref().expect("opened bundle"))?;
                Some(
                    json!({"format":"FRUNIT01: magic, repeated record kind[4], u32 form ID, u64 file offset, u32 decoded payload length and raw payload", "bytes":bytes, "sha256":sha256}),
                )
            } else {
                None
            };
            let issues: usize = reports.iter().map(|report| report.units_with_issues).sum();
            emit(
                &json!({"schema_version":1,"profile":"nv-original", "scope":"authored script tables and top-level caller bindings; no runtime values or winning embedded-script identity", "plugins":reports, "units_with_issues":issues, "comparison_bundle":bundle_receipt, "execution_ready":false,"retail_parity_accepted":false}),
                output,
                &install,
            )?;
            if issues != 0 {
                return Err("script table associations have issues; see report".into());
            }
        }
        ScriptsCommand::Catalogue { install } => {
            let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
            emit(&catalogue, output, &install)?;
        }
        ScriptsCommand::Scripts {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let mut bundle = comparison_bundle
                .as_ref()
                .map(|path| -> Result<_> {
                    let parent = path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(Path::new("."))
                        .canonicalize()?;
                    if parent.starts_with(protected_tree(&install)?) {
                        return Err(
                            "script comparison bundle must be outside the installation".into()
                        );
                    }
                    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
                    file.write_all(b"FROBS001")?;
                    Ok(io::BufWriter::new(file))
                })
                .transpose()?;
            let mut reports = Vec::new();
            for path in data_files(&install, &["esm", "esp"])? {
                eprintln!("Inspecting compiled scripts in {}", path.display());
                reports.push(fallout_data::obscript_census::inspect(
                    &path,
                    defer_unrelated_payloads,
                    |_, bytes, _| {
                        if let Some(bundle) = &mut bundle {
                            bundle
                                .write_all(&(bytes.len() as u32).to_le_bytes())
                                .map_err(|error| {
                                    fallout_data::Error::Resolution(error.to_string())
                                })?;
                            bundle.write_all(bytes).map_err(|error| {
                                fallout_data::Error::Resolution(error.to_string())
                            })?;
                        }
                        Ok(())
                    },
                )?);
            }
            let bundle_receipt = if let Some(mut bundle) = bundle {
                bundle.flush()?;
                bundle.get_ref().sync_all()?;
                drop(bundle);
                let path = comparison_bundle.as_ref().expect("opened bundle path");
                let (bytes, sha256) = baseline::digest_file(path)?;
                Some(
                    json!({"format":"FROBS001: magic, repeated u32 byte length and raw SCDA; order equals successfully decoded report plugins/bodies", "bytes":bytes,"sha256":sha256}),
                )
            } else {
                None
            };
            let issues: usize = reports.iter().map(|report| report.bodies_with_issues).sum();
            emit(
                &json!({
                    "schema_version":1, "profile":"nv-original",
                    "scope":"authored compiled bodies; explicit focused scan defers other payloads; no winning-script or execution claim",
                    "plugins":reports, "bodies_with_issues":issues,
                    "comparison_bundle":bundle_receipt,
                    "framing_digest_recipe":"SHA256 of concatenated 24-byte little-endian tuples: u32 start/end/operand offsets, u16 opcode, u8 reference presence, u16 reference index, u8 event presence, u16 event ID, u32 event end jump; absent values zero",
                    "execution_ready":false, "retail_parity_accepted":false,
                }),
                output,
                &install,
            )?;
            if issues != 0 {
                return Err("compiled script framing or metadata has issues; see report".into());
            }
        }
    }
    Ok(())
}

fn run_world(command: WorldCommand, output: Option<&Path>) -> Result<()> {
    match command {
        WorldCommand::DialogueMembership {
            install,
            load_order,
            index_cache,
        } => {
            let report =
                dialogue_inspection::inspect(&install, &load_order, index_cache.as_deref())?;
            let counts = &report["membership"]["counts"];
            let failures = [
                "missing_parents",
                "null_parents",
                "missing_topics",
                "deleted_topics",
                "wrong_topic_kinds",
            ]
            .into_iter()
            .map(|key| counts[key].as_u64().unwrap_or(0))
            .sum::<u64>();
            let failed = failures != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err("dialogue membership has unresolved topic links; see report".into());
            }
        }
        WorldCommand::CellResidencySources {
            install,
            load_order,
            index_cache,
            cache,
            cell,
            source_timeout_ms,
        } => {
            let report = world_preparation_inspection::residency(
                &install,
                &load_order,
                index_cache.as_deref(),
                cache.as_deref(),
                world_preparation_inspection::ResidencyInput {
                    cell: parse_cell_key(&cell)?,
                    source_timeout_ms,
                },
            )?;
            let available = report["captured_sources_available"].as_bool() == Some(true);
            emit(&report, output, &install)?;
            if !available {
                return Err("cell source dependencies are unavailable; see report".into());
            }
        }
        WorldCommand::DoorResidencySources {
            install,
            load_order,
            index_cache,
            cache,
            cell,
            door,
            source_timeout_ms,
        } => {
            let report = world_preparation_inspection::door_residency(
                &install,
                &load_order,
                index_cache.as_deref(),
                cache.as_deref(),
                world_preparation_inspection::DoorInput {
                    source: world_preparation_inspection::ResidencyInput {
                        cell: parse_cell_key(&cell)?,
                        source_timeout_ms,
                    },
                    door: parse_cell_key(&door)?,
                },
            )?;
            let available = report["captured_sources_available"].as_bool() == Some(true);
            emit(&report, output, &install)?;
            if !available {
                return Err(
                    "door destination source dependencies are unavailable; see report".into(),
                );
            }
        }
        WorldCommand::GridResidencySources {
            install,
            load_order,
            index_cache,
            cache,
            world,
            grid_x,
            grid_y,
            source_timeout_ms,
            include_terrain,
        } => {
            let inspect = if include_terrain {
                world_preparation_inspection::grid_cell_residency
            } else {
                world_preparation_inspection::grid_residency
            };
            let report = inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                cache.as_deref(),
                world_preparation_inspection::GridInput {
                    world: parse_cell_key(&world)?,
                    grid: [grid_x, grid_y],
                    source_timeout_ms,
                },
            )?;
            let available = report["captured_sources_available"].as_bool() == Some(true);
            emit(&report, output, &install)?;
            if !available {
                return Err("grid CELL source dependencies are unavailable; see report".into());
            }
        }
        WorldCommand::GridTerrainSources {
            install,
            load_order,
            index_cache,
            cache,
            world,
            grid_x,
            grid_y,
            source_timeout_ms,
        } => {
            let report = world_preparation_inspection::grid_terrain(
                &install,
                &load_order,
                index_cache.as_deref(),
                cache.as_deref(),
                world_preparation_inspection::GridInput {
                    world: parse_cell_key(&world)?,
                    grid: [grid_x, grid_y],
                    source_timeout_ms,
                },
            )?;
            let available = report["captured_sources_available"].as_bool() == Some(true);
            emit(&report, output, &install)?;
            if !available {
                return Err("grid CELL terrain sources are unavailable; see report".into());
            }
        }
        WorldCommand::ConversationSources {
            install,
            load_order,
            index_cache,
            topic,
            info,
            speaker,
            bind_result_fragments,
        } => {
            let request = world_preparation_inspection::Input {
                topic: parse_cell_key(&topic)?,
                info: parse_cell_key(&info)?,
                speaker: speaker.as_deref().map(parse_cell_key).transpose()?,
                bind_result_fragments,
            };
            let report = world_preparation_inspection::conversation(
                &install,
                &load_order,
                index_cache.as_deref(),
                request,
            )?;
            emit(&report, output, &install)?;
        }
        WorldCommand::Narrative {
            install,
            defer_unrelated_payloads,
            comparison_bundle,
        } => {
            let report = narrative_inspection::inspect(
                &install,
                defer_unrelated_payloads,
                comparison_bundle.as_deref(),
            )?;
            let failed = report["findings"] != 0;
            emit(&report, output, &install)?;
            if failed {
                return Err("quest/dialogue ownership has source findings; see report".into());
            }
        }
        WorldCommand::Terrain {
            install,
            load_order,
            editor_id,
            index_cache,
            body_cache,
            oracle_report,
            reconstruct_heights,
            inspect_mesh,
            inspect_textures,
            prepare_textures,
            inspect_blends,
            texture_cache,
            neighbor_editor_id,
            neighbor_form,
        } => {
            let names: Vec<String> = serde_json::from_reader(baseline::open_source(&load_order)?)?;
            let body_root = body_cache
                .as_ref()
                .map(|root| {
                    let root = fallout_data::cache::validate_root(root, &install)?;
                    fallout_data::cache::validate_root(&root, &install.join("Data"))
                })
                .transpose()?;
            let mut store = if let Some(root) = &index_cache {
                fallout_data::store::RecordStore::open_nv_headers_cached(
                    &install.join("Data"),
                    &names,
                    plugin::Limits::default(),
                    root,
                )?
            } else {
                fallout_data::store::RecordStore::open_nv_headers(
                    &install.join("Data"),
                    &names,
                    plugin::Limits::default(),
                )?
            };
            let texture_plan = if prepare_textures {
                let root = store.cell_by_editor_id(editor_id.as_bytes())?.0;
                let assets = fallout_data::assets::ArchiveAssets::open_nv(&install)?;
                Some(fallout_data::terrain::preparation::TextureSourcePlan::load(
                    &mut store,
                    &root,
                    assets.mounts(),
                    Default::default(),
                )?)
            } else {
                None
            };
            let mut legacy_report = if texture_plan.is_none() {
                Some(fallout_data::terrain::inspect_cell(
                    &mut store,
                    editor_id.as_bytes(),
                    body_root.as_deref().map(|root| (root, install.as_path())),
                )?)
            } else {
                None
            };
            if inspect_textures && let Some(report) = &mut legacy_report {
                let mut assets = fallout_data::assets::ArchiveAssets::open_nv(&install)?;
                report.texture_dependencies = Some(fallout_data::terrain::textures::inspect(
                    &mut store,
                    report,
                    &mut assets,
                    body_root.as_deref().map(|root| (root, install.as_path())),
                    texture_cache.as_deref(),
                    fallout_data::terrain::textures::Limits::default(),
                )?);
            }
            let report = texture_plan
                .as_ref()
                .map(|plan| plan.terrain())
                .or(legacy_report.as_ref())
                .expect("one terrain source report");
            let texture_preparation = if let Some(plan) = &texture_plan {
                let mut preparation = fallout_data::terrain::preparation::TexturePreparation::new(
                    plan.clone(),
                    &install,
                    texture_cache.as_deref(),
                    Default::default(),
                )?;
                Some(preparation.wait()?.publish_for(report)?)
            } else {
                None
            };
            let comparison = oracle_report
                .as_deref()
                .map(|oracle| {
                    terrain_compare::compare(
                        report,
                        oracle,
                        body_root.as_deref().expect("required body cache"),
                        &install,
                        reconstruct_heights,
                        inspect_mesh,
                        inspect_blends,
                    )
                })
                .transpose()?;
            let clean = report.integrity_failures == 0
                && report.link_failures == 0
                && report
                    .texture_dependencies
                    .as_ref()
                    .is_none_or(|textures| textures.failures == 0)
                && comparison.as_ref().is_none_or(|result| result.all_equal);
            let clean = clean
                && texture_preparation
                    .as_ref()
                    .is_none_or(|receipt| receipt.plan().texture_sources.failures == 0);
            let mut value = serde_json::to_value(report)?;
            if let Some(receipt) = texture_preparation {
                value["texture_preparation"] = serde_json::to_value(receipt)?;
            }
            if inspect_blends {
                let mut maps = Vec::new();
                for entry in &report.landscapes {
                    let blends = match &entry.fields {
                        Some(fallout_data::terrain::Fields::Land(land)) => {
                            Some(fallout_data::terrain::blends::build(land)?)
                        }
                        _ => None,
                    };
                    maps.push(json!({"key":entry.key,"decoded_sha256":entry.decoded_sha256,"blends":blends}));
                }
                value["blend_maps"] = maps.into();
            }
            if inspect_mesh {
                let hidden = match &report.cell.fields {
                    Some(fallout_data::terrain::Fields::Cell(cell)) => {
                        cell.land_flags().unwrap_or(0)
                    }
                    _ => return Err("mesh inspection requires CELL fields".into()),
                };
                let mut meshes = Vec::new();
                for entry in &report.landscapes {
                    let geometry = match &entry.fields {
                        Some(fallout_data::terrain::Fields::Land(land))
                            if land.heights.is_some() =>
                        {
                            Some(fallout_data::terrain::mesh::build(land, hidden)?)
                        }
                        _ => None,
                    };
                    meshes.push(json!({"key":entry.key,"decoded_sha256":entry.decoded_sha256,"geometry":geometry}));
                }
                value["source_meshes"] = meshes.into();
            }
            if reconstruct_heights {
                let surface = fallout_data::terrain::reconstruct_cell(report)?;
                let neighbor = if let Some(neighbor_id) = neighbor_editor_id {
                    Some(fallout_data::terrain::inspect_cell(
                        &mut store,
                        neighbor_id.as_bytes(),
                        None,
                    )?)
                } else if let Some(key) = neighbor_form {
                    Some(fallout_data::terrain::inspect_cell_key(
                        &mut store, &key, None,
                    )?)
                } else {
                    None
                };
                if let Some(neighbor) = neighbor {
                    if neighbor.integrity_failures != 0 || neighbor.link_failures != 0 {
                        return Err("neighbor contains integrity or reference failures".into());
                    }
                    let neighbor_surface = fallout_data::terrain::reconstruct_cell(&neighbor)?;
                    value["edge_comparison"] = serde_json::to_value(
                        fallout_data::terrain::compare_neighbor(&surface, &neighbor_surface)?,
                    )?;
                    value["neighbor"] = serde_json::to_value(&neighbor)?;
                    value["neighbor_surface"] = serde_json::to_value(neighbor_surface)?;
                }
                value["surface"] = serde_json::to_value(surface)?;
            }
            if let Some(comparison) = comparison {
                value["comparison"] = serde_json::to_value(comparison)?;
            }
            emit(&value, output, &install)?;
            if !clean {
                return Err("terrain inspection contains integrity, reference, or field-comparison failures".into());
            }
        }
        WorldCommand::Cell {
            install,
            load_order,
            editor_id,
            inspect_checksum_mismatches,
            defer_unread_payloads,
            index_cache,
            inspect_models,
            include_dependencies,
            model_cache,
        } => {
            let names: Vec<String> = serde_json::from_reader(baseline::open_source(&load_order)?)?;
            let limits = plugin::Limits {
                inspect_checksum_mismatches,
                ..Default::default()
            };
            let mut store = if let Some(root) = &index_cache {
                fallout_data::store::RecordStore::open_nv_headers_cached(
                    &install.join("Data"),
                    &names,
                    limits,
                    root,
                )?
            } else if defer_unread_payloads {
                fallout_data::store::RecordStore::open_nv_headers(
                    &install.join("Data"),
                    &names,
                    limits,
                )?
            } else {
                fallout_data::store::RecordStore::open_nv(&install.join("Data"), &names, limits)?
            };
            let root = if include_dependencies {
                Some(store.cell_by_editor_id(editor_id.as_bytes())?.0)
            } else {
                None
            };
            let dependency_report = if let Some(root) = root.as_ref().filter(|_| !inspect_models) {
                Some(fallout_data::world::dependencies::inspect_cell_key(
                    &mut store,
                    root,
                    Default::default(),
                )?)
            } else {
                None
            };
            let mut mounts = MountIndex::default();
            for path in data_files(&install, &["bsa"])? {
                NvArchive::open(&path)?.census(&mut mounts)?;
            }
            let model_plan = if let Some(root) = root.as_ref().filter(|_| inspect_models) {
                Some(fallout_data::world::preparation::CellModelPlan::load(
                    &mut store,
                    root,
                    &mounts,
                    Default::default(),
                )?)
            } else {
                None
            };
            let mut report =
                fallout_data::world::inspect_cell(&mut store, editor_id.as_bytes(), &mounts)?;
            let model_preparation = if let Some(plan) = &model_plan {
                let mut preparation = fallout_data::world::preparation::CellPreparation::new(
                    plan.clone(),
                    &install,
                    model_cache.as_deref(),
                    Default::default(),
                )?;
                Some(preparation.wait()?.publish_into(&mut report)?)
            } else {
                None
            };
            if inspect_models && model_plan.is_none() {
                fallout_data::model_probe::inspect_models(
                    &mut report,
                    &install,
                    model_cache.as_deref(),
                )?;
            }
            let clean = report.integrity_failures == 0
                && report.link_failures == 0
                && report.model_probes.iter().all(|p| p.error.is_none());
            if let Some(dependency_report) = model_plan
                .as_ref()
                .map(|plan| plan.graph())
                .or(dependency_report.as_ref())
            {
                #[derive(serde::Serialize)]
                struct WithDependencies<'a> {
                    #[serde(flatten)]
                    cell: &'a fallout_data::world::CellReport,
                    dependency_report: &'a fallout_data::world::dependencies::Report,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    model_preparation: Option<&'a fallout_data::world::preparation::Receipt>,
                }
                emit(
                    &WithDependencies {
                        cell: &report,
                        dependency_report,
                        model_preparation: model_preparation.as_ref(),
                    },
                    output,
                    &install,
                )?;
            } else {
                emit(&report, output, &install)?;
            }
            if !clean {
                return Err("cell inspection contains integrity, reference, or model failures; runtime acceptance remains blocked".into());
            }
        }
        WorldCommand::Plan {
            install,
            load_order,
            inspect_checksum_mismatches,
        } => {
            let names: Vec<String> = serde_json::from_reader(baseline::open_source(&load_order)?)?;
            if names.is_empty() {
                return Err("load order is empty".into());
            }
            let baseline = baseline::discover(&install, |_| {})?;
            let mut indices = Vec::new();
            for name in names {
                identity::plugin_name(&name)?;
                indices.push(content::index_plugin_with_limits(
                    &install.join("Data").join(name),
                    plugin::Limits {
                        inspect_checksum_mismatches,
                        ..Default::default()
                    },
                )?);
            }
            // Reuse the resolver's dependency checks; a plan cannot bless a cyclic
            // master list just because none of its preparation jobs have run yet.
            if inspect_checksum_mismatches {
                content::inspect_resolution(&indices, ProfileId::NvOriginal)?;
            } else {
                content::resolve(&indices, ProfileId::NvOriginal)?;
            }
            let plan = fallout_data::planning::dry_run(&baseline, &indices)?;
            let clean = plan.integrity_failures == 0 && plan.missing_required.is_empty();
            emit(&plan, output, &install)?;
            if !clean {
                return Err("import plan has integrity failures or missing required inputs".into());
            }
        }
    }
    Ok(())
}

fn run_physics(command: PhysicsCommand, output: Option<&Path>) -> Result<()> {
    match command {
        PhysicsCommand::NavigationRoute {
            install,
            load_order,
            editor_id,
            index_cache,
            request,
        } => {
            let report = navigation_inspection::inspect(
                &install,
                &load_order,
                index_cache.as_deref(),
                &editor_id,
                request.as_deref(),
            )?;
            emit(&report, output, &install)?;
        }
        PhysicsCommand::CellCollision {
            install,
            load_order,
            editor_id,
            index_cache,
            source_cache,
            request,
        } => {
            let report = collision::cell_query(
                &install,
                &load_order,
                index_cache.as_deref(),
                source_cache.as_deref(),
                &editor_id,
                &request,
            )?;
            emit(&report, output, &install)?;
        }
        PhysicsCommand::NifCollision {
            input,
            oracle_report,
            query_request,
        } => {
            if let Some(request) = query_request {
                let report = collision::query(&input, &request)?;
                emit(&report, output, &input)?;
                return Ok(());
            }
            let report = collision::inspect(&input, oracle_report.as_deref())?;
            emit(&report, output, &input)?;
            if report.failures != 0 {
                return Err(
                    "collision decoding or independent comparison failed; see report".into(),
                );
            }
        }
    }
    Ok(())
}

fn run_sources(command: SourcesCommand, output: Option<&Path>) -> Result<()> {
    match command {
        SourcesCommand::Baseline { install } => {
            let report =
                baseline::discover(&install, |path| eprintln!("Hashing {}", path.display()))?;
            let complete = report.missing_required.is_empty();
            emit(&report, output, &install)?;
            if !complete {
                return Err("baseline is missing required inputs; see the report".into());
            }
        }
        SourcesCommand::VfsProfile {
            install,
            documents,
            local_appdata,
        } => {
            if let Some(path) = output {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?;
                for root in [&documents, &local_appdata] {
                    if parent.starts_with(protected_tree(root)?) {
                        return Err(
                            "profile report output must be outside all configuration source roots"
                                .into(),
                        );
                    }
                }
            }
            let report = fallout_data::vfs::profile::observe(
                &install,
                &documents,
                &local_appdata,
                Default::default(),
            )?;
            emit(&report, output, &install)?;
        }
        SourcesCommand::RetailProfileCapture {
            install,
            documents,
            local_appdata,
            package,
        } => {
            if output.is_some() {
                return Err("retail-profile-capture writes its immutable --package; --output is not supported".into());
            }
            retail_profile::capture(
                &install,
                documents.as_deref(),
                local_appdata.as_deref(),
                &package,
            )?;
        }
        SourcesCommand::Census {
            install,
            inspect_checksum_mismatches,
        } => {
            let mut plugins = Vec::new();
            let mut archives = Vec::new();
            let mut mounts = MountIndex::default();
            for path in data_files(&install, &["esm", "esp"])? {
                eprintln!("Scanning {}", path.display());
                let limits = plugin::Limits {
                    inspect_checksum_mismatches,
                    ..Default::default()
                };
                plugins.push(content::index_plugin_with_limits(&path, limits)?.census);
            }
            for path in data_files(&install, &["bsa"])? {
                eprintln!("Indexing {}", path.display());
                archives.push(NvArchive::open(&path)?.census(&mut mounts)?);
            }
            let missing: Vec<_> = baseline::OFFICIAL_PLUGINS
                .iter()
                .filter(|name| !plugins.iter().any(|p| p.name.eq_ignore_ascii_case(name)))
                .collect();
            let integrity_failures: usize = plugins.iter().map(|p| p.integrity_issues.len()).sum();
            let report = json!({"schema_version":1,"profile":"nv-original","plugins":plugins,"archives":archives,
                "integrity_failures":integrity_failures,"inspection_mode":inspect_checksum_mismatches,
                "parity":{"data_framing":fallout_data::parity::Status::Decoded,"behavior":fallout_data::parity::Status::Unknown,
                    "presentation":fallout_data::parity::Status::Unknown,"ecosystem":fallout_data::parity::Status::Unknown},
                "cross_archive_path_collisions":mounts.collisions().count(),"missing_official_plugins":missing,
                "asset_payloads_validated":false,"faithful_scenario_accepted":false,
                "unknown":["archive precedence/invalidation","loose file precedence","record field semantics","scripts","NIF/animation/collision","UI/audio semantics"]});
            emit(&report, output, &install)?;
            if integrity_failures > 0 {
                return Err("diagnostic census finished with integrity failures; data acceptance remains blocked".into());
            }
            if !missing.is_empty() {
                return Err("official corpus is incomplete; see the report".into());
            }
        }
        SourcesCommand::Inspect { plugin: path, form } => {
            let file = baseline::open_source(&path)?;
            let len = file.metadata()?.len();
            let mut found = None;
            plugin::visit(
                &mut BufReader::new(file),
                len,
                &path.display().to_string(),
                plugin::Limits::default(),
                |event| {
                    if let plugin::Event::Record(record) = event
                        && record.header.form_id == form
                    {
                        let mut fields = Vec::new();
                        plugin::visit_subrecords(record, &path.display().to_string(), |sub| {
                            let preview: String = sub
                                .data
                                .iter()
                                .take(64)
                                .map(|b| format!("{b:02x}"))
                                .collect();
                            fields.push(json!({"kind":plugin::signature(sub.kind),"decoded_payload_offset":sub.payload_offset,
                                "bytes":sub.data.len(),"preview_hex":preview,"preview_truncated":sub.data.len()>64}));
                            Ok(())
                        })?;
                        found = Some(json!({"source":path,"header":record.header,"fields":fields,
                            "status":"decoded bytes; no gameplay semantics"}));
                    }
                    Ok(())
                },
            )?;
            emit(
                &found.ok_or_else(|| format!("FormID {form:08X} not found"))?,
                output,
                &path,
            )?;
        }
        SourcesCommand::Resolve {
            install,
            load_order,
            inspect_checksum_mismatches,
        } => {
            let names: Vec<String> = serde_json::from_reader(baseline::open_source(&load_order)?)?;
            if names.is_empty() {
                return Err("load order is empty".into());
            }
            let mut indices = Vec::new();
            for name in names {
                identity::plugin_name(&name)?;
                indices.push(content::index_plugin_with_limits(
                    &install.join("Data").join(name),
                    plugin::Limits {
                        inspect_checksum_mismatches,
                        ..Default::default()
                    },
                )?);
            }
            let report = if inspect_checksum_mismatches {
                content::inspect_resolution(&indices, ProfileId::NvOriginal)?
            } else {
                content::resolve(&indices, ProfileId::NvOriginal)?.report(&indices)
            };
            let clean = report.integrity_failures == 0 && report.script_links.unresolved.is_empty();
            emit(&report, output, &install)?;
            if !clean {
                return Err(
                    "resolution report contains integrity failures or unresolved SCRO links".into(),
                );
            }
        }
        SourcesCommand::Asset {
            archive: path,
            path: member,
            cache_root,
            inspect_nif,
        } => {
            let normalized = fallout_data::vfs::AssetPath::new(member.as_bytes())?;
            let archive = NvArchive::open(&path)?;
            let matching: Vec<_> = archive
                .backend()
                .entries_with_ids()
                .filter(|(_, e)| {
                    e.path().is_some_and(|p| {
                        let raw: &[u8] = p.as_ref();
                        fallout_data::vfs::AssetPath::new(raw).ok().as_ref() == Some(&normalized)
                    })
                })
                .collect();
            if matching.len() != 1 {
                return Err(format!("expected one member, found {}", matching.len()).into());
            }
            let (id, entry) = matching[0];
            let bytes = archive.read(id)?;
            let nif = inspect_nif
                .then(|| fallout_data::nif::inspect(&bytes, &member))
                .transpose()?;
            let cache = if let Some(root) = cache_root {
                let (_, source_sha256) = baseline::digest_file(&path)?;
                let identity = fallout_data::cache::ArtifactIdentity {
                    profile: ProfileId::NvOriginal,
                    source_sha256,
                    path_bytes: normalized.bytes().to_vec(),
                    transform_version: "nv-bsa104-decode-v1".into(),
                };
                Some(fallout_data::cache::publish(
                    &root,
                    &protected_tree(&path)?,
                    identity,
                    &bytes,
                )?)
            } else {
                None
            };
            emit(
                &json!({"archive":path,"path":member,"file_offset":entry.file().data_offset,"decoded_bytes":bytes.len(),
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"semantic_support":"unknown","cache":cache,"nif":nif}),
                output,
                &path,
            )?;
        }
    }
    Ok(())
}
