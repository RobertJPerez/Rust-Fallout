//! Two independently authored source spaces and literal attachment expectations.
use fallout_data::{
    nif_animation::attachment::{self, Limits, Request, SourcePolicy},
    nif_skin::pose::Affine,
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
const NAME: &[u8] = b"Bip01 R Hand\0\xff";
const ROT: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
const HALF: [[f32; 3]; 3] = [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]];
const CALLER: Affine = [[-2., 0., 0., 7.], [0., 3., 0., 8.], [0., 0., 4., 9.]];
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn node(
    name: u32,
    children: &[u32],
    translation: [f32; 3],
    rotation: [[f32; 3]; 3],
    scale: f32,
    controller: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[name, 0, controller, 0x1234_5678]);
    for value in translation
        .into_iter()
        .chain(rotation.into_iter().flatten())
        .chain([scale])
    {
        words(&mut out, &[value.to_bits()]);
    }
    words(&mut out, &[0, NULL, children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn container(blocks: &[(&str, Vec<u8>)], strings: &[&[u8]], roots: &[u32]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, 34]);
    out.extend([0; 3]);
    let mut names = Vec::new();
    for (name, _) in blocks {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    out.extend((names.len() as u16).to_le_bytes());
    for name in &names {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        out.extend((names.iter().position(|n| n == name).unwrap() as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        words(&mut out, &[payload.len() as u32]);
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
        out.extend(*string);
    }
    words(&mut out, &[0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[roots.len() as u32]);
    words(&mut out, roots);
    out
}
fn skeleton(roots: &[u32], child_name: u32, root_name: &[u8], unknown_child: bool) -> Vec<u8> {
    container(
        &[
            (
                "NiNode",
                node(
                    1,
                    if unknown_child { &[1, 2] } else { &[1] },
                    [1., 2., 3.],
                    ROT,
                    2.,
                    NULL,
                ),
            ),
            ("NiNode", node(child_name, &[], [4., 5., 6.], HALF, 0.5, 2)),
            ("NiFloatInterpolator", vec![0]),
        ],
        &[NAME, root_name],
        roots,
    )
}
fn asset(translation: [f32; 3]) -> Vec<u8> {
    container(
        &[
            ("NiNode", node(0, &[], translation, ROT, 2., 1)),
            ("NiFloatInterpolator", vec![0]),
        ],
        &[b"attachment-root"],
        &[0],
    )
}
fn pair() -> (Vec<u8>, Vec<u8>) {
    (
        skeleton(&[0], 0, b"skeleton-root", false),
        asset([10., -20., 30.]),
    )
}
fn request<'a>(s: &[u8], a: &[u8]) -> Request<'a> {
    Request {
        expected_skeleton_sha256: Sha256::digest(s).into(),
        expected_attachment_sha256: Sha256::digest(a).into(),
        node: 1,
        node_name_bytes: NAME,
        attachment_root: 0,
        attachment_parent_to_node: CALLER,
        source_policy: SourcePolicy::StoredNiAvLocals,
    }
}
fn failure(s: &[u8], a: &[u8], request: Request<'_>, limits: Limits, reason: &str) {
    let (s_before, a_before) = (s.to_vec(), a.to_vec());
    let error = attachment::evaluate(s, a, "authored", request, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason}: {error}");
    assert_eq!(s, s_before);
    assert_eq!(a, a_before);
}
#[test]
fn literal_noncommuting_transform_order_applies_root_once_and_preserves_source() {
    let (s, a) = pair();
    let result =
        attachment::evaluate(&s, &a, "authored", request(&s, &a), Default::default()).unwrap();
    assert_eq!(
        result.attachment_source_to_skeleton_source,
        [[0., 3., 0., -1.], [2., 0., 0., 3.], [0., 0., 4., 24.]]
    );
    assert_eq!(
        result.root_to_skeleton_source,
        [[6., 0., 0., -61.], [0., -4., 0., 23.], [0., 0., 8., 144.]]
    );
    // Root-local [1,2,3] -> asset source [6,-18,36] -> skeleton [-55,15,168].
    // Applying root_to_skeleton to the already source-world point would yield
    // [-25,95,432], which is deliberately a different operation.
    assert_eq!(
        result
            .skeleton_path
            .iter()
            .map(|n| n.source.block)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(result.skeleton_path[0].unapplied_controller, Some(2));
    assert_eq!(result.attachment_root.unapplied_controller, Some(1));
    assert_eq!(result.node_name_bytes, NAME);
    assert_eq!(result.skeleton_path[0].flags, 0x1234_5678);
    assert_eq!(result.skeleton_sha256, format!("{:x}", Sha256::digest(&s)));
    assert_eq!(
        result.attachment_sha256,
        format!("{:x}", Sha256::digest(&a))
    );
    for (bytes, source) in [
        (&s, &result.skeleton_path[0].source),
        (&s, &result.skeleton_path[1].source),
        (&a, &result.attachment_root.source),
    ] {
        assert_eq!(
            source.sha256,
            format!(
                "{:x}",
                Sha256::digest(&bytes[source.offset..source.offset + source.bytes])
            )
        );
    }
    assert!(!result.retail_behavior_verified);
}
#[test]
fn both_expected_hashes_refuse_stale_source_before_source_decode_or_extra_allocation() {
    let (s, a) = pair();
    let r = request(&s, &a);
    let limits = Limits {
        array_bytes: 0,
        ..Default::default()
    };
    failure(
        b"changed skeleton",
        &a,
        r,
        limits,
        "skeleton source SHA256 differs",
    );
    failure(
        &s,
        b"changed attachment",
        r,
        limits,
        "attachment source SHA256 differs",
    );
}
#[test]
fn exact_node_id_and_raw_name_do_not_search_or_fold_names() {
    let (s, a) = pair();
    let r = request(&s, &a);
    failure(
        &s,
        &a,
        Request { node: 99, ..r },
        Default::default(),
        "node is not decoded",
    );
    failure(
        &s,
        &a,
        Request {
            node_name_bytes: b"bip01 r hand",
            ..r
        },
        Default::default(),
        "raw node name differs",
    );
    let s = skeleton(&[0], 0, b"wanted-root", false);
    failure(
        &s,
        &a,
        Request {
            node_name_bytes: b"wanted-root",
            ..request(&s, &a)
        },
        Default::default(),
        "raw node name differs",
    );
}
#[test]
fn duplicate_name_still_binds_exact_explicit_node_and_missing_name_refuses() {
    let (_, a) = pair();
    let s = skeleton(&[0], 0, NAME, false);
    let result =
        attachment::evaluate(&s, &a, "duplicates", request(&s, &a), Default::default()).unwrap();
    assert_eq!(result.skeleton_path[0].source.block, 1);
    let s = skeleton(&[0], NULL, NAME, false);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "no authored name",
    );
}
#[test]
fn disconnected_and_unresolved_source_forests_refuse() {
    let (_, a) = pair();
    let s = skeleton(&[], 0, b"root", false);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "not reachable from footer",
    );
    let s = skeleton(&[0], 0, b"root", true);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "unresolved scene ancestry",
    );
}
#[test]
fn attachment_requires_exact_footer_root_without_fallback() {
    let (s, a) = pair();
    let r = request(&s, &a);
    failure(
        &s,
        &a,
        Request {
            attachment_root: 1,
            ..r
        },
        Default::default(),
        "not an exact footer root",
    );
}
#[test]
fn finite_forward_mapping_permits_zero_and_refuses_nonfinite_or_overflow() {
    let (s, a) = pair();
    let r = request(&s, &a);
    for value in [f64::NAN, f64::INFINITY] {
        let mut matrix = CALLER;
        matrix[0][0] = value;
        failure(
            &s,
            &a,
            Request {
                attachment_parent_to_node: matrix,
                ..r
            },
            Default::default(),
            "nonfinite or overflowing",
        );
    }
    let mut matrix = CALLER;
    matrix[1][1] = f64::MAX;
    matrix[1][3] = f64::MAX;
    failure(
        &s,
        &a,
        Request {
            attachment_parent_to_node: matrix,
            ..r
        },
        Default::default(),
        "nonfinite or overflowing",
    );
    let zero = [[0.; 4]; 3];
    let result = attachment::evaluate(
        &s,
        &a,
        "zero",
        Request {
            attachment_parent_to_node: zero,
            ..r
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.root_to_skeleton_source,
        [[0., 0., 0., -9.], [0., 0., 0., 10.], [0., 0., 0., 15.]]
    );
}
#[test]
fn exact_result_work_depth_and_input_budgets_with_one_under() {
    let (s, a) = pair();
    let r = request(&s, &a);
    let result = attachment::evaluate(&s, &a, "baseline", r, Default::default()).unwrap();
    let limits = Limits {
        array_bytes: result.retained_bytes,
        work_units: result.work_units,
        ancestry_depth: 2,
        combined_input_bytes: s.len() + a.len(),
        ..Default::default()
    };
    assert!(attachment::evaluate(&s, &a, "exact", r, limits).is_ok());
    failure(
        &s,
        &a,
        r,
        Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        "array storage budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            work_units: limits.work_units - 1,
            ..limits
        },
        "work budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            ancestry_depth: 1,
            ..limits
        },
        "ancestry depth budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            combined_input_bytes: limits.combined_input_bytes - 1,
            ..limits
        },
        "input byte budget",
    );
    let scene = fallout_data::nif_scene::Limits {
        input_bytes: s.len().max(a.len()),
        ..limits.scene
    };
    assert!(attachment::evaluate(&s, &a, "per source", r, Limits { scene, ..limits }).is_ok());
    failure(
        &s,
        &a,
        r,
        Limits {
            scene: fallout_data::nif_scene::Limits {
                input_bytes: scene.input_bytes - 1,
                ..scene
            },
            ..limits
        },
        "input byte budget",
    );
}
#[test]
fn changed_attachment_keeps_skeleton_mapping_independent_and_changes_root_pose() {
    let (s, a) = pair();
    let first = attachment::evaluate(&s, &a, "first", request(&s, &a), Default::default()).unwrap();
    let changed = asset([11., -20., 30.]);
    let second = attachment::evaluate(
        &s,
        &changed,
        "second",
        request(&s, &changed),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        first.attachment_source_to_skeleton_source,
        second.attachment_source_to_skeleton_source
    );
    assert_ne!(first.attachment_sha256, second.attachment_sha256);
    assert_eq!(second.root_to_skeleton_source[1][3], 25.);
}
