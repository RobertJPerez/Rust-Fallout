//! These fixtures are authored here; no retail model bytes are checked in.
use fallout_data::nif_scene::{self, ObjectKind, Topology};

fn u16s(b: &mut Vec<u8>, values: &[u16]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
fn u32s(b: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
fn floats(b: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
const NULL: u32 = u32::MAX;

fn net() -> Vec<u8> {
    let mut b = Vec::new();
    u32s(&mut b, &[0, 0, NULL]);
    b
}

fn shader_header() -> Vec<u8> {
    let mut b = net();
    u16s(&mut b, &[1]);
    u32s(&mut b, &[1, 0x82000000, 0x21]);
    floats(&mut b, &[0.75]);
    u32s(&mut b, &[3]);
    b
}

fn texture_set(paths: &[&[u8]]) -> Vec<u8> {
    let mut b = Vec::new();
    u32s(&mut b, &[paths.len() as u32]);
    for path in paths {
        u32s(&mut b, &[path.len() as u32]);
        b.extend(*path);
    }
    b
}

fn container(blocks: &[(&str, Vec<u8>)], roots: &[u32], stream: u32) -> Vec<u8> {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    u32s(&mut b, &[0x1402_0007]);
    b.push(1);
    u32s(&mut b, &[11, blocks.len() as u32, stream]);
    b.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    u16s(&mut b, &[types.len() as u16]);
    for name in &types {
        u32s(&mut b, &[name.len() as u32]);
        b.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        u16s(
            &mut b,
            &[types.iter().position(|n| n == name).unwrap() as u16],
        );
    }
    for (_, payload) in blocks {
        u32s(&mut b, &[payload.len() as u32]);
    }
    u32s(&mut b, &[1, 4, 4]);
    b.extend(b"name");
    u32s(&mut b, &[0]);
    for (_, payload) in blocks {
        b.extend(payload);
    }
    u32s(&mut b, &[roots.len() as u32]);
    u32s(&mut b, roots);
    b
}

fn av(stream: u32) -> Vec<u8> {
    let mut b = Vec::new();
    u32s(&mut b, &[0, 0, NULL]); // name, empty extras, no controller
    if stream <= 26 {
        u16s(&mut b, &[14]);
    } else {
        u32s(&mut b, &[14]);
    }
    floats(
        &mut b,
        &[0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.],
    );
    u32s(&mut b, &[0, NULL]);
    b
}
fn node(children: &[u32], stream: u32) -> Vec<u8> {
    let mut b = av(stream);
    u32s(&mut b, &[children.len() as u32]);
    u32s(&mut b, children);
    u32s(&mut b, &[0]);
    b
}
fn shape(data: u32, stream: u32) -> Vec<u8> {
    let mut b = av(stream);
    u32s(&mut b, &[data, NULL, 0, NULL]);
    b.push(0);
    b
}
fn geometry() -> Vec<u8> {
    let mut b = Vec::new();
    u32s(&mut b, &[0]);
    u16s(&mut b, &[4]);
    b.extend([0, 0, 1]);
    floats(&mut b, &[0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.]);
    u16s(&mut b, &[0x1001]);
    b.push(1);
    for vector in [[0., 0., 1.], [1., 0., 0.], [0., 1., 0.]] {
        for _ in 0..4 {
            floats(&mut b, &vector);
        }
    }
    floats(&mut b, &[0.5, 0.5, 0., 1.]);
    b.push(1);
    for _ in 0..4 {
        floats(&mut b, &[1., 0.5, 0.25, 1.]);
    }
    floats(&mut b, &[0., 0., 1., 0., 0., 1., 1., 1.]);
    u16s(&mut b, &[0x4000]);
    u32s(&mut b, &[NULL]);
    b
}
fn strips(indices: &[u16]) -> Vec<u8> {
    let mut b = geometry();
    u16s(
        &mut b,
        &[
            indices.len().saturating_sub(2) as u16,
            1,
            indices.len() as u16,
        ],
    );
    b.push(1);
    u16s(&mut b, indices);
    b
}

#[test]
fn strips_keep_winding_through_degenerate_connectors_and_restart_each_strip() {
    let mut b = geometry();
    u16s(&mut b, &[7, 2, 7, 4]);
    b.push(1);
    u16s(&mut b, &[0, 1, 2, 2, 1, 3, 0, 0, 1, 2, 3]);
    let (_, scene) = nif_scene::decode(
        &container(&[("NiTriStripsData", b)], &[], 34),
        "strip fixture",
    )
    .unwrap();
    let m = &scene.meshes[0];
    assert_eq!(
        m.triangles,
        [[0, 1, 2], [2, 3, 1], [1, 3, 0], [0, 1, 2], [1, 3, 2]]
    );
    assert_eq!(m.strip_degenerate_triangles, 2);
    assert!(m.source_triangle_count_matches);
    assert_eq!(m.vertices[3], [1., 1., 0.]);
    assert_eq!(m.normals, [[0., 0., 1.]; 4]);
    assert_eq!(m.tangents, [[1., 0., 0.]; 4]);
    assert_eq!(m.bitangents, [[0., 1., 0.]; 4]);
    assert_eq!(m.colors[0], [1., 0.5, 0.25, 1.]);
    assert_eq!(m.uv_sets[0][3], [1., 1.]);
}

#[test]
fn triangle_lists_and_match_groups_are_preserved() {
    let mut b = geometry();
    u16s(&mut b, &[2]);
    u32s(&mut b, &[6]);
    b.push(1);
    u16s(&mut b, &[0, 1, 2, 1, 3, 2, 1, 2, 0, 1]);
    let blocks = [("NiTriShape", shape(1, 34)), ("NiTriShapeData", b)];
    let (_, scene) = nif_scene::decode(&container(&blocks, &[0], 34), "triangle fixture").unwrap();
    assert_eq!(scene.meshes[0].triangles, [[0, 1, 2], [1, 3, 2]]);
    assert!(
        matches!(&scene.meshes[0].topology, Topology::Triangles { match_groups, .. } if match_groups == &vec![vec![0, 1]])
    );
    assert!(matches!(
        &scene.objects[0].kind,
        ObjectKind::Mesh { data: Some(1), .. }
    ));
}

#[test]
fn hierarchy_composes_rotation_scale_and_translation_in_parent_order() {
    let mut parent = node(&[1, NULL], 34);
    // Parent: translate (10,20,30), rotate +90 around Z, scale 2.
    let mut encoded = Vec::new();
    floats(
        &mut encoded,
        &[10., 20., 30., 0., -1., 0., 1., 0., 0., 0., 0., 1., 2.],
    );
    parent[16..68].copy_from_slice(&encoded);
    let mut child = node(&[], 34);
    child[16..20].copy_from_slice(&3f32.to_le_bytes());
    let (_, scene) = nif_scene::decode(
        &container(&[("BSFadeNode", parent), ("NiNode", child)], &[0], 34),
        "transform fixture",
    )
    .unwrap();
    assert_eq!(
        scene.world_transforms[1].matrix,
        [[0., -2., 0., 10.], [2., 0., 0., 26.], [0., 0., 2., 30.]]
    );
    assert_eq!(scene.world_transforms[1].parent, Some(0));
    assert!(scene.world_transforms[1].reachable_from_footer);
}

#[test]
fn cycles_multiple_parents_and_wrong_mesh_data_types_fail() {
    for (blocks, roots) in [
        (
            vec![("NiNode", node(&[1], 34)), ("NiNode", node(&[0], 34))],
            vec![],
        ),
        (
            vec![("NiNode", node(&[1, 1], 34)), ("NiNode", node(&[], 34))],
            vec![0],
        ),
        (
            vec![
                ("NiNode", node(&[2], 34)),
                ("NiNode", node(&[2], 34)),
                ("NiNode", node(&[], 34)),
            ],
            vec![0, 1],
        ),
        (
            vec![("NiTriStrips", shape(1, 34)), ("NiNode", node(&[], 34))],
            vec![0],
        ),
        (
            vec![("NiNode", node(&[1], 34)), ("NiNode", node(&[], 34))],
            vec![0, 1],
        ),
    ] {
        assert!(nif_scene::decode(&container(&blocks, &roots, 34), "invalid graph").is_err());
    }
}

#[test]
fn unknown_branches_and_unreachable_objects_are_explicit() {
    let blocks = [
        ("NiNode", node(&[1], 34)),
        ("FutureAVObject", vec![7, 8]),
        ("NiNode", node(&[], 34)),
    ];
    let (_, scene) = nif_scene::decode(&container(&blocks, &[0], 34), "unknown branch").unwrap();
    assert_eq!(scene.unsupported_blocks["FutureAVObject"], [1]);
    assert_eq!(scene.unsupported_scene_edges[0].target, 1);
    assert!(!scene.world_transforms[1].reachable_from_footer);
    assert!(!scene.runtime_ready);
}

#[test]
fn supported_blocks_require_exact_consumption_and_bounded_arrays() {
    for (kind, payload) in [
        ("NiNode", node(&[], 34)),
        ("NiTriStripsData", strips(&[0, 1, 2, 3])),
    ] {
        for length in 0..payload.len() {
            assert!(
                nif_scene::decode(
                    &container(&[(kind, payload[..length].to_vec())], &[], 34),
                    "truncated block"
                )
                .is_err(),
                "{kind} {length}"
            );
        }
        let mut extended = payload;
        extended.push(0);
        assert!(
            nif_scene::decode(&container(&[(kind, extended)], &[], 34), "extended block").is_err()
        );
    }
    let mut bomb = node(&[], 34);
    bomb[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(nif_scene::decode(&container(&[("NiNode", bomb)], &[], 34), "count bomb").is_err());
}

#[test]
fn bad_vertices_nonfinite_values_and_invalid_reference_indices_fail() {
    let invalid_indices = strips(&[0, 1, 4]);
    assert!(
        nif_scene::decode(
            &container(&[("NiTriStripsData", invalid_indices)], &[], 34),
            "bad vertex"
        )
        .is_err()
    );
    let mut nan = strips(&[0, 1, 2]);
    nan[9..13].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(nif_scene::decode(&container(&[("NiTriStripsData", nan)], &[], 34), "NaN").is_err());
    for offset in [0, 8, 72] {
        let mut bad = node(&[], 34);
        bad[offset..offset + 4].copy_from_slice(&9u32.to_le_bytes());
        assert!(nif_scene::decode(&container(&[("NiNode", bad)], &[], 34), "bad link").is_err());
    }
}

#[test]
fn old_stream_flag_widths_and_empty_geometry_do_not_shift_fields() {
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let (_, scene) = nif_scene::decode(
            &container(&[("NiNode", node(&[], stream))], &[0], stream),
            "flag width",
        )
        .unwrap();
        assert_eq!(scene.objects[0].flags, 14);
        assert_eq!(scene.objects[0].transform.scale, 1.);
    }
    let mut b = Vec::new();
    u32s(&mut b, &[0]);
    u16s(&mut b, &[0]);
    b.extend([0; 3]);
    u16s(&mut b, &[0]);
    b.push(0);
    floats(&mut b, &[0.; 4]);
    b.push(0);
    u16s(&mut b, &[0]);
    u32s(&mut b, &[NULL]);
    u16s(&mut b, &[0, 0]);
    b.push(0);
    let (_, scene) =
        nif_scene::decode(&container(&[("NiTriStripsData", b)], &[], 34), "empty mesh").unwrap();
    assert!(scene.meshes[0].vertices.is_empty());
    assert!(!scene.meshes[0].has_vertices);
}

#[test]
fn deep_scene_graphs_use_bounded_iterative_traversal() {
    let count = 10_000;
    let blocks: Vec<_> = (0..count)
        .map(|i| {
            let child = [i + 1];
            ("NiNode", node(if i + 1 < count { &child } else { &[] }, 34))
        })
        .collect();
    let (_, scene) = nif_scene::decode(&container(&blocks, &[0], 34), "deep graph").unwrap();
    assert_eq!(scene.world_transforms.len(), count as usize);
    assert!(scene.world_transforms.last().unwrap().reachable_from_footer);
}

#[test]
fn malformed_scene_mutations_never_panic() {
    let original = container(
        &[
            ("NiNode", node(&[1], 34)),
            ("NiTriStrips", shape(2, 34)),
            ("NiTriStripsData", strips(&[0, 1, 2, 3])),
        ],
        &[0],
        34,
    );
    for offset in 0..original.len() {
        let mut mutated = original.clone();
        mutated[offset] = 255;
        let _ = nif_scene::decode(&mutated, "mutation");
    }
}

#[test]
fn scene_budgets_apply_across_blocks_before_allocating_arrays() {
    let bytes = container(&[("NiTriStripsData", strips(&[0, 1, 2, 3]))], &[], 34);
    let defaults = nif_scene::Limits::default();
    for limits in [
        nif_scene::Limits {
            input_bytes: bytes.len() - 1,
            ..defaults
        },
        nif_scene::Limits {
            blocks: 0,
            ..defaults
        },
        nif_scene::Limits {
            array_bytes: 16,
            ..defaults
        },
    ] {
        assert!(nif_scene::decode_with_limits(&bytes, "budget fixture", limits).is_err());
    }
    let mut payload = Vec::new();
    u32s(&mut payload, &[0]);
    u16s(&mut payload, &[0]);
    payload.extend([0; 3]);
    u16s(&mut payload, &[0]);
    payload.push(0);
    floats(&mut payload, &[0.; 4]);
    payload.push(0);
    u16s(&mut payload, &[0]);
    u32s(&mut payload, &[NULL]);
    u16s(&mut payload, &[0, 100]);
    u16s(&mut payload, &[0; 100]);
    payload.push(1);
    let bytes = container(
        &[
            ("NiTriStripsData", payload.clone()),
            ("NiTriStripsData", payload),
        ],
        &[],
        34,
    );
    // Zero-length strips still allocate Vec headers. The shared budget catches that.
    assert!(
        nif_scene::decode_with_limits(
            &bytes,
            "empty strip bomb",
            nif_scene::Limits {
                array_bytes: 3000,
                ..defaults
            }
        )
        .is_err()
    );
}

#[test]
fn material_version_fields_remain_distinguishable_from_defaults() {
    use nif_scene::material::MaterialData;
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let mut material = net();
        if stream < 26 {
            floats(&mut material, &[0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
        }
        floats(&mut material, &[1., 0.5, 0.25, 0.1, 0.2, 0.3, 32., 0.75]);
        if stream > 21 {
            floats(&mut material, &[1.5]);
        }
        let mut pp = shader_header();
        u32s(&mut pp, &[2]);
        if stream > 14 {
            floats(&mut pp, &[0.125]);
            u32s(&mut pp, &[7]);
        }
        if stream > 24 {
            floats(&mut pp, &[4., 0.25]);
        }
        let blocks = [
            ("NiMaterialProperty", material),
            ("BSShaderPPLightingProperty", pp),
            (
                "BSShaderTextureSet",
                texture_set(&[b"textures/a.dds", b"", b"b.dds"]),
            ),
        ];
        let (_, scene) =
            nif_scene::decode(&container(&blocks, &[], stream), "material versions").unwrap();
        match &scene.materials[0].data {
            MaterialData::Material {
                ambient,
                diffuse,
                emissive_multiplier,
                alpha,
                ..
            } => {
                assert_eq!(ambient.is_some(), stream < 26);
                assert_eq!(diffuse.is_some(), stream < 26);
                assert_eq!(emissive_multiplier.is_some(), stream > 21);
                assert_eq!(*alpha, 0.75);
            }
            _ => panic!("wrong material kind"),
        }
        match &scene.materials[1].data {
            MaterialData::PerPixelLighting {
                refraction_strength,
                parallax_passes,
                ..
            } => {
                assert_eq!(refraction_strength.is_some(), stream > 14);
                assert_eq!(parallax_passes.is_some(), stream > 24);
            }
            _ => panic!("wrong shader kind"),
        }
        assert_eq!(scene.textures.len(), 2);
        assert_eq!(scene.textures[1].slot, 2);
        assert_eq!(
            scene.textures[1].asset_path.as_ref().unwrap().bytes(),
            b"textures/b.dds"
        );
    }
}

#[test]
fn legacy_texture_slots_preserve_transforms_and_shader_maps() {
    use nif_scene::material::MaterialData;
    let mut source = net();
    source.push(1);
    u32s(&mut source, &[0, NULL, 6, 2, 3]);
    source.extend([1, 1, 0]);
    let mut texture = net();
    u16s(&mut texture, &[4]);
    u32s(&mut texture, &[12]);
    for slot in 0..12 {
        texture.push(u8::from(matches!(slot, 0 | 5 | 7)));
        if matches!(slot, 0 | 5 | 7) {
            u32s(&mut texture, &[1]);
            u16s(&mut texture, &[0x3200]);
            texture.push(1);
            floats(&mut texture, &[0.1, 0.2, 2., 3., 0.5]);
            u32s(&mut texture, &[2]);
            floats(&mut texture, &[0.5, 0.5]);
            if slot == 5 {
                floats(&mut texture, &[2., 0.5, 1., 0., 0., 1.]);
            }
            if slot == 7 {
                floats(&mut texture, &[0.25]);
            }
        }
    }
    u32s(&mut texture, &[2]);
    texture.extend([0, 1]);
    u32s(&mut texture, &[1]);
    u16s(&mut texture, &[0x3200]);
    texture.push(0);
    u32s(&mut texture, &[99]);
    let (_, scene) = nif_scene::decode(
        &container(
            &[
                ("NiTexturingProperty", texture),
                ("NiSourceTexture", source),
            ],
            &[],
            34,
        ),
        "texture transforms",
    )
    .unwrap();
    if let MaterialData::Texturing {
        slots,
        bump_luma,
        bump_matrix,
        parallax_offset,
        shader_textures,
        ..
    } = &scene.materials[0].data
    {
        assert_eq!(slots.len(), 12);
        assert!(slots[1].is_none());
        assert_eq!(
            slots[0]
                .as_ref()
                .unwrap()
                .transform
                .as_ref()
                .unwrap()
                .method,
            2
        );
        assert_eq!(*bump_luma, Some([2., 0.5]));
        assert_eq!(*bump_matrix, Some([1., 0., 0., 1.]));
        assert_eq!(*parallax_offset, Some(0.25));
        assert!(shader_textures[0].is_none());
        assert_eq!(shader_textures[1].as_ref().unwrap().map_id, 99);
    } else {
        panic!("wrong material kind");
    }
    assert_eq!(scene.textures[0].raw_path, b"name");
}

#[test]
fn texture_paths_preserve_bytes_and_report_unsafe_sources() {
    let paths = [
        b"Textures\\A.DDS".as_slice(),
        b"a.dds",
        b"textures/\xE9.dds",
        b"C:\\export\\a.dds",
        b"../a.dds",
        b"a\0.dds",
    ];
    let (_, scene) = nif_scene::decode(
        &container(&[("BSShaderTextureSet", texture_set(&paths))], &[], 34),
        "texture paths",
    )
    .unwrap();
    assert_eq!(scene.textures.len(), paths.len());
    for (reference, original) in scene.textures.iter().zip(paths) {
        assert_eq!(reference.raw_path, original);
    }
    assert_eq!(scene.textures[0].asset_path, scene.textures[1].asset_path);
    assert_eq!(
        scene.textures[2].asset_path.as_ref().unwrap().bytes(),
        b"textures/\xE9.dds"
    );
    assert!(
        scene.textures[3..]
            .iter()
            .all(|t| t.error.is_some() && t.asset_path.is_none())
    );
    let bytes = container(
        &[("BSShaderTextureSet", texture_set(&[b"textures/a.dds"]))],
        &[],
        34,
    );
    assert!(
        nif_scene::decode_with_limits(
            &bytes,
            "derived budget",
            nif_scene::Limits {
                array_bytes: 40,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn material_links_and_nonfinite_values_are_rejected() {
    let mut pp = shader_header();
    u32s(&mut pp, &[1]);
    floats(&mut pp, &[0.]);
    u32s(&mut pp, &[0]);
    floats(&mut pp, &[4., 1.]);
    assert!(
        nif_scene::decode(
            &container(
                &[
                    ("BSShaderPPLightingProperty", pp),
                    ("NiNode", node(&[], 34))
                ],
                &[],
                34
            ),
            "wrong texture-set target"
        )
        .is_err()
    );
    let mut material = net();
    floats(
        &mut material,
        &[1., 1., 1., 0., 0., 0., f32::INFINITY, 1., 1.],
    );
    assert!(
        nif_scene::decode(
            &container(&[("NiMaterialProperty", material)], &[], 34),
            "invalid glossiness"
        )
        .is_err()
    );
}

#[test]
fn material_payloads_require_exact_bounds_and_preserve_flag_bits() {
    let mut alpha = net();
    u16s(&mut alpha, &[0xFEED]);
    alpha.push(123);
    let mut stencil = net();
    u16s(&mut stencil, &[0x4D80]);
    u32s(&mut stencil, &[17, 0xABCD]);
    let mut shade = net();
    u16s(&mut shade, &[1]);
    let mut unlit = shader_header();
    u32s(&mut unlit, &[5]);
    unlit.extend(b"a.dds");
    floats(&mut unlit, &[1., 0., 1., 0.]);
    for (kind, payload) in [
        ("NiAlphaProperty", alpha),
        ("NiStencilProperty", stencil),
        ("NiShadeProperty", shade),
        ("BSShaderNoLightingProperty", unlit),
        ("BSShaderTextureSet", texture_set(&[b"textures/a.dds"])),
    ] {
        assert!(
            nif_scene::decode(
                &container(&[(kind, payload.clone())], &[], 34),
                "valid property"
            )
            .is_ok()
        );
        for length in 0..payload.len() {
            assert!(
                nif_scene::decode(
                    &container(&[(kind, payload[..length].to_vec())], &[], 34),
                    "truncated property"
                )
                .is_err(),
                "{kind} {length}"
            );
        }
        let mut extended = payload;
        extended.push(0);
        assert!(
            nif_scene::decode(
                &container(&[(kind, extended)], &[], 34),
                "extended property"
            )
            .is_err()
        );
    }
}
