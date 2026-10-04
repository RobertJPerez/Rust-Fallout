mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, Owner, Value},
    save::{Captured, Recovery, Repository, format},
};
use sha2::{Digest, Sha256};

fn fixture() -> (tempfile::TempDir, fallout_data::loaded_scripts::Catalogue) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    (dir, catalogue)
}
fn world(c: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut w = World::with_campaign(
        c,
        Limits::default(),
        CampaignId::from_bytes([0x34; 16]).unwrap(),
    )
    .unwrap();
    let reference = w.register_reference(Some(form(0x100))).unwrap();
    let h = w
        .create_instance(
            &definition(c),
            Owner::Placed { reference },
            Context::default(),
        )
        .unwrap();
    w.assign(
        h,
        &[(
            42,
            Value::Number {
                bits: 0x7ff8_1234_5678_9abc,
            },
        )],
    )
    .unwrap();
    w.advance_clocks(Clocks {
        tick: 7,
        game_nanoseconds: 11,
        menu_nanoseconds: 13,
        real_nanoseconds: 17,
    })
    .unwrap();
    w.enqueue(
        h,
        Trigger::Block {
            event_id: 0,
            begin_byte_offset: 0,
        },
        Context::default(),
    )
    .unwrap();
    w
}
// An authored schema-2 envelope follows the documented wire contract. It does
// not call the production encoder or silently drop an initialized inventory.
fn legacy_json(w: &World<'_>) -> serde_json::Value {
    let mut v = serde_json::to_value(w.snapshot()).unwrap();
    assert_eq!(v["inventory_banks"], serde_json::json!([]));
    assert_eq!(v["next_item"], 1);
    let map = v.as_object_mut().unwrap();
    map.remove("inventory_banks");
    map.remove("reference_states");
    map.remove("next_item");
    map.insert("schema_version".into(), 2.into());
    v
}
fn envelope(w: &World<'_>, body: &[u8]) -> Vec<u8> {
    let s = w.snapshot();
    let mut meta = Vec::new();
    meta.extend(1_u32.to_le_bytes());
    meta.extend(2_u32.to_le_bytes());
    meta.extend(9_u64.to_le_bytes());
    meta.extend(s.clocks.tick.to_le_bytes());
    for i in (0..64).step_by(2) {
        meta.push(u8::from_str_radix(&s.catalogue_sha256[i..i + 2], 16).unwrap());
    }
    meta.extend((body.len() as u64).to_le_bytes());
    meta.extend(s.campaign.bytes());
    meta.extend(s.state_revision.to_le_bytes());
    let mut out = b"FRSAVE01".to_vec();
    out.extend(1_u16.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out.extend(2_u32.to_le_bytes());
    for (tag, payload) in [(b"META", meta.as_slice()), (b"STAT", body)] {
        out.extend(tag);
        out.extend(1_u32.to_le_bytes());
        out.extend((payload.len() as u64).to_le_bytes());
        out.extend(Sha256::digest(payload));
        out.extend(payload);
    }
    out.extend(Sha256::digest(&out));
    out
}
fn bytes(w: &World<'_>) -> Vec<u8> {
    envelope(w, &serde_json::to_vec(&legacy_json(w)).unwrap())
}
fn seal(bytes: &mut [u8], metadata: bool) {
    if metadata {
        let h = Sha256::digest(&bytes[64..152]);
        bytes[32..64].copy_from_slice(&h);
    }
    let end = bytes.len() - 32;
    let h = Sha256::digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&h);
}
#[test]
fn explicit_import_preserves_all_existing_state_and_identifies_original_bytes() {
    let (_dir, c) = fixture();
    let w = world(&c);
    let old = bytes(&w);
    assert!(format::decode(&old, Limits::default()).is_err());
    let imported = format::migrate_v2(&old, Limits::default()).unwrap();
    assert_eq!(imported.source_state_schema, 2);
    assert_eq!(imported.source_metadata.generation, 9);
    assert_eq!(
        imported.source_metadata.container_sha256,
        format!("{:x}", Sha256::digest(&old))
    );
    assert_eq!(imported.snapshot, w.snapshot());
    let restored = World::restore(&c, imported.snapshot, Limits::default()).unwrap();
    assert_eq!(restored.snapshot(), w.snapshot());
    assert!(
        restored
            .inventory_count(w.snapshot().references[0].id, &form(0x100))
            .is_err()
    );
    let new = format::encode(&Captured::at_boundary(&restored), 1).unwrap();
    assert!(format::migrate_v2(&new, Limits::default()).is_err());
    assert_ne!(
        imported.source_metadata.snapshot_sha256,
        format::decode(&new, Limits::default())
            .unwrap()
            .metadata
            .snapshot_sha256
    );
}
#[test]
fn legacy_shape_rejects_unknown_duplicate_and_disagreeing_schema_fields() {
    let (_dir, c) = fixture();
    let w = world(&c);
    let mut v = legacy_json(&w);
    v["unknown"] = true.into();
    assert!(
        format::migrate_v2(
            &envelope(&w, &serde_json::to_vec(&v).unwrap()),
            Limits::default()
        )
        .is_err()
    );
    let mut v = legacy_json(&w);
    v["schema_version"] = 3.into();
    assert!(
        format::migrate_v2(
            &envelope(&w, &serde_json::to_vec(&v).unwrap()),
            Limits::default()
        )
        .is_err()
    );
    let body = serde_json::to_string(&legacy_json(&w)).unwrap();
    let duplicate = format!("{{\"schema_version\":2,{}", &body[1..]);
    assert!(format::migrate_v2(&envelope(&w, duplicate.as_bytes()), Limits::default()).is_err());
    let mut v = legacy_json(&w);
    v["campaign"] = serde_json::json!(vec![0_u8; 16]);
    assert!(
        format::migrate_v2(
            &envelope(&w, &serde_json::to_vec(&v).unwrap()),
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn rehashed_metadata_cannot_disagree_with_legacy_canonical_state() {
    let (_dir, c) = fixture();
    let w = world(&c);
    let old = bytes(&w);
    for position in [64, 68, 72, 80, 88, 120, 128, 144] {
        let mut bad = old.clone();
        if position == 72 {
            bad[72..80].fill(0);
        } else {
            bad[position] ^= 1;
        }
        seal(&mut bad, true);
        assert!(
            format::migrate_v2(&bad, Limits::default()).is_err(),
            "metadata {position}"
        );
    }
}
#[test]
fn legacy_extents_integrity_and_budgets_fail_before_restore() {
    let (_dir, c) = fixture();
    let w = world(&c);
    let old = bytes(&w);
    for length in [0, 7, 15, 63, 151, 199, 231, old.len() - 1] {
        assert!(format::migrate_v2(&old[..length], Limits::default()).is_err());
    }
    for position in [
        0,
        8,
        16,
        20,
        24,
        32,
        152,
        156,
        160,
        180,
        old.len() - 33,
        old.len() - 1,
    ] {
        let mut bad = old.clone();
        bad[position] ^= 1;
        assert!(format::migrate_v2(&bad, Limits::default()).is_err());
        if position < old.len() - 32 {
            seal(&mut bad, false);
            assert!(format::migrate_v2(&bad, Limits::default()).is_err());
        }
    }
    assert!(
        format::migrate_v2(
            &old,
            Limits {
                max_snapshot_bytes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        format::migrate_v2(
            &old,
            Limits {
                max_instances: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn imported_shape_still_requires_source_bound_and_identity_validation() {
    let (_dir, c) = fixture();
    let w = world(&c);
    let old = bytes(&w);
    let mut changed = format::migrate_v2(&old, Limits::default())
        .unwrap()
        .snapshot;
    changed.catalogue_sha256 = "00".repeat(32);
    assert!(World::restore(&c, changed, Limits::default()).is_err());
    let mut v = legacy_json(&w);
    v["next_reference"] = 1.into();
    let imported = format::migrate_v2(
        &envelope(&w, &serde_json::to_vec(&v).unwrap()),
        Limits::default(),
    )
    .unwrap();
    assert!(World::restore(&c, imported.snapshot, Limits::default()).is_err());
}
#[test]
fn imported_state_publishes_into_a_new_repository_and_keeps_legacy_file_intact() {
    let (dir, c) = fixture();
    let w = world(&c);
    let old = bytes(&w);
    let original = dir.path().join("old.frsv");
    std::fs::write(&original, &old).unwrap();
    let imported = format::migrate_v2(&old, Limits::default()).unwrap();
    let restored = World::restore(&c, imported.snapshot, Limits::default()).unwrap();
    let repo = Repository::create(
        &dir.path().join("new"),
        std::slice::from_ref(&original),
        restored.campaign(),
    )
    .unwrap();
    let receipt = repo.commit(&Captured::at_boundary(&restored)).unwrap();
    assert_eq!(receipt.metadata.generation, 1);
    assert_eq!(
        repo.load(&c, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        w.snapshot()
    );
    assert_eq!(std::fs::read(original).unwrap(), old);
}
