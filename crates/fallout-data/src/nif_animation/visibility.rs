//! Local visibility at explicit source-key time. No clock, parent visibility,
//! manager, timeline crossing or retail event evaluation is admitted.
use super::{
    Controller, boolean,
    pose::{SourceSpan, span},
    read,
};
use crate::{
    Error, Result,
    nif_scene::{self, cursor::Reader},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub mod path;

pub const CONTRACT: &str = "engineering-linked-local-visibility-v1";
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub object: u32,
    pub controller: u32,
    pub source_time: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub keys: boolean::keyframes::Limits,
    pub scene: nif_scene::Limits,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        let mut keys = boolean::keyframes::Limits::default();
        keys.booleans.components.splines.keyframes.animation.blocks = 16_384;
        keys.booleans
            .components
            .splines
            .keyframes
            .animation
            .array_bytes = 32 * 1024 * 1024;
        keys.booleans
            .components
            .splines
            .keyframes
            .max_combined_retained_bytes = 64 * 1024 * 1024;
        keys.array_bytes = 16 * 1024 * 1024;
        keys.key_work = 1_000_000;
        Self {
            keys,
            scene: nif_scene::Limits {
                blocks: 16_384,
                array_bytes: 32 * 1024 * 1024,
                ..Default::default()
            },
            array_bytes: 1024 * 1024,
            work_units: 1_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selection {
    AuthoredPose,
    HeldKey { index: usize, time_bits: u32 },
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub source_sha256: String,
    pub object: SourceSpan,
    pub controller: SourceSpan,
    pub interpolator: SourceSpan,
    pub data: Option<SourceSpan>,
    pub requested_time_f64_bits: u64,
    pub selection: Selection,
    pub raw_value: u8,
    pub local_visible: bool,
    pub object_flags: u32,
    pub raw_interpolator_value: u8,
    pub unapplied_controller_fields: Controller,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
struct Budget<'a> {
    source: &'a str,
    work: usize,
}
impl Budget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!("{}: local visibility: {detail}", self.source))
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}
/// Direct source-domain sampling with no sorting, raw-byte conversion, guessed
/// missing values or source-controller fallback. Effective parent visibility is
/// a separate consumer state; this result deliberately describes this object.
pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<Evaluation> {
    let budget = Budget {
        source,
        work: limits.work_units,
    };
    if !request.source_time.is_finite() {
        return Err(budget.fail("requested source time must be finite"));
    }
    let (index, decoded) = boolean::keyframes::decode_with_limits(bytes, source, limits.keys)?;
    let (_, scene) = nif_scene::decode_with_limits(bytes, source, limits.scene)?;
    evaluate_loaded(
        SourceView {
            bytes,
            index: &index,
            decoded: &decoded,
            scene: &scene,
        },
        request,
        limits,
        budget,
    )
}

