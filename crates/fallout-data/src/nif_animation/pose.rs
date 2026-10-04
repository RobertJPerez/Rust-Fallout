//! Explicit source-time translation/scale pose with exact same-container links.
//! Raw clocks/quaternions remain observations, without playback/event semantics.
use super::{Controller, Data, TransformInterpolator, keyframe, sampling};
use crate::{
    Error, Result, nif, nif_scene,
    nif_skin::pose::{Affine, compose, scene_affine},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-linked-source-pose-v1";

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub object: u32,
    pub controller: u32,
    /// Direct source-key domain. Frequency, phase, repeat/cycle flags are unapplied.
    pub source_time: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub keys: keyframe::Limits,
    pub scene: nif_scene::Limits,
    pub sampling: sampling::Limits,
    /// Additional element/string storage for maps and returned observations.
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        let animation = super::Limits {
            blocks: 16_384,
            array_bytes: 32 * 1024 * 1024,
            ..Default::default()
        };
        Self {
            keys: keyframe::Limits {
                animation,
                array_bytes: 32 * 1024 * 1024,
                max_combined_retained_bytes: 64 * 1024 * 1024,
                key_work: 1_000_000,
            },
            scene: nif_scene::Limits {
                blocks: 16_384,
                array_bytes: 32 * 1024 * 1024,
                ..Default::default()
            },
            sampling: sampling::Limits {
                validation_work: 1_000_000,
                sampling_work: 1_000_000,
            },
            array_bytes: 4 * 1024 * 1024,
            work_units: 1_000_000,
            ancestry_depth: 1024,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SourceSpan {
    pub block: u32,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct SourceLocal {
    pub translation_bits: [u32; 3],
    pub rotation_bits: [[u32; 3]; 3],
    pub scale_bits: u32,
}

impl From<nif_scene::Transform> for SourceLocal {
    fn from(value: nif_scene::Transform) -> Self {
        Self {
            translation_bits: value.translation.map(f32::to_bits),
            rotation_bits: value.rotation.map(|row| row.map(f32::to_bits)),
            scale_bits: value.scale.to_bits(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Ancestor {
    pub source: SourceSpan,
    pub local: SourceLocal,
    pub flags: u32,
    pub parent: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ObjectPose {
    pub contract: &'static str,
    pub source_sha256: String,
    pub object: SourceSpan,
    pub controller: SourceSpan,
    pub interpolator: SourceSpan,
    pub data: SourceSpan,
    pub requested_time_f64_bits: u64,
    pub source_local: SourceLocal,
    pub object_flags: u32,
    /// Source time is supplied directly; all flags/clock fields remain unapplied.
    pub unapplied_controller_fields: Controller,
    /// Includes exact WXYZ words; absent key groups retain NiAV locals instead.
    pub unapplied_interpolator_fields: TransformInterpolator,
    pub translation: sampling::Diagnostic,
    pub scale: sampling::Diagnostic,
    /// Nearest parent first, up to the decoded reachable footer root.
    pub static_ancestors: Vec<Ancestor>,
    pub local: Affine,
    pub source_world: Affine,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}

pub(super) struct Budget<'a> {
    pub(super) source: &'a str,
    pub(super) bytes: usize,
    pub(super) work: usize,
}
impl Budget<'_> {
    pub(super) fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!("{}: linked source pose: {detail}", self.source))
    }
    pub(super) fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|size| self.bytes.checked_sub(size))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
    pub(super) fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}

pub(super) fn span(bytes: &[u8], index: &nif::NifIndex, block: u32) -> SourceSpan {
    let selected = &index.blocks[block as usize];
    SourceSpan {
        block,
        offset: selected.offset,
        bytes: selected.bytes,
        sha256: format!(
            "{:x}",
            Sha256::digest(&bytes[selected.offset..selected.offset + selected.bytes])
        ),
    }
}

pub(super) struct SceneMapping<'a> {
    scene: &'a nif_scene::Scene,
    objects: Vec<Option<usize>>,
    worlds: Vec<Option<usize>>,
    selected_world: nif_scene::WorldTransform,
}
impl<'a> SceneMapping<'a> {
    pub(super) fn prepare(
        scene: &'a nif_scene::Scene,
        index: &nif::NifIndex,
        object: u32,
        budget: &mut Budget<'_>,
    ) -> Result<Self> {
        budget.reserve::<Option<usize>>(index.blocks.len() * 2)?;
        let mut objects = vec![None; index.blocks.len()];
        let mut worlds = vec![None; index.blocks.len()];
        for (i, value) in scene.objects.iter().enumerate() {
            objects[value.block as usize] = Some(i);
        }
        for (i, value) in scene.world_transforms.iter().enumerate() {
            worlds[value.block as usize] = Some(i);
        }
        let selected_world = worlds
            .get(object as usize)
            .copied()
            .flatten()
            .map(|i| &scene.world_transforms[i])
            .ok_or_else(|| budget.fail("missing selected object ancestry"))?;
        if !selected_world.reachable_from_footer {
            return Err(budget.fail("selected object is not reachable from footer"));
        }
        Ok(Self {
            scene,
            objects,
            worlds,
            selected_world: *selected_world,
        })
    }
    pub(super) fn compose(
        &self,
        bytes: &[u8],
        index: &nif::NifIndex,
        local: Affine,
        budget: &mut Budget<'_>,
        ancestry_depth: usize,
    ) -> Result<(Affine, Vec<Ancestor>)> {
        let mut world = local;
        let mut parent = self.selected_world.parent;
        let mut ancestors = Vec::new();
        let mut depth = 1;
        while let Some(id) = parent {
            budget.charge(1)?;
            depth += 1;
            if depth > ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            let object = self
                .objects
                .get(id as usize)
                .copied()
                .flatten()
                .map(|i| &self.scene.objects[i])
                .ok_or_else(|| budget.fail("unresolved ancestor object"))?;
            let ancestor_world = self
                .worlds
                .get(id as usize)
                .copied()
                .flatten()
                .map(|i| &self.scene.world_transforms[i])
                .ok_or_else(|| budget.fail("unresolved ancestor world link"))?;
            if object.controller.is_some() {
                return Err(budget.fail(&format!("ancestor {id} controller is unapplied")));
            }
            budget.reserve::<Ancestor>(1)?;
            budget.reserve::<u8>(64)?;
            world = compose(scene_affine(object.transform), world);
            ancestors.push(Ancestor {
                source: span(bytes, index, id),
                local: object.transform.into(),
                flags: object.flags,
                parent: ancestor_world.parent,
            });
            parent = ancestor_world.parent;
        }
        if !local.iter().chain(&world).flatten().all(|v| v.is_finite()) {
            return Err(budget.fail("evaluated matrix overflow"));
        }
        Ok((world, ancestors))
    }
}
pub(super) fn component_local(
    transform: nif_scene::Transform,
    translation: &sampling::Diagnostic,
    scale: &sampling::Diagnostic,
) -> Affine {
    let selected_translation = match &translation.evaluation {
        sampling::Evaluated::Translation {
            sample: Some(value),
        } => value.evaluated_f64_bits.map(f64::from_bits),
        _ => transform.translation.map(f64::from),
    };
    let selected_scale = match &scale.evaluation {
        sampling::Evaluated::Scale {
            sample: Some(value),
        } => f64::from_bits(value.evaluated_f64_bits[0]),
        _ => f64::from(transform.scale),
    };
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c == 3 {
                selected_translation[r]
            } else {
                f64::from(transform.rotation[r][c]) * selected_scale
            }
        })
    })
}

