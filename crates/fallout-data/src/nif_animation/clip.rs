//! Exact two-source, explicit-time engineering clip binding. Actor selection,
//! quaternion conventions, clocks, sequence scheduling and events stay separate.
use super::{
    ControlledBlock, Data, NoteLinks, TransformInterpolator, keyframe,
    pose::{self, Ancestor, Budget, SceneMapping, SourceLocal, SourceSpan, component_local, span},
    sampling,
};
use crate::{Result, nif_scene, nif_skin::pose::Affine};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-exact-external-clip-pose-v1";
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub expected_skeleton_sha256: [u8; 32],
    pub expected_clip_sha256: [u8; 32],
    pub object: u32,
    pub node_name_bytes: &'a [u8],
    pub sequence: u32,
    pub controlled_ordinal: usize,
    pub source_time: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub pose: pose::Limits,
    pub combined_input_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pose: Default::default(),
            combined_input_bytes: 128 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SequenceFields {
    pub name: Option<u32>,
    pub declared_controlled_blocks: u32,
    pub array_grow_by: u32,
    pub weight_bits: u32,
    pub text_keys: Option<u32>,
    pub cycle_type: u32,
    pub frequency_bits: u32,
    pub start_bits: u32,
    pub stop_bits: u32,
    pub manager: Option<u32>,
    pub accum_root_name: Option<u32>,
    pub notes: NoteLinks,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub skeleton_sha256: String,
    pub clip_sha256: String,
    pub object: SourceSpan,
    pub sequence: SourceSpan,
    pub controlled_ordinal: usize,
    pub controlled_packet: ControlledBlock,
    pub node_name_bytes: Vec<u8>,
    pub interpolator: SourceSpan,
    pub data: SourceSpan,
    pub requested_time_f64_bits: u64,
    pub source_local: SourceLocal,
    pub object_flags: u32,
    pub unapplied_object_controller: Option<u32>,
    pub unapplied_sequence_fields: SequenceFields,
    pub unapplied_interpolator_fields: TransformInterpolator,
    pub translation: sampling::Diagnostic,
    pub scale: sampling::Diagnostic,
    pub static_ancestors: Vec<Ancestor>,
    pub local: Affine,
    pub source_world: Affine,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}
/// Both sources are freshly decoded. Name equality and uniqueness validate an
/// explicit destination; they never select an actor, equipment or alternate node.
pub fn evaluate(
    skeleton_bytes: &[u8],
    clip_bytes: &[u8],
    source: &str,
    request: Request<'_>,
    limits: Limits,
) -> Result<Evaluation> {
    let mut budget = Budget {
        source,
        bytes: limits.pose.array_bytes,
        work: limits.pose.work_units,
    };
    if !request.source_time.is_finite() {
        return Err(budget.fail("requested source time must be finite"));
    }
    if limits.pose.ancestry_depth == 0 {
        return Err(budget.fail("ancestry depth budget exceeded"));
    }
    if skeleton_bytes.len() > limits.pose.scene.input_bytes
        || clip_bytes.len() > limits.pose.keys.animation.input_bytes
        || skeleton_bytes
            .len()
            .checked_add(clip_bytes.len())
            .is_none_or(|n| n > limits.combined_input_bytes)
    {
        return Err(budget.fail("combined clip/skeleton input byte budget exceeded"));
    }
    let skeleton_hash: [u8; 32] = Sha256::digest(skeleton_bytes).into();
    let clip_hash: [u8; 32] = Sha256::digest(clip_bytes).into();
    if skeleton_hash != request.expected_skeleton_sha256 {
        return Err(budget.fail("skeleton source SHA256 differs from request"));
    }
    if clip_hash != request.expected_clip_sha256 {
        return Err(budget.fail("clip source SHA256 differs from request"));
    }
    let (clip_index, decoded) = keyframe::decode_with_limits(clip_bytes, source, limits.pose.keys)?;
    let (skeleton_index, scene) =
        nif_scene::decode_with_limits(skeleton_bytes, source, limits.pose.scene)?;
    let view = Sources {
        skeleton_index: &skeleton_index,
        clip_index: &clip_index,
        decoded: &decoded,
        scene: &scene,
    };
    let bound = bind(view, request.into(), &mut budget)?;
    let mapping = SceneMapping::prepare(&scene, &skeleton_index, request.object, &mut budget)?;
    evaluate_bound(
        bound,
        request,
        limits.pose.into(),
        budget,
        Observation::Input {
            skeleton_bytes,
            clip_bytes,
            view,
            mapping: &mapping,
        },
    )
}

mod prepared;
pub use prepared::{
    BatchLimits, BindingRequest, ClipBatch, PreparationLimits, PreparationUsage, PreparedClipSource,
};

