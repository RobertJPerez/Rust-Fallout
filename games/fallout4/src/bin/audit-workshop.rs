//! Inventories physical Fallout 4 workshop records without resolving runtime identity.
use fallout_data::plugin::{self, Event};
use fallout4_prep::{Error, Result, census, formid, workshop};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Component, Path, PathBuf},
};

const MAX_PROOF_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PLUGINS: usize = 65_536;

#[derive(Clone, Copy)]
struct FormIdRecord<'a> {
    plugin: &'a str,
    plugin_sha256: &'a str,
    listed_master_count: usize,
    kind: &'a str,
    offset: u64,
    version: u16,
    form_id_raw: u32,
}

struct FormIdSlot<'a> {
    field: &'a str,
    occurrence: usize,
    subrecord: Option<&'a str>,
    payload_offset: Option<usize>,
    raw: u32,
}

fn write_formid_slot(
    out: &mut impl Write,
    census_hash: &str,
    record: FormIdRecord<'_>,
    slot: FormIdSlot<'_>,
    counts: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<()> {
    let observation = formid::observe(slot.raw, record.listed_master_count);
    let field_key = format!("{}.{}", record.kind, slot.field);
    let pattern_key = format!("{:?}", observation.pattern);
    *counts
        .entry(field_key.clone())
        .or_default()
        .entry(pattern_key.clone())
        .or_default() += 1;
    serde_json::to_writer(
        &mut *out,
        &json!({
            "source_census_sha256":census_hash,
            "plugin":record.plugin,
            "plugin_sha256":record.plugin_sha256,
            "listed_master_count":record.listed_master_count,
            "record_kind":record.kind,
            "record_offset":record.offset,
            "record_version":record.version,
            "record_form_id_raw":record.form_id_raw,
            "field":slot.field,
            "occurrence":slot.occurrence,
            "subrecord":slot.subrecord,
            "payload_offset":slot.payload_offset,
            "form_id":observation
        }),
    )?;
    out.write_all(b"\n")?;
    Ok(())
}

fn write_record_header_formid(
    out: &mut impl Write,
    census_hash: &str,
    record: FormIdRecord<'_>,
    counts: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<()> {
    write_formid_slot(
        out,
        census_hash,
        record,
        FormIdSlot {
            field: "MajorRecord.FormID",
            occurrence: 0,
            subrecord: None,
            payload_offset: None,
            raw: record.form_id_raw,
        },
        counts,
    )
}

fn write_links(
    out: &mut impl Write,
    census_hash: &str,
    record: FormIdRecord<'_>,
    field: &str,
    links: &[workshop::RawFormId],
    counts: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<()> {
    for (occurrence, link) in links.iter().enumerate() {
        write_formid_slot(
            out,
            census_hash,
            record,
            FormIdSlot {
                field,
                occurrence,
                subrecord: Some(&link.subrecord),
                payload_offset: Some(link.payload_offset),
                raw: link.raw,
            },
            counts,
        )?;
    }
    Ok(())
}

fn write_recipe_formids(
    out: &mut impl Write,
    census_hash: &str,
    listed_master_count: usize,
    recipe: &workshop::Recipe,
    counts: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<()> {
    let record = FormIdRecord {
        plugin: &recipe.plugin,
        plugin_sha256: &recipe.plugin_sha256,
        listed_master_count,
        kind: "COBJ",
        offset: recipe.record_offset,
        version: recipe.record_version,
        form_id_raw: recipe.form_id_raw,
    };
    write_record_header_formid(out, census_hash, record, counts)?;
    for (field, links) in [
        ("CreatedObject", &recipe.created_objects),
        ("WorkbenchKeyword", &recipe.workbench_keywords),
        ("MenuArtObject", &recipe.menu_art_objects),
        ("PickUpSound", &recipe.pickup_sounds),
        ("PutDownSound", &recipe.putdown_sounds),
        ("Category", &recipe.categories),
    ] {
        write_links(out, census_hash, record, field, links, counts)?;
    }
    let components: Vec<_> = recipe
        .components
        .iter()
        .map(|row| row.component.clone())
        .collect();
    write_links(out, census_hash, record, "Component", &components, counts)?;
    for (condition_index, condition) in recipe.decoded_conditions.iter().enumerate() {
        let offset = condition.payload_offset;
        let mut slots = Vec::new();
        // Condition.xml declares Reference as a FormLink for the fixed ConditionData suffix.
        slots.push(("Condition.Reference", condition.reference_raw, offset + 24));
        if condition.comparison_value_is_global_form_id {
            slots.push((
                "Condition.ComparisonValue",
                condition.comparison_value_raw,
                offset + 4,
            ));
        }
        if condition.function_parameter_hint.parameter_one_category == "form" {
            slots.push((
                "Condition.ParameterOneRecord",
                condition.parameter_one_raw,
                offset + 12,
            ));
        }
        if condition.function_parameter_hint.parameter_two_category == "form" {
            slots.push((
                "Condition.ParameterTwoRecord",
                condition.parameter_two_raw,
                offset + 16,
            ));
        }
        for (field, raw, payload_offset) in slots {
            write_formid_slot(
                out,
                census_hash,
                record,
                FormIdSlot {
                    field,
                    occurrence: condition_index,
                    subrecord: Some("CTDA"),
                    payload_offset: Some(payload_offset),
                    raw,
                },
                counts,
            )?;
        }
    }
    Ok(())
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::Unsupported(reason.into())
}

fn checked_new_output(
    requested: &Path,
    local_dir: &Path,
    install: &Path,
    proof: &Path,
) -> Result<PathBuf> {
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    let local = fs::canonicalize(local_dir)?;
    let install = fs::canonicalize(install)?;
    let proof = fs::canonicalize(proof)?;
    if !parent.starts_with(&local) || parent.starts_with(&install) || parent.starts_with(&proof) {
        return Err(unsupported(
            "new workshop evidence must be under ignored local/, outside retail inputs and proof",
        ));
    }
    let name = requested
        .file_name()
        .filter(|name| *name != "." && *name != "..")
        .ok_or_else(|| unsupported("new output directory name required"))?;
    let output = parent.join(name);
    if output.exists() {
        return Err(unsupported(
            "output directory already exists; choose a fresh local/ directory",
        ));
    }
    Ok(output)
}

fn direct_data_file(install: &Path, relative: &str) -> Result<PathBuf> {
    let mut components = Path::new(relative).components();
    if !matches!(components.next(), Some(Component::Normal(name)) if name == "Data") {
        return Err(unsupported("source is not under direct Data"));
    }
    let Some(Component::Normal(name)) = components.next() else {
        return Err(unsupported("plugin source is not a direct Data member"));
    };
    if components.next().is_some() || name.to_string_lossy().contains(['/', '\\', ':', '\0']) {
        return Err(unsupported("unsafe plugin source path in frozen proof"));
    }
    let path = install.join("Data").join(name);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(unsupported("plugin source is not a regular file"));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(unsupported("plugin source exceeds the file byte budget"));
    }
    Ok(path)
}

fn read_bounded_json(path: &Path, max_bytes: u64) -> Result<(Value, Vec<u8>)> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(unsupported(
            "frozen proof file is missing or exceeds its byte budget",
        ));
    }
    let bytes = fs::read(path)?;
    Ok((serde_json::from_slice(&bytes)?, bytes))
}

fn load_proof(proof_dir: &Path) -> Result<(Value, String, String)> {
    let proof_dir = fs::canonicalize(proof_dir)?;
    let (complete, complete_bytes) =
        read_bounded_json(&proof_dir.join("complete.json"), 16 * 1024 * 1024)?;
    let (census_value, census_bytes) =
        read_bounded_json(&proof_dir.join("census.json"), MAX_PROOF_BYTES)?;
    let census_hash = census::sha256(&census_bytes);
    if complete["census_sha256"].as_str() != Some(&census_hash) {
        return Err(unsupported(
            "frozen proof completion marker does not bind census.json",
        ));
    }
    let plugins = census_value["plugins"]
        .as_array()
        .ok_or_else(|| unsupported("frozen census has no plugin array"))?;
    if plugins.is_empty() || plugins.len() > MAX_PLUGINS {
        return Err(unsupported(
            "frozen plugin list is empty or exceeds its budget",
        ));
    }
    Ok((census_value, census_hash, census::sha256(&complete_bytes)))
}

fn plugin_file<'a>(census_value: &'a Value, plugin_name: &str) -> Result<&'a Value> {
    let files = census_value["files"]
        .as_array()
        .ok_or_else(|| unsupported("frozen census has no file fingerprint list"))?;
    let expected = format!("Data/{plugin_name}");
    let row = files
        .iter()
        .find(|row| row["path"].as_str() == Some(&expected))
        .ok_or_else(|| unsupported("plugin has no direct Data fingerprint in the proof"))?;
    if row["bytes"].as_u64().is_none() || row["sha256"].as_str().is_none() {
        return Err(unsupported("plugin fingerprint row is incomplete"));
    }
    Ok(row)
}

