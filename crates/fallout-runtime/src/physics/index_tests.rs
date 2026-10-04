use super::*;
use fallout_data::coordinates::Affine;
use serde_json::json;

fn words(v: V) -> [String; 3] {
    v.map(|x| format!("{:016x}", x.to_bits()))
}

#[test]
fn transformed_enclosures_keep_boundary_candidates_and_export_literal_word_oracle() {
    let frames = [
        [[1., 0., 0., 5.], [0., 1., 0., -7.], [0., 0., 1., 11.]],
        [[-1., 0., 0., 5.], [0., 1., 0., -7.], [0., 0., 1., 11.]],
        [[0., -1., 0., 5.], [1., 0., 0., -7.], [0., 0., 1., 11.]],
        [[0.6, -0.8, 0., 5.], [0.8, 0.6, 0., -7.], [0., 0., 1., 11.]],
        [
            [f64::from(0.6f32), -f64::from(0.8f32), 0., 5.],
            [f64::from(0.8f32), f64::from(0.6f32), 0., -7.],
            [0., 0., 1., 11.],
        ],
        [[1., 1e-7, 0., 5.], [0., 1., 0., -7.], [0., 0., 1., 11.]],
        [[2., 0., 0., 5.], [0., 2., 0., -7.], [0., 0., 2., 11.]],
        [[0.5, 0., 0., 5.], [0., 0.5, 0., -7.], [0., 0., 0.5, 11.]],
        [[-2., 0., 0., 5.], [0., -2., 0., -7.], [0., 0., -2., 11.]],
        [
            [0.6, -0.8, 0., 1e15],
            [0.8, 0.6, 0., -1e15],
            [0., 0., 1., 1e15],
        ],
        [
            [1e-50, 0., 0., 0.],
            [0., 1e-50, 0., 0.],
            [0., 0., 1e-50, 0.],
        ],
        [[1e50, 0., 0., 0.], [0., 1e50, 0., 0.], [0., 0., 1e50, 0.]],
        [
            [1., 0., 0., f64::from_bits(1)],
            [0., 1., 0., -f64::from_bits(1)],
            [0., 0., 1., f64::from_bits(7)],
        ],
        [[1., 0., 0., 1e50], [0., 1., 0., -1e50], [0., 0., 1., 1e50]],
    ];
    let minimum = [-1., -2., -3.];
    let maximum = [1., 2., 3.];
    let mut evidence = Vec::new();
    for rows in frames {
        let authored = Affine { rows };
        let transform = Similarity::new(authored, 1e-3).unwrap();
        let bounds = Bounds::transformed(minimum, maximum, &transform).unwrap();
        let inverse = transform.inverse_rows();
        let linear = std::array::from_fn(|i| std::array::from_fn(|j| inverse[i][j]));
        let forward = enclosure::inverse(linear).unwrap();
        let mut centers = vec![
            [0.; 3],
            [10., -10., 10.],
            [1e15, -1e15, 1e15],
            [1e50, -1e50, 1e50],
            [f64::from_bits(1), -f64::from_bits(1), f64::from_bits(7)],
            authored.point([0.; 3]),
            authored.point([1.000_000_1, 0., 0.]),
        ];
        for corner in 0..8 {
            centers.push(authored.point(std::array::from_fn(|i| {
                if corner & (1 << i) == 0 {
                    minimum[i]
                } else {
                    maximum[i]
                }
            })));
        }
        let mut rays = Vec::new();
        let mut overlaps = Vec::new();
        for center in centers {
            let local_center = transform.local_point(center);
            if (0..3).all(|i| minimum[i] <= local_center[i] && local_center[i] <= maximum[i]) {
                assert!(bounds.may_overlap(center, 0.));
                assert!(bounds.may_ray(Ray {
                    origin: center,
                    direction: [1., 0., 0.],
                    max_distance: 0.
                }));
            }
            for direction in [
                [1., 0., 0.],
                [-1., 0., 0.],
                [0., 1., 0.],
                [0.6, 0.8, 0.],
                [1., f64::from_bits(1), 0.],
            ] {
                for max_distance in [0., 1., 1e15, 1e50] {
                    let ray = Ray {
                        origin: center,
                        direction,
                        max_distance,
                    };
                    rays.push(json!({
                        "origin": words(center), "direction": words(direction), "maximum": format!("{:016x}", max_distance.to_bits()),
                        "local_origin": words(local_center), "local_direction": words(transform.local_vector(direction)),
                        "padding": words(bounds.ray_padding(ray)), "may_ray": bounds.may_ray(ray),
                    }));
                }
            }
            for radius in [0., f64::from_bits(1), 0.5, 2., 1e15] {
                overlaps.push(json!({
                    "center": words(center), "radius": format!("{:016x}", radius.to_bits()),
                    "local_center": words(local_center), "local_radius": format!("{:016x}", (radius / transform.scale).to_bits()),
                    "padding": words(bounds.overlap_padding(center, radius)), "may_overlap": bounds.may_overlap(center, radius),
                }));
            }
        }
        evidence.push(json!({
            "authored": rows.map(|row| row.map(|x| format!("{:016x}", x.to_bits()))),
            "inverse": inverse.map(|row| row.map(|x| format!("{:016x}", x.to_bits()))),
            "forward_interval": forward.map(|row| row.map(|x| [format!("{:016x}", x.lower.to_bits()), format!("{:016x}", x.upper.to_bits())])),
            "source_minimum": words(minimum), "source_maximum": words(maximum),
            "minimum": words(bounds.minimum), "maximum": words(bounds.maximum),
            "scale": format!("{:016x}", transform.scale.to_bits()),
            "rays": rays, "overlaps": overlaps,
        }));
    }
    // Optional private evidence uses actual production projections/padding, so
    // a Fraction oracle never has to imitate Rust's floating point evaluation.
    if let Some(path) = std::env::var_os("FALLOUT_PHYSICS_ENVELOPE_EVIDENCE") {
        std::fs::write(path, serde_json::to_vec(&evidence).unwrap()).unwrap();
    }
}

#[test]
fn uncertifiable_enclosures_and_query_overflow_keep_exhaustive_candidates() {
    assert!(enclosure::inverse([[0.; 3]; 3]).is_none());
    assert!(
        enclosure::inverse([[f64::MAX, 0., 0.], [0., f64::MAX, 0.], [0., 0., f64::MAX]]).is_none()
    );
    let transform = Similarity::new(
        Affine {
            rows: [[1., 0., 0., 5.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        },
        1e-3,
    )
    .unwrap();
    assert!(Bounds::transformed([-f64::MAX; 3], [f64::MAX; 3], &transform).is_none());
    let bounds = Bounds::transformed([-1.; 3], [1.; 3], &transform).unwrap();
    assert!(bounds.may_ray(Ray {
        origin: [f64::MAX; 3],
        direction: [f64::MAX; 3],
        max_distance: f64::MAX
    }));
    assert!(bounds.may_overlap([f64::MAX; 3], f64::MAX));
}
