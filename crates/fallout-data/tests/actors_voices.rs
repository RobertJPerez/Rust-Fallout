use fallout_data::{
    actors::{
        self, associations,
        dependencies::Sex,
        races,
        voices::{self, Limits},
    },
    identity::{FormKey, ProfileId},
    inventory, plugin,
    store::RecordStore,
};
use sha2::Digest;
use std::{fs, io::Write, path::Path};

fn field(tag: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(tag: &[u8; 4], id: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 && flags & plugin::DELETED == 0 && version != 99 {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(body).unwrap();
        [
            &(body.len() as u32).to_le_bytes()[..],
            &encoder.finish().unwrap(),
        ]
        .concat()
    } else {
        body.to_vec()
    };
    [
        tag.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        &body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &body)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn configuration(flags: u32, template: u16) -> Vec<u8> {
    let mut bytes = [0; 24];
    bytes[..4].copy_from_slice(&flags.to_le_bytes());
    bytes[22..].copy_from_slice(&template.to_le_bytes());
    field(b"ACBS", &bytes)
}
fn actor(kind: &[u8; 4], flags: u32, template: u16, voice: u32) -> Vec<u8> {
    let mut bytes = [
        field(b"EDID", b"Chosen\0"),
        configuration(flags, template),
        field(b"DATA", &vec![0; if kind == b"NPC_" { 11 } else { 17 }]),
        field(b"VTCK", &voice.to_le_bytes()),
    ]
    .concat();
    if kind == b"NPC_" {
        bytes.extend(field(b"RNAM", &0x300_u32.to_le_bytes()));
    }
    bytes
}
fn race(male: u32, female: u32) -> Vec<u8> {
    [
        field(b"EDID", b"Race\0"),
        field(b"DATA", &[0; 36]),
        field(b"PNAM", &[0; 4]),
        field(b"UNAM", &[0; 4]),
        field(
            b"VTCK",
            &[male.to_le_bytes(), female.to_le_bytes()].concat(),
        ),
    ]
    .concat()
}
fn voice_records() -> Vec<Vec<u8>> {
    vec![
        disk(b"VTYP", 0x200, 0, 1, &field(b"EDID", b"LegacyNoFlags\0")),
        disk(
            b"VTYP",
            0x201,
            0,
            14,
            &[field(b"EDID", b"Male\0"), field(b"DNAM", &[0x80])].concat(),
        ),
        disk(
            b"VTYP",
            0x202,
            plugin::COMPRESSED,
            15,
            &[field(b"EDID", b"Female\0"), field(b"DNAM", &[255])].concat(),
        ),
        disk(
            b"VTYP",
            0x203,
            plugin::COMPRESSED | plugin::DELETED,
            99,
            b"unread tombstone",
        ),
        disk(
            b"VTYP",
            0x204,
            plugin::COMPRESSED,
            99,
            b"unread unknown version",
        ),
        disk(b"MISC", 0x400, 0, 15, &[]),
    ]
}
fn fixture(path: &Path, kind: &[u8; 4], actor: &[u8], race: &[u8], voices: &[Vec<u8>]) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let mut bytes = [
        header(&[]),
        disk(kind, 0x100, 0, 15, actor),
        disk(b"RACE", 0x300, 0, 15, race),
    ]
    .concat();
    for voice in voices {
        bytes.extend(voice);
    }
    fs::write(path.join("Data/FalloutNV.esm"), bytes).unwrap();
}
fn with_sources(
    path: &Path,
    order: &[&str],
    callback: impl FnOnce(
        &mut RecordStore,
        &actors::Catalogue<'_>,
        &associations::Catalogue<'_>,
        &races::Catalogue,
    ),
) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        Default::default(),
    )
    .unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let races = races::Catalogue::load(&mut store, Default::default()).unwrap();
    callback(&mut store, &actors, &associations, &races);
}
fn export(name: &str, path: &Path, order: &[&str]) {
    if let Some(root) = std::env::var_os("FALLOUT_ACTOR_VOICE_EVIDENCE_DIR") {
        let target = Path::new(&root).join(name);
        fs::create_dir_all(target.join("Data")).unwrap();
        for name in order {
            fs::copy(path.join("Data").join(name), target.join("Data").join(name)).unwrap();
        }
        fs::write(
            target.join("order.json"),
            serde_json::to_vec(order).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn source_links_keep_sex_alignment_optional_flags_unknown_bits_and_physical_origins() {
    let dir = tempfile::tempdir().unwrap();
    let actor_bytes = actor(b"NPC_", 1, 0, 0x200);
    let race_bytes = race(0x201, 0x202);
    fixture(
        dir.path(),
        b"NPC_",
        &actor_bytes,
        &race_bytes,
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert_eq!(result.authored_sex, Some(Sex::Female));
            assert_eq!(
                result.configuration.as_ref().unwrap().field_decoded_offset,
                13
            );
            assert_eq!(result.actor_voices[0].field.decoded_offset, 60);
            assert_eq!(result.actor_voices[0].association.binding.raw_form, 0x200);
            assert_eq!(result.actor_voices[0].target_source_index, Some(0));
            assert_eq!(
                result.race_requests[0]
                    .voices
                    .iter()
                    .map(|voice| (
                        voice.sex,
                        voice.matches_authored_actor_sex,
                        voice.target_source_index
                    ))
                    .collect::<Vec<_>>(),
                vec![
                    (Sex::Male, Some(false), Some(1)),
                    (Sex::Female, Some(true), Some(2))
                ]
            );
            assert_eq!(result.race_requests[0].voices[0].field.decoded_offset, 73);
            assert_eq!(result.voice_sources[0].header.version, 1);
            assert!(
                result.voice_sources[0]
                    .fields
                    .iter()
                    .all(|field| field.flags.is_none())
            );
            assert_eq!(result.voice_sources[1].fields[1].flags, Some(0x80));
            assert_eq!(result.voice_sources[2].fields[1].flags, Some(255));
            assert!(
                result
                    .voice_sources
                    .iter()
                    .all(|source| source.source_body_read)
            );
            assert!(result.issues.is_empty());
            assert_eq!(result.counts.declarations, 4);
            assert_eq!(result.counts.fields, 15);
            assert!(
                !result.template_inheritance_supported
                    && !result.effective_voice_selection_supported
                    && !result.dialogue_truth_supported
                    && !result.audio_path_selection_supported
                    && !result.original_behavior_verified
            );
            assert_eq!(
                actors
                    .get(&key(0x100))
                    .unwrap()
                    .source
                    .decoded_record_sha256,
                Some(format!("{:x}", sha2::Sha256::digest(&actor_bytes)))
            );
        },
    );
    export("base", dir.path(), &["FalloutNV.esm"]);
}

#[test]
fn winning_actor_race_and_voice_overrides_replace_source_inputs_without_fallback() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 1, 0, 0x200),
        &race(0x201, 0x202),
        &voice_records(),
    );
    fs::write(
        dir.path().join("Data/A.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(
                b"NPC_",
                0x100,
                plugin::COMPRESSED,
                14,
                &actor(b"NPC_", 0, 0, 0x202),
            ),
            disk(b"RACE", 0x300, plugin::COMPRESSED, 15, &race(0x202, 0x202)),
            disk(
                b"VTYP",
                0x202,
                plugin::COMPRESSED,
                4,
                &[
                    field(b"EDID", b"Winner\0"),
                    field(b"DNAM", &[2]),
                    field(b"DNAM", &[129]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    with_sources(
        dir.path(),
        &["FalloutNV.esm", "A.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert_eq!(result.actor.source.plugin, "A.esm");
            assert_eq!(result.authored_sex, Some(Sex::Male));
            assert_eq!(
                result.race_requests[0].definition.unwrap().source.plugin,
                "A.esm"
            );
            assert_eq!(result.voice_sources.len(), 1);
            assert_eq!(result.voice_sources[0].source.plugin, "A.esm");
            assert_eq!(result.voice_sources[0].header.version, 4);
            assert_eq!(
                result.voice_sources[0]
                    .fields
                    .iter()
                    .filter_map(|field| field.flags)
                    .collect::<Vec<_>>(),
                vec![2, 129]
            );
            assert_eq!(result.issues[0].code, "ambiguous_voice_type_flags");
            assert!(
                result.race_requests[0]
                    .voices
                    .iter()
                    .all(|voice| voice.target_source_index == Some(0))
            );
        },
    );
    export("override", dir.path(), &["FalloutNV.esm", "A.esm"]);
}

#[test]
fn ambiguous_singletons_retain_every_occurrence_without_following_any_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let ambiguous_actor = [
        actor(b"NPC_", 0, 0, 0x204),
        field(b"VTCK", &0x203_u32.to_le_bytes()),
        field(b"RNAM", &0x300_u32.to_le_bytes()),
    ]
    .concat();
    fixture(
        dir.path(),
        b"NPC_",
        &ambiguous_actor,
        &race(0x204, 0x203),
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert_eq!(result.actor_voices.len(), 2);
            assert_eq!(result.race_requests.len(), 2);
            assert!(result.voice_sources.is_empty());
            assert!(
                result
                    .actor_voices
                    .iter()
                    .all(|voice| voice.ambiguous_source && voice.target_source_index.is_none())
            );
            assert!(
                result
                    .race_requests
                    .iter()
                    .all(|race| race.ambiguous_source && race.definition.is_none())
            );
            assert_eq!(result.issues.len(), 4);
        },
    );
    export("ambiguous-actor", dir.path(), &["FalloutNV.esm"]);
    let ambiguous_race = [
        race(0x204, 0x203),
        field(
            b"VTCK",
            &[0x201_u32.to_le_bytes(), 0x202_u32.to_le_bytes()].concat(),
        ),
    ]
    .concat();
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 0, 0, 0x200),
        &ambiguous_race,
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert_eq!(result.voice_sources.len(), 1);
            assert_eq!(result.race_requests[0].voices.len(), 4);
            assert!(
                result.race_requests[0]
                    .voices
                    .iter()
                    .all(|voice| voice.ambiguous_source && voice.target_source_index.is_none())
            );
            assert_eq!(result.issues.len(), 2);
        },
    );
    export("ambiguous-race", dir.path(), &["FalloutNV.esm"]);
}

