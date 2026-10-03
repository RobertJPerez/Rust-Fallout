//! Original source graphs: membership describes decoded edges, never an external rig.
use fallout_data::nif_skin::{
    binding::{self, Diagnostic, Limits},
    partition,
};

const NULL: u32 = u32::MAX;
const STREAMS: [u32; 12] = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn shorts(out: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32, roots: &[u32]) -> Vec<u8> {
    container_with_strings(blocks, stream, roots, &[])
}
fn container_with_strings(
    blocks: &[(&str, Vec<u8>)],
    stream: u32,
    roots: &[u32],
    strings: &[&str],
) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, stream]);
    out.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    shorts(&mut out, &[types.len() as u16]);
    for name in &types {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        shorts(
            &mut out,
            &[types.iter().position(|v| v == name).unwrap() as u16],
        );
    }
    for (_, block) in blocks {
        words(&mut out, &[block.len() as u32]);
    }
    words(
        &mut out,
        &[
            strings.len() as u32,
            strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32,
        ],
    );
    for string in strings {
        words(&mut out, &[string.len() as u32]);
        out.extend(string.as_bytes());
    }
    words(&mut out, &[0]);
    for (_, block) in blocks {
        out.extend(block);
    }
    words(&mut out, &[roots.len() as u32]);
    words(&mut out, roots);
    out
}
fn av(stream: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, 0, NULL]);
    if stream <= 26 {
        shorts(&mut out, &[65535]);
    } else {
        words(&mut out, &[0xFFFF_FFFF]);
    }
    for value in [1f32, -0., 3., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.5] {
        words(&mut out, &[value.to_bits()]);
    }
    words(&mut out, &[0, NULL]);
    out
}
fn node(stream: u32, children: &[u32]) -> Vec<u8> {
    let mut out = av(stream);
    words(&mut out, &[children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn instance(root: u32, bones: &[u32], partition: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, partition, root, bones.len() as u32]);
    words(&mut out, bones);
    out
}
fn shape(stream: u32, data: u32, skin: u32) -> Vec<u8> {
    let mut out = av(stream);
    words(&mut out, &[data, skin, 0, NULL]);
    out.push(0);
    out
}
fn geometry() -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0]);
    shorts(&mut out, &[0]);
    out.extend([0; 3]);
    shorts(&mut out, &[0]);
    out.push(0);
    words(&mut out, &[0, 0, 0, 1f32.to_bits()]);
    out.push(0);
    shorts(&mut out, &[0]);
    words(&mut out, &[NULL]);
    shorts(&mut out, &[0]);
    words(&mut out, &[0]);
    out.push(0);
    shorts(&mut out, &[0]);
    out
}
fn fixture(stream: u32) -> Vec<(&'static str, Vec<u8>)> {
    let mut part = Vec::new();
    words(&mut part, &[0]);
    vec![
        ("NiNode", node(stream, &[1, 3])),
        ("BSFadeNode", node(stream, &[])),
        ("NiSkinInstance", instance(0, &[1, 0, 1], 4)),
        ("NiTriShape", shape(stream, 5, 2)),
        ("NiSkinPartition", part),
        ("NiTriShapeData", geometry()),
    ]
}

