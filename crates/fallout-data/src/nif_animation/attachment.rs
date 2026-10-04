//! Identity-bound rigid attachment of stored source locals; no inferred socket,
//! bind pose, controller evaluation, world placement or actor equipment state.
use super::pose::{SourceLocal, SourceSpan, span};
use crate::{
    Error, Result, nif, nif_scene,
    nif_skin::pose::{Affine, compose, scene_affine},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod sampled;
pub use sampled::{SampledEvaluation, SampledLimits, SampledRequest, evaluate_sampled};

pub const CONTRACT: &str = "engineering-source-local-rigid-attachment-v1";
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicy {
    StoredNiAvLocals,
}
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub expected_skeleton_sha256: [u8; 32],
    pub expected_attachment_sha256: [u8; 32],
    pub node: u32,
    pub node_name_bytes: &'a [u8],
    pub attachment_root: u32,
    /// Attachment source-parent axes to selected skeleton-node axes. The
    /// attachment's authored root local is already in its source-world meshes.
    pub attachment_parent_to_node: Affine,
    pub source_policy: SourcePolicy,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub scene: nif_scene::Limits,
    pub combined_input_bytes: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            scene: nif_scene::Limits {
                blocks: 16_384,
                array_bytes: 32 * 1024 * 1024,
                ..Default::default()
            },
            combined_input_bytes: 128 * 1024 * 1024,
            array_bytes: 4 * 1024 * 1024,
            work_units: 1_000_000,
            ancestry_depth: 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct NodeSource {
    pub source: SourceSpan,
    pub local: SourceLocal,
    pub flags: u32,
    pub unapplied_controller: Option<u32>,
    pub parent: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub skeleton_sha256: String,
    pub attachment_sha256: String,
    pub node_name_bytes: Vec<u8>,
    /// Selected node first; the reachable skeleton footer root is last.
    pub skeleton_path: Vec<NodeSource>,
    pub attachment_root: NodeSource,
    pub source_policy: SourcePolicy,
    pub attachment_parent_to_node: Affine,
    /// Apply once to the attachment's already source-world mesh transforms.
    pub attachment_source_to_skeleton_source: Affine,
    /// Apply to coordinates local to the attachment root, not source-world meshes.
    pub root_to_skeleton_source: Affine,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
struct Budget<'a> {
    source: &'a str,
    bytes: usize,
    work: usize,
}
impl Budget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!(
            "{}: rigid source attachment: {detail}",
            self.source
        ))
    }
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| self.bytes.checked_sub(n))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}
fn source_node(
    bytes: &[u8],
    index: &nif::NifIndex,
    object: &nif_scene::Object,
    parent: Option<u32>,
    budget: &mut Budget<'_>,
) -> Result<NodeSource> {
    budget.reserve::<NodeSource>(1)?;
    budget.reserve::<u8>(64)?;
    Ok(NodeSource {
        source: span(bytes, index, object.block),
        local: object.transform.into(),
        flags: object.flags,
        unapplied_controller: object.controller,
        parent,
    })
}
fn finite(matrix: Affine, budget: &Budget<'_>) -> Result<Affine> {
    if matrix.iter().flatten().all(|v| v.is_finite()) {
        Ok(matrix)
    } else {
        Err(budget.fail("nonfinite or overflowing attachment matrix"))
    }
}
/// Exact caller identity/name/placement, composed in original source axes/units.
/// Stored NiAV locals may be animated; raw controller IDs remain unapplied.
pub fn evaluate(
    skeleton_bytes: &[u8],
    attachment_bytes: &[u8],
    source: &str,
    request: Request<'_>,
    limits: Limits,
) -> Result<Evaluation> {
    let budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if skeleton_bytes.len() > limits.scene.input_bytes
        || attachment_bytes.len() > limits.scene.input_bytes
        || skeleton_bytes
            .len()
            .checked_add(attachment_bytes.len())
            .is_none_or(|n| n > limits.combined_input_bytes)
    {
        return Err(budget.fail("input byte budget exceeded"));
    }
    finite(request.attachment_parent_to_node, &budget)?;
    let skeleton_hash: [u8; 32] = Sha256::digest(skeleton_bytes).into();
    let attachment_hash: [u8; 32] = Sha256::digest(attachment_bytes).into();
    if skeleton_hash != request.expected_skeleton_sha256 {
        return Err(budget.fail("skeleton source SHA256 differs from request"));
    }
    if attachment_hash != request.expected_attachment_sha256 {
        return Err(budget.fail("attachment source SHA256 differs from request"));
    }
    let (skeleton_index, skeleton) =
        nif_scene::decode_with_limits(skeleton_bytes, source, limits.scene)?;
    let (attachment_index, attachment) =
        nif_scene::decode_with_limits(attachment_bytes, source, limits.scene)?;
    evaluate_loaded(
        skeleton_bytes,
        attachment_bytes,
        request,
        limits,
        Loaded {
            skeleton_index: &skeleton_index,
            skeleton: &skeleton,
            attachment_index: &attachment_index,
            attachment: &attachment,
        },
        budget,
    )
}