fn run_audit(install_arg: &Path, proof_arg: &Path, output_arg: &Path) -> Result<()> {
    let install = fs::canonicalize(install_arg)?;
    let proof_path = fs::canonicalize(proof_arg)?;
    let local = fs::canonicalize("local")?;
    if !proof_path.starts_with(&local) {
        return Err(unsupported(
            "completed retail proof must remain under ignored local/",
        ));
    }
    let (census_value, census_hash, proof_complete_hash) = load_proof(&proof_path)?;
    let output = checked_new_output(output_arg, &local, &install, &proof_path)?;
    fs::create_dir(&output)?;
    let recipes_path = output.join("recipes.jsonl");
    let recipes_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&recipes_path)?;
    let mut recipes_out = BufWriter::new(recipes_file);
    let component_path = output.join("component-scrap.jsonl");
    let component_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&component_path)?;
    let mut component_out = BufWriter::new(component_file);
    let misc_path = output.join("misc-scrap-breakdowns.jsonl");
    let misc_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&misc_path)?;
    let mut misc_out = BufWriter::new(misc_file);
    let global_path = output.join("global-values.jsonl");
    let global_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&global_path)?;
    let mut global_out = BufWriter::new(global_file);
    let formid_inventory_path = output.join("formid-slots.jsonl");
    let formid_inventory_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&formid_inventory_path)?;
    let mut formid_inventory_out = BufWriter::new(formid_inventory_file);
    let condition_schema_path = output.join("condition-function-schema.jsonl");
    let condition_schema_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&condition_schema_path)?;
    let mut condition_schema_out = BufWriter::new(condition_schema_file);
    let mut condition_schema_rows = 0u64;
    for (function_index, hint) in fallout4_prep::condition::function_parameter_schemas() {
        serde_json::to_writer(
            &mut condition_schema_out,
            &json!({
                "source_census_sha256":census_hash,
                "function_index":function_index,
                "function_name":hint.function_name,
                "mapping_status":hint.mapping_status,
                "parameter_one_type":hint.parameter_one_type,
                "parameter_one_category":hint.parameter_one_category,
                "parameter_two_type":hint.parameter_two_type,
                "parameter_two_category":hint.parameter_two_category,
                "parameter_three_type":hint.parameter_three_type,
                "parameter_three_category":hint.parameter_three_category
            }),
        )?;
        condition_schema_out.write_all(b"\n")?;
        condition_schema_rows += 1;
    }
    if condition_schema_rows != 479 {
        return Err(unsupported(
            "pinned Fallout 4 condition schema did not yield all expected function rows",
        ));
    }
    let plugins = census_value["plugins"].as_array().unwrap();
    let mut total = workshop::Counts::default();
    let mut per_plugin = Vec::with_capacity(plugins.len());
    let mut cobj_records_census_total = 0u64;
    let mut total_cmpo_records = 0u64;
    let mut total_misc_records = 0u64;
    let mut total_misc_component_entries = 0u64;
    let mut total_component_display_index_bytes = 0u64;
    let mut total_glob_records = 0u64;
    let mut total_global_value_subrecords = 0u64;
    let mut formid_slot_counts = BTreeMap::<String, BTreeMap<String, u64>>::new();

    for plugin_row in plugins {
        let name = plugin_row["name"]
            .as_str()
            .ok_or_else(|| unsupported("plugin name missing in frozen census"))?;
        if name.is_empty() || name.bytes().any(|byte| b"/\\:\0".contains(&byte)) {
            return Err(unsupported("unsafe plugin name in frozen census"));
        }
        let source_hash = plugin_file(&census_value, name)?["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let source_bytes = plugin_file(&census_value, name)?["bytes"].as_u64().unwrap();
        let listed_master_count = plugin_row["masters"]
            .as_array()
            .ok_or_else(|| unsupported(format!("{name}: TES4 master array missing")))?
            .len();
        let expected_cobj = plugin_row["record_kinds"]["COBJ"]
            .as_u64()
            .unwrap_or_default();
        let expected_cmpo = plugin_row["record_kinds"]["CMPO"]
            .as_u64()
            .unwrap_or_default();
        let expected_misc = plugin_row["record_kinds"]["MISC"]
            .as_u64()
            .unwrap_or_default();
        let expected_glob = plugin_row["record_kinds"]["GLOB"]
            .as_u64()
            .unwrap_or_default();
        cobj_records_census_total += expected_cobj;
        let relative = format!("Data/{name}");
        let source = direct_data_file(&install, &relative)?;
        if fs::metadata(&source)?.len() != source_bytes
            || census::hash_file(&source)? != source_hash
        {
            return Err(unsupported(format!(
                "{name}: source size or hash differs from frozen proof"
            )));
        }

        let mut found = 0u64;
        let mut found_cmpo = 0u64;
        let mut found_misc = 0u64;
        let mut found_glob = 0u64;
        let mut misc_component_entries = 0u64;
        let mut component_display_index_bytes = 0u64;
        let mut global_value_subrecords = 0u64;
        let mut counts = workshop::Counts::default();
        let mut callback_error = None;
        let file = File::open(&source)?;
        let parse = plugin::visit(
            &mut BufReader::new(file),
            source_bytes,
            name,
            plugin::Limits::default(),
            |event| {
                let Event::Record(record) = event else {
                    return Ok(());
                };
                match &record.header.kind {
                    b"COBJ" => {
                        found += 1;
                        let recipe = match workshop::parse_record(record, name, &source_hash) {
                            Ok(recipe) => recipe,
                            Err(error) => {
                                callback_error = Some(error);
                                return Err(fallout_data::Error::Unsupported(
                                    "workshop recipe decoding failed".into(),
                                ));
                            }
                        };
                        counts.add(&recipe);
                        if let Err(error) = write_recipe_formids(
                            &mut formid_inventory_out,
                            &census_hash,
                            listed_master_count,
                            &recipe,
                            &mut formid_slot_counts,
                        ) {
                            callback_error = Some(error);
                            return Err(fallout_data::Error::Unsupported(
                                "raw FormID evidence output failed".into(),
                            ));
                        }
                        if let Err(error) = serde_json::to_writer(
                            &mut recipes_out,
                            &json!({"source_census_sha256":census_hash,"recipe":recipe}),
                        )
                        .and_then(|()| recipes_out.write_all(b"\n").map_err(serde_json::Error::io))
                        {
                            callback_error = Some(Error::from(error));
                            return Err(fallout_data::Error::Unsupported(
                                "workshop evidence output failed".into(),
                            ));
                        }
                    }
                    b"CMPO" => {
                        found_cmpo += 1;
                        let component = match workshop::parse_component_scrap_record(
                            record,
                            name,
                            &source_hash,
                        ) {
                            Ok(component) => component,
                            Err(error) => {
                                callback_error = Some(error);
                                return Err(fallout_data::Error::Unsupported(
                                    "component scrap decoding failed".into(),
                                ));
                            }
                        };
                        let formid_record = FormIdRecord {
                            plugin: name,
                            plugin_sha256: &source_hash,
                            listed_master_count,
                            kind: "CMPO",
                            offset: component.record_offset,
                            version: component.record_version,
                            form_id_raw: component.form_id_raw,
                        };
                        if let Err(error) = write_record_header_formid(
                            &mut formid_inventory_out,
                            &census_hash,
                            formid_record,
                            &mut formid_slot_counts,
                        )
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "CraftingSound",
                                &component.crafting_sounds,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "ScrapItem",
                                &component.scrap_items,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "ModScrapScalar",
                                &component.scrap_scalars,
                                &mut formid_slot_counts,
                            )
                        }) {
                            callback_error = Some(error);
                            return Err(fallout_data::Error::Unsupported(
                                "raw FormID evidence output failed".into(),
                            ));
                        }
                        if let Err(error) = serde_json::to_writer(
                            &mut component_out,
                            &json!({"source_census_sha256":census_hash,"component":component}),
                        )
                        .and_then(|()| {
                            component_out
                                .write_all(b"\n")
                                .map_err(serde_json::Error::io)
                        }) {
                            callback_error = Some(Error::from(error));
                            return Err(fallout_data::Error::Unsupported(
                                "component scrap evidence output failed".into(),
                            ));
                        }
                    }
                    b"MISC" => {
                        found_misc += 1;
                        let misc = match workshop::parse_misc_scrap_breakdown(
                            record,
                            name,
                            &source_hash,
                        ) {
                            Ok(misc) => misc,
                            Err(error) => {
                                callback_error = Some(error);
                                return Err(fallout_data::Error::Unsupported(
                                    "misc scrap breakdown decoding failed".into(),
                                ));
                            }
                        };
                        let formid_record = FormIdRecord {
                            plugin: name,
                            plugin_sha256: &source_hash,
                            listed_master_count,
                            kind: "MISC",
                            offset: misc.record_offset,
                            version: misc.record_version,
                            form_id_raw: misc.form_id_raw,
                        };
                        if let Err(error) = write_record_header_formid(
                            &mut formid_inventory_out,
                            &census_hash,
                            formid_record,
                            &mut formid_slot_counts,
                        )
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "PreviewTransform",
                                &misc.preview_transforms,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "PickUpSound",
                                &misc.pickup_sounds,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "PutDownSound",
                                &misc.putdown_sounds,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "Keyword",
                                &misc.keywords,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "FeaturedItemMessage",
                                &misc.featured_item_messages,
                                &mut formid_slot_counts,
                            )
                        })
                        .and_then(|()| {
                            let links: Vec<_> = misc
                                .components
                                .iter()
                                .map(|row| row.component.clone())
                                .collect();
                            write_links(
                                &mut formid_inventory_out,
                                &census_hash,
                                formid_record,
                                "Component",
                                &links,
                                &mut formid_slot_counts,
                            )
                        }) {
                            callback_error = Some(error);
                            return Err(fallout_data::Error::Unsupported(
                                "raw FormID evidence output failed".into(),
                            ));
                        }
                        misc_component_entries += misc.components.len() as u64;
                        component_display_index_bytes +=
                            misc.component_display_indices.len() as u64;
                        if let Err(error) = serde_json::to_writer(
                            &mut misc_out,
                            &json!({"source_census_sha256":census_hash,"misc":misc}),
                        )
                        .and_then(|()| misc_out.write_all(b"\n").map_err(serde_json::Error::io))
                        {
                            callback_error = Some(Error::from(error));
                            return Err(fallout_data::Error::Unsupported(
                                "misc scrap evidence output failed".into(),
                            ));
                        }
                    }
                    b"GLOB" => {
                        found_glob += 1;
                        let global = match workshop::parse_global_raw_value_record(
                            record,
                            name,
                            &source_hash,
                        ) {
                            Ok(global) => global,
                            Err(error) => {
                                callback_error = Some(error);
                                return Err(fallout_data::Error::Unsupported(
                                    "global raw-value decoding failed".into(),
                                ));
                            }
                        };
                        let formid_record = FormIdRecord {
                            plugin: name,
                            plugin_sha256: &source_hash,
                            listed_master_count,
                            kind: "GLOB",
                            offset: global.record_offset,
                            version: global.record_version,
                            form_id_raw: global.form_id_raw,
                        };
                        if let Err(error) = write_record_header_formid(
                            &mut formid_inventory_out,
                            &census_hash,
                            formid_record,
                            &mut formid_slot_counts,
                        ) {
                            callback_error = Some(error);
                            return Err(fallout_data::Error::Unsupported(
                                "raw FormID evidence output failed".into(),
                            ));
                        }
                        global_value_subrecords += global.value_subrecords.len() as u64;
                        if let Err(error) = serde_json::to_writer(
                            &mut global_out,
                            &json!({"source_census_sha256":census_hash,"global":global}),
                        )
                        .and_then(|()| global_out.write_all(b"\n").map_err(serde_json::Error::io))
                        {
                            callback_error = Some(Error::from(error));
                            return Err(fallout_data::Error::Unsupported(
                                "global raw-value evidence output failed".into(),
                            ));
                        }
                    }
                    _ => return Ok(()),
                }
                Ok(())
            },
        );
        if let Some(error) = callback_error {
            return Err(error);
        }
        parse?;
        if found != expected_cobj {
            return Err(unsupported(format!(
                "{name}: COBJ count differs from frozen census ({found} != {expected_cobj})"
            )));
        }
        if found_cmpo != expected_cmpo {
            return Err(unsupported(format!(
                "{name}: CMPO count differs from frozen census ({found_cmpo} != {expected_cmpo})"
            )));
        }
        if found_misc != expected_misc {
            return Err(unsupported(format!(
                "{name}: MISC count differs from frozen census ({found_misc} != {expected_misc})"
            )));
        }
        if found_glob != expected_glob {
            return Err(unsupported(format!(
                "{name}: GLOB count differs from frozen census ({found_glob} != {expected_glob})"
            )));
        }
        if census::hash_file(&source)? != source_hash {
            return Err(unsupported(format!("{name}: source changed during audit")));
        }
        total.recipes += counts.recipes;
        for (version, count) in &counts.record_versions {
            *total.record_versions.entry(*version).or_default() += count;
        }
        total.components += counts.components;
        total.categories += counts.categories;
        total.created_object_counts += counts.created_object_counts;
        total.raw_condition_subrecords += counts.raw_condition_subrecords;
        total.decoded_ctda_records += counts.decoded_ctda_records;
        total.opaque_subrecords += counts.opaque_subrecords;
        total.recipes_with_workbench_keyword += counts.recipes_with_workbench_keyword;
        total.recipes_with_created_object += counts.recipes_with_created_object;
        total.diagnostics += counts.diagnostics;
        for (kind, count) in &counts.uninterpreted_subrecord_kinds {
            *total
                .uninterpreted_subrecord_kinds
                .entry(kind.clone())
                .or_default() += count;
        }
        total_cmpo_records += found_cmpo;
        total_misc_records += found_misc;
        total_misc_component_entries += misc_component_entries;
        total_component_display_index_bytes += component_display_index_bytes;
        total_glob_records += found_glob;
        total_global_value_subrecords += global_value_subrecords;
        per_plugin.push(json!({
            "plugin":name,
            "source_sha256":source_hash,
            "source_bytes":source_bytes,
            "cobj_records":found,
            "cmpo_records":found_cmpo,
            "misc_records":found_misc,
            "glob_records":found_glob,
            "misc_component_entries":misc_component_entries,
            "component_display_index_bytes":component_display_index_bytes,
            "global_value_subrecords":global_value_subrecords,
            "counts":counts
        }));
    }

    recipes_out.flush()?;
    drop(recipes_out);
    component_out.flush()?;
    drop(component_out);
    misc_out.flush()?;
    drop(misc_out);
    global_out.flush()?;
    drop(global_out);
    formid_inventory_out.flush()?;
    drop(formid_inventory_out);
    condition_schema_out.flush()?;
    drop(condition_schema_out);
    if total.recipes != cobj_records_census_total {
        return Err(unsupported("total COBJ count differs from frozen census"));
    }
    let complete = json!({
        "schema":1,
        "status":"completed-physical-fo4-workshop-recipe-scrap-and-global-structure-audit",
        "source_census_sha256":census_hash,
        "source_proof_complete_sha256":proof_complete_hash,
        "private_directory":output,
        "recipes_jsonl_sha256":census::hash_file(&recipes_path)?,
        "component_scrap_jsonl_sha256":census::hash_file(&component_path)?,
        "misc_scrap_breakdowns_jsonl_sha256":census::hash_file(&misc_path)?,
        "global_values_jsonl_sha256":census::hash_file(&global_path)?,
        "formid_slots_jsonl_sha256":census::hash_file(&formid_inventory_path)?,
        "condition_function_schema_jsonl_sha256":census::hash_file(&condition_schema_path)?,
        "condition_function_schema_rows":condition_schema_rows,
        "formid_slots":formid_slot_counts.values().flat_map(|patterns| patterns.values()).sum::<u64>(),
        "formid_slot_patterns_by_field":formid_slot_counts,
        "physical_plugins_scanned":plugins.len(),
        "cobj_records_from_frozen_census":cobj_records_census_total,
        "cmpo_records_from_frozen_census":total_cmpo_records,
        "misc_records_from_frozen_census":total_misc_records,
        "glob_records_from_frozen_census":total_glob_records,
        "misc_cvpa_component_entries":total_misc_component_entries,
        "misc_cdix_display_index_bytes":total_component_display_index_bytes,
        "global_value_subrecords":total_global_value_subrecords,
        "counts":total,
        "sources_rehashed_before_and_after_each_plugin":true,
        "record_schema_revision":"pinned FO4 COBJ, CMPO, MISC and GLOB schemas plus 32-byte CTDA fixed fields and the complete 479-function condition metadata table; per-record versions retained without dialect selection",
        "plugins":per_plugin,
        "limits":[
            "physical recipe candidates only; no active load order, override winner, FormKey or ESL slot is selected",
            "CTDA fixed fields are decoded and retained with exact bytes; function parameters remain raw and no condition is evaluated",
            "Pinned-schema workshop FormLink slots are emitted separately with exact source offsets and raw bit-pattern candidates; no FormKey, ESL runtime slot or override winner is assigned",
            "CTDA parameters are included only when the pinned function map classifies the slot as a Form link; unknown/defaulted slots remain unresolved",
            "CMPO/MISC/GLOB links, type bytes and FLTV payloads remain raw structure; numeric scalar values, item yield and conversion behavior are not evaluated",
            "no material/perk inventory, placement validation, commit/refund, workshop power or settlement behavior is implemented"
        ]
    });
    let complete_path = output.join("complete.json");
    let complete_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(complete_path)?;
    let mut complete_out = BufWriter::new(complete_file);
    serde_json::to_writer_pretty(&mut complete_out, &complete)?;
    complete_out.write_all(b"\n")?;
    complete_out.flush()?;
    println!("{}", serde_json::to_string_pretty(&complete)?);
    Ok(())
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 3 {
        if let Err(error) = run_audit(
            Path::new(&args[0]),
            Path::new(&args[1]),
            Path::new(&args[2]),
        ) {
            eprintln!("{error}");
            std::process::exit(1);
        }
    } else {
        eprintln!(
            "usage: audit-workshop <install-root> <completed-proof-dir> <new-local-directory>"
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn only_new_outputs_under_local_are_accepted() {
        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().join("local");
        let install = temp.path().join("install");
        let proof = local.join("proof");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&proof).unwrap();
        let inside = local.join("recipes-new");
        assert!(checked_new_output(&inside, &local, &install, &proof).is_ok());
        assert!(checked_new_output(&install.join("out"), &local, &install, &proof).is_err());
        assert!(checked_new_output(&proof.join("out"), &local, &install, &proof).is_err());
        assert!(
            checked_new_output(&temp.path().join("outside"), &local, &install, &proof).is_err()
        );
        fs::create_dir(&inside).unwrap();
        assert!(checked_new_output(&inside, &local, &install, &proof).is_err());
    }

    #[test]
    fn proof_member_paths_must_be_direct_data_files() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path();
        fs::create_dir_all(install.join("Data")).unwrap();
        fs::write(install.join("Data/Workshop.esm"), b"test").unwrap();
        assert!(direct_data_file(install, "Data/Workshop.esm").is_ok());
        for invalid in [
            "Data/../Workshop.esm",
            "Data/sub/Workshop.esm",
            "../Data/Workshop.esm",
            "Data/a\\b.esm",
        ] {
            assert!(direct_data_file(install, invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn completion_marker_must_bind_the_frozen_census_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let proof = temp.path().join("proof");
        fs::create_dir_all(&proof).unwrap();
        fs::write(proof.join("census.json"), br#"{"plugins":[],"files":[]}"#).unwrap();
        fs::write(proof.join("complete.json"), br#"{"census_sha256":"wrong"}"#).unwrap();
        assert!(load_proof(&proof).is_err());
    }

    #[test]
    fn formid_slot_rows_keep_field_provenance_and_raw_selector_candidates() {
        let record = FormIdRecord {
            plugin: "Workshop.esl",
            plugin_sha256: "plugin-hash",
            listed_master_count: 1,
            kind: "COBJ",
            offset: 0x1234,
            version: 131,
            form_id_raw: 0x0100_0020,
        };
        let slot = FormIdSlot {
            field: "Condition.Reference",
            occurrence: 2,
            subrecord: Some("CTDA"),
            payload_offset: Some(0x88),
            raw: 0xFE01_0234,
        };
        let mut output = Vec::new();
        let mut counts = BTreeMap::new();
        write_formid_slot(&mut output, "census-hash", record, slot, &mut counts).unwrap();
        let row: Value = serde_json::from_slice(output.strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(row["plugin"], "Workshop.esl");
        assert_eq!(row["record_offset"], 0x1234);
        assert_eq!(row["record_form_id_raw"], 0x0100_0020);
        assert_eq!(row["field"], "Condition.Reference");
        assert_eq!(row["occurrence"], 2);
        assert_eq!(row["subrecord"], "CTDA");
        assert_eq!(row["payload_offset"], 0x88);
        assert_eq!(row["form_id"]["raw"], 0xFE01_0234u32);
        assert_eq!(row["form_id"]["small_selector_candidate"], 0x010);
        assert_eq!(row["form_id"]["small_low_12_candidate"], 0x234);
        assert_eq!(counts["COBJ.Condition.Reference"]["SmallMarkerPattern"], 1);
    }

    #[test]
    fn ctda_link_slots_use_their_fixed_payload_offsets_and_mapped_parameter_type() {
        let explicit_form_and_number = fallout4_prep::condition::FunctionParameterHint {
            function_name: Some("GetStageDone"),
            mapping_status: "explicit",
            parameter_one_type: "Quest",
            parameter_one_category: "form",
            parameter_two_type: "QuestStage",
            parameter_two_category: "number",
            parameter_three_type: "None",
            parameter_three_category: "none",
        };
        let defaulted_parameters = fallout4_prep::condition::FunctionParameterHint {
            function_name: Some("IsInInterior"),
            mapping_status: "function-enum-known-parameter-map-defaulted",
            parameter_one_type: "Unspecified",
            parameter_one_category: "unresolved",
            parameter_two_type: "Unspecified",
            parameter_two_category: "unresolved",
            parameter_three_type: "Unspecified",
            parameter_three_category: "unresolved",
        };
        let two_form_links = fallout4_prep::condition::FunctionParameterHint {
            function_name: Some("GetFactionRankDifference"),
            mapping_status: "explicit",
            parameter_one_type: "Faction",
            parameter_one_category: "form",
            parameter_two_type: "Actor",
            parameter_two_category: "form",
            parameter_three_type: "None",
            parameter_three_category: "none",
        };
        let recipe = workshop::Recipe {
            plugin: "Workshop.esm".into(),
            plugin_sha256: "plugin-hash".into(),
            record_offset: 0x1234,
            record_version: 131,
            record_flags: 0,
            form_id_raw: 0x0100_0020,
            editor_ids: Vec::new(),
            created_objects: Vec::new(),
            workbench_keywords: Vec::new(),
            menu_art_objects: Vec::new(),
            pickup_sounds: Vec::new(),
            putdown_sounds: Vec::new(),
            components: Vec::new(),
            categories: Vec::new(),
            created_object_counts: Vec::new(),
            condition_subrecords: Vec::new(),
            decoded_conditions: vec![
                fallout4_prep::condition::RawCondition {
                    payload_offset: 0x40,
                    payload_sha256: String::new(),
                    payload_bytes_hex: String::new(),
                    packed_flags_and_operator: 0,
                    flag_bits: 0x04,
                    compare_operator_bits: 0,
                    comparison_value_is_global_form_id: true,
                    comparison_value_raw: 0xFE02_0345,
                    unknown1: [0; 3],
                    function_index: 59,
                    unknown2: 0,
                    parameter_one_raw: 0xFE03_0456,
                    parameter_two_raw: 17,
                    function_parameter_hint: explicit_form_and_number,
                    run_on_raw: 0,
                    reference_raw: 0xFE01_0234,
                    unknown3_raw: 0,
                },
                fallout4_prep::condition::RawCondition {
                    payload_offset: 0xA0,
                    payload_sha256: String::new(),
                    payload_bytes_hex: String::new(),
                    packed_flags_and_operator: 0,
                    flag_bits: 0,
                    compare_operator_bits: 0,
                    comparison_value_is_global_form_id: false,
                    comparison_value_raw: 0xFE05_0678,
                    unknown1: [0; 3],
                    function_index: 300,
                    unknown2: 0,
                    parameter_one_raw: 0xFE06_0789,
                    parameter_two_raw: 0xFE07_089A,
                    function_parameter_hint: defaulted_parameters,
                    run_on_raw: 0,
                    reference_raw: 0xFE04_0567,
                    unknown3_raw: 0,
                },
                fallout4_prep::condition::RawCondition {
                    payload_offset: 0x120,
                    payload_sha256: String::new(),
                    payload_bytes_hex: String::new(),
                    packed_flags_and_operator: 0,
                    flag_bits: 0,
                    compare_operator_bits: 0,
                    comparison_value_is_global_form_id: false,
                    comparison_value_raw: 0xFE0B_0CDE,
                    unknown1: [0; 3],
                    function_index: 60,
                    unknown2: 0,
                    parameter_one_raw: 0xFE08_09AB,
                    parameter_two_raw: 0xFE09_0ABC,
                    function_parameter_hint: two_form_links,
                    run_on_raw: 0,
                    reference_raw: 0xFE0A_0BCD,
                    unknown3_raw: 0,
                },
            ],
            opaque_subrecords: Vec::new(),
            diagnostics: Vec::new(),
        };

        let mut output = Vec::new();
        let mut counts = BTreeMap::new();
        write_recipe_formids(&mut output, "census-hash", 1, &recipe, &mut counts).unwrap();
        let condition_rows: Vec<Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .filter(|row: &Value| row["subrecord"] == "CTDA")
            .collect();

        let observed: Vec<_> = condition_rows
            .iter()
            .map(|row| {
                (
                    row["field"].as_str().unwrap(),
                    row["occurrence"].as_u64().unwrap(),
                    row["payload_offset"].as_u64().unwrap(),
                    row["form_id"]["raw"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            observed,
            vec![
                ("Condition.Reference", 0, 0x58, 0xFE01_0234),
                ("Condition.ComparisonValue", 0, 0x44, 0xFE02_0345),
                ("Condition.ParameterOneRecord", 0, 0x4C, 0xFE03_0456),
                ("Condition.Reference", 1, 0xB8, 0xFE04_0567),
                ("Condition.Reference", 2, 0x138, 0xFE0A_0BCD),
                ("Condition.ParameterOneRecord", 2, 0x12C, 0xFE08_09AB),
                ("Condition.ParameterTwoRecord", 2, 0x130, 0xFE09_0ABC),
            ]
        );
        assert_eq!(condition_rows.len(), 7);
    }
}
