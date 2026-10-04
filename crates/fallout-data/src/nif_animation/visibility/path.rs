//! Exact source-local observations along one validated ancestry path.
//! No stored flags/held keys are combined into an effective render decision.
use super::{Budget, Evaluation, SourceView, boolean, evaluate_loaded, nif_scene, span};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-required-path-local-visibility-v1";
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub expected_source_sha256: [u8; 32],
    pub object: u32,
    pub channels: &'a [super::Request],
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub local: super::Limits,
    pub channels: usize,
    pub path_nodes: usize,
    pub ancestry_depth: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            local: Default::default(),
            channels: 256,
            path_nodes: 4096,
            ancestry_depth: 1024,
            decoder_array_admission_bytes: 128 * 1024 * 1024,
            decoder_check_admission_units: 96_000_000,
            array_bytes: 8 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub object: super::SourceSpan,
    pub parent: Option<u32>,
    /// Complete stored NiAV flag word, with no effective visibility interpretation.
    pub object_flags: u32,
    pub object_controller: Option<u32>,
    pub local: Option<Evaluation>,
}
#[derive(Debug, Serialize)]
pub struct Observation {
    pub contract: &'static str,
    pub source_sha256: String,
    pub selected: super::SourceSpan,
    pub nodes: Vec<Node>,
    pub boolean_source_decodes: usize,
    pub scene_decodes: usize,
    pub source_catalogue_retained_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
struct PathBudget<'a> {
    source: &'a str,
    bytes: usize,
    work: usize,
}
impl PathBudget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!("{}: path visibility: {detail}", self.source))
    }
    fn charge(&mut self, units: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(units)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| self.bytes.checked_sub(n))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
}
pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request<'_>,
    limits: Limits,
) -> Result<Observation> {
    let mut budget = PathBudget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if request.channels.len() > limits.channels {
        return Err(budget.fail("explicit channel count budget exceeded"));
    }
    if limits.ancestry_depth == 0 || limits.path_nodes == 0 {
        return Err(budget.fail("required path depth/node budget exceeded"));
    }
    let source_keys = limits.local.keys.booleans.components.splines.keyframes;
    if bytes.len() > limits.local.scene.input_bytes
        || bytes.len() > source_keys.animation.input_bytes
    {
        return Err(budget.fail("source input byte budget exceeded"));
    }
    // Admit source hashes and disjoint per-node span visits conservatively before
    // invoking the unchanged local evaluator, which retains its old counters.
    let byte_visits = request
        .channels
        .len()
        .checked_mul(5)
        .and_then(|n| n.checked_add(3))
        .and_then(|n| n.checked_mul(bytes.len()))
        .ok_or_else(|| budget.fail("source byte work product overflow"))?;
    budget.charge(byte_visits)?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if digest != request.expected_source_sha256 {
        return Err(budget.fail("source SHA256 differs"));
    }
    let arrays = source_keys
        .max_combined_retained_bytes
        .checked_add(limits.local.scene.array_bytes)
        .filter(|n| *n <= limits.decoder_array_admission_bytes)
        .ok_or_else(|| budget.fail("decoder array admission exceeded"))?;
    let checks = [
        source_keys.animation.reference_checks,
        source_keys.key_work,
        limits.local.keys.booleans.components.splines.spline_work,
        limits.local.keys.booleans.components.component_work,
        limits.local.keys.booleans.boolean_work,
        limits.local.keys.key_work,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("decoder check admission exceeded"))?;
    budget.reserve::<Observation>(1)?;
    budget.reserve::<u8>(2 * 64)?;
    let (index, decoded) =
        boolean::keyframes::decode_with_limits(bytes, source, limits.local.keys)?;
    let (_, scene) = nif_scene::decode_with_limits(bytes, source, limits.local.scene)?;
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.reserve::<Option<usize>>(
        index
            .blocks
            .len()
            .checked_mul(3)
            .ok_or_else(|| budget.fail("source map product overflow"))?,
    )?;
    budget.reserve::<usize>(scene.objects.len())?;
    budget.charge(scene.objects.len())?;
    budget.charge(scene.world_transforms.len())?;
    budget.charge(request.channels.len())?;
    let mut objects = vec![None; index.blocks.len()];
    let mut worlds = vec![None; index.blocks.len()];
    let mut channels = vec![None; index.blocks.len()];
    for (ordinal, object) in scene.objects.iter().enumerate() {
        objects[object.block as usize] = Some(ordinal);
    }
    for (ordinal, world) in scene.world_transforms.iter().enumerate() {
        worlds[world.block as usize] = Some(ordinal);
    }
    for (ordinal, channel) in request.channels.iter().enumerate() {
        if !channel.source_time.is_finite() {
            return Err(budget.fail("requested source time must be finite"));
        }
        if !objects
            .get(channel.object as usize)
            .is_some_and(Option::is_some)
        {
            return Err(budget.fail("explicit channel object is not decoded"));
        }
        if channels[channel.object as usize].replace(ordinal).is_some() {
            return Err(budget.fail("duplicate explicit channel object"));
        }
    }
    let mut current = Some(request.object);
    let mut path = Vec::with_capacity(scene.objects.len());
    while let Some(id) = current {
        budget.charge(1)?;
        if path.len() >= limits.ancestry_depth || path.len() >= limits.path_nodes {
            return Err(budget.fail("required path depth/node budget exceeded"));
        }
        let world = worlds
            .get(id as usize)
            .and_then(|v| *v)
            .map(|ordinal| &scene.world_transforms[ordinal])
            .ok_or_else(|| budget.fail("selected object ancestry is unavailable"))?;
        if !world.reachable_from_footer {
            return Err(budget.fail("selected object is not reachable from footer"));
        }
        path.push(id);
        current = world.parent;
    }
    budget.charge(path.len())?;
    path.reverse();
    let mut matched = 0;
    // Validate the complete exact request set before any channel output exists.
    budget.charge(path.len())?;
    for &id in &path {
        let object = &scene.objects
            [objects[id as usize].ok_or_else(|| budget.fail("required object unavailable"))?];
        match (object.controller, channels[id as usize]) {
            (Some(controller), Some(ordinal)) => {
                if index.block_types[index.blocks[controller as usize].type_index as usize]
                    != "NiVisController"
                {
                    return Err(
                        budget.fail("required object controller is not supported NiVisController")
                    );
                }
                if request.channels[ordinal].controller != controller {
                    return Err(budget.fail("object.controller differs from requested controller"));
                }
                matched += 1;
            }
            (Some(_), None) => {
                return Err(budget.fail("required object controller has no explicit channel"));
            }
            (None, Some(_)) => {
                return Err(
                    budget.fail("explicit channel targets a required object without a controller")
                );
            }
            (None, None) => {}
        }
    }
    if matched != request.channels.len() {
        return Err(budget.fail("explicit channel is outside required ancestry path"));
    }
    budget.reserve::<Node>(path.len())?;
    budget.reserve::<u8>(
        path.len()
            .checked_mul(64)
            .ok_or_else(|| budget.fail("path span product overflow"))?,
    )?;
    budget.charge(path.len())?;
    let mut nodes = Vec::with_capacity(path.len());
    for id in path {
        let object = &scene.objects
            [objects[id as usize].ok_or_else(|| budget.fail("required object unavailable"))?];
        let world = &scene.world_transforms
            [worlds[id as usize].ok_or_else(|| budget.fail("required ancestry unavailable"))?];
        let local = if let Some(ordinal) = channels[id as usize] {
            let local_limits = super::Limits {
                array_bytes: limits.local.array_bytes.min(budget.bytes),
                work_units: limits.local.work_units.min(budget.work),
                ..limits.local
            };
            let result = evaluate_loaded(
                SourceView {
                    bytes,
                    index: &index,
                    decoded: &decoded,
                    scene: &scene,
                },
                request.channels[ordinal],
                local_limits,
                Budget {
                    source,
                    work: local_limits.work_units,
                },
            )?;
            budget.reserve::<u8>(result.retained_bytes)?;
            budget.charge(result.work_units)?;
            Some(result)
        } else {
            None
        };
        nodes.push(Node {
            object: span(bytes, &index, id),
            parent: world.parent,
            object_flags: object.flags,
            object_controller: object.controller,
            local,
        });
    }
    let prior = &decoded.source.source.source;
    let retained = [
        prior.animation.retained_bytes,
        prior.keys.retained_bytes,
        prior.splines.retained_bytes,
        decoded.source.source.components.retained_bytes,
        decoded.source.booleans.retained_bytes,
        decoded.keys.retained_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .ok_or_else(|| budget.fail("source catalogue retention overflow"))?;
    // The whole digest was already checked. Format it without a second source hash.
    let mut source_sha256 = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        source_sha256.push(HEX[(byte >> 4) as usize] as char);
        source_sha256.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(Observation {
        contract: CONTRACT,
        source_sha256,
        selected: span(bytes, &index, request.object),
        nodes,
        boolean_source_decodes: 1,
        scene_decodes: 1,
        source_catalogue_retained_bytes: retained,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
