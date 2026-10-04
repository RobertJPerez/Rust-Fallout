use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum ScriptsCommand {
    /// Bind authored winning CTDA operands and static form dependencies.
    ConditionDependencies {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        include_source_owners: bool,
        #[arg(long)]
        include_source_runs: bool,
        /// Read one explicit engineering query from a canonical snapshot; no condition truth.
        #[arg(long)]
        engineering_query_input: Option<PathBuf>,
        /// Observe an ordered batch of physical condition sites with shared budgets.
        #[arg(long, conflicts_with = "engineering_query_input")]
        engineering_query_batch: Option<PathBuf>,
        /// Query explicit sites/subjects across several exact source records.
        #[arg(long, conflicts_with_all = ["engineering_query_input", "engineering_query_batch", "include_source_owners", "include_source_runs"])]
        engineering_query_records: Option<PathBuf>,
    },
    /// Hash original compressed record inputs and exact decoded outputs.
    CompressedRecords {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        inspect_checksum_mismatches: bool,
    },
    /// Link authored quest scripts and inspect foreign declarations without values.
    QuestScripts {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        script_comparison_bundle: Option<PathBuf>,
        #[arg(long)]
        quest_comparison_bundle: Option<PathBuf>,
    },
    /// Load immutable winning scripts, source owners and reference dependencies.
    LoadedScripts {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Decode authored condition fields, retaining short layouts and raw parameters.
    Conditions {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Associate all supported compiled operands with their owning script tables.
    OperandBindings {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Decode native operands from vanilla compiled scripts; invokes no handler.
    NativeArguments {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Prepare exact winning script versions and owning source-table bindings.
    SourcePlans {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
        /// Exact source roots/cohort for bounded execution capability diagnostics.
        #[arg(long)]
        execution_admission: Option<PathBuf>,
        /// Bounded cooperative cache preparation; optional cancellation-by-drop.
        #[arg(long)]
        cooperative_preparation: Option<PathBuf>,
        /// Prepare only explicit exact script handles; infers no dependencies.
        #[arg(long)]
        selected_source: Option<PathBuf>,
    },
    /// Match source delimiters and raw distances in an offline SCDA bundle.
    ControlFlow {
        /// Installation protected from report output; the bundle stays read-only.
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        diagnose_structure: bool,
    },
    /// Build source-token postfix structure from a hash-bound offline bundle.
    ExpressionPlans {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        /// Preserve structural findings in a diagnostic report; still returns 1.
        #[arg(long)]
        diagnose_structure: bool,
    },
    /// Inspect vanilla expression tokens without evaluating or executing them.
    Expressions {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Associate authored script caller references with each unit's own tables.
    #[command(name = "script-bindings")]
    Bindings {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        defer_unrelated_payloads: bool,
        /// Local raw decoded records for an independent metadata comparison.
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
    /// Inspect vanilla command/event metadata from the exact pinned executable.
    #[command(name = "command-catalogue")]
    Catalogue {
        #[arg(long)]
        install: PathBuf,
    },
    /// Inventory compiled script instruction headers and event IDs; executes nothing.
    Scripts {
        #[arg(long)]
        install: PathBuf,
        /// Read only record kinds containing SCDA in the pinned FNV schema.
        #[arg(long)]
        defer_unrelated_payloads: bool,
        /// Write raw SCDA bodies for a separate offline framing comparison.
        #[arg(long)]
        comparison_bundle: Option<PathBuf>,
    },
}
