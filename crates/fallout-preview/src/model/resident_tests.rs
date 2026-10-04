//! Real residency leases feed the existing DDS adapter without archive access.
use super::*;
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{preparation::CellModelPlan, residency},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Sources(PathBuf);
impl Drop for Sources {
    fn drop(&mut self) {
        // This directory was created exclusively by this fixture. Residency
        // handles drop first, so cleanup never touches an installed game source.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sub(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u16).to_le_bytes(),
        payload,
    ]
    .concat()
}
fn record(kind: &[u8; 4], id: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        payload,
    ]
    .concat()
}
fn group(kind: i32, payload: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(payload.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}

fn write_archive(path: &Path, folder: &[u8], name: &[u8], payload: &[u8]) {
    // One authored, uncompressed BSA104 member. No production writer or second
    // importer supplies the expected source bytes for these tests.
    let table = 54 + folder.len();
    let offset = table + 16 + name.len() + 1;
    let mut bytes = vec![0; offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, folder.len() as u32 + 1),
        (28, name.len() as u32 + 1),
        (44, 1),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = folder.len() as u8 + 1;
    bytes[53..53 + folder.len()].copy_from_slice(folder);
    bytes[table..table + 8].copy_from_slice(&1u64.to_le_bytes());
    bytes[table + 8..table + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[table + 12..table + 16].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[table + 16..table + 16 + name.len()].copy_from_slice(name);
    bytes.extend(payload);
    fs::write(path, bytes).unwrap();
}

fn fixture() -> (
    Sources,
    residency::CellResidency,
    Arc<ResidentTextures>,
    Vec<u8>,
) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-fallout-resident-dds-{}-{unique}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let files = Sources(root.clone());
    let texture_path = b"textures/t.dds";
    let payload = [
        1u32.to_le_bytes().as_slice(),
        &(texture_path.len() as u32).to_le_bytes(),
        texture_path,
    ]
    .concat();
    let kind = b"BSShaderTextureSet";
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    nif.extend(0x14020007u32.to_le_bytes());
    nif.push(1);
    for value in [11u32, 1, 34] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend([0; 3]);
    nif.extend(1u16.to_le_bytes());
    nif.extend((kind.len() as u32).to_le_bytes());
    nif.extend(kind);
    nif.extend(0u16.to_le_bytes());
    nif.extend((payload.len() as u32).to_le_bytes());
    // Empty string/group tables, one texture-set payload and no footer roots.
    nif.extend([0; 12]);
    nif.extend(payload);
    nif.extend([0; 4]);
    let mut dds = vec![0; 136];
    dds[..4].copy_from_slice(b"DDS ");
    for (at, value) in [
        (4, 124u32),
        (8, 0xa1007),
        (12, 4),
        (16, 4),
        (20, 8),
        (28, 1),
        (76, 32),
        (80, 4),
        (108, 0x401008),
    ] {
        dds[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    dds[84..88].copy_from_slice(b"DXT1");
    dds[128..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    write_archive(&root.join("model.bsa"), b"meshes", b"m.nif", &nif);
    write_archive(&root.join("texture.bsa"), b"textures", b"t.dds", &dds);
    let mut esm = record(
        b"TES4",
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    esm.extend(record(b"STAT", 0x400, &sub(b"MODL", b"m.nif\0")));
    esm.extend(record(
        b"CELL",
        0x200,
        &[sub(b"EDID", b"ResidentDDS\0"), sub(b"DATA", &[1])].concat(),
    ));
    let reference = record(
        b"REFR",
        0x300,
        &[
            sub(b"NAME", &0x400u32.to_le_bytes()),
            sub(b"DATA", &[0; 24]),
        ]
        .concat(),
    );
    esm.extend(group(6, &group(9, &reference)));
    fs::write(root.join("FalloutNV.esm"), esm).unwrap();
    let mut mounts = MountIndex::default();
    for name in ["model.bsa", "texture.bsa"] {
        NvArchive::open(&root.join(name))
            .unwrap()
            .census(&mut mounts)
            .unwrap();
    }
    let mut store =
        RecordStore::open_nv_headers(&root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let key = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x200,
    };
    let plan = CellModelPlan::load(&mut store, &key, &mounts, Default::default()).unwrap();
    let mut owner = residency::CellResidency::new(&root, None, Default::default()).unwrap();
    let ticket = owner.request(plan).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while owner.poll().unwrap().stage != residency::Stage::Decoded {
        assert!(Instant::now() < deadline, "model lease did not complete");
        thread::sleep(Duration::from_millis(1));
    }
    let plan =
        residency::TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default())
            .unwrap();
    owner.request_textures(&ticket, plan).unwrap();
    while owner.poll().unwrap().texture_state != residency::TextureState::Decoded {
        assert!(Instant::now() < deadline, "texture lease did not complete");
        thread::sleep(Duration::from_millis(1));
    }
    let textures = owner.texture_sources(&ticket).unwrap();
    (files, owner, textures, dds)
}

#[test]
fn resident_dds_uses_exact_payload_and_sampler_sharing_without_an_archive_adapter() {
    let (_files, _owner, sources, dds) = fixture();
    let path = AssetPath::new(b"textures/t.dds").unwrap();
    let mut adapted = Textures::default();
    assert_eq!(adapted.load_resident(&sources, &path, 0).unwrap(), 0);
    assert_eq!(adapted.load_resident(&sources, &path, 0).unwrap(), 0);
    assert_eq!(adapted.load_resident(&sources, &path, 3).unwrap(), 1);
    assert_eq!(adapted.images.len(), 2);
    assert_eq!(adapted.images[0].data.as_deref(), Some(dds[128..].as_ref()));
    assert_eq!(
        adapted.evidence[0].sha256,
        format!("{:x}", Sha256::digest(&dds))
    );
    assert_eq!(adapted.evidence[0].width, 4);
    assert_eq!(adapted.evidence[0].format, "Bc1RgbaUnormSrgb");
    let ImageSampler::Descriptor(clamped) = &adapted.images[0].sampler else {
        panic!()
    };
    let ImageSampler::Descriptor(repeated) = &adapted.images[1].sampler else {
        panic!()
    };
    assert_eq!(clamped.address_mode_u, ImageAddressMode::ClampToEdge);
    assert_eq!(repeated.address_mode_u, ImageAddressMode::Repeat);
    let missing = AssetPath::new(b"textures/not-in-plan.dds").unwrap();
    // A prior cached image cannot substitute for a path missing in this lease.
    adapted.load_bytes(&missing, 0, &dds).unwrap();
    let error = adapted.load_resident(&sources, &missing, 0).unwrap_err();
    assert!(error.to_string().contains("absent from resident plan"));
    assert_eq!(adapted.images.len(), 3);
}

#[test]
fn cached_resident_sampler_is_refused_after_unload() {
    let (_files, mut owner, sources, _) = fixture();
    let path = AssetPath::new(b"textures/t.dds").unwrap();
    let mut adapted = Textures::default();
    adapted.load_resident(&sources, &path, 0).unwrap();
    owner.unload().unwrap();
    assert!(owner.snapshot().pinned_source_bytes > 0);
    let error = adapted.load_resident(&sources, &path, 0).unwrap_err();
    assert!(error.to_string().contains("stale"));
    assert_eq!(adapted.images.len(), 1);
}
