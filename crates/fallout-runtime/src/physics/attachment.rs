//! Static engineering attachment frames from exact source scene ancestry.
//! No source report or caller-decoded graph can authorize this construction.
use super::{
    BodyPlacement, EngineeringUnits, QueryError, QueryLimits, QueryResult, StaticScene,
    math::{Similarity, compose},
};
use crate::identity::ReferenceId;
use fallout_data::{coordinates::Affine, nif, nif_collision, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("source attachment decoding: {0}")]
    Decode(#[from] fallout_data::Error),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error("source attachment refusal: {0}")]
    Invalid(&'static str),
}
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub reference: ReferenceId,
    pub source_sha256: [u8; 32],
    pub collision_object: u32,
    pub body_block: u32,
    pub target_block: u32,
    /// Explicit caller frame in source units, outside the NIF ancestry.
    pub placement_to_source: Affine,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub blocks: usize,
    pub decoded_metadata_bytes: usize,
    /// One allowance split into two equal existing-decoder array caps.
    pub array_bytes: usize,
    pub source_link_visits: usize,
    pub ancestry_visits: usize,
    pub scope_metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 4 * 1024 * 1024,
            blocks: 10_000,
            decoded_metadata_bytes: 64 * 1024 * 1024,
            array_bytes: 16 * 1024 * 1024,
            source_link_visits: 200_000,
            ancestry_visits: 10_000,
            scope_metadata_bytes: 16 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (value, ceiling) in [
            (self.source_bytes, max.source_bytes),
            (self.blocks, max.blocks),
            (self.decoded_metadata_bytes, max.decoded_metadata_bytes),
            (self.array_bytes, max.array_bytes),
            (self.source_link_visits, max.source_link_visits),
            (self.ancestry_visits, max.ancestry_visits),
            (self.scope_metadata_bytes, max.scope_metadata_bytes),
        ] {
            if value > ceiling {
                return Err(QueryError::Budget("attachment limit ceiling").into());
            }
        }
        Ok(self)
    }
}
fn charge(left: &mut usize, n: usize, name: &'static str) -> Result<()> {
    *left = left.checked_sub(n).ok_or(QueryError::Budget(name))?;
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub struct Span {
    pub block: u32,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
}
fn span(bytes: &[u8], index: &nif::NifIndex, block: u32) -> Result<Span> {
    let row = index
        .blocks
        .get(block as usize)
        .ok_or(Error::Invalid("source block absent"))?;
    let end = row
        .offset
        .checked_add(row.bytes)
        .ok_or(Error::Invalid("source span overflow"))?;
    let payload = bytes
        .get(row.offset..end)
        .ok_or(Error::Invalid("source span absent"))?;
    Ok(Span {
        block,
        offset: row.offset,
        bytes: row.bytes,
        sha256: format!("{:x}", Sha256::digest(payload)),
    })
}
fn verify_block(bytes: &[u8], index: &nif::NifIndex, block: &nif_collision::Block) -> Result<Span> {
    let observed = span(bytes, index, block.block)?;
    if observed.offset != block.source_offset
        || observed.bytes != block.source_bytes
        || observed.sha256 != block.source_sha256
    {
        return Err(Error::Invalid(
            "decoded collision span differs from exact source",
        ));
    }
    Ok(observed)
}
#[derive(Clone, Debug, Serialize)]
pub struct Ancestor {
    pub span: Span,
    pub parent: Option<u32>,
    pub local: Affine,
}
#[derive(Clone, Debug, Serialize)]
pub struct Usage {
    pub source_bytes: usize,
    pub blocks: usize,
    /// Conservative admission reservation, not an observed memory peak.
    pub decoded_reservation_bytes: usize,
    pub source_link_visits: usize,
    pub ancestry_visits: usize,
    pub scope_metadata_bytes: usize,
}
/// Observation only. The occurrence includes the exact collision-object block,
/// so two separate queries selecting a shared body remain distinguishable.
#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    pub source_sha256: String,
    pub reference: ReferenceId,
    pub collision_object: Span,
    pub body: Span,
    pub target: Span,
    pub collision_flags: u16,
    pub blend_gains: Option<[f32; 2]>,
    /// Target first, then each parent, ending at one exact footer root.
    pub ancestry: Vec<Ancestor>,
    pub source_world: Affine,
    pub caller_placement: Affine,
    pub attachment_to_source: Affine,
    pub units: EngineeringUnits,
    pub usage: Usage,
}
pub struct SourceAttachment {
    scope: Scope,
    placement: BodyPlacement,
    collision: nif_collision::Collision,
}
impl SourceAttachment {
    pub fn derive(
        bytes: &[u8],
        selection: Selection,
        units: EngineeringUnits,
        limits: Limits,
    ) -> Result<Self> {
        let limits = limits.validate()?;
        if bytes.len() > limits.source_bytes {
            return Err(QueryError::Budget("attachment source bytes").into());
        }
        if !units.havok_to_source.is_finite()
            || units.havok_to_source <= 0.
            || !units.source_to_query.is_finite()
            || units.source_to_query <= 0.
            || !units.transform_tolerance.is_finite()
            || !(0. ..=1e-3).contains(&units.transform_tolerance)
        {
            return Err(Error::Invalid(
                "explicit engineering units/tolerance required",
            ));
        }
        if <[u8; 32]>::from(Sha256::digest(bytes)) != selection.source_sha256 {
            return Err(Error::Invalid("whole source SHA differs"));
        }
        Similarity::new(selection.placement_to_source, units.transform_tolerance)?;
        let index_scratch = bytes
            .len()
            .checked_mul(32)
            .ok_or(QueryError::Budget("attachment index scratch"))?;
        if index_scratch > limits.decoded_metadata_bytes / 4 {
            return Err(QueryError::Budget("attachment index scratch").into());
        }
        let preflight = nif::inspect(bytes, "source attachment preflight")?;
        let count = preflight.blocks.len();
        if count > limits.blocks {
            return Err(QueryError::Budget("attachment blocks").into());
        }
        let longest = preflight
            .block_types
            .iter()
            .map(String::len)
            .max()
            .unwrap_or(0);
        // Both existing graph resolvers/index readers, including opaque cloned
        // names, plus conservative tables and a single split array allowance.
        let reserved = bytes
            .len()
            .checked_mul(128 + 2 * longest)
            .and_then(|n| {
                count
                    .checked_mul(8192)
                    .and_then(|fixed| n.checked_add(fixed))
            })
            .and_then(|n| n.checked_add(limits.array_bytes))
            .ok_or(QueryError::Budget("attachment decoded reservation"))?;
        if reserved > limits.decoded_metadata_bytes {
            return Err(QueryError::Budget("attachment decoded reservation").into());
        }
        let mut links = limits.source_link_visits;
        charge(&mut links, count, "attachment source links")?;
        let mut metadata = limits.scope_metadata_bytes;
        charge(&mut metadata, 4096, "attachment metadata")?;
        charge(
            &mut metadata,
            count
                .checked_mul(1024)
                .ok_or(QueryError::Budget("attachment metadata"))?,
            "attachment metadata",
        )?;
        drop(preflight);
        let decoder_limits = nif_scene::Limits {
            input_bytes: limits.source_bytes,
            blocks: limits.blocks,
            array_bytes: limits.array_bytes / 2,
        };
        let (index, collision) = nif_collision::decode_with_limits(
            bytes,
            "source attachment collision",
            decoder_limits,
        )?;
        let (_, scene) =
            nif_scene::decode_with_limits(bytes, "source attachment scene", decoder_limits)?;
        let mut collision_blocks = BTreeMap::new();
        let mut same_target = 0;
        for block in &collision.blocks {
            charge(&mut links, 1, "attachment source links")?;
            collision_blocks.insert(block.block, block);
            if let nif_collision::Data::CollisionObject { target, .. } = &block.data {
                charge(&mut links, 2, "attachment source links")?;
                same_target += usize::from(*target == Some(selection.target_block));
            }
        }
        // An opaque source block could conceal an additional scene parent. The
        // supported union is deliberately strict; do not infer absence of links.
        for ids in scene.unsupported_blocks.values() {
            for id in ids {
                charge(&mut links, 1, "attachment source links")?;
                if !collision_blocks.contains_key(id) {
                    return Err(Error::Invalid(
                        "opaque source block prevents complete attachment ancestry",
                    ));
                }
            }
        }
        if !scene.unsupported_scene_edges.is_empty() {
            return Err(Error::Invalid("unsupported source scene edge"));
        }
        let selected = collision_blocks
            .get(&selection.collision_object)
            .ok_or(Error::Invalid("selected collision object absent"))?;
        let nif_collision::Data::CollisionObject {
            target,
            body,
            flags,
            blend_gains,
        } = &selected.data
        else {
            return Err(Error::Invalid("selection is not a collision object"));
        };
        if *target != Some(selection.target_block) || *body != Some(selection.body_block) {
            return Err(Error::Invalid(
                "selected object target/body differs or is null",
            ));
        }
        if same_target != 1 {
            return Err(Error::Invalid("ambiguous collision target"));
        }
        let body = collision_blocks
            .get(&selection.body_block)
            .ok_or(Error::Invalid("selected body absent"))?;
        if !matches!(body.data, nif_collision::Data::RigidBody { .. }) {
            return Err(Error::Invalid("selected body is not a rigid body"));
        }
        let mut objects = BTreeMap::new();
        let mut incoming = 0;
        for object in &scene.objects {
            charge(&mut links, 3, "attachment source links")?;
            if let nif_scene::ObjectKind::Node { children, effects } = &object.kind {
                charge(
                    &mut links,
                    children.len() + effects.len(),
                    "attachment source links",
                )?;
            }
            if object.collision == Some(selection.collision_object) {
                incoming += 1;
                if object.block != selection.target_block {
                    return Err(Error::Invalid(
                        "collision backreference belongs to another target",
                    ));
                }
            }
            objects.insert(object.block, object);
        }
        if incoming != 1 {
            return Err(Error::Invalid(
                "missing or ambiguous scene collision backreference",
            ));
        }
        let mut worlds = BTreeMap::new();
        for world in &scene.world_transforms {
            charge(&mut links, 1, "attachment source links")?;
            worlds.insert(world.block, world);
        }
        let mut roots = BTreeSet::new();
        for root in &index.roots {
            charge(&mut links, 1, "attachment source links")?;
            if let Some(root) = root
                && !roots.insert(*root)
            {
                return Err(Error::Invalid("repeated footer root"));
            }
        }
        let source_world = worlds.get(&selection.target_block).ok_or(Error::Invalid(
            "selected target has no supported scene transform",
        ))?;
        if !source_world.reachable_from_footer {
            return Err(Error::Invalid(
                "target ancestry is outside the footer roots",
            ));
        }
        let source_world = Affine {
            rows: source_world.matrix,
        };
        let mut ancestry = Vec::new();
        let mut ancestry_left = limits.ancestry_visits;
        let mut current = Some(selection.target_block);
        while let Some(id) = current {
            charge(&mut ancestry_left, 1, "attachment ancestry visits")?;
            charge(&mut metadata, 1024, "attachment metadata")?;
            let object = objects
                .get(&id)
                .ok_or(Error::Invalid("unsupported required ancestor"))?;
            if object.controller.is_some() {
                return Err(Error::Invalid("controller on required ancestry"));
            }
            let transform = object.transform;
            let local = Affine {
                rows: std::array::from_fn(|r| {
                    [
                        f64::from(transform.rotation[r][0]) * f64::from(transform.scale),
                        f64::from(transform.rotation[r][1]) * f64::from(transform.scale),
                        f64::from(transform.rotation[r][2]) * f64::from(transform.scale),
                        f64::from(transform.translation[r]),
                    ]
                }),
            };
            Similarity::new(local, units.transform_tolerance)?;
            let parent = worlds
                .get(&id)
                .ok_or(Error::Invalid("ancestor world transform absent"))?
                .parent;
            if parent.is_none() && !roots.contains(&id) {
                return Err(Error::Invalid("ancestor is not a footer root"));
            }
            ancestry.push(Ancestor {
                span: span(bytes, &index, id)?,
                parent,
                local,
            });
            current = parent;
        }
        Similarity::new(source_world, units.transform_tolerance)?;
        // Scene already composed root-to-target. Do not multiply target locals
        // again. StaticScene alone applies Havok units and active body pose.
        let attachment_to_source = compose(selection.placement_to_source, source_world);
        Similarity::new(attachment_to_source, units.transform_tolerance)?;
        let scope = Scope {
            source_sha256: format!("{:x}", Sha256::digest(bytes)),
            reference: selection.reference,
            collision_object: verify_block(bytes, &index, selected)?,
            body: verify_block(bytes, &index, body)?,
            target: span(bytes, &index, selection.target_block)?,
            collision_flags: *flags,
            blend_gains: *blend_gains,
            ancestry,
            source_world,
            caller_placement: selection.placement_to_source,
            attachment_to_source,
            units,
            usage: Usage {
                source_bytes: bytes.len(),
                blocks: count,
                decoded_reservation_bytes: reserved,
                source_link_visits: limits.source_link_visits - links,
                ancestry_visits: limits.ancestry_visits - ancestry_left,
                scope_metadata_bytes: limits.scope_metadata_bytes - metadata,
            },
        };
        let placement = BodyPlacement {
            reference: selection.reference,
            source_sha256: selection.source_sha256,
            body_block: selection.body_block,
            attachment_to_source,
        };
        Ok(Self {
            scope,
            placement,
            collision,
        })
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn placement(&self) -> &BodyPlacement {
        &self.placement
    }
    pub fn build_scene(&self, limits: QueryLimits) -> QueryResult<StaticScene> {
        StaticScene::build(
            &self.collision,
            std::slice::from_ref(&self.placement),
            self.scope.units,
            limits,
        )
    }
}