struct SourceView<'a> {
    bytes: &'a [u8],
    index: &'a crate::nif::NifIndex,
    decoded: &'a boolean::keyframes::Source,
    scene: &'a nif_scene::Scene,
}
fn evaluate_loaded(
    view: SourceView<'_>,
    request: Request,
    limits: Limits,
    mut budget: Budget<'_>,
) -> Result<Evaluation> {
    let SourceView {
        bytes,
        index,
        decoded,
        scene,
    } = view;
    let source = budget.source;
    if !request.source_time.is_finite() {
        return Err(budget.fail("requested source time must be finite"));
    }
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.charge(
        scene.objects.len()
            + scene.world_transforms.len()
            + decoded.source.booleans.blocks.len()
            + decoded.keys.blocks.len(),
    )?;
    let object = scene
        .objects
        .iter()
        .find(|o| o.block == request.object)
        .ok_or_else(|| budget.fail("selected object is not decoded"))?;
    if object.controller != Some(request.controller) {
        return Err(budget.fail("object.controller differs from requested controller"));
    }
    if !scene
        .world_transforms
        .iter()
        .any(|w| w.block == request.object && w.reachable_from_footer)
    {
        return Err(budget.fail("selected object is not reachable from footer"));
    }
    let selected = index
        .blocks
        .get(request.controller as usize)
        .ok_or_else(|| budget.fail("controller block is out of range"))?;
    if index.block_types[selected.type_index as usize] != "NiVisController" {
        return Err(budget.fail("selected controller is not NiVisController"));
    }
    let retained = std::mem::size_of::<Evaluation>() + 5 * 64;
    let mut remaining = limits
        .array_bytes
        .checked_sub(retained)
        .ok_or_else(|| budget.fail("array storage budget exceeded"))?;
    // The existing single-interpolator reader charges exact primitive/link work.
    let reader = Reader {
        data: &bytes[selected.offset..selected.offset + selected.bytes],
        base: selected.offset,
        position: 0,
        source,
        index,
        array_bytes_left: &mut remaining,
    };
    let controller = read::single_controller(reader, &mut budget.work)?;
    if controller.target != Some(request.object) {
        return Err(budget.fail("controller.target differs from requested object"));
    }
    if controller.next_controller.is_some() {
        return Err(budget.fail("controller chain is unapplied"));
    }
    // Only the pinned 8-bit flag set/defined cycle values are admitted. Manager
    // state is unavailable. Recognized clocks/other flags are still unapplied.
    if controller.flags & !0x00FF != 0
        || (controller.flags >> 1) & 3 == 3
        || controller.flags & 0x0020 != 0
    {
        return Err(budget.fail("controller flags require unavailable semantics"));
    }
    let interpolator_id = controller
        .interpolator
        .ok_or_else(|| budget.fail("missing Boolean interpolator"))?;
    let interpolator = decoded
        .source
        .booleans
        .blocks
        .iter()
        .find(|b| b.block == interpolator_id)
        .ok_or_else(|| budget.fail("Boolean interpolator is not decoded"))?;
    if interpolator.block_type != "NiBoolInterpolator" {
        return Err(budget.fail("timeline key crossing is unapplied"));
    }
    let (data, selection, raw_value) = if let Some(data_id) = interpolator.data.data {
        let block = decoded
            .keys
            .blocks
            .iter()
            .find(|b| b.block == data_id)
            .ok_or_else(|| budget.fail("NiBoolData is not decoded"))?;
        let keys = &block.data.keys;
        if keys.is_empty() || block.data.key_type != Some(5) {
            return Err(budget.fail("authored constant key group unavailable"));
        }
        budget.charge(keys.len() * 2 + 2)?;
        let mut previous = None;
        for key in keys {
            let time = f32::from_bits(key.time_bits);
            if !time.is_finite() || previous.is_some_and(|old| time <= old) {
                return Err(budget.fail("key times are not finite strictly increasing"));
            }
            if key.raw_value > 1 {
                return Err(budget.fail("authored key Boolean value unavailable"));
            }
            previous = Some(time);
        }
        let key_time = |i: usize| f64::from(f32::from_bits(keys[i].time_bits));
        let last = keys.len() - 1;
        if request.source_time < key_time(0) || request.source_time > key_time(last) {
            return Err(budget.fail("requested time would extrapolate outside source keys"));
        }
        let (mut left, mut right) = (0, keys.len());
        while left < right {
            budget.charge(1)?;
            let middle = left + (right - left) / 2;
            if key_time(middle) <= request.source_time {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        let selected = left - 1;
        (
            Some(span(bytes, index, data_id)),
            Selection::HeldKey {
                index: selected,
                time_bits: keys[selected].time_bits,
            },
            keys[selected].raw_value,
        )
    } else {
        (None, Selection::AuthoredPose, interpolator.data.raw_value)
    };
    if raw_value > 1 {
        return Err(budget.fail("authored pose Boolean value unavailable"));
    }
    Ok(Evaluation {
        contract: CONTRACT,
        source_sha256: format!("{:x}", Sha256::digest(bytes)),
        object: span(bytes, index, request.object),
        controller: span(bytes, index, request.controller),
        interpolator: span(bytes, index, interpolator_id),
        data,
        requested_time_f64_bits: request.source_time.to_bits(),
        selection,
        raw_value,
        local_visible: raw_value == 1,
        object_flags: object.flags,
        raw_interpolator_value: interpolator.data.raw_value,
        unapplied_controller_fields: controller,
        retained_bytes: retained,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
