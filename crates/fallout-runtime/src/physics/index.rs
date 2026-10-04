//! Conservative source culling. Every accepted candidate still reaches its
//! original shape predicate; uncertifiable transform enclosures fall back.
use super::{
    QueryError, QueryResult, Ray,
    enclosure::{
        self, Interval, upper_product as product, upper_quotient as quotient, upper_sum as sum,
    },
    math::{Similarity, V},
    shape::cuboid_may_ray,
};
use std::{
    iter::{Copied, Peekable},
    ops::Range,
    slice::Iter,
    vec::IntoIter,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    minimum: V,
    maximum: V,
    coefficients: [[f64; 3]; 3],
    constant: V,
    underflow: V,
    radius_coefficients: V,
    projection_magnitudes: [[f64; 4]; 3],
    inverse_scale: f64,
    exact_identity: bool,
}
impl Bounds {
    pub fn new(minimum: V, maximum: V) -> Self {
        Self {
            minimum: minimum.map(f64::next_down),
            maximum: maximum.map(f64::next_up),
            coefficients: [[0.; 3]; 3],
            constant: [0.; 3],
            underflow: [0.; 3],
            radius_coefficients: [1.; 3],
            projection_magnitudes: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            inverse_scale: 1.,
            exact_identity: true,
        }
    }
    pub fn transformed(minimum: V, maximum: V, transform: &Similarity) -> Option<Self> {
        if transform.is_identity() {
            return Some(Self::new(minimum, maximum));
        }
        let inverse = transform.inverse_rows();
        let linear = std::array::from_fn(|i| std::array::from_fn(|j| inverse[i][j]));
        let forward = enclosure::inverse(linear)?;
        let local = [
            Interval::new(minimum[0], maximum[0])?.subtract(Interval::point(inverse[0][3]))?,
            Interval::new(minimum[1], maximum[1])?.subtract(Interval::point(inverse[1][3]))?,
            Interval::new(minimum[2], maximum[2])?.subtract(Interval::point(inverse[2][3]))?,
        ];
        let mut result = Self::new([0.; 3], [0.; 3]);
        result.exact_identity = false;
        result.projection_magnitudes = inverse.map(|row| row.map(f64::abs));
        result.inverse_scale = quotient(1., transform.scale);
        // Existing Affine.point/local_vector use at most 7/6 rounded operations.
        // 16EPS exceeds gamma7 (unit roundoff EPS/2); 8minsub covers underflow.
        let gamma = 16. * f64::EPSILON;
        for (i, row) in forward.iter().enumerate() {
            let mut world = Interval::point(0.);
            let mut row_sum = 0.;
            for (j, element) in row.iter().enumerate() {
                world = world.add(element.multiply(local[j])?)?;
                let magnitude = element.absolute_upper();
                row_sum = sum(row_sum, magnitude);
                result.constant[i] =
                    sum(result.constant[i], product(magnitude, inverse[j][3].abs()));
                for (k, value) in linear[j].iter().enumerate() {
                    result.coefficients[i][k] =
                        sum(result.coefficients[i][k], product(magnitude, value.abs()));
                }
            }
            result.minimum[i] = world.lower.next_down();
            result.maximum[i] = world.upper.next_up();
            result.constant[i] = product(gamma, result.constant[i]);
            result.coefficients[i] = result.coefficients[i].map(|v| product(gamma, v));
            // Include one more minsub for rounded radius division. This extra
            // allowance is conservative for rays as well as overlaps.
            result.underflow[i] = product(f64::from_bits(9), row_sum);
            result.radius_coefficients[i] =
                product(quotient(row_sum, transform.scale), 1. + f64::EPSILON);
        }
        if result
            .minimum
            .iter()
            .chain(&result.maximum)
            .chain(result.coefficients.iter().flatten())
            .chain(&result.constant)
            .chain(&result.underflow)
            .chain(&result.radius_coefficients)
            .chain(std::iter::once(&result.inverse_scale))
            .any(|v| !v.is_finite())
        {
            return None;
        }
        Some(result)
    }
    fn union(self, other: Self) -> Self {
        Self {
            minimum: std::array::from_fn(|i| self.minimum[i].min(other.minimum[i])),
            maximum: std::array::from_fn(|i| self.maximum[i].max(other.maximum[i])),
            coefficients: std::array::from_fn(|i| {
                std::array::from_fn(|j| self.coefficients[i][j].max(other.coefficients[i][j]))
            }),
            constant: std::array::from_fn(|i| self.constant[i].max(other.constant[i])),
            underflow: std::array::from_fn(|i| self.underflow[i].max(other.underflow[i])),
            radius_coefficients: std::array::from_fn(|i| {
                self.radius_coefficients[i].max(other.radius_coefficients[i])
            }),
            projection_magnitudes: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    self.projection_magnitudes[i][j].max(other.projection_magnitudes[i][j])
                })
            }),
            inverse_scale: self.inverse_scale.max(other.inverse_scale),
            exact_identity: self.exact_identity && other.exact_identity,
        }
    }
    fn center(self, axis: usize) -> f64 {
        0.5 * self.minimum[axis] + 0.5 * self.maximum[axis]
    }
    fn projection_in_domain(self, value: V, point: bool) -> bool {
        self.projection_magnitudes.iter().all(|row| {
            let mut magnitude = if point { row[3] } else { 0. };
            for (coefficient, value) in row[..3].iter().zip(value) {
                magnitude = sum(magnitude, product(*coefficient, value.abs()));
            }
            sum(
                product(magnitude, 1. + 16. * f64::EPSILON),
                f64::from_bits(8),
            ) <= 1e50
        })
    }
    fn ray_padding(self, ray: Ray) -> V {
        std::array::from_fn(|i| {
            let mut error = sum(
                self.constant[i],
                product(self.underflow[i], sum(1., ray.max_distance)),
            );
            for k in 0..3 {
                error = sum(
                    error,
                    product(
                        self.coefficients[i][k],
                        sum(
                            ray.origin[k].abs(),
                            product(ray.max_distance, ray.direction[k].abs()),
                        ),
                    ),
                );
            }
            error
        })
    }
    fn may_ray(self, ray: Ray) -> bool {
        // Culling must not hide the original local-domain refusal. If the
        // conversion cannot be certified within that domain, retain the leaf.
        if !self.exact_identity
            && (!self.projection_in_domain(ray.origin, true)
                || !self.projection_in_domain(ray.direction, false))
        {
            return true;
        }
        let (minimum, maximum) = if self.exact_identity {
            (self.minimum, self.maximum)
        } else {
            let Some(bounds) = self.expanded(self.ray_padding(ray)) else {
                return true;
            };
            bounds
        };
        cuboid_may_ray(
            ray.origin,
            ray.direction,
            minimum,
            maximum,
            ray.max_distance,
        )
    }
    fn overlap_padding(self, center: V, radius: f64) -> V {
        std::array::from_fn(|i| {
            let mut error = sum(
                self.constant[i],
                sum(
                    self.underflow[i],
                    product(self.radius_coefficients[i], radius),
                ),
            );
            for (k, value) in center.iter().enumerate() {
                error = sum(error, product(self.coefficients[i][k], value.abs()));
            }
            error
        })
    }
    fn may_overlap(self, center: V, radius: f64) -> bool {
        if !self.exact_identity
            && (!self.projection_in_domain(center, true)
                || sum(product(self.inverse_scale, radius), f64::from_bits(1)) > 1e50)
        {
            return true;
        }
        self.expanded(self.overlap_padding(center, radius))
            .is_none_or(|(minimum, maximum)| {
                (0..3).all(|i| minimum[i] <= center[i] && center[i] <= maximum[i])
            })
    }
    fn expanded(self, pad: V) -> Option<(V, V)> {
        if pad.iter().any(|v| !v.is_finite() || *v < 0.) {
            return None;
        }
        let minimum: V = std::array::from_fn(|i| (self.minimum[i] - pad[i]).next_down());
        let maximum: V = std::array::from_fn(|i| (self.maximum[i] + pad[i]).next_up());
        (minimum.iter().chain(&maximum).all(|v| v.is_finite())).then_some((minimum, maximum))
    }
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;

