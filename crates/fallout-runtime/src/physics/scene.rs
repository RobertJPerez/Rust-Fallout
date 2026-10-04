use super::{
    index::{Bounds, Candidates, Index},
    math::*,
    shape::Shape,
    *,
};
use fallout_data::{
    coordinates::Affine,
    nif_collision::{Collision, Data},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug)]
struct Geometry {
    shape: Shape,
    triangle: Option<usize>,
    material: u32,
    filter: Option<SourceFilter>,
    welding: Option<u16>,
    shell: f32,
}
#[derive(Debug)]
struct Leaf {
    geometry: Arc<Geometry>,
    transform: Similarity,
    source: SourceId,
    body_filter: SourceFilter,
    shell: f32,
}

/// Units are immutable after construction, including the validated tolerance.
/// ```compile_fail
/// use fallout_runtime::physics::StaticScene;
/// fn bypass(scene: &mut StaticScene) { scene.units.transform_tolerance = 10.; }
/// ```
#[derive(Debug)]
pub struct StaticScene {
    leaves: Vec<Leaf>,
    units: EngineeringUnits,
    index: Option<Index>,
}

fn unsupported(block: u32, reason: &'static str) -> QueryError {
    QueryError::Unsupported { block, reason }
}
fn radius(block: u32, value: f32) -> QueryResult<f64> {
    if value.is_finite() && value >= 0. {
        Ok(f64::from(value))
    } else {
        Err(unsupported(block, "negative or nonfinite radius"))
    }
}
fn vector(block: u32, v: [f32; 3]) -> QueryResult<V> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(v.map(f64::from))
    } else {
        Err(unsupported(block, "nonfinite geometry"))
    }
}
fn charge(left: &mut usize, count: usize, name: &'static str) -> QueryResult<()> {
    *left = left.checked_sub(count).ok_or(QueryError::Budget(name))?;
    Ok(())
}

