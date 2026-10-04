//! Explicit resident model subsets with one source lease and one global scene.
use super::{
    BodyPlacement, EngineeringUnits, Hit, QueryBudget, QueryError, QueryLimits, Ray, StaticScene,
    cell::{CellError, CellResult},
};
use fallout_data::{
    identity::FormKey,
    nif_collision,
    world::residency::{CellResidency, Readiness, ResidentSources, Ticket},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug)]
pub struct ModelSelection {
    pub model_index: usize,
    pub placements: Vec<BodyPlacement>,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub scene: QueryLimits,
    pub models: usize,
    pub placements: usize,
    pub source_bytes: usize,
    /// Aggregate decoded collision metadata/arrays, plus temporary decoder
    /// reservation checked before each existing decoder call.
    pub decoded_metadata_bytes: usize,
    pub scope_metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            scene: QueryLimits::default(),
            models: 64,
            placements: 1024,
            source_bytes: 64 * 1024 * 1024,
            decoded_metadata_bytes: 64 * 1024 * 1024,
            scope_metadata_bytes: 16 * 1024 * 1024,
        }
    }
}
fn budget(name: &'static str) -> CellError {
    QueryError::Budget(name).into()
}
fn charge(left: &mut usize, count: usize, name: &'static str) -> CellResult<()> {
    *left = left.checked_sub(count).ok_or_else(|| budget(name))?;
    Ok(())
}
fn index_metadata(index: &fallout_data::nif::NifIndex) -> CellResult<usize> {
    let mut bytes = std::mem::size_of::<fallout_data::nif::NifIndex>();
    for (count, size) in [
        (
            index.blocks.capacity(),
            std::mem::size_of::<fallout_data::nif::Block>(),
        ),
        (index.groups.capacity(), 4),
        (index.roots.capacity(), std::mem::size_of::<Option<u32>>()),
        (
            index.export_strings.capacity(),
            std::mem::size_of::<Vec<u8>>(),
        ),
        (index.strings.capacity(), std::mem::size_of::<Vec<u8>>()),
        (index.block_types.capacity(), std::mem::size_of::<String>()),
    ] {
        bytes = bytes
            .checked_add(
                count
                    .checked_mul(size)
                    .ok_or_else(|| budget("NIF index metadata"))?,
            )
            .ok_or_else(|| budget("NIF index metadata"))?;
    }
    for value in index.export_strings.iter().chain(&index.strings) {
        bytes = bytes
            .checked_add(value.capacity())
            .ok_or_else(|| budget("NIF index metadata"))?;
    }
    for value in &index.block_types {
        bytes = bytes
            .checked_add(value.capacity())
            .ok_or_else(|| budget("NIF index metadata"))?;
    }
    for name in index.block_counts.keys() {
        bytes = bytes
            .checked_add(256 + name.len())
            .ok_or_else(|| budget("NIF index metadata"))?;
    }
    Ok(bytes)
}
impl Limits {
    fn validate(self) -> CellResult<Self> {
        let max = Self::default();
        for (value, ceiling) in [
            (self.models, max.models),
            (self.placements, max.placements),
            (self.source_bytes, max.source_bytes),
            (self.decoded_metadata_bytes, max.decoded_metadata_bytes),
            (self.scope_metadata_bytes, max.scope_metadata_bytes),
            (self.scene.blocks, max.scene.blocks),
            (self.scene.shape_visits, max.scene.shape_visits),
            (self.scene.primitives, max.scene.primitives),
            (self.scene.geometry_elements, max.scene.geometry_elements),
        ] {
            if value > ceiling {
                return Err(budget("selection limit ceiling"));
            }
        }
        Ok(self)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ModelScope {
    pub model_index: usize,
    pub source_sha256: String,
    pub body_placements: usize,
    pub source_bytes: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Usage {
    pub source_bytes: usize,
    pub decoded_metadata_bytes: usize,
    pub scope_metadata_bytes: usize,
    pub blocks: usize,
    pub shape_visits: usize,
    pub primitives: usize,
    /// Includes every intermediate model index and the final combined index.
    pub geometry_elements: usize,
}
/// Diagnostic only; no admission accepts this value as authority.
#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    pub root: FormKey,
    pub source_identity: String,
    pub generation: u64,
    pub units: EngineeringUnits,
    pub models: Vec<ModelScope>,
    pub usage: Usage,
}
struct Authority {
    scope: Scope,
    ticket: Ticket,
    live: AtomicBool,
}
impl Authority {
    fn check(&self, owner: &CellResidency) -> CellResult<()> {
        if !self.live.load(Ordering::Acquire) {
            return Err(CellError::Invalid("model selection lease ended"));
        }
        owner.sources(&self.ticket)?;
        Ok(())
    }
}
pub struct SelectionHits {
    authority: Arc<Authority>,
    hits: Vec<Hit>,
}
impl SelectionHits {
    pub fn scope<'a>(&'a self, owner: &'a CellResidency) -> CellResult<&'a Scope> {
        self.authority.check(owner)?;
        Ok(&self.authority.scope)
    }
    pub fn hits<'a>(&'a self, owner: &'a CellResidency) -> CellResult<&'a [Hit]> {
        self.authority.check(owner)?;
        Ok(&self.hits)
    }
}
struct Retained {
    scene: StaticScene,
    authority: Arc<Authority>,
    sources: Arc<ResidentSources>,
}
impl Drop for Retained {
    fn drop(&mut self) {
        self.authority.live.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub struct CellSelection {
    retained: Option<Retained>,
}
impl CellSelection {
    pub fn admit(
        &mut self,
        owner: &mut CellResidency,
        ticket: &Ticket,
        selections: &[ModelSelection],
        units: EngineeringUnits,
        limits: Limits,
    ) -> CellResult<Scope> {
        self.retained = None;
        let sources = owner.sources(ticket)?;
        owner.report_collision(ticket, Readiness::Unsupported)?;
        let limits = limits.validate()?;
        if selections.is_empty() || selections.len() > limits.models {
            return Err(budget("selected models"));
        }
        let mut source_left = limits.source_bytes;
        let mut placements_left = limits.placements;
        let mut metadata = limits.scope_metadata_bytes;
        charge(
            &mut metadata,
            1024 + 4 * ticket.root().origin_plugin.len(),
            "scope metadata",
        )?;
        let mut model_indices = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut models = Vec::new();
        // Preflight all captured sources/identities before any scene compilation.
        for selection in selections {
            charge(&mut metadata, 1024, "model metadata")?;
            if !model_indices.insert(selection.model_index) {
                return Err(CellError::Invalid("duplicate selected model index"));
            }
            if selection.placements.is_empty() {
                return Err(CellError::Invalid("selected model has no body placements"));
            }
            charge(
                &mut placements_left,
                selection.placements.len(),
                "body placements",
            )?;
            let bytes = sources.model(selection.model_index)?;
            charge(&mut source_left, bytes.len(), "selected source bytes")?;
            let sha: [u8; 32] = Sha256::digest(bytes).into();
            for p in &selection.placements {
                charge(&mut metadata, 256, "placement identity metadata")?;
                if p.source_sha256 != sha {
                    return Err(CellError::Invalid(
                        "placement source SHA differs from captured model",
                    ));
                }
                if !identities.insert((p.reference, p.source_sha256, p.body_block)) {
                    return Err(CellError::Invalid(
                        "duplicate selected source/reference/body identity",
                    ));
                }
            }
            models.push(ModelScope {
                model_index: selection.model_index,
                source_sha256: format!("{:x}", Sha256::digest(bytes)),
                body_placements: selection.placements.len(),
                source_bytes: bytes.len(),
            });
        }
        let mut remaining = limits.scene;
        let mut decoded = limits.decoded_metadata_bytes;
        let mut scenes = Vec::with_capacity(selections.len());
        for selection in selections {
            let bytes = sources.model(selection.model_index)?;
            let index_scratch = bytes
                .len()
                .checked_mul(32)
                .ok_or_else(|| budget("collision decode scratch"))?;
            if index_scratch > decoded / 2 {
                return Err(budget("collision decode scratch"));
            }
            // Existing graph validation clones a target type name per opaque
            // link. Bound that amplification before invoking the decoder; use
            // the existing container index reader, never a second parser.
            let preflight =
                fallout_data::nif::inspect(bytes, "selected resident collision preflight")?;
            let longest = preflight
                .block_types
                .iter()
                .map(String::len)
                .max()
                .unwrap_or(0);
            let factor = 64usize
                .checked_add(longest)
                .ok_or_else(|| budget("collision graph scratch"))?;
            let graph_scratch = bytes
                .len()
                .checked_mul(factor)
                .ok_or_else(|| budget("collision graph scratch"))?;
            charge(
                &mut decoded,
                index_metadata(&preflight)?,
                "NIF preflight metadata",
            )?;
            if graph_scratch > decoded / 2 {
                return Err(budget("collision graph scratch"));
            }
            if preflight.blocks.len() > remaining.blocks.min(decoded / 4 / 4096) {
                return Err(budget("selection blocks/decode tables"));
            }
            drop(preflight);
            // Partition temporary reservation before decoding: source/index
            // scratch half, conservative fixed object tables quarter, arrays
            // quarter. Uneven/large source branches explicitly refuse.
            let (index, collision) = nif_collision::decode_with_limits(
                bytes,
                "selected resident collision model",
                nif_collision::Limits {
                    blocks: remaining.blocks.min(decoded / 4 / 4096),
                    array_bytes: decoded / 4,
                    ..Default::default()
                },
            )?;
            charge(
                &mut decoded,
                collision.retained_bytes,
                "decoded collision metadata",
            )?;
            charge(
                &mut decoded,
                index_metadata(&index)?,
                "decoded NIF index metadata",
            )?;
            let (scene, used) =
                StaticScene::build_counted(&collision, &selection.placements, units, remaining)?;
            charge(
                &mut remaining.blocks,
                index.blocks.len().max(used.blocks),
                "selection blocks",
            )?;
            charge(
                &mut remaining.shape_visits,
                used.shape_visits,
                "selection shape visits",
            )?;
            charge(
                &mut remaining.primitives,
                used.primitives,
                "selection primitives",
            )?;
            charge(
                &mut remaining.geometry_elements,
                used.geometry_elements,
                "selection geometry elements",
            )?;
            scenes.push(scene);
        }
        let primitive_count = limits.scene.primitives - remaining.primitives;
        charge(
            &mut remaining.shape_visits,
            primitive_count,
            "selection merge visits",
        )?;
        let scene = StaticScene::combine(
            scenes,
            limits.scene.primitives,
            &mut remaining.geometry_elements,
        )?;
        sources.ticket().check()?;
        let active = owner.sources(ticket)?;
        if !Arc::ptr_eq(&active, &sources) {
            return Err(CellError::Invalid("model selection source batch changed"));
        }
        let scope = Scope {
            root: ticket.root().clone(),
            source_identity: ticket.identity().into(),
            generation: ticket.generation(),
            units: scene.units(),
            models,
            usage: Usage {
                source_bytes: limits.source_bytes - source_left,
                decoded_metadata_bytes: limits.decoded_metadata_bytes - decoded,
                scope_metadata_bytes: limits.scope_metadata_bytes - metadata,
                blocks: limits.scene.blocks - remaining.blocks,
                shape_visits: limits.scene.shape_visits - remaining.shape_visits,
                primitives: primitive_count,
                geometry_elements: limits.scene.geometry_elements - remaining.geometry_elements,
            },
        };
        let authority = Arc::new(Authority {
            scope: scope.clone(),
            ticket: ticket.clone(),
            live: AtomicBool::new(true),
        });
        self.retained = Some(Retained {
            scene,
            authority,
            sources,
        });
        Ok(scope)
    }
    fn current(&mut self, owner: &CellResidency) -> CellResult<&Retained> {
        let retained = self
            .retained
            .as_ref()
            .ok_or(CellError::Invalid("no admitted model selection"))?;
        let check = retained.authority.check(owner).and_then(|()| {
            let active = owner.sources(&retained.authority.ticket)?;
            if !Arc::ptr_eq(&active, &retained.sources) {
                return Err(CellError::Invalid("model selection source batch differs"));
            }
            Ok(())
        });
        if let Err(error) = check {
            self.retained = None;
            return Err(error);
        }
        Ok(self.retained.as_ref().expect("checked selection"))
    }
    pub fn ray_cast(
        &mut self,
        owner: &CellResidency,
        ray: Ray,
        budget: QueryBudget,
    ) -> CellResult<SelectionHits> {
        let retained = self.current(owner)?;
        let hits = retained.scene.ray_cast(ray, budget)?;
        retained.authority.check(owner)?;
        Ok(SelectionHits {
            authority: Arc::clone(&retained.authority),
            hits,
        })
    }
    pub fn overlap_sphere(
        &mut self,
        owner: &CellResidency,
        center: [f64; 3],
        radius: f64,
        budget: QueryBudget,
    ) -> CellResult<SelectionHits> {
        let retained = self.current(owner)?;
        let mut hits = retained.scene.overlap_sphere(center, radius, budget)?;
        hits.sort_by(|a, b| a.source.cmp(&b.source));
        retained.authority.check(owner)?;
        Ok(SelectionHits {
            authority: Arc::clone(&retained.authority),
            hits,
        })
    }
    pub fn invalidate(&mut self, owner: &CellResidency) -> bool {
        self.retained.is_some() && self.current(owner).is_err()
    }
    pub fn release(&mut self, owner: &mut CellResidency) -> CellResult<()> {
        if let Some(retained) = self.retained.take()
            && owner.sources(&retained.authority.ticket).is_ok()
        {
            owner.report_collision(&retained.authority.ticket, Readiness::Pending)?;
        }
        Ok(())
    }
    pub fn retained_primitive_count(&self) -> usize {
        self.retained
            .as_ref()
            .map_or(0, |r| r.scene.primitive_count())
    }
}
