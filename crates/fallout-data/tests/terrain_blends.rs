use fallout_data::{
    terrain::{self, AlphaVertex, Landscape, Layer},
    world::SourceField,
};

fn layer(
    kind: &str,
    quadrant: u8,
    index: i16,
    texture: u32,
    alpha: Option<Vec<(u16, f32)>>,
) -> Layer {
    Layer {
        kind: kind.into(),
        decoded_offset: 42,
        texture_raw: texture,
        quadrant,
        unused: 99,
        layer: index,
        alpha: alpha.map(|vertices| SourceField {
            decoded_offset: 56,
            value: vertices
                .into_iter()
                .map(|(position, opacity)| AlphaVertex {
                    position,
                    unused: [1, 2],
                    opacity_bits: opacity.to_bits(),
                })
                .collect(),
        }),
    }
}
fn build(layers: Vec<Layer>) -> terrain::blends::BlendMaps {
    terrain::blends::build(&Landscape {
        layers,
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn sparse_samples_and_binary32_quantization_preserve_quadrant_boundaries() {
    let maps = build(vec![
        layer("BTXT", 2, -1, 12, None),
        layer(
            "ATXT",
            2,
            0,
            13,
            Some(vec![(0, 0.5), (16, 1.), (272, 0.25), (288, 0.)]),
        ),
    ]);
    let q = &maps.quadrants[2];
    assert_eq!(q.overlays[0].weights[0], 127);
    assert_eq!(q.overlays[0].weights[16], 255);
    assert_eq!(q.overlays[0].weights[272], 63);
    assert_eq!(q.base.as_ref().unwrap().weights[0], 128);
    assert_eq!(q.base.as_ref().unwrap().weights[1], 255);
    assert_eq!(maps.missing_base_quadrants, [0, 1, 3]);
    assert_eq!(maps.clamped_samples, 0);
}

#[test]
fn missing_alpha_base_and_null_defaults_remain_distinct() {
    let maps = build(vec![
        layer("ATXT", 3, 0, 0, None),
        layer("ATXT", 3, 1, 11, Some(vec![])),
    ]);
    assert!(maps.quadrants[3].base.is_none());
    assert!(maps.quadrants[3].overlays[0].alpha_missing);
    assert!(!maps.quadrants[3].overlays[1].alpha_missing);
    assert_eq!(maps.unapplied_default_layers, 1);
    assert_eq!(maps.missing_base_quadrants, [0, 1, 2, 3]);
    assert!(
        maps.quadrants[3]
            .overlays
            .iter()
            .flat_map(|v| &v.weights)
            .all(|v| *v == 0)
    );
}

#[test]
fn saturation_and_clamping_are_visible_without_renormalizing_overlay_weights() {
    let maps = build(vec![
        layer("BTXT", 0, -1, 1, None),
        layer(
            "ATXT",
            0,
            0,
            2,
            Some(vec![(0, 0.75), (1, -10.), (2, f32::MAX)]),
        ),
        layer("ATXT", 0, 1, 3, Some(vec![(0, 0.75)])),
    ]);
    assert_eq!(maps.clamped_samples, 2);
    assert_eq!(maps.overfull_vertices, 1);
    let q = &maps.quadrants[0];
    assert_eq!(q.overlays[0].weights[0], 191);
    assert_eq!(q.overlays[1].weights[0], 191);
    assert_eq!(q.base.as_ref().unwrap().weights[0], 0);
    assert_eq!(q.base.as_ref().unwrap().weights[1], 255);
    assert_eq!(q.overlays[0].weights[2], 255);
}

#[test]
fn ambiguous_layers_bad_samples_and_budgets_refuse_interpretation() {
    let cases = vec![
        vec![layer("BTXT", 4, -1, 1, None)],
        vec![layer("BTXT", 0, -1, 1, None), layer("BTXT", 0, -1, 2, None)],
        vec![layer("ATXT", 0, 1, 1, None)],
        vec![layer("ATXT", 0, -1, 1, None)],
        vec![layer("ATXT", 0, 0, 1, Some(vec![(0, 1.), (0, 0.)]))],
        vec![layer("ATXT", 0, 0, 1, Some(vec![(289, 0.)]))],
        vec![layer("ATXT", 0, 0, 1, Some(vec![(0, f32::NAN)]))],
        vec![layer("WHAT", 0, 0, 1, None)],
        (0..257).map(|_| layer("BTXT", 0, -1, 1, None)).collect(),
        vec![layer("ATXT", 0, 0, 1, Some(vec![(0, 0.); 290]))],
    ];
    for layers in cases {
        assert!(
            terrain::blends::build(&Landscape {
                layers,
                ..Default::default()
            })
            .is_err()
        );
    }
}

#[test]
fn local_quadrant_triangles_partition_the_complete_source_surface() {
    let full = terrain::mesh::indices(0).unwrap();
    let mut remapped = Vec::new();
    for q in 0..4 {
        for triangle in terrain::blends::indices().as_chunks::<3>().0 {
            remapped.push(
                triangle
                    .iter()
                    .map(|v| {
                        terrain::blends::source_vertex(q, *v as usize % 17, *v as usize / 17)
                            .unwrap() as u32
                    })
                    .collect::<Vec<_>>(),
            );
        }
    }
    remapped.sort();
    let mut expected = full
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.to_vec())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(remapped, expected);
    assert_eq!(terrain::blends::source_vertex(3, 16, 16), Some(1088));
    assert_eq!(terrain::blends::source_vertex(4, 0, 0), None);
    assert_eq!(terrain::blends::source_vertex(0, 17, 0), None);
}
