use fallout_data::{
    actors::{
        self,
        associations::{self, Catalogue, Role},
    },
    inventory::{self, Status},
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn link(kind: &[u8; 4], raw: u32) -> Vec<u8> {
    field(kind, &raw.to_le_bytes())
}
fn record(kind: &[u8; 4], raw: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn scalars() -> Vec<u8> {
    [field(b"ACBS", &[0; 24]), field(b"DATA", &[0; 11])].concat()
}
fn required() -> Vec<u8> {
    [
        link(b"VTCK", 0x201),
        link(b"RNAM", 0x202),
        link(b"CNAM", 0x203),
    ]
    .concat()
}
fn targets() -> Vec<u8> {
    [
        (b"VTYP", 0x201),
        (b"RACE", 0x202),
        (b"CLAS", 0x203),
        (b"FACT", 0x204),
        (b"LVLI", 0x205),
        (b"SPEL", 0x206),
        (b"ENCH", 0x207),
        (b"PACK", 0x208),
        (b"PACK", 0x209),
        (b"WEAP", 0x210),
    ]
    .into_iter()
    .flat_map(|(kind, id)| record(kind, id, 0, &[]))
    .collect()
}
fn source(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|n| (*n).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn actor_input(store: &mut RecordStore) -> inventory::Catalogue {
    inventory::Catalogue::load(store, inventory::Limits::default()).unwrap()
}
fn single(path: &Path, body: Vec<u8>) {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), targets(), record(b"NPC_", 0x100, 0, &body)].concat(),
    )
    .unwrap();
}

#[test]
fn authored_occurrences_keep_order_signed_rank_unused_bytes_and_domains() {
    let directory = tempfile::tempdir().unwrap();
    let faction1 = field(b"SNAM", &[0x04, 0x02, 0, 0, 0x80, 0xab, 0xcd, 0xef]);
    let faction2 = field(b"SNAM", &[0x04, 0x02, 0, 0, 0x7f, 1, 2, 3]);
    single(
        directory.path(),
        [
            scalars(),
            required(),
            faction1,
            field(b"UNKN", &[9]),
            faction2,
            link(b"INAM", 0x205),
            link(b"SPLO", 0x206),
            link(b"EITM", 0x207),
            link(b"PKID", 0x209),
            link(b"PKID", 0x208),
        ]
        .concat(),
    );
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let input = actor_input(&mut store);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, &actors, associations::Limits::default()).unwrap();
    assert_eq!(catalogue.counts().bindings, 10);
    assert_eq!(catalogue.counts().source_findings, 0);
    let (key, definition) = catalogue.iter().next().unwrap();
    assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
    assert!(std::ptr::eq(catalogue.sources(), actors.sources()));
    assert_eq!(
        catalogue.winning_content_sha256(),
        actors.winning_content_sha256()
    );
    assert_eq!(
        definition
            .associations
            .iter()
            .map(|a| a.field_index)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5, 7, 8, 9, 10, 11, 12]
    );
    assert_eq!(
        definition
            .associations
            .iter()
            .map(|a| a.role)
            .collect::<Vec<_>>(),
        vec![
            Role::Voice,
            Role::Race,
            Role::Class,
            Role::Faction,
            Role::Faction,
            Role::DeathItem,
            Role::ActorEffect,
            Role::UnarmedEffect,
            Role::Package,
            Role::Package
        ]
    );
    assert_eq!(definition.associations[3].faction_rank, Some(-128));
    assert_eq!(
        definition.associations[3].faction_unused,
        Some([0xab, 0xcd, 0xef])
    );
    assert_eq!(definition.associations[4].faction_rank, Some(127));
    assert_eq!(definition.associations[4].faction_unused, Some([1, 2, 3]));
    assert_eq!(definition.associations[8].binding.raw_form, 0x209);
    assert_eq!(definition.associations[9].binding.raw_form, 0x208);
    assert!(
        definition
            .associations
            .iter()
            .all(|a| a.binding.status == Status::Defined && a.schema_kind_allowed == Some(true))
    );
}

#[test]
fn null_missing_deleted_wrong_kind_and_duplicate_singletons_stay_explicit() {
    let directory = tempfile::tempdir().unwrap();
    single(
        directory.path(),
        [
            scalars(),
            required(),
            link(b"INAM", 0),
            link(b"INAM", 0x999),
            link(b"RNAM", 0x210),
            link(b"VTCK", 0x211),
        ]
        .concat(),
    );
    let mut bytes = fs::read(directory.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"VTYP", 0x211, plugin::DELETED, b"unparsed"));
    fs::write(directory.path().join("FalloutNV.esm"), bytes).unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let input = actor_input(&mut store);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, &actors, associations::Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(definition.associations[3].binding.status, Status::Null);
    assert_eq!(definition.associations[3].schema_kind_allowed, None);
    assert_eq!(definition.associations[4].binding.status, Status::Missing);
    assert_eq!(definition.associations[5].binding.status, Status::Defined);
    assert_eq!(definition.associations[5].schema_kind_allowed, Some(false));
    assert_eq!(definition.associations[6].binding.status, Status::Deleted);
    assert_eq!(
        definition.associations[6]
            .binding
            .target
            .as_ref()
            .unwrap()
            .record_flags,
        plugin::DELETED
    );
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|f| f.code)
            .collect::<Vec<_>>(),
        vec![
            "multiple_singleton_associations",
            "association_target_missing",
            "multiple_singleton_associations",
            "association_target_wrong_kind",
            "multiple_singleton_associations",
            "association_target_deleted"
        ]
    );
}

