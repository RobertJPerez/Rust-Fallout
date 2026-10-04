use crate::actor_inspection;
use clap::Subcommand;
use fallout_data::identity;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum ActorsCommand {
    /// Preserve authored NPC_/CREA scalar words with exact inventory provenance.
    ActorSources {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long)]
        compare_oracle: Option<PathBuf>,
        #[arg(long)]
        include_associations: bool,
        #[arg(long)]
        include_classes: bool,
        #[arg(long)]
        include_factions: bool,
        #[arg(long)]
        include_placements: bool,
        #[arg(long)]
        include_races: bool,
        #[arg(long)]
        include_packages: bool,
        #[arg(long, requires = "include_packages")]
        include_package_dependencies: bool,
        #[arg(long, requires = "include_packages", value_parser = actor_inspection::parse_root)]
        package_destination: Option<identity::FormKey>,
        #[arg(long)]
        include_dependencies: bool,
        #[arg(long = "dependency-root", requires = "include_dependencies", value_parser = actor_inspection::parse_root)]
        dependency_roots: Vec<identity::FormKey>,
        #[arg(long, requires_all = ["include_dependencies", "dependency_roots"])]
        include_render_dependencies: bool,
        #[arg(long, requires_all = ["include_dependencies", "dependency_roots"])]
        include_template_dependencies: bool,
        /// Explicit caller-selected equipment winner; requires one actor root and a role.
        #[arg(long, requires_all = ["include_dependencies", "dependency_roots", "equipment_role"], value_parser = actor_inspection::parse_root)]
        equipment_source: Option<identity::FormKey>,
        #[arg(long, requires = "equipment_source", value_parser = actor_inspection::parse_equipment_role)]
        equipment_role: Option<fallout_data::actors::dependencies::equipment::Role>,
        #[arg(long, value_parser = actor_inspection::parse_root)]
        voice_root: Option<identity::FormKey>,
        #[arg(long, value_parser = actor_inspection::parse_root)]
        script_root: Option<identity::FormKey>,
        #[arg(long, requires_all = ["include_dependencies", "dependency_roots"], value_parser = actor_inspection::parse_creature_directory)]
        creature_model_directory: Option<fallout_data::vfs::AssetPath>,
    },
    /// Observe authored PKID/CTDA requests over explicitly restored canonical state.
    ActorPackageContext {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long, value_parser = actor_inspection::parse_root)]
        actor_root: identity::FormKey,
        #[arg(long)]
        native_snapshot: PathBuf,
        #[arg(long)]
        explicit_subject: Option<std::num::NonZeroU64>,
        #[arg(long)]
        engineering_observation: bool,
        /// Exact pinned descriptor image, also usable with authored plugin fixtures.
        #[arg(long)]
        condition_executable: Option<PathBuf>,
        /// Preserve authored SNAM/FACT relationship requests without live faction rules.
        #[arg(long)]
        include_faction_requests: bool,
        /// Join raw actor scalars to source template categories; evaluated values remain unavailable.
        #[arg(long)]
        include_stat_requests: bool,
        /// Report the exact refusal for a package operation; never execute AI.
        #[arg(long, value_parser = actor_inspection::parse_package_operation)]
        package_capability: Option<fallout_runtime::actor_rules::packages::Operation>,
        #[arg(long, requires = "explicit_subject")]
        include_actor_context: bool,
        #[arg(long, requires = "explicit_subject")]
        equipment_item: Option<std::num::NonZeroU64>,
        #[arg(long, requires = "explicit_subject")]
        inventory_boot_request: Option<PathBuf>,
    },
    /// Preserve winning base inventory entries, ownership words and template inputs.
    BaseInventory {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
    },
    /// Preserve winning ordered form-list members and structural dependencies.
    FormLists {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long, requires = "root_id")]
        root_plugin: Option<String>,
        #[arg(long, requires = "root_plugin")]
        root_id: Option<u32>,
    },
    /// Preserve authored leveled lists and plan inventory/template dependencies.
    LeveledLists {
        #[arg(long)]
        install: PathBuf,
        #[arg(long)]
        load_order: PathBuf,
        #[arg(long)]
        index_cache: Option<PathBuf>,
        #[arg(long, requires = "root_id")]
        root_plugin: Option<String>,
        #[arg(long, requires = "root_plugin")]
        root_id: Option<u32>,
    },
}
