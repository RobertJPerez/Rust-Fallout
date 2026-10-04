//! Exact direct source links reach the existing named engineering cubic sampler.
//! Stored NiAV rotation stays explicit; clocks and retail playback are unapplied.
use super::{Ancestor, Budget, SceneMapping, SourceLocal, SourceSpan, span};
use crate::{
    Error, Result, nif_animation,
    nif_animation::spline::{self as source_spline, components, sampling},
    nif_scene,
    nif_skin::{pose::Affine, storage},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-linked-compact-spline-source-pose-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Contract {
    EngineeringOpenUniformCubicComponentsV1,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalPolicy {
    ReplaceTranslationScaleKeepStoredNiAvRotation,
}
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub expected_source_sha256: [u8; 32],
    pub object: u32,
    pub controller: u32,
    pub node_name_bytes: &'a [u8],
    /// Direct compact-source interval; controller clocks do not map this time.
    pub source_time: f64,
    pub contract: Contract,
    pub local_policy: LocalPolicy,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub components: components::Limits,
    pub scene: nif_scene::Limits,
    pub sampling: sampling::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub metadata_array_bytes: usize,
    pub metadata_work_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
    /// Live input, complete declared source/metadata allowances and own output.
    pub combined_retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        let pose = super::Limits::default();
        Self {
            components: components::Limits {
                splines: source_spline::Limits {
                    keyframes: pose.keys,
                    array_bytes: 32 * 1024 * 1024,
                    spline_work: 1_000_000,
                },
                array_bytes: 32 * 1024 * 1024,
                component_work: 1_000_000,
            },
            scene: pose.scene,
            sampling: sampling::Limits {
                validation_units: 1_000_000,
                sampling_units: 1_000_000,
            },
            decoder_array_admission_bytes: 128 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            metadata_array_bytes: 128 * 1024 * 1024,
            metadata_work_units: 16_000_000,
            array_bytes: 16 * 1024 * 1024,
            work_units: 256_000_000,
            ancestry_depth: 1024,
            combined_retained_bytes: 320 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub source_bytes: usize,
    pub whole_source_sha256_traversals: usize,
    pub component_decodes: usize,
    pub scene_decodes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub metadata_peak_bytes: usize,
    pub metadata_work_units: usize,
    pub source_catalogue_charged_bytes: usize,
    pub scene_index_and_payload_bytes: usize,
    pub charged_output_bytes: usize,
    pub combined_retained_bytes: usize,
    /// Conservative declared decoder/metadata contributions plus own work.
    pub charged_work_units: usize,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub source_sha256: String,
    pub object: SourceSpan,
    pub controller: SourceSpan,
    pub node_name_bytes: Vec<u8>,
    pub requested_time_f64_bits: u64,
    pub source_local: SourceLocal,
    pub object_flags: u32,
    pub local_policy: LocalPolicy,
    pub unapplied_controller_fields: nif_animation::Controller,
    /// Only the admitted compact-transform variant, with all raw fields intact.
    pub unapplied_interpolator_fields: source_spline::Data,
    pub translation: sampling::Diagnostic,
    pub scale: sampling::Diagnostic,
    pub translation_compact_controls: Vec<i16>,
    pub scale_compact_controls: Vec<i16>,
    pub static_ancestors: Vec<Ancestor>,
    pub local: Affine,
    pub source_world: Affine,
    pub sample_work: sampling::Usage,
    pub usage: Usage,
    pub runtime_ready: bool,
    pub retail_behavior_verified: bool,
}

fn checked_sum(values: &[usize], budget: &Budget<'_>, reason: &str) -> Result<usize> {
    values.iter().try_fold(0usize, |n, value| {
        n.checked_add(*value).ok_or_else(|| budget.fail(reason))
    })
}
fn copy_compact(data: &source_spline::Data) -> source_spline::Data {
    let source_spline::Data::CompactTransform {
        start_bits,
        stop_bits,
        spline_data,
        basis_data,
        translation_bits,
        rotation_wxyz_bits,
        scale_bits,
        translation_handle,
        rotation_handle,
        scale_handle,
        translation_offset_bits,
        translation_half_range_bits,
        rotation_offset_bits,
        rotation_half_range_bits,
        scale_offset_bits,
        scale_half_range_bits,
    } = data
    else {
        unreachable!("caller admitted the exact compact transform")
    };
    source_spline::Data::CompactTransform {
        start_bits: *start_bits,
        stop_bits: *stop_bits,
        spline_data: *spline_data,
        basis_data: *basis_data,
        translation_bits: *translation_bits,
        rotation_wxyz_bits: *rotation_wxyz_bits,
        scale_bits: *scale_bits,
        translation_handle: *translation_handle,
        rotation_handle: *rotation_handle,
        scale_handle: *scale_handle,
        translation_offset_bits: *translation_offset_bits,
        translation_half_range_bits: *translation_half_range_bits,
        rotation_offset_bits: *rotation_offset_bits,
        rotation_half_range_bits: *rotation_half_range_bits,
        scale_offset_bits: *scale_offset_bits,
        scale_half_range_bits: *scale_half_range_bits,
    }
}

pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request<'_>,
    limits: Limits,
) -> Result<Evaluation> {
    // Refuse an unbounded diagnostic label before formatting any contextual error.
    if source.len() > 4096 {
        return Err(Error::Unsupported(
            "compact source pose label byte budget exceeded".into(),
        ));
    }
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if !request.source_time.is_finite() {
        return Err(budget.fail("compact source time must be finite"));
    }
    if limits.ancestry_depth == 0 {
        return Err(budget.fail("compact pose ancestry depth budget exceeded"));
    }
    if bytes.len() > limits.components.splines.keyframes.animation.input_bytes
        || bytes.len() > limits.scene.input_bytes
    {
        return Err(budget.fail("compact pose source input byte budget exceeded"));
    }
    // Both policy enums are closed and required; neither is an inferred default.
    match (request.contract, request.local_policy) {
        (
            Contract::EngineeringOpenUniformCubicComponentsV1,
            LocalPolicy::ReplaceTranslationScaleKeepStoredNiAvRotation,
        ) => {}
    }
    let decoder_arrays = checked_sum(
        &[
            limits
                .components
                .splines
                .keyframes
                .max_combined_retained_bytes,
            limits.scene.array_bytes,
        ],
        &budget,
        "compact decoder array admission overflow",
    )?;
    if decoder_arrays > limits.decoder_array_admission_bytes {
        return Err(budget.fail("compact decoder array admission exceeded"));
    }
    let decoder_checks = checked_sum(
        &[
            limits
                .components
                .splines
                .keyframes
                .animation
                .reference_checks,
            limits.components.splines.keyframes.key_work,
            limits.components.splines.spline_work,
            limits.components.component_work,
            limits.sampling.validation_units,
            limits.sampling.sampling_units,
        ],
        &budget,
        "compact decoder check admission overflow",
    )?;
    if decoder_checks > limits.decoder_check_admission_units {
        return Err(budget.fail("compact decoder check admission exceeded"));
    }
    let declared_bytes = checked_sum(
        &[
            bytes.len(),
            decoder_arrays,
            limits.metadata_array_bytes,
            limits.array_bytes,
        ],
        &budget,
        "compact combined storage admission overflow",
    )?;
    if declared_bytes > limits.combined_retained_bytes {
        return Err(budget.fail("compact combined storage admission exceeded"));
    }
    // Decoder/sampler and metadata caps are conservatively charged before they
    // run. Own SHA, lookup, ancestry and copied controls add separate real work.
    budget.charge(decoder_checks)?;
    budget.charge(limits.metadata_work_units)?;
    budget.charge(bytes.len())?;
    budget.charge(32)?;
    let source_digest: [u8; 32] = Sha256::digest(bytes).into();
    if source_digest != request.expected_source_sha256 {
        return Err(budget.fail("compact pose source SHA256 differs"));
    }
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<u8>(11 * 64)?;
    budget.reserve::<u64>(4)?;
    budget.reserve::<u8>(request.node_name_bytes.len())?;
    budget.charge(request.node_name_bytes.len())?;
    let (component_index, decoded) =
        components::decode_with_limits(bytes, source, limits.components)?;
    let first_metadata = storage::measure(
        &component_index,
        None,
        source,
        limits.metadata_array_bytes,
        limits.metadata_work_units,
    )?;
    // The first internally decoded index is not retained alongside the Scene's
    // index. All component/source catalogues remain owned until sampling ends.
    drop(component_index);
    let (index, scene) = nif_scene::decode_with_limits(bytes, source, limits.scene)?;
    let second_metadata = storage::measure(
        &index,
        Some(&scene),
        source,
        limits.metadata_array_bytes,
        limits
            .metadata_work_units
            .checked_sub(first_metadata.work_units)
            .ok_or_else(|| budget.fail("compact metadata work budget exceeded"))?,
    )?;
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("compact pose unresolved source ancestry"));
    }
    budget.charge(checked_sum(
        &[
            scene.objects.len(),
            scene.world_transforms.len(),
            decoded.source.animation.blocks.len(),
            decoded.source.splines.blocks.len(),
            decoded.components.blocks.len(),
            index.blocks.len(),
            index.blocks.len(),
        ],
        &budget,
        "compact binding/lookup work overflow",
    )?)?;
    let object = scene
        .objects
        .iter()
        .find(|o| o.block == request.object)
        .ok_or_else(|| budget.fail("compact selected object is not decoded"))?;
    if !matches!(object.kind, nif_scene::ObjectKind::Node { .. }) {
        return Err(budget.fail("compact selected object is not a node"));
    }
    let name = object
        .name
        .ok_or_else(|| budget.fail("compact selected node has no authored name"))?;
    if index.strings[name as usize] != request.node_name_bytes {
        return Err(budget.fail("compact selected raw node name differs"));
    }
    if object.controller != Some(request.controller) {
        return Err(budget.fail("compact object.controller differs from requested controller"));
    }
    let controller = decoded
        .source
        .animation
        .blocks
        .iter()
        .find(|b| b.block == request.controller)
        .and_then(|b| match &b.data {
            nif_animation::Data::TransformController { controller } => Some(controller),
            _ => None,
        })
        .ok_or_else(|| budget.fail("compact controller is not decoded NiTransformController"))?;
    if controller.target != Some(request.object) {
        return Err(budget.fail("compact controller.target differs from selected object"));
    }
    if controller.next_controller.is_some() {
        return Err(budget.fail("compact controller chain is unapplied"));
    }
    let interpolator_id = controller
        .interpolator
        .ok_or_else(|| budget.fail("compact transform interpolator is missing"))?;
    let interpolator = decoded
        .source
        .splines
        .blocks
        .iter()
        .find(|b| b.block == interpolator_id)
        .ok_or_else(|| budget.fail("controller interpolator is not decoded compact transform"))?;
    let source_spline::Data::CompactTransform {
        rotation_handle, ..
    } = &interpolator.data
    else {
        return Err(budget.fail("controller interpolator has wrong compact source kind"));
    };
    if *rotation_handle != 65535 {
        return Err(budget.fail("compact active rotation handle is unapplied"));
    }
    let mut sampling_budget = sampling::Budget::new(limits.sampling);
    let translation_window = sampling::prepare(
        &decoded,
        interpolator_id,
        sampling::Channel::Translation,
        &mut sampling_budget,
    )?;
    let scale_window = sampling::prepare(
        &decoded,
        interpolator_id,
        sampling::Channel::Scale,
        &mut sampling_budget,
    )?;
    let copied_controls = checked_sum(
        &[
            translation_window.compact_controls().len(),
            scale_window.compact_controls().len(),
        ],
        &budget,
        "compact control copy cardinality overflow",
    )?;
    budget.reserve::<i16>(copied_controls)?;
    budget.charge(copied_controls)?;
    let translation_compact_controls = translation_window.compact_controls().to_vec();
    let scale_compact_controls = scale_window.compact_controls().to_vec();
    let translation = translation_window.observe(request.source_time, &mut sampling_budget)?;
    let scale = scale_window.observe(request.source_time, &mut sampling_budget)?;
    let t_words: [u64; 3] = translation
        .sample
        .evaluated_f64_bits
        .as_slice()
        .try_into()
        .map_err(|_| budget.fail("compact translation result cardinality differs"))?;
    let t = t_words.map(f64::from_bits);
    let scale_value = f64::from_bits(scale.sample.evaluated_f64_bits[0]);
    let local = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c == 3 {
                t[r]
            } else {
                f64::from(object.transform.rotation[r][c]) * scale_value
            }
        })
    });
    // Ancestor source spans are disjoint; one complete input allowance bounds
    // their hashes without an uncharged per-parent payload traversal.
    budget.charge(bytes.len())?;
    let mapping = SceneMapping::prepare(&scene, &index, request.object, &mut budget)?;
    let (world, ancestors) =
        mapping.compose(bytes, &index, local, &mut budget, limits.ancestry_depth)?;
    let span_work = checked_sum(
        &[
            index.blocks[request.object as usize].bytes,
            index.blocks[request.controller as usize].bytes,
        ],
        &budget,
        "compact selected span hashing overflow",
    )?;
    budget.charge(span_work)?;
    let catalogue_bytes = checked_sum(
        &[
            decoded.source.animation.retained_bytes,
            decoded.source.keys.retained_bytes,
            decoded.source.splines.retained_bytes,
            decoded.components.retained_bytes,
        ],
        &budget,
        "compact source catalogue retention overflow",
    )?;
    let output_bytes = limits.array_bytes - budget.bytes;
    let combined_bytes = checked_sum(
        &[
            bytes.len(),
            catalogue_bytes,
            second_metadata.retained_bytes,
            output_bytes,
        ],
        &budget,
        "compact live source/output retention overflow",
    )?;
    if combined_bytes > limits.combined_retained_bytes {
        return Err(budget.fail("compact live source/output retention exceeded"));
    }
    budget.charge(32)?;
    let mut source_sha256 = String::with_capacity(64);
    for byte in source_digest {
        source_sha256.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        source_sha256.push(char::from(b"0123456789abcdef"[(byte & 15) as usize]));
    }
    Ok(Evaluation {
        contract: CONTRACT,
        source_sha256,
        object: span(bytes, &index, request.object),
        controller: span(bytes, &index, request.controller),
        node_name_bytes: request.node_name_bytes.to_vec(),
        requested_time_f64_bits: request.source_time.to_bits(),
        source_local: object.transform.into(),
        object_flags: object.flags,
        local_policy: request.local_policy,
        unapplied_controller_fields: controller.clone(),
        unapplied_interpolator_fields: copy_compact(&interpolator.data),
        translation,
        scale,
        translation_compact_controls,
        scale_compact_controls,
        static_ancestors: ancestors,
        local,
        source_world: world,
        sample_work: sampling_budget.usage(),
        usage: Usage {
            source_bytes: bytes.len(),
            whole_source_sha256_traversals: 1,
            component_decodes: 1,
            scene_decodes: 1,
            decoder_array_admission_bytes: decoder_arrays,
            decoder_check_admission_units: decoder_checks,
            metadata_peak_bytes: first_metadata
                .retained_bytes
                .max(second_metadata.retained_bytes),
            metadata_work_units: first_metadata.work_units + second_metadata.work_units,
            source_catalogue_charged_bytes: catalogue_bytes,
            scene_index_and_payload_bytes: second_metadata.retained_bytes,
            charged_output_bytes: output_bytes,
            combined_retained_bytes: combined_bytes,
            charged_work_units: limits.work_units - budget.work,
        },
        runtime_ready: false,
        retail_behavior_verified: false,
    })
}
