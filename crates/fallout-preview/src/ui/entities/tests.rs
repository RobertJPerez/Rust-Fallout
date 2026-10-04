use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const TEXT: &str = concat!(
    "<rect name='Root' note='日本 &amp; &custom; &#x1F642;'>",
    "<string name='Value'>α &lt;&gt;&quot;&apos;&amp; &#65; &#x03A9; &person; / &empty;",
    "<![CDATA[ &literal; ]]><!--ignored--></string></rect>"
);
fn document(source: &str) -> Document {
    super::super::parse(source.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
}
fn payload(document: &Document) -> String {
    format!("{:x}", Sha256::digest(document.source_utf8.as_bytes()))
}
fn selection(document: &Document, name: &str) -> Selection {
    let node = document.named_element(name).unwrap();
    Selection::ElementText {
        node,
        span: document.nodes[node].span,
    }
}
fn attribute(document: &Document, name: &str) -> Selection {
    let node = document.named_element("Root").unwrap();
    let attribute = document.nodes[node]
        .attributes
        .iter()
        .find(|a| document.text(a.name) == name)
        .unwrap();
    Selection::Attribute {
        node,
        name_span: attribute.name,
        value_span: attribute.raw_value,
    }
}
fn definitions(values: &[(&str, &str)]) -> Vec<Definition> {
    values
        .iter()
        .map(|(name, value)| Definition {
            name: (*name).into(),
            value: (*value).into(),
        })
        .collect()
}
fn resolve_text(
    document: &Document,
    definitions: &[Definition],
    limits: Limits,
) -> Result<Resolution> {
    resolve(
        document,
        &payload(document),
        &selection(document, "Value"),
        definitions,
        limits,
    )
}

#[test]
fn complete_selected_utf8_builtins_numeric_literal_values_and_cdata_match_authored_bytes() {
    let doc = document(TEXT);
    let values = definitions(&[("person", "王 & L"), ("empty", "")]);
    let result = resolve_text(&doc, &values, Limits::default()).unwrap();
    let expected = "α <>\"'& A Ω 王 & L /  &literal; ";
    assert_eq!(
        result.value.as_deref().unwrap().as_bytes(),
        expected.as_bytes()
    );
    assert!(result.unresolved.is_empty());
    assert_eq!(result.usage.references, 9);
    assert_eq!(result.usage.expanded_bytes, expected.len());
    assert_eq!(result.usage.copied_bytes, expected.len());
    assert_eq!(doc.source_utf8, TEXT);
    let result = resolve(
        &doc,
        &payload(&doc),
        &attribute(&doc, "note"),
        &definitions(&[("custom", "非ASCII")]),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(result.value.as_deref(), Some("日本 & 非ASCII 🙂"));
    assert_eq!(result.usage.references, 3);
}

#[test]
fn missing_repeated_entities_preserve_exact_spans_and_never_publish_partial_text() {
    let source = "<x name='Value'> &missing; &missing; &amp; </x>";
    let doc = document(source);
    let result = resolve_text(&doc, &[], Limits::default()).unwrap();
    assert_eq!(result.value, None);
    assert_eq!(result.usage.copied_bytes, 0);
    assert_eq!(result.usage.references, 3);
    assert_eq!(
        result
            .unresolved
            .iter()
            .map(|u| (u.span.start, u.span.end, u.name.as_str()))
            .collect::<Vec<_>>(),
        [(17, 26, "missing"), (27, 36, "missing")]
    );
    for entity in &result.unresolved {
        assert_eq!(doc.text(entity.span), "&missing;");
    }
    assert!(
        resolve_text(
            &doc,
            &[],
            Limits {
                references: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let bad = document("<x name='Value'>&missing;&#xD800;</x>");
    assert!(resolve_text(&bad, &[], Limits::default()).is_err());
    let html = document("<x name='Value'>&nbsp;</x>");
    assert!(
        resolve_text(&html, &[], Limits::default())
            .unwrap()
            .value
            .is_none()
    );
}

#[test]
fn explicit_empty_field_attribute_and_replacement_stay_distinct_from_unavailable() {
    let doc = document("<x name='Value'/>");
    let result = resolve_text(
        &doc,
        &[],
        Limits {
            pieces: 0,
            references: 0,
            input_bytes: 0,
            expanded_bytes: 0,
            metadata_bytes: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(result.value.as_deref(), Some(""));
    let doc = document("<x name='Root' v=''/>");
    assert_eq!(
        resolve(
            &doc,
            &payload(&doc),
            &attribute(&doc, "v"),
            &[],
            Limits::default()
        )
        .unwrap()
        .value
        .as_deref(),
        Some("")
    );
    let doc = document("<x name='Value'>&empty;&empty;</x>");
    let values = definitions(&[("empty", "")]);
    let result = resolve_text(
        &doc,
        &values,
        Limits {
            expanded_bytes: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(result.value.as_deref(), Some(""));
    assert_eq!(result.usage.references, 2);
    assert_eq!(result.usage.pieces, 2);
    assert!(
        resolve_text(
            &doc,
            &values,
            Limits {
                references: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        resolve_text(
            &doc,
            &values,
            Limits {
                pieces: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        resolve_text(&doc, &[], Limits::default())
            .unwrap()
            .value
            .is_none()
    );
}

#[test]
fn exact_expansion_and_work_caps_succeed_and_each_one_under_refuses() {
    let doc = document(TEXT);
    let values = definitions(&[("person", "王 & L"), ("empty", "")]);
    let result = resolve_text(&doc, &values, Limits::default()).unwrap();
    let exact = Limits {
        definitions: 2,
        environment_bytes: result.usage.environment_bytes,
        pieces: result.usage.pieces,
        references: result.usage.references,
        input_bytes: result.usage.input_bytes,
        expanded_bytes: result.usage.expanded_bytes,
        metadata_bytes: result.usage.metadata_bytes,
        ..Limits::default()
    };
    assert!(resolve_text(&doc, &values, exact).is_ok());
    for limits in [
        Limits {
            definitions: 1,
            ..exact
        },
        Limits {
            environment_bytes: exact.environment_bytes - 1,
            ..exact
        },
        Limits {
            pieces: exact.pieces - 1,
            ..exact
        },
        Limits {
            references: exact.references - 1,
            ..exact
        },
        Limits {
            input_bytes: exact.input_bytes - 1,
            ..exact
        },
        Limits {
            expanded_bytes: exact.expanded_bytes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
    ] {
        assert!(resolve_text(&doc, &values, limits).is_err());
    }
    assert!(
        resolve_text(
            &doc,
            &values,
            Limits {
                document: super::super::Limits {
                    source_bytes: TEXT.len() - 1,
                    ..super::super::Limits::default()
                },
                ..exact
            }
        )
        .is_err()
    );
}

#[test]
fn numeric_unicode_validation_comes_from_pinned_api_and_malformed_attributes_refuse() {
    for reference in [
        "&#0;",
        "&#xD800;",
        "&#x110000;",
        "&#4294967296;",
        "&#+65;",
        "&#-1;",
        "&#x;",
        "&#X41;",
    ] {
        let doc = document(&format!("<x name='Value'>{reference}</x>"));
        assert!(
            resolve_text(&doc, &[], Limits::default()).is_err(),
            "{reference}"
        );
    }
    for raw in ["A &oops B", "A &oops &amp;"] {
        let doc = document(&format!("<x name='Root' v='{raw}'/>"));
        assert!(
            resolve(
                &doc,
                &payload(&doc),
                &attribute(&doc, "v"),
                &[],
                Limits::default()
            )
            .is_err()
        );
    }
    let doc = document("<x name='Value'>&#x1F642;&#937;&#1;</x>");
    // The pinned tokenizer explicitly does not implement complete XML LegalChar
    // checking. Its admitted nonzero control is preserved, without a display claim.
    assert_eq!(
        resolve_text(&doc, &[], Limits::default())
            .unwrap()
            .value
            .as_deref(),
        Some("🙂Ω\u{1}")
    );
}

#[test]
fn stale_selection_nested_markup_reserved_duplicate_and_nested_definitions_refuse() {
    let doc = document(TEXT);
    let selected = selection(&doc, "Value");
    assert!(resolve(&doc, &"0".repeat(64), &selected, &[], Limits::default()).is_err());
    assert!(
        resolve(
            &doc,
            &payload(&doc),
            &Selection::ElementText {
                node: usize::MAX,
                span: Span {
                    start: 0,
                    end: TEXT.len()
                }
            },
            &[],
            Limits::default()
        )
        .is_err()
    );
    let Selection::ElementText { node, span } = selected else {
        unreachable!()
    };
    assert!(
        resolve(
            &doc,
            &payload(&doc),
            &Selection::ElementText {
                node,
                span: Span {
                    end: span.end + 1,
                    ..span
                }
            },
            &[],
            Limits::default()
        )
        .is_err()
    );
    let Selection::Attribute {
        node,
        name_span,
        value_span,
    } = attribute(&doc, "note")
    else {
        unreachable!()
    };
    assert!(
        resolve(
            &doc,
            &payload(&doc),
            &Selection::Attribute {
                node,
                name_span: Span {
                    start: name_span.start + 1,
                    ..name_span
                },
                value_span
            },
            &[],
            Limits::default()
        )
        .is_err()
    );
    for values in [
        definitions(&[("amp", "override")]),
        definitions(&[("#65", "override")]),
        definitions(&[("", "")]),
        definitions(&[("x", "one"), ("x", "two")]),
        definitions(&[("x", "&nested;")]),
        definitions(&[("x", "A & B;")]),
        definitions(&[("x", "\0")]),
    ] {
        assert!(resolve_text(&doc, &values, Limits::default()).is_err());
    }
    let nested = document("<x name='Value'><copy src='parent()' trait='width'/></x>");
    assert!(resolve_text(&nested, &[], Limits::default()).is_err());
    let values = definitions(&[("x", &"&".repeat(65536))]);
    assert!(resolve_text(&doc, &values, Limits::default()).is_err()); // byte charge precedes content scan
}

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fallout-preview-entities-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
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
                .starts_with("fallout-preview-entities-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn request_and_report_exact_byte_caps_and_unknown_malformed_unicode_fields_refuse() {
    let files = Files::new();
    let doc = document(TEXT);
    let request = Request {
        schema_version: 1,
        source: includes::Source {
            path: "menus/authored.xml".into(),
            archive_sha256: "0".repeat(64),
            payload_sha256: payload(&doc),
        },
        selection: selection(&doc, "Value"),
        definitions: definitions(&[("person", "王 & L"), ("empty", "")]),
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    let file = files.0.join("request.json");
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
    json["inferred_entities"] = true.into();
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(read_request(&file, Limits::default()).is_err());
    let malformed = serde_json::to_string(&request)
        .unwrap()
        .replace("王 & L", r"\ud800");
    fs::write(&file, malformed).unwrap();
    let error = read_request(&file, Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("surrogate") || error.contains("hex escape"));
    let resolution = resolve(
        &doc,
        &payload(&doc),
        &request.selection,
        &request.definitions,
        Limits::default(),
    )
    .unwrap();
    let report = Report {
        schema_version: 1,
        source: super::super::Report {
            schema_version: 1,
            source: fallout_data::vfs::AssetSource {
                container: "authored.bsa".into(),
                entry_index: 0,
                original_path: b"menus/authored.xml".to_vec(),
            },
            archive_sha256: request.source.archive_sha256.clone(),
            payload_sha256: payload(&doc),
            document: doc,
            selected_element: None,
            interpretation: "authored source observation",
            original_display_ready: false,
        },
        request: &request,
        resolution,
        interpretation: "authored selected value",
        original_display_ready: false,
    };
    let mut output = Vec::new();
    write_report(&mut output, &report, Limits::default().output_bytes).unwrap();
    assert!(write_report(Vec::new(), &report, output.len()).is_ok());
    assert!(write_report(Vec::new(), &report, output.len() - 1).is_err());
}