struct Loaded<'a> {
    skeleton_index: &'a nif::NifIndex,
    skeleton: &'a nif_scene::Scene,
    attachment_index: &'a nif::NifIndex,
    attachment: &'a nif_scene::Scene,
}
fn evaluate_loaded(
    skeleton_bytes: &[u8],
    attachment_bytes: &[u8],
    request: Request<'_>,
    limits: Limits,
    loaded: Loaded<'_>,
    mut budget: Budget<'_>,
) -> Result<Evaluation> {
    let Loaded {
        skeleton_index,
        skeleton,
        attachment_index,
        attachment,
    } = loaded;
    if !skeleton.unsupported_scene_edges.is_empty()
        || !attachment.unsupported_scene_edges.is_empty()
    {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<u8>(128)?;
    budget.charge(
        skeleton.objects.len()
            + skeleton.world_transforms.len()
            + attachment.objects.len()
            + attachment.world_transforms.len(),
    )?;
    budget.reserve::<Option<usize>>(skeleton_index.blocks.len())?;
    budget.reserve::<Option<usize>>(skeleton_index.blocks.len())?;
    budget.charge(skeleton_index.blocks.len() * 2)?;
    let mut object_ids = vec![None; skeleton_index.blocks.len()];
    let mut worlds = vec![None; skeleton_index.blocks.len()];
    for (i, object) in skeleton.objects.iter().enumerate() {
        object_ids[object.block as usize] = Some(i);
    }
    for (i, world) in skeleton.world_transforms.iter().enumerate() {
        worlds[world.block as usize] = Some(i);
    }
    let node_position = object_ids
        .get(request.node as usize)
        .copied()
        .flatten()
        .ok_or_else(|| budget.fail("selected skeleton node is not decoded"))?;
    let node = &skeleton.objects[node_position];
    if !matches!(node.kind, nif_scene::ObjectKind::Node { .. }) {
        return Err(budget.fail("selected skeleton object is not a node"));
    }
    let name = node
        .name
        .ok_or_else(|| budget.fail("selected skeleton node has no authored name"))?;
    if skeleton_index.strings[name as usize] != request.node_name_bytes {
        return Err(budget.fail("selected skeleton raw node name differs"));
    }
    let node_world = &skeleton.world_transforms[worlds[request.node as usize]
        .ok_or_else(|| budget.fail("selected skeleton node has no source world"))?];
    if !node_world.reachable_from_footer {
        return Err(budget.fail("selected skeleton node is not reachable from footer"));
    }
    if !attachment_index
        .roots
        .contains(&Some(request.attachment_root))
    {
        return Err(budget.fail("selected attachment root is not an exact footer root"));
    }
    let root = attachment
        .objects
        .iter()
        .find(|o| o.block == request.attachment_root)
        .ok_or_else(|| budget.fail("selected attachment root is not decoded"))?;
    if !matches!(root.kind, nif_scene::ObjectKind::Node { .. }) {
        return Err(budget.fail("selected attachment root is not a node"));
    }
    let root_world = attachment
        .world_transforms
        .iter()
        .find(|w| w.block == root.block)
        .ok_or_else(|| budget.fail("selected attachment root has no source world"))?;
    if root_world.parent.is_some() || !root_world.reachable_from_footer {
        return Err(budget.fail("selected attachment root ancestry differs"));
    }
    budget.reserve::<u8>(request.node_name_bytes.len())?;
    let mut path = Vec::new();
    let mut next = Some(request.node);
    while let Some(block) = next {
        budget.charge(1)?;
        if path.len() >= limits.ancestry_depth {
            return Err(budget.fail("ancestry depth budget exceeded"));
        }
        let object = &skeleton.objects[object_ids[block as usize]
            .ok_or_else(|| budget.fail("required ancestor is not decoded"))?];
        let world = &skeleton.world_transforms[worlds[block as usize]
            .ok_or_else(|| budget.fail("required ancestor has no source world"))?];
        path.push(source_node(
            skeleton_bytes,
            skeleton_index,
            object,
            world.parent,
            &mut budget,
        )?);
        next = world.parent;
    }
    let root_source = source_node(attachment_bytes, attachment_index, root, None, &mut budget)?;
    budget.charge(24)?;
    let mapping = finite(
        compose(node_world.matrix, request.attachment_parent_to_node),
        &budget,
    )?;
    let root_matrix = finite(compose(mapping, scene_affine(root.transform)), &budget)?;
    Ok(Evaluation {
        contract: CONTRACT,
        skeleton_sha256: format!("{:x}", Sha256::digest(skeleton_bytes)),
        attachment_sha256: format!("{:x}", Sha256::digest(attachment_bytes)),
        node_name_bytes: request.node_name_bytes.to_vec(),
        skeleton_path: path,
        attachment_root: root_source,
        source_policy: request.source_policy,
        attachment_parent_to_node: request.attachment_parent_to_node,
        attachment_source_to_skeleton_source: mapping,
        root_to_skeleton_source: root_matrix,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