#[test]
fn unavailable_bindings_and_unknown_versions_preserve_headers_without_decoding_bodies() {
    let dir = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 0, 0, 0x204),
        &race(0x203, 0x400),
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert_eq!(result.voice_sources.len(), 1);
            assert_eq!(result.voice_sources[0].header.version, 99);
            assert!(!result.voice_sources[0].source_body_read);
            assert!(result.voice_sources[0].fields.is_empty());
            assert_eq!(result.voice_sources[0].source.decoded_record_sha256, None);
            assert_eq!(
                result.race_requests[0].voices[0].binding.status,
                inventory::Status::Deleted
            );
            assert_eq!(
                result.race_requests[0].voices[1]
                    .binding
                    .target
                    .as_ref()
                    .unwrap()
                    .kind,
                *b"MISC"
            );
            assert_eq!(
                result
                    .issues
                    .iter()
                    .map(|issue| issue.code)
                    .collect::<Vec<_>>(),
                vec![
                    "unsupported_voice_type_version",
                    "unavailable_voice_type",
                    "unavailable_voice_type"
                ]
            );
        },
    );
    export("unavailable", dir.path(), &["FalloutNV.esm"]);
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 0, 0, 0),
        &race(0, 0x999),
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            assert!(result.voice_sources.is_empty());
            assert_eq!(
                result.actor_voices[0].association.binding.status,
                inventory::Status::Null
            );
            assert_eq!(
                result.race_requests[0].voices[1].binding.status,
                inventory::Status::Missing
            );
            assert_eq!(result.issues.len(), 3);
            assert!(
                voices::request(store, actors, links, races, &key(0x999), Default::default())
                    .is_err()
            );
        },
    );
    export("null-missing", dir.path(), &["FalloutNV.esm"]);
}

