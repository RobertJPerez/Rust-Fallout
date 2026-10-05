use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum RuntimeCommand {
    /// Compose explicit engineering inventory/quest boot into a new native save.
    RouteBoot {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        save_root: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
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
        /// Explicit source selections and retained state for atomic group admission.
        #[arg(long, requires = "source_reference_repository", conflicts_with_all = ["engineering_event_commit", "host_snapshot", "host_requirements"])]
        source_reference_group: Option<PathBuf>,
        /// New native repository for the explicit group before/current boundaries.
        #[arg(long, requires = "source_reference_group", conflicts_with_all = ["engineering_event_commit", "host_snapshot", "host_requirements"])]
        source_reference_repository: Option<PathBuf>,
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
        /// Engineering sequential own-local copies, one commit per complete event.
        #[arg(long, conflicts_with_all = ["replacement_trace", "replacement_copy"])]
        replacement_multi_copy: Option<PathBuf>,
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
        /// Consume an explicit existing saved journal prefix with engineering copies.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_native_request", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_copy_batch_request: Option<PathBuf>,
        /// Copy the complete source event of an existing explicit Quest/Placed owner.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_multi_copy_request: Option<Box<PathBuf>>,
        /// Copy one explicitly qualified foreign numeric local into the own saved head.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_foreign_copy_request: Option<PathBuf>,
        /// Copy one own typed reference local at the saved journal head.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_foreign_copy_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_reference_copy_request: Option<Box<PathBuf>>,
        /// Boot the exact source-attached script of an existing authored reference.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_foreign_copy_request", "snapshot_reference_copy_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        reference_boot_request: Option<Box<PathBuf>>,
        /// Queue one explicitly selected source block on an existing saved instance.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_foreign_copy_request", "snapshot_reference_copy_request", "reference_boot_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_event_request: Option<Box<PathBuf>>,
        /// Assign one exactly representable integral source token at the saved head.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_foreign_copy_request", "snapshot_reference_copy_request", "reference_boot_request", "snapshot_event_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_literal_assignment_request: Option<Box<PathBuf>>,
        /// Explicit source-native GetItemCount assignment through canonical state.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))), group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_foreign_copy_request", "snapshot_reference_copy_request", "reference_boot_request", "snapshot_event_request", "snapshot_literal_assignment_request", "snapshot_native_request", "snapshot_native_plan_request", "snapshot_native_current", "quest_boot_request", "quest_boot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_native_assignment_request: Option<Box<PathBuf>>,
        /// Observe explicitly selected native occurrences from saved state.
        #[arg(long, group = "saved_snapshot_request", requires = "snapshot_input", conflicts_with_all = ["quest_boot_request", "quest_boot_output", "snapshot_copy_request", "snapshot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_native_request: Option<PathBuf>,
        /// Reuse a source-native query plan after dropping and cold-restoring state.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "snapshot_native_current"], conflicts_with_all = ["snapshot_copy_request", "snapshot_copy_batch_request", "snapshot_native_request", "quest_boot_request", "quest_boot_output", "snapshot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        snapshot_native_plan_request: Option<PathBuf>,
        /// Strict current state queried by the owned plan from snapshot-input.
        #[arg(long, requires = "snapshot_native_plan_request")]
        snapshot_native_current: Option<PathBuf>,
        /// Create one explicitly selected source-attached quest owner in a private result.
        #[arg(long, group = "saved_snapshot_request", requires_all = ["snapshot_input", "quest_boot_output"], conflicts_with_all = ["snapshot_copy_request", "snapshot_native_request", "snapshot_output", "engineering_local_copy", "native_capabilities", "player_id", "prepared_sources"])]
        quest_boot_request: Option<PathBuf>,
        #[arg(long, requires = "quest_boot_request")]
        quest_boot_output: Option<PathBuf>,
        /// Strict current canonical snapshot; no migration or engineering seeding.
        #[arg(long, requires = "saved_snapshot_request")]
        snapshot_input: Option<PathBuf>,
        /// Fresh snapshot artifact, written only after canonical copy commit.
        #[arg(long, requires = "saved_snapshot_request", value_parser = clap::builder::TypedValueParser::map(clap::builder::OsStringValueParser::new(), |value| Box::new(PathBuf::from(value))))]
        snapshot_output: Option<Box<PathBuf>>,
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
