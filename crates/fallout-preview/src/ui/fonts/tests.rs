use super::*;
use clap::Parser;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

const XML: &str = "<text name='Caption'><font> 7 </font><string>&unsupplied;</string><x><copy src='parent()' trait='x'/></x></text>";
const INI: &[u8] =
    b"\xef\xbb\xbf[Fonts]\r\nsFontFile_7=Textures\\Fonts\\Authored.fnt\r\n;legacy=\xe9\r\n";
const FONT: &[u8] = b"AUTHORED OPAQUE FNT\x00\xff\x7f";
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn archive(folder: &[u8], name: &[u8], payload: &[u8]) -> Vec<u8> {
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
    bytes
}
fn dds() -> Vec<u8> {
    let mut bytes = vec![0; 136];
    bytes[..4].copy_from_slice(b"DDS ");
    for (at, value) in [
        (4, 124u32),
        (8, 0x81007),
        (12, 4),
        (16, 4),
        (20, 8),
        (28, 1),
        (76, 32),
        (80, 4),
        (108, 0x1000),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[84..88].copy_from_slice(b"DXT1");
    bytes[128..130].copy_from_slice(&0xffffu16.to_le_bytes());
    bytes
}
fn document(xml: &str) -> Document {
    super::super::parse(xml.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
}
struct Fixture {
    root: PathBuf,
    install: PathBuf,
    ini_path: PathBuf,
    request: Request,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local")
            .join(format!(
                "v3-view-22-fixture-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let install = root.join("install");
        let documents = root.join("redirected-documents");
        let local_appdata = root.join("local-appdata");
        let ini_path = documents.join("My Games/FalloutNV/Fallout.ini");
        for path in [
            install.join("Data"),
            ini_path.parent().unwrap().to_path_buf(),
            local_appdata.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(&ini_path, INI).unwrap();
        let xml = archive(b"menus", b"authored.xml", XML.as_bytes());
        let font = archive(b"textures/fonts", b"authored.fnt", FONT);
        let texture = archive(b"textures", b"atlas.dds", &dds());
        for (name, bytes) in [
            ("xml.bsa", &xml),
            ("font.bsa", &font),
            ("texture.bsa", &texture),
        ] {
            fs::write(install.join("Data").join(name), bytes).unwrap();
        }
        let doc = document(XML);
        Self {
            root,
            install,
            ini_path,
            request: Request {
                schema_version: 1,
                policy: POLICY.into(),
                source: includes::Source {
                    path: "menus/authored.xml".into(),
                    archive_sha256: hash(&xml),
                    payload_sha256: hash(XML.as_bytes()),
                },
                text: Selection {
                    node: 0,
                    span: doc.nodes[0].span,
                    name: Some("Caption".into()),
                },
                slot: 7,
                ini: IniSelection {
                    documents: documents.canonicalize().unwrap(),
                    local_appdata: local_appdata.canonicalize().unwrap(),
                    source: INI_SOURCES[1].into(),
                    sha256: hash(INI),
                    section: "Fonts".into(),
                    key: "sFontFile_7".into(),
                    section_line: Span { start: 0, end: 12 },
                    entry_line: Span { start: 12, end: 53 },
                },
                font: FontMember {
                    path: "Textures\\Fonts\\Authored.fnt".into(),
                    archive_sha256: Some(hash(&font)),
                    payload_sha256: Some(hash(FONT)),
                },
                textures: vec![includes::Source {
                    path: "TEXTURES/ATLAS.DDS".into(),
                    archive_sha256: hash(&texture),
                    payload_sha256: hash(&dds()),
                }],
            },
        }
    }
    fn profile(&self) -> profile::Snapshot {
        profile::observe(
            &self.install,
            &self.request.ini.documents,
            &self.request.ini.local_appdata,
            profile::Limits::default(),
        )
        .unwrap()
    }
    fn change_xml(&mut self, xml: &str) {
        let bytes = archive(b"menus", b"authored.xml", xml.as_bytes());
        fs::write(self.install.join("Data/xml.bsa"), &bytes).unwrap();
        self.request.source.archive_sha256 = hash(&bytes);
        self.request.source.payload_sha256 = hash(xml.as_bytes());
        self.request.text.span = document(xml).nodes[0].span;
    }
}

#[test]
fn literal_font_words_spans_and_uninterpreted_original_text_fields_are_preserved() {
    let fixture = Fixture::new();
    let doc = document(XML);
    let selected = project(&doc, &fixture.request, Limits::default()).unwrap();
    assert_eq!(selected.node, 1);
    assert_eq!(selected.span, Span { start: 21, end: 37 });
    assert_eq!(selected.inner_span, Span { start: 27, end: 30 });
    assert_eq!(selected.bits, 0x40e00000);
    assert_eq!(selected.slot, 7);
    assert_eq!(doc.text(selected.inner_span), " 7 ");
    assert!(
        selected
            .projection
            .rows
            .iter()
            .any(|r| r.name == "string" && !matches!(r.outcome, traits::Outcome::Value { .. }))
    );
    assert!(
        selected
            .projection
            .rows
            .iter()
            .any(|r| r.name == "x" && !matches!(r.outcome, traits::Outcome::Value { .. }))
    );
    let snapshot = fixture.profile();
    let ini = select_ini(&snapshot, &fixture.request.ini, Limits::default()).unwrap();
    let source = &snapshot.sources[ini.source_index];
    assert_eq!(source.raw_bytes(), INI);
    assert_eq!(
        ini.key,
        profile::Span {
            offset: 12,
            bytes: 11
        }
    );
    assert_eq!(
        ini.value,
        profile::Span {
            offset: 24,
            bytes: 27
        }
    );
    assert_eq!(ini.path.bytes(), b"textures/fonts/authored.fnt");
    assert_eq!(ini.section_name.read(source), Some(b"Fonts".as_slice()));
}

#[test]
fn missing_duplicate_operator_fractional_nonfinite_font_or_wrong_text_identity_refuses() {
    for xml in [
        XML.replace("<font> 7 </font>", ""),
        XML.replace(" 7 ", ""),
        XML.replace("<font> 7 </font>", "<font>7</font><font>7</font>"),
        XML.replace(" 7 ", "<copy src='parent()' trait='font'/>"),
        XML.replace(" 7 ", "&font;"),
        XML.replace(" 7 ", "7.5"),
        XML.replace(" 7 ", "NaN"),
        XML.replace(" 7 ", "-7"),
        XML.replace(" 7 ", "8"),
        XML.replace("<text ", "<rect ")
            .replace("</text>", "</rect>"),
    ] {
        let mut fixture = Fixture::new();
        fixture.change_xml(&xml);
        assert!(
            project(&document(&xml), &fixture.request, Limits::default()).is_err(),
            "{xml}"
        );
    }
    let mut fixture = Fixture::new();
    let xml = format!("{XML}<text name='Caption'/>");
    fixture.change_xml(&xml);
    assert!(project(&document(&xml), &fixture.request, Limits::default()).is_err());
    fixture.request.text.name = None;
    assert!(project(&document(&xml), &fixture.request, Limits::default()).is_ok());
    fixture.request.text.span.end += 1;
    assert!(project(&document(&xml), &fixture.request, Limits::default()).is_err());
    fixture.request.text.span.end -= 1;
    fixture.request.slot = u32::MAX;
    assert!(project(&document(&xml), &fixture.request, Limits::default()).is_err());
}

#[test]
fn exact_ini_spelling_sections_keys_spans_and_source_hash_never_choose_a_winner() {
    for suffix in [
        b"[fonts]\nsFontFile_7=Textures\\Fonts\\Authored.fnt".as_slice(),
        b"sfontfile_7=Textures\\Fonts\\Authored.fnt",
        b"sFontFile_7=Textures\\Fonts\\Other.fnt",
    ] {
        let mut fixture = Fixture::new();
        let bytes = [INI, suffix].concat();
        fs::write(&fixture.ini_path, &bytes).unwrap();
        fixture.request.ini.sha256 = hash(&bytes);
        assert!(select_ini(&fixture.profile(), &fixture.request.ini, Limits::default()).is_err());
    }
    for field in [
        "hash",
        "section-case",
        "key-case",
        "section-span",
        "entry-span",
        "source",
    ] {
        let mut fixture = Fixture::new();
        match field {
            "hash" => fixture.request.ini.sha256 = "f".repeat(64),
            "section-case" => fixture.request.ini.section = "fonts".into(),
            "key-case" => fixture.request.ini.key = "sfontfile_7".into(),
            "section-span" => fixture.request.ini.section_line.end -= 2,
            "entry-span" => fixture.request.ini.entry_line.end -= 2,
            "source" => fixture.request.ini.source = INI_SOURCES[0].into(),
            _ => unreachable!(),
        }
        assert!(
            select_ini(&fixture.profile(), &fixture.request.ini, Limits::default()).is_err(),
            "{field}"
        );
    }
    // Existing producer resets the section after malformed syntax; the consumer
    // keeps the earlier exact selection and never treats the later key as an override.
    let mut fixture = Fixture::new();
    let bytes = [INI, b"[broken\nsFontFile_7=elsewhere"].concat();
    fs::write(&fixture.ini_path, &bytes).unwrap();
    fixture.request.ini.sha256 = hash(&bytes);
    assert!(select_ini(&fixture.profile(), &fixture.request.ini, Limits::default()).is_ok());
}

#[test]
fn actual_font_payload_lease_retains_identity_bytes_profile_pins_and_releases_last_owner() {
    let fixture = Fixture::new();
    let (report, lease) = bind(&fixture.install, &fixture.request, Limits::default()).unwrap();
    let lease = lease.expect("complete dependency payload lease");
    assert!(Arc::ptr_eq(&report.profile, &lease.profile));
    assert_eq!(lease.font.bytes.as_ref(), FONT);
    assert_eq!(lease.textures[0].bytes.as_ref(), dds());
    assert_eq!(lease.text.path.bytes(), b"menus/authored.xml");
    assert_eq!(lease.text.payload_sha256, hash(XML.as_bytes()));
    assert_eq!(lease.text.text_node, 0);
    assert_eq!(lease.text.font_node, 1);
    assert_eq!(lease.text.font_bits, 0x40e00000);
    assert_eq!(lease.font.receipt.payload_sha256, hash(FONT));
    assert_eq!(report.usage.retained_payload_bytes, FONT.len() + 136);
    assert!(matches!(report.outcome, Outcome::PayloadsBound { .. }));
    assert!(
        !report.font_codec_available
            && !report.font_layout_available
            && !report.font_texture_relationship_verified
            && !report.original_display_ready
    );
    let copy = lease.clone();
    drop(report);
    drop(lease);
    #[cfg(windows)]
    assert!(fs::write(&fixture.ini_path, b"must stay pinned").is_err());
    // Archive receipts are preparation snapshots; the immutable bytes do not
    // change when a private authored archive is subsequently replaced.
    fs::write(
        fixture.install.join("Data/font.bsa"),
        b"private replacement",
    )
    .unwrap();
    assert_eq!(copy.font.bytes.as_ref(), FONT);
    drop(copy);
    fs::write(&fixture.ini_path, INI).unwrap();
}

#[test]
fn missing_member_diagnostics_are_complete_and_never_retain_a_payload_prefix() {
    for missing in ["font", "texture", "both"] {
        let mut fixture = Fixture::new();
        if missing != "texture" {
            fs::remove_file(fixture.install.join("Data/font.bsa")).unwrap();
            fixture.request.font.archive_sha256 = None;
            fixture.request.font.payload_sha256 = None;
        }
        if missing != "font" {
            fs::remove_file(fixture.install.join("Data/texture.bsa")).unwrap();
        }
        let (report, lease) = bind(&fixture.install, &fixture.request, Limits::default()).unwrap();
        assert!(lease.is_none());
        assert_eq!(report.usage.retained_payload_bytes, 0);
        let Outcome::MissingMembers {
            missing: members,
            present_payload_identities_unverified,
        } = &report.outcome
        else {
            panic!("missing must stay unavailable")
        };
        assert!(*present_payload_identities_unverified);
        assert_eq!(
            members.iter().map(|m| m.request_index).collect::<Vec<_>>(),
            match missing {
                "font" => vec![0],
                "texture" => vec![1],
                _ => vec![0, 1],
            }
        );
        assert!(!report.original_display_ready);
    }
}

#[test]
fn ambiguous_conflicting_changed_or_unbound_members_refuse_and_drop_all_owned_source_pins() {
    for failure in [
        "font-duplicate",
        "font-conflict",
        "texture-duplicate",
        "texture-conflict",
        "font-payload",
        "font-archive",
        "texture-payload",
        "texture-archive",
        "xml-payload",
        "xml-archive",
        "present-unbound",
        "ini-path",
    ] {
        let mut fixture = Fixture::new();
        match failure {
            "font-duplicate" | "texture-duplicate" => {
                let name = if failure.starts_with("font") {
                    "font.bsa"
                } else {
                    "texture.bsa"
                };
                fs::copy(
                    fixture.install.join("Data").join(name),
                    fixture.install.join("Data/duplicate.bsa"),
                )
                .unwrap();
            }
            "font-conflict" => fs::write(
                fixture.install.join("Data/conflict.bsa"),
                archive(b"Textures/Fonts", b"AUTHORED.FNT", b"other"),
            )
            .unwrap(),
            "texture-conflict" => fs::write(
                fixture.install.join("Data/conflict.bsa"),
                archive(b"textures", b"atlas.dds", b"other"),
            )
            .unwrap(),
            "font-payload" => fixture.request.font.payload_sha256 = Some("f".repeat(64)),
            "font-archive" => fixture.request.font.archive_sha256 = Some("f".repeat(64)),
            "texture-payload" => fixture.request.textures[0].payload_sha256 = "f".repeat(64),
            "texture-archive" => fixture.request.textures[0].archive_sha256 = "f".repeat(64),
            "xml-payload" => fixture.request.source.payload_sha256 = "f".repeat(64),
            "xml-archive" => fixture.request.source.archive_sha256 = "f".repeat(64),
            "present-unbound" => {
                fixture.request.font.archive_sha256 = None;
                fixture.request.font.payload_sha256 = None;
            }
            "ini-path" => fixture.request.font.path = "textures/fonts/other.fnt".into(),
            _ => unreachable!(),
        }
        assert!(
            bind(&fixture.install, &fixture.request, Limits::default()).is_err(),
            "{failure}"
        );
        fs::write(&fixture.ini_path, INI).unwrap();
    }
}

#[test]
fn exact_member_aggregate_metadata_profile_and_selected_line_caps_succeed_then_refuse_one_under() {
    let fixture = Fixture::new();
    let (report, lease) = bind(&fixture.install, &fixture.request, Limits::default()).unwrap();
    let metadata = report.usage.metadata_bytes;
    let exact = Limits {
        literal: traits::Limits {
            copy_bytes: report.selection.projection.usage.reserved_copy_bytes,
            metadata_bytes: report.selection.projection.usage.metadata_bytes,
            ..Default::default()
        },
        profile: profile::Limits {
            file_bytes: INI.len(),
            total_bytes: INI.len(),
            lines: 3,
            keys: 1,
            identifier_bytes: 16,
            ..Default::default()
        },
        selected_line_bytes: 41,
        member_bytes: 136,
        retained_bytes: FONT.len() + 136,
        textures: 1,
        metadata_bytes: metadata,
    };
    drop(lease);
    drop(report);
    assert!(bind(&fixture.install, &fixture.request, exact).is_ok());
    for limits in [
        Limits {
            selected_line_bytes: 40,
            ..exact
        },
        Limits {
            member_bytes: 135,
            ..exact
        },
        Limits {
            retained_bytes: exact.retained_bytes - 1,
            ..exact
        },
        Limits {
            textures: 0,
            ..exact
        },
        Limits {
            metadata_bytes: metadata - 1,
            ..exact
        },
        Limits {
            profile: profile::Limits {
                file_bytes: INI.len() - 1,
                ..exact.profile
            },
            ..exact
        },
        Limits {
            profile: profile::Limits {
                total_bytes: INI.len() - 1,
                ..exact.profile
            },
            ..exact
        },
        Limits {
            profile: profile::Limits {
                lines: 2,
                ..exact.profile
            },
            ..exact
        },
        Limits {
            profile: profile::Limits {
                keys: 0,
                ..exact.profile
            },
            ..exact
        },
        Limits {
            profile: profile::Limits {
                identifier_bytes: 15,
                ..exact.profile
            },
            ..exact
        },
        Limits {
            literal: traits::Limits {
                copy_bytes: exact.literal.copy_bytes - 1,
                ..exact.literal
            },
            ..exact
        },
        Limits {
            literal: traits::Limits {
                metadata_bytes: exact.literal.metadata_bytes - 1,
                ..exact.literal
            },
            ..exact
        },
    ] {
        assert!(bind(&fixture.install, &fixture.request, limits).is_err());
        fs::write(&fixture.ini_path, INI).unwrap();
    }
    assert!(
        bind(
            &fixture.install,
            &fixture.request,
            Limits {
                member_bytes: 8 * 1024 * 1024 + 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        bind(
            &fixture.install,
            &fixture.request,
            Limits {
                profile: profile::Limits {
                    lines: 16385,
                    ..Default::default()
                },
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn strict_font_request_report_output_and_distinct_cli_mode_boundaries_are_enforced() {
    let fixture = Fixture::new();
    let value = serde_json::to_value(&fixture.request).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let path = fixture.root.join("request.json");
    fs::write(&path, &bytes).unwrap();
    let limits = Limits {
        literal: traits::Limits {
            request_bytes: bytes.len(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(read_request(&path, limits).is_ok());
    assert!(
        read_request(
            &path,
            Limits {
                literal: traits::Limits {
                    request_bytes: bytes.len() - 1,
                    ..limits.literal
                },
                ..limits
            }
        )
        .is_err()
    );
    for kind in [
        "unknown",
        "nested",
        "missing-policy",
        "partial-hash",
        "relative-root",
        "duplicate-texture",
        "wrong-texture-path",
        "large-font-path",
    ] {
        let mut changed = value.clone();
        match kind {
            "unknown" => {
                changed["guess"] = true.into();
            }
            "nested" => {
                changed["ini"]["guess"] = 0.into();
            }
            "missing-policy" => {
                changed.as_object_mut().unwrap().remove("policy");
            }
            "partial-hash" => {
                changed["font"]["payload_sha256"] = serde_json::Value::Null;
            }
            "relative-root" => {
                changed["ini"]["documents"] = "relative".into();
            }
            "duplicate-texture" => {
                let t = changed["textures"][0].clone();
                changed["textures"].as_array_mut().unwrap().push(t);
            }
            "wrong-texture-path" => {
                changed["textures"][0]["path"] = "atlas.dds".into();
            }
            "large-font-path" => {
                changed["font"]["path"] = ("x".repeat(4097) + ".fnt").into();
            }
            _ => unreachable!(),
        }
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(read_request(&path, Limits::default()).is_err(), "{kind}");
    }
    let (report, _lease) = bind(&fixture.install, &fixture.request, Limits::default()).unwrap();
    let mut output = Vec::new();
    write_report(&mut output, &report, Limits::default().literal.output_bytes).unwrap();
    let mut exact = Vec::new();
    write_report(&mut exact, &report, output.len()).unwrap();
    assert_eq!(output, exact);
    let mut refused = Vec::new();
    assert!(write_report(&mut refused, &report, output.len() - 1).is_err());
    assert!(refused.len() < output.len());
    let good = [
        "fallout-preview",
        "--install",
        "install",
        "--menu-font",
        "request",
        "--report",
        "report",
    ];
    assert!(crate::Options::try_parse_from(good).is_ok());
    for extra in [
        vec!["--headless", "--capture", "capture"],
        vec!["--model", "meshes/never.nif"],
        vec!["--menu", "menus/never.xml"],
        vec![
            "--camera-position",
            "1,2,3",
            "--camera-look-at",
            "0,0,0",
            "--load-order",
            "order",
        ],
    ] {
        assert!(crate::Options::try_parse_from(good.into_iter().chain(extra)).is_err());
    }
}
