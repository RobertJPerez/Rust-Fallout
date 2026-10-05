use fallout_data::baseline::digest_reader;
use skyrim_prep::{
    Error, Result, bindings, census, generic_models, landscape_links, movement_profile, nif_header,
    nif_index, placed_uses, plugin, profile, static_asset_trace, static_models, texture_sets,
    trace,
};
use std::{
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const HELP: &str = "skyrim-prep inspect --data <Skyrim Special Edition/Data> [--runtime-version <observed version>] [--output <new JSON path>]\nskyrim-prep inspect-plugin --plugin <ESM/ESP/ESL path> [--output <new JSON path>]\nskyrim-prep export-bindings --plugin <ESM/ESP/ESL path> [--output <new JSONL path>]\nskyrim-prep export-stat-models --plugin <ESM/ESP/ESL path> [--output <new JSONL path>]\nskyrim-prep export-generic-models --plugin <ESM/ESP/ESL path> [--output <new JSONL path>]\nskyrim-prep trace-stat-assets --data <Skyrim Special Edition/Data> --plugin <ESM/ESP/ESL path> [--plugin <another ESM/ESP/ESL path> ...] [--output <new JSONL path>]\nskyrim-prep trace-texture-sets --data <Skyrim Special Edition/Data> --plugin <ESM/ESP/ESL path> [--plugin <another ESM/ESP/ESL path> ...] [--output <new JSONL path>]\nskyrim-prep trace-landscape-links --data <Skyrim Special Edition/Data> --plugin <ESM/ESP/ESL path> [--plugin <another ESM/ESP/ESL path> ...] [--output <new JSONL path>]\nskyrim-prep trace-movement-profile --data <Skyrim Special Edition/Data> --plugin <ESM/ESP/ESL path> [--plugin <another ESM/ESP/ESL path> ...] [--output <new JSONL path>]\nskyrim-prep trace-placed-uses --data <Skyrim Special Edition/Data> --target-plugin <filename> --form-id <0xXXXXXXXX> [--plugin <filename> ...] [--output <new JSON path>]\nskyrim-prep trace-missing --census <census JSON path> [--output <new JSON path>]\nskyrim-prep map-profile --data <Skyrim Special Edition/Data> --order <explicit-active-order.json> [--output <new JSON path>]\nskyrim-prep nif-list --archive <Skyrim v105 BSA path> [--prefix <asset path prefix>] [--offset <0..1000000>] [--limit <1..512>] [--output <new JSON path>]\nskyrim-prep nif-header --archive <Skyrim v105 BSA path> --member <archive-relative NIF path> [--output <new JSON path>]\nskyrim-prep nif-index --archive <Skyrim v105 BSA path> --member <archive-relative NIF path> [--output <new JSON path>]\nExplicit order JSON: {\"schema_version\":1,\"active_plugins\":[\"Skyrim.esm\", ...]} in exact caller-supplied active order. Reads original files only. Exit 0: completed declared scope without blocking input findings; 2: findings; 1: command failure. No exit status certifies gameplay or that the game consumed a supplied order. NIF path indexes do not decompress assets; NIF header and outer-table indexing never certify block payload or runtime compatibility. STAT model exports preserve source paths and field hashes; generic model exports cover only schema-allowlisted non-STAT kinds.";

fn publish(path: &Path, data: &Path, bytes: &[u8]) -> Result<()> {
    publish_with(path, data, |writer| Ok(writer.write_all(bytes)?))
}

fn publish_with<T>(
    path: &Path,
    data: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<T>,
) -> Result<T> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    let data = data.canonicalize()?;
    // Protect the selected installation, including its executable and settings.
    let protected = if data
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("Data"))
    {
        data.parent().unwrap_or(&data)
    } else {
        &data
    };
    if parent.starts_with(protected) {
        return Err(Error::Unsupported(
            "report destination is inside the source installation".into(),
        ));
    }
    let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
    let result = write(temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    Ok(result)
}

fn parse_form_id(text: &str) -> Result<u32> {
    let parsed = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        text.parse::<u32>()
    };
    parsed.map_err(|_| Error::Unsupported(format!("invalid 32-bit FormID: {text:?}")))
}