#[test]
fn traits_inheritance_duplicate_configuration_and_creature_flags_never_invent_sex_or_paths() {
    let dir = tempfile::tempdir().unwrap();
    for (name, kind, body, issue) in [
        (
            "inherited",
            *b"NPC_",
            actor(b"NPC_", 1, 1, 0x200),
            Some("traits_template_selection_unsupported"),
        ),
        (
            "duplicate-config",
            *b"NPC_",
            [actor(b"NPC_", 1, 0, 0x200), configuration(0, 0)].concat(),
            Some("ambiguous_actor_configuration"),
        ),
        ("creature", *b"CREA", actor(b"CREA", 1, 0, 0x200), None),
    ] {
        fixture(
            dir.path(),
            &kind,
            &body,
            &race(0x201, 0x202),
            &voice_records(),
        );
        with_sources(
            dir.path(),
            &["FalloutNV.esm"],
            |store, actors, links, races| {
                let result =
                    voices::request(store, actors, links, races, &key(0x100), Default::default())
                        .unwrap();
                assert_eq!(result.authored_sex, None);
                assert!(
                    result
                        .race_requests
                        .iter()
                        .flat_map(|race| &race.voices)
                        .all(|voice| voice.matches_authored_actor_sex.is_none())
                );
                assert_eq!(result.issues.first().map(|issue| issue.code), issue);
                if kind == *b"CREA" {
                    assert!(result.race_requests.is_empty());
                }
            },
        );
        export(name, dir.path(), &["FalloutNV.esm"]);
    }
}