#[derive(Clone, Copy)]
struct Sources<'a> {
    skeleton_index: &'a crate::nif::NifIndex,
    clip_index: &'a crate::nif::NifIndex,
    decoded: &'a keyframe::Source,
    scene: &'a nif_scene::Scene,
}
struct Bound<'a> {
    object: &'a nif_scene::Object,
    sequence: &'a super::Sequence,
    packet: &'a ControlledBlock,
    interpolator: &'a TransformInterpolator,
    data: &'a keyframe::Block,
    interpolator_id: u32,
    data_id: u32,
}
fn bind<'a>(
    view: Sources<'a>,
    request: BindingRequest<'_>,
    budget: &mut Budget<'_>,
) -> Result<Bound<'a>> {
    let Sources {
        skeleton_index,
        clip_index,
        decoded,
        scene,
    } = view;
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.charge(
        scene.objects.len() * 3
            + scene.world_transforms.len()
            + decoded.animation.blocks.len() * 2
            + decoded.keys.blocks.len()
            + 1,
    )?;
    let object = scene
        .objects
        .iter()
        .find(|o| o.block == request.object)
        .ok_or_else(|| budget.fail("selected skeleton object is not decoded"))?;
    if !matches!(object.kind, nif_scene::ObjectKind::Node { .. }) {
        return Err(budget.fail("selected skeleton object is not a node"));
    }
    let name = object
        .name
        .ok_or_else(|| budget.fail("selected skeleton node has no authored name"))?;
    if skeleton_index.strings[name as usize] != request.node_name_bytes {
        return Err(budget.fail("selected skeleton raw node name differs"));
    }
    // Compare each stored string once, so repeated name IDs cannot multiply a
    // long byte comparison by the node count. Total string bytes are input-bound.
    budget.charge(skeleton_index.strings.len())?;
    budget.reserve::<bool>(skeleton_index.strings.len())?;
    let matching_names: Vec<bool> = skeleton_index
        .strings
        .iter()
        .map(|name| name == request.node_name_bytes)
        .collect();
    let count = scene
        .objects
        .iter()
        .filter(|o| {
            matches!(o.kind, nif_scene::ObjectKind::Node { .. })
                && o.name.is_some_and(|n| matching_names[n as usize])
        })
        .count();
    if count != 1 {
        return Err(budget.fail("selected skeleton raw node name is not unique"));
    }
    let sequence = decoded
        .animation
        .blocks
        .iter()
        .find(|b| b.block == request.sequence)
        .and_then(|b| match &b.data {
            Data::ControllerSequence { sequence } => Some(sequence),
            _ => None,
        })
        .ok_or_else(|| budget.fail("selected clip sequence is not decoded NiControllerSequence"))?;
    let packet = sequence
        .controlled_blocks
        .get(request.controlled_ordinal)
        .ok_or_else(|| budget.fail("controlled-block ordinal is out of range"))?;
    let packet_name = packet
        .node_name
        .ok_or_else(|| budget.fail("controlled packet has no raw node name"))?;
    if clip_index.strings[packet_name as usize] != request.node_name_bytes {
        return Err(budget.fail("controlled packet raw node name differs"));
    }
    if packet.controller.is_some()
        || packet.property_type.is_some()
        || packet.controller_id.is_some()
        || packet.interpolator_id.is_some()
    {
        return Err(
            budget.fail("controlled packet property/controller/identifier binding is unapplied")
        );
    }
    if !packet
        .controller_type
        .is_some_and(|id| clip_index.strings[id as usize] == b"NiTransformController")
    {
        return Err(budget.fail("controlled packet transform-controller type is unavailable"));
    }
    let interpolator_id = packet
        .interpolator
        .ok_or_else(|| budget.fail("controlled packet interpolator is absent"))?;
    let interpolator = decoded
        .animation
        .blocks
        .iter()
        .find(|b| b.block == interpolator_id)
        .and_then(|b| match &b.data {
            Data::TransformInterpolator { interpolator } => Some(interpolator),
            _ => None,
        })
        .ok_or_else(|| {
            budget.fail(&format!(
                "controlled interpolator {interpolator_id} is not decoded NiTransformInterpolator"
            ))
        })?;
    let data_id = interpolator.data.ok_or_else(|| {
        budget.fail(&format!(
            "controlled interpolator {interpolator_id} has no authored key data"
        ))
    })?;
    let data = decoded
        .keys
        .blocks
        .iter()
        .find(|b| b.block == data_id)
        .ok_or_else(|| {
            budget.fail(&format!(
                "controlled data {data_id} is not decoded NiTransformData"
            ))
        })?;
    if !matches!(data.data.rotation, keyframe::Rotation::Absent) {
        return Err(budget.fail(&format!("clip sequence {} controlled {} interpolator {interpolator_id} data {data_id}: rotation key mapping is unapplied",request.sequence,request.controlled_ordinal)));
    }
    Ok(Bound {
        object,
        sequence,
        packet,
        interpolator,
        data,
        interpolator_id,
        data_id,
    })
}

