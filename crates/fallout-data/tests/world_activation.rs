//! Independent activation-parent literals, physical source spans and deferred headers.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::activation::{Limits, PlacedActivationSources},
};
use std::{fs, io::Write};
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}
fn header(masters: &[&[u8]]) -> Vec<u8> {
    let mut data = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        data.extend(field(b"MAST", &[*master, &[0]].concat()));
        data.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &data)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
fn body() -> Vec<u8> {
    [
        field(b"NAME", &[0, 5, 0, 0]),
        field(b"XAPR", &[1, 2, 0, 0, 0, 0, 0, 128]),
        field(b"XAPD", &[254]),
        field(b"XAPR", &[0, 2, 0, 0, 1, 0, 0, 0]),
        field(b"XATO", &[255, 128, b'O', 0]),
        field(b"ZZZZ", &[91, 92]),
        field(b"XAPR", &[1, 2, 0, 0, 0, 0, 128, 191]),
        field(b"XAPR", &[2, 2, 0, 0, 69, 35, 193, 127]),
        field(b"DATA", &[0; 24]),
    ]
    .concat()
}
const TARGETS: [&[u8; 4]; 7] = [
    b"PGRE", b"PMIS", b"PBEA", b"REFR", b"ACHR", b"ACRE", b"PLYR",
];
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(body: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let mut bytes = [header(&[]), record(b"REFR", 0x100, 0, body)].concat();
        for (index, kind) in TARGETS.iter().enumerate() {
            bytes.extend(record(
                kind,
                0x200 + index as u32,
                0,
                &field(b"ZZZZ", &[index as u8]),
            ));
        }
        fs::write(root.path().join("Data/Base.esm"), bytes).unwrap();
        Self {
            root,
            names: vec!["Base.esm".into()],
        }
    }
    fn try_store(&self, forensic: bool) -> fallout_data::Result<RecordStore> {
        let open = if forensic {
            RecordStore::open_nv
        } else {
            RecordStore::open_nv_headers
        };
        open(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits {
                inspect_checksum_mismatches: forensic,
                ..Default::default()
            },
        )
    }
    fn store(&self) -> RecordStore {
        self.try_store(false).unwrap()
    }
    fn load(
        &self,
        store: &mut RecordStore,
        limits: Limits,
    ) -> fallout_data::Result<PlacedActivationSources> {
        PlacedActivationSources::load(store, &key(0x100), limits)
    }
    fn patch(&mut self, masters: &[&[u8]], body: &[u8]) {
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [header(masters), body.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
}
#[test]
fn literal_occurrences_raw_delay_words_non_utf8_prompt_and_order_are_exact() {
    let f = Fixture::new(&body());
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    assert_eq!(r.placed.header.offset, 42);
    assert_eq!(r.placement.base.value, 0x500);
    assert_eq!(
        (
            r.usage.records,
            r.usage.fields,
            r.usage.occurrences,
            r.usage.links,
            r.usage.prompt_bytes,
            r.usage.read_bytes,
            r.usage.raw_bytes
        ),
        (5, 9, 4, 4, 4, 121, 73)
    );
    assert_eq!(r.placement.unhandled_fields["XAPR"], 4);
    assert_eq!(r.placement.unhandled_fields["XAPD"], 1);
    assert_eq!(r.placement.unhandled_fields["XATO"], 1);
    let flags = r.activation_flags.as_ref().unwrap();
    assert_eq!(flags.raw, 254);
    assert_eq!(flags.field.physical_field_ordinal, 2);
    assert_eq!(flags.field.site.decoded_header_offset, 24);
    assert_eq!(flags.field.site.span.decoded_offset, 30);
    assert_eq!(flags.field.site.span.bytes, 1);
    assert_eq!(flags.field.physical_framing_offset, Some(90));
    let prompt = r.prompt.as_ref().unwrap();
    assert_eq!(prompt.raw, [255, 128, b'O', 0]);
    assert_eq!(prompt.field.framing, field(b"XATO", &[255, 128, b'O', 0]));
    assert_eq!(prompt.field.physical_field_ordinal, 4);
    assert_eq!(prompt.field.site.decoded_header_offset, 45);
    assert_eq!(prompt.field.site.span.decoded_offset, 51);
    assert_eq!(prompt.field.site.span.bytes, 4);
    assert_eq!(prompt.field.physical_framing_offset, Some(111));
    use sha2::Digest;
    assert_eq!(
        prompt.sha256,
        format!("{:x}", sha2::Sha256::digest([255, 128, b'O', 0]))
    );
    for (index, (raw, bits, ordinal, offset, target_header)) in [
        (0x201, 0x80000000, 1, 10, 218),
        (0x200, 0x00000001, 3, 31, 187),
        (0x201, 0xbf800000, 6, 63, 218),
        (0x202, 0x7fc12345, 7, 77, 249),
    ]
    .into_iter()
    .enumerate()
    {
        let parent = &r.parents[index];
        assert_eq!(parent.parent_raw, raw);
        assert_eq!(parent.delay_bits, bits);
        assert_eq!(parent.target.key, Some(key(raw)));
        assert_eq!(parent.target.status, "resolved");
        assert_eq!(
            parent.target.expected_kinds,
            ["REFR", "ACRE", "ACHR", "PGRE", "PMIS", "PBEA", "PLYR"]
        );
        assert_eq!(parent.field.physical_field_ordinal, ordinal);
        assert_eq!(parent.field.logical_field_ordinal, ordinal);
        assert_eq!(parent.field.site.decoded_header_offset, offset);
        assert_eq!(parent.field.site.span.decoded_offset, offset + 6);
        assert_eq!(parent.field.site.span.bytes, 8);
        assert_eq!(
            parent.field.physical_framing_offset,
            Some(66 + offset as u64)
        );
        let expected = [raw.to_le_bytes(), bits.to_le_bytes()].concat();
        assert_eq!(parent.field.framing, field(b"XAPR", &expected));
        assert_eq!(parent.source.as_ref().unwrap().header.offset, target_header);
        assert!(parent.header_source_available);
        assert_eq!(
            parent.behavior_status,
            "unknown; target body, delay and activation not evaluated"
        );
    }
    let report = serde_json::to_value(&source).unwrap();
    assert_eq!(report["parents"][0]["delay_bits"], 0x80000000_u32);
    assert_eq!(report["parents"][3]["delay_bits"], 0x7fc12345_u32);
    assert!(!r.runtime_ready);
}
#[test]
fn all_seven_target_header_domains_and_three_root_kinds_keep_target_bodies_unread() {
    for (index, kind) in TARGETS.iter().enumerate() {
        let mut bytes = body();
        bytes[16..20].copy_from_slice(&(0x200 + index as u32).to_le_bytes());
        let f = Fixture::new(&bytes);
        let source = f.load(&mut f.store(), Default::default()).unwrap();
        let parent = &source.receipt().parents[0];
        assert_eq!(parent.source.as_ref().unwrap().header.kind, **kind);
        assert_eq!(
            parent.source.as_ref().unwrap().header.offset,
            187 + 31 * index as u64
        );
        assert_eq!(parent.target.status, "resolved");
        assert_eq!(source.receipt().usage.read_bytes, 121); // Invalid placement bodies are intentionally deferred.
    }
    for kind in [b"REFR", b"ACHR", b"ACRE"] {
        let mut f = Fixture::new(&body());
        f.patch(&[b"Base.esm"], &record(kind, 0x100, 0, &body()));
        assert_eq!(
            f.load(&mut f.store(), Default::default())
                .unwrap()
                .receipt()
                .placed
                .header
                .kind,
            *kind
        );
    }
}
#[test]
fn absence_flags_only_prompt_only_null_unavailable_and_runtime_player_are_explicit() {
    let core = [field(b"NAME", &[0, 5, 0, 0]), field(b"DATA", &[0; 24])].concat();
    let f = Fixture::new(&core);
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    assert!(source.receipt().parents.is_empty());
    assert!(source.receipt().activation_flags.is_none());
    assert!(source.receipt().prompt.is_none());
    for (kind, raw) in [
        (b"XAPD".as_slice(), vec![254]),
        (b"XATO".as_slice(), vec![0]),
    ] {
        let f = Fixture::new(&[core.clone(), field(kind.try_into().unwrap(), &raw)].concat());
        let source = f.load(&mut f.store(), Default::default()).unwrap();
        assert!(source.receipt().parents.is_empty());
        assert_eq!(source.receipt().activation_flags.is_some(), kind == b"XAPD");
        assert_eq!(source.receipt().prompt.is_some(), kind == b"XATO");
    }
    for (raw, kind, flags, status) in [
        (0_u32, b"PMIS", 0, "null"),
        (0x999, b"PMIS", 0, "missing"),
        (0x201, b"PMIS", plugin::DELETED, "deleted"),
        (0x201, b"STAT", 0, "wrong-record-kind"),
    ] {
        let mut bytes = body();
        bytes[16..20].copy_from_slice(&raw.to_le_bytes());
        let mut f = Fixture::new(&bytes);
        f.patch(&[b"Base.esm"], &record(kind, 0x201, flags, &[1, 2, 3]));
        let source = f.load(&mut f.store(), Default::default()).unwrap();
        let parent = &source.receipt().parents[0];
        assert_eq!(parent.target.status, status);
        assert!(!parent.header_source_available);
        assert_eq!(
            parent.source.is_some(),
            ["deleted", "wrong-record-kind"].contains(&status)
        );
    }
    let mut bytes = body();
    bytes[16..20].copy_from_slice(&0x14_u32.to_le_bytes());
    let mut f = Fixture::new(&bytes);
    fs::rename(
        f.root.path().join("Data/Base.esm"),
        f.root.path().join("Data/FalloutNV.esm"),
    )
    .unwrap();
    f.names = vec!["FalloutNV.esm".into()];
    let root = FormKey {
        origin_plugin: "falloutnv.esm".into(),
        ..key(0x100)
    };
    let source = PlacedActivationSources::load(&mut f.store(), &root, Default::default()).unwrap();
    let parent = &source.receipt().parents[0];
    assert_eq!(parent.target.status, "runtime-player-binding-unimplemented");
    assert_eq!(
        parent.target.key,
        Some(FormKey {
            local_id: 0x14,
            ..root
        })
    );
    assert!(parent.source.is_none());
    assert!(!parent.header_source_available);
}
#[test]
fn cross_master_override_preserves_canonical_identity_and_actual_parent_header() {
    let mut f = Fixture::new(&body());
    fs::write(f.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    f.names.push("Other.esm".into());
    let mut bytes = body();
    for at in [19, 40, 72, 86] {
        bytes[at] = 1;
    }
    f.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"ACHR", 0x01000100, 0, &bytes),
            record(b"PBEA", 0x01000201, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    assert_eq!(r.placed.source_ordinal, 2);
    assert_eq!(r.placed.header.offset, 101);
    let p = &r.parents[0];
    assert_eq!(p.parent_raw, 0x01000201);
    assert_eq!(p.target.key, Some(key(0x201)));
    assert_eq!(p.source.as_ref().unwrap().source_ordinal, 2);
    assert_eq!(p.source.as_ref().unwrap().header.offset, 246);
    assert_eq!(p.source.as_ref().unwrap().header.kind, *b"PBEA");
    assert_eq!(r.usage.read_bytes, 121);
}
#[test]
fn duplicate_singletons_short_parents_bad_strings_invalid_roots_and_preflight_refuse() {
    for (kind, data) in [(b"XAPD".as_slice(), vec![1]), (b"XATO".as_slice(), vec![0])] {
        let f = Fixture::new(&[body(), field(kind.try_into().unwrap(), &data)].concat());
        assert!(f.load(&mut f.store(), Default::default()).is_err());
    }
    let core = [field(b"NAME", &[0, 5, 0, 0]), field(b"DATA", &[0; 24])].concat();
    for width in [0, 1, 2, 3, 4, 7, 9, 12] {
        let f = Fixture::new(&[core.clone(), field(b"XAPR", &vec![0; width])].concat());
        assert!(f.load(&mut f.store(), Default::default()).is_err());
    }
    for bytes in [vec![], vec![1, 2], vec![0, 0], vec![1, 0, 2, 0]] {
        let f = Fixture::new(&[core.clone(), field(b"XATO", &bytes)].concat());
        assert!(f.load(&mut f.store(), Default::default()).is_err());
    }
    for width in [0, 2, 8] {
        let f = Fixture::new(&[core.clone(), field(b"XAPD", &vec![0; width])].concat());
        assert!(f.load(&mut f.store(), Default::default()).is_err());
    }
    let f = Fixture::new(&[body(), b"XAPR\x08\0\0".to_vec()].concat());
    assert!(
        f.try_store(false)
            .and_then(|mut s| f.load(&mut s, Default::default()))
            .is_err()
    );
    let f = Fixture::new(&field(b"ZZZZ", &[1]));
    let err = f
        .load(
            &mut f.store(),
            Limits {
                fields: 0,
                ..Default::default()
            },
        )
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("physical fields budget"), "{err}");
    let mut f = Fixture::new(&body());
    f.patch(&[b"Missing.esm"], &record(b"REFR", 0x100, 0, &body()));
    assert!(f.try_store(false).is_err());
    let mut f = Fixture::new(&body());
    f.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::DELETED, &body()),
    );
    assert!(f.load(&mut f.store(), Default::default()).is_err());
    let f = Fixture::new(&body());
    for root in [
        key(0x999),
        key(0x200),
        FormKey {
            profile: ProfileId::Fo3Original,
            ..key(0x100)
        },
    ] {
        assert!(PlacedActivationSources::load(&mut f.store(), &root, Default::default()).is_err());
    }
}
#[test]
fn extended_and_compressed_parent_frames_keep_both_ordinals_and_physical_unknown() {
    let mut extended = body()[..10].to_vec();
    extended.extend(field(b"XXXX", &8_u32.to_le_bytes()));
    extended.extend(b"XAPR\0\0");
    extended.extend([1, 2, 0, 0, 0, 0, 0, 128]);
    extended.extend(&body()[24..]);
    let f = Fixture::new(&extended);
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    let p = &r.parents[0];
    assert_eq!(r.usage.fields, 10);
    assert_eq!(p.field.physical_field_ordinal, 2);
    assert_eq!(p.field.logical_field_ordinal, 1);
    assert_eq!(p.field.site.decoded_header_offset, 20);
    assert_eq!(p.field.site.span.decoded_offset, 26);
    assert_eq!(p.field.decoded_framing_offset, 10);
    assert_eq!(p.field.framing.len(), 24);
    assert_eq!(p.field.physical_framing_offset, Some(76));
    assert_eq!(r.parents[1].field.physical_field_ordinal, 4);
    assert_eq!(r.parents[1].field.logical_field_ordinal, 3);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&extended).unwrap();
    let stored = [
        (extended.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    let mut f = Fixture::new(&body());
    f.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::COMPRESSED, &stored),
    );
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let p = &source.receipt().parents[0];
    assert!(p.field.physical_framing_offset.is_none());
    assert!(
        source
            .receipt()
            .prompt
            .as_ref()
            .unwrap()
            .field
            .physical_framing_offset
            .is_none()
    );
    assert_eq!(p.field.stored_body_offset, 95);
    assert_eq!(p.field.stored_body_bytes, stored.len() as u32);
    assert_eq!(p.field.site.decoded_header_offset, 20);
}
#[test]
fn all_ten_exact_allowances_and_one_under_refusal_preserve_retry_identity() {
    let f = Fixture::new(&body());
    let mut store = f.store();
    let source = f.load(&mut store, Default::default()).unwrap();
    let u = &source.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        occurrences: u.occurrences,
        links: u.links,
        prompt_bytes: u.prompt_bytes,
        record_bytes: 121,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
    };
    let identity = source.identity().to_owned();
    assert_eq!(f.load(&mut store, exact).unwrap().identity(), identity);
    let mut limits = Vec::new();
    macro_rules! under {
        ($field:ident) => {
            limits.push(Limits {
                $field: exact.$field - 1,
                ..exact
            });
        };
    }
    under!(sources);
    under!(records);
    under!(fields);
    under!(occurrences);
    under!(links);
    under!(prompt_bytes);
    under!(record_bytes);
    under!(read_bytes);
    under!(raw_bytes);
    under!(metadata_bytes);
    for limit in limits {
        assert!(f.load(&mut store, limit).is_err());
    }
    assert_eq!(f.load(&mut store, exact).unwrap().identity(), identity);
    assert!(
        f.load(
            &mut store,
            Limits {
                occurrences: 257,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn ordered_cohort_names_count_and_deferred_bytes_remain_required_for_reuse() {
    let mut f = Fixture::new(&body());
    f.patch(&[b"Base.esm"], &[]);
    fs::write(f.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    f.names.push("Other.esm".into());
    let store = f.store();
    let mut store = store;
    let source = f.load(&mut store, Default::default()).unwrap();
    drop(store);
    f.names.swap(1, 2);
    assert!(source.validate_sources(&mut f.store()).is_err());
    f.names.swap(1, 2);
    f.names.pop();
    assert!(source.validate_sources(&mut f.store()).is_err());
    f.names.push("Other.esm".into());
    fs::copy(
        f.root.path().join("Data/Other.esm"),
        f.root.path().join("Data/Renamed.esm"),
    )
    .unwrap();
    f.names[2] = "Renamed.esm".into();
    assert!(source.validate_sources(&mut f.store()).is_err());
    f.names[2] = "Other.esm".into();
    fs::write(
        f.root.path().join("Data/Other.esm"),
        [header(&[]), record(b"STAT", 0x900, 0, &[1, 2, 3])].concat(),
    )
    .unwrap();
    assert!(source.validate_sources(&mut f.store()).is_err());
}
#[test]
fn recovered_checksum_body_cannot_construct_activation_source_authority() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&body()).unwrap();
    let mut stream = encoder.finish().unwrap();
    *stream.last_mut().unwrap() ^= 1;
    let stored = [121_u32.to_le_bytes().as_slice(), &stream].concat();
    let mut f = Fixture::new(&body());
    f.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::COMPRESSED, &stored),
    );
    assert!(
        f.try_store(false)
            .and_then(|mut store| f.load(&mut store, Default::default()))
            .is_err()
    );
    assert!(
        f.load(&mut f.try_store(true).unwrap(), Default::default())
            .is_err()
    );
}
