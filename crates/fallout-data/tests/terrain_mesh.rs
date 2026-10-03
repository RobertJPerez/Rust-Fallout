//! Geometry fixtures check coverage and orientation independently of index order.
use fallout_data::{
    terrain::{CellFields, HeightMap, Landscape, heights::SAMPLE_COUNT, mesh},
    world::SourceField,
};

fn field<T>(value: T) -> SourceField<T> {
    SourceField {
        decoded_offset: 6,
        value,
    }
}
fn flat() -> Landscape {
    Landscape {
        heights: Some(field(HeightMap {
            offset_bits: 1.25f32.to_bits(),
            deltas: vec![0; SAMPLE_COUNT],
            unused: [3, 5, 7],
        })),
        ..Default::default()
    }
}

#[test]
fn every_triangle_faces_up_and_covers_its_quad_once() {
    for hidden in 0u8..16 {
        let mesh = mesh::build(&flat(), hidden).unwrap();
        assert_eq!(mesh.local_positions.len(), 1089);
        assert_eq!(
            mesh.indices.len(),
            (4 - hidden.count_ones() as usize) * 256 * 6
        );
        let mut areas = [[0.; 32]; 32];
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = t.map(|i| mesh.local_positions[i as usize]);
            let cross = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
            assert_eq!(cross, 128. * 128.);
            let x = (p.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min) / 128.) as usize;
            let y = (p.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min) / 128.) as usize;
            areas[y][x] += cross / (2. * 128. * 128.);
        }
        for (y, row) in areas.iter().enumerate() {
            for (x, area) in row.iter().enumerate() {
                let quadrant = usize::from(x >= 16) + 2 * usize::from(y >= 16);
                assert_eq!(
                    *area,
                    if hidden & (1 << quadrant) == 0 {
                        1.
                    } else {
                        0.
                    }
                );
            }
        }
    }
}

#[test]
fn adjacent_quads_alternate_diagonal_without_reordering_vertices() {
    let indices = mesh::indices(0).unwrap();
    assert_eq!(&indices[..12], &[0, 1, 34, 0, 34, 33, 1, 2, 34, 2, 35, 34]);
    assert_eq!(&indices[32 * 6..32 * 6 + 6], &[33, 34, 66, 34, 67, 66]);
}

#[test]
fn signed_authored_normals_are_normalized_without_repair() {
    for raw in [[128, 1, 127], [255, 0, 0], [0, 0, 127], [17, 39, 220]] {
        let actual = mesh::normal_bits(raw).unwrap().map(f32::from_bits);
        let length = actual.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.).abs() < 1e-6);
        for (value, byte) in actual.into_iter().zip(raw) {
            assert_eq!(value.is_sign_negative(), (byte as i8) < 0);
        }
    }
    assert!(mesh::normal_bits([0; 3]).is_err());
}

#[test]
fn absent_fields_stay_absent_and_bounds_keep_source_heights() {
    let surface = mesh::build(&flat(), 15).unwrap();
    assert!(surface.normal_bits.is_none() && surface.colors.is_none());
    assert_eq!(surface.bounds, [[0., 0., 10.], [4096., 4096., 10.]]);
    assert!(surface.indices.is_empty());
}

#[test]
fn malformed_or_missing_inputs_never_manufacture_a_surface() {
    assert!(mesh::build(&Landscape::default(), 0).is_err());
    for flags in [16, 128, 255] {
        assert!(mesh::build(&flat(), flags).is_err());
    }
    for count in [0, 1088, 1090] {
        let mut land = flat();
        land.normals = Some(field(vec![[0, 0, 127]; count]));
        assert!(mesh::build(&land, 0).is_err());
        land.normals = None;
        land.colors = Some(field(vec![[1, 2, 3]; count]));
        assert!(mesh::build(&land, 0).is_err());
    }
    let mut land = flat();
    land.heights.as_mut().unwrap().value.offset_bits = f32::MAX.to_bits();
    assert!(mesh::build(&land, 0).is_err());
}

#[test]
fn xclc_padding_is_preserved_but_never_interpreted_as_hide_flags() {
    let mut cell = CellFields::default();
    assert_eq!(cell.land_flags(), None);
    cell.quadrant_flags = Some(field(u32::from_le_bytes([5, 255, 128, 61])));
    assert_eq!(cell.land_flags(), Some(5));
    assert_eq!(
        cell.quadrant_flags.unwrap().value.to_le_bytes(),
        [5, 255, 128, 61]
    );
}
