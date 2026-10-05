use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum PhysicsCommand {
    /// Classify explicit source points against exact selected navigation triangles.
    NavigationEndpoints {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Advance or cancel a bounded source navigation search without restarting.
    NavigationSearch {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Certify an internally generated same-cell source triangle corridor.
    NavigationCorridor {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Inspect an explicit source CELL set under shared navigation and route limits.
    NavigationCellSet {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Inspect source-authored selected-cell navigation; optionally request a bounded route.
    NavigationRoute {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        request: Option<PathBuf>,
    },
    /// Query selected engineering collision through sealed cell source residency.
    CellCollision {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        source_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Query an explicit resident model subset under one collision budget.
    CellCollisionSelection {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        source_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Query selected collision bound to existing canonical references in a strict saved World.
    ReferenceCollision {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        save_root: PathBuf,
        #[arg(long)]
        editor_id: String,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        source_cache: Option<PathBuf>,
        #[arg(long)]
        request: PathBuf,
    },
    /// Derive one exact NIF collision attachment and query its source geometry.
    CollisionAttachment {
        input: PathBuf,
        #[arg(long)]
        request: PathBuf,
    },
    /// Propose an explicitly scoped engineering source-sphere sweep.
    CollisionSweep {
        input: PathBuf,
        #[arg(long)]
        request: PathBuf,
    },
    /// Decode authored NV collision data; optionally compare a raw nifly oracle report.
    NifCollision {
        input: PathBuf,
        #[arg(long)]
        oracle_report: Option<PathBuf>,
        /// Explicit frozen-body engineering ray/overlap request (one input file).
        #[arg(long, conflicts_with = "oracle_report")]
        query_request: Option<PathBuf>,
    },
}
