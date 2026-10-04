use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const A: &str = "<rect name='Root'><a>0</a><b><copy src='root' trait='a'/></b><c><copy src='root' trait='a'/></c><d><copy src='root' trait='b'/><add src='root' trait='c'/></d><e>unused</e></rect>";
const B: &str = "<rect name='Other'><d><copy src='other' trait='d'/></d><e>safe</e></rect>";
fn documents(sources: &[&str]) -> Vec<Document> {
    sources
        .iter()
        .map(|s| {
            super::super::parse(s.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
        })
        .collect()
}
fn point(documents: &[Document], source: usize, node: usize) -> Endpoint {
    Endpoint {
        source,
        node,
        span: documents[source].nodes[node].span,
        name_span: documents[source].nodes[node].name.unwrap(),
    }
}
fn make_operator(document: &Document, node: usize) -> Operator {
    let n = &document.nodes[node];
    let attribute = |name| {
        let a = n
            .attributes
            .iter()
            .find(|a| document.text(a.name) == name)
            .unwrap();
        Operand {
            name_span: a.name,
            value_span: a.raw_value,
        }
    };
    Operator {
        node,
        span: n.span,
        src: attribute("src"),
        trait_operand: attribute("trait"),
    }
}
fn bind(
    documents: &[Document],
    from: (usize, usize),
    to: (usize, usize),
    operator: usize,
) -> Binding {
    Binding {
        from: point(documents, from.0, from.1),
        to: point(documents, to.0, to.1),
        operator: make_operator(&documents[to.0], operator),
    }
}
fn make_request(documents: &[Document]) -> Request {
    Request {
        schema_version: 1,
        sources: documents
            .iter()
            .enumerate()
            .map(|(i, d)| includes::Source {
                path: format!("menus/authored{i}.xml"),
                archive_sha256: "0".repeat(64),
                payload_sha256: format!("{:x}", Sha256::digest(d.source_utf8.as_bytes())),
            })
            .collect(),
        bindings: vec![
            bind(documents, (0, 1), (0, 3), 4),
            bind(documents, (0, 1), (0, 5), 6),
            bind(documents, (0, 3), (0, 7), 8),
            bind(documents, (0, 5), (0, 7), 9),
            bind(documents, (0, 7), (1, 1), 2),
            bind(documents, (0, 1), (0, 3), 4),
        ],
        inputs: vec![
            Input {
                endpoint: point(documents, 0, 1),
                value: "0".into(),
            },
            Input {
                endpoint: point(documents, 0, 10),
                value: "unused".into(),
            },
        ],
        changes: vec![],
    }
}
fn change(request: &Request, revision: u64, endpoint: Endpoint, value: &str) -> Change {
    Change {
        cohort_sha256: cohort_sha256(&request.sources),
        expected_revision: revision,
        updates: vec![Input {
            endpoint,
            value: value.into(),
        }],
    }
}
fn snapshot(session: &Session<'_>) -> serde_json::Value {
    serde_json::to_value(session.snapshot()).unwrap()
}
#[test]
fn two_document_diamond_reverse_index_is_once_only_and_noop_preserves_revision() {
    let docs = documents(&[A, B]);
    let request = make_request(&docs);
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(session.graph.graph_nodes, 6);
    assert_eq!(session.graph.bindings, 6);
    assert_eq!(session.graph.unique_edges, 5);
    assert_eq!(session.graph.source.files, 2);
    assert_eq!(session.graph.source.bytes, A.len() + B.len());
    let report = session
        .change(&change(&request, 0, point(&docs, 0, 1), "1"))
        .unwrap();
    assert_eq!(report.changed_inputs, [point(&docs, 0, 1)]);
    assert_eq!(
        report.affected,
        [
            point(&docs, 0, 5),
            point(&docs, 0, 3),
            point(&docs, 0, 7),
            point(&docs, 1, 1)
        ]
    );
    assert_eq!(report.usage.visits, 10);
    assert_eq!(report.usage.copied_value_bytes, 1);
    assert_eq!((report.previous_revision, report.revision), (0, 1));
    assert_eq!(
        snapshot(&session),
        serde_json::json!([
        {"endpoint":point(&docs,0,1),"value":"1"},{"endpoint":point(&docs,0,10),"value":"unused"}])
    );
    let before = snapshot(&session);
    let noop = session
        .change(&change(&request, 1, point(&docs, 0, 1), "1"))
        .unwrap();
    assert_eq!((noop.previous_revision, noop.revision), (1, 1));
    assert!(noop.changed_inputs.is_empty() && noop.affected.is_empty());
    assert_eq!(noop.usage.visits, 0);
    assert_eq!(noop.usage.copied_value_bytes, 0);
    assert_eq!(snapshot(&session), before);
    let unrelated = session
        .change(&change(&request, 1, point(&docs, 0, 10), "caller"))
        .unwrap();
    assert_eq!(unrelated.changed_inputs, [point(&docs, 0, 10)]);
    assert!(unrelated.affected.is_empty());
    assert_eq!(session.revision(), 2);
}
#[test]
fn explicit_nested_operator_position_and_multispan_cycles_have_exact_closed_witness() {
    let source = "<rect name='Cycle'><a><copy src='this' trait='b'/></a><b><copy src='this' trait='a'/></b></rect>";
    let docs = documents(&[source]);
    let mut request = make_request(&documents(&[A, B]));
    request.sources.truncate(1);
    request.sources[0].payload_sha256 = format!("{:x}", Sha256::digest(source.as_bytes()));
    request.bindings = vec![
        bind(&docs, (0, 3), (0, 1), 2),
        bind(&docs, (0, 1), (0, 3), 4),
    ];
    request.inputs = vec![];
    let error = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .err()
    .unwrap();
    let cycle = error.downcast_ref::<Cycle>().unwrap();
    assert_eq!(
        cycle.witness,
        [point(&docs, 0, 1), point(&docs, 0, 3), point(&docs, 0, 1)]
    );
    assert!(
        error.to_string().contains("source0 node1") && error.to_string().contains("source0 node3")
    );
    let source =
        "<rect name='Nested'><a>0</a><b><add><copy src='this' trait='a'/></add></b></rect>";
    let docs = documents(&[source]);
    request.sources[0].payload_sha256 = format!("{:x}", Sha256::digest(source.as_bytes()));
    request.bindings = vec![bind(&docs, (0, 1), (0, 3), 5)];
    request.inputs = vec![Input {
        endpoint: point(&docs, 0, 1),
        value: "0".into(),
    }];
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        session
            .change(&change(&request, 0, point(&docs, 0, 1), "1"))
            .unwrap()
            .affected,
        [point(&docs, 0, 3)]
    );
    let unknown = source
        .replace("<add>", "<unknown>")
        .replace("</add>", "</unknown>");
    let docs = documents(&[&unknown]);
    request.sources[0].payload_sha256 = format!("{:x}", Sha256::digest(unknown.as_bytes()));
    request.bindings = vec![bind(&docs, (0, 1), (0, 3), 5)];
    request.inputs = vec![Input {
        endpoint: point(&docs, 0, 1),
        value: "0".into(),
    }];
    assert!(
        Session::new(
            &docs.iter().collect::<Vec<_>>(),
            &request,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn source_cohort_and_exact_endpoint_operator_operand_positions_cannot_be_substituted() {
    let docs = documents(&[A, B]);
    for field in [
        "schema",
        "missing_document",
        "source_hash",
        "duplicate_source",
        "from_span",
        "from_name",
        "source_index",
        "from_not_trait",
        "trait_name",
        "operator_span",
        "operator_position",
        "src_span",
        "trait_span",
        "duplicate_input",
    ] {
        let mut request = make_request(&docs);
        match field {
            "schema" => request.schema_version = 2,
            "missing_document" => {
                request.sources.pop();
            }
            "source_hash" => request.sources[0].payload_sha256 = "f".repeat(64),
            "duplicate_source" => request.sources[1].path = "MENUS\\AUTHORED0.XML".into(),
            "from_span" => request.bindings[0].from.span.end += 1,
            "from_name" => request.bindings[0].from.name_span.end += 1,
            "source_index" => request.bindings[0].from.source = usize::MAX,
            "from_not_trait" => request.bindings[0].from = point(&docs, 0, 0),
            "trait_name" => request.bindings[0].from = point(&docs, 0, 10),
            "operator_span" => request.bindings[0].operator.span.end += 1,
            "operator_position" => request.bindings[0].operator = make_operator(&docs[0], 6),
            "src_span" => request.bindings[0].operator.src.value_span.start += 1,
            "trait_span" => request.bindings[0].operator.trait_operand.value_span.end += 1,
            "duplicate_input" => request.inputs.push(Input {
                endpoint: point(&docs, 0, 1),
                value: "other".into(),
            }),
            _ => unreachable!(),
        }
        assert!(
            Session::new(
                &docs.iter().collect::<Vec<_>>(),
                &request,
                Limits::default()
            )
            .is_err(),
            "{field}"
        );
    }
    let docs2 = documents(&[B, A]);
    let request = make_request(&docs);
    assert!(
        Session::new(
            &docs2.iter().collect::<Vec<_>>(),
            &request,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn one_operand_cannot_resolve_to_two_different_same_named_source_traits() {
    let other = "<rect name='Other'><a>0</a></rect>";
    let docs = documents(&[A, other]);
    let mut request = make_request(&documents(&[A, B]));
    request.sources[1].payload_sha256 = format!("{:x}", Sha256::digest(other.as_bytes()));
    request.bindings = vec![
        bind(&docs, (0, 1), (0, 3), 4),
        bind(&docs, (1, 1), (0, 3), 4),
    ];
    request.inputs.truncate(1);
    let error = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("operand bound to multiple"));
    request.bindings.pop();
    request.bindings.push(bind(&docs, (0, 1), (0, 3), 4));
    let session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(session.graph.bindings, 2);
    assert_eq!(session.graph.unique_edges, 1);
}
#[test]
fn every_exact_source_graph_value_and_validation_budget_has_a_one_under_refusal() {
    let docs = documents(&[A, B]);
    let request = make_request(&docs);
    let session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    let graph = &session.graph;
    let source = includes::Limits {
        files: 2,
        source_bytes: graph.source.bytes,
        events: graph.source.events,
        nodes: graph.source.nodes,
        metadata_bytes: graph.source.metadata_bytes,
        document: super::super::Limits {
            source_bytes: A.len(),
            nodes: docs[0].nodes.len(),
            events: docs[0].event_count,
            metadata_bytes: docs[0].charged_metadata_bytes,
            ..super::super::Limits::default()
        },
        ..includes::Limits::default()
    };
    let exact = Limits {
        source,
        graph_nodes: 6,
        bindings: 6,
        inputs: 2,
        value_bytes: 7,
        value_bytes_per_input: 6,
        validation_work: graph.validation_work,
        validation_bytes: graph.validation_bytes,
        metadata_bytes: graph.metadata_bytes,
        ..Limits::default()
    };
    assert!(Session::new(&docs.iter().collect::<Vec<_>>(), &request, exact).is_ok());
    for limit in [
        Limits {
            source: includes::Limits { files: 1, ..source },
            ..exact
        },
        Limits {
            source: includes::Limits {
                source_bytes: source.source_bytes - 1,
                ..source
            },
            ..exact
        },
        Limits {
            source: includes::Limits {
                events: source.events - 1,
                ..source
            },
            ..exact
        },
        Limits {
            source: includes::Limits {
                nodes: source.nodes - 1,
                ..source
            },
            ..exact
        },
        Limits {
            source: includes::Limits {
                metadata_bytes: source.metadata_bytes - 1,
                ..source
            },
            ..exact
        },
        Limits {
            source: includes::Limits {
                document: super::super::Limits {
                    source_bytes: A.len() - 1,
                    ..source.document
                },
                ..source
            },
            ..exact
        },
        Limits {
            graph_nodes: 5,
            ..exact
        },
        Limits {
            bindings: 5,
            ..exact
        },
        Limits { inputs: 1, ..exact },
        Limits {
            value_bytes: 6,
            ..exact
        },
        Limits {
            value_bytes_per_input: 5,
            ..exact
        },
        Limits {
            validation_work: exact.validation_work - 1,
            ..exact
        },
        Limits {
            validation_bytes: exact.validation_bytes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
    ] {
        assert!(Session::new(&docs.iter().collect::<Vec<_>>(), &request, limit).is_err());
    }
}
#[test]
fn change_work_rows_cohort_identity_revision_and_value_refusals_are_atomic() {
    let docs = documents(&[A, B]);
    let request = make_request(&docs);
    let original = serde_json::to_value(&request.inputs).unwrap();
    for limit in [
        Limits {
            propagation_work: 9,
            ..Limits::default()
        },
        Limits {
            report_rows: 4,
            ..Limits::default()
        },
        Limits {
            value_bytes: 7,
            ..Limits::default()
        },
        Limits {
            value_bytes_per_input: 6,
            ..Limits::default()
        },
    ] {
        let mut session = Session::new(&docs.iter().collect::<Vec<_>>(), &request, limit).unwrap();
        let value = if limit.value_bytes == 7 {
            "xx"
        } else if limit.value_bytes_per_input == 6 {
            "1234567"
        } else {
            "1"
        };
        assert!(
            session
                .change(&change(&request, 0, point(&docs, 0, 1), value))
                .is_err()
        );
        assert_eq!(session.revision(), 0);
        assert_eq!(snapshot(&session), original);
    }
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits {
            propagation_work: 10,
            report_rows: 5,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(
        session
            .change(&change(&request, 0, point(&docs, 0, 1), "1"))
            .unwrap()
            .affected
            .len(),
        4
    );
    for field in [
        "cohort",
        "revision",
        "span",
        "name",
        "unknown",
        "derived",
        "duplicate",
        "field_cap",
    ] {
        let mut session = Session::new(
            &docs.iter().collect::<Vec<_>>(),
            &request,
            Limits::default(),
        )
        .unwrap();
        let mut change = change(&request, 0, point(&docs, 0, 1), "1");
        match field {
            "cohort" => change.cohort_sha256 = "f".repeat(64),
            "revision" => change.expected_revision = 1,
            "span" => change.updates[0].endpoint.span.end += 1,
            "name" => change.updates[0].endpoint.name_span.end += 1,
            "unknown" => change.updates[0].endpoint.node = usize::MAX,
            "derived" => change.updates[0].endpoint = point(&docs, 0, 3),
            "duplicate" => change.updates.push(Input {
                endpoint: point(&docs, 0, 1),
                value: "2".into(),
            }),
            "field_cap" => {
                session.limits.change_fields = 0;
            }
            _ => unreachable!(),
        }
        assert!(session.change(&change).is_err(), "{field}");
        assert_eq!(session.revision(), 0);
        assert_eq!(snapshot(&session), original);
    }
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    session.revision = u64::MAX;
    assert!(
        session
            .change(&change(&request, u64::MAX, point(&docs, 0, 1), "1"))
            .is_err()
    );
    assert_eq!(session.revision(), u64::MAX);
    assert_eq!(snapshot(&session), original);
    let noop = session
        .change(&change(&request, u64::MAX, point(&docs, 0, 1), "0"))
        .unwrap();
    assert_eq!(noop.revision, u64::MAX);
    assert!(noop.affected.is_empty());
}
#[test]
fn complete_batch_value_admission_is_order_independent_and_report_refusal_copies_nothing() {
    let docs = documents(&[A, B]);
    let request = make_request(&docs);
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits {
            value_bytes: 7,
            value_bytes_per_input: 6,
            ..Limits::default()
        },
    )
    .unwrap();
    let mut change = change(&request, 0, point(&docs, 0, 1), "unused");
    change.updates.push(Input {
        endpoint: point(&docs, 0, 10),
        value: "0".into(),
    });
    assert_eq!(session.change(&change).unwrap().revision, 1);
    assert_eq!(
        snapshot(&session),
        serde_json::json!([
        {"endpoint":point(&docs,0,1),"value":"unused"},{"endpoint":point(&docs,0,10),"value":"0"}])
    );
    let before = snapshot(&session);
    change.expected_revision = 1;
    change.updates[0].value = "1".into();
    assert!(session.change_bounded(&change, 4).is_err());
    assert_eq!(snapshot(&session), before);
    assert_eq!(session.revision(), 1);
}
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fallout-preview-dependency-{}-{}-{}",
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
                .starts_with("fallout-preview-dependency-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn strict_request_and_complete_multi_source_report_have_exact_byte_boundaries() {
    let docs = documents(&[A, B]);
    let mut request = make_request(&docs);
    request.changes = vec![
        change(&request, 0, point(&docs, 0, 1), "1"),
        change(&request, 1, point(&docs, 0, 1), "1"),
    ];
    let files = Files::new();
    let file = files.0.join("request.json");
    let bytes = serde_json::to_vec(&request).unwrap();
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
    let mut unknown = serde_json::to_value(&request).unwrap();
    unknown["copy_path_policy"] = "guess".into();
    fs::write(&file, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert!(read_request(&file, Limits::default()).is_err());
    let malformed = serde_json::to_string(&request)
        .unwrap()
        .replace("unused", r"\ud800");
    fs::write(&file, malformed).unwrap();
    let error = read_request(&file, Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("surrogate") || error.contains("hex escape"));
    assert!(
        Session::new(
            &docs.iter().collect::<Vec<_>>(),
            &request,
            Limits {
                steps: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let mut session = Session::new(
        &docs.iter().collect::<Vec<_>>(),
        &request,
        Limits::default(),
    )
    .unwrap();
    let changes: Vec<_> = request
        .changes
        .iter()
        .map(|change| session.change(change).unwrap())
        .collect();
    let final_inputs = session.snapshot();
    let final_revision = session.revision();
    let cohort_sha256 = session.cohort().into();
    let graph = session.graph;
    let sources = docs
        .into_iter()
        .enumerate()
        .map(|(i, document)| super::super::Report {
            schema_version: 1,
            source: fallout_data::vfs::AssetSource {
                container: "authored.bsa".into(),
                entry_index: i,
                original_path: request.sources[i].path.as_bytes().to_vec(),
            },
            archive_sha256: "0".repeat(64),
            payload_sha256: request.sources[i].payload_sha256.clone(),
            document,
            selected_element: None,
            interpretation: "authored exact source",
            original_display_ready: false,
        })
        .collect();
    let report = Report {
        schema_version: 1,
        sources,
        request: &request,
        cohort_sha256,
        graph,
        changes,
        final_revision,
        final_inputs,
        interpretation: "authored dependency work",
        original_display_ready: false,
    };
    let mut bytes = Vec::new();
    write_report(&mut bytes, &report, Limits::default().output_bytes).unwrap();
    assert!(write_report(Vec::new(), &report, bytes.len()).is_ok());
    assert!(write_report(Vec::new(), &report, bytes.len() - 1).is_err());
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["changes"][0]["revision"], 1);
    assert_eq!(json["changes"][1]["affected"], serde_json::json!([]));
    assert_eq!(json["final_revision"], 1);
    assert_eq!(json["sources"][0]["document"]["source_utf8"], A);
    assert_eq!(json["sources"][1]["document"]["source_utf8"], B);
}
