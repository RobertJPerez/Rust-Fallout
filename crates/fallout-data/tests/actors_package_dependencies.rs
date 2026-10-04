use fallout_data::{
    actors::{
        package_dependencies::{Catalogue, Limits},
        packages,
    },
    condition_operands, loaded_scripts, plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(tag: &[u8; 4], value: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(value.len() as u16).to_le_bytes(), value].concat()
}
fn record(tag: &[u8; 4], id: u32, flags: u32, version: u16, value: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(value.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        value,
    ]
    .concat()
}
fn condition(bytes: usize, function: u16) -> Vec<u8> {
    let mut value = vec![0; bytes];
    value[4..8].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    value[8..10].copy_from_slice(&function.to_le_bytes());
    value[12..16].copy_from_slice(&0x200u32.to_le_bytes());
    value
}
fn script(refs: &[(&[u8; 4], u32)], compiled: bool) -> Vec<u8> {
    let mut header = [0; 20];
    header[4..8].copy_from_slice(&(refs.len() as u32).to_le_bytes());
    header[12..16].copy_from_slice(&u32::from(!refs.is_empty()).to_le_bytes());
    header[18..20].copy_from_slice(&0x8001u16.to_le_bytes());
    let mut body = field(b"SCHR", &header);
    if compiled {
        body.extend(field(b"SCDA", &[]));
    }
    if !refs.is_empty() {
        let mut declaration = [0xa5; 24];
        declaration[..4].copy_from_slice(&77u32.to_le_bytes());
        declaration[16] = 0;
        body.extend(field(b"SLSD", &declaration));
        body.extend(field(b"SCVR", b"authored_\xe9\0"));
    }
    for (tag, raw) in refs {
        body.extend(field(tag, &raw.to_le_bytes()));
    }
    body
}
fn body() -> Vec<u8> {
    let mut result = [
        field(b"PKDT", &[0; 12]),
        field(b"PSDT", &[0; 8]),
        field(b"CTDA", &condition(20, 0xfffe)),
        field(b"UNKN", &[0xa5]),
        field(b"CTDA", &condition(28, 0xfffd)),
        field(b"POBA", &[]),
        field(b"INAM", &0x200u32.to_le_bytes()),
        script(&[(b"SCRO", 0x200), (b"SCRO", 0x999), (b"SCRV", 42)], true),
        field(b"TNAM", &0x201u32.to_le_bytes()),
        field(b"POCA", &[]),
        field(b"INAM", &0x201u32.to_le_bytes()),
        script(&[], false),
        field(b"TNAM", &0u32.to_le_bytes()),
    ]
    .concat();
    // Preserve a third physical CTDA site after script metadata without grouping.
    result.extend(field(b"CTDA", &condition(24, 0xfffc)));
    result
}
fn write(path: &Path, value: &[u8]) {
    let header = record(
        b"TES4",
        0,
        0,
        15,
        &field(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header,
            record(b"PACK", 0x100, 0, 15, value),
            record(b"IDLE", 0x200, 0, 15, &[]),
            record(b"DIAL", 0x201, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn source(path: &Path) -> RecordStore {
    RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
        .unwrap()
}
fn with_inputs<T>(
    path: &Path,
    action: impl FnOnce(&mut RecordStore, &packages::Catalogue, &loaded_scripts::Catalogue) -> T,
) -> T {
    let mut store = source(path);
    let packages = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    action(&mut store, &packages, &scripts)
}

#[test]
fn complete_source_join_borrows_script_identity_and_preserves_physical_sites_and_roles() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), &body());
    with_inputs(directory.path(), |store, packages, scripts| {
        let catalogue = Catalogue::load(
            store,
            packages,
            scripts,
            &Default::default(),
            Default::default(),
        )
        .unwrap();
        let (_, definition) = catalogue.iter().next().unwrap();
        assert!(std::ptr::eq(
            definition.record().unwrap(),
            packages.iter().next().unwrap().1.record().unwrap()
        ));
        let prepared = definition.conditions.as_ref().unwrap();
        assert_eq!(
            prepared
                .sites()
                .iter()
                .map(|s| s.raw_bytes().len())
                .collect::<Vec<_>>(),
            [20, 28, 24]
        );
        assert_eq!(prepared.sites()[1].preceding_field_kind(), Some("UNKN"));
        assert_eq!(prepared.sites()[2].preceding_field_kind(), Some("TNAM"));
        assert_eq!(definition.scripts.len(), 2);
        let original = scripts.record_scripts(definition.key).next().unwrap();
        assert!(std::ptr::eq(
            definition.scripts[0].handle,
            original.handle()
        ));
        assert!(std::ptr::eq(
            definition.scripts[0].references.as_ptr(),
            original.references().as_ptr()
        ));
        assert_eq!(
            definition.scripts[0].physical_marker.unwrap().kind,
            *b"POBA"
        );
        assert_eq!(
            definition.scripts[1].physical_marker.unwrap().kind,
            *b"POCA"
        );
        assert!(!definition.scripts[0].owner.schema_ownership_verified);
        assert_eq!(
            definition.scripts[0].references[1].status,
            loaded_scripts::ReferenceStatus::MissingForm
        );
        assert_eq!(
            definition.scripts[0].references[2].status,
            loaded_scripts::ReferenceStatus::MissingVariableDeclaration
        );
        assert_eq!(definition.scripts[0].version.compiled_bytes, Some(0));
        assert_eq!(definition.scripts[1].version.compiled_bytes, None);
        assert_eq!(definition.findings.len(), 1);
        assert_eq!(
            definition.findings[0].code,
            "package_event_link_schema_kind_mismatch"
        );
        assert_eq!(catalogue.counts().conditions, 3);
        assert_eq!(catalogue.counts().scripts, 2);
        assert_eq!(catalogue.counts().references, 3);
        assert_eq!(
            catalogue.counts().field_visits,
            catalogue.counts().fields * 2
        );
    });
}

#[test]
fn every_aggregate_budget_accepts_exact_source_extent_and_rejects_one_less() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), &body());
    with_inputs(directory.path(), |store, packages, scripts| {
        let defaults = Limits::default();
        let loaded =
            Catalogue::load(store, packages, scripts, &Default::default(), defaults).unwrap();
        let counts = loaded.counts();
        let exact = Limits {
            max_records: counts.records,
            max_record_bytes: counts.decoded_bytes,
            max_decoded_bytes: counts.decoded_bytes,
            max_fields: counts.fields,
            max_field_visits: counts.field_visits,
            max_conditions: counts.conditions,
            max_condition_bytes: counts.condition_retained_bytes,
            max_event_fields: counts.event_fields,
            max_event_links: counts.event_links,
            max_scripts: counts.scripts,
            max_declarations: counts.declarations,
            max_references: counts.references,
            max_projection_bytes: counts.projection_bytes,
        };
        Catalogue::load(store, packages, scripts, &Default::default(), exact).unwrap();
        let mut cases = Vec::new();
        macro_rules! one_less {
            ($field:ident) => {{
                if exact.$field > 0 {
                    let mut limit = exact;
                    limit.$field -= 1;
                    cases.push(limit);
                }
            }};
        }
        one_less!(max_records);
        one_less!(max_record_bytes);
        one_less!(max_decoded_bytes);
        one_less!(max_fields);
        one_less!(max_field_visits);
        one_less!(max_conditions);
        one_less!(max_condition_bytes);
        one_less!(max_event_fields);
        one_less!(max_event_links);
        one_less!(max_scripts);
        one_less!(max_declarations);
        one_less!(max_references);
        one_less!(max_projection_bytes);
        for limits in cases {
            assert!(
                Catalogue::load(
                    store,
                    packages,
                    scripts,
                    &condition_operands::Signatures::new(),
                    limits
                )
                .is_err()
            );
        }
    });
}