fn data_plugin_path(data: &Path, name: &str) -> Result<PathBuf> {
    let path = data.join(name);
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::Unsupported(format!(
            "plugin is not a regular non-symlink file: {}",
            path.display()
        )));
    }
    let canonical = path.canonicalize()?;
    if canonical.parent() != Some(data) {
        return Err(Error::Unsupported(format!(
            "plugin resolves outside the selected Data directory: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

fn run() -> Result<i32> {
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        println!("{HELP}");
        return Ok(0);
    };
    if command == "--help" || command == "-h" {
        println!("{HELP}");
        return Ok(0);
    }
    if command != "inspect"
        && command != "inspect-plugin"
        && command != "export-bindings"
        && command != "export-stat-models"
        && command != "export-generic-models"
        && command != "trace-stat-assets"
        && command != "trace-texture-sets"
        && command != "trace-landscape-links"
        && command != "trace-movement-profile"
        && command != "trace-placed-uses"
        && command != "trace-missing"
        && command != "map-profile"
        && command != "nif-header"
        && command != "nif-index"
        && command != "nif-list"
    {
        return Err(Error::Unsupported(HELP.into()));
    }
    let mut trace_plugin_paths = Vec::new();
    let (
        mut data,
        mut output,
        mut version,
        mut order_path,
        mut archive_path,
        mut member_path,
        mut prefix,
        mut limit,
        mut offset,
    ) = (None, None, None, None, None, None, None, None, None);
    let (mut target_plugin, mut target_form_id) = (None, None);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| Error::Unsupported(format!("missing value for {flag:?}")))?;
        match flag.to_str() {
            Some("--data") if data.is_none() && command == "inspect" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "map-profile" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "trace-stat-assets" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "trace-texture-sets" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "trace-landscape-links" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "trace-movement-profile" => {
                data = Some(PathBuf::from(value))
            }
            Some("--data") if data.is_none() && command == "trace-placed-uses" => {
                data = Some(PathBuf::from(value))
            }
            Some("--census") if data.is_none() && command == "trace-missing" => {
                data = Some(PathBuf::from(value));
            }
            Some("--plugin")
                if data.is_none()
                    && (command == "inspect-plugin"
                        || command == "export-bindings"
                        || command == "export-stat-models"
                        || command == "export-generic-models") =>
            {
                data = Some(PathBuf::from(value))
            }
            Some("--plugin")
                if command == "trace-stat-assets"
                    || command == "trace-texture-sets"
                    || command == "trace-landscape-links"
                    || command == "trace-movement-profile"
                    || command == "trace-placed-uses" =>
            {
                trace_plugin_paths.push(PathBuf::from(value))
            }
            Some("--target-plugin")
                if target_plugin.is_none() && command == "trace-placed-uses" =>
            {
                target_plugin = Some(PathBuf::from(value))
            }
            Some("--form-id") if target_form_id.is_none() && command == "trace-placed-uses" => {
                target_form_id = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("invalid FormID text".into()))?,
                )
            }
            Some("--order") if order_path.is_none() && command == "map-profile" => {
                order_path = Some(PathBuf::from(value))
            }
            Some("--archive")
                if archive_path.is_none()
                    && (command == "nif-header"
                        || command == "nif-index"
                        || command == "nif-list") =>
            {
                archive_path = Some(PathBuf::from(value))
            }
            Some("--member")
                if member_path.is_none() && (command == "nif-header" || command == "nif-index") =>
            {
                member_path = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("member path must be Unicode".into()))?,
                )
            }
            Some("--prefix") if prefix.is_none() && command == "nif-list" => {
                prefix = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("asset prefix must be Unicode".into()))?,
                )
            }
            Some("--limit") if limit.is_none() && command == "nif-list" => {
                limit = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("invalid NIF index limit".into()))?
                        .parse::<usize>()
                        .map_err(|_| Error::Unsupported("invalid NIF index limit".into()))?,
                )
            }
            Some("--offset") if offset.is_none() && command == "nif-list" => {
                offset = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("invalid NIF index offset".into()))?
                        .parse::<usize>()
                        .map_err(|_| Error::Unsupported("invalid NIF index offset".into()))?,
                )
            }
            Some("--output") if output.is_none() => output = Some(PathBuf::from(value)),
            Some("--runtime-version") if version.is_none() && command == "inspect" => {
                version = Some(
                    value
                        .into_string()
                        .map_err(|_| Error::Unsupported("invalid version text".into()))?,
                )
            }
            _ => {
                return Err(Error::Unsupported(format!(
                    "unknown or duplicate argument {flag:?}"
                )));
            }
        }
    }
    if command == "nif-header" || command == "nif-index" || command == "nif-list" {
        let archive_path = archive_path.ok_or_else(|| Error::Unsupported(HELP.into()))?;
        if (command == "nif-header" || command == "nif-index")
            && (member_path.is_none() || prefix.is_some() || limit.is_some())
            || command == "nif-list" && member_path.is_some()
        {
            return Err(Error::Unsupported(HELP.into()));
        }
        let metadata = std::fs::symlink_metadata(&archive_path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::Unsupported(
                "archive input must be a regular non-symlink file".into(),
            ));
        }
        let archive_path = archive_path.canonicalize()?;
        if command == "nif-list" {
            let report = nif_header::list_archive_nifs(
                &archive_path,
                prefix.as_deref().map(str::as_bytes),
                offset.unwrap_or(0),
                limit.unwrap_or(128),
            )?;
            let encoded = serde_json::to_vec_pretty(&report)?;
            if let Some(output) = output {
                let archive_parent = archive_path
                    .parent()
                    .ok_or_else(|| Error::Unsupported("archive has no parent directory".into()))?;
                let archive_data = archive_path
                    .ancestors()
                    .find(|path| {
                        path.file_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case("Data"))
                    })
                    .unwrap_or(archive_parent);
                publish(&output, archive_data, &encoded)?;
            } else {
                std::io::stdout().lock().write_all(&encoded)?;
            }
            eprintln!(
                "{} NIF paths; returned {}; offset {}; truncated {}; {} duplicates, {} hash-only and {} invalid paths; index only",
                report.matching_members,
                report.returned_members.len(),
                report.offset,
                report.truncated,
                report.duplicate_normalized_paths,
                report.hash_only_entries,
                report.invalid_asset_paths
            );
            return Ok(
                if report.duplicate_normalized_paths == 0
                    && report.hash_only_entries == 0
                    && report.invalid_asset_paths == 0
                {
                    0
                } else {
                    2
                },
            );
        }
        let member_path = member_path.expect("NIF member argument checked");
        let archive_parent = archive_path
            .parent()
            .ok_or_else(|| Error::Unsupported("archive has no parent directory".into()))?;
        let archive_data = archive_path
            .ancestors()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Data"))
            })
            .unwrap_or(archive_parent);
        if command == "nif-header" {
            let report = nif_header::inspect_archive_member(&archive_path, member_path.as_bytes())?;
            let encoded = serde_json::to_vec_pretty(&report)?;
            if let Some(output) = output {
                publish(&output, archive_data, &encoded)?;
            } else {
                std::io::stdout().lock().write_all(&encoded)?;
            }
            eprintln!(
                "{} bytes; {}; header-only, mesh/runtime compatibility unverified",
                report.asset_bytes, report.header.classification
            );
            return Ok(if report.header.skyrim_se_header_tuple {
                0
            } else {
                2
            });
        }

        let report = nif_index::inspect_archive_member(&archive_path, member_path.as_bytes())?;
        let encoded = serde_json::to_vec_pretty(&report)?;
        if let Some(output) = output {
            publish(&output, archive_data, &encoded)?;
        } else {
            std::io::stdout().lock().write_all(&encoded)?;
        }
        eprintln!(
            "{} blocks; {} block types; SSE outer table indexed, all block payloads opaque",
            report.index.blocks.len(),
            report.index.block_types.len()
        );
        return Ok(0);
    }
    if command == "trace-placed-uses" {
        let data = data
            .ok_or_else(|| Error::Unsupported(HELP.into()))?
            .canonicalize()?;
        let target_name = target_plugin
            .and_then(|path| path.into_os_string().into_string().ok())
            .ok_or_else(|| Error::Unsupported(HELP.into()))?;
        let target_name = placed_uses::validate_plugin_name(&target_name)?;
        let target_form_id = parse_form_id(
            target_form_id
                .as_deref()
                .ok_or_else(|| Error::Unsupported(HELP.into()))?,
        )?;
        let target_path = data_plugin_path(&data, &target_name)?;
        let mut source_paths = Vec::new();
        for name in trace_plugin_paths {
            let name = name
                .into_os_string()
                .into_string()
                .map_err(|_| Error::Unsupported("plugin filename must be Unicode".into()))?;
            let name = placed_uses::validate_plugin_name(&name)?;
            source_paths.push(data_plugin_path(&data, &name)?);
        }
        let report = placed_uses::inspect(&target_path, target_form_id, &source_paths)?;
        let bytes = serde_json::to_vec_pretty(&report)?;
        if let Some(output) = output {
            publish(&output, &data, &bytes)?;
        } else {
            std::io::stdout().lock().write_all(&bytes)?;
        }
        let unresolved = report
            .sources
            .iter()
            .map(|source| source.unresolved_name_fields)
            .sum::<u64>();
        let malformed = report
            .sources
            .iter()
            .map(|source| source.malformed_name_fields)
            .sum::<u64>();
        let model_path_findings = report
            .target
            .model_fields
            .iter()
            .filter(|field| field.finding.is_some())
            .count();
        eprintln!(
            "{} source plugins; {} placed references; {} target placements; {} unresolved NAME links; {} malformed NAME fields; {} target model-path findings; source-only evidence",
            report.sources.len(),
            report
                .sources
                .iter()
                .map(|source| source.placed_references)
                .sum::<u64>(),
            report.placements.len(),
            unresolved,
            malformed,
            model_path_findings
        );
        return Ok(
            if unresolved == 0 && malformed == 0 && model_path_findings == 0 {
                0
            } else {
                2
            },
        );
    }
    if command == "trace-texture-sets" {
        let data = data
            .ok_or_else(|| Error::Unsupported(HELP.into()))?
            .canonicalize()?;
        if trace_plugin_paths.is_empty() {
            return Err(Error::Unsupported(HELP.into()));
        }
        let summary = if let Some(output) = output {
            publish_with(&output, &data, |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary = texture_sets::export_many(&data, &trace_plugin_paths, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            texture_sets::export_many(&data, &trace_plugin_paths, &mut std::io::stdout().lock())?
        };
        eprintln!(
            "{} plugins; {} TXST records; {} fields; {} normalized texture paths; {} malformed paths; {} archive index failures; {} unmatched",
            summary.plugin_files_scanned,
            summary.texture_set_records,
            summary.texture_fields,
            summary.normalized_texture_references,
            summary.malformed_texture_paths,
            summary.archive_index_failures,
            summary.references_without_candidate_match
        );
        return Ok(
            if summary.malformed_texture_paths == 0
                && summary.duplicate_texture_slots == 0
                && summary.references_without_candidate_match == 0
                && summary.archive_index_failures == 0
                && summary.archive_hash_only_entries == 0
                && summary.archive_invalid_paths == 0
                && summary.duplicate_archive_candidate_entries == 0
            {
                0
            } else {
                2
            },
        );
    }
    if command == "trace-landscape-links" {
        let data = data
            .ok_or_else(|| Error::Unsupported(HELP.into()))?
            .canonicalize()?;
        if trace_plugin_paths.is_empty() {
            return Err(Error::Unsupported(HELP.into()));
        }
        let summary = if let Some(output) = output {
            publish_with(&output, &data, |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary =
                    landscape_links::export_many(&data, &trace_plugin_paths, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            landscape_links::export_many(&data, &trace_plugin_paths, &mut std::io::stdout().lock())?
        };
        eprintln!(
            "{} plugins; {} LAND records; {} base and {} alpha layers; {} LTEX records; {} TNAM fields; {} full-ID joins; {} light links need a profile; {} CELL region links ({} with scanned candidates); {} CELL context links ({} with scanned candidates); {} LCTN rows, {} PNAM parent links ({} with scanned candidates, {} unresolved); {} XWEM paths ({} normalized, {} loose matches, {} archive matches, {} unmatched, {} malformed; {} archives indexed, {} failures); {} malformed XCLR fields; {} other malformed fields; {} nonzero auxiliary bytes retained",
            summary.plugin_files_scanned,
            summary.land_records,
            summary.landscape_base_layers,
            summary.landscape_alpha_layers,
            summary.ltex_records,
            summary.ltex_texture_set_fields,
            summary.links_with_scanned_target_candidates,
            summary.light_links_requiring_profile,
            summary.cell_region_links,
            summary.cell_region_links_with_scanned_target_candidates,
            summary.cell_context_form_links,
            summary.cell_context_links_with_scanned_target_candidates,
            summary.location_records,
            summary.location_parent_form_links,
            summary.location_parent_links_with_scanned_target_candidates,
            summary.location_parent_links_without_scanned_target_candidates
                + summary.location_parent_links_unresolved_without_profile,
            summary.cell_water_environment_map_fields,
            summary.cell_water_environment_map_paths_normalized,
            summary.cell_water_environment_map_references_with_loose_file,
            summary.cell_water_environment_map_references_with_archive_member,
            summary.cell_water_environment_map_references_without_candidate_match,
            summary.malformed_cell_water_environment_map_fields,
            summary.archives_indexed_for_cell_water_environment_map,
            summary.archive_index_failures_for_cell_water_environment_map,
            summary.malformed_xclr_fields,
            summary.malformed_link_payloads + summary.malformed_layer_payloads,
            summary.nonzero_auxiliary_layer_bytes
        );
        return Ok(
            if summary.malformed_link_payloads == 0
                && summary.malformed_layer_payloads == 0
                && summary.duplicate_layer_slots == 0
                && summary.out_of_range_quadrants == 0
                && summary.invalid_master_indices == 0
                && summary.light_links_requiring_profile == 0
                && summary.links_without_scanned_target_candidates == 0
                && summary.malformed_xclr_fields == 0
                && summary.malformed_cell_water_environment_map_fields == 0
                && summary.cell_water_environment_map_references_without_candidate_match == 0
                && summary.archive_index_failures_for_cell_water_environment_map == 0
            {
                0
            } else {
                2
            },
        );
    }
    if command == "trace-movement-profile" {
        let data = data
            .ok_or_else(|| Error::Unsupported(HELP.into()))?
            .canonicalize()?;
        if trace_plugin_paths.is_empty() {
            return Err(Error::Unsupported(HELP.into()));
        }
        let summary = if let Some(output) = output {
            publish_with(&output, &data, |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary =
                    movement_profile::export_many(&data, &trace_plugin_paths, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            movement_profile::export_many(
                &data,
                &trace_plugin_paths,
                &mut std::io::stdout().lock(),
            )?
        };
        eprintln!(
            "{} plugins; {} Player NPC candidates; {} RACE records; {} MOVT records; {} player/race and {} race/movement links ({} with candidates, {} without, {} profile-dependent); {} movement speed fields; {} race speed overrides; {} malformed links/float fields",
            summary.plugin_files_scanned,
            summary.player_npc_records,
            summary.race_records,
            summary.movement_records,
            summary.player_race_links,
            summary.race_movement_links,
            summary.links_with_scanned_target_candidates,
            summary.links_without_scanned_target_candidates,
            summary.links_unresolved_without_profile,
            summary.movement_speed_fields,
            summary.race_movement_speed_fields,
            summary.malformed_link_fields + summary.malformed_float_fields
        );
        return Ok(
            if summary.player_npc_records == 0
                || summary.malformed_link_fields > 0
                || summary.malformed_float_fields > 0
                || summary.links_without_scanned_target_candidates > 0
                || summary.links_unresolved_without_profile > 0
            {
                2
            } else {
                0
            },
        );
    }
    if command == "trace-stat-assets" {
        let data = data.ok_or_else(|| Error::Unsupported(HELP.into()))?;
        if trace_plugin_paths.is_empty() {
            return Err(Error::Unsupported(HELP.into()));
        }
        let summary = if let Some(output) = output {
            publish_with(&output, &data, |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary =
                    static_asset_trace::export_many(&data, &trace_plugin_paths, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            static_asset_trace::export_many(
                &data,
                &trace_plugin_paths,
                &mut std::io::stdout().lock(),
            )?
        };
        eprintln!(
            "{} plugin files; {} source references; {} loose matches; {} archive matches; {} unmatched; {} model/path findings; {} editor-ID findings; {} archive index failures",
            summary.plugin_files_scanned,
            summary.source_asset_references,
            summary.references_with_loose_file,
            summary.references_with_archive_member,
            summary.references_without_candidate_match,
            summary.source_path_findings,
            summary.source_editor_id_findings,
            summary.archive_index_failures
        );
        return Ok(
            if summary.source_path_findings == 0
                && summary.source_editor_id_findings == 0
                && summary.archive_index_failures == 0
                && summary.archive_hash_only_entries == 0
                && summary.archive_invalid_paths == 0
                && summary.references_without_candidate_match == 0
            {
                0
            } else {
                2
            },
        );
    }
    let data = data.ok_or_else(|| Error::Unsupported(HELP.into()))?;
    if command == "map-profile" {
        let data = data.canonicalize()?;
        let order_path = order_path.ok_or_else(|| Error::Unsupported(HELP.into()))?;
        let order_file = std::fs::File::open(&order_path)?;
        if order_file.metadata()?.len() > 1024 * 1024 {
            return Err(Error::Unsupported(
                "explicit active-order JSON exceeds 1 MiB".into(),
            ));
        }
        let mut order_bytes = Vec::new();
        order_file
            .take(1024 * 1024 + 1)
            .read_to_end(&mut order_bytes)?;
        let order = profile::decode_explicit_order(&order_bytes)?;
        let mut reports = Vec::with_capacity(order.active_plugins.len());
        for name in &order.active_plugins {
            let normalized = fallout_data::identity::plugin_name(name)?;
            let extension = Path::new(&normalized)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !matches!(extension, "esm" | "esp" | "esl") {
                return Err(Error::Unsupported(format!(
                    "unsupported active plugin extension in {name:?}"
                )));
            }
            let path = data.join(name);
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(Error::Unsupported(format!(
                    "active plugin is not a regular file: {}",
                    path.display()
                )));
            }
            let canonical = path.canonicalize()?;
            if canonical.parent() != Some(data.as_path()) {
                return Err(Error::Unsupported(format!(
                    "active plugin resolves outside the selected Data directory: {}",
                    path.display()
                )));
            }
            eprintln!("Mapping active plugin {}", canonical.display());
            let report = plugin::inspect(&canonical)?;
            reports.push(report);
        }
        let mapping = profile::RuntimeProfile::build(&reports, &order.active_plugins)?;
        let plugin_findings = reports
            .iter()
            .filter(|report| report.issue_count != 0)
            .map(|report| profile::PluginFindings {
                file: report.file.clone(),
                count: report.issue_count,
                examples: report.issue_examples.clone(),
            })
            .collect::<Vec<_>>();
        let order_name = order_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Unsupported("order input has no Unicode file name".into()))?;
        let (bytes, sha256) = digest_reader(&mut Cursor::new(order_bytes.as_slice()))?;
        let report = profile::Report {
            schema_version: 1,
            order_input: profile::OrderFingerprint {
                file_name: order_name.into(),
                bytes,
                sha256,
            },
            mapping,
            plugin_findings,
        };
        let encoded = serde_json::to_vec_pretty(&report)?;
        if let Some(output) = output {
            publish(&output, &data, &encoded)?;
        } else {
            std::io::stdout().lock().write_all(&encoded)?;
        }
        eprintln!(
            "{} active plugins mapped; this does not establish live game order or winners",
            report.mapping.plugins.len()
        );
        return Ok(if report.plugin_findings.is_empty() {
            0
        } else {
            2
        });
    }
    if command == "trace-missing" {
        let report = trace::inspect(&data, |p| eprintln!("Tracing {p}"))?;
        let bytes = serde_json::to_vec_pretty(&report)?;
        if let Some(output) = output {
            publish(&output, &report.data, &bytes)?;
        } else {
            std::io::stdout().lock().write_all(&bytes)?;
        }
        eprintln!(
            "{} absent script paths; {} attachments; {} definitions; {} direct references; {} actor-path definitions; {} actor-path links; {} unresolved actor links; {} unresolved actor identities",
            report.missing_script_paths.len(),
            report.attachments.len(),
            report.definition_candidates.len(),
            report.direct_references.len(),
            report.actor_paths.actor_definitions.len(),
            report.actor_paths.links.len(),
            report.actor_paths.unresolved_links.len(),
            report.actor_paths.unresolved_source_record_keys
        );
        return Ok(
            if report.vmad_failures
                + report.unresolved_selected_record_keys
                + report.unresolved_link_indices
                == 0
                && report.actor_paths.unresolved_links.is_empty()
                && report.actor_paths.unresolved_source_record_keys == 0
                && report.unlocated_script_paths.is_empty()
            {
                0
            } else {
                2
            },
        );
    }
    if command == "export-bindings" {
        let data = data.canonicalize()?;
        let summary = if let Some(output) = output {
            publish_with(&output, data.parent().unwrap(), |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary = bindings::export(&data, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            bindings::export(&data, &mut std::io::stdout().lock())?
        };
        eprintln!(
            "{} VMAD bindings; {} decoded tails; {} failures",
            summary.bindings, summary.decoded_tails, summary.failures
        );
        return Ok(if summary.failures == 0 { 0 } else { 2 });
    }
    if command == "export-stat-models" {
        let data = data.canonicalize()?;
        let summary = if let Some(output) = output {
            publish_with(&output, data.parent().unwrap(), |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary = static_models::export(&data, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            static_models::export(&data, &mut std::io::stdout().lock())?
        };
        eprintln!(
            "{} STAT records; {} EDIDs ({} findings); {} MODL paths; {} distant-LOD paths; {} model path findings; {} LOD shape findings",
            summary.stat_records,
            summary.editor_id_fields,
            summary.malformed_editor_ids,
            summary.model_path_fields,
            summary.distant_lod_paths,
            summary.malformed_model_paths,
            summary.malformed_lod_shapes
        );
        return Ok(
            if summary.malformed_model_paths == 0
                && summary.malformed_editor_ids == 0
                && summary.malformed_lod_shapes == 0
                && summary.records_without_modl == 0
                && summary.duplicate_model_fields == 0
            {
                0
            } else {
                2
            },
        );
    }
    if command == "export-generic-models" {
        let data = data.canonicalize()?;
        let summary = if let Some(output) = output {
            publish_with(&output, data.parent().unwrap(), |file| {
                let mut writer = std::io::BufWriter::new(file);
                let summary = generic_models::export(&data, &mut writer)?;
                writer.flush()?;
                Ok(summary)
            })?
        } else {
            generic_models::export(&data, &mut std::io::stdout().lock())?
        };
        eprintln!(
            "{} schema-selected records across {} kinds; {} MODL fields; {} normalized references; {} path findings",
            summary.records_scanned,
            summary.records_by_kind.len(),
            summary.model_path_fields,
            summary.normalized_model_references,
            summary.malformed_model_paths
        );
        return Ok(if summary.malformed_model_paths == 0 {
            0
        } else {
            2
        });
    }
    if command == "inspect-plugin" {
        let data = data.canonicalize()?;
        let report = plugin::inspect(&data)?;
        let bytes = serde_json::to_vec_pretty(&report)?;
        if let Some(output) = output {
            publish(&output, data.parent().unwrap(), &bytes)?;
        } else {
            std::io::stdout().lock().write_all(&bytes)?;
        }
        eprintln!(
            "{} records; {} decoded VMAD tails; {} input findings",
            report.records, report.vmad_decoded_tails, report.issue_count
        );
        return Ok(if report.issue_count == 0 { 0 } else { 2 });
    }
    let report = census::inspect(&data, version, |p| eprintln!("Inspecting {}", p.display()))?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(output) = output {
        publish(&output, &data, &bytes)?;
    } else {
        std::io::stdout().lock().write_all(&bytes)?;
    }
    eprintln!(
        "{} plugins, {} archives, {} script header tuples, {} PEX header findings, {} blocking input findings; gameplay unimplemented",
        report.plugins.len(),
        report.archives.len(),
        report.script_header_counts.len(),
        report.script_header_findings,
        report.blocking_findings,
    );
    Ok(if report.blocking_findings == 0 { 0 } else { 2 })
}
fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_installation_is_never_a_report_destination_and_previous_report_survives() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("Data");
        std::fs::create_dir(&data).unwrap();
        assert!(publish(&root.path().join("report.json"), &data, b"{}").is_err());
        let dest = tempfile::tempdir().unwrap();
        let output = dest.path().join("report.json");
        publish(&output, &data, b"first").unwrap();
        assert!(publish(&output, &data, b"second").is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"first");
    }
    #[test]
    fn failed_stream_does_not_publish_a_partial_report() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let output = dest.path().join("partial.jsonl");
        let result: Result<()> = publish_with(&output, source.path(), |writer| {
            writer.write_all(b"partial")?;
            Err(Error::Unsupported("synthetic failure".into()))
        });
        assert!(result.is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
    }
}
