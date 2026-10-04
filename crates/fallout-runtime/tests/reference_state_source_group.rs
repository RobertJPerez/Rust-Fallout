mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore, world::Transform};
use fallout_runtime::{
    Error, Limits, World,
    foreign::{Content, Failure as ContentFailure},
    identity::CampaignId,
    reference_state::{
        Pose, SourceReferenceFailure as Failure, SourceReferenceGroupLimits as GroupLimits,
        SourceReferenceLimits, SourceReferenceOutcome as Outcome,
        SourceReferenceRequest as Request, State,
    },
    save::{Captured, Recovery, Repository, format},
    snapshot::Snapshot,
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &unit(&[(42, 0)], &[])),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(b"CELL", 0x401, plugin::DELETED, &field(b"DATA", &[1])),
            record(b"CELL", 0x402, 0, &field(b"DATA", &[1])),
            record(b"REFR", 0x500, 0, &[]),
            record(b"ACHR", 0x501, 0, &[]),
            record(b"ACRE", 0x502, 0, &[]),
            record(b"REFR", 0x503, plugin::DELETED, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn source(root: &Path, order: &[&str]) -> (Catalogue, Content) {
    let mut store = RecordStore::open_nv_headers(
        root,
        &order.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn pose() -> Pose {
    Pose::from_source(
        &Transform {
            position: [1.0, -0.0, f32::from_bits(1)],
            rotation: [0.5, -1.0, 2.0],
        },
        Some(1.25),
    )
    .unwrap()
}
fn retained() -> State {
    State::new(
        form(0x402),
        Pose::from_source(
            &Transform {
                position: [8192.25, -0.0, -30.5],
                rotation: [0.125, -0.75, 1.5],
            },
            None,
        )
        .unwrap(),
        false,
    )
    .unwrap()
}
struct Inputs {
    keys: [fallout_data::identity::FormKey; 3],
    cell: fallout_data::identity::FormKey,
    pose: Pose,
}
impl Inputs {
    fn new() -> Self {
        Self {
            keys: [form(0x501), form(0x500), form(0x502)],
            cell: form(0x400),
            pose: pose(),
        }
    }
    fn requests(&self) -> [Request<'_>; 3] {
        static ACHR: [[u8; 4]; 1] = [*b"ACHR"];
        static REFR: [[u8; 4]; 1] = [*b"REFR"];
        static ACRE: [[u8; 4]; 1] = [*b"ACRE"];
        [
            Request {
                authored: &self.keys[0],
                allowed_kinds: &ACHR,
                cell: &self.cell,
                pose: &self.pose,
                enabled: false,
            },
            Request {
                authored: &self.keys[1],
                allowed_kinds: &REFR,
                cell: &self.cell,
                pose: &self.pose,
                enabled: true,
            },
            Request {
                authored: &self.keys[2],
                allowed_kinds: &ACRE,
                cell: &self.cell,
                pose: &self.pose,
                enabled: true,
            },
        ]
    }
}
fn host(catalogue: &Catalogue, limits: Limits) -> World<'_> {
    World::with_campaign(
        catalogue,
        limits,
        CampaignId::from_bytes([0x17; 16]).unwrap(),
    )
    .unwrap()
}
fn prepared<'a>(catalogue: &'a Catalogue, content: &Content, limits: Limits) -> World<'a> {
    let mut world = host(catalogue, limits);
    world.register_reference(None).unwrap();
    let key = form(0x500);
    let cell = form(0x400);
    let pose = pose();
    let stage = world
        .stage_source_reference(
            Request {
                authored: &key,
                allowed_kinds: &[*b"REFR"],
                cell: &cell,
                pose: &pose,
                enabled: true,
            },
            content,
            SourceReferenceLimits::default(),
        )
        .unwrap();
    let initial = world.commit_source_reference(stage).unwrap();
    world
        .commit_reference_state(
            world
                .stage_reference_state(initial.view(), retained())
                .unwrap(),
        )
        .unwrap();
    world
}
fn literal_expected(before: &Snapshot) -> Snapshot {
    let mut value = serde_json::to_value(before).unwrap();
    value["state_revision"] = json!(4);
    value["next_reference"] = json!(5);
    value["references"] = json!([
        {"id":1,"authored":null}, {"id":2,"authored":form(0x500)},
        {"id":3,"authored":form(0x501)}, {"id":4,"authored":form(0x502)}]);
    value["reference_states"] = json!([
        {"id":2,"state":retained()},
        {"id":3,"state":{"schema_version":1,"cell":form(0x400),
            "pose":{"position_bits":[1065353216_u32,2147483648_u32,1],"rotation_bits":[1056964608,3212836864_u32,1073741824],"scale_bits":1067450368},"enabled":false}},
        {"id":4,"state":{"schema_version":1,"cell":form(0x400),
            "pose":{"position_bits":[1065353216_u32,2147483648_u32,1],"rotation_bits":[1056964608,3212836864_u32,1073741824],"scale_bits":1067450368},"enabled":true}}
    ]);
    serde_json::from_value(value).unwrap()
}