#[derive(Debug)]
enum Node {
    Leaf {
        bounds: Bounds,
        ordinal: usize,
    },
    Branch {
        bounds: Bounds,
        left: usize,
        right: usize,
    },
}
impl Node {
    fn bounds(&self) -> Bounds {
        match *self {
            Self::Leaf { bounds, .. } | Self::Branch { bounds, .. } => bounds,
        }
    }
}

#[derive(Debug)]
pub(super) struct Index {
    nodes: Vec<Node>,
    root: usize,
    fallback: Vec<usize>,
}
fn charge(elements: &mut usize, count: usize, name: &'static str) -> QueryResult<()> {
    *elements = elements
        .checked_sub(count)
        .ok_or(QueryError::Budget(name))?;
    Ok(())
}

// Each range halves: stack depth is at most usize::BITS even if a caller raises
// scene limits. This is unrelated to source DAG depth/cycles.
fn build_range(
    entries: &[(usize, Bounds)],
    nodes: &mut Vec<Node>,
    elements: &mut usize,
) -> QueryResult<usize> {
    let node = if entries.len() == 1 {
        Node::Leaf {
            bounds: entries[0].1,
            ordinal: entries[0].0,
        }
    } else {
        let midpoint = entries.len() / 2;
        let left = build_range(&entries[..midpoint], nodes, elements)?;
        let right = build_range(&entries[midpoint..], nodes, elements)?;
        Node::Branch {
            bounds: nodes[left].bounds().union(nodes[right].bounds()),
            left,
            right,
        }
    };
    charge(elements, 1, "geometry/index elements")?;
    let index = nodes.len();
    nodes.push(node);
    Ok(index)
}
impl Index {
    pub fn build(
        mut entries: Vec<(usize, Bounds)>,
        fallback: Vec<usize>,
        elements: &mut usize,
    ) -> QueryResult<Option<Self>> {
        if entries.len() < 8 {
            return Ok(None);
        }
        let bounds = entries
            .iter()
            .map(|v| v.1)
            .reduce(Bounds::union)
            .expect("nonempty index");
        let axis = (0..3)
            .max_by(|&a, &b| {
                (bounds.maximum[a] - bounds.minimum[a])
                    .total_cmp(&(bounds.maximum[b] - bounds.minimum[b]))
            })
            .expect("three axes");
        entries.sort_unstable_by(|a, b| {
            a.1.center(axis)
                .total_cmp(&b.1.center(axis))
                .then(a.0.cmp(&b.0))
        });
        charge(elements, fallback.len(), "geometry/index elements")?;
        let mut nodes = Vec::new();
        let root = build_range(&entries, &mut nodes, elements)?;
        Ok(Some(Self {
            nodes,
            root,
            fallback,
        }))
    }
    fn candidates(
        &self,
        primitive_work: usize,
        mut may_intersect: impl FnMut(Bounds) -> bool,
    ) -> QueryResult<Candidates<'_>> {
        // Bound broad-phase work independently while preserving the existing
        // narrow primitive counter. At most2P nodes precede at mostP primitives.
        let mut visits = primitive_work.saturating_mul(2).min(self.nodes.len());
        let mut stack = vec![self.root];
        let mut selected = Vec::new();
        while let Some(index) = stack.pop() {
            charge(&mut visits, 1, "spatial index visits")?;
            let node = &self.nodes[index];
            if !may_intersect(node.bounds()) {
                continue;
            }
            match *node {
                Node::Leaf { ordinal, .. } => selected.push(ordinal),
                Node::Branch { left, right, .. } => {
                    stack.push(right);
                    stack.push(left);
                }
            }
        }
        selected.sort_unstable();
        Ok(Candidates::Indexed {
            selected: selected.into_iter().peekable(),
            fallback: self.fallback.iter().copied().peekable(),
        })
    }
    pub fn ray(&self, ray: Ray, primitive_work: usize) -> QueryResult<Candidates<'_>> {
        self.candidates(primitive_work, |bounds| bounds.may_ray(ray))
    }
    pub fn overlap(
        &self,
        center: V,
        radius: f64,
        primitive_work: usize,
    ) -> QueryResult<Candidates<'_>> {
        self.candidates(primitive_work, |bounds| bounds.may_overlap(center, radius))
    }
    pub fn first_candidates(
        &self,
        ray: Ray,
        node_visits: usize,
        traversal_entries: usize,
    ) -> QueryResult<FirstCandidates<'_>> {
        // build_range halves positive ranges and emits exactly2N-1 nodes.
        // ceil(log2 N)+1 bounds the pending depth-first stack. Charge its
        // retained capacity before allocation, independently of node visits.
        let leaves = self.nodes.len() / 2 + 1;
        let depth = (usize::BITS - (leaves - 1).leading_zeros()) as usize + 1;
        if depth > traversal_entries {
            return Err(QueryError::Budget("first-hit traversal entries"));
        }
        let mut stack = Vec::with_capacity(depth);
        stack.push(self.root);
        Ok(FirstCandidates::Indexed {
            index: self,
            ray,
            node_visits,
            stack,
            stack_limit: depth,
            fallback: self.fallback.iter().copied(),
        })
    }
}

