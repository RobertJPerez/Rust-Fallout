use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum RuntimeCommand {
    /// Compare shared native/condition entry routing over explicit host state.
    PrimitiveQueryState {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
    },
    /// Repeat shared query traces in a cold source-bound process.
    PrimitiveQueryLoadProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        query_inputs: PathBuf,
    },
    /// Validate explicit host item mutations against winning source headers.
    SourceItemState {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
    },
    /// Repeat source-bound item checks after cold native restoration.
    SourceItemLoadProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        query_inputs: PathBuf,
    },
    /// Probe explicit mutable item state, queries and owned native persistence.
    ItemState {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
    },
    /// Cold-restore an item probe and query explicit subjects/items from a file.
    ItemLoadProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        query_inputs: PathBuf,
    },
    /// Probe compiled foreign locals through explicit host live script instances.
    ForeignContext {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        new_repository: Option<PathBuf>,
    },
    /// Cold-restore an engineering save and repeat every compiled foreign lookup.
    ForeignLoadProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        player_id: u64,
    },
    /// Exercise filesystem save/recovery on explicit engineering state in a new directory.
    NativeSaveProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
        /// Explicit bounded engineering transaction checked before creating a save.
        #[arg(long)]
        engineering_event_commit: Option<PathBuf>,
    },
    /// Restore a native save in a fresh process against exact original content.
    NativeLoadProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
    },
    /// Observe source-bound current/previous availability without selection or repair.
    NativeSlotAvailability {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
    },
    /// Exercise a single asynchronous native restore and explicit host admission.
    NativeRestoreProbe {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        request_id: std::num::NonZeroU64,
        #[arg(long)]
        recover_previous: bool,
    },
    /// Explicitly import our schema-2 native save into a new repository.
    NativeMigrateV2 {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
    },
    /// Explicitly import our schema-3 native save, keeping pose/enable unavailable.
    NativeMigrateV3 {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        new_repository: PathBuf,
    },
    /// Inspect native container integrity without loading or changing game state.
    NativeSaveFile {
        #[arg(long)]
        file: PathBuf,
    },
    /// Inspect compiled local schemas and exercise native canonical state.
    ScriptState {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        /// Explicit bounded engineering inputs for a staged local/event-head commit.
        #[arg(long)]
        engineering_event_commit: Option<PathBuf>,
        /// Source-bound canonical snapshot for a read-only host input check.
        #[arg(
            long,
            requires = "host_requirements",
            conflicts_with = "engineering_event_commit"
        )]
        host_snapshot: Option<PathBuf>,
        /// Explicit canonical data requirements; grants no execution authority.
        #[arg(
            long,
            requires = "host_snapshot",
            conflicts_with = "engineering_event_commit"
        )]
        host_requirements: Option<PathBuf>,
    },
    /// Prepare bounded source windows for explicit engineering pending events.
    EventFrames {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
        /// Capture one historical owned frame for a consumer thread after World drop.
        #[arg(
            long,
            requires = "snapshot_input",
            conflicts_with = "comparison_bundle"
        )]
        owned_observation: Option<PathBuf>,
        #[arg(long, requires = "owned_observation")]
        snapshot_input: Option<PathBuf>,
    },
    /// Author bounded source fixtures and trace shape without original expectations.
    ScriptFixture {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        profile_receipt: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Compare imported semantic captures against exact prepared script sources.
    ScriptTrace {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        profile_receipt: PathBuf,
        #[arg(long)]
        original_trace: Option<PathBuf>,
        #[arg(long)]
        replacement_trace: Option<PathBuf>,
        /// Produce engineering own-local copies through canonical commit APIs.
        #[arg(long, conflicts_with = "replacement_trace")]
        replacement_copy: Option<PathBuf>,
    },
    /// Inspect source operands against explicit live engineering storage.
    EventOperands {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        /// Explicit identity already present in the engineering world.
        #[arg(long)]
        player_id: Option<u64>,
        /// Prepare immutable source plans once and reuse them across events.
        #[arg(long)]
        prepared_sources: bool,
        /// Inventory source-bound native capabilities with faithful rejection.
        #[arg(long)]
        native_capabilities: bool,
        /// Exercise one source-bound local copy over explicit engineering inputs.
        #[arg(long, conflicts_with = "native_capabilities")]
        engineering_local_copy: Option<PathBuf>,
        /// Consume a saved journal head using explicit engineering activation/intent.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_copy_request: Option<PathBuf>,
        /// Observe explicitly selected native occurrences from saved state.
        #[arg(long, group = "saved_snapshot_request", requires = "snapshot_input", conflicts_with_all = ["quest_boot_request", "quest_boot_output", "snapshot_copy_request", "snapshot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_native_request: Option<PathBuf>,
        /// Create one explicitly selected source-attached quest owner in a private result.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "quest_boot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_native_request", "snapshot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        quest_boot_request: Option<PathBuf>,
        #[arg(long, requires = "quest_boot_request")]
        quest_boot_output: Option<PathBuf>,
        /// Strict current canonical snapshot; no migration or engineering seeding.
        #[arg(long, requires = "saved_snapshot_request")]
        snapshot_input: Option<PathBuf>,
        /// Fresh snapshot artifact, written only after canonical copy commit.
        #[arg(long, requires = "snapshot_copy_request")]
        snapshot_output: Option<PathBuf>,
    },
    /// Exercise shared source ownership and canonical state across a worker.
    SharedRuntime {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
    },
}