#[test]
fn malformed_marker_link_and_late_condition_fail_for_the_specific_source_reason() {
    for (extra, expected) in [
        (field(b"POEA", &[0]), "event marker must contain zero bytes"),
        (
            field(b"INAM", &[0; 3]),
            "event link must contain four bytes",
        ),
        (field(b"CTDA", &[0; 21]), "CTDA"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), &[body(), extra].concat());
        with_inputs(directory.path(), |store, packages, scripts| {
            let error = Catalogue::load(
                store,
                packages,
                scripts,
                &Default::default(),
                Default::default(),
            )
            .err()
            .unwrap()
            .to_string();
            assert!(error.contains(expected), "{error}");
        });
    }
}

#[test]
fn equal_counts_with_changed_source_bytes_cannot_join_an_old_script_catalogue() {
    let original = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    write(original.path(), &body());
    write(changed.path(), &[body(), field(b"DIFF", &[1])].concat());
    with_inputs(original.path(), |_, _, scripts| {
        let mut store = source(changed.path());
        let packages = packages::Catalogue::load(&mut store, Default::default()).unwrap();
        let result = Catalogue::load(
            &mut store,
            &packages,
            scripts,
            &Default::default(),
            Default::default(),
        );
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("source cohort differs")
        );
    });
}

#[test]
fn deleted_winner_is_retained_without_decoding_its_invalid_compressed_body() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), &body());
    let path = directory.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"PACK", 0x101, 0x40020, 15, &[0xff]));
    fs::write(&path, bytes).unwrap();
    with_inputs(directory.path(), |store, packages, scripts| {
        let catalogue = Catalogue::load(
            store,
            packages,
            scripts,
            &Default::default(),
            Default::default(),
        )
        .unwrap();
        let (_, tombstone) = catalogue
            .iter()
            .find(|(key, _)| key.local_id == 0x101)
            .unwrap();
        assert!(tombstone.deleted);
        assert!(tombstone.record().is_none());
        assert!(tombstone.conditions.is_none());
        assert!(tombstone.scripts.is_empty());
        assert!(tombstone.event_fields.is_empty());
        assert_eq!(catalogue.counts().records, 2);
        assert_eq!(catalogue.counts().deleted_records, 1);
        assert_eq!(catalogue.counts().decoded_bytes, body().len());
        let limits = Limits {
            max_records: 1,
            ..Default::default()
        };
        assert!(Catalogue::load(store, packages, scripts, &Default::default(), limits).is_err());
    });
}

