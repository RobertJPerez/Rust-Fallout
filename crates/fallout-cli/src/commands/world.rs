use crate::parse_cell_key;
use clap::Subcommand;
use fallout_data::identity;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum WorldCommand {
    /// Index winning INFO records by authored topic-child group membership.
    DialogueMembership {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
    },
    /// Inspect exact cell model and texture residency through bounded source jobs.
    CellResidencySources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        /// Private decoded-member cache outside the source installation.
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        cell: String,
        /// Per-stage worker polling deadline; planning/fingerprinting is separate.
        #[arg(long, default_value_t = 30_000)]
        source_timeout_ms: u64,
    },
    /// Inspect source-selected door destination CELL model/texture residency.
    DoorResidencySources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
        /// Explicit source CELL, not a changed runtime current cell.
        #[arg(long)]
        cell: String,
        #[arg(long)]
        door: String,
        #[arg(long, default_value_t = 30_000)]
        source_timeout_ms: u64,
    },
    /// Inspect a unique explicit WRLD/XCLC grid CELL through bounded source jobs.
    GridResidencySources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        world: String,
        #[arg(long, allow_hyphen_values = true)]
        grid_x: i32,
        #[arg(long, allow_hyphen_values = true)]
        grid_y: i32,
        #[arg(long, default_value_t = 30_000)]
        source_timeout_ms: u64,
        /// Include strict terrain textures in this CELL's residency epoch.
        #[arg(long)]
        include_terrain: bool,
    },
    /// Inspect an explicit WRLD/XCLC CELL's strict terrain texture source jobs.
    GridTerrainSources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        world: String,
        #[arg(long, allow_hyphen_values = true)]
        grid_x: i32,
        #[arg(long, allow_hyphen_values = true)]
        grid_y: i32,
        #[arg(long, default_value_t = 30_000)]
        source_timeout_ms: u64,
    },
    /// Prepare an explicitly requested winning topic/INFO for source consumers.
    ConversationSources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        /// Canonical origin-plugin:local-hex-id; never a runtime load-order slot.
        #[arg(long)]
        topic: String,
        #[arg(long)]
        info: String,
        #[arg(long)]
        speaker: Option<String>,
        #[arg(long)]
        bind_result_fragments: bool,
    },
    /// Preserve authored quest/dialogue sections and condition/script ownership.
    Narrative {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Decode an exterior CELL, its parent worlds and winning LAND source fields.
    Terrain {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        /// Cache tagged decoded bodies outside the installation for offline comparison.
        #[arg(long)]
        body_cache: Option<PathBuf>,
        #[arg(long, requires = "body_cache")]
        oracle_report: Option<PathBuf>,
        /// Convert VHGT using the pinned ESM4 height convention; source fields stay intact.
        #[arg(long)]
        reconstruct_heights: bool,
        /// Inspect source-local mesh geometry; retail topology remains unmeasured.
        #[arg(long, requires = "reconstruct_heights")]
        inspect_mesh: bool,
        /// Resolve LTEX/TXST records and verify authored texture archive bytes.
        #[arg(long)]
        inspect_textures: bool,
        /// Prepare exact one-LAND texture sources through bounded cancellable jobs.
        #[arg(long, requires = "inspect_textures", conflicts_with = "body_cache")]
        prepare_textures: bool,
        /// Expand authored quadrant alpha samples under the inspection blend model.
        #[arg(long)]
        inspect_blends: bool,
        #[arg(long, requires = "inspect_textures")]
        texture_cache: Option<PathBuf>,
        /// Compare an explicitly selected cardinal neighbor in the same worldspace.
        #[arg(
            long,
            requires = "reconstruct_heights",
            conflicts_with = "neighbor_form"
        )]
        neighbor_editor_id: Option<String>,
        /// Unnamed neighbor: origin plugin and local hexadecimal ID, e.g. FalloutNV.esm:DAEB9.
        #[arg(long, requires = "reconstruct_heights", value_parser = parse_cell_key)]
        neighbor_form: Option<identity::FormKey>,
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
        /// Validate headers/CELL metadata now and other record bodies on access.
        #[arg(long, conflicts_with = "inspect_checksum_mismatches")]
        defer_unread_payloads: bool,
        /// Reuse source-bound plugin metadata from an existing cache outside the installation.
        #[arg(long, requires = "defer_unread_payloads")]
        index_cache: Option<PathBuf>,
        /// Decode unambiguous model candidates and inspect their NIF containers.
        #[arg(long)]
        inspect_models: bool,
        /// Include a bounded source-only CELL/WRLD/placement dependency graph.
        #[arg(long)]
        include_dependencies: bool,
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
}
