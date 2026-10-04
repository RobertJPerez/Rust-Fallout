//! Authored source CELL -> actual draw admission -> Continue/cold native restore.
use super::*;
use fallout_data::{
    identity::ProfileId, plugin, store::RecordStore, world::Transform as SourceTransform,
};
use fallout_runtime::{
    identity::CampaignId,
    reference_state::{Pose, State},
};
use std::{
    fs,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Files {
    root: PathBuf,
    retain: bool,
}
impl Drop for Files {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn form(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id,
    }
}
fn field(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u16).to_le_bytes(),
        payload,
    ]
    .concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
fn group(kind: i32, payload: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(payload.len() as u32 + 24).to_le_bytes(),
        &0x400_u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
fn archive(path: &Path) {
    let payload = include_bytes!("../testdata/source-pose-triangle.packet");
    let folder = b"meshes";
    let name = b"native.nif";
    let table = 54 + folder.len();
    let offset = table + 16 + name.len() + 1;
    let mut bytes = vec![0; offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104u32),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, folder.len() as u32 + 1),
        (28, name.len() as u32 + 1),
        (44, 1),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = folder.len() as u8 + 1;
    bytes[53..53 + folder.len()].copy_from_slice(folder);
    bytes[table..table + 8].copy_from_slice(&1u64.to_le_bytes());
    bytes[table + 8..table + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[table + 12..table + 16].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[table + 16..table + 16 + name.len()].copy_from_slice(name);
    bytes.extend(payload);
    fs::write(path, bytes).unwrap();
}
fn catalogue(install: &Path) -> Arc<Catalogue> {
    let mut store = RecordStore::open_nv_headers(
        &install.join("Data"),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap())
}
fn state(world: &mut World<'_>, position: [f32; 3], enabled: bool) {
    let view = world
        .reference_view(world.authored_reference(&form(0x500)).unwrap())
        .unwrap();
    let pose = Pose::from_source(
        &SourceTransform {
            position,
            rotation: [0.; 3],
        },
        Some(1.),
    )
    .unwrap();
    let proposal = world
        .stage_reference_state(&view, State::new(form(0x400), pose, enabled).unwrap())
        .unwrap();
    world.commit_reference_state(proposal).unwrap();
}

fn authored() -> (Files, World<'static>, Repository) {
    let retained = std::env::var_os("FALLOUT_PREVIEW_NATIVE_EVIDENCE");
    let root = retained.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "fallout-preview-native-draw-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    });
    fs::create_dir(&root).unwrap();
    let files = Files {
        root: root.clone(),
        retain: retained.is_some(),
    };
    let install = root.join("install");
    fs::create_dir_all(install.join("Data")).unwrap();
    let mut esm = record(
        b"TES4",
        0,
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    esm.extend(record(b"STAT", 0x100, 0, &field(b"MODL", b"native.nif\0")));
    esm.extend(record(
        b"CELL",
        0x400,
        0,
        &[field(b"EDID", b"NativeDraw\0"), field(b"DATA", &[1])].concat(),
    ));
    let mut references = Vec::new();
    for index in 0..4 {
        let transform = [0f32, index as f32 * 10., 0., 0., 0., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        references.extend(record(
            b"REFR",
            0x500 + index,
            if index == 0 {
                plugin::INITIALLY_DISABLED
            } else {
                0
            },
            &[
                field(b"NAME", &0x100_u32.to_le_bytes()),
                field(b"DATA", &transform),
                field(b"XSCL", &1f32.to_le_bytes()),
            ]
            .concat(),
        ));
    }
    esm.extend(group(6, &group(9, &references)));
    fs::write(install.join("Data/FalloutNV.esm"), esm).unwrap();
    archive(&install.join("Data/native.bsa"));
    fs::write(root.join("order.json"), b"[\"FalloutNV.esm\"]\n").unwrap();
    let catalogue = catalogue(&install);
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0xA7; 16]).unwrap(),
    )
    .unwrap();
    for id in [0x500, 0x501, 0x502] {
        world.register_reference(Some(form(id))).unwrap();
    }
    state(&mut world, [0.; 3], true);
    let view = world
        .reference_view(world.authored_reference(&form(0x502)).unwrap())
        .unwrap();
    let pose = Pose::from_source(
        &SourceTransform {
            position: [0., 20., 0.],
            rotation: [0.; 3],
        },
        None,
    )
    .unwrap();
    let proposal = world
        .stage_reference_state(&view, State::new(form(0x400), pose, true).unwrap())
        .unwrap();
    world.commit_reference_state(proposal).unwrap();
    let repository = Repository::create(
        &root.join("native-first"),
        std::slice::from_ref(&install),
        world.campaign(),
    )
    .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let gpu_baseline = Repository::create(
        &root.join("native-baseline"),
        std::slice::from_ref(&install),
        world.campaign(),
    )
    .unwrap();
    gpu_baseline.commit(&Captured::at_boundary(&world)).unwrap();
    // Independent source-bound snapshots for fixed-camera GPU cold comparisons.
    let baseline = world.snapshot();
    for (name, position, enabled) in [
        ("native-shifted", [0., 40., 0.], true),
        ("native-hidden", [0.; 3], false),
    ] {
        state(&mut world, position, enabled);
        let repo = Repository::create(
            &root.join(name),
            std::slice::from_ref(&install),
            world.campaign(),
        )
        .unwrap();
        repo.commit(&Captured::at_boundary(&world)).unwrap();
    }
    world.replace_from_snapshot(baseline).unwrap();
    (files, world, repository)
}

