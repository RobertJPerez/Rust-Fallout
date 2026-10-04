//! Immutable queries over authored collision core geometry in explicit engineering
//! units. Frozen bodies are not Havok simulation, margins, filters or retail movement.
pub mod attachment;
pub mod cell;
mod enclosure;
mod index;
mod math;
pub mod multi;
pub mod reference;
mod scene;
mod shape;
pub mod sweep;

use crate::identity::ReferenceId;
use fallout_data::coordinates::Affine;
pub use scene::StaticScene;
use serde::{Deserialize, Serialize};

pub type QueryResult<T> = std::result::Result<T, QueryError>;
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("collision query budget exceeded: {0}")]
    Budget(&'static str),
    #[error("invalid collision query input: {0}")]
    Invalid(&'static str),
    #[error("unsupported collision capability at block {block}: {reason}")]
    Unsupported { block: u32, reason: &'static str },
}

/// No implicit game-to-Havok constant. All results retain source axes. Placement
/// translations are source units; query endpoints/radii are query units.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringUnits {
    pub havok_to_source: f64,
    pub source_to_query: f64,
    /// Relative transform/quaternion validation tolerance; an engineering choice.
    pub transform_tolerance: f64,
}
#[derive(Clone, Debug)]
pub struct BodyPlacement {
    pub reference: ReferenceId,
    pub source_sha256: [u8; 32],
    pub body_block: u32,
    /// Complete authored collision-attachment frame in source units. The caller
    /// composes NIF ancestry and the central reference Affine before this boundary.
    pub attachment_to_source: Affine,
}
#[derive(Clone, Copy, Debug)]
pub struct QueryLimits {
    pub blocks: usize,
    pub shape_visits: usize,
    pub primitives: usize,
    pub geometry_elements: usize,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            blocks: 100_000,
            shape_visits: 200_000,
            primitives: 100_000,
            geometry_elements: 1_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct QueryBudget {
    pub primitive_tests: usize,
    pub geometry_tests: usize,
    pub hits: usize,
}
impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            primitive_tests: 100_000,
            geometry_tests: 1_000_000,
            hits: 10_000,
        }
    }
}
/// Independent work and retained traversal bounds for a single-result ray.
/// Ceilings can only be reduced; no allowance renews per branch or primitive.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirstHitBudget {
    pub index_visits: usize,
    pub primitive_tests: usize,
    pub geometry_tests: usize,
    pub traversal_entries: usize,
}
impl Default for FirstHitBudget {
    fn default() -> Self {
        Self {
            index_visits: 200_000,
            primitive_tests: 100_000,
            geometry_tests: 1_000_000,
            traversal_entries: 64,
        }
    }
}
impl FirstHitBudget {
    fn validate(self) -> QueryResult<()> {
        let ceiling = Self::default();
        if self.index_visits > ceiling.index_visits
            || self.primitive_tests > ceiling.primitive_tests
            || self.geometry_tests > ceiling.geometry_tests
            || self.traversal_entries > ceiling.traversal_entries
        {
            return Err(QueryError::Invalid(
                "first-hit budgets must only reduce ceilings",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ray {
    pub origin: [f64; 3],
    /// Must be unit length within the scene's declared engineering tolerance.
    pub direction: [f64; 3],
    pub max_distance: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SourceId {
    pub reference: ReferenceId,
    pub source_sha256: [u8; 32],
    pub body_block: u32,
    pub shape_block: u32,
    /// Deterministic leaf occurrence distinguishes shared DAG paths.
    pub occurrence: usize,
    pub triangle: Option<usize>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SourceFilter {
    pub layer: u8,
    pub flags_and_parts: u8,
    pub group: u16,
}
impl From<&fallout_data::nif_collision::Filter> for SourceFilter {
    fn from(v: &fallout_data::nif_collision::Filter) -> Self {
        Self {
            layer: v.layer,
            flags_and_parts: v.flags_and_parts,
            group: v.group,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub source: SourceId,
    pub distance: f64,
    pub position: [f64; 3],
    pub material: u32,
    /// Retained bits only. No retail layer/group rejection is inferred.
    pub body_filter: SourceFilter,
    pub shape_filter: Option<SourceFilter>,
    pub welding: Option<u16>,
    /// Core queries exclude convex/packed shell margins explicitly.
    pub authored_shell_radius: f32,
}
