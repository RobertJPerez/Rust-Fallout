use crate::nif_animation_inspection;
use clap::{Args, Subcommand};
use std::path::PathBuf;

// Keep the established NIF command names together when Clap flattens this family.
#[allow(clippy::enum_variant_names)]
#[derive(Subcommand)]
pub(crate) enum AssetsCommand {
    /// Compose an explicit supported set of parent/child channels; no blending.
    NifSourcePoseSet {
        input: PathBuf,
        #[arg(long)]
        request: PathBuf,
    },
    /// Prepare one exact source once and sample an explicit bounded time list.
    NifSourcePoseBatch {
        input: PathBuf,
        #[arg(long)]
        request: PathBuf,
    },
    /// Collect exact source-local visibility along one required ancestry path.
    NifVisibilityPath {
        input: PathBuf,
        #[arg(long)]
        request: PathBuf,
    },
    /// Bind exact external source keys to one skeleton node; playback unverified.
    NifClipPose {
        skeleton: PathBuf,
        clip: PathBuf,
        #[arg(long)]
        request: PathBuf,
        /// Prepare both exact sources once and sample an ordered source_times list.
        #[arg(long)]
        batch: bool,
    },
    /// Resolve exact source-local rigid attachment; clocks/equipment state unapplied.
    NifRigidAttachment {
        skeleton: PathBuf,
        attachment: PathBuf,
        #[arg(long)]
        request: PathBuf,
        /// Evaluate one exact controller/time request for the selected node.
        #[arg(long)]
        sampled: bool,
    },
    /// Evaluate linked translation/scale at explicit source time; playback unverified.
    NifSourcePose {
        input: PathBuf,
        /// Sample exact NiVisController local visibility; parent/clock semantics unapplied.
        #[arg(long)]
        local_visibility: bool,
        #[arg(long)]
        object: u32,
        #[arg(long)]
        controller: u32,
        #[arg(long, allow_hyphen_values = true)]
        source_time: f64,
    },
    /// Decode bounded authored animation framing and compare raw native fields.
    NifAnimation {
        input: PathBuf,
        #[arg(long)]
        oracle_report: Option<PathBuf>,
        #[arg(long)]
        include_keyframes: bool,
        #[arg(long)]
        include_splines: bool,
        #[arg(long)]
        include_spline_components: bool,
        #[arg(long)]
        include_bool_interpolators: bool,
        #[arg(long)]
        include_bool_keys: bool,
        #[arg(long, requires_all = ["sample_block", "sample_channel"], allow_hyphen_values = true)]
        sample_time: Option<f64>,
        #[arg(long, requires = "sample_time")]
        sample_block: Option<u32>,
        #[arg(long, value_enum, requires = "sample_time")]
        sample_channel: Option<nif_animation_inspection::SampleChannel>,
        /// Observe exact physical text keys over an explicit source-time interval.
        #[arg(long, conflicts_with_all = ["oracle_report", "include_keyframes", "include_splines", "include_spline_components", "include_bool_interpolators", "include_bool_keys", "sample_time", "sample_block", "sample_channel"])]
        markers_request: Option<PathBuf>,
    },
    /// Decode exact NV skin source fields and optionally compare an independent oracle.
    NifSkin(Box<NifSkinArgs>),
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
}

// The path-heavy skin modes should not enlarge every asset command value.
#[derive(Args)]
pub(crate) struct NifSkinArgs {
    pub(crate) input: PathBuf,
    #[arg(long)]
    pub(crate) oracle_report: Option<PathBuf>,
    #[arg(long)]
    pub(crate) include_partitions: bool,
    #[arg(long)]
    pub(crate) include_bindings: bool,
    /// Evaluate one exact geometry block using stored source locals.
    #[arg(long, requires = "pose_weight_tolerance", conflicts_with_all = ["oracle_report", "include_partitions", "include_bindings"])]
    pub(crate) pose_geometry: Option<u32>,
    /// Admit the raw weight sum within this absolute tolerance; never normalize.
    #[arg(long, requires = "pose_geometry", allow_hyphen_values = true)]
    pub(crate) pose_weight_tolerance: Option<f64>,
    /// Deform one skin with an exact same-container, explicit-time channel.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings"])]
    pub(crate) sampled_pose_request: Option<PathBuf>,
    /// Export every exact raw influence and reconstruct one source-local skin.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request"])]
    pub(crate) influences_request: Option<PathBuf>,
    /// Explicit external stored-local skeleton source; requires a complete map.
    #[arg(long, requires = "external_skin_request")]
    pub(crate) external_rig: Option<PathBuf>,
    #[arg(long, requires = "external_rig", conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request", "influences_request"])]
    pub(crate) external_skin_request: Option<PathBuf>,
    /// Evaluate unique explicit geometries from one source decode.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request", "influences_request", "external_rig", "external_skin_request"])]
    pub(crate) shared_skin_request: Option<PathBuf>,
    /// Export exact authored partition influence and topology streams.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request", "influences_request", "external_rig", "external_skin_request", "shared_skin_request"])]
    pub(crate) partition_streams_request: Option<PathBuf>,
    /// Select an authored partition from one existing geometry deformation.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request", "influences_request", "external_rig", "external_skin_request", "shared_skin_request", "partition_streams_request"])]
    pub(crate) partition_pose_request: Option<PathBuf>,
    /// Apply the complete explicitly sampled required skin forest.
    #[arg(long, conflicts_with_all = ["pose_geometry", "pose_weight_tolerance", "oracle_report", "include_partitions", "include_bindings", "sampled_pose_request", "influences_request", "external_rig", "external_skin_request", "shared_skin_request", "partition_streams_request", "partition_pose_request"])]
    pub(crate) pose_set_request: Option<PathBuf>,
}