fn cold(root: &Path, name: &str) {
    let install = root.join("install");
    let session = Session::load(
        &root.join(name),
        std::slice::from_ref(&install),
        catalogue(&install),
        form(0x400),
        (0x500..=0x503).map(form).collect(),
    )
    .unwrap();
    let shutdown = Shutdown::default();
    let (host, observation) = session.start([0.; 3], shutdown.clone()).unwrap();
    assert_eq!(
        observation.report.bindings[0]
            .canonical
            .as_ref()
            .unwrap()
            .reference()
            .0
            .get(),
        1
    );
    assert_eq!(
        observation.report.bindings[1].display,
        "canonical-state-unavailable"
    );
    assert_eq!(
        observation.report.bindings[2].display,
        "canonical-scale-unavailable"
    );
    assert_eq!(
        observation.report.bindings[3].display,
        "canonical-identity-unavailable"
    );
    fs::write(
        root.join(format!("cold-{name}.json")),
        serde_json::to_vec_pretty(&observation.report).unwrap(),
    )
    .unwrap();
    drop(host);
    shutdown.finish().unwrap();
}

#[test]
fn actual_cell_draw_continue_and_cold_restore_keep_canonical_identity() {
    if let Some(root) = std::env::var_os("FALLOUT_PREVIEW_NATIVE_COLD") {
        cold(
            &PathBuf::from(root),
            &std::env::var("FALLOUT_PREVIEW_NATIVE_REPOSITORY").unwrap(),
        );
        return;
    }
    let (files, mut world, repository) = authored();
    let root = &files.root;
    let install = root.join("install");
    let order = root.join("order.json");
    let shutdown = Shutdown::default();
    let request_shutdown = shutdown.clone();
    let repository_path = repository.path().to_path_buf();
    let job = crate::loading::Job::start(7, move |context| {
        crate::scene::load_cell(
            &install,
            &order,
            "NativeDraw",
            &context,
            Some(&repository_path),
            request_shutdown,
        )
        .map_err(|error| error.to_string())
    })
    .unwrap();
    let mut job = job;
    let deadline = Instant::now() + Duration::from_secs(10);
    let (prepared, report, sources) = loop {
        match job.poll(7) {
            crate::loading::Poll::Pending => {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            crate::loading::Poll::Ready(value) => break value,
            crate::loading::Poll::Failed(error) => {
                panic!("authored source CELL preparation: {error}")
            }
            _ => panic!("authored source CELL lost its result"),
        }
    };
    let crate::scene::Report::Cell(report) = report else {
        panic!("not a CELL report");
    };
    assert_eq!(report.schema_version, 4);
    assert_eq!(report.rendered_references, 1);
    assert_eq!(prepared.instances.len(), 4);
    assert_eq!(prepared.instances[0].visibility, Visibility::Inherited);
    assert_eq!(prepared.instances[0].transform, Transform::IDENTITY);
    assert!(
        prepared.instances[1..]
            .iter()
            .all(|instance| instance.visibility == Visibility::Hidden)
    );
    let original = world.snapshot();
    let queue = crate::DrawScene {
        queue: crate::upload::Queue::new(7, prepared).unwrap(),
        cell: Some(sources),
    };
    let mut app = crate::tests::loading_app(crate::Phase::Uploading(queue));
    while !matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Ready(_)
    ) {
        app.update();
    }
    let id = world.authored_reference(&form(0x500)).unwrap();
    let mut views = app
        .world_mut()
        .query::<(&crate::scene::ReferenceView, &Transform, &Visibility)>();
    let (_, transform, visible) = views
        .iter(app.world())
        .find(|(view, _, _)| view.key == form(0x500))
        .unwrap();
    assert_eq!(*transform, Transform::IDENTITY);
    assert_eq!(*visible, Visibility::Inherited);
    state(&mut world, [0., 40., 0.], true);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    app.world_mut()
        .resource_mut::<crate::input::Actions>()
        .continue_saved = true;
    app.update();
    app.world_mut()
        .resource_mut::<crate::input::Actions>()
        .continue_saved = false;
    loop {
        app.update();
        if let crate::Phase::Ready(queue) = &app.world().resource::<crate::Loading>().phase
            && queue
                .cell
                .as_ref()
                .unwrap()
                .native
                .as_ref()
                .unwrap()
                .title()
                .starts_with("Continue restored")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual host Continue did not complete"
        );
        thread::yield_now();
    }
    let (view, transform, visible) = views
        .iter(app.world())
        .find(|(view, _, _)| view.key == form(0x500))
        .unwrap();
    assert_eq!(view.canonical.as_ref().unwrap().reference(), id);
    assert_eq!(
        view.canonical.as_ref().unwrap().revision(),
        world.revision()
    );
    assert_eq!(transform.translation, Vec3::new(0., 0., -40.));
    assert_eq!(*visible, Visibility::Inherited);
    assert_ne!(world.snapshot(), original);
    let expected = world.snapshot();
    app.world_mut()
        .resource_mut::<crate::input::Actions>()
        .close = true;
    app.update();
    drop(app);
    shutdown.finish().unwrap();
    let (cold_world, _) = repository
        .load(
            catalogue(&root.join("install")),
            Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    assert_eq!(cold_world.snapshot(), expected);
    for name in ["native-first", "native-shifted", "native-hidden"] {
        let child = Command::new(std::env::current_exe().unwrap()).args(["--exact","native::render_tests::actual_cell_draw_continue_and_cold_restore_keep_canonical_identity","--nocapture"])
            .env_remove("FALLOUT_PREVIEW_NATIVE_EVIDENCE").env("FALLOUT_PREVIEW_NATIVE_COLD",root).env("FALLOUT_PREVIEW_NATIVE_REPOSITORY",name).output().unwrap();
        assert!(
            child.status.success(),
            "cold child: {}",
            String::from_utf8_lossy(&child.stderr)
        );
    }
}