enum Observation<'a> {
    Input {
        skeleton_bytes: &'a [u8],
        clip_bytes: &'a [u8],
        view: Sources<'a>,
        mapping: &'a SceneMapping<'a>,
    },
    Prepared(&'a PreparedClipSource),
}
impl Observation<'_> {
    fn skeleton_sha256(&self) -> String {
        match self {
            Self::Input { skeleton_bytes, .. } => format!("{:x}", Sha256::digest(skeleton_bytes)),
            Self::Prepared(p) => p.skeleton_sha256.clone(),
        }
    }
    fn clip_sha256(&self) -> String {
        match self {
            Self::Input { clip_bytes, .. } => format!("{:x}", Sha256::digest(clip_bytes)),
            Self::Prepared(p) => p.clip_sha256.clone(),
        }
    }
    fn object_span(&self, id: u32) -> SourceSpan {
        match self {
            Self::Input {
                skeleton_bytes,
                view,
                ..
            } => span(skeleton_bytes, view.skeleton_index, id),
            Self::Prepared(p) => p.object_span.clone(),
        }
    }
    fn clip_span(&self, id: u32) -> SourceSpan {
        match self {
            Self::Input {
                clip_bytes, view, ..
            } => span(clip_bytes, view.clip_index, id),
            Self::Prepared(p) => p.clip_span(id),
        }
    }
    fn compose(
        &self,
        local: Affine,
        budget: &mut Budget<'_>,
        depth: usize,
    ) -> Result<(Affine, Vec<Ancestor>)> {
        match self {
            Self::Input {
                skeleton_bytes,
                view,
                mapping,
                ..
            } => mapping.compose(skeleton_bytes, view.skeleton_index, local, budget, depth),
            Self::Prepared(p) => p.compose(local, budget, depth),
        }
    }
}
fn evaluate_bound(
    bound: Bound<'_>,
    request: Request<'_>,
    limits: pose::SampleLimits,
    mut budget: Budget<'_>,
    observation: Observation<'_>,
) -> Result<Evaluation> {
    let Bound {
        object,
        sequence,
        packet,
        interpolator,
        data,
        interpolator_id,
        data_id,
    } = bound;
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<u8>(8 * 64)?;
    budget.reserve::<u8>(request.node_name_bytes.len())?;
    if let NoteLinks::Array { targets, .. } = &sequence.notes {
        budget.reserve::<Option<u32>>(targets.len())?;
        budget.charge(targets.len())?;
    }
    let mut sampling_budget = sampling::Budget::new(limits.sampling);
    let translation = sampling::evaluate(
        data,
        sampling::Channel::Translation,
        request.source_time,
        &mut sampling_budget,
    )?;
    let scale = sampling::evaluate(
        data,
        sampling::Channel::Scale,
        request.source_time,
        &mut sampling_budget,
    )?;
    let local = component_local(object.transform, &translation, &scale);
    let (world, ancestors) = observation.compose(local, &mut budget, limits.ancestry_depth)?;
    Ok(Evaluation {
        contract: CONTRACT,
        skeleton_sha256: observation.skeleton_sha256(),
        clip_sha256: observation.clip_sha256(),
        object: observation.object_span(request.object),
        sequence: observation.clip_span(request.sequence),
        controlled_ordinal: request.controlled_ordinal,
        controlled_packet: packet.clone(),
        node_name_bytes: request.node_name_bytes.to_vec(),
        interpolator: observation.clip_span(interpolator_id),
        data: observation.clip_span(data_id),
        requested_time_f64_bits: request.source_time.to_bits(),
        source_local: object.transform.into(),
        object_flags: object.flags,
        unapplied_object_controller: object.controller,
        unapplied_sequence_fields: SequenceFields {
            name: sequence.name,
            declared_controlled_blocks: sequence.declared_controlled_blocks,
            array_grow_by: sequence.array_grow_by,
            weight_bits: sequence.weight_bits,
            text_keys: sequence.text_keys,
            cycle_type: sequence.cycle_type,
            frequency_bits: sequence.frequency_bits,
            start_bits: sequence.start_bits,
            stop_bits: sequence.stop_bits,
            manager: sequence.manager,
            accum_root_name: sequence.accum_root_name,
            notes: sequence.notes.clone(),
        },
        unapplied_interpolator_fields: interpolator.clone(),
        translation,
        scale,
        static_ancestors: ancestors,
        local,
        source_world: world,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        sample_work: sampling_budget.usage(),
        retail_behavior_verified: false,
    })
}
