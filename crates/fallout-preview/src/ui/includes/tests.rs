use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    inputs: BTreeMap<String, String>,
    sources: BTreeMap<String, Source>,
}
fn archive(folder: &[u8], name: &[u8], payload: &[u8]) -> Vec<u8> {
    // Project-authored uncompressed one-member BSA104, independently specified
    // below. The production archive reader is the only include source importer.
    let table = 54 + folder.len();
    let offset = table + 16 + name.len() + 1;
    let mut bytes = vec![0; offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104),
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
    bytes
}
impl Fixture {
    fn new(inputs: &[(&str, &str)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fallout-preview-include-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("Data")).unwrap();
        let mut sources = BTreeMap::new();
        let mut saved = BTreeMap::new();
        for (name, source) in inputs {
            let bytes = archive(b"menus", name.as_bytes(), source.as_bytes());
            fs::write(root.join("Data").join(format!("{name}.bsa")), &bytes).unwrap();
            let key = format!("menus/{name}");
            sources.insert(
                key.clone(),
                Source {
                    path: key.clone(),
                    archive_sha256: format!("{:x}", Sha256::digest(&bytes)),
                    payload_sha256: format!("{:x}", Sha256::digest(source.as_bytes())),
                },
            );
            saved.insert(key, source.to_string());
        }
        Self {
            root,
            inputs: saved,
            sources,
        }
    }
    fn request(&self) -> Request {
        let mut bindings = Vec::new();
        for (source_path, bytes) in &self.inputs {
            let doc = parse(bytes.as_bytes().to_vec(), super::super::Limits::default()).unwrap();
            for node in &doc.nodes {
                if node.kind == Kind::Element
                    && node.name.is_some_and(|span| doc.text(span) == "include")
                {
                    let Some(attr) = node
                        .attributes
                        .iter()
                        .find(|attr| doc.text(attr.name) == "src")
                    else {
                        continue;
                    };
                    let raw_src = doc.text(attr.raw_value).to_owned();
                    let target = self.sources.get(&raw_src).cloned().unwrap_or(Source {
                        path: raw_src.clone(),
                        archive_sha256: "0".repeat(64),
                        payload_sha256: "0".repeat(64),
                    });
                    bindings.push(Binding {
                        source_path: source_path.clone(),
                        source_payload_sha256: self.sources[source_path].payload_sha256.clone(),
                        src_span: attr.raw_value,
                        raw_src,
                        target,
                    });
                }
            }
        }
        Request {
            schema_version: 1,
            root: self.sources["menus/root.xml"].clone(),
            bindings,
        }
    }
    fn inspect(&self, request: Request, limits: Limits) -> Result<Report> {
        inspect(
            &self.root,
            &path("menus/root.xml").unwrap(),
            Some("Root"),
            request,
            limits,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fallout-preview-include-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}
const ROOT: &str = "<rect name='Root'><include src='menus/b.xml'/><include src='menus/b.xml'> <!--opaque--> </include></rect>";
const LEAF: &str =
    "<template name='Button'><text name='Label'><string> &label; </string></text></template>";

#[test]
fn actual_two_archive_closure_reuses_exact_document_and_retains_source_edges() {
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    let report = f.inspect(f.request(), Limits::default()).unwrap();
    assert_eq!(report.files.len(), 2);
    assert_eq!(report.edges.len(), 2);
    assert_eq!(report.files[0].document.source_utf8, ROOT);
    assert_eq!(report.files[1].document.source_utf8, LEAF);
    assert_eq!(report.files[0].path.bytes(), b"menus/root.xml");
    assert_eq!(report.files[1].path.bytes(), b"menus/b.xml");
    assert_eq!(report.selected_root_element, Some(0));
    assert_eq!(report.usage.maximum_depth, 2);
    assert_eq!(report.usage.source_bytes, ROOT.len() + LEAF.len());
    for edge in &report.edges {
        assert_eq!((edge.source_file, edge.target_file), (0, 1));
        assert_eq!(report.files[0].document.text(edge.src_span), "menus/b.xml");
    }
    assert_eq!(
        (report.edges[0].src_span.start, report.edges[0].src_span.end),
        (32, 43)
    );
    assert_eq!(
        (report.edges[1].src_span.start, report.edges[1].src_span.end),
        (60, 71)
    );
    assert!(!report.original_display_ready);
    assert_eq!(
        report.files[1].archive_sha256,
        f.sources["menus/b.xml"].archive_sha256
    );
}

#[test]
fn aggregate_exact_caps_succeed_and_each_one_under_refuses() {
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    let report = f.inspect(f.request(), Limits::default()).unwrap();
    let exact = Limits {
        files: 2,
        edges: 2,
        depth: 2,
        source_bytes: report.usage.source_bytes,
        events: report.usage.events,
        nodes: report.usage.nodes,
        metadata_bytes: report.usage.metadata_bytes,
        ..Limits::default()
    };
    assert!(f.inspect(f.request(), exact).is_ok());
    for limits in [
        Limits { files: 1, ..exact },
        Limits { edges: 1, ..exact },
        Limits { depth: 1, ..exact },
        Limits {
            source_bytes: exact.source_bytes - 1,
            ..exact
        },
        Limits {
            events: exact.events - 1,
            ..exact
        },
        Limits {
            nodes: exact.nodes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
    ] {
        assert!(f.inspect(f.request(), limits).is_err());
    }
    let mut output = Vec::new();
    write_report(&mut output, &report, exact.output_bytes).unwrap();
    assert!(write_report(Vec::new(), &report, output.len()).is_ok());
    assert!(write_report(Vec::new(), &report, output.len() - 1).is_err());
    let request = serde_json::to_vec(&f.request()).unwrap();
    let file = f.root.join("request.json");
    fs::write(&file, &request).unwrap();
    assert!(
        read_request(
            &file,
            Limits {
                request_bytes: request.len(),
                ..exact
            }
        )
        .is_ok()
    );
    assert!(
        read_request(
            &file,
            Limits {
                request_bytes: request.len() - 1,
                ..exact
            }
        )
        .is_err()
    );
}

#[test]
fn diamond_reuse_does_not_hide_longer_include_depth() {
    let f = Fixture::new(&[
        (
            "root.xml",
            "<rect name='Root'><include src='menus/a.xml'/><include src='menus/b.xml'/></rect>",
        ),
        ("a.xml", "<include src='menus/c.xml'/><rect/>"),
        ("b.xml", "<include src='menus/d.xml'/><rect/>"),
        ("d.xml", "<include src='menus/a.xml'/><rect/>"),
        ("c.xml", "<rect name='Leaf'/>"),
    ]);
    let report = f
        .inspect(
            f.request(),
            Limits {
                depth: 5,
                ..Limits::default()
            },
        )
        .unwrap();
    assert_eq!(
        report
            .files
            .iter()
            .map(|file| file.path.bytes())
            .collect::<Vec<_>>(),
        [
            b"menus/root.xml".as_slice(),
            b"menus/a.xml",
            b"menus/c.xml",
            b"menus/b.xml",
            b"menus/d.xml"
        ]
    );
    assert_eq!(report.files.len(), 5);
    assert_eq!(report.edges.len(), 5);
    assert_eq!(report.usage.maximum_depth, 5);
    let error = f
        .inspect(
            f.request(),
            Limits {
                depth: 4,
                ..Limits::default()
            },
        )
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("reused source"));
}

#[test]
fn cycle_witness_contains_exact_member_and_value_spans() {
    let f = Fixture::new(&[
        (
            "root.xml",
            "<rect name='Root'><include src='menus/a.xml'/></rect>",
        ),
        ("a.xml", "<include src='menus/root.xml'/><rect/>"),
    ]);
    let error = f
        .inspect(f.request(), Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cycle"));
    assert!(error.contains("\"menus/root.xml\":32..43 -> \"menus/a.xml\""));
    assert!(error.contains("\"menus/a.xml\":14..28 -> \"menus/root.xml\""));
}

#[test]
fn exact_binding_shape_and_hashes_refuse_stale_unused_and_invented_paths() {
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    let mut request = f.request();
    request.bindings.pop();
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("Unbound")
    );
    let mut request = f.request();
    request.root.archive_sha256 = "0".repeat(64);
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("archive SHA")
    );
    let mut request = f.request();
    request.bindings[0].source_payload_sha256 = "0".repeat(64);
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("Stale")
    );
    let mut request = f.request();
    request.bindings[0].src_span.start += 1;
    assert!(f.inspect(request, Limits::default()).is_err());
    let mut request = f.request();
    request.bindings[0].raw_src = "menus/unrelated.xml".into();
    assert!(f.inspect(request, Limits::default()).is_err());
    let mut request = f.request();
    request.bindings[0].target.path = "menus/../b.xml".into();
    assert!(f.inspect(request, Limits::default()).is_err());
    let mut request = f.request();
    request.bindings[0].target.payload_sha256 = "0".repeat(64);
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("conflicting")
    );
    let mut request = f.request();
    request.bindings[1].src_span = request.bindings[0].src_span;
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("duplicated")
    );
    let mut request = f.request();
    request.bindings.push(Binding {
        source_path: "menus/unused.xml".into(),
        source_payload_sha256: "0".repeat(64),
        src_span: Span { start: 1, end: 2 },
        raw_src: "menus/b.xml".into(),
        target: f.sources["menus/b.xml"].clone(),
    });
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("unused")
    );
    let mut request = f.request();
    request.root.payload_sha256 = "0".repeat(64);
    assert!(
        f.inspect(request, Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("payload SHA")
    );
}

