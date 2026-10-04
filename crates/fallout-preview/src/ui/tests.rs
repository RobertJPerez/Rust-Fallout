use super::*;

const SOURCE: &str = concat!(
    "\u{feff}\n<!--authored-->\n<include src='menus/template.xml'/>\n",
    "<rect name='Root' note='&custom;'>",
    "<width><copy src='parent()' trait='width'/><sub> 17 </sub><unknown value='2'/></width>",
    "<text name='Button'><id>42</id><string> A &label; B <![CDATA[C & D]]></string></text>",
    "</rect>"
);

fn parsed() -> Document {
    parse(SOURCE.as_bytes().to_vec(), Limits::default()).unwrap()
}
fn child<'a>(document: &'a Document, parent: usize, name: &str) -> (usize, &'a Node) {
    document.nodes[parent]
        .children
        .iter()
        .copied()
        .find_map(|id| {
            let node = &document.nodes[id];
            (node.name.is_some_and(|span| document.text(span) == name)).then_some((id, node))
        })
        .unwrap()
}

#[test]
fn retained_source_topology_preserves_operators_actions_and_unresolved_values() {
    let d = parsed();
    assert_eq!(d.source_utf8, SOURCE);
    assert_eq!(d.utf8_bom_bytes, 3);
    let root = d.named_element("Root").unwrap();
    let rect = &d.nodes[root];
    assert_eq!(d.text(rect.open), "<rect name='Root' note='&custom;'>");
    assert_eq!(d.text(rect.close.unwrap()), "</rect>");
    assert_eq!(d.text(rect.attributes[1].raw_value), "&custom;");
    assert_eq!(rect.attributes.len(), 2);
    let (width, value) = child(&d, root, "width");
    assert_eq!(value.parent, Some(root));
    let operator_names: Vec<_> = value
        .children
        .iter()
        .map(|id| d.text(d.nodes[*id].name.unwrap()))
        .collect();
    assert_eq!(operator_names, ["copy", "sub", "unknown"]);
    let (_, copy) = child(&d, width, "copy");
    assert!(copy.empty_element);
    assert_eq!(d.text(copy.span), "<copy src='parent()' trait='width'/>");
    assert_eq!(
        copy.attributes
            .iter()
            .map(|a| (d.text(a.name), d.text(a.raw_value)))
            .collect::<Vec<_>>(),
        [("src", "parent()"), ("trait", "width")]
    );
    let button = d.named_element("Button").unwrap();
    let (_, action) = child(&d, button, "id");
    assert_eq!(d.text(d.nodes[action.children[0]].value.unwrap()), "42");
    let (_, string) = child(&d, button, "string");
    let values: Vec<_> = string
        .children
        .iter()
        .map(|id| {
            let node = &d.nodes[*id];
            (&node.kind, d.text(node.value.unwrap()))
        })
        .collect();
    assert_eq!(
        values,
        [
            (&Kind::Text, " A "),
            (&Kind::EntityReference, "label"),
            (&Kind::Text, " B "),
            (&Kind::Cdata, "C & D")
        ]
    );
    let entity = &d.nodes[string.children[1]];
    assert_eq!(d.text(entity.span), "&label;");
    assert_eq!(d.nodes[0].span.start, 3);
    let include = d
        .nodes
        .iter()
        .find(|n| n.name.is_some_and(|s| d.text(s) == "include"))
        .unwrap();
    assert_eq!(include.parent, None);
    assert_eq!(
        d.text(include.attributes[0].raw_value),
        "menus/template.xml"
    );
}

#[test]
fn exact_boundaries_refuse_before_retained_growth_and_encoding_dtd_stay_explicit() {
    let d = parsed();
    let exact = Limits {
        source_bytes: SOURCE.len(),
        events: d.event_count,
        nodes: d.nodes.len(),
        metadata_bytes: d.charged_metadata_bytes,
        ..Limits::default()
    };
    assert!(parse(SOURCE.as_bytes().to_vec(), exact).is_ok());
    for limits in [
        Limits {
            source_bytes: SOURCE.len() - 1,
            ..exact
        },
        Limits {
            events: d.event_count - 1,
            ..exact
        },
        Limits {
            nodes: d.nodes.len() - 1,
            ..exact
        },
        Limits {
            metadata_bytes: d.charged_metadata_bytes - 1,
            ..exact
        },
        Limits {
            attributes_per_element: 1,
            ..exact
        },
        Limits { depth: 2, ..exact },
    ] {
        assert!(parse(SOURCE.as_bytes().to_vec(), limits).is_err());
    }
    let external = "<!DOCTYPE rect SYSTEM 'file:///no-such-ui-input.dtd'><rect>&external;</rect>";
    let d = parse(external.as_bytes().to_vec(), Limits::default()).unwrap();
    assert_eq!(d.nodes[0].kind, Kind::Doctype);
    assert_eq!(
        d.text(d.nodes[0].span),
        "<!DOCTYPE rect SYSTEM 'file:///no-such-ui-input.dtd'>"
    );
    assert!(
        d.nodes
            .iter()
            .any(|n| n.kind == Kind::EntityReference && d.text(n.value.unwrap()) == "external")
    );
    for source in [
        b"<a>".as_slice(),
        b"<a></b>",
        b"<a x='1' x='2'/>",
        b"<a>&dangling</a>",
        b"<?xml version='1.0' encoding='windows-1252'?><a/>",
        b"<a>\xff</a>",
        b"",
    ] {
        assert!(
            parse(source.to_vec(), Limits::default()).is_err(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn authored_name_ambiguity_is_refused_and_reports_have_a_real_output_ceiling() {
    let d = parse(
        b"<rect name='Same'/><text name='Same'/>".to_vec(),
        Limits::default(),
    )
    .unwrap();
    assert!(d.named_element("Same").is_err());
    assert!(d.named_element("Absent").is_err());
    let mut bytes = Vec::new();
    let mut writer = BoundedOutput {
        inner: &mut bytes,
        written: 0,
        limit: 4,
    };
    writer.write_all(b"1234").unwrap();
    assert!(writer.write_all(b"5").is_err());
    assert_eq!(writer.written, 4);
    assert_eq!(bytes, b"1234");
}

#[test]
fn original_style_divider_comments_remain_opaque_and_unclosed_comments_refuse() {
    let source = b"<!-- source ---- divider --><rect name='Root'/>";
    let d = parse(source.to_vec(), Limits::default()).unwrap();
    assert_eq!(d.nodes[0].kind, Kind::Comment);
    assert_eq!(d.text(d.nodes[0].span), "<!-- source ---- divider -->");
    assert_eq!(d.named_element("Root").unwrap(), 1);
    assert!(parse(b"<!-- source ---- divider".to_vec(), Limits::default()).is_err());
}
