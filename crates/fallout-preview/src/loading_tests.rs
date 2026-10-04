//! Exercise retry/cancel through the actual host, source adapter and draw queue.
use super::*;
use std::{
    fs,
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

fn authored_request() -> (Options, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../local")
        .join(format!(
            "v3-view-09-fixture-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    let install = root.join("install");
    fs::create_dir_all(install.join("Data")).unwrap();
    let source = root.join("source.nif");
    let options = Options::try_parse_from([
        "fallout-preview",
        "--install",
        install.to_str().unwrap(),
        "--model-file",
        source.to_str().unwrap(),
        "--pose-object",
        "1",
        "--pose-controller",
        "2",
        "--pose-time",
        "0",
    ])
    .unwrap();
    (options, source)
}

fn until(app: &mut App, done: impl Fn(&Phase) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(&app.world().resource::<Loading>().phase) {
        assert!(
            Instant::now() < deadline,
            "controlled host transition did not complete"
        );
        app.update();
        std::thread::yield_now();
    }
}

#[test]
fn missing_source_retry_reads_repaired_authored_bytes_and_admits_new_epoch() {
    let (options, source) = authored_request();
    let mut app = tests::loading_app(Phase::WaitingForWindow);
    app.insert_resource(options);
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(WindowCreated { window });
    app.update();
    until(&mut app, |phase| matches!(phase, Phase::Failed(_)));
    assert_eq!(app.world().resource::<Loading>().epoch, 7);
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert_eq!(app.world().resource::<Capture>().frame, 63);
    // Only this authored, owned input is repaired. The retry uses production
    // file/source-pose preparation, rather than substituting a ready fixture.
    fs::write(
        &source,
        include_bytes!("testdata/source-pose-triangle.packet"),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<input::Actions>()
        .retry_loading = true;
    app.update();
    assert!(matches!(
        app.world().resource::<Loading>().phase,
        Phase::Preparing(_)
    ));
    assert_eq!(app.world().resource::<Loading>().epoch, 8);
    app.world_mut()
        .resource_mut::<input::Actions>()
        .retry_loading = false;
    until(&mut app, |phase| matches!(phase, Phase::Ready(_)));
    assert_eq!(app.world().resource::<Capture>().frame, 0);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mesh = meshes.iter().next().unwrap().1;
    assert!(matches!(mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values))
        if values == &[[-6., -1., -5.], [-6., -1., -7.], [-6., -3., -5.]]));
}

#[test]
fn cancelling_gated_source_keeps_window_updates_and_refuses_retry_until_return() {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let job = loading::Job::start(7, move |_| {
        entered.send(()).unwrap();
        gate.recv().unwrap();
        Ok(tests::ready_fixture(7))
    })
    .unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut app = tests::loading_app(Phase::Preparing(job));
    app.world_mut()
        .resource_mut::<input::Actions>()
        .cancel_loading = true;
    app.update();
    assert!(matches!(
        app.world().resource::<Loading>().phase,
        Phase::Draining(_)
    ));
    assert_eq!(app.world().resource::<Loading>().epoch, 8);
    *app.world_mut().resource_mut::<input::Actions>() = input::Actions {
        retry_loading: true,
        ..default()
    };
    for _ in 0..3 {
        app.update();
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::Draining(_)
        ));
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert_eq!(app.world().resource::<Capture>().frame, 63);
    }
    *app.world_mut().resource_mut::<input::Actions>() = input::Actions::default();
    release.send(()).unwrap();
    until(&mut app, |phase| matches!(phase, Phase::Cancelled));
    let window = app
        .world_mut()
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    assert!(window.title.contains("Enter retries"));
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn retry_never_replaces_scoped_outputs_or_reuses_exhausted_epoch() {
    for capture_output in [false, true] {
        let (mut options, source) = authored_request();
        let output = source.with_extension("immutable-evidence");
        fs::write(&output, b"prior scoped evidence").unwrap();
        if capture_output {
            options.capture = Some(output.clone());
        } else {
            options.report = Some(output.clone());
        }
        let mut app = tests::loading_app(Phase::Failed("Selected source failed".into()));
        app.insert_resource(options);
        app.world_mut()
            .resource_mut::<input::Actions>()
            .retry_loading = true;
        app.update();
        assert!(matches!(&app.world().resource::<Loading>().phase,
            Phase::Failed(error) if error.contains("already exists")));
        assert_eq!(app.world().resource::<Loading>().epoch, 7);
        assert_eq!(fs::read(output).unwrap(), b"prior scoped evidence");
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    }
    let mut app = tests::loading_app(Phase::Cancelled);
    app.world_mut().resource_mut::<Loading>().epoch = u64::MAX;
    app.world_mut()
        .resource_mut::<input::Actions>()
        .retry_loading = true;
    app.update();
    assert!(matches!(&app.world().resource::<Loading>().phase,
        Phase::Failed(error) if error.contains("epoch exhausted")));
    assert_eq!(app.world().resource::<Loading>().epoch, u64::MAX);
}

#[test]
fn source_retry_failure_remains_visible_and_never_admits_fallback() {
    let mut app = tests::loading_app(Phase::Failed("Original source failure".into()));
    app.world_mut()
        .resource_mut::<input::Actions>()
        .retry_loading = true;
    app.update();
    assert_eq!(app.world().resource::<Loading>().epoch, 8);
    app.world_mut()
        .resource_mut::<input::Actions>()
        .retry_loading = false;
    until(&mut app, |phase| matches!(phase, Phase::Failed(_)));
    let window = app
        .world_mut()
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    assert!(window.title.contains("Failed:"));
    assert!(window.title.contains("Enter retries"));
    assert_eq!(app.world().resource::<Capture>().frame, 63);
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn upload_cancel_retires_actual_handles_before_terminal_retry_is_available() {
    let mut app = tests::loading_app(Phase::Uploading(tests::ready_fixture(7).upload));
    app.update();
    assert!(matches!(
        app.world().resource::<Loading>().phase,
        Phase::Uploading(_)
    ));
    assert!(!app.world().resource::<Assets<Mesh>>().is_empty());
    app.world_mut()
        .resource_mut::<input::Actions>()
        .cancel_loading = true;
    app.update();
    assert_eq!(app.world().resource::<Loading>().epoch, 8);
    assert!(matches!(
        app.world().resource::<Loading>().phase,
        Phase::Disposing(_, None)
    ));
    *app.world_mut().resource_mut::<input::Actions>() = input::Actions {
        retry_loading: true,
        ..default()
    };
    until(&mut app, |phase| matches!(phase, Phase::Cancelled));
    assert_eq!(app.world().resource::<Loading>().epoch, 8);
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(
        app.world()
            .resource::<Assets<material::InspectionMaterial>>()
            .is_empty()
    );
    assert_eq!(app.world().resource::<Capture>().frame, 63);
}

#[test]
fn secondary_window_close_cannot_cancel_primary_source_request() {
    let mut app = tests::loading_app(Phase::WaitingForWindow);
    let secondary = app.world_mut().spawn(Window::default()).id();
    app.world_mut()
        .write_message(WindowCloseRequested { window: secondary });
    app.update();
    assert!(matches!(
        app.world().resource::<Loading>().phase,
        Phase::WaitingForWindow
    ));
    assert_eq!(app.world().resource::<Loading>().epoch, 7);
    assert_eq!(
        *app.world().resource::<input::Context>(),
        input::Context::Loading
    );
}
