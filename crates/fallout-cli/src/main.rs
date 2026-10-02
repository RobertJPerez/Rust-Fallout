use clap::{Parser, Subcommand};
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

#[derive(Subcommand)]
enum Command {
    /// Resolve and verify external texture dependencies from a NIF or model cache directory.
    NifAssets {
        input: PathBuf,
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        texture_cache: Option<PathBuf>,
    },
    /// Decode NV scene nodes and mesh payloads from a local NIF or cached blob.
    NifScene { input: PathBuf },
    /// Inventory all archived NIF/KF containers and retain every unsupported member.
    NifCensus {
        #[arg(long)]
        install: PathBuf,
        /// Also decode supported scene/mesh blocks; list each payload failure separately.
        #[arg(long)]
        inspect_scenes: bool,
    },
    /// Inspect a real CELL and its winning placements, base forms, and model candidates.
    Cell {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        inspect_checksum_mismatches: bool,
        /// Decode unambiguous model candidates and inspect their NIF containers.
        #[arg(long)]
        inspect_models: bool,
        /// Cache decoded model bytes outside the installation for independent tools.
        #[arg(long, requires = "inspect_models")]
        model_cache: Option<PathBuf>,
    },
    /// Dry-run source preparation with digests and master dependencies; writes no assets.
    Plan {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        inspect_checksum_mismatches: bool,
    },
    /// Fingerprint the installation, including loose data. Reads every source byte.
    Baseline {
        #[arg(long)]
        install: PathBuf,
    },
    /// Count all top-level ESM/ESP records and BSA entries, preserving unknowns.
    Census {
        #[arg(long)]
        install: PathBuf,
        /// Continue checksum-only defects for diagnosis; tainted records stay untrusted.
        #[arg(long)]
        inspect_checksum_mismatches: bool,
    },
    /// Show a definition's raw header, source offsets, and bounded field previews.
    Inspect {
        plugin: PathBuf,
        #[arg(long,value_parser=parse_form)]
        form: u32,
    },
    /// Resolve an explicit JSON array of plugin names. Never changes the retail order.
    Resolve {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        /// Inspect chains around checksum defects; exits unsuccessfully if any remain.
        #[arg(long)]
        inspect_checksum_mismatches: bool,
    },
    /// Decode and hash one archive member; optionally cache it outside the installation.
    Asset {
        archive: PathBuf,
        path: String,
        #[arg(long)]
        cache_root: Option<PathBuf>,
        /// Validate NV NIF container tables and inventory block types.
        #[arg(long)]
        inspect_nif: bool,
    },
}

fn parse_form(raw: &str) -> std::result::Result<u32, String> {
    u32::from_str_radix(raw.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
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
        Command::NifAssets {
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
        Command::NifScene { input } => {
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
        Command::NifCensus {
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
        Command::Cell {
            install,
            load_order,
            editor_id,
            inspect_checksum_mismatches,
            inspect_models,
            model_cache,
        } => {
            let names: Vec<String> = serde_json::from_reader(baseline::open_source(&load_order)?)?;
            let mut store = fallout_data::store::RecordStore::open_nv(
                &install.join("Data"),
                &names,
                plugin::Limits {
                    inspect_checksum_mismatches,
                    ..Default::default()
                },
            )?;
            let mut mounts = MountIndex::default();
            for path in data_files(&install, &["bsa"])? {
                NvArchive::open(&path)?.census(&mut mounts)?;
            }
            let mut report =
                fallout_data::world::inspect_cell(&mut store, editor_id.as_bytes(), &mounts)?;
            if inspect_models {
                fallout_data::model_probe::inspect_models(
                    &mut report,
                    &install,
                    model_cache.as_deref(),
                )?;
            }
            let clean = report.integrity_failures == 0
                && report.link_failures == 0
                && report.model_probes.iter().all(|p| p.error.is_none());
            emit(&report, output, &install)?;
            if !clean {
                return Err("cell inspection contains integrity, reference, or model failures; runtime acceptance remains blocked".into());
            }
        }
        Command::Plan {
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
        Command::Baseline { install } => {
            let report =
                baseline::discover(&install, |path| eprintln!("Hashing {}", path.display()))?;
            let complete = report.missing_required.is_empty();
            emit(&report, output, &install)?;
            if !complete {
                return Err("baseline is missing required inputs; see the report".into());
            }
        }
        Command::Census {
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
        Command::Inspect { plugin: path, form } => {
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
        Command::Resolve {
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
        Command::Asset {
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