/// This bounded family is the actual convex hull of all eight authored corners,
/// not a bounds approximation of arbitrary collision vertices or visual meshes.
fn convex_cuboid(block: u32, vertices: &[[f32; 4]], planes: &[[f32; 4]]) -> QueryResult<Shape> {
    let refused = || {
        unsupported(
            block,
            "convex source is not a certified eight-corner cuboid",
        )
    };
    if vertices.len() != 8
        || planes.len() != 6
        || vertices
            .iter()
            .any(|v| v[3] != 0. || v.iter().any(|x| !x.is_finite()))
        || planes.iter().any(|p| p.iter().any(|x| !x.is_finite()))
    {
        return Err(refused());
    }
    let minimum: [f32; 3] =
        std::array::from_fn(|i| vertices.iter().map(|v| v[i]).fold(f32::INFINITY, f32::min));
    let maximum: [f32; 3] = std::array::from_fn(|i| {
        vertices
            .iter()
            .map(|v| v[i])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    if (0..3).any(|i| minimum[i] >= maximum[i]) {
        return Err(refused());
    }
    let mut corners = 0u8;
    for vertex in vertices {
        let mut corner = 0;
        for i in 0..3 {
            if vertex[i] == maximum[i] {
                corner |= 1 << i;
            } else if vertex[i] != minimum[i] {
                return Err(refused());
            }
        }
        let bit = 1 << corner;
        if corners & bit != 0 {
            return Err(refused());
        }
        corners |= bit;
    }
    let mut faces = 0u8;
    for plane in planes {
        if plane[..3].iter().filter(|v| **v != 0.).count() != 1 {
            return Err(refused());
        }
        let axis = plane[..3]
            .iter()
            .position(|v| *v != 0.)
            .expect("one plane axis");
        let positive = plane[axis] > 0.;
        let boundary = if positive {
            maximum[axis]
        } else {
            minimum[axis]
        };
        // Exact source-f32 product relation allows authored normal drift while
        // keeping raw planes. Query geometry is explicitly the vertex hull.
        if plane[3] != -(plane[axis] * boundary) {
            return Err(refused());
        }
        let bit = 1 << (2 * axis + usize::from(positive));
        if faces & bit != 0 {
            return Err(refused());
        }
        faces |= bit;
    }
    Ok(Shape::ConvexCuboid {
        minimum: minimum.map(f64::from),
        maximum: maximum.map(f64::from),
        _source_vertices: Box::new(vertices.try_into().expect("eight checked source vertices")),
        _source_planes: Box::new(planes.try_into().expect("six checked source planes")),
    })
}
fn geometry(
    block: u32,
    data: &Data,
    elements: &mut usize,
    primitives: usize,
) -> QueryResult<Vec<Arc<Geometry>>> {
    let (shape, material, shell) = match data {
        Data::Sphere {
            radius: r,
            material,
        } => (Shape::Sphere(radius(block, *r)?), *material, 0.),
        Data::Box {
            half_extents,
            material,
            radius: r,
            ..
        } => {
            radius(block, *r)?;
            let v = vector(block, *half_extents)?;
            if v.iter().any(|x| *x <= 0.) {
                return Err(unsupported(block, "degenerate box"));
            }
            (Shape::Box(v), *material, *r)
        }
        Data::Capsule {
            first,
            second,
            first_radius,
            second_radius,
            radius: r,
            material,
            ..
        } => {
            if r != first_radius || r != second_radius {
                return Err(unsupported(block, "unequal capsule radii"));
            }
            (
                Shape::Capsule {
                    a: vector(block, *first)?,
                    b: vector(block, *second)?,
                    radius: radius(block, *r)?,
                },
                *material,
                0.,
            )
        }
        Data::PackedData {
            triangles,
            vertices,
            vertex_count,
            compressed,
            subparts,
            ..
        } => {
            if *compressed {
                return Err(unsupported(block, "uncertified packed vertex compression"));
            }
            if vertices.len() != *vertex_count as usize {
                return Err(unsupported(block, "packed vertex count differs"));
            }
            if triangles.len() > primitives {
                return Err(QueryError::Budget("packed primitives"));
            }
            charge(elements, vertices.len(), "geometry elements")?;
            charge(elements, triangles.len(), "geometry elements")?;
            charge(elements, subparts.len(), "geometry elements")?;
            let mut owners = Vec::new();
            let mut end = 0usize;
            for (part_id, part) in subparts.iter().enumerate() {
                end = end
                    .checked_add(part.vertices as usize)
                    .ok_or(unsupported(block, "subpart range overflow"))?;
                if end > vertices.len() {
                    return Err(unsupported(block, "subpart vertex range outside data"));
                }
                owners.resize(end, part_id);
            }
            if end != vertices.len() {
                return Err(unsupported(block, "missing packed subpart metadata"));
            }
            let mut result = Vec::new();
            for (id, triangle) in triangles.iter().enumerate() {
                let indices = triangle.indices.map(usize::from);
                let owner = *owners
                    .get(indices[0])
                    .ok_or(unsupported(block, "triangle index outside data"))?;
                if indices.iter().any(|i| owners.get(*i) != Some(&owner)) {
                    return Err(unsupported(block, "triangle crosses subparts"));
                }
                let part = &subparts[owner];
                let mut points = [[0.; 3]; 3];
                for i in 0..3 {
                    points[i] = vector(
                        block,
                        *vertices
                            .get(indices[i])
                            .ok_or(unsupported(block, "triangle index outside data"))?,
                    )?;
                }
                result.push(Arc::new(Geometry {
                    shape: Shape::Triangle(points),
                    triangle: Some(id),
                    material: part.material,
                    filter: Some((&part.filter).into()),
                    welding: Some(triangle.welding),
                    shell: 0.,
                }));
            }
            return Ok(result);
        }
        Data::ConvexVertices {
            vertices,
            planes,
            radius: r,
            material,
            ..
        } => {
            radius(block, *r)?;
            charge(elements, vertices.len(), "convex source elements")?;
            charge(elements, planes.len(), "convex source elements")?;
            (convex_cuboid(block, vertices, planes)?, *material, *r)
        }
        _ => {
            return Err(unsupported(
                block,
                "block is not a supported query primitive",
            ));
        }
    };
    charge(elements, 1, "geometry elements")?;
    Ok(vec![Arc::new(Geometry {
        shape,
        triangle: None,
        material,
        filter: None,
        welding: None,
        shell,
    })])
}

impl StaticScene {
    pub fn units(&self) -> EngineeringUnits {
        self.units
    }
    /// Build atomically: any unsupported reachable shape or exceeded budget returns
    /// an error, never a successful incomplete body. Shared DAG geometry is cached;
    /// occurrence expansion is iterative and separately bounded.
    pub fn build(
        collision: &Collision,
        placements: &[BodyPlacement],
        units: EngineeringUnits,
        limits: QueryLimits,
    ) -> QueryResult<Self> {
        if !units.havok_to_source.is_finite()
            || units.havok_to_source <= 0.
            || !units.source_to_query.is_finite()
            || units.source_to_query <= 0.
            || !units.transform_tolerance.is_finite()
            || !(0. ..=1e-3).contains(&units.transform_tolerance)
        {
            return Err(QueryError::Invalid(
                "explicit engineering units/tolerance required",
            ));
        }
        if placements.is_empty() {
            return Err(QueryError::Invalid(
                "at least one selected body is required",
            ));
        }
        if collision.blocks.len() > limits.blocks || placements.len() > limits.shape_visits {
            return Err(QueryError::Budget("block/placement count"));
        }
        let mut blocks = BTreeMap::new();
        for block in &collision.blocks {
            if blocks.insert(block.block, block).is_some() {
                return Err(QueryError::Invalid("duplicate collision block"));
            }
        }
        let mut scene = Self {
            leaves: Vec::new(),
            units,
            index: None,
        };
        let mut cached = BTreeMap::<u32, Vec<Arc<Geometry>>>::new();
        let mut visits = limits.shape_visits;
        let mut elements = limits.geometry_elements;
        let mut placement_ids = BTreeSet::new();
        for placement in placements {
            let start_count = scene.leaves.len();
            if !placement_ids.insert((
                placement.reference,
                placement.source_sha256,
                placement.body_block,
            )) {
                return Err(QueryError::Invalid("duplicate body placement identity"));
            }
            let root = blocks
                .get(&placement.body_block)
                .ok_or(unsupported(placement.body_block, "missing body"))?;
            let Data::RigidBody { body } = &root.data else {
                return Err(unsupported(
                    root.block,
                    "placement does not select rigid body",
                ));
            };
            let shape = body
                .world
                .shape
                .ok_or(unsupported(root.block, "body has no shape"))?;
            let mut pose = identity();
            if body.transform_active {
                let [x, y, z, w] = body.rotation.map(f64::from);
                let norm = x * x + y * y + z * z + w * w;
                if !norm.is_finite() || (norm - 1.).abs() > units.transform_tolerance {
                    return Err(unsupported(root.block, "nonunit body quaternion"));
                }
                pose.rows = [
                    [
                        1. - 2. * (y * y + z * z),
                        2. * (x * y - z * w),
                        2. * (x * z + y * w),
                        f64::from(body.translation[0]),
                    ],
                    [
                        2. * (x * y + z * w),
                        1. - 2. * (x * x + z * z),
                        2. * (y * z - x * w),
                        f64::from(body.translation[1]),
                    ],
                    [
                        2. * (x * z - y * w),
                        2. * (y * z + x * w),
                        1. - 2. * (x * x + y * y),
                        f64::from(body.translation[2]),
                    ],
                ];
            }
            let mut havok = identity();
            let mut output = identity();
            for i in 0..3 {
                havok.rows[i][i] = units.havok_to_source;
                output.rows[i][i] = units.source_to_query;
            }
            let frame = compose(
                output,
                compose(placement.attachment_to_source, compose(havok, pose)),
            );
            let mut stack = vec![(shape, frame, 0f32)];
            let mut occurrence = 0;
            while let Some((id, frame, shell)) = stack.pop() {
                charge(
                    &mut visits,
                    1,
                    "shape visits (including cycles/shared paths)",
                )?;
                let block = blocks
                    .get(&id)
                    .ok_or(unsupported(id, "shape target was not decoded"))?;
                match &block.data {
                    Data::Transform { shape, matrix, .. } => {
                        if [matrix[0][3], matrix[1][3], matrix[2][3], matrix[3][3]]
                            != [0., 0., 0., 1.]
                        {
                            return Err(unsupported(id, "projective shape matrix"));
                        }
                        // Matrix44 file vectors are columns, unlike the decoder's
                        // stored triples for NiTransform rotations.
                        let local = Affine {
                            rows: std::array::from_fn(|i| {
                                std::array::from_fn(|j| f64::from(matrix[j][i]))
                            }),
                        };
                        stack.push((
                            shape.ok_or(unsupported(id, "null transformed shape"))?,
                            compose(frame, local),
                            shell,
                        ));
                    }
                    Data::List {
                        shapes, filters, ..
                    } => {
                        if filters
                            .iter()
                            .any(|f| f.layer != 0 || f.flags_and_parts != 0 || f.group != 0)
                        {
                            return Err(unsupported(id, "nonzero list filter override"));
                        }
                        if stack
                            .len()
                            .checked_add(shapes.len())
                            .is_none_or(|count| count > visits)
                        {
                            return Err(QueryError::Budget("pending shape visits"));
                        }
                        for shape in shapes.iter().rev() {
                            stack.push((
                                shape.ok_or(unsupported(id, "null list child"))?,
                                frame,
                                shell,
                            ));
                        }
                    }
                    Data::Mopp { shape, scale, .. } => {
                        if *scale != 1. {
                            return Err(unsupported(
                                id,
                                "nonunit MOPP wrapper scale is unverified",
                            ));
                        }
                        // MOPP is acceleration metadata. Query the exact child
                        // geometry exhaustively without interpreting MOPP bytes.
                        stack.push((
                            shape.ok_or(unsupported(id, "null MOPP child"))?,
                            frame,
                            shell,
                        ));
                    }
                    Data::PackedShape {
                        data,
                        scale,
                        scale_copy,
                        radius: r,
                        radius_copy,
                        ..
                    } => {
                        if scale != scale_copy || r != radius_copy {
                            return Err(unsupported(id, "packed scale/radius copies disagree"));
                        }
                        radius(id, *r)?;
                        let mut local = identity();
                        for (i, value) in scale.iter().take(3).enumerate() {
                            local.rows[i][i] = f64::from(*value);
                        }
                        stack.push((
                            data.ok_or(unsupported(id, "null packed data"))?,
                            compose(frame, local),
                            *r,
                        ));
                    }
                    data => {
                        if let std::collections::btree_map::Entry::Vacant(entry) = cached.entry(id)
                        {
                            entry.insert(geometry(id, data, &mut elements, limits.primitives)?);
                        }
                        let values = &cached[&id];
                        if values.len() > limits.primitives.saturating_sub(scene.leaves.len()) {
                            return Err(QueryError::Budget("query primitives"));
                        }
                        for value in values {
                            let transform = Similarity::new(frame, units.transform_tolerance)?;
                            scene.leaves.push(Leaf {
                                geometry: Arc::clone(value),
                                transform,
                                source: SourceId {
                                    reference: placement.reference,
                                    source_sha256: placement.source_sha256,
                                    body_block: placement.body_block,
                                    shape_block: id,
                                    occurrence,
                                    triangle: value.triangle,
                                },
                                body_filter: (&body.world.filter).into(),
                                shell: if shell != 0. { shell } else { value.shell },
                            });
                        }
                        occurrence += 1;
                    }
                }
            }
            if scene.leaves.len() == start_count {
                return Err(unsupported(
                    root.block,
                    "selected body has no query geometry",
                ));
            }
        }
        let mut indexed = Vec::new();
        let mut fallback = Vec::new();
        for (ordinal, leaf) in scene.leaves.iter().enumerate() {
            let (minimum, maximum) = leaf.geometry.shape.bounds();
            if let Some(bounds) = Bounds::transformed(minimum, maximum, &leaf.transform) {
                indexed.push((ordinal, bounds));
            } else {
                fallback.push(ordinal);
            }
        }
        scene.index = Index::build(indexed, fallback, &mut elements)?;
        Ok(scene)
    }
    pub fn primitive_count(&self) -> usize {
        self.leaves.len()
    }
    /// Shell margins, runtime filters, activation and dynamics remain unavailable.
    pub fn faithful_ready(&self) -> bool {
        false
    }
    fn hit(leaf: &Leaf, distance: f64, position: V) -> Hit {
        let g = &leaf.geometry;
        Hit {
            source: leaf.source.clone(),
            distance,
            position,
            material: g.material,
            body_filter: leaf.body_filter,
            shape_filter: g.filter,
            welding: g.welding,
            authored_shell_radius: leaf.shell,
        }
    }
    pub fn ray_cast(&self, ray: Ray, mut budget: QueryBudget) -> QueryResult<Vec<Hit>> {
        if !query_domain(ray.origin)
            || !query_domain(ray.direction)
            || !ray.max_distance.is_finite()
            || !(0. ..=1e50).contains(&ray.max_distance)
            || (dot(ray.direction, ray.direction) - 1.).abs() > self.units.transform_tolerance
        {
            return Err(QueryError::Invalid(
                "finite unit ray and bounded distance required",
            ));
        }
        let mut hits = Vec::new();
        let candidates = match &self.index {
            Some(index) => index.ray(ray, budget.primitive_tests)?,
            None => Candidates::All(0..self.leaves.len()),
        };
        for ordinal in candidates {
            let leaf = &self.leaves[ordinal];
            charge(&mut budget.primitive_tests, 1, "primitive tests")?;
            charge(
                &mut budget.geometry_tests,
                leaf.geometry.shape.cost(),
                "geometry tests",
            )?;
            let o = leaf.transform.local_point(ray.origin);
            let d = leaf.transform.local_vector(ray.direction);
            if let Some(distance) = leaf.geometry.shape.ray(o, d, ray.max_distance)? {
                if !distance.is_finite() {
                    return Err(QueryError::Invalid("overflowing intersection"));
                }
                if distance <= ray.max_distance {
                    charge(&mut budget.hits, 1, "query hits")?;
                    let position =
                        std::array::from_fn(|i| ray.direction[i].mul_add(distance, ray.origin[i]));
                    if !finite(position) {
                        return Err(QueryError::Invalid("overflowing hit position"));
                    }
                    hits.push(Self::hit(leaf, distance, position));
                }
            }
        }
        hits.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.source.cmp(&b.source))
        });
        Ok(hits)
    }
    /// Closed sphere overlap, including triangle edges and degenerate triangles.
    /// An error discards all partial hits when any declared budget is exhausted.
    pub fn overlap_sphere(
        &self,
        center: V,
        radius: f64,
        mut budget: QueryBudget,
    ) -> QueryResult<Vec<Hit>> {
        if !query_domain(center) || !radius.is_finite() || !(0. ..=1e50).contains(&radius) {
            return Err(QueryError::Invalid(
                "finite sphere with nonnegative bounded radius required",
            ));
        }
        let mut hits = Vec::new();
        let candidates = match &self.index {
            Some(index) => index.overlap(center, radius, budget.primitive_tests)?,
            None => Candidates::All(0..self.leaves.len()),
        };
        for ordinal in candidates {
            let leaf = &self.leaves[ordinal];
            charge(&mut budget.primitive_tests, 1, "primitive tests")?;
            charge(
                &mut budget.geometry_tests,
                leaf.geometry.shape.cost(),
                "geometry tests",
            )?;
            let local = leaf.transform.local_point(center);
            let r = radius / leaf.transform.scale;
            if !query_domain(local) || !r.is_finite() || r > 1e50 {
                return Err(QueryError::Invalid("overflowing local sphere"));
            }
            if leaf.geometry.shape.overlap(local, r)? {
                charge(&mut budget.hits, 1, "query hits")?;
                hits.push(Self::hit(leaf, 0., center));
            }
        }
        Ok(hits)
    }
}
