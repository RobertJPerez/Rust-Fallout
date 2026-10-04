use fallout_data::{
    actors::{
        self, associations,
        dependencies::{Catalogue, CreaturePartSelection, CreaturePartsLimits, LookupStatus},
    },
    assets::ArchiveAssets,
    identity::{FormKey, ProfileId},
    inventory, leveled, plugin,
    store::RecordStore,
    vfs::AssetPath,
};
use std::{fs, io::Write, path::Path};
fn field(kind: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 && flags & plugin::DELETED == 0 {
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
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15_u16.to_le_bytes(),
        &[0; 2],
        &body,
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
    disk(b"TES4", 0, 0, &bytes)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn body(case: &str) -> Vec<u8> {
    let mut config = [0; 24];
    config[0] = 1;
    if case == "template" {
        config[22] = 64;
    }
    let mut bytes = field(b"EDID", b"AuthoredCreature\0");
    if case != "missing-config" {
        bytes.extend(field(b"ACBS", &config));
    }
    if case == "duplicate-config" {
        bytes.extend(field(b"ACBS", &config));
    }
    bytes.extend(field(b"DATA", &[0; 17]));
    if case != "missing-model" {
        bytes.extend(field(b"MODL", b"Creatures\\Dog\\Skeleton.nif\0"));
    }
    let raw: Vec<u8> = match case {
        "empty" => b"dogskin.nif\0eyessetblue.nif\0\0".to_vec(),
        "unsafe" => b"/dogskin.nif\0..\\escape.nif\0C:\\drive.nif\0bad//part.nif\0".to_vec(),
        "long" => [vec![b'x'; 4097], vec![0]].concat(),
        _ => b"DogSkin.NIF\0EyesSetBlue.nif\0".to_vec(),
    };
    if case != "absent" {
        bytes.extend(field(b"NIFZ", &raw));
    }
    if case == "duplicate" {
        bytes.extend(field(b"NIFZ", b"dogskin.nif\0"));
    }
    bytes
}
fn fixture(path: &Path, case: &str) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let flags = if case == "compressed" {
        plugin::COMPRESSED
    } else {
        0
    };
    let npc = [field(b"ACBS", &[0; 24]), field(b"DATA", &[0; 11])].concat();
    fs::write(
        path.join("Data/FalloutNV.esm"),
        [
            header(&[]),
            disk(b"CREA", 0x100, flags, &body(case)),
            disk(b"NPC_", 0x101, 0, &npc),
            disk(b"CREA", 0x102, plugin::DELETED, b"unread tombstone"),
        ]
        .concat(),
    )
    .unwrap();
    if case == "override" {
        fs::write(
            path.join("Data/ActorPatch.esp"),
            [
                header(&["FalloutNV.esm"]),
                disk(b"CREA", 0x100, plugin::COMPRESSED, &body("empty")),
            ]
            .concat(),
        )
        .unwrap();
    }
    let mut builder = dream_archive::Tes4BsaBuilder::fallout_new_vegas();
    for name in [
        "meshes/creatures/dog/skeleton.nif",
        "meshes/creatures/dog/dogskin.nif",
        "meshes/creatures/dog/eyessetblue.nif",
        "meshes/alternate/dogskin.nif",
    ] {
        builder
            .add_bytes(name, b"metadata only; no NIF content")
            .unwrap();
    }
    builder.write_path(path.join("Data/A.bsa")).unwrap();
    if case == "collision" {
        let mut builder = dream_archive::Tes4BsaBuilder::fallout_new_vegas();
        builder
            .add_bytes(
                "meshes/creatures/dog/dogskin.nif",
                b"second physical source",
            )
            .unwrap();
        builder.write_path(path.join("Data/B.bsa")).unwrap();
    }
    let names = if case == "override" {
        vec!["FalloutNV.esm", "ActorPatch.esp"]
    } else {
        vec!["FalloutNV.esm"]
    };
    fs::write(path.join("order.json"), serde_json::to_vec(&names).unwrap()).unwrap();
    fs::write(path.join("order.txt"), names.join("\n")).unwrap();
}
fn with_sources(path: &Path, callback: impl FnOnce(&Catalogue<'_>, &ArchiveAssets)) {
    let names: Vec<String> =
        serde_json::from_slice(&fs::read(path.join("order.json")).unwrap()).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&path.join("Data"), &names, Default::default()).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let dependencies = Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let assets = ArchiveAssets::open_nv(path).unwrap();
    callback(&dependencies, &assets);
}
fn directory() -> AssetPath {
    AssetPath::new(b"Meshes\\Creatures\\Dog").unwrap()
}

#[test]
fn explicit_directory_resolves_two_frames_without_changing_default_source_requests() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid");
    with_sources(temp.path(), |catalogue, assets| {
        let source = catalogue
            .render_manifest(&key(0x100), assets, Default::default())
            .unwrap();
        let report = catalogue
            .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
            .unwrap();
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            serde_json::to_value(&report.render).unwrap()
        );
        assert_eq!(report.render.sex, None);
        assert!(!source.selected_requests_admitted);
        assert!(report.part_requests_admitted);
        assert!(!report.effective_part_selection_supported && !report.rig_playback_supported);
        assert_eq!(report.counts.aggregate_candidates, 3);
        assert_eq!(report.requests.len(), 2);
        for (index, name) in [b"dogskin.nif".as_slice(), b"eyessetblue.nif".as_slice()]
            .iter()
            .enumerate()
        {
            let request = &report.requests[index];
            let physical = &report.render.manifest.paths[request.manifest_path_index];
            assert_eq!(physical.lookup_status, LookupStatus::RelativeBaseUnresolved);
            assert_eq!(
                request.asset_path.as_ref().unwrap().bytes(),
                [b"meshes/creatures/dog/".as_slice(), name].concat()
            );
            assert_eq!(
                request.lookup_status,
                Some(LookupStatus::OneArchiveCandidate)
            );
            assert_eq!(request.candidates.len(), 1);
        }
        let alternate = catalogue
            .creature_parts_manifest(
                &key(0x100),
                &AssetPath::new(b"meshes/alternate").unwrap(),
                assets,
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            alternate.requests[0].lookup_status,
            Some(LookupStatus::OneArchiveCandidate)
        );
        assert_eq!(
            alternate.requests[1].lookup_status,
            Some(LookupStatus::MissingArchiveCandidate)
        );
        assert!(!alternate.part_requests_admitted);
        assert_eq!(
            serde_json::to_value(&alternate.render).unwrap(),
            serde_json::to_value(&source).unwrap()
        );
    });
}
#[test]
fn source_frames_keep_empty_unsafe_long_and_archive_collisions_explicit() {
    for (case, statuses) in [
        (
            "empty",
            vec![
                LookupStatus::OneArchiveCandidate,
                LookupStatus::OneArchiveCandidate,
                LookupStatus::EmptySourcePath,
            ],
        ),
        ("unsafe", vec![LookupStatus::UnsafeAssetPath; 4]),
        ("long", vec![LookupStatus::LookupPathTooLong]),
        (
            "collision",
            vec![
                LookupStatus::ArchiveCollision,
                LookupStatus::OneArchiveCandidate,
            ],
        ),
        ("absent", vec![]),
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), case);
        with_sources(temp.path(), |catalogue, assets| {
            let report = catalogue
                .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
                .unwrap();
            assert_eq!(
                report
                    .requests
                    .iter()
                    .map(|r| r.lookup_status.unwrap())
                    .collect::<Vec<_>>(),
                statuses
            );
            assert!(!report.part_requests_admitted);
            if case == "empty" {
                let last = &report.render.manifest.paths[report.requests[2].manifest_path_index];
                assert!(last.raw.is_empty());
                assert_eq!(last.field_byte_offset, 28);
            }
            if case == "collision" {
                assert_eq!(report.requests[0].candidates.len(), 2);
            }
        });
    }
}
#[test]
fn duplicated_or_unverified_source_selection_retains_frames_without_directory_lookup() {
    for (case, selection, count) in [
        ("duplicate", CreaturePartSelection::AmbiguousSource, 3),
        (
            "template",
            CreaturePartSelection::ModelTemplateSelectionUnsupported,
            2,
        ),
        (
            "missing-config",
            CreaturePartSelection::ActorConfigurationUnavailable,
            2,
        ),
        (
            "duplicate-config",
            CreaturePartSelection::ActorConfigurationUnavailable,
            2,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), case);
        with_sources(temp.path(), |catalogue, assets| {
            let report = catalogue
                .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
                .unwrap();
            assert_eq!(report.requests.len(), count);
            assert_eq!(report.counts.lookup_attempts, 0);
            assert!(!report.part_requests_admitted);
            for request in report.requests {
                assert_eq!(request.selection, selection);
                assert!(request.lookup_status.is_none() && request.candidates.is_empty());
            }
        });
    }
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "missing-model");
    with_sources(temp.path(), |catalogue, assets| {
        let report = catalogue
            .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
            .unwrap();
        assert!(
            report
                .requests
                .iter()
                .all(|r| r.lookup_status == Some(LookupStatus::OneArchiveCandidate))
        );
        assert!(!report.part_requests_admitted);
        assert!(!report.render.issues.is_empty());
    });
}
#[test]
fn compressed_winning_override_retains_actual_source_header_and_frame_offsets() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "override");
    with_sources(temp.path(), |catalogue, assets| {
        let report = catalogue
            .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
            .unwrap();
        assert_eq!(report.render.sources[0].source.plugin, "ActorPatch.esp");
        assert_eq!(report.render.sources[0].header.flags, plugin::COMPRESSED);
        assert_eq!(report.requests.len(), 3);
        let path = &report.render.manifest.paths[report.requests[1].manifest_path_index];
        assert_eq!(path.field_byte_offset, 12);
        assert_eq!(path.raw, b"eyessetblue.nif");
        assert_eq!(
            path.field_decoded_offset,
            catalogue.get(&key(0x100)).unwrap().fields[path.field_index].decoded_offset
        );
    });
}
#[test]
fn wrong_roots_and_directory_authority_cannot_be_inferred_from_source_model() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid");
    with_sources(temp.path(), |catalogue, assets| {
        for root in [key(0x101), key(0x102), key(0x999)] {
            assert!(
                catalogue
                    .creature_parts_manifest(&root, &directory(), assets, Default::default())
                    .is_err()
            );
        }
        for raw in [
            b"textures/creatures/dog".as_slice(),
            b"meshes",
            b"creatures/dog",
        ] {
            assert!(
                catalogue
                    .creature_parts_manifest(
                        &key(0x100),
                        &AssetPath::new(raw).unwrap(),
                        assets,
                        Default::default()
                    )
                    .is_err()
            );
        }
        assert!(AssetPath::new(b"meshes/../dog").is_err());
        let near = AssetPath::new(&[b"meshes/".as_slice(), &vec![b'x'; 4090]].concat()).unwrap();
        assert!(
            catalogue
                .creature_parts_manifest(&key(0x100), &near, assets, Default::default())
                .is_err()
        );
        let near = AssetPath::new(&[b"meshes/".as_slice(), &vec![b'x'; 4089]].concat()).unwrap();
        let report = catalogue
            .creature_parts_manifest(&key(0x100), &near, assets, Default::default())
            .unwrap();
        assert!(
            report
                .requests
                .iter()
                .all(|r| r.lookup_status == Some(LookupStatus::LookupPathTooLong))
        );
    });
}
#[test]
fn requests_and_aggregate_candidate_path_visit_and_projection_budgets_have_exact_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "valid");
    with_sources(temp.path(), |catalogue, assets| {
        let report = catalogue
            .creature_parts_manifest(&key(0x100), &directory(), assets, Default::default())
            .unwrap();
        let mut exact = CreaturePartsLimits {
            max_requests: report.counts.requests,
            max_lookup_path_bytes: report.counts.lookup_path_bytes,
            max_visits: report.counts.visits,
            max_projection_bytes: serde_json::to_vec(&report).unwrap().len(),
            ..Default::default()
        };
        exact.render.manifest.max_candidates = report.counts.aggregate_candidates;
        exact.render.manifest.max_candidate_bytes = report.counts.aggregate_candidate_bytes;
        assert!(
            catalogue
                .creature_parts_manifest(&key(0x100), &directory(), assets, exact)
                .is_ok()
        );
        for name in [
            "request",
            "lookup path byte",
            "visit",
            "projection byte",
            "candidate",
            "candidate byte",
        ] {
            let mut under = exact;
            match name {
                "request" => under.max_requests -= 1,
                "lookup path byte" => under.max_lookup_path_bytes -= 1,
                "visit" => under.max_visits -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                "candidate" => under.render.manifest.max_candidates -= 1,
                "candidate byte" => under.render.manifest.max_candidate_bytes -= 1,
                _ => unreachable!(),
            }
            let error = catalogue
                .creature_parts_manifest(&key(0x100), &directory(), assets, under)
                .unwrap_err();
            assert!(error.to_string().contains(name), "{name}: {error}");
        }
        exact.render.max_requests = 0;
        assert!(
            catalogue
                .creature_parts_manifest(&key(0x100), &directory(), assets, exact)
                .is_err()
        );
    });
}
#[test]
fn export_authored_selected_sources_for_independent_cli_reader_when_requested() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_CREATURE_EVIDENCE_DIR") else {
        return;
    };
    let destination = std::path::PathBuf::from(destination);
    fs::create_dir_all(&destination).unwrap();
    for case in [
        "valid",
        "empty",
        "unsafe",
        "long",
        "absent",
        "collision",
        "duplicate",
        "template",
        "missing-config",
        "duplicate-config",
        "missing-model",
        "compressed",
        "override",
    ] {
        let path = destination.join(case);
        assert!(!path.exists());
        fixture(&path, case);
    }
}
