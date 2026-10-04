use super::*;
use crate::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::tests::{Fixture, Pause},
    store::RecordStore,
    vfs::MountIndex,
};
use std::{fs, sync::Arc, time::Instant};

struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn sub(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(tag: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}
fn group(kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn setup(count: usize) -> (Fixture, TextureSourcePlan) {
    let fixture = Fixture::new(&[1, 2, 3, 4], true);
    let mut sources = record(
        b"TES4",
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    sources.extend(record(b"WRLD", 0x100, &sub(b"DATA", &[0])));
    let mut mounts = MountIndex::default();
    let mut layers = Vec::new();
    for index in 0..count {
        let file = [b'a' + index as u8, b'.', b'd', b'd', b's'];
        let mut bsa = fs::read(&fixture.source_member.container).unwrap();
        bsa.splice(53..60, b"textures\\land\0".iter().copied());
        bsa[52] = 14;
        bsa[24..28].copy_from_slice(&14u32.to_le_bytes());
        bsa[79..83].copy_from_slice(&89u32.to_le_bytes());
        bsa[83..88].copy_from_slice(&file);
        let path = fixture.source.path().join(format!("source{index}.bsa"));
        fs::write(&path, bsa).unwrap();
        NvArchive::open(&path).unwrap().census(&mut mounts).unwrap();
        let mut texture_path = b"land/".to_vec();
        texture_path.extend(file);
        texture_path.push(0);
        sources.extend(record(
            b"LTEX",
            0x400 + index as u32,
            &sub(b"TNAM", &(0x500 + index as u32).to_le_bytes()),
        ));
        sources.extend(record(
            b"TXST",
            0x500 + index as u32,
            &sub(b"TX00", &texture_path),
        ));
        layers.extend(sub(
            b"BTXT",
            &[
                (0x400 + index as u32).to_le_bytes().as_slice(),
                &[index as u8, 0, 255, 255],
            ]
            .concat(),
        ));
    }
    let cell = record(
        b"CELL",
        0x200,
        &[
            sub(b"EDID", b"PreparedExterior\0"),
            sub(b"DATA", &[0]),
            sub(b"XCLC", &[0; 12]),
        ]
        .concat(),
    );
    let children = group(6, &group(9, &record(b"LAND", 0x300, &layers)));
    let mut world_group = group(1, &[cell, children].concat());
    world_group[8..12].copy_from_slice(&0x100u32.to_le_bytes());
    sources.extend(world_group);
    fs::write(fixture.source.path().join("FalloutNV.esm"), sources).unwrap();
    let mut store = RecordStore::open_nv_headers(
        fixture.source.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let root = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x200,
    };
    let plan = TextureSourcePlan::load(&mut store, &root, &mounts, Default::default()).unwrap();
    (fixture, plan)
}
fn drained(preparation: &TexturePreparation) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while preparation.usage().outstanding != 0 {
        assert!(
            Instant::now() < deadline,
            "old worker reservations did not drain"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(preparation.usage().decoded_bytes, 0);
}

#[test]
fn terrain_replacement_revokes_controlled_running_and_queued_members() {
    for after_extract in [false, true] {
        let (fixture, plan) = setup(2);
        let original = serde_json::to_vec(plan.terrain()).unwrap();
        let mut preparation = TexturePreparation::new(
            plan.clone(),
            fixture.source.path(),
            Some(fixture.cache.path()),
            resource_jobs::Limits {
                workers: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let pause = Pause::new(after_extract);
        preparation.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        assert!(!preparation.poll().unwrap());
        pause.reached();
        assert_eq!(preparation.usage().outstanding, 2);
        assert!(preparation.take_ready().unwrap().is_none());
        preparation.replace(plan.clone()).unwrap();
        pause.release();
        pause.completed();
        drained(&preparation);
        assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 0);
        assert_eq!(serde_json::to_vec(plan.terrain()).unwrap(), original);
        preparation.pause = None;
        let receipt = preparation
            .wait()
            .unwrap()
            .publish_for(plan.terrain())
            .unwrap();
        assert_eq!(receipt.generation, 2);
        assert_eq!(receipt.textures.len(), 2);
        drained(&preparation);
    }
}

#[test]
fn terrain_retry_revokes_completed_unconsumed_worker_payload_but_reuses_valid_cache() {
    let (fixture, plan) = setup(1);
    let mut preparation = TexturePreparation::new(
        plan.clone(),
        fixture.source.path(),
        Some(fixture.cache.path()),
        Default::default(),
    )
    .unwrap();
    let pause = Pause::new(true);
    preparation.pause = Some(pause.clone());
    let _release = Release(pause.clone());
    preparation.poll().unwrap();
    pause.reached();
    pause.release();
    pause.completed();
    assert_eq!(preparation.usage().outstanding, 1);
    assert!(preparation.take_ready().unwrap().is_none());
    preparation.retry().unwrap();
    drained(&preparation);
    preparation.pause = None;
    let receipt = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    assert_eq!(receipt.generation, 2);
    assert!(receipt.textures[0].cache.as_ref().unwrap().reused);
    drained(&preparation);
}
