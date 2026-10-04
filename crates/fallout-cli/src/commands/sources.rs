use crate::parse_form;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum SourcesCommand {
    /// Fingerprint the installation, including loose data. Reads every source byte.
    Baseline {
        #[arg(long)]
        install: PathBuf,
    },
    /// Observe explicit NV configuration sources; runtime precedence remains unverified.
    VfsProfile {
        #[arg(long)]
        install: PathBuf,
        /// Explicit Windows Known Folder Documents root, including redirects.
        #[arg(long)]
        documents: PathBuf,
        #[arg(long)]
        local_appdata: PathBuf,
    },
    /// Capture exact original files into a fresh private package; never launches the game.
    RetailProfileCapture {
        #[arg(long)]
        install: PathBuf,
        /// Defaults to the actual Windows Documents known folder, including redirects.
        #[arg(long)]
        documents: Option<PathBuf>,
        /// Defaults to the actual Windows LocalApplicationData known folder.
        #[arg(long)]
        local_appdata: Option<PathBuf>,
        /// New directory under an existing parent, outside all source roots.
        #[arg(long)]
        package: PathBuf,
    },
    /// Verify sealed profile evidence; process-required mode refuses profile-only captures.
    RetailProfileVerify {
        #[arg(long)]
        package: PathBuf,
        /// Independently retained SHA-256 of capture.json.
        #[arg(long)]
        receipt_sha256: String,
        #[arg(long)]
        require_process: bool,
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