#[test]
fn aggregate_condition_work_is_charged_across_records_and_warm_load_is_identical() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), &body());
    let path = directory.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"PACK", 0x101, 0, 15, &body()));
    fs::write(&path, bytes).unwrap();
    with_inputs(directory.path(), |store, packages, scripts| {
        let cold = Catalogue::load(
            store,
            packages,
            scripts,
            &Default::default(),
            Default::default(),
        )
        .unwrap();
        let counts = serde_json::to_value(cold.counts()).unwrap();
        let definitions = cold
            .iter()
            .map(|(_, d)| serde_json::to_value(d).unwrap())
            .collect::<Vec<_>>();
        let warm = Catalogue::load(
            store,
            packages,
            scripts,
            &Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(counts, serde_json::to_value(warm.counts()).unwrap());
        assert_eq!(
            definitions,
            warm.iter()
                .map(|(_, d)| serde_json::to_value(d).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(warm.counts().conditions, 6);
        assert_eq!(warm.counts().scripts, 4);
        let limits = Limits {
            max_conditions: 3,
            ..Default::default()
        };
        let error = Catalogue::load(store, packages, scripts, &Default::default(), limits)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("condition row budget"), "{error}");
        let limits = Limits {
            max_scripts: 2,
            ..Default::default()
        };
        let error = Catalogue::load(store, packages, scripts, &Default::default(), limits)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("package dependency script budget"),
            "{error}"
        );
    });
}

#[test]
fn caller_signature_binds_source_dependencies_without_resolving_live_values() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), &body());
    with_inputs(directory.path(), |store, packages, scripts| {
        let signatures = [(
            0xfffe,
            condition_operands::Signature {
                parameters: vec![condition_operands::Parameter {
                    type_id: 6,
                    optional_word: 0,
                }],
            },
        )]
        .into_iter()
        .collect();
        let catalogue =
            Catalogue::load(store, packages, scripts, &signatures, Default::default()).unwrap();
        let (_, definition) = catalogue.iter().next().unwrap();
        let binding = definition.conditions.as_ref().unwrap().sites()[0].binding();
        let dependency = binding.operands[0].form_dependency.as_ref().unwrap();
        assert_eq!(dependency.key.as_ref().unwrap().local_id, 0x200);
        assert_eq!(dependency.target.as_ref().unwrap().record_kind, "IDLE");
        assert_eq!(
            binding.signature_status,
            condition_operands::SignatureStatus::DescriptorBound
        );
        assert!(!binding.live_values_resolved);
        assert!(!binding.evaluation_ready);
    });
}