/// Stream candidates without retaining or sorting every admitted ordinal.
/// The original full ray controls culling; lacking a certified strictly-farther
/// bound never permits a best-distance early exit or hiding a narrow refusal.
pub(super) enum FirstCandidates<'a> {
    All(Range<usize>),
    Indexed {
        index: &'a Index,
        ray: Ray,
        node_visits: usize,
        stack: Vec<usize>,
        stack_limit: usize,
        fallback: Copied<Iter<'a, usize>>,
    },
}
impl FirstCandidates<'_> {
    pub fn next(&mut self) -> QueryResult<Option<usize>> {
        match self {
            Self::All(range) => Ok(range.next()),
            Self::Indexed {
                index,
                ray,
                node_visits,
                stack,
                stack_limit,
                fallback,
            } => {
                while let Some(node_index) = stack.pop() {
                    charge(node_visits, 1, "first-hit index visits")?;
                    let node = &index.nodes[node_index];
                    if !node.bounds().may_ray(*ray) {
                        continue;
                    }
                    match *node {
                        Node::Leaf { ordinal, .. } => return Ok(Some(ordinal)),
                        Node::Branch { left, right, .. } => {
                            if stack.len().checked_add(2).is_none_or(|n| n > *stack_limit) {
                                return Err(QueryError::Budget("first-hit traversal entries"));
                            }
                            stack.push(right);
                            stack.push(left);
                        }
                    }
                }
                Ok(fallback.next())
            }
        }
    }
}

pub(super) enum Candidates<'a> {
    All(Range<usize>),
    Indexed {
        selected: Peekable<IntoIter<usize>>,
        fallback: Peekable<Copied<Iter<'a, usize>>>,
    },
}
impl Iterator for Candidates<'_> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(range) => range.next(),
            Self::Indexed { selected, fallback } => {
                if selected
                    .peek()
                    .is_some_and(|i| fallback.peek().is_none_or(|j| i < j))
                {
                    selected.next()
                } else {
                    fallback.next()
                }
            }
        }
    }
}
