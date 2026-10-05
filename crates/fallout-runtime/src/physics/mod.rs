//! Immutable queries over authored collision core geometry in explicit engineering
//! units. Frozen bodies are not Havok simulation, margins, filters or retail movement.
pub mod attachment;
pub mod cell;
mod enclosure;
mod finite;
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
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ray {
    pub origin: [f64; 3],
    /// Must be unit length within the scene's declared engineering tolerance.
    pub direction: [f64; 3],
    pub max_distance: f64,
}
/// Original finite endpoints; no normalized direction replaces their line.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub start: [f64; 3],
    pub end: [f64; 3],
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FiniteQueryLimits {
    pub admission_tests: usize,
    pub primitive_tests: usize,
    pub geometry_tests: usize,
    pub predicate_tests: usize,
    pub rows: usize,
    /// Logical retained result capacity, excluding the immutable source scene.
    pub retained_bytes: usize,
}
impl Default for FiniteQueryLimits {
    fn default() -> Self {
        Self {
            admission_tests: 100_000,
            primitive_tests: 100_000,
            geometry_tests: 1_000_000,
            predicate_tests: 819_200_000,
            rows: 10_000,
            retained_bytes: 16 * 1024 * 1024,
        }
    }
}
impl FiniteQueryLimits {
    fn validate(self) -> QueryResult<()> {
        let max = Self::default();
        if self.admission_tests > max.admission_tests
            || self.primitive_tests > max.primitive_tests
            || self.geometry_tests > max.geometry_tests
            || self.predicate_tests > max.predicate_tests
            || self.rows > max.rows
            || self.retained_bytes > max.retained_bytes
        {
            return Err(QueryError::Invalid(
                "finite query limits must only reduce ceilings",
            ));
        }
        Ok(())
    }
}
pub type SegmentQueryLimits = FiniteQueryLimits;
pub type IntervalQueryLimits = FiniteQueryLimits;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct FiniteQueryWork {
    pub admission_tests: usize,
    pub primitive_tests: usize,
    pub geometry_tests: usize,
    pub predicate_tests: usize,
    pub rows: usize,
    pub retained_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct FiniteQueryReport<T> {
    pub results: Vec<T>,
    pub work: FiniteQueryWork,
}
/// Raw source words accompany results; this DTO is never preparation authority.
// Inline source words let the query precharge the entire retained row capacity;
// boxing the largest variant would introduce a second allocation per result.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreGeometry {
    Sphere {
        radius_binary32: u32,
    },
    Box {
        half_extents_binary32: [u32; 3],
    },
    ConvexCuboid {
        minimum_binary64: [u64; 3],
        maximum_binary64: [u64; 3],
        vertices_binary32: [[u32; 4]; 8],
        planes_binary32: [[u32; 4]; 6],
    },
    Capsule {
        first_binary32: [u32; 3],
        second_binary32: [u32; 3],
        radius_binary32: u32,
    },
    Triangle {
        vertices_binary32: [[u32; 3]; 3],
    },
}
#[derive(Debug, Serialize)]
pub struct SegmentIntersection {
    /// A certified point on the original line and source core, possibly after
    /// entry. The entry enclosure carries the boundary information separately.
    pub provenance: Hit,
    pub parameter: f64,
    pub entry_parameter_bounds: [f64; 2],
    pub exit_parameter_bounds: [f64; 2],
    pub distance_bounds: [f64; 2],
    pub source_core: CoreGeometry,
}
#[derive(Debug, Serialize)]
pub struct SolidOccupancy {
    pub provenance: Hit,
    pub witness_parameter: f64,
    pub entry_parameter_bounds: [f64; 2],
    pub exit_parameter_bounds: [f64; 2],
    pub initial_containment: bool,
    pub source_core: CoreGeometry,
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