/// Existing decoders build their own immutable index from these bytes. No public
/// predecoded catalogue or name-based link can substitute stale source identity.
pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<ObjectPose> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if !request.source_time.is_finite() {
        return Err(budget.fail("requested source time must be finite"));
    }
    if limits.ancestry_depth == 0 {
        return Err(budget.fail("ancestry depth budget exceeded"));
    }
    let (index, decoded) = keyframe::decode_with_limits(bytes, source, limits.keys)?;
    let (_, scene) = nif_scene::decode_with_limits(bytes, source, limits.scene)?;
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.charge(
        scene.objects.len() * 2
            + scene.world_transforms.len()
            + decoded.animation.blocks.len() * 2
            + decoded.keys.blocks.len(),
    )?;
    let object = scene
        .objects
        .iter()
        .find(|o| o.block == request.object)
        .ok_or_else(|| budget.fail("selected object is not decoded"))?;
    if object.controller != Some(request.controller) {
        return Err(budget.fail("selected object.controller differs from requested controller"));
    }
    let controller = decoded
        .animation
        .blocks
        .iter()
        .find(|b| b.block == request.controller)
        .and_then(|b| match &b.data {
            Data::TransformController { controller } => Some(controller),
            _ => None,
        })
        .ok_or_else(|| budget.fail("selected controller is not a decoded NiTransformController"))?;
    if controller.target != Some(request.object) {
        return Err(budget.fail("selected controller.target differs from requested object"));
    }
    if controller.next_controller.is_some() {
        return Err(budget.fail("controller chain is unapplied"));
    }
    let interpolator_id = controller
        .interpolator
        .ok_or_else(|| budget.fail("missing transform interpolator"))?;
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
            budget.fail("controller interpolator is not a decoded NiTransformInterpolator")
        })?;
    let data_id = interpolator
        .data
        .ok_or_else(|| budget.fail("missing transform key data"))?;
    let data = decoded
        .keys
        .blocks
        .iter()
        .find(|b| b.block == data_id)
        .ok_or_else(|| budget.fail("interpolator data is not decoded NiTransformData"))?;
    if !matches!(data.data.rotation, keyframe::Rotation::Absent) {
        return Err(budget.fail("rotation key mapping is unapplied"));
    }
    let mapping = SceneMapping::prepare(&scene, &index, request.object, &mut budget)?;
    budget.reserve::<ObjectPose>(1)?;
    // Whole source, four spans and two borrowed-source sampling receipts.
    budget.reserve::<u8>(7 * 64)?;
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
    let (world, ancestors) =
        mapping.compose(bytes, &index, local, &mut budget, limits.ancestry_depth)?;
    Ok(ObjectPose {
        contract: CONTRACT,
        source_sha256: format!("{:x}", Sha256::digest(bytes)),
        object: span(bytes, &index, request.object),
        controller: span(bytes, &index, request.controller),
        interpolator: span(bytes, &index, interpolator_id),
        data: span(bytes, &index, data_id),
        requested_time_f64_bits: request.source_time.to_bits(),
        source_local: object.transform.into(),
        object_flags: object.flags,
        unapplied_controller_fields: controller.clone(),
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
