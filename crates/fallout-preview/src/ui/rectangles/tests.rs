use super::*;
use clap::Parser;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ROOT: &str = "<rect name='Panel'><x>10</x><y>12</y><width>30</width><height>20</height><depth>1</depth><red>255</red><green>0</green><blue>0</blue><alpha>255</alpha><visible>1</visible>";
const CHILD: &str = "<rect name='Child'><x>4</x><y>5</y><width>8</width><height>6</height><depth>2</depth><red>0</red><green>255</green><blue>0</blue><alpha>127.5</alpha><visible>1</visible></rect>";
pub(crate) fn document(source: &str) -> Document {
    super::super::parse(source.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
}
pub(crate) fn request(doc: &Document) -> Request {
    let node = doc.named_element("Panel").unwrap();
    Request {
        schema_version: 1,
        source: includes::Source {
            path: "menus/authored.xml".into(),
            archive_sha256: "0".repeat(64),
            payload_sha256: format!("{:x}", Sha256::digest(doc.source_utf8.as_bytes())),
        },
        tile: traits::Tile {
            name: "Panel".into(),
            node,
            span: doc.nodes[node].span,
        },
        policy: POLICY.into(),
        viewport: Viewport {
            width: 64,
            height: 48,
            background: [0., 0., 0., 1.],
            depth_range: [-10., 10.],
        },
        parent: Parent {
            origin: [2., 3., 0.5],
            opacity: 0.5,
            visible: true,
        },
    }
}
fn authored() -> Document {
    document(&format!("{ROOT}{CHILD}</rect>"))
}
#[test]
fn source_rectangles_have_independent_bounds_bits_color_and_ancestor_semantics() {
    let doc = authored();
    let request = request(&doc);
    let plan = project(&doc, &request, Limits::default()).unwrap();
    assert_eq!(plan.rectangles.len(), 2);
    let root = &plan.rectangles[0];
    let child = &plan.rectangles[1];
    assert_eq!(root.bounds, [12., 15., 42., 35.]);
    assert_eq!(root.depth, 1.5);
    assert_eq!(child.bounds, [16., 20., 24., 26.]);
    assert_eq!(child.depth, 3.5);
    assert_eq!(root.rgba, [1., 0., 0., 0.5]);
    assert_eq!(child.rgba, [0., 1., 0., 0.25]);
    assert_eq!(child.parent_rectangle, Some(0));
    assert!(child.effective_visible);
    assert_eq!(
        child.positions,
        [
            [16., -20., 3.5],
            [24., -20., 3.5],
            [24., -26., 3.5],
            [16., -26., 3.5]
        ]
    );
    assert_eq!(
        child.mesh_positions,
        [[-4., 3., 0.], [4., 3., 0.], [4., -3., 0.], [-4., -3., 0.]]
    );
    for rectangle in &plan.rectangles {
        assert_eq!(rectangle.span, doc.nodes[rectangle.node].span);
        for n in &rectangle.numeric {
            assert_eq!(n.bits, n.value.to_bits());
            assert_eq!(
                doc.text(n.inner_span).parse::<f32>().unwrap().to_bits(),
                n.bits
            );
            assert_eq!(doc.nodes[n.node].span, n.span);
        }
    }
    let doc = document(&format!(
        "{}{CHILD}</rect><rect name='Child'/>",
        ROOT.replace("<visible>1", "<visible>0")
    ));
    let request = request_for(&doc);
    let plan = project(&doc, &request, Limits::default()).unwrap();
    assert_eq!(plan.rectangles.len(), 2);
    assert!(plan.rectangles.iter().all(|r| !r.effective_visible));
}
fn request_for(doc: &Document) -> Request {
    request(doc)
}
#[test]
fn unsupported_missing_duplicate_empty_entities_and_operations_never_receive_defaults() {
    let complete = format!("{ROOT}</rect>");
    for source in [
        complete.replace("<red>255</red>", ""),
        complete.replace("<width>30</width>", "<width/>"),
        complete.replace("<x>10</x>", "<x>10</x><x>10</x>"),
        complete.replace("<x>10</x>", "<x><copy src='parent()' trait='x'/></x>"),
        complete.replace("<visible>1</visible>", "<visible>&true;</visible>"),
        complete.replace("<visible>1</visible>", "<visible>2</visible>"),
        complete.replace("<x>10</x>", "<x>NAN</x>"),
        complete.replace("<x>10</x>", "<x>1e100</x>"),
        complete.replace("<x>10</x>", "<x>1e-100</x>"),
        complete.replace("</rect>", "<text name='Label'/></rect>"),
        complete.replace("</rect>", "<id>1</id></rect>"),
        complete.replace("<red>255</red>", "<red>256</red>"),
        complete.replace("<width>30</width>", "<width>0</width>"),
        complete.replace("<width>30</width>", "<width>0.0000001</width>"),
        complete.replace("<red>255</red>", "<red>1e-44</red>"),
        complete.replace("<alpha>255</alpha>", "<alpha>1e-38</alpha>"),
        complete
            .replace("<x>10</x>", "<x>0</x>")
            .replace("<y>12</y>", "<y>0</y>")
            .replace("<width>30</width>", "<width>1.4e-45</width>")
            .replace("<height>20</height>", "<height>1.4e-45</height>"),
        complete.replace("name='Panel'", "name='Panel' note='extra'"),
        complete.replace("<x>10</x>", "<x units='pixels'>10</x>"),
        complete.replace("</rect>", "unexpected</rect>"),
        complete.replace("<depth>1</depth>", "<depth>20</depth>"),
    ] {
        let doc = document(&source);
        let request = request(&doc);
        assert!(
            project(&doc, &request, Limits::default()).is_err(),
            "{source}"
        );
    }
    let source = format!(
        "{ROOT}{} </rect>",
        CHILD.replace("<depth>2</depth>", "<depth>0</depth>")
    );
    let doc = document(&source);
    assert!(project(&doc, &request(&doc), Limits::default()).is_err());
}
#[test]
fn complete_admission_exact_caps_succeed_and_each_one_under_refuses() {
    let doc = authored();
    let request = request(&doc);
    let plan = project(&doc, &request, Limits::default()).unwrap();
    let exact = Limits {
        rectangles: 2,
        depth: 2,
        work: plan.usage.traversal_work,
        projection_copies: plan.usage.reserved_projection_copies,
        projection_metadata: plan.usage.projection_metadata,
        plan_metadata: plan.usage.plan_metadata,
        mesh_bytes: plan.usage.mesh_bytes,
        ..Limits::default()
    };
    assert!(project(&doc, &request, exact).is_ok());
    for limits in [
        Limits {
            rectangles: 1,
            ..exact
        },
        Limits { depth: 1, ..exact },
        Limits {
            work: exact.work - 1,
            ..exact
        },
        Limits {
            projection_copies: exact.projection_copies - 1,
            ..exact
        },
        Limits {
            projection_metadata: exact.projection_metadata - 1,
            ..exact
        },
        Limits {
            plan_metadata: exact.plan_metadata - 1,
            ..exact
        },
        Limits {
            mesh_bytes: exact.mesh_bytes - 1,
            ..exact
        },
    ] {
        assert!(project(&doc, &request, limits).is_err());
    }
}
#[test]
fn stale_identity_and_request_bounds_refuse_before_draw_preparation() {
    let doc = authored();
    for field in [
        "schema",
        "policy",
        "payload",
        "span",
        "node",
        "name",
        "parent",
        "opacity",
        "pixels",
        "dimension",
        "background",
        "depth",
    ] {
        let mut request = request(&doc);
        match field {
            "schema" => request.schema_version = 2,
            "policy" => request.policy = "guess".into(),
            "payload" => request.source.payload_sha256 = "f".repeat(64),
            "span" => request.tile.span.end += 1,
            "node" => request.tile.node = usize::MAX,
            "name" => request.tile.name = "Child".into(),
            "parent" => request.parent.origin[0] = f64::INFINITY,
            "opacity" => request.parent.opacity = 1.01,
            "pixels" => {
                request.viewport.width = 4096;
                request.viewport.height = 4096;
            }
            "dimension" => request.viewport.width = 0,
            "background" => request.viewport.background[0] = f32::NAN,
            "depth" => request.viewport.depth_range = [10., -10.],
            _ => unreachable!(),
        }
        assert!(
            project(&doc, &request, Limits::default()).is_err(),
            "{field}"
        );
    }
}
pub(crate) struct Fixture {
    pub root: PathBuf,
    pub request: Arc<Request>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local")
            .join(format!(
                "v3-view-20-fixture-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(root.join("Data")).unwrap();
        let doc = authored();
        let mut request = request(&doc);
        // One-member uncompressed authored BSA104. Production archive importer
        // is still the sole source reader; no local XML substitute reaches load.
        let folder = b"menus";
        let name = b"authored.xml";
        let payload = doc.source_utf8.as_bytes();
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
        request.source.archive_sha256 = format!("{:x}", Sha256::digest(&bytes));
        fs::write(root.join("Data/authored.bsa"), bytes).unwrap();
        Self {
            root,
            request: Arc::new(request),
        }
    }
    pub(crate) fn options(&self) -> crate::Options {
        let mut options = crate::Options::try_parse_from([
            "fallout-preview",
            "--install",
            self.root.to_str().unwrap(),
            "--menu-rectangles",
            "request.json",
            "--report",
            self.root.join("report.json").to_str().unwrap(),
        ])
        .unwrap();
        options.rectangle_request = Some(self.request.clone());
        options
    }
}
#[test]
fn real_archive_loading_admits_tile_labels_and_retirement_preserves_unrelated_entities() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let mut app = crate::tests::loading_app(crate::Phase::WaitingForWindow);
    app.insert_resource(options);
    app.update();
    assert!(matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::WaitingForWindow
    ));
    assert!(!fixture.root.join("report.json").exists());
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .write_message(bevy::window::WindowCreated { window });
    app.update();
    let unrelated = app.world_mut().spawn(Transform::IDENTITY).id();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Ready(_)
    ) {
        assert!(Instant::now() < deadline, "loading failed");
        app.update();
        std::thread::yield_now();
    }
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 2);
    let mut query = app.world_mut().query::<&TileView>();
    let views = query.iter(app.world()).collect::<Vec<_>>();
    assert_eq!(views.len(), 4);
    for view in views {
        assert_eq!(view.epoch, 7);
        assert!(Arc::ptr_eq(
            &view.source,
            &query.iter(app.world()).next().unwrap().source
        ));
        assert_eq!(
            view.source.payload_sha256,
            fixture.request.source.payload_sha256
        );
    }
    let mut camera = app.world_mut().query::<(&Transform, &Projection)>();
    let (transform, projection) = camera.single(app.world()).unwrap();
    assert_eq!(*transform, super::camera(&fixture.request).0);
    assert!(matches!(projection, Projection::Orthographic(_)));
    app.world_mut()
        .write_message(bevy::window::WindowCloseRequested { window });
    app.update();
    while !matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Cancelled
    ) {
        assert!(Instant::now() < deadline);
        app.update();
    }
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(
        app.world()
            .resource::<Assets<material::InspectionMaterial>>()
            .is_empty()
    );
    assert_eq!(
        app.world_mut()
            .query::<&TileView>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(app.world().get_entity(unrelated).is_ok());
}
#[test]
fn cancelled_completed_source_result_retains_ownership_until_host_drain_and_never_publishes() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let (send, recv) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let job = loading::Job::start(7, move |context| {
        let ready = crate::prepare_scene(&options, &context, 7).map_err(|e| e.to_string())?;
        send.send(()).unwrap();
        gate.recv().unwrap();
        Ok(ready)
    })
    .unwrap();
    recv.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut app = crate::tests::loading_app(crate::Phase::Preparing(job));
    app.world_mut()
        .resource_mut::<crate::input::Actions>()
        .cancel_loading = true;
    app.update();
    assert!(matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Draining(_)
    ));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Cancelled
    ) {
        assert!(Instant::now() < deadline);
        app.update();
        std::thread::yield_now();
    }
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert_eq!(
        app.world_mut()
            .query::<&TileView>()
            .iter(app.world())
            .count(),
        0
    );
}
