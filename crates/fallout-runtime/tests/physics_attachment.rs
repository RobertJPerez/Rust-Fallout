//! Literal source attachment fixtures; source transforms are signed permutations.
use fallout_data::coordinates::Affine;
use fallout_runtime::{
    identity::ReferenceId,
    physics::{
        attachment::{self, Selection, SourceAttachment},
        *,
    },
};
use sha2::{Digest, Sha256};
use std::{fs, num::NonZeroU64};
const NULL: u32 = u32::MAX;
fn container(blocks: &[(&str, Vec<u8>)], roots: &[u32]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x14020007u32.to_le_bytes());
    b.push(1);
    for n in [11u32, blocks.len() as u32, 34] {
        b.extend(n.to_le_bytes());
    }
    b.extend([0; 3]);
    b.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        b.extend((name.len() as u32).to_le_bytes());
        b.extend(name.as_bytes());
    }
    for i in 0..blocks.len() {
        b.extend((i as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        b.extend((data.len() as u32).to_le_bytes());
    }
    for n in [0u32, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    let mut spans = Vec::new();
    for (_, data) in blocks {
        spans.push((b.len(), data.len()));
        b.extend(data);
    }
    b.extend((roots.len() as u32).to_le_bytes());
    for n in roots {
        b.extend(n.to_le_bytes());
    }
    (b, spans)
}
fn node(
    translation: [f32; 3],
    rotation: [[f32; 3]; 3],
    scale: f32,
    collision: u32,
    children: &[u32],
) -> Vec<u8> {
    let mut b = Vec::new();
    for n in [NULL, 0, NULL, 14] {
        b.extend(n.to_le_bytes());
    }
    for n in translation
        .into_iter()
        .chain(rotation.into_iter().flatten())
        .chain([scale])
    {
        b.extend(n.to_le_bytes());
    }
    for n in [0, collision, children.len() as u32] {
        b.extend(n.to_le_bytes());
    }
    for n in children {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    b
}
fn object(target: u32) -> Vec<u8> {
    [
        target.to_le_bytes().as_slice(),
        &0xd321u16.to_le_bytes(),
        &1u32.to_le_bytes(),
    ]
    .concat()
}
const I: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const RX: [[f32; 3]; 3] = [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]];
const RZ: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&3u32.to_le_bytes());
    body[4..8].copy_from_slice(&[5, 0xe7, 0x34, 0x12]);
    for (at, value) in [(52, 1f32), (56, 2.), (60, 3.), (72, 1.)] {
        body[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    // Active body is a half-turn about Y: x->-x, y->y, z->-z.
    let mut shape = [
        17u32.to_le_bytes().as_slice(),
        &0.25f32.to_le_bytes(),
        &[0; 8],
    ]
    .concat();
    for value in [1f32, 2., 3., 0.] {
        shape.extend(value.to_le_bytes());
    }
    vec![
        ("bhkCollisionObject", object(2)),
        ("bhkRigidBodyT", body),
        ("NiNode", node([3., 4., 5.], RX, 0.5, 0, &[])),
        ("bhkBoxShape", shape),
        // Parent ID4 is higher than its child ID2.
        ("BSFadeNode", node([10., 20., 30.], RZ, 2., NULL, &[2])),
    ]
}
fn fixture(mode: usize) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut blocks = blocks();
    let mut roots = vec![4];
    match mode {
        0 => {}
        1 => {
            blocks.push(("bhkCollisionObject", object(6)));
            blocks.push(("NiNode", node([7., 8., 9.], I, 1., 5, &[])));
            blocks[4].1 = node([10., 20., 30.], RZ, 2., NULL, &[2, 6]);
        }
        2 => blocks[4].1[8..12].copy_from_slice(&1u32.to_le_bytes()),
        3 => {
            blocks.push(("NiBillboardNode", node([0.; 3], I, 1., NULL, &[2])));
        }
        4 => blocks[0].1[..4].copy_from_slice(&NULL.to_le_bytes()),
        5 => {
            blocks.push(("bhkCollisionObject", object(2)));
        }
        6 => {
            blocks[2].1 = node([3., 4., 5.], RX, 0.5, 0, &[4]);
            roots.clear();
        }
        7 => blocks[2].1[64..68].copy_from_slice(&0f32.to_le_bytes()),
        8 => roots.clear(),
        9 => {
            blocks.push(("NiNode", node([0.; 3], I, 1., 0, &[])));
        }
        10 => blocks[2].1[72..76].copy_from_slice(&NULL.to_le_bytes()),
        11 => blocks[0].1[6..10].copy_from_slice(&NULL.to_le_bytes()),
        12 => {
            blocks.push(("NiNode", node([0.; 3], I, 1., NULL, &[2])));
            roots.push(5);
        }
        13 => roots.push(4),
        14 => blocks[2].1[32..36].copy_from_slice(&0.25f32.to_le_bytes()),
        15 => blocks[2].1[16..20].copy_from_slice(&f32::INFINITY.to_le_bytes()),
        16 => {
            blocks.push(("NiObject", vec![]));
        }
        17 => blocks[2].1[8..12].copy_from_slice(&1u32.to_le_bytes()),
        _ => panic!("unknown fixture"),
    }
    container(&blocks, &roots)
}
fn selection(bytes: &[u8]) -> Selection {
    Selection {
        reference: ReferenceId(NonZeroU64::new(0xf000000000000018).unwrap()),
        source_sha256: Sha256::digest(bytes).into(),
        collision_object: 0,
        body_block: 1,
        target_block: 2,
        placement_to_source: Affine {
            rows: [[0., 0., 1., 100.], [0., 1., 0., 200.], [-1., 0., 0., 300.]],
        },
    }
}
fn units() -> EngineeringUnits {
    EngineeringUnits {
        havok_to_source: 2.,
        source_to_query: 3.,
        transform_tolerance: 0.,
    }
}
fn derived(bytes: &[u8]) -> SourceAttachment {
    SourceAttachment::derive(bytes, selection(bytes), units(), Default::default()).unwrap()
}
fn ray() -> Ray {
    Ray {
        origin: [400., 684., 876.],
        direction: [1., 0., 0.],
        max_distance: 100.,
    }
}
fn conservative_entry(actual: f64, literal: f64) {
    // The unchanged slab contract returns a representable entry witness. With
    // query scale6 the stored inverse has nonbinary thirds; bound the forward
    // witness error without pretending that its distance must equal an integer.
    assert!(
        actual >= literal && actual - literal <= 1e-10,
        "{actual} vs {literal}"
    );
}
#[test]
fn higher_id_noncommuting_ancestry_body_and_caller_frames_apply_once() {
    let (bytes, spans) = fixture(0);
    let attachment = derived(&bytes);
    let scope = attachment.scope();
    assert_eq!(
        scope
            .ancestry
            .iter()
            .map(|a| a.span.block)
            .collect::<Vec<_>>(),
        vec![2, 4]
    );
    assert_eq!(scope.ancestry[0].parent, Some(4));
    assert_eq!(scope.ancestry[1].parent, None);
    assert_eq!(
        scope.source_world.rows,
        [[0., 0., 1., 2.], [1., 0., 0., 26.], [0., 1., 0., 40.]]
    );
    assert_eq!(
        scope.attachment_to_source.rows,
        [[0., 1., 0., 140.], [1., 0., 0., 226.], [0., 0., -1., 298.]]
    );
    for s in [
        &scope.collision_object,
        &scope.body,
        &scope.target,
        &scope.ancestry[1].span,
    ] {
        assert_eq!((s.offset, s.bytes), spans[s.block as usize]);
        assert_eq!(
            s.sha256,
            format!("{:x}", Sha256::digest(&bytes[s.offset..s.offset + s.bytes]))
        );
    }
    assert_eq!(scope.collision_flags, 0xd321);
    assert_eq!(scope.usage.source_link_visits, 23);
    assert_eq!(scope.usage.ancestry_visits, 2);
    assert_eq!(scope.usage.scope_metadata_bytes, 11264);
    let scene = attachment.build_scene(Default::default()).unwrap();
    let hits = scene.ray_cast(ray(), Default::default()).unwrap();
    assert_eq!(hits.len(), 1);
    conservative_entry(hits[0].distance, 20.);
    conservative_entry(hits[0].position[0], 420.);
    assert_eq!(hits[0].position[1..], [684., 876.]);
    assert_eq!(hits[0].source.reference, selection(&bytes).reference);
    assert_eq!(hits[0].source.body_block, 1);
    assert_eq!(hits[0].source.shape_block, 3);
    assert_eq!(hits[0].material, 17);
    assert_eq!(hits[0].body_filter.flags_and_parts, 0xe7);
    assert_eq!(hits[0].authored_shell_radius, 0.25);
    assert_eq!(
        scene
            .overlap_sphere([419.5, 684., 876.], 1., Default::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        scene
            .overlap_sphere([418., 684., 876.], 1., Default::default())
            .unwrap()
            .is_empty()
    );
    assert!(!scene.faithful_ready());
}
#[test]
fn separate_shared_body_occurrences_keep_exact_object_and_target_identity() {
    let (bytes, _) = fixture(1);
    let first = derived(&bytes);
    let mut request = selection(&bytes);
    request.collision_object = 5;
    request.target_block = 6;
    let second = SourceAttachment::derive(&bytes, request, units(), Default::default()).unwrap();
    assert_eq!(first.scope().body.sha256, second.scope().body.sha256);
    assert_eq!(first.scope().reference, second.scope().reference);
    assert_eq!(first.scope().source_sha256, second.scope().source_sha256);
    assert_eq!(first.scope().collision_object.block, 0);
    assert_eq!(second.scope().collision_object.block, 5);
    assert_eq!(
        second
            .scope()
            .ancestry
            .iter()
            .map(|a| a.span.block)
            .collect::<Vec<_>>(),
        vec![6, 4]
    );
    assert_eq!(
        second.scope().source_world.rows,
        [[0., -2., 0., -6.], [2., 0., 0., 34.], [0., 0., 2., 48.]]
    );
    assert_ne!(
        first.placement().attachment_to_source.rows,
        second.placement().attachment_to_source.rows
    );
    let second_scene = second.build_scene(Default::default()).unwrap();
    // Second source center (-14,38,60), caller ->(160,238,314), query ->(480,714,942).
    let hits = second_scene
        .ray_cast(
            Ray {
                origin: [400., 714., 942.],
                direction: [1., 0., 0.],
                max_distance: 100.,
            },
            Default::default(),
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    conservative_entry(hits[0].distance, 44.); // Query X half extent36.
    assert_eq!(
        hits[0].source,
        first
            .build_scene(Default::default())
            .unwrap()
            .ray_cast(ray(), Default::default())
            .unwrap()[0]
            .source
    );
}
#[test]
fn null_ambiguous_opaque_controller_cycle_and_invalid_ancestry_refuse() {
    for mode in 2..=17 {
        let (bytes, _) = fixture(mode);
        assert!(
            SourceAttachment::derive(&bytes, selection(&bytes), units(), Default::default())
                .is_err(),
            "mode {mode}"
        );
    }
    let (bytes, _) = fixture(0);
    for (object, body, target) in [(999, 1, 2), (1, 1, 2), (0, 3, 2), (0, 1, 4)] {
        let mut s = selection(&bytes);
        s.collision_object = object;
        s.body_block = body;
        s.target_block = target;
        assert!(SourceAttachment::derive(&bytes, s, units(), Default::default()).is_err());
    }
}
#[test]
fn exact_global_source_decode_link_ancestry_and_metadata_boundaries() {
    let (bytes, _) = fixture(0);
    let scope = derived(&bytes).scope().clone();
    let exact = attachment::Limits {
        source_bytes: bytes.len(),
        blocks: 5,
        decoded_metadata_bytes: scope.usage.decoded_reservation_bytes,
        source_link_visits: 23,
        ancestry_visits: 2,
        scope_metadata_bytes: 11264,
        ..Default::default()
    };
    assert!(SourceAttachment::derive(&bytes, selection(&bytes), units(), exact).is_ok());
    for case in 0..6 {
        let mut limits = exact;
        match case {
            0 => limits.source_bytes -= 1,
            1 => limits.blocks -= 1,
            2 => limits.decoded_metadata_bytes -= 1,
            3 => limits.source_link_visits -= 1,
            4 => limits.ancestry_visits -= 1,
            5 => limits.scope_metadata_bytes -= 1,
            _ => unreachable!(),
        }
        assert!(
            SourceAttachment::derive(&bytes, selection(&bytes), units(), limits).is_err(),
            "case {case}"
        );
    }
    assert!(
        SourceAttachment::derive(
            &bytes,
            selection(&bytes),
            units(),
            attachment::Limits {
                array_bytes: 16,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(
        SourceAttachment::derive(
            &bytes,
            selection(&bytes),
            units(),
            attachment::Limits {
                array_bytes: 15,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        SourceAttachment::derive(
            &bytes,
            selection(&bytes),
            units(),
            attachment::Limits {
                source_bytes: 4 * 1024 * 1024 + 1,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn whole_source_digest_explicit_units_and_caller_transform_cannot_be_forged() {
    let (bytes, _) = fixture(0);
    let mut s = selection(&bytes);
    s.source_sha256[31] ^= 1;
    assert!(SourceAttachment::derive(&bytes, s, units(), Default::default()).is_err());
    for value in [0., -1., f64::NAN, f64::INFINITY] {
        let mut u = units();
        u.havok_to_source = value;
        assert!(
            SourceAttachment::derive(&bytes, selection(&bytes), u, Default::default()).is_err()
        );
    }
    for value in [0., 2., f64::INFINITY] {
        let mut s = selection(&bytes);
        s.placement_to_source.rows[0][2] = value;
        assert!(SourceAttachment::derive(&bytes, s, units(), Default::default()).is_err());
    }
    let mut s = selection(&bytes);
    s.placement_to_source.rows[0][2] = -1.;
    assert!(
        SourceAttachment::derive(&bytes, s, units(), Default::default()).is_ok(),
        "authored reflection stays explicit"
    );
}
#[test]
fn source_body_active_flag_and_shell_scope_keep_the_existing_predicate() {
    let mut blocks = blocks();
    blocks[1].0 = "bhkRigidBody";
    let (bytes, _) = container(&blocks, &[4]);
    let attachment = derived(&bytes);
    let hits = attachment
        .build_scene(Default::default())
        .unwrap()
        .ray_cast(
            Ray {
                origin: [400., 678., 894.],
                direction: [1., 0., 0.],
                max_distance: 100.,
            },
            Default::default(),
        )
        .unwrap();
    conservative_entry(hits[0].distance, 8.);
    assert_eq!(hits[0].authored_shell_radius, 0.25);
    let placement = attachment.placement().clone();
    let (_, collision) =
        fallout_data::nif_collision::decode(&bytes, "literal independent direct placement")
            .unwrap();
    let explicit =
        StaticScene::build(&collision, &[placement], units(), Default::default()).unwrap();
    let original = explicit
        .ray_cast(
            Ray {
                origin: [400., 678., 894.],
                direction: [1., 0., 0.],
                max_distance: 100.,
            },
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(hits).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}
#[test]
#[ignore = "explicit private source-attachment CLI fixture export"]
fn attachment_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_ATTACHMENT_FIXTURE").expect("private fixture path"),
    );
    fs::create_dir(&root).unwrap();
    for mode in 0..=17 {
        let (bytes, _) = fixture(mode);
        fs::write(root.join(format!("source-{mode}.nif")), &bytes).unwrap();
        let mut r = serde_json::json!({"reference":0xf000000000000018u64,"source_sha256":format!("{:x}",Sha256::digest(&bytes)),
            "collision_object":0,"body_block":1,"target_block":2,"placement_rows":selection(&bytes).placement_to_source.rows,
            "units":{"havok_to_source":2.0,"source_to_query":3.0,"transform_tolerance":0.0},
            "ray":{"origin":[400.0,684.0,876.0],"direction":[1.0,0.0,0.0],"max_distance":100.0},
            "overlap":{"center":[419.5,684.0,876.0],"radius":1.0}});
        fs::write(
            root.join(format!("request-{mode}.json")),
            serde_json::to_vec_pretty(&r).unwrap(),
        )
        .unwrap();
        if mode == 1 {
            r["collision_object"] = 5.into();
            r["target_block"] = 6.into();
            r["ray"]["origin"] = serde_json::json!([400.0, 714.0, 942.0]);
            r["overlap"]["center"] = serde_json::json!([443.5, 714.0, 942.0]);
            fs::write(
                root.join("request-shared-second.json"),
                serde_json::to_vec_pretty(&r).unwrap(),
            )
            .unwrap();
        }
    }
}
