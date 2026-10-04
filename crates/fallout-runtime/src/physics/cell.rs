//! Selected-model engineering geometry tied to the existing cell owner and epoch.
//! This cache never owns persistent references or certifies whole-cell simulation.
use super::{
    BodyPlacement, EngineeringUnits, Hit, QueryBudget, QueryError, QueryLimits, Ray, StaticScene,
};
use fallout_data::{
    identity::FormKey,
    nif_collision,
    resource_jobs::JobError,
    world::residency::{CellResidency, Readiness, ResidentSources, Ticket},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub type CellResult<T> = std::result::Result<T, CellError>;
#[derive(Debug, thiserror::Error)]
pub enum CellError {
    #[error("cell collision source lease: {0}")]
    Residency(#[from] JobError),
    #[error("cell collision decoding: {0}")]
    Decode(#[from] fallout_data::Error),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error("cell collision admission: {0}")]
    Invalid(&'static str),
}

#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    pub root: FormKey,
    pub source_identity: String,
    pub generation: u64,
    pub model_index: usize,
    pub source_sha256: String,
    pub units: EngineeringUnits,
}

/// No detached usable hit accessor or serialization. The caller must borrow the
/// original owner while validating and reading these results. A future canonical
/// movement commit must still validate its own reference revision and this epoch.
pub struct CellHits {
    scope: Scope,
    ticket: Ticket,
    hits: Vec<Hit>,
}
impl CellHits {
    pub fn scope<'a>(&'a self, world: &'a CellResidency) -> CellResult<&'a Scope> {
        world.sources(&self.ticket)?;
        Ok(&self.scope)
    }
    pub fn hits<'a>(&'a self, world: &'a CellResidency) -> CellResult<&'a [Hit]> {
        world.sources(&self.ticket)?;
        Ok(&self.hits)
    }
}

struct Retained {
    scene: StaticScene,
    scope: Scope,
    ticket: Ticket,
    sources: Arc<ResidentSources>,
}

/// One bounded selected-model scene. Source bytes and plan accounting remain
/// world-owned and charged through the retained upstream lease.
#[derive(Default)]
pub struct CellCollision {
    retained: Option<Retained>,
}
impl CellCollision {
    /// Replacing this selection releases the previous geometry first. Failed
    /// decoding/build/admission leaves no queryable replacement or partial scene.
    /// Caller placements are explicit engineering inputs, not canonical writes.
    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        &mut self,
        world: &mut CellResidency,
        ticket: &Ticket,
        model_index: usize,
        placements: &[BodyPlacement],
        units: EngineeringUnits,
        limits: QueryLimits,
    ) -> CellResult<Scope> {
        self.retained = None;
        let sources = world.sources(ticket)?;
        let build = (|| -> CellResult<Retained> {
            let bytes = sources.model(model_index)?;
            let digest: [u8; 32] = Sha256::digest(bytes).into();
            if placements.is_empty() || placements.iter().any(|p| p.source_sha256 != digest) {
                return Err(CellError::Invalid(
                    "placement source SHA differs from captured model",
                ));
            }
            let (_, collision) = nif_collision::decode_with_limits(
                bytes,
                "resident collision model",
                nif_collision::Limits {
                    blocks: limits.blocks,
                    ..Default::default()
                },
            )?;
            let scene = StaticScene::build(&collision, placements, units, limits)?;
            sources.ticket().check()?;
            let scope = Scope {
                root: ticket.root().clone(),
                source_identity: ticket.identity().to_owned(),
                generation: ticket.generation(),
                model_index,
                source_sha256: format!("{:x}", Sha256::digest(bytes)),
                units: scene.units(),
            };
            Ok(Retained {
                scene,
                scope,
                ticket: ticket.clone(),
                sources,
            })
        })();
        // Selected engineering geometry does not prove measured units, filter /
        // shell / dynamics semantics or complete cell collision coverage. Even
        // a successful selected build must keep the faithful activation gate shut.
        world.report_collision(ticket, Readiness::Unsupported)?;
        let retained = build?;
        let scope = retained.scope.clone();
        self.retained = Some(retained);
        Ok(scope)
    }

    fn current(&mut self, world: &CellResidency) -> CellResult<&Retained> {
        let retained = self
            .retained
            .as_ref()
            .ok_or(CellError::Invalid("no admitted collision geometry"))?;
        let valid = world
            .sources(&retained.ticket)
            .map_err(CellError::from)
            .and_then(|sources| {
                if Arc::ptr_eq(&sources, &retained.sources) {
                    Ok(())
                } else {
                    Err(CellError::Invalid(
                        "collision lease differs from active source batch",
                    ))
                }
            });
        if let Err(error) = valid {
            self.retained = None;
            return Err(error);
        }
        Ok(self.retained.as_ref().expect("validated retained geometry"))
    }

    pub fn ray_cast(
        &mut self,
        world: &CellResidency,
        ray: Ray,
        budget: QueryBudget,
    ) -> CellResult<CellHits> {
        let retained = self.current(world)?;
        let hits = retained.scene.ray_cast(ray, budget)?;
        world.sources(&retained.ticket)?;
        Ok(CellHits {
            scope: retained.scope.clone(),
            ticket: retained.ticket.clone(),
            hits,
        })
    }
    pub fn overlap_sphere(
        &mut self,
        world: &CellResidency,
        center: [f64; 3],
        radius: f64,
        budget: QueryBudget,
    ) -> CellResult<CellHits> {
        let retained = self.current(world)?;
        let hits = retained.scene.overlap_sphere(center, radius, budget)?;
        world.sources(&retained.ticket)?;
        Ok(CellHits {
            scope: retained.scope.clone(),
            ticket: retained.ticket.clone(),
            hits,
        })
    }

    /// Called at the owning application's unload/retry boundary. A stale scene
    /// releases both geometry and its complete upstream source/plan lease.
    pub fn invalidate(&mut self, world: &CellResidency) -> bool {
        self.retained.is_some() && self.current(world).is_err()
    }
    pub fn release(&mut self, world: &mut CellResidency) -> CellResult<()> {
        if let Some(retained) = self.retained.take()
            && world.sources(&retained.ticket).is_ok()
        {
            world.report_collision(&retained.ticket, Readiness::Pending)?;
        }
        Ok(())
    }
    /// Diagnostic ownership count only; live readiness requires current owner.
    pub fn retained_primitive_count(&self) -> usize {
        self.retained
            .as_ref()
            .map_or(0, |r| r.scene.primitive_count())
    }
}
