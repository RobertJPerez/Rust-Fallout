//! Authored root/bone order bound to the decoded source graph. Membership is
//! explicitly limited to decoded edges; no pose or external-rig policy is implied.
mod graph;

use crate::{Error, Result, nif, nif_scene};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub partition: super::partition::Limits,
    /// Charged source and graph storage, including bounded traversal scratch.
    pub array_bytes: usize,
    pub graph_checks: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            partition: Default::default(),
            array_bytes: 64 * 1024 * 1024,
            graph_checks: 16_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub block: u32,
    pub block_type: String,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub name: Option<u32>,
    pub extra_data: Vec<Option<u32>>,
    pub controller: Option<u32>,
    pub flags: u32,
    pub transform: super::Transform,
    pub properties: Vec<Option<u32>>,
    pub collision: Option<u32>,
    pub children: Vec<Option<u32>>,
    pub effects: Vec<Option<u32>>,
    pub parent: Option<u32>,
    pub reachable_from_footer: bool,
}

#[derive(Debug, Serialize)]
pub struct Target {
    pub target: Option<u32>,
    pub decoded_node: bool,
    pub parent: Option<u32>,
    pub reachable_from_footer: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Bone {
    pub ordinal: usize,
    pub node: Target,
    pub decoded_root_contains: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Owner {
    pub geometry: u32,
    pub parent: Option<u32>,
    pub reachable_from_footer: bool,
    pub decoded_root_contains: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Instance {
    pub instance: u32,
    pub skeleton_root: Target,
    pub bones: Vec<Bone>,
    pub owners: Vec<Owner>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Diagnostic {
    MissingRoot {
        instance: u32,
    },
    UndecodedRoot {
        instance: u32,
        target: u32,
        block_type: String,
    },
    RootUnreachableFooter {
        instance: u32,
        target: u32,
    },
    MissingBone {
        instance: u32,
        ordinal: usize,
    },
    UndecodedBone {
        instance: u32,
        ordinal: usize,
        target: u32,
        block_type: String,
    },
    BoneUnreachableFooter {
        instance: u32,
        ordinal: usize,
        target: u32,
    },
    BoneOutsideDecodedRoot {
        instance: u32,
        ordinal: usize,
        target: u32,
    },
    OwnerUnreachableFooter {
        instance: u32,
        geometry: u32,
    },
    OwnerOutsideDecodedRoot {
        instance: u32,
        geometry: u32,
    },
}

#[derive(Debug, Serialize)]
pub struct Catalogue {
    pub ancestry_scope: &'static str,
    pub nodes: Vec<Node>,
    pub instances: Vec<Instance>,
    pub footer_roots: Vec<Option<u32>>,
    pub unsupported_scene_edges: Vec<nif_scene::UnsupportedEdge>,
    pub diagnostics: Vec<Diagnostic>,
    /// Conservative charged storage; includes temporary graph vectors.
    pub retained_bytes: usize,
    pub runtime_ready: bool,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub skin: super::partition::Source,
    pub bindings: Catalogue,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}

pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    let (index, source, _) = decode_with_scene(bytes, source, limits)?;
    Ok((index, source))
}

pub(super) fn decode_with_scene(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source, nif_scene::Scene)> {
    let (index, skin, scene) =
        super::partition::decode_with_scene(bytes, source, limits.partition)?;
    let bindings = graph::bind(bytes, &index, &skin.skin, &scene, source, limits)?;
    Ok((index, Source { skin, bindings }, scene))
}

fn reserve<T>(remaining: &mut usize, count: usize, source: &str) -> Result<()> {
    *remaining = count
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| remaining.checked_sub(bytes))
        .ok_or_else(|| Error::Unsupported(format!("{source}: binding storage budget exceeded")))?;
    Ok(())
}