#[test]
fn malformed_voice_flags_or_race_pairs_refuse_instead_of_truncating() {
    let dir = tempfile::tempdir().unwrap();
    for size in [0, 2] {
        let mut records = voice_records();
        records[0] = disk(b"VTYP", 0x200, 0, 15, &field(b"DNAM", &vec![0; size]));
        fixture(
            dir.path(),
            b"NPC_",
            &actor(b"NPC_", 0, 0, 0x200),
            &race(0x201, 0x202),
            &records,
        );
        with_sources(
            dir.path(),
            &["FalloutNV.esm"],
            |store, actors, links, races| {
                assert!(matches!(
                    voices::request(store, actors, links, races, &key(0x100), Default::default()),
                    Err(fallout_data::Error::Format { .. })
                ));
            },
        );
    }
    for size in [0, 4, 12] {
        let body = field(b"VTCK", &vec![0; size]);
        fixture(
            dir.path(),
            b"NPC_",
            &actor(b"NPC_", 0, 0, 0x200),
            &body,
            &voice_records(),
        );
        with_sources(
            dir.path(),
            &["FalloutNV.esm"],
            |store, actors, links, races| {
                assert!(matches!(
                    voices::request(store, actors, links, races, &key(0x100), Default::default()),
                    Err(fallout_data::Error::Format { .. })
                ));
            },
        );
    }
}

