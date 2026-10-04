use fallout_data::{
    Error,
    vfs::profile::{self, Limits, LineKind, State},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

struct Inputs {
    root: tempfile::TempDir,
    install: PathBuf,
    documents: PathBuf,
    appdata: PathBuf,
}
impl Inputs {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("installation");
        let documents = root.path().join("redirected-Documents");
        let appdata = root.path().join("LocalAppData");
        for directory in [
            install.join("Data"),
            documents.join("My Games/FalloutNV"),
            appdata.join("FalloutNV"),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        Self {
            root,
            install,
            documents,
            appdata,
        }
    }
    fn ini(&self) -> PathBuf {
        self.documents.join("My Games/FalloutNV/Fallout.ini")
    }
    fn observe(&self, limits: Limits) -> fallout_data::Result<profile::Snapshot> {
        profile::observe(&self.install, &self.documents, &self.appdata, limits)
    }
}

#[test]
fn physical_bytes_duplicates_sections_and_opaque_syntax_survive() {
    let inputs = Inputs::new();
    let raw = b"\xef\xbb\xbf[Archive]\r\nSArchiveList=first\r\nsarchivelist = second ; literal\n[ARCHIVE]\nSArchiveList=third\n[broken\nSArchiveList=fourth\n;comment\n\nopaque\nlegacy=\xe9";
    fs::write(inputs.ini(), raw).unwrap();
    let snapshot = inputs.observe(Limits::default()).unwrap();
    let source = &snapshot.sources[1];
    assert_eq!(source.state, State::Present);
    assert_eq!(source.bytes, Some(raw.len()));
    assert_eq!(
        source.sha256.as_deref(),
        Some(format!("{:x}", Sha256::digest(raw)).as_str())
    );
    let reconstructed: Vec<_> = source
        .lines
        .iter()
        .flat_map(|line| line.span.read(source).unwrap())
        .copied()
        .collect();
    assert_eq!(reconstructed, raw);
    assert_eq!(source.lines.len(), 11);
    for index in [2, 4] {
        assert!(matches!(
            &source.lines[index].content,
            LineKind::Setting {
                duplicate_of: Some(2),
                ..
            }
        ));
    }
    let LineKind::Setting { value, .. } = source.lines[2].content else {
        panic!("missing setting")
    };
    assert_eq!(value.read(source).unwrap(), b"second ; literal");
    assert!(matches!(source.lines[5].content, LineKind::Unparsed));
    assert!(matches!(
        source.lines[6].content,
        LineKind::Setting {
            section: None,
            duplicate_of: None,
            ..
        }
    ));
    assert!(matches!(source.lines[7].content, LineKind::Comment));
    assert!(matches!(source.lines[8].content, LineKind::Blank));
    assert!(matches!(source.lines[9].content, LineKind::Unparsed));
    let LineKind::Setting { value, .. } = source.lines[10].content else {
        panic!("legacy setting lost")
    };
    assert_eq!(value.read(source).unwrap(), b"\xe9");
    assert!(!snapshot.runtime_ready);
    assert_eq!(snapshot.unverified.len(), 4);
    let json = serde_json::to_string(&snapshot).unwrap();
    let private_root = serde_json::to_string(inputs.root.path().to_str().unwrap()).unwrap();
    assert!(!json.contains(private_root.trim_matches('"')));
}

#[test]
fn absent_empty_and_ordered_list_candidates_have_distinct_receipts() {
    let inputs = Inputs::new();
    fs::write(
        inputs.appdata.join("FalloutNV/plugins.txt"),
        b"#comment\r\n*Second.esm\nFirst.esm\n*Second.esm",
    )
    .unwrap();
    fs::write(inputs.appdata.join("FalloutNV/NVDLCList.txt"), b"").unwrap();
    let snapshot = inputs.observe(Limits::default()).unwrap();
    assert_eq!(snapshot.sources.len(), 7);
    assert_eq!(snapshot.sources[0].state, State::Missing);
    assert_eq!(snapshot.sources[0].bytes, None);
    assert_eq!(snapshot.sources[0].sha256, None);
    let empty = &snapshot.sources[4];
    assert_eq!(empty.state, State::Empty);
    assert_eq!(empty.bytes, Some(0));
    assert!(empty.lines.is_empty());
    assert_eq!(
        empty.sha256.as_deref(),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
    let list = &snapshot.sources[3];
    let entries: Vec<_> = list
        .lines
        .iter()
        .filter_map(|line| match line.content {
            LineKind::ListEntry { value } => Some(value.read(list).unwrap()),
            _ => None,
        })
        .collect();
    assert_eq!(
        entries,
        [b"*Second.esm".as_slice(), b"First.esm", b"*Second.esm"]
    );
}

fn unsupported(result: fallout_data::Result<profile::Snapshot>, expected: &str) {
    assert!(matches!(result, Err(Error::Unsupported(message)) if message.contains(expected)));
}

#[test]
fn empty_explicit_roots_never_fall_back_to_the_working_directory() {
    let inputs = Inputs::new();
    let empty = std::path::Path::new("");
    for roots in [
        (empty, inputs.documents.as_path(), inputs.appdata.as_path()),
        (inputs.install.as_path(), empty, inputs.appdata.as_path()),
        (inputs.install.as_path(), inputs.documents.as_path(), empty),
    ] {
        unsupported(
            profile::observe(roots.0, roots.1, roots.2, Limits::default()),
            "explicit nonempty paths",
        );
    }
}

#[test]
fn source_and_aggregate_limits_fail_before_reading_or_retaining_too_much() {
    let inputs = Inputs::new();
    let raw = b"[A]\nkey=value\n";
    fs::write(inputs.ini(), raw).unwrap();
    unsupported(
        inputs.observe(Limits {
            files: 6,
            ..Default::default()
        }),
        "file budget",
    );
    unsupported(
        inputs.observe(Limits {
            file_bytes: raw.len() - 1,
            ..Default::default()
        }),
        "file type/byte budget",
    );
    fs::write(inputs.install.join("Fallout_default.ini"), raw).unwrap();
    unsupported(
        inputs.observe(Limits {
            total_bytes: 2 * raw.len() - 1,
            ..Default::default()
        }),
        "aggregate byte",
    );
    let snapshot = inputs
        .observe(Limits {
            total_bytes: 2 * raw.len(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(snapshot.sources[0].sha256, snapshot.sources[1].sha256);
    drop(snapshot);
    unsupported(
        inputs.observe(Limits {
            file_bytes: 1024 * 1024 + 1,
            ..Default::default()
        }),
        "supported ceilings",
    );
}

#[test]
fn lines_keys_and_identifier_work_are_charged_across_files_and_duplicates() {
    let inputs = Inputs::new();
    fs::write(inputs.ini(), b"[A]\nK=1\nk=2").unwrap();
    unsupported(
        inputs.observe(Limits {
            lines: 2,
            ..Default::default()
        }),
        "physical line",
    );
    unsupported(
        inputs.observe(Limits {
            keys: 1,
            ..Default::default()
        }),
        "key/list entry",
    );
    unsupported(
        inputs.observe(Limits {
            identifier_bytes: 3,
            ..Default::default()
        }),
        "identifier byte",
    );
    let snapshot = inputs
        .observe(Limits {
            lines: 3,
            keys: 2,
            identifier_bytes: 4,
            ..Default::default()
        })
        .unwrap();
    drop(snapshot);
    fs::write(inputs.appdata.join("FalloutNV/plugins.txt"), b"Other.esm").unwrap();
    unsupported(
        inputs.observe(Limits {
            keys: 2,
            ..Default::default()
        }),
        "key/list entry",
    );
}

#[test]
fn encoding_and_control_errors_are_explicit_and_release_earlier_sources() {
    let inputs = Inputs::new();
    let defaults = inputs.install.join("Fallout_default.ini");
    fs::write(&defaults, b"[General]\nsLanguage=ENGLISH").unwrap();
    fs::write(inputs.ini(), b"\xff\xfe[\0A\0]").unwrap();
    unsupported(inputs.observe(Limits::default()), "UTF-16/32");
    // Failure drops previously opened Windows write-denying source handles.
    fs::write(&defaults, b"released").unwrap();
    fs::write(inputs.ini(), b"[A]\nkey=\0").unwrap();
    assert!(
        matches!(inputs.observe(Limits::default()), Err(Error::Format { source_name, offset: 8, reason })
        if source_name == "documents/My Games/FalloutNV/Fallout.ini" && reason.contains("control byte"))
    );
}

#[test]
fn source_handles_release_only_after_snapshot_drop() {
    let inputs = Inputs::new();
    fs::write(inputs.ini(), b"key=value").unwrap();
    let snapshot = inputs.observe(Limits::default()).unwrap();
    #[cfg(windows)]
    assert!(fs::write(inputs.ini(), b"replacement").is_err());
    drop(snapshot);
    fs::write(inputs.ini(), b"replacement").unwrap();
    assert_eq!(fs::read(inputs.ini()).unwrap(), b"replacement");
    assert!(
        profile::Span {
            offset: usize::MAX,
            bytes: 1
        }
        .read(&inputs.observe(Limits::default()).unwrap().sources[1])
        .is_none()
    );
}
