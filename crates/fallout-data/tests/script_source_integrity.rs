use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits},
    plugin,
    store::RecordStore,
};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, bytes: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut bytes = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        bytes.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        bytes.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &bytes)
}
fn script(kind: &[u8; 4]) -> Vec<u8> {
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
    let unit = [field(b"SCHR", &schr), field(b"SCDA", &[0x1d, 0, 0, 0])].concat();
    match kind {
        b"QUST" => [
            field(b"INDX", &7_i16.to_le_bytes()),
            field(b"QSDT", &[0]),
            unit,
        ]
        .concat(),
        b"INFO" => [unit.clone(), field(b"NEXT", &[]), unit].concat(),
        _ => unit,
    }
}
fn compressed(bytes: &[u8], corrupt: bool) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    encoder.write_all(bytes).unwrap();
    let mut zlib = encoder.finish().unwrap();
    if corrupt {
        *zlib.last_mut().unwrap() ^= 1;
    }
    [&(bytes.len() as u32).to_le_bytes()[..], &zlib].concat()
}
fn open(path: &Path, names: &[String], forensic: bool) -> RecordStore {
    RecordStore::open_nv(
        path,
        names,
        plugin::Limits {
            inspect_checksum_mismatches: forensic,
            ..Default::default()
        },
    )
    .unwrap()
}
fn retain_case(name: &str, path: &Path, observation: serde_json::Value) {
    let Some(root) = std::env::var_os("FALLOUT_SCRIPT_INTEGRITY_EVIDENCE_DIR") else {
        return;
    };
    let case = Path::new(&root).join(name);
    fs::create_dir(&case).unwrap();
    for entry in fs::read_dir(path).unwrap() {
        let source = entry.unwrap().path();
        fs::copy(&source, case.join(source.file_name().unwrap())).unwrap();
    }
    fs::write(
        case.join("observation.json"),
        serde_json::to_vec_pretty(&observation).unwrap(),
    )
    .unwrap();
}
fn reject_recovered(kind: &[u8; 4], name: &str, bytes: &[u8]) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(kind, 0x100, plugin::COMPRESSED, &compressed(bytes, true)),
        ]
        .concat(),
    )
    .unwrap();
    let mut source = open(directory.path(), &["FalloutNV.esm".into()], true);
    let location = source
        .winner(&FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x100,
        })
        .unwrap();
    let recovered = source.read(location).unwrap();
    assert_eq!(recovered.payload, bytes);
    assert!(recovered.integrity_issue.is_some());
    let mut observed = 0;
    let result = Catalogue::load(&mut source, Limits::default(), |_, _| {
        observed += 1;
        Ok(())
    });
    let observation = match &result {
        Ok(catalogue) => {
            serde_json::json!({"catalogue_admitted":true,"scripts":catalogue.counts.scripts})
        }
        Err(error) => serde_json::json!({"catalogue_admitted":false,"error":error.to_string()}),
    };
    retain_case(
        name,
        directory.path(),
        serde_json::json!({"record_kind":plugin::signature(*kind),"recovered_payload_sha256":format!("{:x}",Sha256::digest(bytes)),
            "integrity_issue":recovered.integrity_issue,"observer_calls":observed,"outcome":observation}),
    );
    match result {
        Err(error) => assert!(error.to_string().contains("untrusted checksum recovery")),
        Ok(catalogue) => panic!(
            "Recovered {} source admitted {} loaded scripts and reached {} observers",
            plugin::signature(*kind),
            catalogue.counts.scripts,
            observed
        ),
    }
    assert_eq!(observed, 0);
}

#[test]
fn recovered_standalone_script_cannot_become_a_loaded_definition() {
    reject_recovered(b"SCPT", "standalone", &script(b"SCPT"));
}
#[test]
fn recovered_quest_log_script_cannot_become_a_loaded_definition() {
    reject_recovered(b"QUST", "quest-log", &script(b"QUST"));
}
#[test]
fn recovered_dialogue_fragments_cannot_become_loaded_definitions() {
    reject_recovered(b"INFO", "dialogue-fragments", &script(b"INFO"));
}
#[test]
fn recovered_script_candidate_without_units_is_rejected_before_skipping_it() {
    reject_recovered(b"QUST", "empty-candidate", &field(b"EDID", b"NoUnit\0"));
}

#[test]
fn valid_compressed_scripts_match_strict_and_forensic_catalogue_projections() {
    let directory = tempfile::tempdir().unwrap();
    let records = [b"SCPT", b"QUST", b"INFO"]
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            record(
                kind,
                0x100 + index as u32,
                plugin::COMPRESSED,
                &compressed(&script(kind), false),
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), records.concat()].concat(),
    )
    .unwrap();
    let names = ["FalloutNV.esm".into()];
    let strict = Catalogue::load(
        &mut open(directory.path(), &names, false),
        Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    let forensic = Catalogue::load(
        &mut open(directory.path(), &names, true),
        Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    let projection = |catalogue: &Catalogue| {
        serde_json::to_value(
            catalogue
                .iter()
                .map(|(_, script)| {
                    (
                        script.handle(),
                        script.version(),
                        script.owner(),
                        script.declarations(),
                        script.references(),
                        script.issues(),
                    )
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    assert_eq!(strict.counts.scripts, 4);
    assert_eq!(projection(&strict), projection(&forensic));
    assert_eq!(
        serde_json::to_value(&strict.counts).unwrap(),
        serde_json::to_value(&forensic.counts).unwrap()
    );
    retain_case(
        "valid-compressed",
        directory.path(),
        serde_json::json!({"strict_forensic_equal":true,"scripts":projection(&strict)}),
    );
}

#[test]
fn recovered_nonwinning_and_deleted_sources_do_not_resurrect_scripts() {
    for deleted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let bytes = script(b"SCPT");
        fs::write(
            directory.path().join("FalloutNV.esm"),
            [
                header(&[]),
                record(
                    b"SCPT",
                    0x100,
                    plugin::COMPRESSED,
                    &compressed(&bytes, true),
                ),
            ]
            .concat(),
        )
        .unwrap();
        fs::write(
            directory.path().join("Patch.esp"),
            [
                header(&["FalloutNV.esm"]),
                record(
                    b"SCPT",
                    0x100,
                    if deleted { plugin::DELETED } else { 0 },
                    if deleted { &[] } else { &bytes },
                ),
            ]
            .concat(),
        )
        .unwrap();
        let mut source = open(
            directory.path(),
            &["FalloutNV.esm".into(), "Patch.esp".into()],
            true,
        );
        assert_eq!(source.integrity_failures(), 1);
        let catalogue = Catalogue::load(&mut source, Limits::default(), |_, _| Ok(())).unwrap();
        assert_eq!(catalogue.counts.scripts, u64::from(!deleted));
        assert_eq!(
            catalogue.counts.deleted_candidates_skipped,
            u64::from(deleted)
        );
        if !deleted {
            assert_eq!(
                catalogue.iter().next().unwrap().1.version().source_plugin,
                "Patch.esp"
            );
        }
        retain_case(
            if deleted {
                "deleted-winner"
            } else {
                "valid-winner"
            },
            directory.path(),
            serde_json::json!({"scripts":catalogue.counts.scripts,"deleted_winner":deleted}),
        );
    }
}
