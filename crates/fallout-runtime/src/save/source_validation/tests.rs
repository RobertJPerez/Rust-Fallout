use super::*;
use crate::{
    events::{Context, Trigger},
    identity::{CampaignId, Owner},
    save::{Captured, Recovery, Repository, test_source as common},
};
use fallout_data::loaded_scripts::Catalogue;
use std::{fs, path::PathBuf, sync::Weak};

const DEFINITIONS: usize = 32;
// Independently derived from authored source names and hexadecimal SHA-256
// version identities, rather than the capture's own accounting implementation.
const IDENTITY_BYTES: usize = DEFINITIONS * ("falloutnv.esm".len() + 64);

struct Fixture {
    path: PathBuf,
    catalogue: Arc<Catalogue>,
    _temporary: Option<tempfile::TempDir>,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let (path, temporary) =
            if let Some(root) = std::env::var_os("FALLOUT_SOURCE_BUDGET_EVIDENCE") {
                let path = PathBuf::from(root).join(name);
                fs::create_dir(&path).unwrap();
                (path, None)
            } else {
                let temporary = tempfile::tempdir().unwrap();
                (temporary.path().to_owned(), Some(temporary))
            };
        let body = common::unit(&[(42, 0)], &[]);
        let mut bytes = common::header(&[]);
        for ordinal in 0..DEFINITIONS {
            bytes.extend(common::record(b"SCPT", 0x300 + ordinal as u32, 0, &body));
        }
        fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
        let catalogue = Arc::new(common::load(&path, &["FalloutNV.esm"]));
        Self {
            path,
            catalogue,
            _temporary: temporary,
        }
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(
            self.path.join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
    fn seed(&self, byte_budget: usize) -> (World<'static>, Vec<Weak<DefinitionSchema>>) {
        let limits = Limits {
            max_instances: DEFINITIONS,
            max_locals: DEFINITIONS,
            max_event_blocks: DEFINITIONS,
            max_pending_events: 1,
            max_snapshot_bytes: byte_budget,
            ..Limits::default()
        };
        let mut world = World::with_campaign(
            Arc::clone(&self.catalogue),
            limits,
            CampaignId::from_bytes([0x79; 16]).unwrap(),
        )
        .unwrap();
        for (ordinal, (_, script)) in self.catalogue.iter().enumerate() {
            assert_eq!(script.handle().key.record.origin_plugin, "falloutnv.esm");
            assert_eq!(script.handle().version_sha256.len(), 64);
            let instance = world
                .create_instance(
                    script.handle(),
                    Owner::Fragment {
                        activation: ((ordinal + 1) as u64).try_into().unwrap(),
                    },
                    Context::default(),
                )
                .unwrap();
            if ordinal + 1 == DEFINITIONS {
                world
                    .enqueue(
                        instance,
                        Trigger::Block {
                            event_id: 0,
                            begin_byte_offset: 0,
                        },
                        Context::default(),
                    )
                    .unwrap();
            } else {
                world.remove_instance(instance).unwrap();
            }
        }
        assert_eq!(world.definitions.len(), DEFINITIONS);
        assert_eq!(world.snapshot().instances.len(), 1);
        assert_eq!(world.snapshot().pending_events.len(), 1);
        assert!(
            world.snapshot().encode(byte_budget).unwrap().len() < byte_budget,
            "this boundary must be retained source identities, not serialized snapshot admission"
        );
        let schemas = world.definitions.values().map(Arc::downgrade).collect();
        (world, schemas)
    }
}

#[test]
fn exact_retained_identity_bytes_admit_owned_context_and_restore_source_bound_state() {
    let fixture = Fixture::new("exact");
    let (world, schemas) = fixture.seed(IDENTITY_BYTES);
    let before: Vec<_> = schemas.iter().map(Weak::strong_count).collect();
    let capture = Captured::at_boundary(&world);
    assert!(capture.source_validation.exceeded.is_none());
    assert_eq!(capture.source_validation.definitions.len(), DEFINITIONS);
    for (schema, owners) in schemas.iter().zip(before) {
        assert_eq!(schema.strong_count(), owners + 1);
    }
    let expected = world.snapshot();
    fixture.write("expected.json", &expected);
    let repository =
        Repository::create(&fixture.path.join("native"), &[], world.campaign()).unwrap();
    drop(world);
    assert!(schemas.iter().all(|schema| schema.strong_count() == 1));
    let receipt = repository.commit(&capture).unwrap();
    assert_eq!(receipt.metadata.generation, 1);
    drop(capture);
    assert!(schemas.iter().all(|schema| schema.upgrade().is_none()));
    let fresh = common::load(&fixture.path, &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&fresh, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
    fixture.write(
        "budget.json",
        &serde_json::json!({"definitions": DEFINITIONS,
        "identity_bytes": IDENTITY_BYTES, "byte_budget": IDENTITY_BYTES,
        "owned_context_definitions": DEFINITIONS, "context_schema_owners_after_drop": 0,
        "generation": receipt.metadata.generation}),
    );
}

#[test]
fn one_over_retained_identity_budget_owns_no_partial_context_and_preserves_current() {
    let fixture = Fixture::new("one-over");
    let (valid, _) = fixture.seed(IDENTITY_BYTES);
    let expected = valid.snapshot();
    fixture.write("expected.json", &expected);
    let repository =
        Repository::create(&fixture.path.join("native"), &[], valid.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&valid)).unwrap();
    drop(valid);
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let (world, schemas) = fixture.seed(IDENTITY_BYTES - 1);
    assert_eq!(world.snapshot(), expected);
    let before: Vec<_> = schemas.iter().map(Weak::strong_count).collect();
    let capture = Captured::at_boundary(&world);
    assert_eq!(
        capture.source_validation.exceeded,
        Some("publication source identity bytes")
    );
    assert!(capture.source_validation.definitions.is_empty());
    for (schema, owners) in schemas.iter().zip(before) {
        assert_eq!(
            schema.strong_count(),
            owners,
            "refused context owns no schema"
        );
    }
    drop(world);
    assert!(schemas.iter().all(|schema| schema.upgrade().is_none()));
    let mut stages = Vec::new();
    assert!(matches!(
        repository.commit_observing(&capture, |stage| stages.push(stage)),
        Err(crate::save::Error::State(Error::Capacity(
            "publication source identity bytes"
        )))
    ));
    assert!(stages.is_empty());
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert!(!repository.path().join("previous.frsv").exists());
    assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 3);
    let fresh = common::load(&fixture.path, &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&fresh, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
    fixture.write(
        "budget.json",
        &serde_json::json!({"definitions": DEFINITIONS,
        "identity_bytes": IDENTITY_BYTES, "byte_budget": IDENTITY_BYTES-1,
        "owned_context_definitions": 0, "context_schema_owners_after_drop": 0,
        "publication_stages": stages, "current_bytes_preserved": true}),
    );
}