#[test]
fn missing_required_npc_links_and_creature_field_overloads_do_not_create_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let creature = [
        field(b"ACBS", &[0; 24]),
        field(b"DATA", &[0; 17]),
        field(b"RNAM", &[255]),
        link(b"CNAM", 0x210),
    ]
    .concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            targets(),
            record(b"NPC_", 0x100, 0, &scalars()),
            record(b"CREA", 0x101, 0, &creature),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let input = actor_input(&mut store);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, &actors, associations::Limits::default()).unwrap();
    let rows = catalogue.iter().map(|(_, d)| d).collect::<Vec<_>>();
    assert_eq!(
        rows[0].findings.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec![
            "missing_voice_association",
            "missing_race_association",
            "missing_class_association"
        ]
    );
    assert!(rows[0].associations.is_empty());
    assert!(rows[1].associations.is_empty());
    assert!(rows[1].findings.is_empty());
}

#[test]
fn association_shapes_and_all_catalogue_budgets_are_enforced() {
    for (kind, size) in [(b"SNAM", 7), (b"SNAM", 9), (b"PKID", 3), (b"PKID", 5)] {
        let directory = tempfile::tempdir().unwrap();
        single(
            directory.path(),
            [scalars(), required(), field(kind, &vec![0; size])].concat(),
        );
        let mut store = source(directory.path(), &["FalloutNV.esm"]);
        let input = actor_input(&mut store);
        let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
        assert!(Catalogue::load(&mut store, &actors, associations::Limits::default()).is_err());
    }
    let directory = tempfile::tempdir().unwrap();
    single(directory.path(), [scalars(), required()].concat());
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let input = actor_input(&mut store);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    for limits in [
        associations::Limits {
            max_records: 0,
            ..Default::default()
        },
        associations::Limits {
            max_fields: 0,
            ..Default::default()
        },
        associations::Limits {
            max_bindings: 0,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut store, &actors, limits).is_err());
    }
}

#[test]
fn source_cohort_checks_reject_equal_counts_with_changed_payload_and_equal_sources_with_changed_winners()
 {
    let directory = tempfile::tempdir().unwrap();
    single(directory.path(), [scalars(), required()].concat());
    for name in ["A.esm", "B.esm"] {
        fs::write(
            directory.path().join(name),
            [
                header(&["FalloutNV.esm"]),
                record(
                    b"NPC_",
                    0x100,
                    0,
                    &[
                        scalars(),
                        required(),
                        link(b"PKID", if name == "A.esm" { 0x208 } else { 0x209 }),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        )
        .unwrap();
    }
    let mut first = source(directory.path(), &["FalloutNV.esm", "A.esm", "B.esm"]);
    let input = actor_input(&mut first);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let mut reversed = source(directory.path(), &["FalloutNV.esm", "B.esm", "A.esm"]);
    assert!(Catalogue::load(&mut reversed, &actors, associations::Limits::default()).is_err());
    drop(first);
    drop(reversed);
    let path = directory.path().join("B.esm");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 4;
    bytes[last..].copy_from_slice(&0x208u32.to_le_bytes());
    fs::write(path, bytes).unwrap();
    let mut changed = source(directory.path(), &["FalloutNV.esm", "A.esm", "B.esm"]);
    let changed_input = actor_input(&mut changed);
    assert_eq!(input.counts.records, changed_input.counts.records);
    assert_eq!(
        input.winning_content_sha256(),
        changed_input.winning_content_sha256()
    );
    assert!(Catalogue::load(&mut changed, &actors, associations::Limits::default()).is_err());
}

#[test]
fn reordered_store_resolves_owning_plugin_by_form_key_without_source_index_aliasing() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), targets()].concat(),
    )
    .unwrap();
    for name in ["A.esm", "B.esm"] {
        fs::write(
            directory.path().join(name),
            [
                header(&["FalloutNV.esm"]),
                record(b"PACK", 0x0100_0400, 0, &[]),
                record(
                    b"NPC_",
                    0x0100_0100,
                    0,
                    &[scalars(), required(), link(b"PKID", 0x0100_0400)].concat(),
                ),
            ]
            .concat(),
        )
        .unwrap();
    }
    let mut original = source(directory.path(), &["FalloutNV.esm", "A.esm", "B.esm"]);
    let input = actor_input(&mut original);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let expected =
        Catalogue::load(&mut original, &actors, associations::Limits::default()).unwrap();
    drop(original);
    let mut reversed = source(directory.path(), &["FalloutNV.esm", "B.esm", "A.esm"]);
    let actual = Catalogue::load(&mut reversed, &actors, associations::Limits::default()).unwrap();
    assert_eq!(
        serde_json::to_value(expected.iter().map(|(_, d)| d).collect::<Vec<_>>()).unwrap(),
        serde_json::to_value(actual.iter().map(|(_, d)| d).collect::<Vec<_>>()).unwrap()
    );
    for (key, definition) in actual.iter() {
        assert_eq!(
            definition.associations[3]
                .binding
                .key
                .as_ref()
                .unwrap()
                .origin_plugin,
            key.origin_plugin
        );
    }
}

#[test]
fn deleted_actor_winner_has_no_associations_or_fallback() {
    let directory = tempfile::tempdir().unwrap();
    single(directory.path(), [scalars(), required()].concat());
    fs::write(
        directory.path().join("Addon.esm"),
        [
            header(&["FalloutNV.esm"]),
            record(b"NPC_", 0x100, plugin::DELETED, b"unparsed tombstone"),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm", "Addon.esm"]);
    let input = actor_input(&mut store);
    let actors = actors::Catalogue::load(&input, actors::Limits::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, &actors, associations::Limits::default()).unwrap();
    assert_eq!(catalogue.counts().records, 1);
    assert_eq!(catalogue.counts().bindings, 0);
    let definition = catalogue.iter().next().unwrap().1;
    assert!(definition.associations.is_empty());
    assert!(definition.findings.is_empty());
}