#[test]
fn missing_ambiguous_and_changed_actual_archives_never_return_a_closure() {
    let missing = Fixture::new(&[(
        "root.xml",
        "<rect name='Root'><include src='menus/missing.xml'/></rect>",
    )]);
    assert!(
        missing
            .inspect(missing.request(), Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("0 candidates")
    );
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    fs::copy(
        f.root.join("Data/b.xml.bsa"),
        f.root.join("Data/duplicate.bsa"),
    )
    .unwrap();
    assert!(
        f.inspect(f.request(), Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("2 candidates")
    );
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    let archive = f.root.join("Data/b.xml.bsa");
    let mut bytes = fs::read(&archive).unwrap();
    bytes.push(0);
    fs::write(&archive, bytes).unwrap();
    assert!(
        f.inspect(f.request(), Limits::default())
            .err()
            .unwrap()
            .to_string()
            .contains("archive SHA")
    );
}

#[test]
fn unsupported_include_fields_entities_and_unknown_request_fields_stay_refusals() {
    for source in [
        "<rect name='Root'><include src='menus/b.xml' extra='1'/></rect>",
        "<rect name='Root'><include src='menus/b.xml'><rect/></include></rect>",
        "<rect name='Root'><include src='&directory;/b.xml'/></rect>",
        "<rect name='Root'><include/></rect>",
    ] {
        let f = Fixture::new(&[("root.xml", source), ("b.xml", LEAF)]);
        assert!(f.inspect(f.request(), Limits::default()).is_err());
    }
    let f = Fixture::new(&[("root.xml", ROOT), ("b.xml", LEAF)]);
    let mut json = serde_json::to_value(f.request()).unwrap();
    json["guessed_precedence"] = true.into();
    let file = f.root.join("bad-request.json");
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(read_request(&file, Limits::default()).is_err());
}