#[test]
fn every_selected_budget_accepts_its_exact_boundary_and_refuses_one_less() {
    let dir = tempfile::tempdir().unwrap();
    // The shared indexed reader bounds both stored and decoded extents by the
    // remaining allowance. Use plain records for literal aggregate boundaries;
    // compressed overrides and optional fields are exercised above.
    let mut records = voice_records();
    records[2] = disk(
        b"VTYP",
        0x202,
        0,
        15,
        &[field(b"EDID", b"Female\0"), field(b"DNAM", &[255])].concat(),
    );
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 0, 0, 0x200),
        &race(0x201, 0x202),
        &records,
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            let result =
                voices::request(store, actors, links, races, &key(0x100), Default::default())
                    .unwrap();
            let maximum_record = result
                .actor
                .record()
                .unwrap()
                .payload
                .len()
                .max(
                    result.race_requests[0]
                        .definition
                        .unwrap()
                        .record()
                        .unwrap()
                        .payload
                        .len(),
                )
                .max(
                    result
                        .voice_sources
                        .iter()
                        .map(|source| {
                            source
                                .fields
                                .iter()
                                .map(|field| field.bytes + 6)
                                .sum::<usize>()
                        })
                        .max()
                        .unwrap(),
                );
            let exact = Limits {
                max_voice_sources: result.voice_sources.len(),
                max_record_bytes: maximum_record,
                max_decoded_bytes: result.counts.decoded_bytes,
                max_fields: result.counts.fields,
                max_declarations: result.counts.declarations,
                max_visits: result.counts.visits,
                max_issues: 0,
                max_projection_bytes: serde_json::to_vec(&result).unwrap().len(),
            };
            voices::request(store, actors, links, races, &key(0x100), exact).unwrap();
            for lower in [
                Limits {
                    max_voice_sources: exact.max_voice_sources - 1,
                    ..exact
                },
                Limits {
                    max_record_bytes: exact.max_record_bytes - 1,
                    ..exact
                },
                Limits {
                    max_decoded_bytes: exact.max_decoded_bytes - 1,
                    ..exact
                },
                Limits {
                    max_fields: exact.max_fields - 1,
                    ..exact
                },
                Limits {
                    max_declarations: exact.max_declarations - 1,
                    ..exact
                },
                Limits {
                    max_visits: exact.max_visits - 1,
                    ..exact
                },
                Limits {
                    max_projection_bytes: exact.max_projection_bytes - 1,
                    ..exact
                },
            ] {
                assert!(voices::request(store, actors, links, races, &key(0x100), lower).is_err());
            }
        },
    );
    fixture(
        dir.path(),
        b"CREA",
        &[configuration(0, 0), field(b"DATA", &[0; 17])].concat(),
        &race(0, 0),
        &voice_records(),
    );
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            assert!(
                voices::request(
                    store,
                    actors,
                    links,
                    races,
                    &key(0x100),
                    Limits {
                        max_issues: 1,
                        ..Default::default()
                    }
                )
                .is_ok()
            );
            assert!(
                voices::request(
                    store,
                    actors,
                    links,
                    races,
                    &key(0x100),
                    Limits {
                        max_issues: 0,
                        ..Default::default()
                    }
                )
                .is_err()
            );
        },
    );
}

#[test]
fn mismatched_store_and_race_cohort_are_rejected_before_any_follow() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fixture(
        dir.path(),
        b"NPC_",
        &actor(b"NPC_", 0, 0, 0x200),
        &race(0x201, 0x202),
        &voice_records(),
    );
    fixture(
        other.path(),
        b"NPC_",
        &actor(b"NPC_", 1, 0, 0x200),
        &race(0x202, 0x201),
        &voice_records(),
    );
    let mut other_store = RecordStore::open_nv_headers(
        &other.path().join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let other_races = races::Catalogue::load(&mut other_store, Default::default()).unwrap();
    with_sources(
        dir.path(),
        &["FalloutNV.esm"],
        |store, actors, links, races| {
            assert!(
                voices::request(
                    &mut other_store,
                    actors,
                    links,
                    races,
                    &key(0x100),
                    Default::default()
                )
                .is_err()
            );
            assert!(
                voices::request(
                    store,
                    actors,
                    links,
                    &other_races,
                    &key(0x100),
                    Default::default()
                )
                .is_err()
            );
        },
    );
}