#[test]
fn mixed_group_is_one_atomic_revision_with_ordered_ids_and_current_retained_views() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = prepared(&catalogue, &content, Limits::default());
    let input = Inputs::new();
    let before = world.snapshot();
    assert_eq!(before.state_revision, 3);
    assert_eq!(before.next_reference, 3);
    let dropped = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    assert_eq!(dropped.usage().created, 2);
    assert_eq!(dropped.usage().reused, 1);
    assert_eq!(dropped.usage().source_checks, 6);
    assert_eq!(dropped.usage().allowed_kinds, 3);
    assert_eq!(
        dropped.rows()[1].existing().unwrap().state(),
        Some(&retained())
    );
    assert_eq!(dropped.rows()[1].requested_state().cell(), &form(0x400));
    drop(dropped);
    assert_eq!(world.snapshot(), before);
    let receipt = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        (receipt.before_revision(), receipt.after_revision()),
        (3, 4)
    );
    assert_eq!(
        receipt
            .rows()
            .iter()
            .map(|r| (r.outcome(), r.view().reference().0.get()))
            .collect::<Vec<_>>(),
        [
            (Outcome::Created, 3),
            (Outcome::Reused, 2),
            (Outcome::Created, 4)
        ]
    );
    assert_eq!(world.snapshot(), literal_expected(&before));
    for row in receipt.rows() {
        assert_eq!(row.view().revision(), 4);
        assert_eq!(
            serde_json::to_value(row.view()).unwrap(),
            serde_json::to_value(world.reference_view(row.view().reference()).unwrap()).unwrap()
        );
        drop(
            world
                .stage_reference_state(row.view(), row.view().state().unwrap().clone())
                .unwrap(),
        );
    }
    assert_eq!(receipt.rows()[1].view().state(), Some(&retained()));
    let after = world.snapshot();
    let reused = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(reused.usage().created, 0);
    assert_eq!(reused.usage().reused, 3);
    assert_eq!((reused.before_revision(), reused.after_revision()), (4, 4));
    assert_eq!(world.snapshot(), after);
}