#[test]
fn all_streams_preserve_authored_bone_order_and_node_bits() {
    for stream in STREAMS {
        let bytes = container(&fixture(stream), stream, &[0]);
        let (index, source) = binding::decode(&bytes, "authored").unwrap();
        let c = &source.bindings;
        assert_eq!(c.nodes.iter().map(|n| n.block).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(c.nodes[0].children, [Some(1), Some(3)]);
        assert_eq!(c.nodes[1].parent, Some(0));
        assert_eq!(
            c.nodes[0].transform.translation_bits,
            [0x3F80_0000, 0x8000_0000, 0x4040_0000]
        );
        assert_eq!(c.nodes[0].transform.scale_bits, 0x3F00_0000);
        assert_eq!(
            c.nodes[0].flags,
            if stream <= 26 { 65535 } else { u32::MAX }
        );
        assert_eq!(
            (c.nodes[1].offset, c.nodes[1].bytes),
            (index.blocks[1].offset, index.blocks[1].bytes)
        );
        let inst = &c.instances[0];
        assert_eq!(inst.skeleton_root.target, Some(0));
        assert!(inst.skeleton_root.decoded_node);
        assert_eq!(
            inst.bones
                .iter()
                .map(|b| (b.ordinal, b.node.target, b.decoded_root_contains))
                .collect::<Vec<_>>(),
            [
                (0, Some(1), Some(true)),
                (1, Some(0), Some(true)),
                (2, Some(1), Some(true))
            ]
        );
        assert_eq!(inst.owners[0].geometry, 3);
        assert_eq!(inst.owners[0].decoded_root_contains, Some(true));
        assert_eq!(c.footer_roots, [Some(0)]);
        assert!(c.diagnostics.is_empty());
        assert!(!c.runtime_ready);
        let (_, old) = partition::decode(&bytes, "old mode").unwrap();
        assert_eq!(
            serde_json::to_value(old).unwrap(),
            serde_json::to_value(&source.skin).unwrap()
        );
    }
}

#[test]
fn null_children_effects_and_footer_positions_are_not_removed() {
    let mut blocks = fixture(34);
    blocks[0].1 = node(34, &[NULL, 1, NULL, 3]);
    // Effects have raw nullable references; they do not establish scene parents.
    let effects_offset = blocks[0].1.len() - 4;
    blocks[0].1.truncate(effects_offset);
    words(&mut blocks[0].1, &[3, NULL, 1, NULL]);
    let (_, s) =
        binding::decode(&container(&blocks, 34, &[NULL, 0, NULL]), "nullable arrays").unwrap();
    assert_eq!(s.bindings.nodes[0].children, [None, Some(1), None, Some(3)]);
    assert_eq!(s.bindings.nodes[0].effects, [None, Some(1), None]);
    assert_eq!(s.bindings.footer_roots, [None, Some(0), None]);
    assert!(s.bindings.diagnostics.is_empty());
}

#[test]
fn outside_root_bones_are_facts_with_authored_duplicates_retained() {
    let mut blocks = fixture(34);
    blocks.push(("NiNode", node(34, &[])));
    blocks[2].1 = instance(0, &[6, 1, 6], 4);
    let (_, s) = binding::decode(&container(&blocks, 34, &[0, 6]), "outside").unwrap();
    assert_eq!(
        s.bindings.instances[0]
            .bones
            .iter()
            .map(|b| b.decoded_root_contains)
            .collect::<Vec<_>>(),
        [Some(false), Some(true), Some(false)]
    );
    assert_eq!(
        s.bindings
            .diagnostics
            .iter()
            .filter(|d| matches!(d, Diagnostic::BoneOutsideDecodedRoot { target: 6, .. }))
            .count(),
        2
    );
    assert!(
        s.bindings.instances[0].bones[0]
            .node
            .reachable_from_footer
            .unwrap()
    );
}

#[test]
fn duplicate_source_names_do_not_alias_bone_identity() {
    let mut blocks = fixture(34);
    blocks.push(("NiNode", node(34, &[])));
    for id in [0, 1, 6] {
        blocks[id].1[..4].copy_from_slice(&0u32.to_le_bytes());
    }
    blocks[2].1 = instance(0, &[6, 1, 6], 4);
    let (_, s) = binding::decode(
        &container_with_strings(&blocks, 34, &[0, 6], &["DuplicateBoneName"]),
        "duplicate names",
    )
    .unwrap();
    assert!(s.bindings.nodes.iter().all(|n| n.name == Some(0)));
    assert_eq!(
        s.bindings.instances[0]
            .bones
            .iter()
            .map(|b| (b.node.target, b.decoded_root_contains))
            .collect::<Vec<_>>(),
        [
            (Some(6), Some(false)),
            (Some(1), Some(true)),
            (Some(6), Some(false))
        ]
    );
}

#[test]
fn unsupported_intermediate_keeps_complete_source_ancestry_unproven() {
    let mut blocks = fixture(34);
    blocks[0].1 = node(34, &[6, 3]);
    let mut unknown = node(34, &[1]);
    shorts(&mut unknown, &[0]); // authored NiBillboardNode field after inherited node
    blocks.push(("NiBillboardNode", unknown));
    let (_, s) =
        binding::decode(&container(&blocks, 34, &[0]), "unsupported intermediate").unwrap();
    assert_eq!(s.bindings.ancestry_scope, "decoded-source-forest");
    assert_eq!(
        s.bindings.instances[0].bones[0].decoded_root_contains,
        Some(false)
    );
    assert_eq!(s.bindings.instances[0].bones[0].node.parent, None);
    assert_eq!(s.bindings.unsupported_scene_edges[0].target, 6);
    // The undecoded intermediary carries an authored edge to node1. The scoped
    // false above therefore cannot establish complete-source non-membership.
    assert_eq!(
        s.bindings.unsupported_scene_edges[0].block_type,
        "NiBillboardNode"
    );
}

#[test]
fn missing_root_and_bones_cannot_certify_membership() {
    let mut blocks = fixture(34);
    blocks[2].1 = instance(NULL, &[NULL, 1, NULL], 4);
    let (_, s) = binding::decode(&container(&blocks, 34, &[0]), "missing").unwrap();
    let i = &s.bindings.instances[0];
    assert!(!i.skeleton_root.decoded_node);
    assert!(i.bones.iter().all(|b| b.decoded_root_contains.is_none()));
    assert!(i.owners[0].decoded_root_contains.is_none());
    assert!(matches!(
        s.bindings.diagnostics[0],
        Diagnostic::MissingRoot { instance: 2 }
    ));
    assert_eq!(
        s.bindings
            .diagnostics
            .iter()
            .filter(|d| matches!(d, Diagnostic::MissingBone { .. }))
            .count(),
        2
    );
}

#[test]
fn undecoded_node_subtypes_stay_unresolved_and_keep_edge_positions() {
    let mut blocks = fixture(34);
    blocks[1] = ("NiBillboardNode", Vec::new());
    let (_, s) = binding::decode(&container(&blocks, 34, &[0]), "undecoded bone").unwrap();
    let bones = &s.bindings.instances[0].bones;
    assert!(!bones[0].node.decoded_node);
    assert!(bones[0].decoded_root_contains.is_none());
    assert_eq!(s.bindings.unsupported_scene_edges.len(), 1);
    assert_eq!(s.bindings.unsupported_scene_edges[0].target, 1);
    assert_eq!(
        s.bindings
            .diagnostics
            .iter()
            .filter(|d| matches!(d, Diagnostic::UndecodedBone { target: 1, .. }))
            .count(),
        2
    );
    blocks[0] = ("NiBillboardNode", Vec::new());
    let (_, s) = binding::decode(&container(&blocks, 34, &[0]), "undecoded root").unwrap();
    assert!(matches!(
        s.bindings.diagnostics[0],
        Diagnostic::UndecodedRoot { target: 0, .. }
    ));
    assert!(
        s.bindings.instances[0]
            .bones
            .iter()
            .all(|b| b.decoded_root_contains.is_none())
    );
}

#[test]
fn footer_unreachability_is_separate_from_decoded_root_membership() {
    let (_, s) = binding::decode(&container(&fixture(34), 34, &[]), "no footer").unwrap();
    assert_eq!(s.bindings.diagnostics.len(), 5);
    assert_eq!(
        s.bindings.instances[0].bones[0].decoded_root_contains,
        Some(true)
    );
    assert_eq!(
        s.bindings.instances[0].bones[0].node.reachable_from_footer,
        Some(false)
    );
}

#[test]
fn shared_owners_and_instances_keep_source_order() {
    let mut blocks = fixture(34);
    blocks.push(("NiTriShape", shape(34, 5, 2)));
    blocks.push(("NiSkinInstance", instance(1, &[1], 4)));
    blocks[0].1 = node(34, &[1, 6, 3]);
    let (_, s) = binding::decode(&container(&blocks, 34, &[0]), "shared").unwrap();
    assert_eq!(
        s.bindings
            .instances
            .iter()
            .map(|i| i.instance)
            .collect::<Vec<_>>(),
        [2, 7]
    );
    assert_eq!(
        s.bindings.instances[0]
            .owners
            .iter()
            .map(|o| o.geometry)
            .collect::<Vec<_>>(),
        [3, 6]
    );
    assert!(s.bindings.instances[1].owners.is_empty());
    assert_eq!(
        s.bindings.instances[1].bones[0].decoded_root_contains,
        Some(true)
    );
}

#[test]
fn cyclic_multi_parent_and_footer_parent_errors_still_use_existing_scene_validation() {
    for (root_children, bone_children, roots) in [
        (vec![1, 3], vec![0], vec![]),
        (vec![1, 1, 3], vec![], vec![0]),
        (vec![1, 3], vec![], vec![0, 1]),
    ] {
        let mut blocks = fixture(34);
        blocks[0].1 = node(34, &root_children);
        blocks[1].1 = node(34, &bone_children);
        assert!(binding::decode(&container(&blocks, 34, &roots), "invalid scene").is_err());
    }
}

#[test]
fn storage_and_graph_work_budgets_charge_empty_graph_relations() {
    let bytes = container(&fixture(34), 34, &[0]);
    let (_, s) = binding::decode(&bytes, "baseline").unwrap();
    let storage = s.bindings.retained_bytes;
    assert!(
        binding::decode_with_limits(
            &bytes,
            "exact storage",
            Limits {
                array_bytes: storage,
                ..Default::default()
            }
        )
        .is_ok()
    );
    let error = binding::decode_with_limits(
        &bytes,
        "under storage",
        Limits {
            array_bytes: storage - 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("binding storage budget"));
    assert!(
        binding::decode_with_limits(
            &bytes,
            "27checks",
            Limits {
                graph_checks: 27,
                ..Default::default()
            }
        )
        .is_ok()
    );
    let error = binding::decode_with_limits(
        &bytes,
        "26checks",
        Limits {
            graph_checks: 26,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("binding graph-check budget"));
}

#[test]
fn deep_graph_membership_is_iterative_and_preserves_root_self_binding() {
    const COUNT: u32 = 10_000;
    let mut blocks = Vec::new();
    for id in 0..COUNT {
        blocks.push((
            "NiNode",
            node(34, &[if id + 1 == COUNT { COUNT + 3 } else { id + 1 }]),
        ));
    }
    blocks.push((
        "NiSkinInstance",
        instance(0, &[COUNT - 1, 0, COUNT - 1], COUNT + 1),
    ));
    let mut part = Vec::new();
    words(&mut part, &[0]);
    blocks.push(("NiSkinPartition", part));
    blocks.push(("NiTriShapeData", geometry()));
    blocks.push(("NiTriShape", shape(34, COUNT + 2, COUNT)));
    let (_, s) = binding::decode(&container(&blocks, 34, &[0]), "deep").unwrap();
    assert_eq!(s.bindings.nodes.len(), COUNT as usize);
    assert!(s.bindings.diagnostics.is_empty());
    assert!(
        s.bindings.instances[0]
            .bones
            .iter()
            .all(|b| b.decoded_root_contains == Some(true))
    );
}
