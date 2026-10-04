use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum PhysicsCommand {
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
