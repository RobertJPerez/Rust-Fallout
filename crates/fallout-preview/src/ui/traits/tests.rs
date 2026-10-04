use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const SOURCE: &str = "<rect name='Panel'><x>0</x><y>-0</y><width>12.5</width><height></height><visible>1</visible><alpha> bad </alpha><string> 日本 &amp; 😀 </string><filename></filename><custom>17</custom><duplicate>1</duplicate><duplicate>2</duplicate><derived><copy src='parent()' trait='width'/></derived><enabled>&true;</enabled><image name='Child'/><!--comment--></rect>";
fn document(source: &str) -> Document {
    super::super::parse(source.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
}
fn make_request(doc: &Document, values: &[(&str, ConversionKind)]) -> Request {
    let node = doc.named_element("Panel").unwrap();
    Request {
        schema_version: 1,
        source: includes::Source {
            path: "menus/authored.xml".into(),
            archive_sha256: "0".repeat(64),
            payload_sha256: format!("{:x}", Sha256::digest(doc.source_utf8.as_bytes())),
        },
        tile: Tile {
            name: "Panel".into(),
            node,
            span: doc.nodes[node].span,
        },
        conversions: values
            .iter()
            .map(|(name, kind)| Conversion {
                name: (*name).into(),
                kind: *kind,
            })
            .collect(),
    }
}
fn project_default(doc: &Document, request: &Request) -> Projection {
    project(
        doc,
        &request.source.payload_sha256,
        request,
        Limits::default(),
    )
    .unwrap()
}
fn reason(row: &Row) -> &Reason {
    let Outcome::Unresolved { reason, .. } = &row.outcome else {
        panic!("Expected unresolved")
    };
    reason
}
fn number(row: &Row) -> (f32, u32) {
    let Outcome::Value {
        value: Literal::FiniteF32 { value, bits },
    } = row.outcome
    else {
        panic!("Expected numeric")
    };
    (value, bits)
}
#[test]
fn complete_source_projection_preserves_spelling_order_zero_negative_zero_empty_and_absent() {
    use ConversionKind::*;
    let doc = document(SOURCE);
    let request = make_request(
        &doc,
        &[
            ("x", FiniteF32),
            ("y", FiniteF32),
            ("width", FiniteF32),
            ("height", FiniteF32),
            ("visible", Boolean01),
            ("alpha", FiniteF32),
            ("string", String),
            ("filename", String),
            ("duplicate", FiniteF32),
            ("derived", FiniteF32),
            ("enabled", Boolean01),
            ("red", FiniteF32),
        ],
    );
    let projection = project_default(&doc, &request);
    assert_eq!(
        projection.direct_children,
        [1, 3, 5, 7, 8, 10, 12, 16, 17, 19, 21, 23, 25, 27, 28]
    );
    assert_eq!(
        projection
            .rows
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        [
            "x",
            "y",
            "width",
            "height",
            "visible",
            "alpha",
            "string",
            "filename",
            "custom",
            "duplicate",
            "duplicate",
            "derived",
            "enabled",
            "image",
            "red"
        ]
    );
    let rows = &projection.rows;
    assert_eq!(number(&rows[0]), (0.0, 0));
    assert_eq!(number(&rows[1]), (-0.0, 0x80000000));
    assert_eq!(number(&rows[2]), (12.5, 0x41480000));
    assert!(matches!(rows[3].outcome, Outcome::Empty));
    assert!(matches!(
        rows[4].outcome,
        Outcome::Value {
            value: Literal::Boolean01 { value: true }
        }
    ));
    assert_eq!(reason(&rows[5]), &Reason::MalformedNumber);
    let Outcome::Value {
        value: Literal::String { value },
    } = &rows[6].outcome
    else {
        panic!()
    };
    assert_eq!(value, " 日本 & 😀 ");
    let Outcome::Value {
        value: Literal::String { value },
    } = &rows[7].outcome
    else {
        panic!()
    };
    assert_eq!(value, "");
    assert_eq!(reason(&rows[8]), &Reason::ConversionUnsupplied);
    assert_eq!(reason(&rows[9]), &Reason::DuplicateDeclaration);
    assert_eq!(reason(&rows[10]), &Reason::DuplicateDeclaration);
    assert_eq!(reason(&rows[11]), &Reason::NestedSource);
    assert_eq!(reason(&rows[12]), &Reason::CustomEntity);
    let Outcome::Unresolved { references, .. } = &rows[12].outcome else {
        panic!()
    };
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].name, "true");
    let offset = SOURCE.find("&true;").unwrap();
    assert_eq!(
        references[0].span,
        Span {
            start: offset,
            end: offset + 6
        }
    );
    assert!(matches!(rows[13].outcome, Outcome::Structural));
    assert!(matches!(rows[14].outcome, Outcome::Absent));
    assert!(
        rows[14].node.is_none() && rows[14].span.is_none() && rows[14].source_spelling.is_none()
    );
    let spelling = [
        "0",
        "-0",
        "12.5",
        "",
        "1",
        " bad ",
        " 日本 &amp; 😀 ",
        "",
        "17",
        "1",
        "2",
        "<copy src='parent()' trait='width'/>",
        "&true;",
        "",
    ];
    for (row, expected) in rows.iter().zip(spelling) {
        assert_eq!(row.source_spelling.as_deref(), Some(expected));
        assert_eq!(doc.text(row.inner_span.unwrap()), expected);
        assert_eq!(doc.text(row.name_span.unwrap()), row.name);
    }
    assert_eq!(
        rows[1].span,
        Some(Span {
            start: SOURCE.find("<y>").unwrap(),
            end: SOURCE.find("</y>").unwrap() + 4
        })
    );
}
#[test]
fn explicit_numeric_and_boolean_policy_refuses_malformed_overflow_nonfinite_and_underflow() {
    for (text, expected) in [
        ("abc", Reason::MalformedNumber),
        ("1 2", Reason::MalformedNumber),
        ("\u{a0}1", Reason::MalformedNumber),
        ("1e99", Reason::Overflow),
        ("-1e999", Reason::Overflow),
        ("NaN", Reason::NonFiniteNumber),
        ("+NaN", Reason::NonFiniteNumber),
        ("-inf", Reason::NonFiniteNumber),
        ("1e-999", Reason::Underflow),
    ] {
        let doc = document(&format!("<rect name='Panel'><x>{text}</x></rect>"));
        let request = make_request(&doc, &[("x", ConversionKind::FiniteF32)]);
        assert_eq!(
            reason(&project_default(&doc, &request).rows[0]),
            &expected,
            "{text}"
        );
    }
    for (text, expected, bits) in [
        ("\t -0e99 \r\n", -0.0_f32, 0x80000000),
        ("2.5e-3", 0.0025_f32, 0.0025_f32.to_bits()),
    ] {
        let doc = document(&format!("<rect name='Panel'><x>{text}</x></rect>"));
        let request = make_request(&doc, &[("x", ConversionKind::FiniteF32)]);
        assert_eq!(
            number(&project_default(&doc, &request).rows[0]),
            (expected, bits)
        );
    }
    for text in ["true", "false", "1.0", "-0", "2", "NaN"] {
        let doc = document(&format!(
            "<rect name='Panel'><visible>{text}</visible></rect>"
        ));
        let request = make_request(&doc, &[("visible", ConversionKind::Boolean01)]);
        assert_eq!(
            reason(&project_default(&doc, &request).rows[0]),
            &Reason::BooleanNotZeroOrOne
        );
    }
    let doc = document("<rect name='Panel'><visible> 0 </visible><x> \r\n </x></rect>");
    let request = make_request(
        &doc,
        &[
            ("visible", ConversionKind::Boolean01),
            ("x", ConversionKind::FiniteF32),
        ],
    );
    let rows = project_default(&doc, &request).rows;
    assert!(matches!(
        rows[0].outcome,
        Outcome::Value {
            value: Literal::Boolean01 { value: false }
        }
    ));
    assert!(matches!(rows[1].outcome, Outcome::Empty));
}
#[test]
fn no_custom_defaults_or_duplicate_precedence_and_builtin_unicode_is_supported() {
    let doc = document(
        "<rect name='Panel'><x a='1'>2</x><string><![CDATA[A &custom;]]> &#x1F642;&lt;</string><X>4</X><include src='other.xml'/></rect>",
    );
    let request = make_request(
        &doc,
        &[
            ("x", ConversionKind::FiniteF32),
            ("string", ConversionKind::String),
            ("red", ConversionKind::FiniteF32),
        ],
    );
    let rows = project_default(&doc, &request).rows;
    assert_eq!(reason(&rows[0]), &Reason::AttributedDeclaration);
    let Outcome::Value {
        value: Literal::String { value },
    } = &rows[1].outcome
    else {
        panic!()
    };
    assert_eq!(value, "A &custom; 🙂<");
    assert_eq!(reason(&rows[2]), &Reason::ConversionUnsupplied);
    assert!(matches!(rows[3].outcome, Outcome::Structural));
    assert!(matches!(rows[4].outcome, Outcome::Absent));
    let doc = document("<rect name='Panel'><x>&#0;</x></rect>");
    let request = make_request(&doc, &[("x", ConversionKind::FiniteF32)]);
    assert!(
        project(
            &doc,
            &request.source.payload_sha256,
            &request,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn exact_projection_caps_succeed_and_each_one_under_refuses_complete_result() {
    let doc = document(SOURCE);
    let request = make_request(
        &doc,
        &[
            ("x", ConversionKind::FiniteF32),
            ("missing", ConversionKind::String),
        ],
    );
    let usage = project_default(&doc, &request).usage;
    let exact = Limits {
        conversions: 2,
        rows: usage.rows,
        copy_bytes: usage.reserved_copy_bytes,
        metadata_bytes: usage.metadata_bytes,
        document: super::super::Limits {
            source_bytes: SOURCE.len(),
            ..super::super::Limits::default()
        },
        ..Limits::default()
    };
    assert!(project(&doc, &request.source.payload_sha256, &request, exact).is_ok());
    for limit in [
        Limits {
            conversions: 1,
            ..exact
        },
        Limits {
            rows: exact.rows - 1,
            ..exact
        },
        Limits {
            copy_bytes: exact.copy_bytes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
        Limits {
            document: super::super::Limits {
                source_bytes: SOURCE.len() - 1,
                ..exact.document
            },
            ..exact
        },
    ] {
        assert!(project(&doc, &request.source.payload_sha256, &request, limit).is_err());
    }
}
#[test]
fn stale_source_span_identity_schema_duplicate_policy_and_ambiguous_names_refuse() {
    let doc = document(SOURCE);
    let base = make_request(&doc, &[("x", ConversionKind::FiniteF32)]);
    assert!(project(&doc, &"f".repeat(64), &base, Limits::default()).is_err());
    for field in [
        "payload",
        "span",
        "node",
        "name",
        "schema",
        "duplicate",
        "structural",
        "empty",
    ] {
        let mut request = make_request(&doc, &[("x", ConversionKind::FiniteF32)]);
        match field {
            "payload" => request.source.payload_sha256 = "f".repeat(64),
            "span" => request.tile.span.end += 1,
            "node" => request.tile.node = usize::MAX,
            "name" => request.tile.name = "Child".into(),
            "schema" => request.schema_version = 2,
            "duplicate" => request.conversions.push(Conversion {
                name: "x".into(),
                kind: ConversionKind::String,
            }),
            "structural" => request.conversions[0].name = "image".into(),
            "empty" => request.conversions[0].name.clear(),
            _ => unreachable!(),
        }
        assert!(
            project(
                &doc,
                &request.source.payload_sha256,
                &request,
                Limits::default()
            )
            .is_err(),
            "{field}"
        );
    }
    let duplicate = document("<rect name='Panel'/><rect name='Panel'/>");
    let mut request = base;
    request.source.payload_sha256 =
        format!("{:x}", Sha256::digest(duplicate.source_utf8.as_bytes()));
    request.tile.node = 0;
    request.tile.span = duplicate.nodes[0].span;
    assert!(
        project(
            &duplicate,
            &request.source.payload_sha256,
            &request,
            Limits::default()
        )
        .is_err()
    );
    let template = document("<template name='Panel'/>");
    let request = self::make_request(&template, &[]);
    assert!(
        project(
            &template,
            &request.source.payload_sha256,
            &request,
            Limits::default()
        )
        .is_err()
    );
}
struct Files(PathBuf);
#[test]
fn missing_reference_outcomes_reserve_retained_metadata_and_preserve_each_source_span() {
    let doc = document("<rect name='Panel'><string>&missing;&missing;</string></rect>");
    let request = make_request(&doc, &[("string", ConversionKind::String)]);
    let projection = project_default(&doc, &request);
    let empty_doc = document("<rect name='Panel'><string></string></rect>");
    let empty_request = make_request(&empty_doc, &[("string", ConversionKind::String)]);
    let empty = project_default(&empty_doc, &empty_request);
    assert_eq!(
        projection.usage.metadata_bytes - empty.usage.metadata_bytes,
        2 * size_of::<entities::Unresolved>()
    );
    assert_eq!(
        projection.usage.reserved_copy_bytes,
        "string".len() + 2 * "&missing;&missing;".len()
    );
    let Outcome::Unresolved { reason, references } = &projection.rows[0].outcome else {
        panic!()
    };
    assert_eq!(*reason, Reason::CustomEntity);
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.name.as_str())
            .collect::<Vec<_>>(),
        ["missing", "missing"]
    );
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.span)
            .collect::<Vec<_>>(),
        [Span { start: 27, end: 36 }, Span { start: 36, end: 45 }]
    );
    assert!(
        project(
            &doc,
            &request.source.payload_sha256,
            &request,
            Limits {
                metadata_bytes: projection.usage.metadata_bytes - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fallout-preview-traits-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fallout-preview-traits-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn request_and_report_exact_bytes_and_strict_shape_are_verified() {
    let doc = document(SOURCE);
    let request = make_request(&doc, &[("visible", ConversionKind::Boolean01)]);
    let files = Files::new();
    let file = files.0.join("request.json");
    let bytes = serde_json::to_vec(&request).unwrap();
    assert!(
        String::from_utf8(bytes.clone())
            .unwrap()
            .contains("boolean-01")
    );
    fs::write(&file, &bytes).unwrap();
    assert!(
        read_request(
            &file,
            Limits {
                request_bytes: bytes.len(),
                ..Limits::default()
            }
        )
        .is_ok()
    );
    assert!(
        read_request(
            &file,
            Limits {
                request_bytes: bytes.len() - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let mut json = serde_json::to_value(&request).unwrap();
    json["default_width"] = 0.into();
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(read_request(&file, Limits::default()).is_err());
    let malformed = serde_json::to_string(&request)
        .unwrap()
        .replace("Panel", r"\ud800");
    fs::write(&file, malformed).unwrap();
    let error = read_request(&file, Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("surrogate") || error.contains("hex escape"));
    let projection = project_default(&doc, &request);
    let report = Report {
        schema_version: 1,
        source: super::super::Report {
            schema_version: 1,
            source: fallout_data::vfs::AssetSource {
                container: "authored.bsa".into(),
                entry_index: 0,
                original_path: b"menus/authored.xml".to_vec(),
            },
            archive_sha256: "0".repeat(64),
            payload_sha256: request.source.payload_sha256.clone(),
            document: doc,
            selected_element: None,
            interpretation: "authored source",
            original_display_ready: false,
        },
        request: &request,
        projection,
        interpretation: "authored literal projection",
        original_display_ready: false,
    };
    let mut bytes = Vec::new();
    write_report(&mut bytes, &report, Limits::default().output_bytes).unwrap();
    assert!(write_report(Vec::new(), &report, bytes.len()).is_ok());
    assert!(write_report(Vec::new(), &report, bytes.len() - 1).is_err());
    let serialized: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serialized["projection"]["rows"][4]["outcome"]["value"],
        serde_json::json!({"kind":"boolean-01","value":true})
    );
}
