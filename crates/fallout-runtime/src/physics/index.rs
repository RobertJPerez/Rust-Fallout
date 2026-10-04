//! Conservative culling of identity-frame source primitives. Every accepted
//! candidate still reaches the original shape predicate; other frames fall back.
use super::{QueryError, QueryResult, Ray, math::V, shape::cuboid_may_ray};
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
}
impl Bounds {
    pub fn new(minimum: V, maximum: V) -> Self {
        Self {
            minimum: minimum.map(f64::next_down),
            maximum: maximum.map(f64::next_up),
        }
    }
    fn union(self, other: Self) -> Self {
        Self {
            minimum: std::array::from_fn(|i| self.minimum[i].min(other.minimum[i])),
            maximum: std::array::from_fn(|i| self.maximum[i].max(other.maximum[i])),
        }
    }
    fn center(self, axis: usize) -> f64 {
        0.5 * self.minimum[axis] + 0.5 * self.maximum[axis]
    }
    fn may_ray(self, ray: Ray) -> bool {
        cuboid_may_ray(
            ray.origin,
            ray.direction,
            self.minimum,
            self.maximum,
            ray.max_distance,
        )
    }
    fn may_overlap(self, center: V, radius: f64) -> bool {
        (0..3).all(|i| {
            let minimum = (self.minimum[i] - radius).next_down();
            let maximum = (self.maximum[i] + radius).next_up();
            // Nonfinite bounds cannot certify exclusion.
            !minimum.is_finite()
                || !maximum.is_finite()
                || (minimum <= center[i] && center[i] <= maximum)
        })
    }
}

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