#[test]
fn bad_final_sources_cells_kinds_poses_and_duplicates_publish_nothing() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let world = prepared(&catalogue, &content, Limits::default());
    let input = Inputs::new();
    let before = world.snapshot();
    for key in [form(0x100), form(0x503), form(0x999), form(0x500)] {
        let mut requests = input.requests();
        requests[2].authored = &key;
        assert!(
            world
                .stage_source_reference_group(&requests, &content, GroupLimits::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    for cell in [form(0x100), form(0x401), form(0x999)] {
        let mut requests = input.requests();
        requests[2].cell = &cell;
        assert!(
            world
                .stage_source_reference_group(&requests, &content, GroupLimits::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    for kinds in [
        vec![],
        vec![*b"ACTI"],
        vec![*b"REFR"],
        vec![*b"ACRE", *b"ACRE"],
    ] {
        let mut requests = input.requests();
        requests[2].allowed_kinds = &kinds;
        assert!(
            world
                .stage_source_reference_group(&requests, &content, GroupLimits::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    for bad in [
        json!({"position_bits":[2139095040,0,0],"rotation_bits":[0,0,0],"scale_bits":null}),
        json!({"position_bits":[0,0,0],"rotation_bits":[0,2143289344_u32,0],"scale_bits":null}),
        json!({"position_bits":[0,0,0],"rotation_bits":[0,0,0],"scale_bits":2147483648_u32}),
    ] {
        let pose: Pose = serde_json::from_value(bad).unwrap();
        let mut requests = input.requests();
        requests[2].pose = &pose;
        assert!(matches!(
            world.stage_source_reference_group(&requests, &content, GroupLimits::default()),
            Err(Failure::State(Error::Invalid(_)))
        ));
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn all_explicit_work_and_copy_bounds_have_exact_and_one_under_exits() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let world = prepared(&catalogue, &content, Limits::default());
    let input = Inputs::new();
    let before = world.snapshot();
    let usage = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap()
        .usage();
    let exact = GroupLimits {
        max_requests: 3,
        max_allowed_kinds_per_request: 1,
        max_allowed_kinds: 3,
        max_source_checks: 6,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .stage_source_reference_group(&input.requests(), &content, exact)
            .unwrap()
            .usage(),
        usage
    );
    for (limits, reason) in [
        (
            GroupLimits {
                max_requests: 2,
                ..exact
            },
            "source reference group requests",
        ),
        (
            GroupLimits {
                max_allowed_kinds_per_request: 0,
                ..exact
            },
            "source reference kinds",
        ),
        (
            GroupLimits {
                max_allowed_kinds: 2,
                ..exact
            },
            "source reference group kinds",
        ),
        (
            GroupLimits {
                max_source_checks: 5,
                ..exact
            },
            "source reference group source checks",
        ),
        (
            GroupLimits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            },
            "source reference group copied bytes",
        ),
    ] {
        assert!(
            matches!(world.stage_source_reference_group(&input.requests(),&content,limits),Err(Failure::State(Error::Capacity(actual))) if actual==reason)
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn cumulative_registry_and_whole_allocator_span_are_checked_before_staging() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let input = Inputs::new();
    let short = prepared(
        &catalogue,
        &content,
        Limits {
            max_references: 3,
            ..Limits::default()
        },
    );
    let before = short.snapshot();
    assert!(matches!(
        short.stage_source_reference_group(&input.requests(), &content, GroupLimits::default()),
        Err(Failure::State(Error::Capacity("live references")))
    ));
    assert_eq!(short.snapshot(), before);
    let mut exact = prepared(
        &catalogue,
        &content,
        Limits {
            max_references: 4,
            ..Limits::default()
        },
    );
    exact
        .commit_source_reference_group(
            exact
                .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(exact.reference_count(), 4);
    for (next, revision, reason) in [
        (u64::MAX - 1, 3, "reference identities"),
        (3, u64::MAX, "state revisions"),
    ] {
        let mut world = prepared(&catalogue, &content, Limits::default());
        let mut before = world.snapshot();
        before.next_reference = next;
        before.state_revision = revision;
        world.replace_from_snapshot(before.clone()).unwrap();
        assert!(
            matches!(world.stage_source_reference_group(&input.requests(),&content,GroupLimits::default()),Err(Failure::State(Error::Capacity(actual))) if actual==reason)
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut world = prepared(&catalogue, &content, Limits::default());
    let mut before = world.snapshot();
    before.next_reference = u64::MAX - 2;
    world.replace_from_snapshot(before).unwrap();
    let receipt = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.rows()[0].view().reference().0.get(), u64::MAX - 2);
    assert_eq!(receipt.rows()[2].view().reference().0.get(), u64::MAX - 1);
    assert_eq!(world.snapshot().next_reference, u64::MAX);
}

#[test]
fn empty_and_legacy_reused_groups_are_noops_even_with_exhausted_counters() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(
        &catalogue,
        Limits {
            max_references: 1,
            ..Limits::default()
        },
    );
    let id = world.register_reference(Some(form(0x500))).unwrap();
    let mut exact = world.snapshot();
    exact.state_revision = u64::MAX;
    exact.next_reference = u64::MAX;
    world.replace_from_snapshot(exact.clone()).unwrap();
    let input = Inputs::new();
    let rows = [input.requests()[1]];
    let reused = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&rows, &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(reused.rows()[0].view().reference(), id);
    assert_eq!(reused.rows()[0].view().state(), None);
    assert_eq!(
        (reused.before_revision(), reused.after_revision()),
        (u64::MAX, u64::MAX)
    );
    assert_eq!(world.snapshot(), exact);
    let empty = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&[], &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    assert!(empty.rows().is_empty());
    assert_eq!(empty.usage().source_checks, 0);
    assert_eq!(empty.usage().created, 0);
    assert_eq!(empty.after_revision(), u64::MAX);
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn any_global_change_restore_peer_commit_campaign_or_source_expires_the_whole_group() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = prepared(&catalogue, &content, Limits::default());
    let input = Inputs::new();
    let stale = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    world.register_reference(None).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_source_reference_group(stale).is_err());
    assert_eq!(world.snapshot(), exact);
    let stale = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    world.replace_from_snapshot(exact.clone()).unwrap();
    assert!(matches!(
        world.commit_source_reference_group(stale),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
    let stale = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    let mut other_snapshot = exact.clone();
    other_snapshot.campaign = CampaignId::from_bytes([0x18; 16]).unwrap();
    let mut other = World::restore(&catalogue, other_snapshot.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_source_reference_group(stale),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), other_snapshot);
    fs::write(root.path().join("Other.esm"), header(&[])).unwrap();
    let (changed_catalogue, changed_content) = source(root.path(), &["FalloutNV.esm", "Other.esm"]);
    assert!(matches!(
        world.stage_source_reference_group(
            &input.requests(),
            &changed_content,
            GroupLimits::default()
        ),
        Err(Failure::Content(ContentFailure::ContentChanged))
    ));
    let mut changed = host(&changed_catalogue, Limits::default());
    let changed_before = changed.snapshot();
    assert!(matches!(
        changed.stage_source_reference_group(&input.requests(), &content, GroupLimits::default()),
        Err(Failure::Content(ContentFailure::ContentChanged))
    ));
    let stale = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    assert!(matches!(
        changed.commit_source_reference_group(stale),
        Err(Error::StaleHandle)
    ));
    assert_eq!(changed.snapshot(), changed_before);
    let winner = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    let loser = world
        .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
        .unwrap();
    world.commit_source_reference_group(winner).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_source_reference_group(loser).is_err());
    assert_eq!(world.snapshot(), exact);
    let empty = world
        .stage_source_reference_group(&[], &content, GroupLimits::default())
        .unwrap();
    world.register_reference(None).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_source_reference_group(empty).is_err());
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn native_before_current_and_two_fresh_source_consumers_preserve_complete_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let retained_root =
        std::env::var_os("FALLOUT_SOURCE_GROUP_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained_root.as_deref().unwrap_or(temp.path());
    fixture(root);
    let bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    let (catalogue, content) = source(root, &["FalloutNV.esm"]);
    let mut world = prepared(&catalogue, &content, Limits::default());
    let input = Inputs::new();
    let before = world.snapshot();
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let receipt = world
        .commit_source_reference_group(
            world
                .stage_source_reference_group(&input.requests(), &content, GroupLimits::default())
                .unwrap(),
        )
        .unwrap();
    let after = world.snapshot();
    assert_eq!(after, literal_expected(&before));
    let capture = Captured::at_boundary(&world);
    world.register_reference(None).unwrap();
    repository.commit(&capture).unwrap();
    for (name, value) in [
        ("before.snapshot.json", &before),
        ("current.snapshot.json", &after),
    ] {
        fs::write(root.join(name), value.encode(1 << 20).unwrap()).unwrap();
    }
    fs::write(
        root.join("group.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(root.join("group.input.json"),serde_json::to_vec_pretty(&json!({
        "existing":{"authored":form(0x500),"allowed_kinds":[*b"REFR"],"cell":form(0x400),"pose":pose(),"enabled":true},
        "retained_state":retained(),"requests":input.requests().iter().map(|r|json!({"authored":r.authored,"allowed_kinds":r.allowed_kinds,"cell":r.cell,"pose":r.pose,"enabled":r.enabled})).collect::<Vec<_>>()
    })).unwrap()).unwrap();
    fs::write(root.join("load-order.json"), br#"["FalloutNV.esm"]"#).unwrap();
    drop(world);
    drop(content);
    drop(catalogue);
    for mode in ["previous", "current"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_source_group_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_SOURCE_GROUP_COLD_ROOT", root)
            .env("FALLOUT_SOURCE_GROUP_COLD_MODE", mode)
            .output()
            .unwrap();
        fs::write(root.join(format!("cold-{mode}.stdout.txt")), &result.stdout).unwrap();
        fs::write(root.join(format!("cold-{mode}.stderr.txt")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), bytes);
}

#[test]
#[ignore = "fresh process selected explicitly by the source group parent"]
fn cold_source_group_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_SOURCE_GROUP_COLD_ROOT").unwrap());
    let mode = std::env::var("FALLOUT_SOURCE_GROUP_COLD_MODE").unwrap();
    let (catalogue, content) = source(&root, &["FalloutNV.esm"]);
    let expected = Snapshot::decode(
        &fs::read(root.join(format!(
            "{}.snapshot.json",
            if mode == "current" {
                "current"
            } else {
                "before"
            }
        )))
        .unwrap(),
        Limits::default(),
    )
    .unwrap();
    let mut world = if mode == "current" {
        let (world, receipt) = Repository::open(&root.join("saved"), &[])
            .unwrap()
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap();
        assert_eq!(receipt.metadata.generation, 2);
        world
    } else {
        let decoded = format::decode(
            &fs::read(root.join("saved/previous.frsv")).unwrap(),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(decoded.metadata.generation, 1);
        World::restore(&catalogue, decoded.snapshot, Limits::default()).unwrap()
    };
    assert_eq!(world.snapshot(), expected);
    fs::write(
        root.join(format!("cold-{mode}.snapshot.json")),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    if mode == "current" {
        let input = Inputs::new();
        let reused = world
            .commit_source_reference_group(
                world
                    .stage_source_reference_group(
                        &input.requests(),
                        &content,
                        GroupLimits::default(),
                    )
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(reused.usage().created, 0);
        assert_eq!(reused.rows()[1].view().state(), Some(&retained()));
        assert_eq!(world.snapshot(), expected);
        fs::write(
            root.join("cold-reused.receipt.json"),
            serde_json::to_vec_pretty(&reused).unwrap(),
        )
        .unwrap();
    }
}
