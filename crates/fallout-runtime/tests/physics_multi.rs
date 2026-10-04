mod common;
use common::{field, form, header, record};
use fallout_data::{
    archive::NvArchive,
    coordinates::Affine,
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        preparation::CellModelPlan,
        residency::{self, CellResidency, Stage},
    },
};
use fallout_runtime::{
    identity::ReferenceId,
    physics::{
        multi::{self, CellSelection, ModelSelection},
        *,
    },
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    num::NonZeroU64,
    time::{Duration, Instant},
};

fn group(kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn nif(radius: f32, material: u32) -> Vec<u8> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&1u32.to_le_bytes());
    body[4..8].copy_from_slice(&[7, 0xa5, 0x34, 0x12]);
    body[80..84].copy_from_slice(&1f32.to_le_bytes());
    container(&[
        ("bhkRigidBody", body),
        (
            "bhkSphereShape",
            [material.to_le_bytes(), radius.to_le_bytes()].concat(),
        ),
    ])
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend(0x14020007u32.to_le_bytes());
    bytes.push(1);
    for v in [11u32, blocks.len() as u32, 34] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend([0; 3]);
    bytes.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    for i in 0..blocks.len() {
        bytes.extend((i as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        bytes.extend((data.len() as u32).to_le_bytes());
    }
    for v in [0u32, 0, 0] {
        bytes.extend(v.to_le_bytes());
    }
    for (_, data) in blocks {
        bytes.extend(data);
    }
    bytes.extend(0u32.to_le_bytes());
    bytes
}
fn triangle_nif() -> Vec<u8> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&1u32.to_le_bytes());
    body[80..84].copy_from_slice(&1f32.to_le_bytes());
    let mut shape = vec![0; 56];
    for at in [16, 20, 24, 36, 40, 44] {
        shape[at..at + 4].copy_from_slice(&1f32.to_le_bytes());
    }
    shape[52..56].copy_from_slice(&2u32.to_le_bytes());
    let mut data = 1u32.to_le_bytes().to_vec();
    for v in [0u16, 1, 2, 0xe123] {
        data.extend(v.to_le_bytes());
    }
    data.extend(3u32.to_le_bytes());
    data.push(0);
    // Source plane X=0, with the origin strictly inside the triangle.
    for p in [[0f32, -2., -2.], [0., 2., -2.], [0., 0., 2.]] {
        for v in p {
            data.extend(v.to_le_bytes());
        }
    }
    data.extend(1u16.to_le_bytes());
    data.extend([9, 0xc3, 0x78, 0x56]);
    data.extend(3u32.to_le_bytes());
    data.extend(29u32.to_le_bytes());
    container(&[
        ("bhkRigidBody", body),
        ("bhkPackedNiTriStripsShape", shape),
        ("hkPackedNiTriStripsData", data),
    ])
}
fn extra_index_nif(long_name: bool) -> Vec<u8> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&1u32.to_le_bytes());
    body[80..84].copy_from_slice(&1f32.to_le_bytes());
    let name = if long_name {
        // Stay inside the existing source index's 1024-byte type-name ceiling.
        format!("bhk{}", "X".repeat(900))
    } else {
        "NiObject".to_owned()
    };
    if long_name {
        body.truncate(228);
        body.extend(32u32.to_le_bytes());
        for _ in 0..32 {
            body.extend(2u32.to_le_bytes());
        }
        body.extend(0u32.to_le_bytes());
    }
    container(&[
        ("bhkRigidBody", body),
        (
            "bhkSphereShape",
            [17u32.to_le_bytes(), 1f32.to_le_bytes()].concat(),
        ),
        (&name, Vec::new()),
    ])
}
struct Fixture {
    source: tempfile::TempDir,
    cache: tempfile::TempDir,
    plan: CellModelPlan,
    models: [Vec<u8>; 2],
}
impl Fixture {
    fn new(mode: usize) -> Self {
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let models = if mode >= 4 {
            [extra_index_nif(mode == 5), extra_index_nif(mode == 5)]
        } else if mode == 3 {
            [triangle_nif(), triangle_nif()]
        } else {
            [
                nif(1., 17),
                nif(
                    if mode == 1 {
                        -1.
                    } else if mode == 2 {
                        1.
                    } else {
                        2.
                    },
                    if mode == 2 { 17 } else { 23 },
                ),
            ]
        };
        let names = b"sphere-a.nif\0sphere-b.nif\0";
        let offset = 92 + names.len();
        let mut bsa = vec![0; offset];
        bsa[..4].copy_from_slice(b"BSA\0");
        for (at, v) in [
            (4, 104u32),
            (8, 36),
            (12, 3),
            (16, 1),
            (20, 2),
            (24, 7),
            (28, names.len() as u32),
            (44, 2),
            (48, 52),
            (68, models[0].len() as u32),
            (72, offset as u32),
            (84, models[1].len() as u32),
            (88, (offset + models[0].len()) as u32),
        ] {
            bsa[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        bsa[52] = 7;
        bsa[53..60].copy_from_slice(b"meshes\0");
        bsa[60..68].copy_from_slice(&1u64.to_le_bytes());
        bsa[76..84].copy_from_slice(&2u64.to_le_bytes());
        bsa[92..].copy_from_slice(names);
        for bytes in &models {
            bsa.extend(bytes);
        }
        let archive = source.path().join("models.bsa");
        fs::write(&archive, bsa).unwrap();
        let mut mounts = MountIndex::default();
        NvArchive::open(&archive)
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        let mut esm = header(&[]);
        for (id, name) in [(0x400, b"sphere-a.nif\0"), (0x401, b"sphere-b.nif\0")] {
            esm.extend(record(b"STAT", id, 0, &field(b"MODL", name)));
        }
        esm.extend(record(
            b"CELL",
            0x200,
            0,
            &[
                field(b"EDID", b"MultiCollisionFixture\0"),
                field(b"DATA", &[1]),
            ]
            .concat(),
        ));
        let mut members = Vec::new();
        for (id, base) in [(0x300, 0x400u32), (0x301, 0x401u32)] {
            members.extend(record(
                b"REFR",
                id,
                0,
                &[
                    field(b"NAME", &base.to_le_bytes()),
                    field(b"DATA", &[0; 24]),
                ]
                .concat(),
            ));
        }
        esm.extend(group(6, &group(9, &members)));
        fs::write(source.path().join("FalloutNV.esm"), esm).unwrap();
        let mut store = RecordStore::open_nv_headers(
            source.path(),
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let plan =
            CellModelPlan::load(&mut store, &form(0x200), &mounts, Default::default()).unwrap();
        assert_eq!(plan.receipt().requests.len(), 2);
        assert_eq!(
            plan.receipt().requests[0].path.bytes(),
            b"meshes/sphere-a.nif"
        );
        Self {
            source,
            cache,
            plan,
            models,
        }
    }
    fn owner(&self) -> CellResidency {
        CellResidency::new(
            self.source.path(),
            Some(self.cache.path()),
            residency::Limits {
                workers: 1,
                models: 2,
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn selections(&self, per_model: usize) -> Vec<ModelSelection> {
        (0..2)
            .map(|model_index| ModelSelection {
                model_index,
                placements: (0..per_model)
                    .map(|i| BodyPlacement {
                        reference: ReferenceId(
                            NonZeroU64::new(
                                0xf000_0000_0000_1000 + (model_index as u64) * 0x100 + i as u64,
                            )
                            .unwrap(),
                        ),
                        source_sha256: Sha256::digest(&self.models[model_index]).into(),
                        body_block: 0,
                        attachment_to_source: Affine {
                            rows: [
                                [1., 0., 0., (model_index as f64) * 5. + (i as f64) * 10.],
                                [0., 1., 0., 0.],
                                [0., 0., 1., 0.],
                            ],
                        },
                    })
                    .collect(),
            })
            .collect()
    }
}
fn decoded(owner: &mut CellResidency) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let state = owner.poll().unwrap();
        if state.stage == Stage::Decoded {
            break;
        }
        assert_ne!(state.stage, Stage::Failed);
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn units() -> EngineeringUnits {
    EngineeringUnits {
        havok_to_source: 1.,
        source_to_query: 1.,
        transform_tolerance: 1e-6,
    }
}
fn ray() -> Ray {
    Ray {
        origin: [-3., 0., 0.],
        direction: [1., 0., 0.],
        max_distance: 100.,
    }
}

#[test]
fn literal_two_models_have_interleaved_global_hits_and_preserved_source_metadata() {
    let fixture = Fixture::new(0);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut selection = CellSelection::default();
    let scope = selection
        .admit(
            &mut owner,
            &ticket,
            &fixture.selections(2),
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    assert_eq!(scope.usage.blocks, 4);
    assert_eq!(scope.usage.primitives, 4);
    assert_eq!(scope.usage.shape_visits, 8);
    assert_eq!(scope.usage.geometry_elements, 2);
    let result = selection
        .ray_cast(&owner, ray(), QueryBudget::default())
        .unwrap();
    let hits = result.hits(&owner).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.distance).collect::<Vec<_>>(),
        vec![2., 6., 12., 16.]
    );
    assert_eq!(
        hits.iter().map(|h| h.material).collect::<Vec<_>>(),
        vec![17, 23, 17, 23]
    );
    assert_eq!(
        hits.iter()
            .map(|h| h.source.reference.0.get())
            .collect::<Vec<_>>(),
        vec![
            0xf000_0000_0000_1000,
            0xf000_0000_0000_1100,
            0xf000_0000_0000_1001,
            0xf000_0000_0000_1101
        ]
    );
    for hit in hits {
        assert_eq!(hit.body_filter.layer, 7);
        assert_eq!(hit.body_filter.flags_and_parts, 0xa5);
        assert_eq!(hit.body_filter.group, 0x1234);
    }
    let overlaps = selection
        .overlap_sphere(&owner, [7., 0., 0.], 20., QueryBudget::default())
        .unwrap();
    let keys = overlaps
        .hits(&owner)
        .unwrap()
        .iter()
        .map(|h| h.source.clone())
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 4);
    assert!(keys.windows(2).all(|p| p[0] < p[1]));
    assert_eq!(
        owner.snapshot().collision,
        residency::Readiness::Unsupported
    );
    assert!(!owner.snapshot().simulation_ready);
}

#[test]
fn aggregate_exact_build_and_query_boundaries_do_not_multiply_per_model() {
    let fixture = Fixture::new(0);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let sources = fixture.models.iter().map(Vec::len).sum::<usize>();
    for case in 0..9 {
        let mut limits = multi::Limits {
            source_bytes: sources,
            models: 2,
            placements: 4,
            scene: QueryLimits {
                blocks: 4,
                shape_visits: 8,
                primitives: 4,
                geometry_elements: 2,
            },
            ..Default::default()
        };
        match case {
            1 => limits.source_bytes -= 1,
            2 => limits.models -= 1,
            3 => limits.placements -= 1,
            4 => limits.scene.blocks -= 1,
            5 => limits.scene.shape_visits -= 1,
            6 => limits.scene.primitives -= 1,
            7 => limits.scene.geometry_elements -= 1,
            8 => limits.decoded_metadata_bytes = 0,
            _ => {}
        }
        let mut selection = CellSelection::default();
        let outcome = selection.admit(&mut owner, &ticket, &fixture.selections(2), units(), limits);
        if case == 0 {
            outcome.unwrap();
            let exact = QueryBudget {
                primitive_tests: 4,
                geometry_tests: 0,
                hits: 4,
            };
            assert_eq!(
                selection
                    .ray_cast(&owner, ray(), exact)
                    .unwrap()
                    .hits(&owner)
                    .unwrap()
                    .len(),
                4
            );
            assert!(
                selection
                    .ray_cast(
                        &owner,
                        ray(),
                        QueryBudget {
                            primitive_tests: 3,
                            ..exact
                        }
                    )
                    .is_err()
            );
            assert!(
                selection
                    .ray_cast(&owner, ray(), QueryBudget { hits: 3, ..exact })
                    .is_err()
            );
            assert_eq!(
                selection
                    .ray_cast(&owner, ray(), exact)
                    .unwrap()
                    .hits(&owner)
                    .unwrap()
                    .len(),
                4
            );
        } else {
            assert!(outcome.is_err(), "case {case}");
            assert_eq!(selection.retained_primitive_count(), 0);
        }
    }
}

#[test]
fn final_combined_index_and_shared_source_bytes_remain_distinct_and_bounded() {
    for mode in [0, 2] {
        let fixture = Fixture::new(mode);
        let mut owner = fixture.owner();
        let ticket = owner.request(fixture.plan.clone()).unwrap();
        decoded(&mut owner);
        let mut selection = CellSelection::default();
        let scope = selection
            .admit(
                &mut owner,
                &ticket,
                &fixture.selections(4),
                units(),
                multi::Limits::default(),
            )
            .unwrap();
        assert_eq!(scope.usage.geometry_elements, 17); // Two source leaves plus 2*8-1 final nodes.
        if mode == 2 {
            assert_eq!(scope.models[0].source_sha256, scope.models[1].source_sha256);
        }
        let exact = QueryBudget {
            primitive_tests: 8,
            geometry_tests: 0,
            hits: 8,
        };
        let hits = selection.ray_cast(&owner, ray(), exact).unwrap();
        let expected = if mode == 2 {
            vec![2., 7., 12., 17., 22., 27., 32., 37.]
        } else {
            vec![2., 6., 12., 16., 22., 26., 32., 36.]
        };
        assert_eq!(
            hits.hits(&owner)
                .unwrap()
                .iter()
                .map(|h| h.distance)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            selection
                .ray_cast(
                    &owner,
                    ray(),
                    QueryBudget {
                        primitive_tests: 7,
                        ..exact
                    }
                )
                .is_err()
        );
        let mut duplicate = fixture.selections(4);
        duplicate.push(duplicate[0].clone());
        assert!(
            selection
                .admit(
                    &mut owner,
                    &ticket,
                    &duplicate,
                    units(),
                    multi::Limits::default()
                )
                .is_err()
        );
        assert_eq!(selection.retained_primitive_count(), 0);
        assert!(hits.hits(&owner).is_err());
    }
}

#[test]
fn late_unsupported_model_and_forged_source_leave_no_partial_geometry() {
    let fixture = Fixture::new(1);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let inputs = fixture.selections(2);
    let mut selection = CellSelection::default();
    selection
        .admit(
            &mut owner,
            &ticket,
            &inputs[..1],
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    let old = selection
        .ray_cast(&owner, ray(), QueryBudget::default())
        .unwrap();
    assert!(
        selection
            .admit(
                &mut owner,
                &ticket,
                &inputs,
                units(),
                multi::Limits::default()
            )
            .is_err()
    );
    assert_eq!(selection.retained_primitive_count(), 0);
    assert!(old.hits(&owner).is_err());
    let mut forged = inputs;
    forged[1].placements[0].source_sha256 = [0; 32];
    assert!(
        selection
            .admit(
                &mut owner,
                &ticket,
                &forged,
                units(),
                multi::Limits::default()
            )
            .is_err()
    );
    assert_eq!(selection.retained_primitive_count(), 0);
}

#[test]
fn original_owner_and_selection_lease_revoke_all_receipts_and_pins() {
    for mode in 0..3 {
        let fixture = Fixture::new(0);
        let mut owner = fixture.owner();
        let ticket = owner.request(fixture.plan.clone()).unwrap();
        decoded(&mut owner);
        let mut selection = CellSelection::default();
        selection
            .admit(
                &mut owner,
                &ticket,
                &fixture.selections(2),
                units(),
                multi::Limits::default(),
            )
            .unwrap();
        let hits = selection
            .ray_cast(&owner, ray(), QueryBudget::default())
            .unwrap();
        let mut foreign = fixture.owner();
        foreign.request(fixture.plan.clone()).unwrap();
        decoded(&mut foreign);
        assert!(hits.hits(&foreign).is_err());
        assert!(hits.scope(&foreign).is_err());
        match mode {
            0 => selection.release(&mut owner).unwrap(),
            1 => drop(selection),
            2 => {
                owner.unload().unwrap();
                assert!(selection.invalidate(&owner));
                assert_eq!(selection.retained_primitive_count(), 0);
            }
            _ => unreachable!(),
        }
        assert!(hits.hits(&owner).is_err());
        owner.unload().unwrap();
        let after = owner.poll().unwrap();
        assert_eq!(after.pinned_source_bytes, 0);
        assert_eq!(after.retained_plans, 0);
        assert_eq!(after.outstanding, 0);
    }
}

#[test]
fn geometry_work_is_one_global_triangle_allowance_across_both_models() {
    let fixture = Fixture::new(3);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut selection = CellSelection::default();
    let scope = selection
        .admit(
            &mut owner,
            &ticket,
            &fixture.selections(2),
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    assert_eq!(scope.usage.blocks, 6);
    assert_eq!(scope.usage.geometry_elements, 10);
    let exact = QueryBudget {
        primitive_tests: 4,
        geometry_tests: 4,
        hits: 4,
    };
    let result = selection.ray_cast(&owner, ray(), exact).unwrap();
    let hits = result.hits(&owner).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.distance).collect::<Vec<_>>(),
        vec![3., 8., 13., 18.]
    );
    for h in hits {
        assert_eq!(h.source.shape_block, 2);
        assert_eq!(h.source.triangle, Some(0));
        assert_eq!(h.welding, Some(0xe123));
        assert_eq!(h.material, 29);
        let filter = h.shape_filter.unwrap();
        assert_eq!(filter.layer, 9);
        assert_eq!(filter.flags_and_parts, 0xc3);
        assert_eq!(filter.group, 0x5678);
    }
    assert!(
        selection
            .ray_cast(
                &owner,
                ray(),
                QueryBudget {
                    geometry_tests: 3,
                    ..exact
                }
            )
            .is_err()
    );
    assert_eq!(
        selection
            .ray_cast(&owner, ray(), exact)
            .unwrap()
            .hits(&owner)
            .unwrap()
            .len(),
        4
    );
    let overlap = selection
        .overlap_sphere(&owner, [0., 0., 0.], 20., exact)
        .unwrap();
    assert_eq!(overlap.hits(&owner).unwrap().len(), 4);
    assert!(
        selection
            .overlap_sphere(
                &owner,
                [0., 0., 0.],
                20.,
                QueryBudget {
                    geometry_tests: 3,
                    ..exact
                }
            )
            .is_err()
    );
}

#[test]
fn metadata_bounds_and_cross_model_duplicate_keys_fail_without_replacement() {
    let fixture = Fixture::new(2);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let inputs = fixture.selections(2);
    let mut selection = CellSelection::default();
    let scope = selection
        .admit(
            &mut owner,
            &ticket,
            &inputs,
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    let exact = multi::Limits {
        scope_metadata_bytes: scope.usage.scope_metadata_bytes,
        ..Default::default()
    };
    selection
        .admit(&mut owner, &ticket, &inputs, units(), exact)
        .unwrap();
    let old = selection
        .ray_cast(&owner, ray(), QueryBudget::default())
        .unwrap();
    assert!(
        selection
            .admit(
                &mut owner,
                &ticket,
                &inputs,
                units(),
                multi::Limits {
                    scope_metadata_bytes: exact.scope_metadata_bytes - 1,
                    ..exact
                }
            )
            .is_err()
    );
    assert!(old.hits(&owner).is_err());
    assert_eq!(selection.retained_primitive_count(), 0);
    let mut duplicate = inputs.clone();
    duplicate[1].placements[0].reference = duplicate[0].placements[0].reference;
    assert!(
        selection
            .admit(
                &mut owner,
                &ticket,
                &duplicate,
                units(),
                multi::Limits::default()
            )
            .is_err()
    );
    assert_eq!(selection.retained_primitive_count(), 0);
}

#[test]
fn noncollision_blocks_and_long_opaque_link_names_are_preflight_bounded() {
    let fixture = Fixture::new(4);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut selection = CellSelection::default();
    let scope = selection
        .admit(
            &mut owner,
            &ticket,
            &fixture.selections(2),
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    assert_eq!(scope.usage.blocks, 6, "noncollision NIF blocks count too");
    assert!(
        selection
            .admit(
                &mut owner,
                &ticket,
                &fixture.selections(2),
                units(),
                multi::Limits {
                    scene: QueryLimits {
                        blocks: 5,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert_eq!(selection.retained_primitive_count(), 0);
    let fixture = Fixture::new(5);
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut selection = CellSelection::default();
    let outcome = selection.admit(
        &mut owner,
        &ticket,
        &fixture.selections(2),
        units(),
        multi::Limits {
            decoded_metadata_bytes: 128 * 1024,
            ..Default::default()
        },
    );
    assert!(
        matches!(
            &outcome,
            Err(fallout_runtime::physics::cell::CellError::Query(
                QueryError::Budget("collision graph scratch")
            ))
        ),
        "{outcome:?}"
    );
    assert_eq!(selection.retained_primitive_count(), 0);
    // The ordinary full allowance preserves the same frozen source sphere core;
    // opaque constraints never become original dynamics/readiness evidence.
    selection
        .admit(
            &mut owner,
            &ticket,
            &fixture.selections(2),
            units(),
            multi::Limits::default(),
        )
        .unwrap();
    assert_eq!(
        selection
            .ray_cast(&owner, ray(), QueryBudget::default())
            .unwrap()
            .hits(&owner)
            .unwrap()
            .len(),
        4
    );
}

#[test]
#[ignore = "explicit private multi-model CLI fixture export"]
fn multi_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_MULTI_FIXTURE").expect("explicit export root"),
    );
    fs::create_dir(&root).unwrap();
    for mode in 0..4 {
        let fixture = Fixture::new(mode);
        let install = root.join(format!("install-{mode}"));
        let data = install.join("Data");
        fs::create_dir_all(&data).unwrap();
        for name in ["FalloutNV.esm", "models.bsa"] {
            fs::copy(fixture.source.path().join(name), data.join(name)).unwrap();
        }
        let models=fixture.selections(2).into_iter().map(|m| {
            let placements=m.placements.into_iter().map(|p|serde_json::json!({"reference":p.reference.0.get(),
                "body_blocks":[p.body_block],"attachment_rows":p.attachment_to_source.rows})).collect::<Vec<_>>();
            serde_json::json!({"model_index":m.model_index,"source_sha256":format!("{:x}",Sha256::digest(&fixture.models[m.model_index])),"placements":placements})
        }).collect::<Vec<_>>();
        let request = serde_json::json!({"models":models,"units":units(),"ray":{"origin":[-3.,0.,0.],"direction":[1.,0.,0.],"max_distance":100.},
            "overlap":{"center":[7.,0.,0.],"radius":20.},"io_deadline_ms":10000,"verify_unload":true});
        fs::write(
            root.join(format!("request-{mode}.json")),
            serde_json::to_vec_pretty(&request).unwrap(),
        )
        .unwrap();
    }
    fs::write(root.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
}
